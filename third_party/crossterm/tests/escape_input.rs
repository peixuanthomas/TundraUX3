use super::*;
use crate::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};

fn key(code: KeyCode) -> InternalEvent {
    InternalEvent::Event(Event::Key(code.into()))
}

fn mouse(kind: MouseEventKind) -> InternalEvent {
    InternalEvent::Event(Event::Mouse(MouseEvent {
        kind,
        column: 44,
        row: 11,
        modifiers: KeyModifiers::NONE,
    }))
}

#[test]
fn escape_preserves_the_following_terminal_report() {
    let cases: Vec<(&[u8], InternalEvent)> = vec![
        (b"\x1b[<35;45;12M", mouse(MouseEventKind::Moved)),
        (
            b"\x1b[<0;45;12M",
            mouse(MouseEventKind::Down(MouseButton::Left)),
        ),
        (
            b"\x1b[<0;45;12m",
            mouse(MouseEventKind::Up(MouseButton::Left)),
        ),
        (
            b"\x1b[<32;45;12M",
            mouse(MouseEventKind::Drag(MouseButton::Left)),
        ),
        (b"\x1b[<64;45;12M", mouse(MouseEventKind::ScrollUp)),
        (b"\x1b[MCM,", mouse(MouseEventKind::Moved)),
        (b"\x1b[67;45;12M", mouse(MouseEventKind::Moved)),
        (b"\x1b[A", key(KeyCode::Up)),
        (b"\x1bOP", key(KeyCode::F(1))),
        (b"\x1b[[A", key(KeyCode::F(1))),
        (b"\x1b[I", InternalEvent::Event(Event::FocusGained)),
        (b"\x1b[O", InternalEvent::Event(Event::FocusLost)),
    ];
    for (report, expected) in cases {
        let mut input = vec![0x1b];
        input.extend_from_slice(report);
        input.push(b'm'); // A real shortcut after the report must still arrive.
                          // Exercise every read boundary while input remains available, including
                          // a second Escape at the very end of a full TTY read buffer.
        for split in 0..=input.len() {
            let mut parser = Parser::default();
            parser.advance(&input[..split], split < input.len());
            parser.advance(&input[split..], false);
            assert_eq!(
                parser.collect::<Vec<_>>(),
                vec![key(KeyCode::Esc), expected.clone(), key(KeyCode::Char('m'))],
                "report {report:?}, split {split}"
            );
        }
    }
}

#[test]
fn ordinary_escape_alt_and_repeated_escape_inputs_are_preserved() {
    for (input, expected) in [
        (&b"\x1b"[..], vec![key(KeyCode::Esc)]),
        (b"\x1b\x1b", vec![key(KeyCode::Esc), key(KeyCode::Esc)]),
        (
            b"\x1b\x1b\x1b",
            vec![key(KeyCode::Esc), key(KeyCode::Esc), key(KeyCode::Esc)],
        ),
        (
            b"\x1bm",
            vec![InternalEvent::Event(Event::Key(KeyEvent::new(
                KeyCode::Char('m'),
                KeyModifiers::ALT,
            )))],
        ),
        (b"m", vec![key(KeyCode::Char('m'))]),
    ] {
        let mut parser = Parser::default();
        parser.advance(input, false);
        parser.expire_at(Instant::now() + ESCAPE_TIMEOUT);
        assert_eq!(parser.collect::<Vec<_>>(), expected, "input {input:?}");
    }
}

#[cfg(feature = "bracketed-paste")]
#[test]
fn escape_before_paste_does_not_interpret_pasted_escapes_as_keys() {
    let mut parser = Parser::default();
    parser.advance(b"\x1b\x1b[200~text\x1b\x1b[<35;45;12M\x1b[201~", false);
    assert_eq!(
        parser.collect::<Vec<_>>(),
        vec![
            key(KeyCode::Esc),
            InternalEvent::Event(Event::Paste("text\x1b\x1b[<35;45;12M".into())),
        ]
    );
}

#[test]
fn partial_reports_wait_for_completion_or_discard_their_late_tail() {
    let start = Instant::now();
    let mut parser = Parser::default();
    parser.advance_at(b"\x1b[<35;45;", false, start);
    assert!(parser.internal_events.is_empty());
    parser.advance_at(b"12M", false, start + REPORT_TIMEOUT / 2);
    assert_eq!(
        parser.collect::<Vec<_>>(),
        vec![mouse(MouseEventKind::Moved)]
    );

    for (prefix, tail) in [
        (&b"\x1b[<35;45;"[..], &b"12M"[..]),
        (b"\x1b[1;", b"2A"),
        (b"\x1bO", b"P"),
        (b"\x1b[MCM", b","),
        (&[0xe2, 0x82], &[0xac]),
    ] {
        let mut parser = Parser::default();
        parser.advance_at(prefix, false, start);
        assert!(parser.poll_timeout(None).is_some());
        parser.expire_at(start + REPORT_TIMEOUT);
        assert!(parser.buffer.is_empty());
        assert!(parser.internal_events.is_empty());
        parser.advance_at(tail, false, start + REPORT_TIMEOUT);
        parser.advance_at(b"m", false, start + REPORT_TIMEOUT);
        assert_eq!(parser.collect::<Vec<_>>(), vec![key(KeyCode::Char('m'))]);
    }
}

#[test]
fn escape_prefix_at_a_short_read_boundary_waits_without_becoming_a_key() {
    let start = Instant::now();
    let mut parser = Parser::default();
    parser.advance_at(b"\x1b", false, start);
    assert!(parser.internal_events.is_empty());
    parser.advance_at(b"[<35;45;12M", false, start + ESCAPE_TIMEOUT / 2);
    assert_eq!(
        parser.collect::<Vec<_>>(),
        vec![mouse(MouseEventKind::Moved)]
    );

    let mut parser = Parser::default();
    parser.advance_at(b"\x1b", false, start);
    parser.expire_at(start + ESCAPE_TIMEOUT);
    assert_eq!(parser.collect::<Vec<_>>(), vec![key(KeyCode::Esc)]);
}

#[test]
fn malformed_mouse_reports_are_dropped_whole_without_panics_or_shortcuts() {
    for report in [
        &b"\x1b[<35;0;12M"[..],
        b"\x1b[<35;45;0M",
        b"\x1b[<35;65536;12M",
        b"\x1b[<35;45;12;1M",
        b"\x1b[<35;45;12;;M",
        b"\x1b[<35;broken;12M",
        b"\x1b[<255;45;12M",
        b"\x1b[67;0;12M",
        b"\x1b[MCM\x00",
    ] {
        let mut parser = Parser::default();
        parser.advance(report, false);
        assert!(parser.internal_events.is_empty(), "report {report:?}");
        parser.advance(b"m", false);
        assert_eq!(parser.collect::<Vec<_>>(), vec![key(KeyCode::Char('m'))]);
    }
}

#[test]
fn interrupted_or_oversized_reports_resynchronize_without_leaking_the_payload() {
    let mut parser = Parser::default();
    parser.advance(b"\x1b[<35;45;\x1b[A", false);
    assert_eq!(parser.collect::<Vec<_>>(), vec![key(KeyCode::Up)]);

    let mut parser = Parser::default();
    parser.advance(b"\x1b[<", false);
    parser.advance(&vec![b'1'; REPORT_LIMIT * 2], false);
    assert!(parser.buffer.len() <= REPORT_LIMIT);
    parser.advance(b";45;12Mm", false);
    assert_eq!(parser.collect::<Vec<_>>(), vec![key(KeyCode::Char('m'))]);

    let mut parser = Parser::default();
    parser.advance(b"\x1b[<35;45;", false);
    parser.expire_at(Instant::now() + REPORT_TIMEOUT);
    parser.advance(b"\x1b[A", false);
    assert_eq!(parser.collect::<Vec<_>>(), vec![key(KeyCode::Up)]);
}

#[cfg(feature = "bracketed-paste")]
#[test]
fn timed_out_or_oversized_paste_is_never_replayed_as_shortcuts() {
    for oversized in [false, true] {
        let mut parser = Parser::default();
        parser.advance(b"\x1b[200~", false);
        if oversized {
            parser.advance(&vec![b'm'; PASTE_LIMIT], false);
        } else {
            parser.expire_at(Instant::now() + PASTE_TIMEOUT);
        }
        assert!(parser.buffer.is_empty());
        parser.advance(b"m\x1b\x1b[<35;45;12M\x1b[20", false);
        parser.advance(b"1~e", false);
        assert_eq!(parser.collect::<Vec<_>>(), vec![key(KeyCode::Char('e'))]);
    }
}

#[test]
fn unsupported_control_strings_are_consumed_whole() {
    for report in [
        &b"\x1b]52;c;secret\x07"[..],
        b"\x1bPsecret\x1b\\",
        b"\x1b_Gsecret\x1b\\",
    ] {
        let mut parser = Parser::default();
        parser.advance(report, false);
        assert!(parser.internal_events.is_empty());
        parser.advance(b"m", false);
        assert_eq!(parser.collect::<Vec<_>>(), vec![key(KeyCode::Char('m'))]);
    }
}

#[test]
fn discarded_reports_emit_a_warning_callback_without_input_bytes() {
    use std::cell::RefCell;
    thread_local! {
        static ERRORS: RefCell<Vec<InputError>> = const { RefCell::new(Vec::new()) };
    }
    crate::event::set_input_error_handler(|error| {
        ERRORS.with(|errors| errors.borrow_mut().push(error))
    });
    ERRORS.with(|errors| errors.borrow_mut().clear());
    let mut parser = Parser::default();
    parser.advance(b"\x1b[<35;0;12M", false);
    parser.advance(b"\x1b[<35;", false);
    parser.expire_at(Instant::now() + REPORT_TIMEOUT);
    parser.advance(b"\x1b[<", false);
    parser.advance(&vec![b'1'; REPORT_LIMIT], false);
    ERRORS.with(|errors| {
        let errors = errors.borrow();
        assert_eq!(
            errors.iter().map(|error| error.kind).collect::<Vec<_>>(),
            vec![
                InputErrorKind::Malformed,
                InputErrorKind::Incomplete,
                InputErrorKind::TooLong
            ]
        );
        assert!(errors.iter().all(|error| error.protocol == "sgr-mouse"));
        assert!(errors
            .iter()
            .all(|error| error.buffered_bytes <= REPORT_LIMIT + 1));
    });
}

#[test]
fn unix_reader_preserves_nonblocking_poll_and_expires_incomplete_reports() {
    use crate::event::source::{unix::UnixInternalEventSource, EventSource};
    use crate::terminal::sys::file_descriptor::FileDesc;
    use std::{io::Write, os::unix::net::UnixStream};

    let (input, mut writer) = UnixStream::pair().unwrap();
    // Match a real raw terminal: the descriptor itself may be blocking.
    #[cfg(not(feature = "libc"))]
    let fd = FileDesc::Owned(input.into());
    #[cfg(feature = "libc")]
    let fd = {
        use std::os::fd::IntoRawFd;
        FileDesc::new(input.into_raw_fd(), true)
    };
    let mut source = UnixInternalEventSource::from_file_descriptor(fd).unwrap();
    let immediate = Some(Duration::ZERO);
    writer.write_all(b"\x1b\x1b[<35;45;12Mm").unwrap();
    for expected in [
        key(KeyCode::Esc),
        mouse(MouseEventKind::Moved),
        key(KeyCode::Char('m')),
    ] {
        assert_eq!(source.try_read(immediate).unwrap(), Some(expected));
    }
    assert_eq!(source.try_read(immediate).unwrap(), None);

    writer.write_all(b"\x1b[<35;45;").unwrap();
    assert_eq!(source.try_read(Some(REPORT_TIMEOUT * 2)).unwrap(), None);
    writer.write_all(b"12Mm").unwrap();
    assert_eq!(
        source.try_read(immediate).unwrap(),
        Some(key(KeyCode::Char('m')))
    );
    assert_eq!(source.try_read(immediate).unwrap(), None);

    // One nonblocking call must yield even with a ready backlog that cannot
    // produce an event. Further calls recover and reach the next valid key.
    writer.write_all(&vec![0xff; 4096]).unwrap();
    writer.write_all(b"m").unwrap();
    assert_eq!(source.try_read(immediate).unwrap(), None);
    let mut recovered = None;
    for _ in 0..5 {
        recovered = source.try_read(immediate).unwrap();
        if recovered.is_some() {
            break;
        }
    }
    assert_eq!(recovered, Some(key(KeyCode::Char('m'))));

    writer.write_all(b"\x1b").unwrap();
    assert_eq!(
        source.try_read(Some(Duration::from_secs(1))).unwrap(),
        Some(key(KeyCode::Esc))
    );
}
