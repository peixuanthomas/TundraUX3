//! Exercise the debug command's real terminal input and rendered button colors.
use platform::{AppPaths, Platform, UserDirs, mock::MockPlatform};
use portable_pty::{CommandBuilder, PtySize, native_pty_system};
use ratatui::layout::Rect;
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::mpsc::{self, Receiver},
    thread::JoinHandle,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use ui::{ScreenKeyboardAction, components::Surface};

const TEST_NAME: &str = "screen_keyboard_accepts_keyboard_and_pointer_input_and_restores_terminal";
const CHILD_ROOT: &str = "TUNDRA_TEST_SCREEN_KEYBOARD_ROOT";
const PRIMARY_MARKER: &str = "SCREEN_KEYBOARD_PRIMARY_SCREEN";
const RESTORED_MARKER: &str = "SCREEN_KEYBOARD_TERMINAL_RESTORED";
const ROWS: u16 = 24;
const COLS: u16 = 80;
const ACCENT: vt100::Color = vt100::Color::Rgb(12, 34, 56);
// Each channel moves 35% toward white, as required for a held button.
const PRESSED: vt100::Color = vt100::Color::Rgb(97, 111, 125);
const TEXT: vt100::Color = vt100::Color::Rgb(230, 241, 244);
const SEEDED_CLIPBOARD: &str = "Seed中文\nλZ\u{1}";
const COPIED_TEXT: &str = "Seed中文\nλZ\nΩ\nTail\nPaste汉!";

#[test]
fn screen_keyboard_accepts_keyboard_and_pointer_input_and_restores_terminal() {
    if let Some(root) = std::env::var_os(CHILD_ROOT) {
        let platform = fixture_platform(Path::new(&root));
        platform.set_clipboard_text(SEEDED_CLIPBOARD);
        #[cfg(windows)]
        let original_input_mode = console_input_mode();
        println!("{PRIMARY_MARKER}");
        std::io::stdout().flush().unwrap();
        let code = cli::run_with_platform(
            ["debug", "screen-keyboard"],
            &platform,
            &mut std::io::stdout(),
            &mut std::io::stderr(),
        );
        assert_eq!(platform.read_clipboard_text().unwrap(), COPIED_TEXT);
        // Windows mouse capture is a console mode, not an output escape code.
        #[cfg(windows)]
        assert_eq!(console_input_mode(), original_input_mode);
        println!("{RESTORED_MARKER}");
        std::io::stdout().flush().unwrap();
        std::process::exit(code);
    }

    let fixture = Fixture::new();
    let mut terminal = PtySession::start(&fixture.0);
    terminal.wait_for("initial keyboard", |screen| {
        screen.alternate_screen()
            && screen.contents().contains("Typed text")
            && screen.contents().contains("F12")
            && screen.contents().contains("Copy")
            && screen.contents().contains("Paste")
    });
    let empty_text = typed_text(terminal.parser.screen(), false);
    assert!(terminal.parser.screen().hide_cursor());
    #[cfg(not(windows))]
    {
        assert_eq!(
            terminal.parser.screen().mouse_protocol_mode(),
            vt100::MouseProtocolMode::AnyMotion
        );
        assert_eq!(
            terminal.parser.screen().mouse_protocol_encoding(),
            vt100::MouseProtocolEncoding::Sgr
        );
    }

    terminal.send(b"aBc");
    terminal.wait_for("physical letter input", |screen| {
        typed_text(screen, false) == "aBc"
    });
    // Rendering, hit testing, and this PTY probe share the same rectangles.
    // Inspect visible label cells because ConPTY can color adjacent blank cells.
    let q = button_position(
        terminal.parser.screen(),
        false,
        ScreenKeyboardAction::Letter('q'),
    );
    terminal.mouse(35, q, false); // SGR mouse move without a held button.
    terminal.wait_for("letter hover accent", |screen| {
        color_at(screen, q) == ACCENT
    });
    terminal.mouse(0, q, false);
    terminal.wait_for("held letter lighter accent", |screen| {
        color_at(screen, q) == PRESSED && typed_text(screen, false) == "aBc"
    });
    terminal.mouse(0, q, true);
    terminal.wait_for("released letter and cleared pointer accent", |screen| {
        color_at(screen, q) == TEXT && typed_text(screen, false) == "aBcq"
    });
    // An identical motion report after release must not resurrect hover.
    terminal.mouse(35, q, false);
    terminal.assert_stays("stationary pointer remains unhighlighted", |screen| {
        color_at(screen, q) == TEXT
    });
    terminal.send(b"D");
    terminal.wait_for("stationary pointer after click", |screen| {
        color_at(screen, q) == TEXT && typed_text(screen, false) == "aBcqD"
    });

    let outside = (0, 0);
    let d = button_position(
        terminal.parser.screen(),
        false,
        ScreenKeyboardAction::Letter('d'),
    );
    terminal.wait_for("physical key selects its button", |screen| {
        color_at(screen, d) == ACCENT
    });
    terminal.mouse(35, outside, false);
    // ConPTY may merge consecutive motion reports. Wait for the move out to
    // clear keyboard focus before sending the move back into the letter.
    terminal.wait_for("move out clears keyboard focus", |screen| {
        color_at(screen, d) == TEXT
    });
    terminal.mouse(35, q, false);
    terminal.wait_for("hover after actual pointer movement", |screen| {
        color_at(screen, q) == ACCENT
    });
    terminal.mouse(0, q, false);
    terminal.wait_for("second letter press", |screen| {
        color_at(screen, q) == PRESSED
    });
    terminal.mouse(32, outside, false); // Drag while holding the left button.
    terminal.wait_for("drag out clears the pressed color", |screen| {
        color_at(screen, q) == TEXT
    });
    terminal.mouse(0, outside, true);
    // A later physical key proves the release was processed without adding q.
    terminal.send(b"E");
    terminal.wait_for("dragged letter was cancelled", |screen| {
        typed_text(screen, false) == "aBcqDE"
    });

    terminal.click(
        ScreenKeyboardAction::Character('1'),
        false,
        "digit click",
        |screen| typed_text(screen, false) == "aBcqDE1",
    );
    terminal.click(
        ScreenKeyboardAction::Character(';'),
        false,
        "symbol click",
        |screen| typed_text(screen, false) == "aBcqDE1;",
    );
    terminal.click(
        ScreenKeyboardAction::Shift,
        false,
        "latched Shift",
        |screen| {
            color_at(
                screen,
                button_position(screen, false, ScreenKeyboardAction::Shift),
            ) == PRESSED
        },
    );
    terminal.click(
        ScreenKeyboardAction::Character('1'),
        false,
        "shifted digit",
        |screen| {
            typed_text(screen, false) == "aBcqDE1;!" && last_key(screen, false).contains("Shift+!")
        },
    );
    terminal.click(
        ScreenKeyboardAction::Character('1'),
        false,
        "Shift remains held",
        |screen| typed_text(screen, false) == "aBcqDE1;!!",
    );
    terminal.click(
        ScreenKeyboardAction::Shift,
        false,
        "release Shift",
        |screen| {
            color_at(
                screen,
                button_position(screen, false, ScreenKeyboardAction::Shift),
            ) == TEXT
        },
    );
    terminal.click(
        ScreenKeyboardAction::Character('1'),
        false,
        "unshifted digit after release",
        |screen| typed_text(screen, false) == "aBcqDE1;!!1",
    );
    terminal.click(
        ScreenKeyboardAction::Function(12),
        false,
        "function key",
        |screen| {
            last_key(screen, false).contains("F12") && typed_text(screen, false) == "aBcqDE1;!!1"
        },
    );
    for (modifier, key, expected) in [
        (
            ScreenKeyboardAction::LeftCtrl,
            ScreenKeyboardAction::Letter('a'),
            "Ctrl+A",
        ),
        (
            ScreenKeyboardAction::RightCtrl,
            ScreenKeyboardAction::Letter('a'),
            "RCtrl+A",
        ),
        (
            ScreenKeyboardAction::Alt,
            ScreenKeyboardAction::Function(1),
            "Alt+F1",
        ),
    ] {
        terminal.click(modifier, false, "modifier toggle", |_| true);
        terminal.click(key, false, expected, |screen| {
            last_key(screen, false).contains(expected)
                && typed_text(screen, false) == "aBcqDE1;!!1"
                && color_at(screen, button_position(screen, false, modifier)) == PRESSED
        });
        terminal.click(modifier, false, "release modifier", |screen| {
            color_at(screen, button_position(screen, false, modifier)) == TEXT
        });
    }
    terminal.click(
        ScreenKeyboardAction::Letter('a'),
        false,
        "Ctrl and Alt were released",
        |screen| typed_text(screen, false) == "aBcqDE1;!!1a",
    );
    terminal.click(
        ScreenKeyboardAction::CapsLock,
        false,
        "Caps Lock on",
        |screen| last_key(screen, false).contains("CapsLock"),
    );
    terminal.click(
        ScreenKeyboardAction::Letter('q'),
        false,
        "Caps Lock first letter",
        |screen| typed_text(screen, false) == "aBcqDE1;!!1aQ",
    );
    terminal.click(
        ScreenKeyboardAction::Letter('w'),
        false,
        "Caps Lock remains active",
        |screen| typed_text(screen, false) == "aBcqDE1;!!1aQW",
    );
    terminal.click(
        ScreenKeyboardAction::CapsLock,
        false,
        "Caps Lock off",
        |screen| last_key(screen, false).contains("CapsLock"),
    );
    terminal.click(
        ScreenKeyboardAction::Letter('q'),
        false,
        "Caps Lock was disabled",
        |screen| typed_text(screen, false) == "aBcqDE1;!!1aQWq",
    );
    terminal.click(ScreenKeyboardAction::Tab, false, "virtual Tab", |screen| {
        last_key(screen, false).contains("Tab")
    });
    terminal.click(
        ScreenKeyboardAction::Backspace,
        false,
        "backspace removes Tab",
        |screen| typed_text(screen, false) == "aBcqDE1;!!1aQWq",
    );

    terminal.click(
        ScreenKeyboardAction::Clear,
        false,
        "clear before clipboard test",
        |screen| typed_text(screen, false) == empty_text,
    );
    terminal.click(
        ScreenKeyboardAction::Paste,
        false,
        "paste from mock clipboard",
        |screen| typed_text(screen, false) == "Seed中文\nλZ",
    );
    terminal.click(
        ScreenKeyboardAction::Enter,
        false,
        "virtual Enter",
        |screen| last_key(screen, false).contains("Enter"),
    );
    terminal.send("Ω\rTail".as_bytes());
    terminal.wait_for("Unicode and physical Enter", |screen| {
        typed_text(screen, false) == "Seed中文\nλZ\nΩ\nTail"
    });
    terminal.click(
        ScreenKeyboardAction::Copy,
        false,
        "copy all lines",
        |screen| last_key(screen, false).contains("Copy"),
    );
    terminal.click(
        ScreenKeyboardAction::Clear,
        false,
        "clear copied text",
        |screen| typed_text(screen, false) == empty_text,
    );
    terminal.click(
        ScreenKeyboardAction::Paste,
        false,
        "paste restores every copied line",
        |screen| typed_text(screen, false) == "Seed中文\nλZ\nΩ\nTail",
    );
    // Unix emits one Paste event for bracketed input. Windows console input
    // records expose its contents as individual keys, so its actual clipboard
    // paste path is covered above and equivalent text is entered here.
    #[cfg(not(windows))]
    terminal.send("\x1b[200~\nPaste汉\x07!\x1b[201~".as_bytes());
    #[cfg(windows)]
    terminal.send("\rPaste汉!".as_bytes());
    terminal.wait_for("pasted or typed multiline Unicode", |screen| {
        typed_text(screen, false) == COPIED_TEXT
    });
    terminal.send(b"\x03");
    terminal.wait_for("physical Ctrl+C copies instead of exiting", |screen| {
        screen.alternate_screen() && last_key(screen, false).contains("Ctrl+C")
    });

    let toggle = button_position(
        terminal.parser.screen(),
        false,
        ScreenKeyboardAction::ToggleKeyboard,
    );
    terminal.mouse(0, toggle, false);
    terminal.wait_for("hide press", |screen| color_at(screen, toggle) == PRESSED);
    let started = Instant::now();
    terminal.mouse(0, toggle, true);
    let original_function_y = button_area(
        terminal.parser.screen(),
        false,
        ScreenKeyboardAction::Function(12),
    )
    .y;
    terminal.wait_for("keyboard slides through intermediate rows", |screen| {
        screen
            .rows(0, COLS)
            .enumerate()
            .any(|(row, text)| row > usize::from(original_function_y) && text.contains("F12"))
    });
    terminal.wait_for("hide keyboard", |screen| {
        screen.contents().contains("Show")
            && !screen.contents().contains("F12")
            && typed_text(screen, true) == COPIED_TEXT
    });
    assert!(
        started.elapsed() >= Duration::from_millis(320),
        "saved 50% speed must slow the 220ms transition"
    );
    terminal.click(
        ScreenKeyboardAction::ToggleKeyboard,
        true,
        "show keyboard",
        |screen| {
            screen.contents().contains("Hide")
                && text_in(
                    screen,
                    button_area(screen, false, ScreenKeyboardAction::Function(12)),
                )
                .concat()
                .contains("F12")
                && typed_text(screen, false) == COPIED_TEXT
        },
    );
    terminal.assert_stays("expanded keyboard settles", |screen| {
        screen.contents().contains("F12")
    });

    terminal.resize(36, 80);
    terminal.wait_for("taller keys show both digit values", |screen| {
        let area = button_area(screen, false, ScreenKeyboardAction::Character('1'));
        let labels = text_in(screen, area);
        area.height >= 2
            && labels.iter().any(|label| label.trim() == "!")
            && labels.iter().any(|label| label.trim() == "1")
            && text_in(
                screen,
                button_area(screen, false, ScreenKeyboardAction::Shift),
            )
            .concat()
            .contains("Shift")
            && text_in(
                screen,
                button_area(screen, false, ScreenKeyboardAction::Exit),
            )
            .concat()
            .contains("Exit")
    });
    terminal.click(
        ScreenKeyboardAction::Shift,
        false,
        "Shift swaps the active glyph",
        |screen| {
            let area = button_area(screen, false, ScreenKeyboardAction::Character('1'));
            let shifted = (area.x..area.right())
                .find(|&column| screen.cell(area.y, column).unwrap().contents() == "!")
                .unwrap();
            let ordinary = (area.x..area.right())
                .find(|&column| screen.cell(area.y + 1, column).unwrap().contents() == "1")
                .unwrap();
            color_at(screen, (area.y, shifted)) == TEXT
                && color_at(screen, (area.y + 1, ordinary)) != TEXT
        },
    );
    terminal.click(
        ScreenKeyboardAction::Shift,
        false,
        "release Shift before wrapping",
        |screen| {
            color_at(
                screen,
                button_position(screen, false, ScreenKeyboardAction::Shift),
            ) == TEXT
        },
    );
    terminal.resize(ROWS, COLS);
    terminal.wait_for("compact keyboard after tall layout", |screen| {
        screen.size() == (ROWS, COLS)
            && button_area(screen, false, ScreenKeyboardAction::Character('1')).height == 2
            && screen.contents().contains("F12")
            && text_in(
                screen,
                button_area(screen, false, ScreenKeyboardAction::Exit),
            )
            .concat()
            .contains("Exit")
    });

    terminal.click(
        ScreenKeyboardAction::Clear,
        false,
        "clear before wrapping",
        |screen| typed_text(screen, false) == empty_text,
    );
    let wrapped = format!("wrap-{}\n中文", "0123456789".repeat(8));
    terminal.send(wrapped.replace('\n', "\r").as_bytes());
    terminal.wait_for("long text wraps without losing characters", |screen| {
        let lines = typed_lines(screen, false);
        lines.len() >= 3 && lines.concat() == wrapped.replace('\n', "")
    });
    let old_q = button_position(
        terminal.parser.screen(),
        false,
        ScreenKeyboardAction::Letter('q'),
    );
    terminal.mouse(0, old_q, false);
    terminal.wait_for("press before resize", |screen| {
        color_at(screen, old_q) == PRESSED
    });
    terminal.resize(20, 64);
    terminal.wait_for("responsive keyboard after resize", |screen| {
        screen.contents().contains("F12")
            && text_in(
                screen,
                button_area(screen, false, ScreenKeyboardAction::Letter('q')),
            )
            .concat()
            .contains('Q')
            && typed_lines(screen, false).concat() == wrapped.replace('\n', "")
    });
    let resized_q = button_position(
        terminal.parser.screen(),
        false,
        ScreenKeyboardAction::Letter('q'),
    );
    terminal.mouse(0, resized_q, true);
    terminal.send(b"x");
    terminal.wait_for("resize cancels the pending click", |screen| {
        typed_lines(screen, false).concat() == format!("{}x", wrapped.replace('\n', ""))
    });
    let exit = button_position(terminal.parser.screen(), false, ScreenKeyboardAction::Exit);
    terminal.mouse(35, exit, false);
    terminal.wait_for("exit hover", |screen| color_at(screen, exit) == ACCENT);
    terminal.mouse(0, exit, false);
    terminal.wait_for("exit press", |screen| color_at(screen, exit) == PRESSED);
    terminal.mouse(0, exit, true);
    terminal.wait_for("restored primary terminal", |screen| {
        !screen.alternate_screen()
            && !screen.hide_cursor()
            && screen.contents().contains(PRIMARY_MARKER)
            && screen.contents().contains(RESTORED_MARKER)
    });
    assert_eq!(
        terminal.parser.screen().mouse_protocol_mode(),
        vt100::MouseProtocolMode::None
    );
    assert_eq!(
        terminal.parser.screen().mouse_protocol_encoding(),
        vt100::MouseProtocolEncoding::Default
    );
    terminal.finish();
}

fn fixture_platform(root: &Path) -> MockPlatform {
    let dirs = UserDirs::new(
        root.join("Desktop"),
        root.join("Documents"),
        root.join("Downloads"),
        root.join("Pictures"),
        root.join("Videos"),
        root.join("Music"),
        root.join("Data"),
    )
    .unwrap();
    let paths = AppPaths::from_parts(
        root.join("config.toml"),
        root.join("state"),
        root.join("cache"),
        root.join("logs"),
        root.join("temp"),
    )
    .unwrap();
    MockPlatform::new(dirs, paths)
}

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().canonicalize().unwrap().join(format!(
            "tundra-screen-keyboard-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir(&root).unwrap();
        let fixture = Self(root);
        let platform = fixture_platform(&fixture.0);
        let storage = storage::StorageManager::from_layout(storage::StorageLayout::from_app_paths(
            &platform.app_paths().unwrap(),
        ));
        let mut config = storage::StorageConfig::default();
        config.appearance.accent_color = storage::BorderColor::Rgb(12, 34, 56);
        config.appearance.motion_preference = storage::MotionPreference::Full;
        config.appearance.animation_speed_percent = 50;
        storage.save_config(&config).unwrap();
        fixture
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

struct PtySession {
    child: Option<Box<dyn portable_pty::Child + Send + Sync>>,
    master: Option<Box<dyn portable_pty::MasterPty + Send>>,
    writer: Option<Box<dyn Write + Send>>,
    reader: Option<JoinHandle<()>>,
    output: Receiver<Vec<u8>>,
    parser: vt100::Parser,
    raw: Vec<u8>,
    query: Vec<u8>,
}

impl PtySession {
    fn start(root: &Path) -> Self {
        let pair = native_pty_system()
            .openpty(PtySize {
                rows: ROWS,
                cols: COLS,
                pixel_width: 0,
                pixel_height: 0,
            })
            .unwrap();
        let mut command = CommandBuilder::new(std::env::current_exe().unwrap());
        command.args(["--exact", TEST_NAME, "--nocapture"]);
        command.env(CHILD_ROOT, root);
        command.env("TERM", "xterm-256color");
        command.env("COLORTERM", "truecolor");
        // The test runner can inherit NO_COLOR=1 from a noninteractive shell.
        command.env("NO_COLOR", "");
        let child = pair.slave.spawn_command(command).unwrap();
        drop(pair.slave);
        let writer = pair.master.take_writer().unwrap();
        let mut reader = pair.master.try_clone_reader().unwrap();
        let (sender, output) = mpsc::channel();
        let reader = std::thread::spawn(move || {
            let mut buffer = [0; 8192];
            while let Ok(count) = reader.read(&mut buffer) {
                if count == 0 || sender.send(buffer[..count].to_vec()).is_err() {
                    break;
                }
            }
        });
        Self {
            child: Some(child),
            master: Some(pair.master),
            writer: Some(writer),
            reader: Some(reader),
            output,
            parser: vt100::Parser::new(ROWS, COLS, 0),
            raw: Vec::new(),
            query: Vec::new(),
        }
    }

    fn send(&mut self, bytes: &[u8]) {
        let writer = self.writer.as_mut().unwrap();
        writer.write_all(bytes).unwrap();
        writer.flush().unwrap();
    }

    fn mouse(&mut self, button: u8, (row, column): (u16, u16), released: bool) {
        self.send(
            format!(
                "\x1b[<{button};{};{}{}",
                column + 1,
                row + 1,
                if released { 'm' } else { 'M' }
            )
            .as_bytes(),
        );
    }

    fn click(
        &mut self,
        action: ScreenKeyboardAction,
        collapsed: bool,
        description: &str,
        expected: impl Fn(&vt100::Screen) -> bool,
    ) {
        let position = button_position(self.parser.screen(), collapsed, action);
        self.mouse(0, position, false);
        self.wait_for(description, |screen| color_at(screen, position) == PRESSED);
        self.mouse(0, position, true);
        // A modifier can change labels after release. Consume that redraw before
        // locating the next button, even when its resulting state is checked later.
        self.wait_for(description, |screen| {
            let modifier = match action {
                ScreenKeyboardAction::Shift => Some("Shift"),
                ScreenKeyboardAction::CapsLock => Some("CapsLock"),
                ScreenKeyboardAction::LeftCtrl => Some("Ctrl"),
                ScreenKeyboardAction::RightCtrl => Some("RCtrl"),
                ScreenKeyboardAction::Alt => Some("Alt"),
                _ => None,
            };
            (color_at(screen, position) != PRESSED
                || modifier.is_some_and(|name| last_key(screen, collapsed).contains(name)))
                && expected(screen)
        });
    }

    fn resize(&mut self, rows: u16, cols: u16) {
        self.master
            .as_ref()
            .unwrap()
            .resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .unwrap();
        self.parser.set_size(rows, cols);
    }

    fn receive(&mut self) {
        if let Ok(bytes) = self.output.recv_timeout(Duration::from_millis(20)) {
            self.raw.extend_from_slice(&bytes);
            for byte in bytes {
                self.parser.process(&[byte]);
                self.query.push(byte);
                if self.query.len() > 4 {
                    self.query.remove(0);
                }
                if self.query == b"\x1b[6n" {
                    let (row, column) = self.parser.screen().cursor_position();
                    self.send(format!("\x1b[{};{}R", row + 1, column + 1).as_bytes());
                }
            }
        }
    }

    fn wait_for(&mut self, description: &str, expected: impl Fn(&vt100::Screen) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(8);
        while !expected(self.parser.screen()) {
            assert!(
                Instant::now() < deadline,
                "timed out waiting for {description}; screen={:?}; raw={:?}",
                self.parser.screen().contents(),
                String::from_utf8_lossy(&self.raw)
            );
            self.receive();
        }
    }

    fn assert_stays(&mut self, description: &str, expected: impl Fn(&vt100::Screen) -> bool) {
        // Give a report which should produce no visible change time to arrive.
        // A later keyboard event would clear an erroneous hover and hide the bug.
        let deadline = Instant::now() + Duration::from_millis(250);
        while Instant::now() < deadline {
            self.receive();
            assert!(expected(self.parser.screen()), "{description}");
        }
    }

    fn finish(&mut self) {
        let deadline = Instant::now() + Duration::from_secs(5);
        let status = loop {
            if let Some(status) = self.child.as_mut().unwrap().try_wait().unwrap() {
                break status;
            }
            assert!(Instant::now() < deadline, "keyboard child did not exit");
            self.receive();
        };
        assert!(status.success(), "keyboard child failed: {status:?}");
        self.close();
        let deadline = Instant::now() + Duration::from_secs(5);
        while !self.reader.as_ref().unwrap().is_finished() {
            assert!(
                Instant::now() < deadline,
                "PTY reader did not stop after close"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        self.reader.take().unwrap().join().unwrap();
    }

    fn close(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        self.writer.take();
        self.master.take();
    }
}

impl Drop for PtySession {
    fn drop(&mut self) {
        // Killing the child closes the Unix slave; closing ConPTY releases its
        // output pipe. Either path lets the blocking reader finish on failures.
        self.close();
        if let Some(reader) = self.reader.take() {
            let deadline = Instant::now() + Duration::from_secs(5);
            while !reader.is_finished() && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(10));
            }
            if reader.is_finished() {
                let _ = reader.join();
            }
        }
    }
}

fn layout(screen: &vt100::Screen, collapsed: bool) -> ui::ScreenKeyboardLayout {
    let (rows, cols) = screen.size();
    ui::screen_keyboard_layout(Rect::new(0, 0, cols, rows), collapsed)
}

fn button_position(
    screen: &vt100::Screen,
    collapsed: bool,
    action: ScreenKeyboardAction,
) -> (u16, u16) {
    let area = button_area(screen, collapsed, action);
    for row in area.y..area.bottom() {
        for col in area.x..area.right() {
            let cell = screen.cell(row, col).unwrap();
            if !cell.contents().trim().is_empty()
                && matches!(cell.fgcolor(), TEXT | ACCENT | PRESSED)
            {
                return (row, col);
            }
        }
    }
    panic!(
        "button {action:?} has no visible label in {:?}",
        screen.contents()
    );
}

fn button_area(screen: &vt100::Screen, collapsed: bool, action: ScreenKeyboardAction) -> Rect {
    layout(screen, collapsed)
        .buttons
        .into_iter()
        .find(|button| button.action == action)
        .unwrap_or_else(|| panic!("button {action:?} missing in {:?}", screen.contents()))
        .area
}

fn text_in(screen: &vt100::Screen, area: Rect) -> Vec<String> {
    let mut lines: Vec<_> = screen
        .rows(area.x, area.width)
        .skip(usize::from(area.y))
        .take(usize::from(area.height))
        .map(|line| line.trim_end().to_string())
        .collect();
    while lines.last().is_some_and(String::is_empty) {
        lines.pop();
    }
    lines
}

fn typed_lines(screen: &vt100::Screen, collapsed: bool) -> Vec<String> {
    let text = layout(screen, collapsed).text;
    text_in(screen, Surface::new().bordered(true).inner(text))
}

fn typed_text(screen: &vt100::Screen, collapsed: bool) -> String {
    typed_lines(screen, collapsed).join("\n")
}

fn last_key(screen: &vt100::Screen, collapsed: bool) -> String {
    text_in(screen, layout(screen, collapsed).status).join("\n")
}

fn color_at(screen: &vt100::Screen, (row, column): (u16, u16)) -> vt100::Color {
    screen.cell(row, column).unwrap().fgcolor()
}

#[cfg(windows)]
fn console_input_mode() -> u32 {
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetStdHandle(handle: u32) -> *mut std::ffi::c_void;
        fn GetConsoleMode(handle: *mut std::ffi::c_void, mode: *mut u32) -> i32;
    }
    let mut mode = 0;
    // STD_INPUT_HANDLE is the signed Windows constant -10 represented as DWORD.
    assert_ne!(
        unsafe { GetConsoleMode(GetStdHandle(-10_i32 as u32), &mut mode) },
        0
    );
    mode
}
