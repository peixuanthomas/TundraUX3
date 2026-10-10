use super::*;
use std::path::Path;

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
fn pty_rejects_invalid_start_directories_before_running_the_child() {
    use watchdog::{AppCriticality, AppDescriptor, AppId, WatchdogConfig, WatchdogRuntime};

    let root = std::env::temp_dir().join(format!(
        "tundra-command-line-invalid-cwd-test-{}-{}",
        std::process::id(),
        NEXT_PTY_READER_TASK_ID.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&root).unwrap();
    let file = root.join("ordinary-file");
    std::fs::write(&file, "not a directory").unwrap();
    let (runtime, process) = WatchdogRuntime::start_isolated(WatchdogConfig::new(
        root.join("reports"),
        root.join("fallback"),
        root.join("data"),
        "command-line-invalid-cwd-test",
        env!("CARGO_PKG_VERSION"),
    ))
    .expect("isolated watchdog");
    let app = process
        .register_app(AppDescriptor::new(
            AppId::from_static("shell.command-line-invalid-cwd-test"),
            "Command Line Start Directory Test",
            env!("CARGO_PKG_VERSION"),
            AppCriticality::Optional,
        ))
        .expect("test app watchdog");
    let reader_tasks = app.task_group("pty-reader");
    #[cfg(windows)]
    let (program, args) = {
        // Keep cmd redirection in a batch file: portable-pty quotes argv with
        // C-style escaping, which cmd does not accept for embedded quotes.
        let script = root.join("write-marker.cmd");
        std::fs::write(
            &script,
            b"@echo off\r\necho started>\"%TUNDRA_CWD_MARKER%\"\r\n",
        )
        .unwrap();
        (
            "cmd.exe",
            vec!["/D".into(), "/C".into(), script.into_os_string()],
        )
    };
    #[cfg(unix)]
    let (program, args) = (
        "/bin/sh",
        vec![OsString::from("-c"), ": > \"$TUNDRA_CWD_MARKER\"".into()],
    );
    let spawn_marker = |cwd: &Path, marker: &Path| {
        CommandLinePty::spawn(
            CommandLinePtyConfig {
                program: program.into(),
                args: args.clone(),
                env: vec![("TUNDRA_CWD_MARKER".into(), marker.as_os_str().to_owned())],
                cwd: Some(cwd.to_owned()),
                columns: DEFAULT_COLUMNS,
                rows: DEFAULT_ROWS,
                scrollback_lines: 0,
            },
            &reader_tasks,
        )
    };

    let valid_marker = root.join("started-valid");
    let pty = spawn_marker(&root, &valid_marker).expect("valid start directory");
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(status) = pty.try_wait().expect("fixture child status") {
            assert!(
                status.success,
                "marker command exited with {status:?}; output: {:?}",
                pty.snapshot()
                    .cells
                    .iter()
                    .flatten()
                    .map(|cell| cell.text.as_str())
                    .collect::<String>()
            );
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "marker command did not finish"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    drop(pty);
    assert!(
        valid_marker.is_file(),
        "the fixture command must really execute"
    );

    for (cwd, marker, expected_kind) in [
        (
            root.join("missing-directory"),
            root.join("started-missing"),
            io::ErrorKind::NotFound,
        ),
        (
            file,
            root.join("started-file"),
            io::ErrorKind::NotADirectory,
        ),
    ] {
        let error = match spawn_marker(&cwd, &marker) {
            Ok(pty) => {
                drop(pty);
                panic!("invalid start directory was accepted: {}", cwd.display());
            }
            Err(error) => error,
        };
        assert_eq!(error.kind(), expected_kind, "{}", error);
        assert!(error.to_string().contains(cwd.to_string_lossy().as_ref()));
        assert!(!marker.exists(), "invalid cwd must not execute the child");
    }

    drop(reader_tasks);
    drop(app);
    drop(process);
    runtime.shutdown().expect("watchdog shutdown");
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

    let child = Arc::clone(&pty.child);
    pty.force_terminate().unwrap();
    let exit_deadline = std::time::Instant::now() + Duration::from_secs(5);
    let status = loop {
        if let Some(status) = child.lock().unwrap().try_wait().expect("PTY child status") {
            break status;
        }
        assert!(
            std::time::Instant::now() < exit_deadline,
            "contained PTY child did not terminate"
        );
        std::thread::sleep(Duration::from_millis(10));
    };
    assert!(!status.success());
    drop(pty);

    drop(reader_tasks);
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
fn cursor_keys_follow_application_cursor_mode() {
    assert_eq!(encode_terminal_input(&TerminalInput::Up, false), b"\x1b[A");
    assert_eq!(encode_terminal_input(&TerminalInput::Up, true), b"\x1bOA");
}

#[test]
fn cli_config_passes_the_current_tundra_username_and_accent() {
    let config = CommandLinePtyConfig::tundra_cli("tundra-cli")
        .with_username("AdminUser")
        .with_accent_color(ratatui::style::Color::Rgb(12, 34, 56));
    assert_eq!(
        config.env,
        [
            (
                OsString::from(COMMAND_LINE_USERNAME_ENV),
                OsString::from("AdminUser")
            ),
            (
                OsString::from(COMMAND_LINE_ACCENT_ENV),
                OsString::from("\x1b[38;2;12;34;56m")
            )
        ]
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
fn command_status_follows_wrapped_prompts_into_history_and_survives_redraws() {
    use ui::components::CommandStatus;
    let mut parser = vt100::Parser::new(3, 12, 20);
    let mut filter = OscFilter::default();
    // Split every byte, including the UTF-8 marker and private CSI sequence.
    for byte in "○\x1b[777;0z user >>".as_bytes() {
        parser.process(&filter.filter(&[*byte]));
    }
    let snapshot = to_ui_snapshot(&TerminalSnapshot::from_parser(&mut parser));
    assert_eq!(
        snapshot.cell(0, 0).unwrap().command_status,
        Some(CommandStatus::Pending)
    );
    // Rustyline redraws the prompt before accepting a long command.
    parser.process("\r\x1b[0K○\x1b[777;0z user >> long-command\r\noutput\r\nmore\r\n".as_bytes());
    // An interactive child can enter and leave the alternate screen.
    parser.process(b"\x1b[?1049hchild screen\x1b[?1049l\x1b[777;1;0z");
    parser.process("○\x1b[777;0z user >> bad\r\n\x1b[777;1;1z".as_bytes());
    parser.process("○\x1b[777;0z user >> ".as_bytes());
    parser.set_size(4, 12);

    let mut statuses = Vec::new();
    let total = TerminalSnapshot::from_parser(&mut parser).scrollback_rows;
    for offset in (0..=total).rev() {
        parser.set_scrollback(offset);
        let snapshot = to_ui_snapshot(&TerminalSnapshot::from_parser(&mut parser));
        // Each top row is visited once; include the remaining live rows last.
        let rows = if offset == 0 { snapshot.rows } else { 1 };
        for row in 0..rows {
            for column in 0..snapshot.columns {
                if let Some(status) = snapshot.cell(column, row).unwrap().command_status {
                    statuses.push(status);
                }
            }
        }
    }
    assert_eq!(
        statuses,
        [
            CommandStatus::Succeeded,
            CommandStatus::Failed,
            CommandStatus::Pending
        ]
    );

    parser.set_scrollback(0);
    parser.process(b"\x1b[3J\x1b[2J\x1b[H\x1b[777;1;1z");
    let cleared = TerminalSnapshot::from_parser(&mut parser);
    assert_eq!(cleared.scrollback_rows, 0);
    assert!(
        cleared
            .cells
            .iter()
            .flatten()
            .all(|cell| cell.command_status.is_none())
    );
}

#[test]
fn command_status_rejects_unmarked_output_and_is_removed_on_overwrite() {
    let mut parser = vt100::Parser::new(2, 20, 10);
    parser.process(b"ordinary\x1b[777;0z\x1b[777;1;0z");
    assert!(
        TerminalSnapshot::from_parser(&mut parser)
            .cells
            .iter()
            .flatten()
            .all(|cell| cell.command_status.is_none())
    );
    parser.process("\r○\x1b[777;0z".as_bytes());
    parser.process(b"\x1b[777;1;2z");
    assert_eq!(
        parser.screen().cell(0, 0).unwrap().command_status(),
        Some(None)
    );
    parser.process(b"\rX\x1b[777;1;0z");
    assert_eq!(parser.screen().cell(0, 0).unwrap().command_status(), None);
}
