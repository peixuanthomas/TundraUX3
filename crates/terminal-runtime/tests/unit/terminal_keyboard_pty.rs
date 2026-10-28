use super::*;
use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use portable_pty::{CommandBuilder, PtySize, native_pty_system};
use std::{
    io::Read,
    sync::mpsc,
    time::{Duration, Instant},
};

const CHILD: &str = "TUNDRA_TEST_KEY_REPORT_CHILD";
const TEST: &str = "session::terminal_keyboard_pty_tests::windows_terminal_reports_round_trip";

#[test]
fn kitty_reports_round_trip_with_all_phases_and_extended_keys() {
    use crossterm::event::{KeyEventState, MediaKeyCode, ModifierKeyCode};
    use ui::{Key as UiKey, KeyEvent as UiKeyEvent, KeyModifiers as UiModifiers};
    const KITTY_CHILD: &str = "TUNDRA_TEST_KITTY_REPORT_CHILD";
    const KITTY_TEST: &str = "session::terminal_keyboard_pty_tests::kitty_reports_round_trip_with_all_phases_and_extended_keys";
    let all_modifiers = UiModifiers {
        shift: true,
        alt: true,
        super_key: true,
        hyper: true,
        meta: true,
        ..UiModifiers::CTRL
    };
    let cases = [
        (
            UiKeyEvent::with_modifiers(UiKey::Enter, UiModifiers::CTRL),
            KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL),
        ),
        (
            UiKeyEvent::with_modifiers(UiKey::Enter, UiModifiers::CTRL).released(),
            KeyEvent::new_with_kind(KeyCode::Enter, KeyModifiers::CONTROL, KeyEventKind::Release),
        ),
        (
            UiKeyEvent::with_modifiers(UiKey::Char('7'), all_modifiers)
                .repeated()
                .with_state(ui::KeyEventState {
                    keypad: true,
                    caps_lock: true,
                    num_lock: true,
                }),
            KeyEvent::new_with_kind_and_state(
                KeyCode::Char('7'),
                KeyModifiers::SHIFT
                    | KeyModifiers::ALT
                    | KeyModifiers::CONTROL
                    | KeyModifiers::SUPER
                    | KeyModifiers::HYPER
                    | KeyModifiers::META,
                KeyEventKind::Repeat,
                KeyEventState::KEYPAD | KeyEventState::CAPS_LOCK | KeyEventState::NUM_LOCK,
            ),
        ),
        (
            UiKeyEvent::new(UiKey::Media(ui::MediaKey::Play)),
            KeyEvent::new(KeyCode::Media(MediaKeyCode::Play), KeyModifiers::NONE),
        ),
        (
            UiKeyEvent::with_modifiers(
                UiKey::Modifier(ui::ModifierKey::RightMeta),
                UiModifiers {
                    meta: true,
                    ..UiModifiers::NONE
                },
            )
            .released(),
            KeyEvent::new_with_kind(
                KeyCode::Modifier(ModifierKeyCode::RightMeta),
                KeyModifiers::META,
                KeyEventKind::Release,
            ),
        ),
        (
            UiKeyEvent::new(UiKey::KeypadBegin),
            KeyEvent::new_with_kind_and_state(
                KeyCode::KeypadBegin,
                KeyModifiers::NONE,
                KeyEventKind::Press,
                KeyEventState::KEYPAD,
            ),
        ),
    ];
    if std::env::var_os(KITTY_CHILD).is_some() {
        let mut guard = TerminalGuard::enter(std::io::stdout()).unwrap();
        assert!(guard.enable_keyboard_reporting().unwrap());
        println!("KITTY_REPORT_READY");
        std::io::stdout().flush().unwrap();
        for (_, expected) in cases {
            assert!(crossterm::event::poll(Duration::from_secs(5)).unwrap());
            assert_eq!(crossterm::event::read().unwrap(), Event::Key(expected));
        }
        guard.restore().unwrap();
        guard.resume().unwrap();
        println!("KITTY_REPORT_RESUMED");
        std::io::stdout().flush().unwrap();
        assert!(crossterm::event::poll(Duration::from_secs(5)).unwrap());
        assert_eq!(
            crossterm::event::read().unwrap(),
            Event::Key(KeyCode::Enter.into())
        );
        guard.restore().unwrap();
        assert!(!is_raw_mode_enabled().unwrap());
        println!("KITTY_REPORT_RESTORED");
        std::io::stdout().flush().unwrap();
        return;
    }
    let pair = native_pty_system()
        .openpty(PtySize {
            rows: 24,
            cols: 100,
            pixel_width: 0,
            pixel_height: 0,
        })
        .unwrap();
    let mut command = CommandBuilder::new(std::env::current_exe().unwrap());
    command.args(["--exact", KITTY_TEST, "--nocapture", "--test-threads=1"]);
    command.env(KITTY_CHILD, "1");
    command.env("TERM", "xterm-256color");
    for name in ["WT_SESSION", "TERM_PROGRAM", "TMUX", "STY"] {
        command.env_remove(name);
    }
    let mut child = KillOnDrop(pair.slave.spawn_command(command).unwrap());
    drop(pair.slave);
    let mut writer = pair.master.take_writer().unwrap();
    let mut reader = pair.master.try_clone_reader().unwrap();
    let (tx, rx) = mpsc::channel();
    let reading = std::thread::spawn(move || {
        let mut bytes = [0; 8192];
        while let Ok(n) = reader.read(&mut bytes) {
            if n == 0 || tx.send(bytes[..n].to_vec()).is_err() {
                break;
            }
        }
    });
    let mut output = Vec::new();
    receive_kitty_until(&rx, &mut output, &mut writer, b"KITTY_REPORT_READY");
    for (key, _) in cases {
        writer
            .write_all(&crate::input::kitty_key_event_bytes(&key, 11).unwrap())
            .unwrap();
    }
    writer.flush().unwrap();
    receive_kitty_until(&rx, &mut output, &mut writer, b"KITTY_REPORT_RESUMED");
    writer
        .write_all(
            &crate::input::kitty_key_event_bytes(&UiKeyEvent::new(UiKey::Enter), 11).unwrap(),
        )
        .unwrap();
    writer.flush().unwrap();
    receive_kitty_until(&rx, &mut output, &mut writer, b"KITTY_REPORT_RESTORED");
    assert!(
        child.0.wait().unwrap().success(),
        "{}",
        String::from_utf8_lossy(&output)
    );
    drop(pair.master);
    drop(writer);
    reading.join().unwrap();
    assert!(
        output
            .windows(6)
            .filter(|bytes| *bytes == b"\x1b[>11u")
            .count()
            >= 2
    );
    assert!(
        output
            .windows(5)
            .filter(|bytes| *bytes == b"\x1b[<1u")
            .count()
            >= 2
    );
}

fn receive_kitty_until(
    rx: &mpsc::Receiver<Vec<u8>>,
    output: &mut Vec<u8>,
    writer: &mut impl Write,
    marker: &[u8],
) {
    let mut answered = output
        .windows(4)
        .filter(|bytes| *bytes == b"\x1b[?u")
        .count();
    let mut cursor_answered = output
        .windows(4)
        .filter(|bytes| *bytes == b"\x1b[6n")
        .count();
    let deadline = Instant::now() + Duration::from_secs(10);
    while !output.windows(marker.len()).any(|window| window == marker) {
        output.extend(
            rx.recv_timeout(deadline.saturating_duration_since(Instant::now()))
                .unwrap_or_else(|error| {
                    panic!(
                        "Kitty PTY missing {}: {error}; {}",
                        String::from_utf8_lossy(marker),
                        String::from_utf8_lossy(output)
                    )
                }),
        );
        let queries = output
            .windows(4)
            .filter(|bytes| *bytes == b"\x1b[?u")
            .count();
        for _ in answered..queries {
            writer.write_all(b"\x1b[?11u\x1b[?1;2c").unwrap();
            writer.flush().unwrap();
        }
        answered = queries;
        let cursor_queries = output
            .windows(4)
            .filter(|bytes| *bytes == b"\x1b[6n")
            .count();
        for _ in cursor_answered..cursor_queries {
            writer.write_all(b"\x1b[1;1R").unwrap();
            writer.flush().unwrap();
        }
        cursor_answered = cursor_queries;
    }
}

#[test]
fn windows_terminal_reports_round_trip() {
    if std::env::var_os(CHILD).is_some() {
        let mut guard = TerminalGuard::enter(std::io::stdout()).unwrap();
        assert!(guard.enable_keyboard_reporting().unwrap());
        println!("KEY_REPORT_READY");
        std::io::stdout().flush().unwrap();
        for expected in [
            KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL),
            KeyEvent::new_with_kind(KeyCode::Enter, KeyModifiers::CONTROL, KeyEventKind::Release),
            KeyEvent::new(KeyCode::Char('j'), KeyModifiers::CONTROL),
            KeyEvent::new(
                KeyCode::Char('X'),
                KeyModifiers::CONTROL | KeyModifiers::SHIFT,
            ),
        ] {
            assert!(crossterm::event::poll(Duration::from_secs(5)).unwrap());
            assert_eq!(crossterm::event::read().unwrap(), Event::Key(expected));
        }
        guard.restore().unwrap();
        assert!(!is_raw_mode_enabled().unwrap());
        guard.resume().unwrap();
        println!("KEY_REPORT_RESUMED");
        std::io::stdout().flush().unwrap();
        assert!(crossterm::event::poll(Duration::from_secs(5)).unwrap());
        assert_eq!(
            crossterm::event::read().unwrap(),
            Event::Key(KeyCode::Enter.into())
        );
        guard.restore().unwrap();
        assert!(!is_raw_mode_enabled().unwrap());
        println!("KEY_REPORT_RESTORED");
        std::io::stdout().flush().unwrap();
        return;
    }
    let pair = native_pty_system()
        .openpty(PtySize {
            rows: 24,
            cols: 100,
            pixel_width: 0,
            pixel_height: 0,
        })
        .unwrap();
    let mut command = CommandBuilder::new(std::env::current_exe().unwrap());
    command.args(["--exact", TEST, "--nocapture", "--test-threads=1"]);
    command.env(CHILD, "1");
    command.env("WT_SESSION", "tundra-key-report-test");
    command.env("TERM", "xterm-256color");
    command.env_remove("TMUX");
    command.env_remove("STY");
    let child = pair.slave.spawn_command(command).unwrap();
    drop(pair.slave);
    let mut child = KillOnDrop(child);
    let mut writer = pair.master.take_writer().unwrap();
    let mut reader = pair.master.try_clone_reader().unwrap();
    let (tx, rx) = mpsc::channel();
    let reading = std::thread::spawn(move || {
        let mut bytes = [0; 8192];
        while let Ok(n) = reader.read(&mut bytes) {
            if n == 0 || tx.send(bytes[..n].to_vec()).is_err() {
                break;
            }
        }
    });
    let mut output = Vec::new();
    receive_until(&rx, &mut output, &mut writer, b"KEY_REPORT_READY");
    assert!(output.windows(8).any(|b| b == b"\x1b[?9001h"));
    writer
        .write_all(
            b"\x1b[13;28;10;1;8;1_\x1b[13;28;10;0;8;1_\x1b[74;36;10;1;8;1_\x1b[88;45;24;1;24;1_",
        )
        .unwrap();
    writer.flush().unwrap();
    receive_until(&rx, &mut output, &mut writer, b"KEY_REPORT_RESUMED");
    assert!(output.windows(8).any(|b| b == b"\x1b[?9001l"));
    writer.write_all(b"\x1b[13;28;13;1;0;1_").unwrap();
    writer.flush().unwrap();
    receive_until(&rx, &mut output, &mut writer, b"KEY_REPORT_RESTORED");
    assert!(
        child.0.wait().unwrap().success(),
        "{}",
        String::from_utf8_lossy(&output)
    );
    drop(pair.master);
    drop(writer);
    reading.join().unwrap();
    assert!(output.windows(8).filter(|b| *b == b"\x1b[?9001h").count() >= 2);
    assert!(output.windows(8).filter(|b| *b == b"\x1b[?9001l").count() >= 2);
}

fn receive_until(
    rx: &mpsc::Receiver<Vec<u8>>,
    output: &mut Vec<u8>,
    writer: &mut impl Write,
    marker: &[u8],
) {
    let mut answered = output
        .windows(4)
        .filter(|bytes| *bytes == b"\x1b[6n")
        .count();
    let deadline = Instant::now() + Duration::from_secs(10);
    while !output.windows(marker.len()).any(|w| w == marker) {
        let remaining = deadline.saturating_duration_since(Instant::now());
        let bytes = rx.recv_timeout(remaining).unwrap_or_else(|error| {
            panic!(
                "PTY missing {}: {error}; {}",
                String::from_utf8_lossy(marker),
                String::from_utf8_lossy(output)
            )
        });
        output.extend(bytes);
        let queries = output
            .windows(4)
            .filter(|bytes| *bytes == b"\x1b[6n")
            .count();
        for _ in answered..queries {
            // A terminal replies to the cursor query used when clearing on resume.
            // ConPTY wraps its VT responses in VK=0 key reports.
            for ch in "\x1b[1;1R".encode_utf16() {
                write!(writer, "\x1b[0;0;{ch};1;0;1_").unwrap();
            }
            writer.flush().unwrap();
        }
        answered = queries;
    }
}

struct KillOnDrop(Box<dyn portable_pty::Child + Send + Sync>);
impl Drop for KillOnDrop {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
fn startup_color_queries_round_trip_without_leaking_into_keyboard_input() {
    const COLOR_CHILD: &str = "TUNDRA_TEST_COLOR_QUERY_CHILD";
    const COLOR_TEST: &str = "session::terminal_keyboard_pty_tests::startup_color_queries_round_trip_without_leaking_into_keyboard_input";
    if std::env::var_os(COLOR_CHILD).is_some() {
        let mut raw = TemporaryRawMode::enter().unwrap();
        let probe = platform::probe_terminal_graphics_capabilities();
        assert_eq!(probe.colors.palette[1], Some([18, 171, 255]));
        assert_eq!(probe.colors.background, Some([10, 20, 30]));
        let capabilities = text_render_capabilities();
        assert_eq!(capabilities.color, ui::ColorCapability::TrueColor);
        assert_eq!(
            capabilities.terminal_colors.palette[1],
            Some([18, 171, 255])
        );
        println!("COLOR_QUERY_COMPLETE");
        std::io::stdout().flush().unwrap();
        assert!(crossterm::event::poll(Duration::from_secs(5)).unwrap());
        assert_eq!(
            crossterm::event::read().unwrap(),
            Event::Key(KeyCode::Char('z').into())
        );
        raw.restore().unwrap();
        println!("COLOR_INPUT_COMPLETE");
        return;
    }
    let pair = native_pty_system()
        .openpty(PtySize {
            rows: 24,
            cols: 100,
            pixel_width: 0,
            pixel_height: 0,
        })
        .unwrap();
    let mut command = CommandBuilder::new(std::env::current_exe().unwrap());
    command.args(["--exact", COLOR_TEST, "--nocapture", "--test-threads=1"]);
    command.env(COLOR_CHILD, "1");
    command.env("TERM", "xterm-256color");
    for name in [
        "COLORTERM",
        "WT_SESSION",
        "TMUX",
        "STY",
        "TERM_PROGRAM",
        "WEZTERM_EXECUTABLE",
    ] {
        command.env_remove(name);
    }
    let child = pair.slave.spawn_command(command).unwrap();
    drop(pair.slave);
    let mut child = KillOnDrop(child);
    let mut writer = pair.master.take_writer().unwrap();
    let mut reader = pair.master.try_clone_reader().unwrap();
    let (tx, rx) = mpsc::channel();
    let reading = std::thread::spawn(move || {
        let mut bytes = [0; 8192];
        while let Ok(n) = reader.read(&mut bytes) {
            if n == 0 || tx.send(bytes[..n].to_vec()).is_err() {
                break;
            }
        }
    });
    let mut output = Vec::new();
    receive_until(&rx, &mut output, &mut writer, b"\x1b[5n");
    for request in [
        b"\x1b]4;0;?".as_slice(),
        b"\x1b]4;15;?",
        b"\x1b]10;?",
        b"\x1b]11;?",
        b"\x1bP+q524742",
    ] {
        assert!(output.windows(request.len()).any(|w| w == request));
    }
    for response in [
        &b"\x1b]4;1;rgb:1234/abcd/ffff\x1b"[..],
        &b"\\\x1b]10;rgb:aa/bb/cc\x07\x1b]11;rgb:0a/14/1e\x1b\\"[..],
        &b"\x1bP1+r524742=38\x1b\\\x1b[0n"[..],
    ] {
        writer.write_all(response).unwrap();
        writer.flush().unwrap();
    }
    receive_until(&rx, &mut output, &mut writer, b"COLOR_QUERY_COMPLETE");
    writer.write_all(b"z").unwrap();
    writer.flush().unwrap();
    receive_until(&rx, &mut output, &mut writer, b"COLOR_INPUT_COMPLETE");
    assert!(
        child.0.wait().unwrap().success(),
        "{}",
        String::from_utf8_lossy(&output)
    );
    drop(pair.master);
    drop(writer);
    reading.join().unwrap();
}
