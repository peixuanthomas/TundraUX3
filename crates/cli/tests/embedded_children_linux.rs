#![cfg(target_os = "linux")]

use std::fs::File;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::path::PathBuf;
use std::time::{Duration, Instant};
use terminal_runtime::pty::{PtyConfig, PtyProcess};
use watchdog::{AppCriticality, AppDescriptor, AppId, WatchdogConfig, WatchdogRuntime};

const DAEMON_MODE: &str = "TUNDRA_TEST_EMBEDDED_DAEMON";
const DAEMON_PID_FILE: &str = "TUNDRA_TEST_EMBEDDED_PID_FILE";

/// Invoked as a separate executable by the real embedded REPL. After fork,
/// only async-signal-safe syscalls run; no Rust allocation or unwinding occurs.
#[test]
fn daemon_fixture() {
    let Ok(mode) = std::env::var(DAEMON_MODE) else {
        return;
    };
    let pid_file = File::create(std::env::var_os(DAEMON_PID_FILE).unwrap()).unwrap();
    let ignore_term = mode == "ignore-term";
    unsafe {
        let child = libc::fork();
        if child != 0 {
            libc::_exit(if child < 0 { 1 } else { 0 });
        }
        if libc::setsid() < 0 {
            libc::_exit(2);
        }
        let child = libc::fork();
        if child != 0 {
            libc::_exit(if child < 0 { 3 } else { 0 });
        }
        libc::signal(libc::SIGHUP, libc::SIG_IGN);
        libc::signal(
            libc::SIGTERM,
            if ignore_term {
                libc::SIG_IGN
            } else {
                libc::SIG_DFL
            },
        );
        // A failed assertion must not leave a live daemon indefinitely.
        libc::signal(libc::SIGALRM, libc::SIG_DFL);
        libc::alarm(30);
        let pid = libc::getpid().to_ne_bytes();
        if libc::write(pid_file.as_raw_fd(), pid.as_ptr().cast(), pid.len()) != pid.len() as isize {
            libc::_exit(4);
        }
        libc::close(pid_file.as_raw_fd());
        libc::close(0);
        libc::close(1);
        libc::close(2);
        loop {
            libc::pause();
        }
    }
}

struct Session {
    pty: Option<PtyProcess>,
    runtime: Option<WatchdogRuntime>,
    root: PathBuf,
    daemons: Vec<(libc::pid_t, OwnedFd)>,
}

impl Session {
    fn new() -> Self {
        // Test-only: after force_terminate kills the CLI, reap its dead orphans
        // here instead of relying on WSL's PID 1. Production Shell is unchanged.
        assert_eq!(
            unsafe { libc::prctl(libc::PR_SET_CHILD_SUBREAPER, 1, 0, 0, 0) },
            0
        );
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "tundra-embedded-children-{}-{nonce}",
            std::process::id()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let (runtime, process) = WatchdogRuntime::start_isolated(WatchdogConfig::new(
            root.join("reports"),
            root.join("fallback"),
            root.join("watchdog"),
            "embedded-child-test",
            "test",
        ))
        .unwrap();
        let app = process
            .register_app(AppDescriptor::new(
                AppId::from_static("embedded-child-test"),
                "Embedded child test",
                "test",
                AppCriticality::Optional,
            ))
            .unwrap();
        // env execs the real binary, preserving its PID. Isolate its storage
        // without changing the test runner's shared process environment.
        let mut args = ["CONFIG", "DATA", "CACHE", "STATE"]
            .into_iter()
            .map(|kind| format!("XDG_{kind}_HOME={}", root.join(kind).display()).into())
            .collect::<Vec<std::ffi::OsString>>();
        args.extend([
            env!("CARGO_BIN_EXE_tundra-cli").into(),
            "repl".into(),
            "--embedded".into(),
        ]);
        let pty = PtyProcess::spawn(
            PtyConfig {
                program: "/usr/bin/env".into(),
                args,
                env: Vec::new(),
                cwd: Some(root.clone()),
                columns: 160,
                rows: 30,
                scrollback_lines: 200,
            },
            &app.task_group("reader"),
        )
        .unwrap();
        let session = Self {
            pty: Some(pty),
            runtime: Some(runtime),
            root,
            daemons: Vec::new(),
        };
        let mut confirmed_root = false;
        session.until(|| {
            let text = session.text();
            if !confirmed_root && text.contains("Press y to continue as root") {
                session.pty().write(b"y").unwrap();
                confirmed_root = true;
            }
            text.contains(">>")
        });
        session
    }

    fn pty(&self) -> &PtyProcess {
        self.pty.as_ref().unwrap()
    }

    fn pid(&self) -> libc::pid_t {
        self.pty().process_id().unwrap().unwrap() as libc::pid_t
    }

    fn text(&self) -> String {
        self.pty()
            .snapshot()
            .cells
            .into_iter()
            .map(|row| {
                row.into_iter()
                    .map(|cell| {
                        if cell.text.is_empty() {
                            " ".into()
                        } else {
                            cell.text
                        }
                    })
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn until(&self, mut ready: impl FnMut() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while !ready() {
            assert!(Instant::now() < deadline, "PTY timed out:\n{}", self.text());
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn daemon(&mut self, ignore_term: bool) -> libc::pid_t {
        let pid_file = self.root.join(format!("daemon-{}.pid", self.daemons.len()));
        let executable = std::env::current_exe().unwrap();
        let command = format!(
            "{DAEMON_MODE}={} {DAEMON_PID_FILE}={} {} --exact daemon_fixture --nocapture\r",
            if ignore_term { "ignore-term" } else { "term" },
            shlex::try_quote(pid_file.to_str().unwrap()).unwrap(),
            shlex::try_quote(executable.to_str().unwrap()).unwrap(),
        );
        self.pty().write(command.as_bytes()).unwrap();
        let mut daemon = None;
        self.until(|| {
            daemon = std::fs::read(&pid_file)
                .ok()
                .and_then(|bytes| bytes.try_into().ok())
                .map(libc::pid_t::from_ne_bytes);
            daemon.is_some()
        });
        let daemon = daemon.unwrap();
        self.daemons.push((daemon, pidfd(daemon)));
        self.until(|| parent_pid(daemon) == Some(self.pid()));
        let session = unsafe { libc::getsid(daemon) };
        assert_ne!(session, self.pid(), "fixture must escape the PTY session");
        assert_ne!(session, daemon, "fixture must perform the second fork");
        // The intermediate session leader can also be an adopted zombie.
        // Retain it for test cleanup if force-close prevents the CLI reaping it.
        if parent_pid(session) == Some(self.pid()) {
            self.daemons.push((session, pidfd(session)));
        }
        daemon
    }

    fn wait_for_exit(&self, expected: Option<u32>) {
        self.until(|| {
            if let Some(status) = self.pty().try_wait().unwrap() {
                if let Some(code) = expected {
                    assert_eq!(status.code, code, "{}", self.text());
                }
                true
            } else {
                false
            }
        });
    }

    fn wait_for_daemon_reaped(&mut self, pid: libc::pid_t) {
        self.until(|| {
            // Only the test's own adopted child can be reaped by this call.
            unsafe { libc::waitpid(pid, std::ptr::null_mut(), libc::WNOHANG) };
            !std::path::Path::new(&format!("/proc/{pid}")).exists()
        });
        self.daemons.retain(|(current, _)| *current != pid);
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        if let Some(pty) = self.pty.take() {
            let _ = pty.force_terminate();
            for (_, daemon) in &self.daemons {
                signal(daemon, libc::SIGKILL);
            }
            let deadline = Instant::now() + Duration::from_secs(2);
            while pty.try_wait().ok().flatten().is_none() && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(10));
            }
            drop(pty);
        }
        for (pid, _) in &self.daemons {
            let deadline = Instant::now() + Duration::from_secs(2);
            while Instant::now() < deadline {
                if unsafe { libc::waitpid(*pid, std::ptr::null_mut(), libc::WNOHANG) } == *pid
                    || !std::path::Path::new(&format!("/proc/{pid}")).exists()
                {
                    break;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
        }
        let _ = self.runtime.take().unwrap().shutdown();
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn parent_pid(pid: libc::pid_t) -> Option<libc::pid_t> {
    std::fs::read_to_string(format!("/proc/{pid}/status"))
        .ok()?
        .lines()
        .find_map(|line| line.strip_prefix("PPid:"))?
        .trim()
        .parse()
        .ok()
}

fn pidfd(pid: libc::pid_t) -> OwnedFd {
    let raw = unsafe { libc::syscall(libc::SYS_pidfd_open, pid, 0) };
    assert!(raw >= 0, "pidfd_open: {}", std::io::Error::last_os_error());
    unsafe { OwnedFd::from_raw_fd(raw as i32) }
}

fn signal(fd: &OwnedFd, number: libc::c_int) {
    unsafe {
        libc::syscall(
            libc::SYS_pidfd_send_signal,
            fd.as_raw_fd(),
            number,
            std::ptr::null::<libc::siginfo_t>(),
            0,
        );
    }
}

#[test]
fn forced_close_reaches_daemonized_children_and_leaves_another_terminal_running() {
    let mut closing = Session::new();
    let closing_daemon = closing.daemon(true);
    let mut other = Session::new();
    let other_daemon = other.daemon(false);
    closing.pty().force_terminate().unwrap();
    closing.wait_for_exit(None);
    closing.wait_for_daemon_reaped(closing_daemon);
    assert!(other.pty().try_wait().unwrap().is_none());
    assert_eq!(parent_pid(other_daemon), Some(other.pid()));
    other.pty().write(b"exit\r").unwrap();
    other.wait_for_exit(Some(0));
    other.wait_for_daemon_reaped(other_daemon);
}

#[test]
fn exit_and_reset_reap_daemons_before_preserving_the_requested_exit_code() {
    for (command, code) in [("exit\r", 0), ("/new\r", 75)] {
        let mut session = Session::new();
        let daemon = session.daemon(false);
        session.pty().write(command.as_bytes()).unwrap();
        if code == 75 {
            session.until(|| session.text().contains("Type RESET"));
            session.pty().write(b"RESET\r").unwrap();
        }
        session.wait_for_exit(Some(code));
        assert!(
            !std::path::Path::new(&format!("/proc/{daemon}")).exists(),
            "CLI must reap the daemon before its own exit"
        );
        session.wait_for_daemon_reaped(daemon);
    }
}

#[test]
fn exit_keeps_root_alive_for_ignored_term_until_shell_terminates_the_tree() {
    let mut session = Session::new();
    let daemon = session.daemon(true);
    session.pty().write(b"exit\r").unwrap();
    session.until(|| {
        session
            .text()
            .contains("Waiting for Command Line child processes")
    });
    let root = pidfd(session.pid());
    for number in [libc::SIGINT, libc::SIGHUP, libc::SIGTERM] {
        signal(&root, number);
    }
    std::thread::sleep(Duration::from_millis(150));
    assert!(session.pty().try_wait().unwrap().is_none());
    assert_eq!(parent_pid(daemon), Some(session.pid()));
    session.pty().force_terminate().unwrap();
    session.wait_for_exit(None);
    session.wait_for_daemon_reaped(daemon);
}
