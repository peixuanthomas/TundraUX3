use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, mpsc};
use std::time::Duration;

use cli::{CliCommand, CliError, UiAction, UiCommand, UiRequest, WaitState};
use platform::mock::{MockPlatform, UnsupportedPlatform};
use platform::{AppPaths, UserDirs};
use shell_control::{
    Action, Button, Node, Phase, Rect, Request, Response, SessionInfo, Snapshot, Target,
};

fn parse(args: &[&str]) -> UiRequest {
    let CliCommand::Ui(UiCommand::Control(command)) =
        cli::parse_args(args.iter().copied()).unwrap()
    else {
        panic!("expected control command");
    };
    command
}

#[test]
fn parses_every_ui_action_and_common_options() {
    let command = parse(&[
        "ui",
        "snapshot",
        "--session",
        "a-1",
        "--snapshot",
        "42",
        "--timeout",
        "17",
    ]);
    assert_eq!(command.session.as_deref(), Some("a-1"));
    assert_eq!(command.snapshot, Some(42));
    assert_eq!(command.timeout_ms, 17);
    assert_eq!(command.action, UiAction::Input(Action::Snapshot));
    assert_eq!(parse(&["ui", "snapshot"]).timeout_ms, 5000);
    assert_eq!(parse(&["ui", "wake"]).action, UiAction::Input(Action::Wake));
    assert_eq!(
        parse(&["ui", "click", "--id", "save"]).action,
        UiAction::Input(Action::Click {
            target: Target::Id { id: "save".into() },
            button: Button::Left,
            double: false,
        })
    );
    assert_eq!(
        parse(&[
            "ui", "click", "--x", "0", "--y", "65535", "--button", "right", "--double"
        ])
        .action,
        UiAction::Input(Action::Click {
            target: Target::Position { x: 0, y: 65535 },
            button: Button::Right,
            double: true,
        })
    );
    assert_eq!(
        parse(&["ui", "move", "--id", "save"]).action,
        UiAction::Input(Action::Move {
            target: Target::Id { id: "save".into() }
        })
    );
    assert_eq!(
        parse(&[
            "ui",
            "drag",
            "--from-id",
            "thumb",
            "--to-x",
            "10",
            "--to-y",
            "20",
            "--duration",
            "500",
            "--button",
            "middle"
        ])
        .action,
        UiAction::Input(Action::Drag {
            from: Target::Id { id: "thumb".into() },
            to: Target::Position { x: 10, y: 20 },
            duration_ms: 500,
            button: Button::Middle,
        })
    );
    assert_eq!(
        parse(&[
            "ui",
            "scroll",
            "--x",
            "1",
            "--y",
            "2",
            "--direction",
            "left",
            "--count",
            "1000"
        ])
        .action,
        UiAction::Input(Action::Scroll {
            target: Target::Position { x: 1, y: 2 },
            direction: "left".into(),
            count: 1000,
        })
    );
    assert_eq!(
        parse(&[
            "ui",
            "key",
            "S",
            "--modifiers",
            "Ctrl,shift",
            "--phase",
            "release"
        ])
        .action,
        UiAction::Input(Action::Key {
            key: "S".into(),
            modifiers: vec!["ctrl".into(), "shift".into()],
            phase: Phase::Release,
        })
    );
    for key in ["中", "F24", "PageUp", "backtab", "enter", " "] {
        assert!(matches!(
            parse(&["ui", "key", key]).action,
            UiAction::Input(Action::Key { .. })
        ));
    }
    for text in ["", "help", "first\nsecond", "中文 😀"] {
        assert_eq!(
            parse(&["ui", "text", text]).action,
            UiAction::Input(Action::Text { text: text.into() })
        );
    }
    assert_eq!(
        parse(&["ui", "text", "--", "--help"]).action,
        UiAction::Input(Action::Text {
            text: "--help".into()
        })
    );
    assert_eq!(parse(&["ui", "text", "--stdin"]).action, UiAction::Stdin);
    assert_eq!(
        parse(&["ui", "wait", "--id", "save"]).action,
        UiAction::Wait {
            id: "save".into(),
            state: WaitState::Actionable
        }
    );
    for (name, state) in [
        ("present", WaitState::Present),
        ("absent", WaitState::Absent),
    ] {
        assert_eq!(
            parse(&["ui", "wait", "--id", "save", "--state", name]).action,
            UiAction::Wait {
                id: "save".into(),
                state
            }
        );
    }
}

#[test]
fn rejects_missing_conflicting_duplicate_unknown_and_out_of_range_arguments() {
    let cases: &[&[&str]] = &[
        &["ui", "unknown"],
        &["ui", "sessions", "--session", "a"],
        &["ui", "snapshot", "extra"],
        &["ui", "wake", "extra"],
        &["ui", "wake", "--id", "start"],
        &["ui", "snapshot", "--timeout", "0"],
        &["ui", "snapshot", "--timeout", "+1"],
        &["ui", "snapshot", "--timeout", "-1"],
        &["ui", "snapshot", "--snapshot", "1.0"],
        &["ui", "snapshot", "--timeout"],
        &["ui", "snapshot", "--timeout", "18446744073709551616"],
        &["ui", "snapshot", "--timeout", "1", "--timeout", "2"],
        &["ui", "snapshot", "--session", ""],
        &["ui", "snapshot", "--double"],
        &["ui", "click"],
        &["ui", "click", "--x", "1"],
        &["ui", "click", "--x", "65536", "--y", "1"],
        &["ui", "click", "--id", "a", "--x", "1", "--y", "2"],
        &["ui", "click", "--id", "a", "--button", "secondary"],
        &["ui", "click", "--id", "a", "--double", "--double"],
        &["ui", "move", "--id", "a", "--button", "left"],
        &[
            "ui",
            "drag",
            "--from-id",
            "a",
            "--to-id",
            "b",
            "--duration",
            "15",
        ],
        &[
            "ui",
            "drag",
            "--from-id",
            "a",
            "--to-id",
            "b",
            "--duration",
            "10001",
        ],
        &["ui", "drag", "--from-id", "a"],
        &["ui", "scroll", "--id", "a"],
        &["ui", "scroll", "--id", "a", "--direction", "diagonal"],
        &[
            "ui",
            "scroll",
            "--id",
            "a",
            "--direction",
            "down",
            "--count",
            "0",
        ],
        &[
            "ui",
            "scroll",
            "--id",
            "a",
            "--direction",
            "down",
            "--count",
            "1001",
        ],
        &["ui", "key"],
        &["ui", "key", "F25"],
        &["ui", "key", "Ctrl+S"],
        &["ui", "key", "enter", "extra"],
        &["ui", "key", "\n"],
        &["ui", "key", "s", "--modifiers", "ctrl,control"],
        &["ui", "key", "s", "--modifiers", "ctrl,"],
        &["ui", "key", "s", "--modifiers", "capslock"],
        &["ui", "key", "s", "--phase", "down"],
        &["ui", "text"],
        &["ui", "text", "a", "b"],
        &["ui", "text", "a", "--stdin"],
        &["ui", "wait"],
        &["ui", "wait", "--id", "a", "--state", "visible"],
        &["ui", "snapshot", "--json"],
    ];
    for args in cases {
        assert_eq!(
            cli::parse_args(args.iter().copied()),
            Err(CliError::InvalidUiArgument),
            "{args:?}"
        );
    }
}

#[test]
fn ui_help_and_argument_errors_need_no_platform_and_never_echo_text() {
    for args in [
        &["ui"][..],
        &["ui", "help"],
        &["help", "ui"],
        &["help", "ui", "text"],
        &["ui", "text", "--help"],
    ] {
        let parsed = cli::parse_args(args.iter().copied()).unwrap();
        assert!(parsed.is_help_or_version());
        let mut output = Vec::new();
        let mut error = Vec::new();
        assert_eq!(
            cli::run_with_platform(
                args.iter().copied(),
                &UnsupportedPlatform,
                &mut output,
                &mut error
            ),
            0
        );
        assert!(String::from_utf8(output).unwrap().contains("tundra-cli ui"));
        assert!(error.is_empty());
    }
    let mut output = Vec::new();
    let mut error = Vec::new();
    assert_eq!(
        cli::run_with_platform(
            ["ui", "text", "private text", "extra"],
            &UnsupportedPlatform,
            &mut output,
            &mut error
        ),
        2
    );
    let text = String::from_utf8(output).unwrap();
    assert!(!text.contains("private text"));
    assert_eq!(
        serde_json::from_str::<Response>(&text)
            .unwrap()
            .error
            .unwrap()
            .code,
        "invalid_arguments"
    );
    assert!(error.is_empty());
}

#[test]
fn ui_help_and_errors_follow_the_current_chinese_language() {
    let language = i18n::LanguageSnapshotData {
        code: "zh-CN".into(),
        generation: 1,
        current: vec![
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../ascii-assets/assets/locales/zh-CN/modules/settings.ftl"
            ))
            .into(),
        ],
        english: vec![],
    }
    .restore()
    .unwrap();
    i18n::with_snapshot(&Arc::new(language), || {
        let mut output = Vec::new();
        let mut error = Vec::new();
        assert_eq!(
            cli::run_with_platform(
                ["ui", "--help"],
                &UnsupportedPlatform,
                &mut output,
                &mut error
            ),
            0
        );
        let text = String::from_utf8(output).unwrap();
        assert!(text.contains("--session"));
        assert!(text.contains("坐标"));
        let mut output = Vec::new();
        assert_eq!(
            cli::run_with_platform(
                ["ui", "click"],
                &UnsupportedPlatform,
                &mut output,
                &mut error
            ),
            2
        );
        let response: Response = serde_json::from_slice(&output).unwrap();
        let error = response.error.unwrap();
        assert_eq!(error.code, "invalid_arguments");
        assert!(error.message.contains("参数"));
    });
}

struct Fixture {
    root: PathBuf,
    platform: MockPlatform,
    directory: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "tu-cli-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        let paths = AppPaths::from_parts(
            root.join("config"),
            root.join("data"),
            root.join("cache"),
            root.join("logs"),
            root.join("tmp"),
        )
        .unwrap();
        let directory = platform::shell_control::directory(&paths);
        let user_dirs = UserDirs::new(&root, &root, &root, &root, &root, &root, &root).unwrap();
        Self {
            root,
            platform: MockPlatform::new(user_dirs, paths),
            directory,
        }
    }

    fn run(&self, args: &[&str]) -> (i32, serde_json::Value) {
        let mut output = Vec::new();
        let mut error = Vec::new();
        let code = cli::run_with_platform(
            args.iter().copied(),
            &self.platform,
            &mut output,
            &mut error,
        );
        assert!(error.is_empty());
        (code, serde_json::from_slice(&output).unwrap())
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

struct MockServer {
    info: SessionInfo,
    requests: mpsc::Receiver<Request>,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

#[test]
fn an_input_that_closes_the_session_can_succeed_without_a_snapshot() {
    let fixture = Fixture::new();
    let _server = MockServer::start(&fixture.directory, |_, _, _| Some(Response::closed()));
    let (code, response) = fixture.run(&["ui", "key", "Enter"]);
    assert_eq!(code, 0);
    assert_eq!(response["closed"], true);
    assert!(response.get("snapshot").is_none());
}

impl MockServer {
    fn start(
        directory: &Path,
        mut respond: impl FnMut(&SessionInfo, &Request, usize) -> Option<Response> + Send + 'static,
    ) -> Self {
        let mut server = platform::shell_control::Server::start(directory).unwrap();
        let info = server.info.clone();
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = stop.clone();
        let (sender, requests) = mpsc::channel();
        let thread = std::thread::spawn(move || {
            let mut count = 0;
            while !worker_stop.load(Ordering::Relaxed) {
                for (id, data) in server.poll() {
                    let request: Request = serde_json::from_slice(&data).unwrap();
                    count += 1;
                    let response = respond(&server.info, &request, count);
                    sender.send(request).unwrap();
                    if let Some(response) = response {
                        server.respond(id, &response);
                    }
                }
                std::thread::sleep(Duration::from_millis(1));
            }
        });
        Self {
            info,
            requests,
            stop,
            thread: Some(thread),
        }
    }
}

impl Drop for MockServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        self.thread.take().unwrap().join().unwrap();
    }
}

fn snapshot(info: &SessionInfo, number: u64) -> Snapshot {
    Snapshot {
        session: info.session.clone(),
        number,
        page: "test".into(),
        width: 120,
        height: 30,
        animation_running: false,
        background_tasks_running: false,
        nodes: vec![],
        lines: vec!["example".into()],
    }
}

#[test]
fn sessions_selection_and_input_use_real_transport_without_storage() {
    let fixture = Fixture::new();
    assert_eq!(
        fixture.run(&["ui", "sessions"]),
        (0, serde_json::json!({"protocol":1,"ok":true,"sessions":[]}))
    );
    assert!(!fixture.root.exists());
    let (code, error) = fixture.run(&["ui", "snapshot"]);
    assert_eq!(code, 1);
    assert_eq!(error["error"]["code"], "no_session");
    let server = MockServer::start(&fixture.directory, |info, _, _| {
        Some(Response::success(snapshot(info, 42)))
    });
    let (code, sessions) = fixture.run(&["ui", "sessions"]);
    assert_eq!(code, 0, "{sessions}");
    assert_eq!(sessions["sessions"][0]["session"], server.info.session);
    let (code, response) = fixture.run(&["ui", "click", "--id", "save", "--snapshot", "41"]);
    assert_eq!(code, 0, "{response}");
    assert_eq!(response["snapshot"]["number"], 42);
    let request = server.requests.try_recv().unwrap();
    assert_eq!(request.protocol, 1);
    assert_eq!(request.snapshot, Some(41));
    assert!(
        matches!(request.action, Action::Click { target: Target::Id { id }, .. } if id == "save")
    );
    let (code, response) = fixture.run(&["ui", "wake"]);
    assert_eq!(code, 0, "{response}");
    assert_eq!(server.requests.try_recv().unwrap().action, Action::Wake);
    let second = MockServer::start(&fixture.directory, |info, _, _| {
        Some(Response::success(snapshot(info, 1)))
    });
    let (code, response) = fixture.run(&["ui", "snapshot"]);
    assert_eq!(code, 1);
    assert_eq!(response["error"]["code"], "session_required");
    let (code, response) = fixture.run(&["ui", "snapshot", "--session", &second.info.session]);
    assert_eq!(code, 0);
    assert_eq!(response["snapshot"]["session"], second.info.session);
    let (code, response) = fixture.run(&["ui", "snapshot", "--session", "missing"]);
    assert_eq!(code, 1);
    assert_eq!(response["error"]["code"], "session_not_found");
    assert!(!fixture.root.join("config").exists());
    assert!(!fixture.root.join("logs").exists());
    assert!(!fixture.root.join("data").exists());
}

#[test]
fn waits_for_actionable_target_by_reading_only_snapshots() {
    let fixture = Fixture::new();
    let server = MockServer::start(&fixture.directory, |info, _, count| {
        if count == 1 {
            return Some(Response::failure("not_ready", "not ready"));
        }
        let mut snapshot = snapshot(info, count as u64);
        let mut node = Node::new(
            "save",
            "button",
            "Save",
            Rect {
                x: 1,
                y: 2,
                width: 8,
                height: 1,
            },
        );
        node.actionable = count > 2;
        snapshot.nodes.push(node);
        snapshot.animation_running = count == 3;
        Some(Response::success(snapshot))
    });
    let (code, response) = fixture.run(&["ui", "wait", "--id", "save", "--timeout", "2000"]);
    assert_eq!(code, 0, "{response}");
    assert_eq!(response["snapshot"]["number"], 3);
    assert_eq!(response["snapshot"]["animation_running"], true);
    let requests = server.requests.try_iter().collect::<Vec<_>>();
    assert_eq!(requests.len(), 3);
    assert!(
        requests
            .iter()
            .all(|request| request.action == Action::Snapshot)
    );
    let (code, _) = fixture.run(&["ui", "wait", "--id", "missing", "--state", "absent"]);
    assert_eq!(code, 0);
}

#[test]
fn stale_snapshot_and_input_timeout_are_not_retried() {
    let fixture = Fixture::new();
    let server = MockServer::start(&fixture.directory, |_, request, _| {
        if request.snapshot.is_some() {
            Some(Response::failure("stale_snapshot", "changed"))
        } else {
            None
        }
    });
    let (code, response) = fixture.run(&["ui", "wait", "--id", "save", "--snapshot", "1"]);
    assert_eq!(code, 1);
    assert_eq!(response["error"]["code"], "stale_snapshot");
    assert_eq!(server.requests.try_iter().count(), 1);
    let (code, response) = fixture.run(&["ui", "text", "private text", "--timeout", "60"]);
    assert_eq!(code, 1);
    assert_eq!(response["error"]["code"], "timeout");
    assert!(!response.to_string().contains("private text"));
    let requests = server.requests.try_iter().collect::<Vec<_>>();
    assert_eq!(requests.len(), 1);
    assert_eq!(
        requests[0].action,
        Action::Text {
            text: "private text".into()
        }
    );
}

#[test]
fn wait_times_out_and_rejects_mismatched_protocol_responses() {
    let fixture = Fixture::new();
    let server = MockServer::start(&fixture.directory, |info, request, _| {
        let mut response = Response::success(snapshot(info, 1));
        if request.snapshot == Some(2) {
            response.protocol = 2;
        }
        if request.snapshot == Some(3) {
            response.snapshot.as_mut().unwrap().session = "wrong".into();
        }
        Some(response)
    });
    let (code, response) = fixture.run(&["ui", "wait", "--id", "missing", "--timeout", "60"]);
    assert_eq!(code, 1);
    assert_eq!(response["error"]["code"], "timeout");
    assert!(
        server
            .requests
            .try_iter()
            .all(|request| request.action == Action::Snapshot)
    );
    for number in ["2", "3"] {
        let (code, response) = fixture.run(&["ui", "snapshot", "--snapshot", number]);
        assert_eq!(code, 1);
        assert_eq!(response["error"]["code"], "protocol_error");
    }
}

#[test]
fn binary_ui_argument_errors_skip_startup_and_redact_text() {
    let blocked = std::env::current_exe().unwrap().join("unavailable");
    let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_tundra-cli"));
    command.args(["ui", "text", "do not echo this", "--invalid"]);
    command.stdin(std::process::Stdio::null());
    for key in [
        "XDG_CONFIG_HOME",
        "XDG_DATA_HOME",
        "XDG_CACHE_HOME",
        "XDG_STATE_HOME",
        "XDG_RUNTIME_DIR",
        "TMPDIR",
        "TMP",
        "TEMP",
        "APPDATA",
        "LOCALAPPDATA",
    ] {
        command.env(key, &blocked);
    }
    let output = command.output().unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stderr.is_empty());
    let response: Response = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(response.error.unwrap().code, "invalid_arguments");
    assert!(
        !String::from_utf8(output.stdout)
            .unwrap()
            .contains("do not echo this")
    );
}

#[cfg(target_os = "linux")]
#[test]
fn binary_stdin_preserves_text_and_sends_it_only_once() {
    use std::io::Write;
    use std::process::{Command, Stdio};

    let fixture = Fixture::new();
    let server = MockServer::start(
        &fixture.root.join("TundraUX3/shell-control"),
        |info, _, _| Some(Response::success(snapshot(info, 1))),
    );
    let text = "中文 😀\r\nnext line\n";
    for (input, expected_code) in [(text.as_bytes(), 0), (&[0xff][..], 1)] {
        let mut child = Command::new(env!("CARGO_BIN_EXE_tundra-cli"))
            .args(["ui", "text", "--stdin", "--session", &server.info.session])
            .env("XDG_CACHE_HOME", &fixture.root)
            .env("XDG_CONFIG_HOME", fixture.root.join("config"))
            .env("XDG_DATA_HOME", fixture.root.join("data"))
            .env("XDG_STATE_HOME", fixture.root.join("logs"))
            .env("XDG_RUNTIME_DIR", &fixture.root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child.stdin.take().unwrap().write_all(input).unwrap();
        let output = child.wait_with_output().unwrap();
        assert_eq!(output.status.code(), Some(expected_code), "{output:?}");
        assert!(output.stderr.is_empty());
        let response: Response = serde_json::from_slice(&output.stdout).unwrap();
        if expected_code == 0 {
            assert!(response.ok);
            assert_eq!(
                server.requests.try_recv().unwrap().action,
                Action::Text { text: text.into() }
            );
        } else {
            assert_eq!(response.error.unwrap().code, "stdin_error");
        }
        assert!(server.requests.try_recv().is_err());
    }
    for directory in ["config", "data", "logs"] {
        assert!(!fixture.root.join(directory).exists());
    }
}
