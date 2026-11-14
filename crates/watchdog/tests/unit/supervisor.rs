use super::*;
use std::io::{BufRead, BufReader, Write};
use std::net::Shutdown;
use std::os::fd::{FromRawFd, OwnedFd};
use std::os::unix::net::UnixStream;
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::process::{self, Stdio};
use std::sync::Mutex;
use std::thread;

const RUN: RunId = RunId(78);
const TICK: Duration = Duration::from_millis(10);
const CHILD_TEST: &str = "supervisor::tests::worker_fixture";

fn fixture(condition: &str, stream: UnixStream) -> Command {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args(["--exact", CHILD_TEST, "--nocapture"])
        .env("TUX_WATCHDOG_FIXTURE", condition)
        .stdin(OwnedFd::from(stream))
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    command
}

fn start_worker(
    supervisor: &mut Supervisor,
    worker: WorkerId,
    condition: &str,
) -> (u32, UnixStream) {
    let (mut parent, child) = UnixStream::pair().unwrap();
    parent
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    let pid = supervisor
        .spawn_worker(worker, &mut fixture(condition, child))
        .unwrap();
    let mut ready = [0];
    std::io::Read::read_exact(&mut parent, &mut ready).unwrap();
    assert_eq!(ready, [b'!']);
    (pid, parent)
}

fn id(role: WorkerRole) -> WorkerId {
    WorkerId {
        run: RUN,
        role,
        generation: ProcessGeneration(1),
    }
}

fn app(session: u64) -> WorkerId {
    id(WorkerRole::App(workspace_protocol::SessionId(session)))
}

fn until(
    timeout: Duration,
    supervisor: &mut Supervisor,
    mut condition: impl FnMut(&mut Supervisor, SupervisionEvent) -> bool,
) {
    let deadline = Instant::now() + timeout;
    loop {
        assert!(Instant::now() < deadline, "watchdog event timed out");
        for event in supervisor.tick() {
            if let SupervisionEvent::ProcessError { error, .. } = &event {
                panic!("process supervision failed: {error}");
            }
            if condition(supervisor, event) {
                return;
            }
        }
        thread::sleep(TICK);
    }
}

#[test]
fn shell_busy_loop_is_force_stopped_and_reaped_within_five_seconds() {
    let mut supervisor = Supervisor::new(RUN);
    let shell = id(WorkerRole::Shell);
    let (pid, _started) = start_worker(&mut supervisor, shell, "busy");
    let start = Instant::now();
    supervisor.progress(shell, 1, None);
    let mut requested = false;
    until(
        Duration::from_secs(5),
        &mut supervisor,
        |_, event| match event {
            SupervisionEvent::ForceStopping { worker } if worker == shell => {
                assert!(start.elapsed() >= Duration::from_secs(4));
                requested = true;
                false
            }
            SupervisionEvent::Exited { worker, status } if worker == shell => {
                assert_eq!(status.signal(), Some(libc::SIGKILL));
                true
            }
            _ => false,
        },
    );
    assert!(start.elapsed() < Duration::from_secs(5));
    assert!(requested);
    assert!(!supervisor.has_workers());
    assert_eq!(
        unsafe { libc::waitpid(pid as i32, std::ptr::null_mut(), libc::WNOHANG) },
        -1
    );
    assert_eq!(
        io::Error::last_os_error().raw_os_error(),
        Some(libc::ECHILD)
    );
}

#[test]
fn a_job_control_stop_is_not_a_child_exit() {
    let mut supervisor = Supervisor::new(RUN);
    let shell = id(WorkerRole::Shell);
    let (pid, _) = start_worker(&mut supervisor, shell, "ignore-close");
    supervisor.progress(shell, 1, None);
    assert_eq!(unsafe { libc::kill(pid as i32, libc::SIGSTOP) }, 0);
    let start = Instant::now();
    while start.elapsed() < Duration::from_millis(500) {
        assert!(supervisor.tick().is_empty());
        assert_eq!(supervisor.state(shell), Some(ProcessState::Running));
        thread::sleep(TICK);
    }
    until(Duration::from_millis(4500), &mut supervisor, |_, event| {
        matches!(event, SupervisionEvent::Exited { worker, status }
            if worker == shell && status.signal() == Some(libc::SIGKILL))
    });
}

#[test]
fn application_lockup_is_reported_and_only_explicit_kill_stops_it() {
    let mut supervisor = Supervisor::new(RUN);
    let app = app(1);
    let (pid, mut stream) = start_worker(&mut supervisor, app, "locked");
    supervisor.progress(app, 1, None);
    until(
        Duration::from_secs(5),
        &mut supervisor,
        |_, event| matches!(event, SupervisionEvent::NoResponse { worker } if worker == app),
    );
    assert_eq!(supervisor.state(app), Some(ProcessState::Unresponsive));
    assert_eq!(unsafe { libc::kill(pid as i32, 0) }, 0);
    let start = Instant::now();
    supervisor.request_force_kill(app).unwrap();
    assert!(start.elapsed() < Duration::from_millis(100));
    assert_eq!(supervisor.state(app), Some(ProcessState::ForceStopping));
    stream
        .set_read_timeout(Some(Duration::from_secs(1)))
        .unwrap();
    let mut bytes = [0];
    assert_eq!(std::io::Read::read(&mut stream, &mut bytes).unwrap(), 0);
    until(Duration::from_secs(1), &mut supervisor, |_, event| {
        matches!(event, SupervisionEvent::Exited { worker, status }
            if worker == app && status.signal() == Some(libc::SIGKILL))
    });
    let later = WorkerId {
        generation: ProcessGeneration(2),
        ..app
    };
    let mut command = Command::new("/bin/sleep");
    command.arg("30");
    supervisor.spawn_worker(later, &mut command).unwrap();
    assert!(supervisor.request_force_kill(app).is_err());
    assert_eq!(supervisor.state(later), Some(ProcessState::Starting));
}

#[test]
fn a_close_timeout_only_notifies_until_the_shell_exits() {
    let mut supervisor = Supervisor::new(RUN);
    let shell = id(WorkerRole::Shell);
    let (shell_pid, mut shell_connection) = start_worker(&mut supervisor, shell, "close-on-byte");
    let app = app(1);
    let (pid, mut app_connection) = start_worker(&mut supervisor, app, "ignore-close");
    supervisor.progress(shell, 1, None);
    supervisor.progress(app, 1, None);
    assert!(supervisor.request_close(app));
    until(
        Duration::from_secs(3),
        &mut supervisor,
        |supervisor, event| match event {
            SupervisionEvent::RequestClose { worker } if worker == app => {
                app_connection.write_all(b"C").unwrap();
                false
            }
            SupervisionEvent::Probe { worker, nonce } => {
                supervisor.progress(worker, 2, Some(nonce));
                false
            }
            SupervisionEvent::CloseTimedOut { worker } if worker == app => true,
            _ => false,
        },
    );
    assert_eq!(unsafe { libc::kill(pid as i32, 0) }, 0);
    assert_eq!(unsafe { libc::kill(shell_pid as i32, 0) }, 0);
    supervisor.progress(app, 3, None);
    shell_connection.write_all(b"C").unwrap();
    let start = Instant::now();
    until(
        Duration::from_secs(3),
        &mut supervisor,
        |_, event| match event {
            SupervisionEvent::Exited { worker, status } if worker == app => {
                assert_eq!(status.signal(), Some(libc::SIGKILL));
                true
            }
            _ => false,
        },
    );
    assert!(start.elapsed() < Duration::from_millis(800));
}

#[test]
fn shell_exit_gives_a_healthy_worker_at_most_two_seconds_to_close() {
    let mut supervisor = Supervisor::new(RUN);
    let shell = id(WorkerRole::Shell);
    let (_, mut shell_connection) = start_worker(&mut supervisor, shell, "close-on-byte");
    let app = app(1);
    let (_, mut app_connection) = start_worker(&mut supervisor, app, "ignore-close");
    supervisor.progress(shell, 1, None);
    supervisor.progress(app, 1, None);
    shell_connection.write_all(b"C").unwrap();
    let start = Instant::now();
    let mut requested = false;
    until(
        Duration::from_secs(3),
        &mut supervisor,
        |_, event| match event {
            SupervisionEvent::RequestClose { worker } if worker == app => {
                app_connection.write_all(b"C").unwrap();
                requested = true;
                false
            }
            SupervisionEvent::Exited { worker, status } if worker == app => {
                assert_eq!(status.signal(), Some(libc::SIGKILL));
                true
            }
            _ => false,
        },
    );
    assert!(requested);
    assert!(start.elapsed() >= CLOSE_TIMEOUT);
    assert!(start.elapsed() < Duration::from_millis(2400));
}

#[test]
fn force_kill_cleans_registered_detached_groups_without_killing_other_workers() {
    let mut supervisor = Supervisor::new(RUN);
    let target = app(1);
    let (target_pid, connection) = start_worker(&mut supervisor, target, "tree-owner");
    let idle = app(2);
    let idle_pid = supervisor
        .spawn_worker(idle, &mut Command::new("/bin/sleep").arg("30"))
        .unwrap();
    let mut tree = BufReader::new(connection);
    let mut leader = String::new();
    tree.read_line(&mut leader).unwrap();
    let leader: u32 = leader.trim().parse().unwrap();
    let resource = super::process::unix_session(leader).unwrap();
    assert!(supervisor.owns_process(target, leader).unwrap());
    assert!(!supervisor.owns_process(target, idle_pid).unwrap());
    assert!(supervisor.register_resource(idle, resource).is_err());
    assert!(
        supervisor
            .register_child_resource(target, idle_pid, resource)
            .is_err()
    );
    supervisor
        .register_child_resource(target, target_pid, resource)
        .unwrap();
    assert!(
        supervisor
            .unregister_child_resource(target, idle_pid, resource)
            .is_err()
    );
    supervisor.unregister_resource(target, resource).unwrap();
    tree.get_mut().write_all(b"R").unwrap();
    let mut descendant = String::new();
    tree.read_line(&mut descendant).unwrap();
    let descendant: i32 = descendant.trim().parse().unwrap();
    let mut detached = String::new();
    tree.read_line(&mut detached).unwrap();
    let detached: i32 = detached.trim().parse().unwrap();
    supervisor.request_force_kill(target).unwrap();
    until(Duration::from_secs(2), &mut supervisor, |_, event| {
        matches!(event, SupervisionEvent::Exited { worker, status }
            if worker == target && status.signal() == Some(libc::SIGKILL))
    });
    for pid in [leader as i32, descendant, detached] {
        assert!(
            super::process::test_process_exited(pid),
            "{pid} remained after tree confirmation"
        );
    }
    assert_eq!(unsafe { libc::kill(idle_pid as i32, 0) }, 0);
}

#[test]
fn worker_fixture() {
    let Ok(condition) = std::env::var("TUX_WATCHDOG_FIXTURE") else {
        return;
    };
    let fd = unsafe { libc::dup(libc::STDIN_FILENO) };
    assert!(fd >= 0);
    let mut stream = unsafe { UnixStream::from_raw_fd(fd) };
    stream
        .set_read_timeout(Some(Duration::from_secs(12)))
        .unwrap();
    let mut byte = [0];
    if condition != "tree-leader" {
        stream.write_all(b"!").unwrap();
    }
    match condition.as_str() {
        "busy" => loop {
            std::hint::spin_loop();
        },
        "locked" => {
            let mutex = Mutex::new(());
            let _lock = mutex.lock().unwrap();
            let _deadlock = mutex.lock().unwrap();
        }
        "ignore-close" => {
            let _ = std::io::Read::read(&mut stream, &mut byte);
            loop {
                thread::park();
            }
        }
        "close-on-byte" => {
            std::io::Read::read_exact(&mut stream, &mut byte).unwrap();
            stream.shutdown(Shutdown::Both).unwrap();
        }
        "tree-owner" => {
            let mut command = fixture("tree-leader", stream);
            unsafe {
                command.pre_exec(|| {
                    if libc::setsid() < 0 {
                        Err(io::Error::last_os_error())
                    } else {
                        Ok(())
                    }
                });
            }
            let mut child = command.spawn().unwrap();
            let _ = child.wait();
        }
        "tree-leader" => {
            stream
                .write_all(format!("{}\n", process::id()).as_bytes())
                .unwrap();
            std::io::Read::read_exact(&mut stream, &mut byte).unwrap();
            let mut child = Command::new("/bin/sleep")
                .arg("30")
                .process_group(0)
                .spawn()
                .unwrap();
            stream
                .write_all(format!("{}\n", child.id()).as_bytes())
                .unwrap();
            let mut new_session = Command::new("/bin/sleep");
            new_session.arg("30");
            unsafe {
                new_session.pre_exec(|| {
                    if libc::setsid() < 0 {
                        Err(io::Error::last_os_error())
                    } else {
                        Ok(())
                    }
                });
            }
            let mut new_session = new_session.spawn().unwrap();
            stream
                .write_all(format!("{}\n", new_session.id()).as_bytes())
                .unwrap();
            let _ = child.wait();
            let _ = new_session.wait();
        }
        "registered-spawn" => {
            use std::sync::{Mutex, mpsc};
            let (report, receive) = mpsc::channel::<bool>();
            let receive = Mutex::new(receive);
            let registration_stream = Mutex::new(stream.try_clone().unwrap());
            set_process_resource_registrar(
                move |resource| {
                    let mut stream = registration_stream.lock().unwrap();
                    stream
                        .write_all(format!("{}\n", serde_json::to_string(&resource)?).as_bytes())?;
                    receive
                        .lock()
                        .unwrap()
                        .recv()
                        .map_err(io::Error::other)?
                        .then_some(())
                        .ok_or_else(|| io::ErrorKind::PermissionDenied.into())
                },
                |_| {},
            )
            .unwrap();
            let wait = thread::spawn(move || {
                let mut decision = [0];
                std::io::Read::read_exact(&mut stream, &mut decision).unwrap();
                report.send(decision == [b'1']).unwrap();
            });
            let mut command = Command::new("/usr/bin/true");
            let result = watchdog_spawn_fixture(&mut command);
            wait.join().unwrap();
            std::process::exit(if result { 0 } else { 2 });
        }
        _ => unreachable!(),
    }
}

fn watchdog_spawn_fixture(command: &mut Command) -> bool {
    crate::spawn_registered_process_group(command)
        .is_ok_and(|(mut child, _registration)| child.wait().is_ok_and(|status| status.success()))
}

#[test]
fn supported_commands_wait_for_the_guardian_registration_before_exec() {
    let mut supervisor = Supervisor::new(RUN);
    let target = app(1);
    let (_, connection) = start_worker(&mut supervisor, target, "registered-spawn");
    let mut connection = BufReader::new(connection);
    let mut message = String::new();
    connection.read_line(&mut message).unwrap();
    let resource = serde_json::from_str(&message).unwrap();
    supervisor.register_resource(target, resource).unwrap();
    connection.get_mut().write_all(b"1").unwrap();
    until(Duration::from_secs(2), &mut supervisor, |_, event| {
        matches!(event, SupervisionEvent::Exited { worker, status }
            if worker == target && status.success())
    });
}
