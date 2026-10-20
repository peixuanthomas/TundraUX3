#![cfg(target_os = "linux")]

use portable_pty::{CommandBuilder, PtySize, native_pty_system};
use std::{
    io::{Read, Write},
    sync::mpsc,
    time::{Duration, Instant},
};

fn wait_for(
    parser: &mut vt100::Parser,
    receiver: &mpsc::Receiver<Vec<u8>>,
    description: &str,
    condition: impl Fn(&vt100::Screen) -> bool,
) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !condition(parser.screen()) {
        assert!(
            Instant::now() < deadline,
            "timeout: {description}\n{}",
            parser.screen().contents()
        );
        if let Ok(bytes) = receiver.recv_timeout(Duration::from_millis(50)) {
            parser.process(&bytes);
        }
    }
}

#[test]
fn rainbow_demo_animates_accepts_keyboard_and_touch_and_restores_terminal() {
    for touch in [false, true] {
        let root =
            std::env::temp_dir().join(format!("tundra-rainbow-pty-{}-{touch}", std::process::id()));
        std::fs::create_dir(&root).unwrap();
        let pair = native_pty_system()
            .openpty(PtySize {
                rows: 16,
                cols: 80,
                pixel_width: 0,
                pixel_height: 0,
            })
            .unwrap();
        let mut command = CommandBuilder::new(env!("CARGO_BIN_EXE_tundra-cli"));
        command.args(["debug", "test-rainbow-text"]);
        command.env("TERM", "xterm-256color");
        command.env("COLORTERM", "truecolor");
        for variable in [
            "XDG_CONFIG_HOME",
            "XDG_DATA_HOME",
            "XDG_CACHE_HOME",
            "XDG_STATE_HOME",
        ] {
            command.env(variable, root.join(variable));
        }
        let mut child = pair.slave.spawn_command(command).unwrap();
        drop(pair.slave);
        let mut reader = pair.master.try_clone_reader().unwrap();
        let mut writer = pair.master.take_writer().unwrap();
        let (sender, receiver) = mpsc::channel();
        let reader = std::thread::spawn(move || {
            let mut buffer = [0; 4096];
            while let Ok(count) = reader.read(&mut buffer) {
                if count == 0 || sender.send(buffer[..count].to_vec()).is_err() {
                    break;
                }
            }
        });
        let mut parser = vt100::Parser::new(16, 80, 0);
        wait_for(&mut parser, &receiver, "rainbow sample", |screen| {
            screen.contents().contains("彩虹文字") && screen.contents().contains("Exit [Esc]")
        });
        let (row, line) = parser
            .screen()
            .rows(0, 80)
            .enumerate()
            .find(|(_, line)| line.contains("彩虹文字"))
            .unwrap();
        let column = line.chars().position(|character| character == 'R').unwrap() as u16;
        let row = row as u16;
        let initial = parser.screen().cell(row, column).unwrap().fgcolor();
        assert_ne!(
            initial,
            parser.screen().cell(row, column + 1).unwrap().fgcolor()
        );
        wait_for(
            &mut parser,
            &receiver,
            "changing foreground colors",
            |screen| screen.cell(row, column).unwrap().fgcolor() != initial,
        );
        if touch {
            let (row, line) = parser
                .screen()
                .rows(0, 80)
                .enumerate()
                .find(|(_, line)| line.contains("Exit [Esc]"))
                .unwrap();
            let column = line.chars().position(|character| character == 'E').unwrap();
            write!(
                writer,
                "\x1b[<0;{};{}M\x1b[<0;{};{}m",
                column + 1,
                row + 1,
                column + 1,
                row + 1
            )
            .unwrap();
        } else {
            writer.write_all(b"\x1b").unwrap();
        }
        writer.flush().unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        let status = loop {
            if let Some(status) = child.try_wait().unwrap() {
                break status;
            }
            if Instant::now() >= deadline {
                child.kill().unwrap();
                panic!("rainbow demo did not exit");
            }
            if let Ok(bytes) = receiver.recv_timeout(Duration::from_millis(50)) {
                parser.process(&bytes);
            }
        };
        assert!(status.success());
        reader.join().unwrap();
        for bytes in receiver.try_iter() {
            parser.process(&bytes);
        }
        assert!(!parser.screen().alternate_screen());
        assert!(!parser.screen().hide_cursor());
        assert_eq!(
            parser.screen().mouse_protocol_mode(),
            vt100::MouseProtocolMode::None
        );
        assert!(
            std::fs::read_dir(&root).unwrap().next().is_none(),
            "demo should not initialize saved settings or logs"
        );
        std::fs::remove_dir(&root).unwrap();
    }
}
