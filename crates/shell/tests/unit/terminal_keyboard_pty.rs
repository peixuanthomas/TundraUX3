use super::*;
use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use portable_pty::{CommandBuilder, PtySize, native_pty_system};
use std::{
    io::Read,
    sync::mpsc,
    time::{Duration, Instant},
};

const CHILD: &str = "TUNDRA_TEST_KEY_REPORT_CHILD";
const TEST: &str =
    "terminal_session::terminal_keyboard_pty_tests::windows_terminal_reports_round_trip";

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
