use vt100::{CursorStyle, Parser};

#[test]
fn decscusr_preserves_all_requested_styles_and_resets() {
    let mut parser = Parser::new(4, 20, 0);
    for (parameter, expected) in [
        (0, CursorStyle::Default),
        (1, CursorStyle::BlinkingBlock),
        (2, CursorStyle::SteadyBlock),
        (3, CursorStyle::BlinkingUnderline),
        (4, CursorStyle::SteadyUnderline),
        (5, CursorStyle::BlinkingBar),
        (6, CursorStyle::SteadyBar),
    ] {
        let sequence = format!("\x1b[{parameter} q");
        for byte in sequence.bytes() {
            parser.process(&[byte]);
        }
        assert_eq!(parser.screen().cursor_style(), expected);
    }

    parser.process(b"\x1b[ q");
    assert_eq!(parser.screen().cursor_style(), CursorStyle::Default);
    parser.process(b"\x1b[5 q\x1bc");
    assert_eq!(parser.screen().cursor_style(), CursorStyle::Default);
}

#[test]
fn unrelated_or_invalid_q_sequences_leave_the_cursor_unchanged() {
    let mut parser = Parser::new(4, 20, 0);
    parser.process(b"\x1b[5 q");
    for sequence in [
        &b"\x1b[2q"[..],
        &b"\x1b[?2q"[..],
        &b"\x1b[7 q"[..],
        &b"\x1b[1;2 q"[..],
        &b"\x1b[1:2 q"[..],
        &b"\x1b[2  q"[..],
    ] {
        parser.process(sequence);
        assert_eq!(parser.screen().cursor_style(), CursorStyle::BlinkingBar);
    }
}

#[test]
fn full_diff_and_cursor_serialization_restore_cursor_appearance() {
    let mut source = Parser::new(4, 20, 0);
    source.process(b"text\x1b[3;7H\x1b[?25l\x1b[6 q");
    let previous = source.screen().clone();
    let mut target = Parser::new(4, 20, 0);
    target.process(&source.screen().state_formatted());
    assert_eq!(target.screen().cursor_style(), CursorStyle::SteadyBar);
    assert_eq!(target.screen().cursor_position(), (2, 6));
    assert!(target.screen().hide_cursor());
    assert_eq!(target.screen().contents(), "text");

    source.process(b"\x1b[0 q");
    target.process(&source.screen().state_diff(&previous));
    assert_eq!(target.screen().cursor_style(), CursorStyle::Default);
    assert_eq!(target.screen().cursor_position(), (2, 6));
    assert!(target.screen().hide_cursor());

    target.process(b"\x1b[5 q");
    target.process(&source.screen().contents_formatted());
    assert_eq!(target.screen().cursor_style(), CursorStyle::Default);
    target.process(b"\x1b[5 q\x1b[H\x1b[?25h");
    target.process(&source.screen().cursor_state_formatted());
    assert_eq!(target.screen().cursor_style(), CursorStyle::Default);
    assert_eq!(target.screen().cursor_position(), (2, 6));
    assert!(target.screen().hide_cursor());
    assert!(source.screen().state_diff(source.screen()).is_empty());
}
