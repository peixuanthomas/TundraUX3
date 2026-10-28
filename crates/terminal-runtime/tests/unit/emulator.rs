use super::*;

#[derive(Clone)]
struct RecordingWriter(Arc<Mutex<Vec<u8>>>);

impl Write for RecordingWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn terminal(rows: u16, columns: u16) -> (Emulator, SharedWriter, Arc<Mutex<Vec<u8>>>) {
    let bytes = Arc::new(Mutex::new(Vec::new()));
    let writer: SharedWriter = Arc::new(Mutex::new(Box::new(RecordingWriter(bytes.clone()))));
    (
        Emulator::new(rows, columns, 20, writer.clone()),
        writer,
        bytes,
    )
}

fn text(terminal: &mut Emulator) -> String {
    terminal
        .snapshot()
        .cells
        .into_iter()
        .map(|row| {
            row.into_iter()
                .map(|cell| cell.text)
                .collect::<String>()
                .trim_end()
                .to_owned()
        })
        .collect()
}

#[test]
fn queries_reply_at_their_position_in_the_stream_and_survive_split_sequences() {
    let (mut terminal, _writer, replies) = terminal(4, 20);
    terminal.process(b"ab\x1b[6ncd\x1b[6n");
    assert_eq!(&*replies.lock().unwrap(), b"\x1b[1;3R\x1b[1;5R");
    replies.lock().unwrap().clear();
    for byte in b"\x1b[2;4H\x1b[6n" {
        terminal.process(&[*byte]);
    }
    assert_eq!(&*replies.lock().unwrap(), b"\x1b[2;4R");
}

#[test]
fn iterm_cell_queries_report_the_current_virtual_pixel_geometry() {
    let (mut terminal, _writer, replies) = terminal(4, 20);
    terminal.process(b"\x1b]1337;ReportCellSize\x07");
    assert_eq!(
        &*replies.lock().unwrap(),
        b"\x1b]1337;ReportCellSize=16.0;8.0\x1b\\"
    );
    replies.lock().unwrap().clear();
    terminal.set_cell_size(10, 20);
    terminal.process(b"\x1b]1337;ReportCellSize\x1b\\");
    assert_eq!(
        &*replies.lock().unwrap(),
        b"\x1b]1337;ReportCellSize=20.0;10.0\x1b\\"
    );
}

#[test]
fn clipboard_title_and_unknown_osc_cannot_escape_or_enter_history() {
    let (mut terminal, _writer, replies) = terminal(4, 20);
    let history = terminal
        .process(b"safe\x1b]52;c;YmxvY2tlZA==\x07\x1b]2;blocked\x1b\\\x1b]999;blocked\x07end");
    assert_eq!(history, b"safeend");
    assert_eq!(text(&mut terminal), "safeend");
    assert!(replies.lock().unwrap().is_empty());
}

#[test]
fn kitty_input_negotiation_preserves_phase_and_legacy_restores_after_pop() {
    let (mut terminal, _writer, replies) = terminal(4, 20);
    terminal.process(b"\x1b[>11u\x1b[?u");
    terminal
        .send_key(&ui::KeyEvent::new(ui::Key::Char('a')))
        .unwrap();
    terminal
        .send_key(&ui::KeyEvent::new(ui::Key::Char('a')).repeated())
        .unwrap();
    terminal
        .send_key(&ui::KeyEvent::new(ui::Key::Char('a')).released())
        .unwrap();
    assert_eq!(
        &*replies.lock().unwrap(),
        b"\x1b[?11u\x1b[97;1u\x1b[97;1:2u\x1b[97;1:3u"
    );
    replies.lock().unwrap().clear();
    terminal.process(b"\x1b[<u");
    terminal
        .send_key(&ui::KeyEvent::new(ui::Key::Char('a')))
        .unwrap();
    terminal
        .send_key(&ui::KeyEvent::new(ui::Key::Char('a')).released())
        .unwrap();
    assert_eq!(&*replies.lock().unwrap(), b"a");
}

#[test]
fn focus_and_mouse_release_are_delivered_only_when_requested() {
    let (mut terminal, _writer, replies) = terminal(4, 20);
    assert!(
        !terminal
            .send_mouse(ui::MouseEvent::down(2, 1, ui::MouseButton::Left))
            .unwrap()
    );
    terminal.process(b"\x1b[?1004h\x1b[?1000h\x1b[?1006h");
    terminal.send_focus(false);
    terminal.send_focus(true);
    assert!(
        terminal
            .send_mouse(ui::MouseEvent::down(2, 1, ui::MouseButton::Left))
            .unwrap()
    );
    assert!(
        terminal
            .send_mouse(ui::MouseEvent::up(2, 1, ui::MouseButton::Left))
            .unwrap()
    );
    let bytes = replies.lock().unwrap();
    assert!(bytes.windows(6).any(|part| part == b"\x1b[O\x1b[I"));
    assert!(bytes.ends_with(b"\x1b[<0;3;2M\x1b[<0;3;2m"));
}

#[test]
fn synchronized_frames_publish_on_reset_or_timeout() {
    let (mut terminal, _writer, _replies) = terminal(4, 20);
    terminal.process(b"before\x1b[?2026h\r\x1b[2Kafter");
    assert_eq!(text(&mut terminal), "before");
    terminal.process(b"\x1b[?2026l");
    assert_eq!(text(&mut terminal), "after");
    terminal.process(b"\x1b[?2026hmore");
    terminal.synchronized = Some(Instant::now() - SYNC_TIMEOUT);
    assert!(terminal.refresh_sync());
    assert_eq!(text(&mut terminal), "aftermore");
}

#[test]
fn command_status_follows_cells_through_scrollback_and_resize() {
    let (mut terminal, _writer, _replies) = terminal(2, 20);
    terminal.process("○\x1b[777;0z user >> command\r\noutput\r\nmore\r\n".as_bytes());
    terminal.process(b"\x1b[?1049hchild\x1b[?1049l\x1b[777;1;0z");
    terminal.resize(16, 3);
    terminal.set_scrollback(usize::MAX);
    let snapshot = terminal.snapshot();
    assert!(
        snapshot
            .cells
            .iter()
            .flatten()
            .any(|cell| cell.command_status == Some(Some(true)))
    );
    terminal.process(b"\x1b[3J\x1b[2J\x1b[H");
    assert!(
        terminal
            .snapshot()
            .cells
            .iter()
            .flatten()
            .all(|cell| cell.command_status.is_none())
    );
}

#[test]
fn repeated_prompt_redraws_keep_marker_metadata_bounded_and_history_statuses() {
    let (mut terminal, _writer, _replies) = terminal(2, 20);
    terminal.process("○\x1b[777;0z saved\r\n\x1b[777;1;0z".as_bytes());
    for _ in 0..2000 {
        terminal.process("\r\x1b[2K○\x1b[777;0z draft".as_bytes());
    }
    assert!(terminal.markers.len() <= 480);
    terminal.process(b"\x1b[?1049h\x1b[?1049l\x1b[777;1;1z");
    let snapshot = terminal.snapshot();
    let statuses = snapshot
        .cells
        .iter()
        .flatten()
        .filter_map(|cell| cell.command_status)
        .collect::<Vec<_>>();
    assert_eq!(statuses, vec![Some(true), Some(false)]);
}

#[test]
fn kitty_and_sixel_images_become_structured_pixels_and_do_not_enter_history() {
    let (mut terminal, _writer, _replies) = terminal(8, 20);
    let kitty = b"\x1b_Ga=T,f=32,s=2,v=1,i=1,c=2,r=1;/wAA/wD/AP8=\x1b\\";
    assert!(terminal.process(kitty).is_empty());
    let images = terminal.ui_snapshot().images;
    assert!(!images.is_empty());
    assert_eq!((images[0].width, images[0].height), (2, 1));
    assert_eq!(&*images[0].rgba, &[255, 0, 0, 255, 0, 255, 0, 255]);
    terminal.process(b"\x1b_Ga=d,d=A\x1b\\");
    assert!(terminal.ui_snapshot().images.is_empty());
    assert!(
        terminal
            .process(b"\x1bPq\"1;1;2;6#1;2;100;0;0!2~\x1b\\")
            .is_empty()
    );
    let images = terminal.ui_snapshot().images;
    assert!(!images.is_empty());
    assert!(
        images
            .iter()
            .all(|image| image.width == 2 && image.height == 6)
    );
}

#[test]
fn retained_terminal_does_not_keep_the_pty_writer_alive() {
    let (mut terminal, writer, replies) = terminal(4, 20);
    drop(writer);
    assert!(
        terminal
            .send_key(&ui::KeyEvent::new(ui::Key::Char('a')))
            .is_err()
    );
    assert!(replies.lock().unwrap().is_empty());
}

#[test]
fn direct_and_compatibility_snapshots_agree_on_blank_cells_spaces_cursor_and_wide_text() {
    let (mut terminal, _writer, _replies) = terminal(4, 20);
    for bytes in [&b""[..], "a 中 z".as_bytes(), b"\x1b[3;8H", b"\x1b[?25l"] {
        terminal.process(bytes);
        assert_eq!(
            terminal.ui_snapshot(),
            crate::snapshot::to_ui_snapshot(&terminal.snapshot())
        );
    }
}

#[test]
fn emoji_variation_selectors_and_combining_text_use_the_same_width_as_ratatui() {
    let (mut terminal, _writer, _replies) = terminal(4, 20);
    let text = "❤️e\u{301}X";
    terminal.process(text.as_bytes());
    let snapshot = terminal.snapshot();
    assert_eq!(snapshot.cells[0][0].text, "❤️");
    assert!(snapshot.cells[0][0].wide);
    assert!(snapshot.cells[0][1].wide_continuation);
    assert_eq!(snapshot.cells[0][2].text, "e\u{301}");
    assert_eq!(snapshot.cells[0][3].text, "X");
    assert_eq!(
        usize::from(snapshot.cursor_column),
        ratatui::text::Span::raw(text).width()
    );
    assert_eq!(
        terminal.ui_snapshot(),
        crate::snapshot::to_ui_snapshot(&snapshot)
    );
}

#[test]
fn iterm_inline_images_are_decoded_and_download_requests_are_not_host_operations() {
    let (mut terminal, _writer, _replies) = terminal(8, 20);
    let png = "iVBORw0KGgoAAAANSUhEUgAAAAIAAAABCAYAAAD0In+KAAAADklEQVR4nGP4z8DwHwQBEPgD/U6VwW8AAAAASUVORK5CYII=";
    let sequence =
        format!("\x1b]1337;File=inline=1;width=2;height=1;preserveAspectRatio=0:{png}\x07");
    for bytes in sequence.as_bytes().chunks(7) {
        assert!(terminal.process(bytes).is_empty());
    }
    let images = terminal.ui_snapshot().images;
    assert!(!images.is_empty());
    assert_eq!((images[0].width, images[0].height), (2, 1));
    assert_eq!(&*images[0].rgba, &[255, 0, 0, 255, 0, 255, 0, 255]);
    terminal.process(format!("\x1b]1337;File=inline=0;name=blocked:{png}\x07").as_bytes());
    assert_eq!(terminal.ui_snapshot().images, images);
}

#[test]
fn image_padding_preserves_native_pixels_and_kitty_offsets_after_coalescing() {
    let png = "iVBORw0KGgoAAAANSUhEUgAAABEAAAARCAYAAAA7bUf6AAAAG0lEQVR4nGP4z8Dwn1LMMGrIqCGjhowaQiVDAFqpP932QbKOAAAAAElFTkSuQmCC";
    for (offsets, expected_padding) in [("", [0, 0, 7, 15]), (",X=3,Y=4", [3, 4, 4, 11])] {
        let (mut terminal, _writer, _replies) = terminal(8, 20);
        terminal.process(format!("\x1b_Ga=T,f=100,i=2{offsets};{png}\x1b\\").as_bytes());
        let images = terminal.ui_snapshot().images;
        assert_eq!(images.len(), 1, "continuous cells should encode one image");
        assert_eq!(images[0].source, [0, 0, 17, 17]);
        assert_eq!(images[0].area, ratatui::layout::Rect::new(0, 0, 3, 2));
        assert_eq!(images[0].cell_size, [8, 16]);
        assert_eq!(images[0].padding, expected_padding);
    }
}

#[test]
fn unicode_graphemes_are_independent_of_pty_chunk_boundaries() {
    for (output, columns) in [
        ("e\u{301}X", 20),
        ("❤️X", 20),
        ("中\u{301}X", 20),
        ("👩\u{200d}💻X", 20),
        ("🇨🇳X", 20),
        ("👍🏽X", 20),
        ("ab❤️X", 4),
        ("abc❤️X", 4),
        ("abc中\u{301}X", 4),
        ("\x1b[4h❤️X", 20),
        ("\x1b[4habc❤️X", 4),
    ] {
        let (mut whole, _writer, _replies) = terminal(3, columns);
        whole.process(output.as_bytes());
        let expected = whole.snapshot();
        for split in 0..=output.len() {
            let (mut fragmented, _writer, _replies) = terminal(3, columns);
            fragmented.process(&output.as_bytes()[..split]);
            fragmented.process(&output.as_bytes()[split..]);
            assert_eq!(
                fragmented.snapshot(),
                expected,
                "output {output:?}, split {split}"
            );
        }
        let (mut one_byte, _writer, _replies) = terminal(3, columns);
        for byte in output.as_bytes() {
            one_byte.process(&[*byte]);
        }
        assert_eq!(one_byte.snapshot(), expected, "bytewise output {output:?}");
    }
}

#[test]
fn unicode_combining_marks_keep_the_base_cell_attributes() {
    let (mut terminal, _writer, _replies) = terminal(3, 20);
    terminal.process("\x1b[31me".as_bytes());
    terminal.process("\x1b[34m\u{301}X".as_bytes());
    let snapshot = terminal.snapshot();
    assert_eq!(snapshot.cells[0][0].text, "e\u{301}");
    assert_eq!(
        snapshot.cells[0][0].foreground,
        crate::snapshot::TerminalColor::Indexed(1)
    );
    assert_eq!(snapshot.cells[0][1].text, "X");
    assert_eq!(
        snapshot.cells[0][1].foreground,
        crate::snapshot::TerminalColor::Indexed(4)
    );
}

#[test]
fn unicode_width_expansion_wraps_cleanly_at_the_right_margin() {
    let (mut terminal, _writer, _replies) = terminal(3, 4);
    terminal.process("abc❤".as_bytes());
    terminal.process("\u{fe0f}X".as_bytes());
    let snapshot = terminal.snapshot();
    assert_eq!(snapshot.cells[0][3].text, " ");
    assert!(!snapshot.cells[0][3].wide);
    assert_eq!(snapshot.cells[1][0].text, "❤️");
    assert!(snapshot.cells[1][0].wide);
    assert!(snapshot.cells[1][1].wide_continuation);
    assert_eq!(snapshot.cells[1][2].text, "X");
    assert_eq!((snapshot.cursor_row, snapshot.cursor_column), (1, 3));
}
