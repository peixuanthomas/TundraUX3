//! Exercise rustyline through the same native PTY and screen parser as Shell.
use portable_pty::{CommandBuilder, PtySize, native_pty_system};
#[path = "support/repl_watchdog.rs"]
mod support;
use std::{
    io::{Read, Write},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

#[test]
fn embedded_prompt_cursor_tracks_visible_text_and_editing() {
    const CHILD: &str = "TUNDRA_TEST_REPL_CURSOR_CHILD";
    if std::env::var_os(CHILD).is_some() {
        let runtime = support::ReplWatchdog::new(Some(
            std::env::var_os("TUNDRA_TEST_REPL_WATCHDOG_ROOT")
                .unwrap()
                .into(),
        ));
        let code = cli::run_with_platform_and_watchdog(
            ["repl", "--embedded"],
            &platform::mock::UnsupportedPlatform,
            &mut std::io::stdout(),
            &mut std::io::stderr(),
            &runtime.process,
            runtime.app.clone(),
        );
        drop(runtime);
        std::process::exit(code);
    }
    struct PathFixture(std::path::PathBuf);
    impl Drop for PathFixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    let fixture = PathFixture(std::env::temp_dir().join(format!(
        "tundra-repl-path-{}-{}", std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
    )));
    for (dir, name) in [
        ("first", "tundra-path-start"),
        ("second", "tundra-path-after"),
    ] {
        std::fs::create_dir_all(fixture.0.join(dir)).unwrap();
        let filename = if cfg!(windows) {
            format!("{name}.cmd")
        } else {
            name.into()
        };
        let path = fixture.0.join(dir).join(filename);
        std::fs::write(&path, "echo PATH_TOOL_EXECUTED").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).unwrap();
        }
    }
    let pair = native_pty_system()
        .openpty(PtySize {
            rows: 30,
            cols: 160,
            pixel_width: 0,
            pixel_height: 0,
        })
        .unwrap();
    let mut command = CommandBuilder::new(std::env::current_exe().unwrap());
    command.args([
        "--exact",
        "embedded_prompt_cursor_tracks_visible_text_and_editing",
        "--nocapture",
    ]);
    command.env(CHILD, "1");
    command.env("TUNDRA_COMMAND_LINE_USERNAME", "cursor-test");
    command.env("TUNDRA_COMMAND_LINE_ACCENT", "\x1b[38;2;12;34;56m");
    command.env("TERM", "xterm-256color");
    // Isolate names from installed tools (e.g. Windows HelpPane also matches
    // hel). SystemCommandSession resolves the interpreter independently.
    command.env("PATH", fixture.0.join("first"));
    command.env("TUNDRA_PATH_UPDATE", fixture.0.join("second"));
    // The parent also removes this directory if an assertion kills the child.
    command.env("TUNDRA_TEST_REPL_WATCHDOG_ROOT", fixture.0.join("watchdog"));
    let child = pair.slave.spawn_command(command).unwrap();
    struct StopChild(Box<dyn portable_pty::Child + Send + Sync>);
    impl Drop for StopChild {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let _child = StopChild(child);
    drop(pair.slave);
    let mut reader = pair.master.try_clone_reader().unwrap();
    let writer = Arc::new(Mutex::new(pair.master.take_writer().unwrap()));
    let parser = Arc::new(Mutex::new(vt100::Parser::new(30, 160, 0)));
    let output = Arc::clone(&parser);
    let raw = Arc::new(Mutex::new(Vec::new()));
    let raw_output = Arc::clone(&raw);
    let replies = Arc::clone(&writer);
    std::thread::spawn(move || {
        let mut buffer = [0; 8192];
        let mut query = Vec::new();
        while let Ok(count) = reader.read(&mut buffer) {
            if count == 0 {
                break;
            }
            raw_output
                .lock()
                .unwrap()
                .extend_from_slice(&buffer[..count]);
            let mut parser = output.lock().unwrap();
            for byte in &buffer[..count] {
                parser.process(&[*byte]);
                query.push(*byte);
                if query.len() > 4 {
                    query.remove(0);
                }
                if query == b"\x1b[6n" {
                    let (row, col) = parser.screen().cursor_position();
                    let mut writer = replies.lock().unwrap();
                    writer
                        .write_all(format!("\x1b[{};{}R", row + 1, col + 1).as_bytes())
                        .unwrap();
                    writer.flush().unwrap();
                }
            }
        }
    });
    let wait_for = |expected: &str, cursor_from_end: u16| {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let parser = parser.lock().unwrap();
            let screen = parser.screen();
            let (row, col) = screen.cursor_position();
            let line = screen.rows(0, 160).nth(usize::from(row)).unwrap();
            let visible_end = (0..160)
                .rev()
                .find(|x| {
                    screen.cell(row, *x).is_some_and(|cell| {
                        cell.is_wide_continuation() || !cell.contents().trim().is_empty()
                    })
                })
                .map_or(0, |x| x + 1);
            // Empty input leaves the prompt's final space before the cursor.
            let end = visible_end + u16::from(expected == ">>");
            if line.trim_end().ends_with(expected) && col == end.saturating_sub(cursor_from_end) {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "expected {expected:?}, cursor {} cells before end; actual row={row}, col={col}, visible_end={visible_end}, line={line:?}, raw={:?}",
                cursor_from_end,
                String::from_utf8_lossy(&raw.lock().unwrap())
            );
            drop(parser);
            std::thread::sleep(Duration::from_millis(10));
        }
    };
    let send = |bytes: &[u8]| {
        let mut writer = writer.lock().unwrap();
        writer.write_all(bytes).unwrap();
        writer.flush().unwrap();
    };
    let submit = |command: &str, failed: bool| {
        let previous_row = parser.lock().unwrap().screen().cursor_position().0;
        let start = raw.lock().unwrap().len();
        send(format!("{command}\r").as_bytes());
        let status = format!("\x1b[777;1;{}z", u8::from(failed));
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if raw.lock().unwrap()[start..]
                .windows(status.len())
                .any(|bytes| bytes == status.as_bytes())
            {
                break;
            }
            assert!(Instant::now() < deadline, "no status for {command:?}");
            std::thread::sleep(Duration::from_millis(10));
        }
        loop {
            wait_for(">>", 0);
            let row = parser.lock().unwrap().screen().cursor_position().0;
            // ConPTY can forward status metadata before its screen update.
            // An old empty prompt must not count as the next command's prompt.
            if (command == "/cls" && row == 1)
                || (command != "/cls" && row >= (previous_row + 2).min(29))
            {
                break;
            }
            assert!(Instant::now() < deadline, "no new prompt for {command:?}");
            std::thread::sleep(Duration::from_millis(10));
        }
    };
    wait_for(">>", 0);
    for (input, rendered, suffix_length) in [("/he", "/help", 2), ("hel", "help", 1)] {
        send(input.as_bytes());
        wait_for(&format!(">> {rendered}"), suffix_length);
        let parser = parser.lock().unwrap();
        let screen = parser.screen();
        let (row, col) = screen.cursor_position();
        for column in col - 3..col {
            assert_eq!(
                screen.cell(row, column).unwrap().fgcolor(),
                vt100::Color::Default
            );
        }
        for column in col..col + suffix_length {
            assert_eq!(
                screen.cell(row, column).unwrap().fgcolor(),
                vt100::Color::Idx(8)
            );
        }
        drop(parser);
        send(b"\x15");
        wait_for(">>", 0);
    }
    // Corrections and multiple choices occupy the following row while the
    // cursor stays in the input. Editing must clear that row on both platforms.
    let wait_for_hint_row = |expected: &str| {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let parser = parser.lock().unwrap();
            let screen = parser.screen();
            let (row, _) = screen.cursor_position();
            let hint = screen.rows(0, 160).nth(usize::from(row + 1)).unwrap();
            if (expected.is_empty() && hint.trim().is_empty())
                || (!expected.is_empty() && hint.starts_with(expected))
            {
                for column in 0..u16::try_from(expected.len()).unwrap() {
                    assert_eq!(
                        screen.cell(row + 1, column).unwrap().fgcolor(),
                        vt100::Color::Idx(8)
                    );
                }
                break;
            }
            assert!(
                Instant::now() < deadline,
                "expected hint {expected:?}, got {hint:?}"
            );
            drop(parser);
            std::thread::sleep(Duration::from_millis(10));
        }
    };
    send(b"tundra-path-st");
    wait_for(">> tundra-path-start", 3);
    send(b"\x15tundra-path-strat");
    wait_for(">> tundra-path-strat", 0);
    wait_for_hint_row("Did you mean: tundra-path-start");
    send(b"\x15");
    wait_for(">>", 0);
    submit(
        if cfg!(windows) {
            "set \"PATH=%TUNDRA_PATH_UPDATE%\""
        } else {
            "export PATH=\"$TUNDRA_PATH_UPDATE\""
        },
        false,
    );
    send(b"tundra-path-aft");
    wait_for(">> tundra-path-after", 2);
    send(b"\x15tundra-path-afetr");
    wait_for(">> tundra-path-afetr", 0);
    wait_for_hint_row("Did you mean: tundra-path-after");
    send(b"\t");
    wait_for(">> tundra-path-after", 0);
    send(b"\x15tundra-path-start");
    wait_for(">> tundra-path-start", 0);
    wait_for_hint_row("");
    send(b"\x15");
    wait_for(">>", 0);
    assert!(!String::from_utf8_lossy(&raw.lock().unwrap()).contains("PATH_TOOL_EXECUTED"));
    for (input, hint) in [
        ("hell", "Did you mean: help"),
        ("/hell", "Did you mean: /help"),
        ("git statsu", "Did you mean: git status"),
        ("/debug", "Suggestions: asset | clear-logs | paths"),
    ] {
        send(input.as_bytes());
        wait_for(&format!(">> {input}"), 0);
        wait_for_hint_row(hint);
        send(b"\x1b[C"); // Information is never appended by Right.
        wait_for(&format!(">> {input}"), 0);
        send(b"\x15");
        wait_for(">>", 0);
        wait_for_hint_row("");
    }
    send(b"/debug scr");
    wait_for(">> /debug screen-keyboard", 12);
    send(b"\x15/hell\t");
    wait_for(">> /help", 0);
    send(b"\x15");
    wait_for(">>", 0);
    wait_for_hint_row("");
    send(b"abc");
    wait_for(">> abc", 0);
    {
        let parser = parser.lock().unwrap();
        let screen = parser.screen();
        let (row, col) = screen.cursor_position();
        // Accent stops before the separator and input. Check actual cells,
        // since escape strings alone cannot detect a color leaking into input.
        for column in 2..col - 7 {
            assert_eq!(
                screen.cell(row, column).unwrap().fgcolor(),
                vt100::Color::Rgb(12, 34, 56)
            );
        }
        for column in col - 7..col {
            // ConPTY can group the blank before >> with the preceding color.
            // Only visible separator and input glyphs need the default color.
            if screen
                .cell(row, column)
                .unwrap()
                .contents()
                .trim()
                .is_empty()
            {
                continue;
            }
            assert_eq!(
                screen.cell(row, column).unwrap().fgcolor(),
                vt100::Color::Default,
                "input column={column}, row={row}, cursor={col}, line={:?}, raw={:?}",
                screen.rows(0, 160).nth(usize::from(row)),
                String::from_utf8_lossy(&raw.lock().unwrap())
            );
        }
    }
    send(b"\x1b[D");
    wait_for(">> abc", 1);
    send(b"X");
    wait_for(">> abXc", 1);
    send(b"\x7f");
    wait_for(">> abc", 1);
    send(b"\x05\x15"); // End, then clear the input.
    wait_for(">>", 0);
    send("中文".as_bytes());
    wait_for(">> 中文", 0);
    send(b"\x15/help\r");
    wait_for(">>", 0);
    for (command, code) in [("echo SPACING_TEST", 0), ("/", 2), ("/help '", 1)] {
        // Clear old output so the final block is easy to locate without
        // depending on platform-specific shell errors or help length.
        submit("/cls", false);
        submit(command, code != 0);
        let parser = parser.lock().unwrap();
        let screen = parser.screen();
        let (row, _) = screen.cursor_position();
        assert!(row >= 2, "no command block for {command:?}, row={row}");
        let rows = screen.rows(0, 160).collect::<Vec<_>>();
        assert!(rows[usize::from(row - 1)].trim().is_empty(), "{rows:?}");
        assert!(!rows[usize::from(row - 2)].trim().is_empty(), "{rows:?}");
        if code == 0 {
            assert_eq!(rows[usize::from(row - 2)], "[system exit code: 0]");
            for column in 0..21 {
                assert_eq!(
                    screen.cell(row - 2, column).unwrap().fgcolor(),
                    vt100::Color::Idx(8)
                );
            }
        }
    }
    submit("/help", false);
    send(b"\x1b[A");
    wait_for(">> /help", 0);
    parser.lock().unwrap().set_size(30, 50);
    pair.master
        .resize(PtySize {
            rows: 30,
            cols: 50,
            pixel_width: 0,
            pixel_height: 0,
        })
        .unwrap();
    // Start a fresh prompt at the new width. Unix SIGWINCH can go to libtest's
    // main thread rather than the worker thread running the child REPL.
    send(b"\x05\x15\r");
    wait_for(">>", 0);
    let column = parser.lock().unwrap().screen().cursor_position().1;
    let count = 100 + usize::from((70 - column) % 50);
    send("a".repeat(count).as_bytes());
    let last_row = "a".repeat(20);
    wait_for(&last_row, 0);
    send(b"\x1b[D");
    wait_for(&last_row, 1);
}
