//! Exercise rustyline through the same native PTY and screen parser as Shell.
use portable_pty::{CommandBuilder, PtySize, native_pty_system};
use std::{
    io::{Read, Write},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

#[test]
fn embedded_prompt_cursor_tracks_visible_text_and_editing() {
    const CHILD: &str = "TUNDRA_TEST_REPL_CURSOR_CHILD";
    if std::env::var_os(CHILD).is_some() {
        std::process::exit(cli::run_with_platform(
            ["repl", "--embedded"],
            &platform::mock::UnsupportedPlatform,
            &mut std::io::stdout(),
            &mut std::io::stderr(),
        ));
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
    command.env("TERM", "xterm-256color");
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
    wait_for(">>", 0);
    send(b"abc");
    wait_for(">> abc", 0);
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
