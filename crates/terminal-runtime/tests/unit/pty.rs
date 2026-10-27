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

#[test]
fn recorded_output_survives_beyond_the_terminal_scrollback_without_osc_controls() {
    let root =
        platform::create_temp_dir(&std::env::temp_dir().join("TundraUX3"), "recorded-pty-test")
            .unwrap();
    let store = platform::task_history::HistoryStore::new(&root);
    let summary =
        platform::task_history::TaskSummary::new("output".into(), "script".into(), "script".into());
    let recorder = TaskRecorder::new(store.clone(), summary).unwrap();
    let mut text = "中文输出\r\n".repeat(3000).into_bytes();
    text.extend_from_slice(b"\x1b]52;c;blocked\x07END");
    let parser = Arc::new(Mutex::new(vt100::Parser::new(2, 12, 0)));
    let writer: Arc<Mutex<Box<dyn Write + Send>>> = Arc::new(Mutex::new(Box::new(
        CollectingWriter(Arc::new(Mutex::new(Vec::new()))),
    )));
    read_pty_output_recorded(
        Box::new(io::Cursor::new(text)),
        parser,
        Arc::new(AtomicU64::new(0)),
        writer,
        Some(&recorder),
    );
    let (bytes, _) = store.output(recorder.id(), 0, 256 * 1024).unwrap();
    let archived = String::from_utf8(bytes).unwrap();
    assert_eq!(archived.matches("中文输出").count(), 3000);
    assert!(!archived.contains("blocked"));
    assert!(archived.ends_with("END"));
    platform::cleanup_temp_path(&root).unwrap();
}

#[test]
fn clearing_a_resized_row_does_not_leave_half_a_wide_character_or_stop_the_reader() {
    // The reported crash erased column 173 in a 174-column terminal: resizing
    // had removed the second cell of a wide character at the right edge.
    for columns in [1, 8, 174] {
        for erase in ["\x1b[K", "\x1b[1K", "\x1b[J", "\x1b[1J", "\x1b[X", "Z"] {
            let mut parser = vt100::Parser::new(3, columns + 1, 0);
            parser.process(format!("\x1b[1;{columns}H\x1b[44m中").as_bytes());
            parser.set_size(2, columns);
            let parser = Arc::new(Mutex::new(parser));
            let revision = Arc::new(AtomicU64::new(0));
            let captured = Arc::new(Mutex::new(Vec::new()));
            let writer: Arc<Mutex<Box<dyn Write + Send>>> =
                Arc::new(Mutex::new(Box::new(CollectingWriter(captured))));
            read_pty_output(
                Box::new(io::Cursor::new(format!("{erase}\r\nALIVE").into_bytes())),
                Arc::clone(&parser),
                Arc::clone(&revision),
                writer,
            );
            assert_eq!(revision.load(Ordering::Acquire), 1);
            let parser = lock_io(&parser).expect("terminal reader must not poison the parser");
            assert!(!parser.screen().cell(0, columns - 1).unwrap().is_wide());
            if columns > 1 {
                assert!(parser.screen().contents().contains("ALIVE"));
            }
        }
    }
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
        PtyProcess::spawn(
            PtyConfig {
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
    let pty = PtyProcess::spawn(
        PtyConfig {
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

#[cfg(unix)]
#[test]
fn pty_cleanup_is_repeatable_for_running_and_exited_children() {
    use watchdog::{AppCriticality, AppDescriptor, AppId, WatchdogConfig, WatchdogRuntime};

    let root = std::env::temp_dir().join(format!(
        "tundra-pty-normal-exit-{}-{}",
        std::process::id(),
        NEXT_PTY_READER_TASK_ID.fetch_add(1, Ordering::Relaxed)
    ));
    let (runtime, process) = WatchdogRuntime::start_isolated(WatchdogConfig::new(
        root.join("reports"),
        root.join("fallback"),
        root.join("data"),
        "pty-normal-exit-test",
        env!("CARGO_PKG_VERSION"),
    ))
    .unwrap();
    let app = process
        .register_app(AppDescriptor::new(
            AppId::from_static("terminal.normal-exit-test"),
            "Terminal normal exit test",
            env!("CARGO_PKG_VERSION"),
            AppCriticality::Optional,
        ))
        .unwrap();
    let tasks = app.task_group("terminal");
    for attempt in 0..34 {
        let command = if attempt == 0 {
            "exit 0"
        } else {
            "exec sleep 60"
        };
        let pty = PtyProcess::spawn(
            PtyConfig {
                program: "/bin/sh".into(),
                args: vec!["-c".into(), command.into()],
                env: Vec::new(),
                cwd: None,
                columns: 80,
                rows: 24,
                scrollback_lines: 0,
            },
            &tasks,
        )
        .unwrap();
        if attempt == 0 {
            let deadline = std::time::Instant::now() + Duration::from_secs(5);
            loop {
                if let Some(status) = pty.try_wait().unwrap() {
                    assert!(status.success);
                    break;
                }
                assert!(std::time::Instant::now() < deadline);
                std::thread::sleep(Duration::from_millis(10));
            }
            pty.snapshot_after_exit();
        } else if attempt == 1 {
            pty.force_terminate().unwrap();
            std::thread::sleep(Duration::from_millis(50));
            pty.force_terminate().unwrap();
            drop(pty);
        } else {
            drop(pty);
        }
    }
    drop(tasks);
    drop(app);
    runtime.shutdown().unwrap();
    assert!(process.drain_incidents().is_empty());
    drop(process);
    std::fs::remove_dir_all(root).unwrap();
}

#[cfg(any(windows, unix))]
#[test]
fn watchdog_shutdown_stops_terminal_even_if_the_view_still_owns_it() {
    use watchdog::{AppCriticality, AppDescriptor, AppId, WatchdogConfig, WatchdogRuntime};
    let root = std::env::temp_dir().join(format!(
        "tundra-pty-shutdown-{}-{}",
        std::process::id(),
        NEXT_PTY_READER_TASK_ID.fetch_add(1, Ordering::Relaxed)
    ));
    let config = WatchdogConfig::new(
        root.join("reports"),
        root.join("fallback"),
        root.join("state"),
        "pty-shutdown-test",
        "test",
    );
    let (runtime, process) = WatchdogRuntime::start_isolated(config).unwrap();
    let app = process
        .register_app(AppDescriptor::new(
            AppId::from_static("pty-test"),
            "PTY test",
            "test",
            AppCriticality::Optional,
        ))
        .unwrap();
    #[cfg(windows)]
    let (program, args) = ("cmd.exe", vec!["/D".into(), "/Q".into()]);
    #[cfg(unix)]
    let (program, args) = ("/bin/sh", Vec::new());
    let pty = PtyProcess::spawn(
        PtyConfig {
            program: program.into(),
            args,
            env: Vec::new(),
            cwd: None,
            columns: 80,
            rows: 24,
            scrollback_lines: 0,
        },
        &app.task_group("terminal"),
    )
    .unwrap();
    assert!(pty.try_wait().unwrap().is_none());
    let start = std::time::Instant::now();
    let stopped = runtime.shutdown();
    assert!(start.elapsed() < Duration::from_secs(4));
    assert!(pty.try_wait().unwrap().is_some());
    drop(pty);
    stopped.unwrap();
    std::fs::remove_dir_all(root).unwrap();
}

#[cfg(target_os = "linux")]
#[test]
fn retained_output_scrolls_and_resizes_after_the_process_and_watchdog_close() {
    use watchdog::{AppCriticality, AppDescriptor, AppId, WatchdogConfig, WatchdogRuntime};

    let root = std::env::temp_dir().join(format!(
        "tundra-pty-retained-output-{}-{}",
        std::process::id(),
        NEXT_PTY_READER_TASK_ID.fetch_add(1, Ordering::Relaxed)
    ));
    let (runtime, process) = WatchdogRuntime::start_isolated(WatchdogConfig::new(
        root.join("reports"),
        root.join("fallback"),
        root.join("data"),
        "pty-retained-output-test",
        env!("CARGO_PKG_VERSION"),
    ))
    .unwrap();
    let app = process
        .register_app(AppDescriptor::new(
            AppId::from_static("terminal.retained-output-test"),
            "Terminal retained output test",
            env!("CARGO_PKG_VERSION"),
            AppCriticality::Optional,
        ))
        .unwrap();
    let tasks = app.task_group("terminal");
    let pty = PtyProcess::spawn(
        PtyConfig {
            program: "/bin/sh".into(),
            args: vec![
                "-c".into(),
                "i=0; while [ \"$i\" -lt 40 ]; do printf 'line-%02d\\n' \"$i\"; i=$((i+1)); done"
                    .into(),
            ],
            env: Vec::new(),
            cwd: None,
            columns: 20,
            rows: 4,
            scrollback_lines: 100,
        },
        &tasks,
    )
    .unwrap();
    let pid = pty.process_id().unwrap().unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while pty.try_wait().unwrap().is_none() {
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(10));
    }
    let output = pty.into_output().unwrap();
    assert!(!Path::new(&format!("/proc/{pid}")).exists());
    drop(tasks);
    drop(app);
    runtime.shutdown().unwrap();
    assert!(process.drain_incidents().is_empty());
    drop(process);

    let snapshot_text = |snapshot: TerminalSnapshot| {
        snapshot
            .cells
            .iter()
            .flatten()
            .map(|cell| cell.text.as_str())
            .collect::<String>()
    };
    assert!(snapshot_text(output.snapshot()).contains("line-39"));
    assert!(output.set_scrollback(usize::MAX).unwrap());
    assert!(snapshot_text(output.snapshot()).contains("line-00"));
    output.resize(32, 6).unwrap();
    output.set_scrollback(usize::MAX).unwrap();
    let resized = output.snapshot();
    assert_eq!((resized.columns, resized.rows), (32, 6));
    assert!(snapshot_text(resized).contains("line-00"));
    output.set_scrollback(0).unwrap();
    assert!(snapshot_text(output.snapshot()).contains("line-39"));
    std::fs::remove_dir_all(root).unwrap();
}

#[cfg(any(windows, target_os = "linux"))]
fn assert_snapshot_contains(
    pty: &PtyProcess,
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
    parser.process(b"\x1b[31;1;3;4;7mX\x1b[6 q\x1b[?1002h\x1b[?1006h");
    let snapshot = TerminalSnapshot::from_parser(&mut parser);
    let cell = &snapshot.cells[0][0];
    assert_eq!(cell.text, "X");
    assert_eq!(cell.foreground, TerminalColor::Indexed(1));
    assert!(cell.bold);
    assert!(cell.italic);
    assert!(cell.underline);
    assert!(cell.inverse);
    assert_eq!(snapshot.cursor_style, TerminalCursorStyle::SteadyBar);
    assert_eq!(
        snapshot.mouse_protocol_mode,
        MouseProtocolMode::ButtonMotion
    );
    assert_eq!(snapshot.mouse_protocol_encoding, MouseProtocolEncoding::Sgr);
    let rendered = to_ui_snapshot(&snapshot);
    assert!(rendered.cell(0, 0).unwrap().style.italic);
    assert_eq!(rendered.cursor_style, ui::CommandLineCursorStyle::SteadyBar);
}

#[cfg(target_os = "linux")]
#[test]
fn child_input_modes_receive_keys_paste_and_content_relative_mouse_reports() {
    use watchdog::{AppCriticality, AppDescriptor, AppId, WatchdogConfig, WatchdogRuntime};

    let root = std::env::temp_dir().join(format!(
        "tundra-child-mouse-test-{}-{}",
        std::process::id(),
        NEXT_PTY_READER_TASK_ID.fetch_add(1, Ordering::Relaxed)
    ));
    let (runtime, process) = WatchdogRuntime::start_isolated(WatchdogConfig::new(
        root.join("reports"),
        root.join("fallback"),
        root.join("data"),
        "child-mouse-test",
        env!("CARGO_PKG_VERSION"),
    ))
    .unwrap();
    let app = process
        .register_app(AppDescriptor::new(
            AppId::from_static("terminal.mouse-test"),
            "Child mouse test",
            env!("CARGO_PKG_VERSION"),
            AppCriticality::Optional,
        ))
        .unwrap();
    let tasks = app.task_group("pty-reader");
    let pty = PtyProcess::spawn(
        PtyConfig {
            program: "/bin/sh".into(),
            args: vec![
                "-c".into(),
                r"stty raw -echo; printf '\033[?1h\033[?2004h\033[?1002h\033[?1006hREADY'; dd bs=1 count=46 2>/dev/null | od -An -v -tx1 | tr -d ' \n'; printf '\033[?1l\033[?2004lNEXT'; dd bs=1 count=6 2>/dev/null | od -An -v -tx1 | tr -d ' \n'; printf DONE".into(),
            ],
            env: Vec::new(),
            cwd: None,
            columns: 100,
            rows: 10,
            scrollback_lines: 0,
        },
        &tasks,
    )
    .unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        let snapshot = pty.snapshot();
        if snapshot.mouse_protocol_mode == MouseProtocolMode::ButtonMotion
            && snapshot.mouse_protocol_encoding == MouseProtocolEncoding::Sgr
        {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "child did not enable tracking"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_snapshot_contains(&pty, "READY", Duration::from_secs(5), "mouse setup");
    let (_, revision) = pty.snapshot_with_revision();
    pty.resize(100, 10).unwrap();
    assert!(pty.snapshot_if_changed(revision).is_none());
    let area = ratatui::layout::Rect::new(10, 5, 100, 10);
    assert!(
        !pty.send_mouse(&ui::MouseEvent::down(9, 8, ui::MouseButton::Left), area)
            .unwrap()
    );
    for event in [
        ui::MouseEvent::down(12, 8, ui::MouseButton::Left),
        ui::MouseEvent::drag(14, 9, ui::MouseButton::Left),
        ui::MouseEvent::up(14, 9, ui::MouseButton::Left),
    ] {
        assert!(pty.send_mouse(&event, area).unwrap());
    }
    pty.send_key(&ui::KeyEvent::new(ui::Key::Up)).unwrap();
    pty.send_key(&ui::KeyEvent::new(ui::Key::Up).released())
        .unwrap();
    pty.send_paste("a\nb").unwrap();
    assert_snapshot_contains(
        &pty,
        "1b5b3c303b333b344d1b5b3c33323b353b354d1b5b3c303b353b356d1b4f411b5b3230307e610a621b5b3230317eNEXT",
        Duration::from_secs(5),
        "child application keys, bracketed paste and mouse reports",
    );
    pty.send_key(&ui::KeyEvent::new(ui::Key::Down)).unwrap();
    pty.send_paste("a\nb").unwrap();
    assert_snapshot_contains(
        &pty,
        "1b5b42610a62DONE",
        Duration::from_secs(5),
        "child reset input modes",
    );
    drop(pty);
    drop(tasks);
    drop(app);
    drop(process);
    runtime.shutdown().unwrap();
    std::fs::remove_dir_all(root).unwrap();
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
fn generic_config_does_not_add_cli_arguments_or_private_environment() {
    let config = PtyConfig::new("example-tui");
    assert_eq!(config.program, OsString::from("example-tui"));
    assert!(config.args.is_empty());
    assert!(config.env.is_empty());
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
