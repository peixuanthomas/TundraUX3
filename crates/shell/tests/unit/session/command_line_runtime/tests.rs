use super::*;

#[derive(Clone)]
struct CollectingWriter(Arc<Mutex<Vec<u8>>>);

impl Write for CollectingWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        lock_io(&self.0)?.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[test]
fn parsed_pty_output_marks_the_terminal_frame_dirty() {
    let parser = Arc::new(Mutex::new(vt100::Parser::new(2, 8, 0)));
    let output_revision = Arc::new(AtomicU64::new(0));
    let captured = Arc::new(Mutex::new(Vec::new()));
    let writer: Arc<Mutex<Box<dyn Write + Send>>> = Arc::new(Mutex::new(Box::new(
        CollectingWriter(Arc::clone(&captured)),
    )));

    read_pty_output(
        Box::new(io::Cursor::new(b"hello".to_vec())),
        Arc::clone(&parser),
        Arc::clone(&output_revision),
        writer,
    );

    assert_eq!(output_revision.load(Ordering::Acquire), 1);
    assert_eq!(
        lock_io(&parser)
            .expect("terminal parser")
            .screen()
            .contents(),
        "hello"
    );
    assert!(lock_io(&captured).expect("terminal response").is_empty());
}

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

#[cfg(any(windows, unix))]
#[test]
fn pty_process_runs_inside_platform_containment() {
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

    #[cfg(target_os = "linux")]
    {
        let current = platform::linux::identity::LinuxUserContext::current().unwrap();
        for (command, expected) in [
            (
                "printf 'uid='; id -u\r",
                format!("uid={}", current.process.uid),
            ),
            (
                "printf 'ruid='; id -ru\r",
                format!("ruid={}", current.process.uid),
            ),
            (
                "printf 'gid='; id -g\r",
                format!("gid={}", current.process.gid),
            ),
            (
                "printf 'home=%s\\n' \"$HOME\"\r",
                format!("home={}", current.home.display()),
            ),
            (
                "printf 'user=%s\\n' \"$USER\"\r",
                format!("user={}", current.username),
            ),
        ] {
            pty.write(command.as_bytes()).unwrap();
            assert_snapshot_contains(
                &pty,
                &expected,
                Duration::from_secs(5),
                "current Linux user identity",
            );
        }
    }

    pty.force_terminate()
        .expect("terminate contained process tree");
    let exit_deadline = std::time::Instant::now() + Duration::from_secs(5);
    let status = loop {
        if let Some(status) = pty.try_wait().expect("PTY child status") {
            break status;
        }
        assert!(
            std::time::Instant::now() < exit_deadline,
            "contained PTY child did not terminate"
        );
        std::thread::sleep(Duration::from_millis(10));
    };
    let _snapshot = pty.snapshot_after_exit();

    assert!(!status.success);

    drop(reader_tasks);
    drop(app);
    drop(process);
    runtime.shutdown().expect("watchdog shutdown");
    let _ = std::fs::remove_dir_all(root);
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

#[cfg(any(windows, target_os = "linux"))]
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

#[test]
fn strips_osc_52_even_when_sequence_is_split() {
    let mut filter = OscFilter::default();
    assert_eq!(filter.filter(b"safe\x1b]52;c;"), b"safe");
    assert_eq!(filter.filter(b"c2VjcmV0\x07after"), b"after");
}

#[test]
fn strips_c1_osc_and_c1_string_terminator() {
    let mut filter = OscFilter::default();
    assert_eq!(
        filter.filter(b"safe\x9d52;c;payload\x9cafter"),
        b"safeafter"
    );
}

#[test]
fn preserves_utf8_bytes_that_match_c1_osc_controls() {
    let mut filter = OscFilter::default();
    let text = "系统保留所有权利。\u{4fdc}";
    let mut filtered = Vec::new();

    for chunk in text.as_bytes().chunks(2) {
        filtered.extend(filter.filter(chunk));
    }

    assert_eq!(filtered, text.as_bytes());
}

#[test]
fn preserves_csi_but_discards_osc_st_terminated() {
    let mut filter = OscFilter::default();
    assert_eq!(
        filter.filter(b"\x1b[31mred\x1b]0;bad\x1b\\ok"),
        b"\x1b[31mredok"
    );
}

#[test]
fn cursor_position_request_is_replied_to_across_reads_without_matching_other_csi() {
    let captured = Arc::new(Mutex::new(Vec::new()));
    let writer: Arc<Mutex<Box<dyn Write + Send>>> = Arc::new(Mutex::new(Box::new(
        CollectingWriter(Arc::clone(&captured)),
    )));
    let mut responder = TerminalResponder::default();
    responder
        .respond(b"\x1b[6n", (0, 0), &writer)
        .expect("complete request is answered");
    responder
        .respond(b"\x1b[", (2, 4), &writer)
        .expect("partial request is accepted");
    responder
        .respond(b"6n", (2, 4), &writer)
        .expect("cursor-position response is written");
    responder
        .respond(b"\x1b[31m", (9, 9), &writer)
        .expect("ordinary CSI is ignored by responder");
    let output = lock_io(&captured).expect("response writer");
    assert_eq!(output.as_slice(), b"\x1b[1;1R\x1b[3;5R");
}

#[test]
fn snapshot_keeps_terminal_attributes() {
    let mut parser = vt100::Parser::new(2, 8, 0);
    parser.process(b"\x1b[31;1;4;7mX");
    let snapshot = TerminalSnapshot::from_parser(&mut parser);
    let cell = &snapshot.cells[0][0];
    assert_eq!(cell.text, "X");
    assert_eq!(cell.foreground, TerminalColor::Indexed(1));
    assert!(cell.bold);
    assert!(cell.underline);
    assert!(cell.inverse);
}

#[test]
fn snapshot_reports_retained_history_and_hides_the_scrolled_cursor() {
    let mut parser = vt100::Parser::new(2, 8, 10);
    parser.process(b"one\r\ntwo\r\nthree");

    let live = TerminalSnapshot::from_parser(&mut parser);
    assert_eq!(live.scrollback_rows, 1);
    assert_eq!(live.scrollback_offset, 0);
    assert!(live.cursor_visible);

    parser.set_scrollback(usize::MAX);
    let history = TerminalSnapshot::from_parser(&mut parser);
    assert_eq!(history.scrollback_rows, 1);
    assert_eq!(history.scrollback_offset, 1);
    assert!(!history.cursor_visible);
    assert!(
        history.cells[0]
            .iter()
            .map(|cell| &cell.text)
            .any(|text| text == "o")
    );
}

#[test]
fn snapshot_supports_scrollback_deeper_than_the_viewport() {
    let mut parser = vt100::Parser::new(2, 8, 10);
    parser.process(b"one\r\ntwo\r\nthree\r\nfour\r\nfive");
    parser.set_scrollback(usize::MAX);

    let history = TerminalSnapshot::from_parser(&mut parser);

    assert_eq!(history.scrollback_rows, 3);
    assert_eq!(history.scrollback_offset, 3);
    assert!(!history.cursor_visible);
    assert_eq!(history.cells[0][0].text, "o");
    assert_eq!(history.cells[1][0].text, "t");
}

#[test]
fn erase_saved_lines_restores_an_empty_terminal() {
    let mut parser = vt100::Parser::new(2, 8, 10);
    parser.process(b"one\r\ntwo\r\nthree\r\nfour");

    let populated = TerminalSnapshot::from_parser(&mut parser);
    assert_eq!(populated.scrollback_rows, 2);

    parser.process(b"\x1b[3J\x1b[2J\x1b[H");

    let cleared = TerminalSnapshot::from_parser(&mut parser);
    assert_eq!(cleared.scrollback_rows, 0);
    assert_eq!(cleared.scrollback_offset, 0);
    assert_eq!((cleared.cursor_row, cleared.cursor_column), (0, 0));
    assert!(
        cleared
            .cells
            .iter()
            .flatten()
            .all(|cell| cell.text.is_empty())
    );
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
fn cursor_keys_follow_application_cursor_mode() {
    assert_eq!(encode_terminal_input(&TerminalInput::Up, false), b"\x1b[A");
    assert_eq!(encode_terminal_input(&TerminalInput::Up, true), b"\x1bOA");
}

#[test]
fn cli_config_passes_the_current_tundra_username() {
    let config = CommandLinePtyConfig::tundra_cli("tundra-cli").with_username("AdminUser");
    assert_eq!(
        config.env,
        [(
            OsString::from(COMMAND_LINE_USERNAME_ENV),
            OsString::from("AdminUser")
        )]
    );
}

#[test]
fn control_and_alt_keys_encode_for_the_child_terminal() {
    let ctrl_d = KeyInput::with_modifiers(InputKey::Char('d'), InputModifiers::CTRL);
    assert_eq!(key_event_bytes(&ctrl_d, false), Some(vec![0x04]));

    let alt_x = KeyInput::with_modifiers(InputKey::Char('x'), InputModifiers::ALT);
    assert_eq!(key_event_bytes(&alt_x, false), Some(b"\x1bx".to_vec()));
}

#[test]
fn paste_follows_the_child_terminal_mode() {
    assert_eq!(paste_bytes("one\ntwo", false), b"one\ntwo");
    assert_eq!(paste_bytes("one\ntwo", true), b"\x1b[200~one\ntwo\x1b[201~");
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
