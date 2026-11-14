use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::thread::{self, JoinHandle};
use std::time::{Duration, SystemTime};

use cli::{CliCommand, CliError, WatchdogCommand};
use platform::mock::MockPlatform;
use platform::shell_control::{Server, create_private_directory, watchdog_directory};
use platform::{AppPaths, UserDirs};
use serde_json::{Value, json};
use workspace_protocol::{
    PROTOCOL_VERSION, ProcessGeneration, ProcessState, RunId, SessionId, SessionInfo,
    WatchdogAction, WatchdogReply, WatchdogRequest, WatchdogResponse, WorkerId, WorkerRole,
};

const RUN: &str = "000000000000000000000000000000a1";

#[test]
fn kill_requires_an_explicit_run_and_one_managed_target() {
    for (args, session) in [
        (vec!["--session", "7", "--run", RUN], Some(SessionId(7))),
        (vec!["--run", RUN, "--shell"], None),
    ] {
        assert_eq!(
            cli::parse_args(["watchdog", "kill"].into_iter().chain(args)),
            Ok(CliCommand::Watchdog(WatchdogCommand::Kill {
                run: RunId(0xa1),
                session,
            }))
        );
    }
    for args in [
        &[][..],
        &["12345"],
        &["--pid", "12345"],
        &["--session", "7"],
        &["--run", "12345", "--shell"],
        &["--run", RUN],
        &["--run", RUN, "--run", RUN, "--shell"],
        &["--run", RUN, "--shell", "--shell"],
        &["--run", RUN, "--shell", "--session", "7"],
        &["--run", RUN, "--session", "+7"],
        &["--run", RUN, "--session", "18446744073709551616"],
    ] {
        assert_eq!(
            cli::parse_args(["watchdog", "kill"].into_iter().chain(args.iter().copied())),
            Err(CliError::InvalidWatchdogArgument),
            "{args:?}"
        );
    }
    assert_eq!(
        cli::parse_args(["watchdog", "list", "--session", "7"]),
        Err(CliError::InvalidWatchdogArgument)
    );
}

#[test]
fn watchdog_connects_without_ui_control_and_distinguishes_requested_and_confirmed_exit() {
    let mut fixture = Fixture::new();
    assert_eq!(
        fixture.success(&["watchdog", "list"]),
        json!({ "protocol": 1, "runs": [] })
    );
    assert!(!fixture.root.exists());
    let run: RunId = RUN.parse().unwrap();
    let app = WorkerId {
        run,
        role: WorkerRole::App(SessionId(7)),
        generation: ProcessGeneration(9),
    };
    let shell = WorkerId {
        run,
        role: WorkerRole::Shell,
        generation: ProcessGeneration(2),
    };
    let workers = vec![
        SessionInfo {
            worker: app,
            pid: 101,
            application: "launcher".into(),
            state: ProcessState::Unresponsive,
            cleanup_error: None,
        },
        SessionInfo {
            worker: shell,
            pid: 102,
            application: "shell".into(),
            state: ProcessState::Running,
            cleanup_error: None,
        },
    ];
    let requests = fixture.start(run, workers.clone(), move |worker| {
        if worker == app {
            WatchdogReply::KillRequested { worker }
        } else {
            WatchdogReply::Killed { worker, code: None }
        }
    });
    assert_eq!(
        fixture.success(&["watchdog", "list"]),
        json!({ "protocol": 1, "runs": [{ "run": RUN, "workers": workers }] })
    );
    assert_eq!(
        fixture.success(&["watchdog", "kill", "--run", RUN, "--session", "7"]),
        json!({ "protocol": 1, "status": "kill_requested", "worker": app })
    );
    assert_eq!(
        fixture.success(&["watchdog", "kill", "--run", RUN, "--shell"]),
        json!({ "protocol": 1, "status": "killed", "worker": shell, "code": null })
    );
    let (code, output, error) =
        fixture.run(&["watchdog", "kill", "--run", RUN, "--session", "101"]);
    assert_eq!(code, 1);
    assert!(output.is_empty());
    assert!(error.contains("no longer available"));
    let wrong = RunId(0xa2).to_string();
    let (code, output, error) = fixture.run(&["watchdog", "kill", "--run", &wrong, "--shell"]);
    assert_eq!(code, 1);
    assert!(output.is_empty());
    assert!(error.contains(&wrong));
    assert_eq!(
        requests
            .try_iter()
            .map(|request| request.action)
            .collect::<Vec<_>>(),
        [
            WatchdogAction::List,
            WatchdogAction::List,
            WatchdogAction::Kill { worker: app },
            WatchdogAction::List,
            WatchdogAction::Kill { worker: shell },
            WatchdogAction::List,
        ]
    );
    assert!(!fixture.root.join("config.toml").exists());
    assert!(!fixture.root.join("data").exists());
}

struct Fixture {
    root: PathBuf,
    paths: AppPaths,
    platform: MockPlatform,
    stop: Arc<AtomicBool>,
    server: Option<JoinHandle<()>>,
}

impl Fixture {
    fn new() -> Self {
        // Keep the full Unix socket path within sockaddr_un.sun_path on macOS.
        #[cfg(unix)]
        let temp_dir = std::fs::canonicalize("/tmp").unwrap();
        #[cfg(windows)]
        let temp_dir = std::env::temp_dir();
        let nonce = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = temp_dir.join(format!("tw-{nonce:x}"));
        let paths = AppPaths::from_parts(
            root.join("config.toml"),
            root.join("data"),
            root.join("cache"),
            root.join("logs"),
            root.join("tmp"),
        )
        .unwrap();
        let dirs = UserDirs::new(&root, &root, &root, &root, &root, &root, &root).unwrap();
        Self {
            root,
            platform: MockPlatform::new(dirs, paths.clone()),
            paths,
            stop: Arc::new(AtomicBool::new(false)),
            server: None,
        }
    }

    fn start(
        &mut self,
        run: RunId,
        workers: Vec<SessionInfo>,
        kill_reply: impl Fn(WorkerId) -> WatchdogReply + Send + 'static,
    ) -> mpsc::Receiver<WatchdogRequest> {
        #[cfg(unix)]
        let endpoint = {
            let dir = self.root.join(format!(".tux-{run}"));
            create_private_directory(&dir).unwrap();
            dir.join("watchdog").to_str().unwrap().to_string()
        };
        #[cfg(windows)]
        let endpoint = format!("tundra-watchdog-{run}");
        let mut server =
            Server::start_watchdog(&watchdog_directory(&self.paths), run.to_string(), endpoint)
                .unwrap();
        let stop = Arc::clone(&self.stop);
        let (sender, requests) = mpsc::channel();
        self.server = Some(thread::spawn(move || {
            while !stop.load(Ordering::Relaxed) {
                for (id, bytes) in server.poll() {
                    let request: WatchdogRequest = serde_json::from_slice(&bytes).unwrap();
                    assert_eq!(request.run, run);
                    assert_eq!(request.protocol, PROTOCOL_VERSION);
                    assert_eq!(request.pid, server.info.pid);
                    sender.send(request).unwrap();
                    let response: WatchdogResponse = Ok(match request.action {
                        WatchdogAction::List => WatchdogReply::List(workers.clone()),
                        WatchdogAction::Kill { worker } => kill_reply(worker),
                    });
                    server.respond_json(id, &response);
                }
                thread::sleep(Duration::from_millis(1));
            }
        }));
        requests
    }

    fn success(&self, args: &[&str]) -> Value {
        let (code, output, error) = self.run(args);
        assert_eq!(code, 0, "{args:?}: {error}");
        assert!(error.is_empty());
        serde_json::from_slice(&output).unwrap()
    }

    fn run(&self, args: &[&str]) -> (i32, Vec<u8>, String) {
        let mut output = Vec::new();
        let mut error = Vec::new();
        let code = cli::run_with_platform(args, &self.platform, &mut output, &mut error);
        (code, output, String::from_utf8(error).unwrap())
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(server) = self.server.take() {
            let joined = server.join();
            let _ = std::fs::remove_dir_all(&self.root);
            assert!(joined.is_ok(), "watchdog server fixture failed");
        }
    }
}
