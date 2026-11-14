use super::*;
use std::process::{Command, Stdio};
use std::time::Duration;
use workspace_protocol::{ProcessGeneration, RunId, SessionId};

const FIXTURE: &str = "TUNDRA_DELEGATED_RESOURCE_TEST";

#[test]
fn delegated_resource_ack_allows_only_the_embedded_workers_own_process_tree() {
    if std::env::var_os(FIXTURE).is_some() {
        install_from_address(&std::env::var(RESOURCE_ENV).unwrap()).unwrap();
        assert!(watchdog::process_session_registration_enabled());
        let mut command = Command::new("/bin/true");
        let (mut child, registration) =
            watchdog::spawn_registered_process_group(&mut command).unwrap();
        assert!(registration.is_some());
        assert!(child.wait().unwrap().success());
        return;
    }

    let root = std::env::temp_dir().canonicalize().unwrap();
    let private = root.join(format!(".tux-delegate-test-{}", std::process::id()));
    platform::shell_control::create_private_directory(&private).unwrap();
    let result = std::panic::catch_unwind(|| exercise_registration(&private));
    std::fs::remove_dir_all(private).unwrap();
    result.unwrap();
}

fn exercise_registration(private: &Path) {
    let owner = WorkerId {
        run: RunId(7),
        role: WorkerRole::App(SessionId(1)),
        generation: ProcessGeneration(1),
    };
    let mut guardian = DelegatedResources::bind(private).unwrap();
    let mut supervisor = Supervisor::new(owner.run);
    let executable = std::env::current_exe().unwrap();
    let test_name = "process_host::delegated::tests::delegated_resource_ack_allows_only_the_embedded_workers_own_process_tree";
    let mut command = Command::new("/bin/sh");
    let pid = supervisor
        .spawn_worker(
            owner,
            command
                .args(["-c", "\"$1\" --exact \"$2\"", "fixture"])
                .arg(&executable)
                .arg(test_name)
                .env(FIXTURE, "1")
                .envs([environment(owner, std::process::id(), private).unwrap()])
                .stdin(Stdio::null())
                .stdout(Stdio::null()),
        )
        .unwrap();

    let endpoint = private.join(ENDPOINT);
    let unrelated =
        LocalConnection::connect(endpoint.to_str().unwrap(), std::process::id()).unwrap();
    let mut attempt = Framed::new(unrelated, MAX_RESOURCE_FRAME_BYTES);
    attempt
        .queue(&ResourceToGuardian::Hello(Hello::new(owner)))
        .unwrap();
    let mut denied = false;
    let mut exited = false;
    let start = Instant::now();
    while (!denied || !exited) && start.elapsed() < Duration::from_secs(4) {
        if !denied {
            match attempt.poll::<GuardianToResource>() {
                Ok(messages) => assert!(messages.is_empty()),
                Err(error) => {
                    assert_eq!(error.kind(), io::ErrorKind::BrokenPipe);
                    denied = true;
                }
            }
        }
        guardian.poll(&mut supervisor);
        for event in supervisor.tick() {
            if let watchdog::SupervisionEvent::Exited { worker, status } = event {
                assert_eq!(worker, owner);
                assert!(status.success());
                exited = true;
            }
        }
        std::thread::sleep(POLL_INTERVAL);
    }
    assert!(denied);
    assert!(exited, "managed worker {pid} did not exit");
}
