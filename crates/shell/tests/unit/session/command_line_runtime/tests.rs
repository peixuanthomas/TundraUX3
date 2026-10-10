use super::*;
use crate::InputModifiers;
use std::ffi::OsString;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
static NEXT_PTY_READER_TASK_ID: AtomicU64 = AtomicU64::new(1);
#[cfg(any(windows, unix))]
#[test]
fn embedded_panic_exit_enters_fullscreen_panic_without_login_or_dialog() {
    use watchdog::{
        AppCriticality, AppDescriptor, AppId, BoundaryKind, BoundarySpec, WatchdogConfig,
        WatchdogRuntime,
    };
    let root = std::env::temp_dir().join(format!(
        "tundra-command-line-panic-test-{}-{}",
        std::process::id(),
        NEXT_PTY_READER_TASK_ID.fetch_add(1, Ordering::Relaxed),
    ));
    let (runtime, process) = WatchdogRuntime::start_isolated(WatchdogConfig::new(
        root.join("reports"),
        root.join("fallback"),
        root.join("data"),
        "shell-panic-test",
        "test",
    ))
    .unwrap();
    let app = process
        .register_app(AppDescriptor::new(
            AppId::from_static("shell"),
            "Shell",
            "test",
            AppCriticality::ProcessCritical,
        ))
        .unwrap();
    let mut host = CommandLineHost::new(app.clone());
    #[cfg(windows)]
    let (program, args) = ("cmd.exe", vec!["/D", "/C", "exit", "76"]);
    #[cfg(unix)]
    let (program, args) = ("/bin/sh", vec!["-c", "exit 76"]);
    let pty = CommandLinePty::spawn(
        CommandLinePtyConfig {
            program: program.into(),
            args: args.into_iter().map(OsString::from).collect(),
            env: Vec::new(),
            cwd: Some(root.clone()),
            columns: DEFAULT_COLUMNS,
            rows: DEFAULT_ROWS,
            scrollback_lines: 100,
        },
        &host.reader_tasks,
    )
    .unwrap();
    host.state = CommandLineHostState::Running(pty);
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        match host.poll() {
            CommandLineHostEvent::PanicRequested => break,
            CommandLineHostEvent::None => {
                assert!(
                    std::time::Instant::now() < deadline,
                    "panic request never reached Shell"
                );
                std::thread::sleep(Duration::from_millis(10));
            }
            event => panic!("unexpected host event: {event:?}"),
        }
    }
    assert!(matches!(host.state, CommandLineHostState::Inactive));
    let caught = app
        .run_boundary(
            BoundarySpec::new("shell.fullscreen-session", BoundaryKind::UiSession),
            crate::session::runtime::trigger_command_line_panic,
        )
        .expect_err("the Shell must really unwind");
    let id = caught.incident_id().to_string();
    let message = crate::session::runtime::finalize_session_panic(caught, "Shell UI");
    assert_eq!(
        message,
        "Shell UI: Intentional watchdog panic test requested from Command Line"
    );
    let mut state = crate::ShellSession::new(crate::ShellLaunchConfig::default(), (120, 40));
    let screen_before = state.active_screen();
    let message = crate::session::runtime::drain_watchdog_incidents(&mut state, &process)
        .expect("panic must request a standalone crash page even without login");
    assert!(message.contains("Intentional watchdog panic"));
    assert!(!message.contains(&id));
    assert!(!message.contains("Report:"));
    assert!(!message.contains("Recovery:"));
    assert!(state.to_notification_view_model().is_none());
    assert_eq!(state.active_screen(), screen_before);
    let catalog = process.list_incident_reports();
    let report = catalog
        .reports
        .iter()
        .find(|report| report.incident_id == id)
        .unwrap();
    assert_eq!(report.kind, watchdog::IncidentKind::Panic);
    assert_eq!(report.boundary, "shell.fullscreen-session");
    assert!(report.summary.contains("Intentional watchdog panic"));
    assert!(report.text_report_path.as_ref().unwrap().is_file());
    assert!(matches!(
        report.recovery,
        watchdog::RecoveryOutcome::Unrecoverable(_)
    ));
    // Background panic receipts must bypass account-gated notifications too.
    for role in [
        None,
        Some(identity::UserRole::User),
        Some(identity::UserRole::Admin),
    ] {
        let mut state = crate::ShellSession::new(crate::ShellLaunchConfig::default(), (120, 40));
        if let Some(role) = role {
            state.app.dispatch_at(
                app::AppCommand::SetAuthSession(Some(identity::AuthSession {
                    source: identity::IdentitySource::LocalAccount,
                    session_id: "panic-test-session".into(),
                    user_id: "panic-test-user".into(),
                    username: "panic-test".into(),
                    role,
                    started_at_epoch_ms: 1,
                })),
                std::time::Instant::now(),
            );
        }
        let screen_before = state.active_screen();
        let caught = app
            .run_boundary(
                BoundarySpec::new("background-task", BoundaryKind::Worker),
                || panic!("background worker failed to read a file"),
            )
            .expect_err("background task panic");
        caught
            .finalize(watchdog::RecoveryOutcome::Recovered(
                "worker restarted".into(),
            ))
            .unwrap();
        let message = crate::session::runtime::drain_watchdog_incidents(&mut state, &process)
            .expect("every account must see background panic details");
        assert!(message.contains("background worker failed to read a file"));
        assert!(!message.contains("Incident:"));
        assert!(!message.contains("Report:"));
        assert!(!message.contains("Recovery:"));
        assert!(state.to_notification_view_model().is_none());
        assert_eq!(state.active_screen(), screen_before);
    }
    drop(host);
    runtime.shutdown().unwrap();
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn scrollbar_drag_maps_both_track_ends_to_history_ends() {
    let scrollbar = ui::CommandLineScrollbarLayout {
        track: Rect::new(12, 5, 1, 10),
        thumb: Rect::new(12, 9, 1, 2),
    };
    assert_eq!(
        command_line_scrollback_offset_for_thumb(100, scrollbar, 5, 0),
        100
    );
    assert_eq!(
        command_line_scrollback_offset_for_thumb(100, scrollbar, 14, 1),
        0
    );
}

#[test]
fn emergency_shortcut_is_exact() {
    let emergency = KeyInput::with_modifiers(InputKey::Char('x'), InputModifiers::CTRL_SHIFT);
    assert!(is_emergency_termination(&emergency));
    assert!(!is_emergency_termination(&KeyInput::with_modifiers(
        InputKey::Char('x'),
        InputModifiers::CTRL,
    )));
}

/// Windows ConPTY is the production backend for the embedded CLI.  A
/// child merely staying alive does not prove that its pseudo console is
/// usable: a broken reader or writer leaves the Shell with an apparently
/// running, but completely blank, Command Line screen.  Keep this test
/// interactive and bounded so it verifies the two directions separately.
#[cfg(windows)]
#[test]
fn windows_conpty_receives_initial_and_typed_output() {
    use watchdog::{AppCriticality, AppDescriptor, AppId, WatchdogConfig, WatchdogRuntime};

    let root = std::env::temp_dir().join(format!(
        "tundra-command-line-conpty-io-test-{}-{}",
        std::process::id(),
        NEXT_PTY_READER_TASK_ID.fetch_add(1, Ordering::Relaxed)
    ));
    let config = WatchdogConfig::new(
        root.join("reports"),
        root.join("fallback"),
        root.join("data"),
        "command-line-conpty-io-test",
        env!("CARGO_PKG_VERSION"),
    );
    let (runtime, process) = WatchdogRuntime::start_isolated(config).expect("isolated watchdog");
    let app = process
        .register_app(AppDescriptor::new(
            AppId::from_static("shell.command-line-conpty-io-test"),
            "Command Line ConPTY I/O Test",
            env!("CARGO_PKG_VERSION"),
            AppCriticality::Optional,
        ))
        .expect("test app watchdog");
    let reader_tasks = app.task_group("pty-reader");
    let pty = CommandLinePty::spawn(
        CommandLinePtyConfig {
            program: OsString::from("cmd.exe"),
            args: vec![
                OsString::from("/D"),
                OsString::from("/Q"),
                OsString::from("/K"),
                OsString::from("echo TUNDRA_PTY_INITIAL_OUTPUT_OK"),
            ],
            env: Vec::new(),
            cwd: None,
            columns: 80,
            rows: 12,
            scrollback_lines: 100,
        },
        &reader_tasks,
    )
    .expect("interactive ConPTY child");
    let (snapshot, snapshot_revision) = pty.snapshot_with_revision();
    let ui_snapshot = Arc::new(to_ui_snapshot(&snapshot));
    let mut host = CommandLineHost {
        state: CommandLineHostState::Running(pty),
        snapshot,
        snapshot_revision,
        ui_snapshot,
        scrollbar_drag_offset: None,
        reader_tasks,
    };

    // The smallest valid outer Shell (108x22) leaves a 106x14 inner
    // terminal.  This guards against accidentally applying the outer
    // minimum to the already-inset PTY dimensions.
    host.resize_terminal(106, 14);
    assert_eq!((host.snapshot.columns, host.snapshot.rows), (106, 14));
    let pty = match &host.state {
        CommandLineHostState::Running(pty) => pty,
        state => panic!("expected running ConPTY host, got {state:?}"),
    };

    assert_snapshot_contains(
        pty,
        "TUNDRA_PTY_INITIAL_OUTPUT_OK",
        Duration::from_secs(5),
        "initial child output",
    );
    pty.write(b"echo TUNDRA_PTY_TYPED_INPUT_OK\r")
        .expect("write command to ConPTY");
    assert_snapshot_contains(
        pty,
        "TUNDRA_PTY_TYPED_INPUT_OK",
        Duration::from_secs(5),
        "output from typed command",
    );
    const UTF8_MARKER: &str = "TUNDRA_PTY_UTF8_保留所有权利_OK";
    pty.write(format!("echo {UTF8_MARKER}\r").as_bytes())
        .expect("write UTF-8 command to ConPTY");
    assert_snapshot_contains(
        pty,
        UTF8_MARKER,
        Duration::from_secs(5),
        "UTF-8 output from typed command",
    );

    host.terminate();
    drop(host);
    drop(app);
    drop(process);
    runtime.shutdown().expect("watchdog shutdown");
    let _ = std::fs::remove_dir_all(root);
}

#[cfg(windows)]
fn assert_snapshot_contains(
    pty: &CommandLinePty,
    expected: &str,
    timeout: Duration,
    description: &str,
) {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        let snapshot = pty.snapshot();
        let screen = snapshot
            .cells
            .iter()
            .flat_map(|row| row.iter())
            .map(|cell| cell.text.as_str())
            .collect::<String>();
        if screen.contains(expected) {
            return;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "PTY did not render {description}; last screen: {screen:?}",
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[cfg(any(windows, unix))]
#[test]
fn shell_back_button_terminates_the_embedded_terminal() {
    use watchdog::{AppCriticality, AppDescriptor, AppId, WatchdogConfig, WatchdogRuntime};

    let root = std::env::temp_dir().join(format!(
        "tundra-command-line-pty-test-{}-{}",
        std::process::id(),
        NEXT_PTY_READER_TASK_ID.fetch_add(1, Ordering::Relaxed)
    ));
    let config = WatchdogConfig::new(
        root.join("reports"),
        root.join("fallback"),
        root.join("data"),
        "command-line-pty-test",
        env!("CARGO_PKG_VERSION"),
    );
    let (runtime, process) = WatchdogRuntime::start_isolated(config).expect("isolated watchdog");
    let app = process
        .register_app(AppDescriptor::new(
            AppId::from_static("shell.command-line-test"),
            "Command Line PTY Test",
            env!("CARGO_PKG_VERSION"),
            AppCriticality::Optional,
        ))
        .expect("test app watchdog");
    let reader_tasks = app.task_group("pty-reader");

    #[cfg(windows)]
    let (program, args) = (
        OsString::from("cmd.exe"),
        vec![OsString::from("/D"), OsString::from("/Q")],
    );
    #[cfg(unix)]
    let (program, args) = (OsString::from("/bin/sh"), Vec::new());
    #[cfg(target_os = "linux")]
    let environment = vec![
        ("HOME".into(), "/root".into()),
        ("USER".into(), "root".into()),
    ];
    #[cfg(not(target_os = "linux"))]
    let environment = Vec::new();
    let pty = CommandLinePty::spawn(
        CommandLinePtyConfig {
            program,
            args,
            env: environment,
            cwd: None,
            columns: 160,
            rows: 12,
            scrollback_lines: 0,
        },
        &reader_tasks,
    )
    .expect("contained PTY process");
    std::thread::sleep(Duration::from_millis(100));
    assert!(
        pty.try_wait().expect("initial PTY child status").is_none(),
        "interactive PTY child exited before containment was exercised"
    );

    let mut host = CommandLineHost::new(app.clone());
    host.state = CommandLineHostState::Running(pty);
    assert!(matches!(
        host.handle_input(
            &InputEvent::key(InputKey::Escape),
            Some(Rect::new(0, 0, 160, 12))
        ),
        CommandLineHostEvent::None
    ));
    assert!(matches!(host.state, CommandLineHostState::Running(_)));
    let mut state = crate::ShellSession::new(crate::ShellLaunchConfig::default(), (120, 40));
    state.set_navigation_path(vec![
        crate::ShellScreen::Home,
        crate::ShellScreen::CommandLine,
    ]);
    state.refresh_hit_map();
    let back = state
        .hit_map()
        .regions()
        .iter()
        .find(|region| region.component == crate::ShellComponent::BackButton)
        .unwrap()
        .area;
    state.button_regions.push(ui::components::ButtonRegion {
        id: "shell.back".into(),
        area: back,
        disabled: false,
    });
    let now = std::time::Instant::now();
    let down = InputEvent::mouse_down(ui::MouseButton::Left, (back.x, back.y));
    let up = InputEvent::mouse_up(ui::MouseButton::Left, (back.x, back.y));
    assert!(state.prepare_button_input(down, now).is_none());
    let click = state
        .prepare_button_input(up.clone(), now + Duration::from_millis(20))
        .unwrap();
    let input = state.normalize_shell_navigation_input(click);
    assert!(matches!(
        host.handle_input(&input, None),
        CommandLineHostEvent::ExitToCaller
    ));
    assert!(matches!(host.state, CommandLineHostState::Inactive));
    let release = state
        .prepare_button_input(up, now + Duration::from_millis(40))
        .unwrap();
    assert!(matches!(
        host.handle_input(&state.normalize_shell_navigation_input(release), None),
        CommandLineHostEvent::None
    ));
    drop(host);

    drop(reader_tasks);
    drop(app);
    drop(process);
    runtime.shutdown().expect("watchdog shutdown");
    let _ = std::fs::remove_dir_all(root);
}
