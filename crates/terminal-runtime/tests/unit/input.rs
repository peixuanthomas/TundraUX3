use super::*;
use ui::{InputPhase, KeyModifiers};

#[test]
fn vim_control_keys_and_function_keys_keep_their_terminal_bytes() {
    for (character, expected) in [
        ('w', 0x17),
        ('n', 0x0e),
        ('p', 0x10),
        ('r', 0x12),
        ('v', 0x16),
    ] {
        let key = KeyInput::with_modifiers(InputKey::Char(character), KeyModifiers::CTRL);
        assert_eq!(key_event_bytes(&key, false), Some(vec![expected]));
        assert_eq!(
            key_event_bytes(&key.clone().repeated(), false),
            Some(vec![expected])
        );
        assert_eq!(key_event_bytes(&key.released(), false), None);
    }
    for (number, expected) in [
        (1, &b"\x1bOP"[..]),
        (4, &b"\x1bOS"[..]),
        (5, &b"\x1b[15~"[..]),
        (12, &b"\x1b[24~"[..]),
    ] {
        assert_eq!(
            key_event_bytes(&KeyInput::new(InputKey::F(number)), false),
            Some(expected.to_vec())
        );
    }
    assert_eq!(
        key_event_bytes(
            &KeyInput::with_phase(InputKey::Char('i'), KeyModifiers::NONE, InputPhase::Release),
            false
        ),
        None
    );
}

#[test]
fn application_cursor_and_modified_navigation_are_distinct() {
    for (key, normal, application) in [
        (InputKey::Home, &b"\x1b[H"[..], &b"\x1bOH"[..]),
        (InputKey::End, &b"\x1b[F"[..], &b"\x1bOF"[..]),
    ] {
        assert_eq!(
            key_event_bytes(&KeyInput::new(key.clone()), false),
            Some(normal.to_vec())
        );
        assert_eq!(
            key_event_bytes(&KeyInput::new(key), true),
            Some(application.to_vec())
        );
    }
    for (key, modifiers, expected) in [
        (InputKey::Left, KeyModifiers::CTRL, &b"\x1b[1;5D"[..]),
        (InputKey::Up, KeyModifiers::SHIFT, &b"\x1b[1;2A"[..]),
        (InputKey::F(1), KeyModifiers::SHIFT, &b"\x1b[1;2P"[..]),
        (InputKey::F(12), KeyModifiers::CTRL, &b"\x1b[24;5~"[..]),
        (InputKey::PageDown, KeyModifiers::ALT, &b"\x1b[6;3~"[..]),
    ] {
        assert_eq!(
            key_event_bytes(&KeyInput::with_modifiers(key, modifiers), true),
            Some(expected.to_vec())
        );
    }
}

#[test]
fn sgr_mouse_reports_use_child_coordinates_buttons_and_modifiers() {
    let area = Rect::new(10, 5, 100, 30);
    for (event, expected) in [
        (MouseEvent::down(12, 8, MouseButton::Left), "\x1b[<0;3;4M"),
        (MouseEvent::up(12, 8, MouseButton::Left), "\x1b[<0;3;4m"),
        (MouseEvent::down(12, 8, MouseButton::Right), "\x1b[<2;3;4M"),
        (MouseEvent::drag(12, 8, MouseButton::Left), "\x1b[<32;3;4M"),
        (MouseEvent::moved(12, 8), "\x1b[<35;3;4M"),
        (
            MouseEvent::new(12, 8, MouseEventKind::Scroll(ScrollDirection::Up)),
            "\x1b[<64;3;4M",
        ),
        (
            MouseEvent::new(12, 8, MouseEventKind::Scroll(ScrollDirection::Down)),
            "\x1b[<65;3;4M",
        ),
        (
            MouseEvent::down(12, 8, MouseButton::Middle).with_modifiers(KeyModifiers {
                shift: true,
                alt: true,
                ..KeyModifiers::CTRL
            }),
            "\x1b[<29;3;4M",
        ),
    ] {
        assert_eq!(
            mouse_event_bytes(
                &event,
                area,
                MouseProtocolMode::AnyMotion,
                MouseProtocolEncoding::Sgr
            ),
            Some(expected.as_bytes().to_vec())
        );
    }
}

#[test]
fn child_tracking_modes_control_which_mouse_events_are_reported() {
    let area = Rect::new(0, 0, 10, 10);
    for (mode, event, expected) in [
        (
            MouseProtocolMode::None,
            MouseEventKind::Down(MouseButton::Left),
            false,
        ),
        (
            MouseProtocolMode::Press,
            MouseEventKind::Down(MouseButton::Left),
            true,
        ),
        (
            MouseProtocolMode::Press,
            MouseEventKind::Up(MouseButton::Left),
            false,
        ),
        (
            MouseProtocolMode::PressRelease,
            MouseEventKind::Up(MouseButton::Left),
            true,
        ),
        (
            MouseProtocolMode::PressRelease,
            MouseEventKind::Drag(MouseButton::Left),
            false,
        ),
        (
            MouseProtocolMode::ButtonMotion,
            MouseEventKind::Drag(MouseButton::Left),
            true,
        ),
        (
            MouseProtocolMode::ButtonMotion,
            MouseEventKind::Moved,
            false,
        ),
        (MouseProtocolMode::AnyMotion, MouseEventKind::Moved, true),
        (
            MouseProtocolMode::AnyMotion,
            MouseEventKind::Click(MouseButton::Left),
            false,
        ),
        (
            MouseProtocolMode::AnyMotion,
            MouseEventKind::DoubleClick(MouseButton::Left),
            false,
        ),
    ] {
        assert_eq!(
            mouse_event_bytes(
                &MouseEvent::new(1, 1, event),
                area,
                mode,
                MouseProtocolEncoding::Sgr
            )
            .is_some(),
            expected
        );
    }
    for event in [
        MouseEvent::down(9, 5, MouseButton::Left),
        MouseEvent::down(20, 5, MouseButton::Left),
        MouseEvent::down(10, 10, MouseButton::Left),
    ] {
        assert!(
            mouse_event_bytes(
                &event,
                Rect::new(10, 5, 10, 5),
                MouseProtocolMode::AnyMotion,
                MouseProtocolEncoding::Sgr
            )
            .is_none()
        );
    }
}

#[test]
fn legacy_mouse_coordinates_use_the_requested_encoding_without_overflow() {
    let area = Rect::new(0, 0, 3000, 3000);
    assert_eq!(
        mouse_event_bytes(
            &MouseEvent::down(0, 0, MouseButton::Left),
            area,
            MouseProtocolMode::PressRelease,
            MouseProtocolEncoding::Default
        ),
        Some(b"\x1b[M !!".to_vec())
    );
    assert_eq!(
        mouse_event_bytes(
            &MouseEvent::up(0, 0, MouseButton::Right),
            area,
            MouseProtocolMode::PressRelease,
            MouseProtocolEncoding::Default
        ),
        Some(b"\x1b[M#!!".to_vec())
    );
    assert!(
        mouse_event_bytes(
            &MouseEvent::down(223, 0, MouseButton::Left),
            area,
            MouseProtocolMode::PressRelease,
            MouseProtocolEncoding::Default
        )
        .is_none()
    );
    let utf8 = mouse_event_bytes(
        &MouseEvent::down(223, 0, MouseButton::Left),
        area,
        MouseProtocolMode::PressRelease,
        MouseProtocolEncoding::Utf8,
    )
    .unwrap();
    assert_eq!(utf8, "\x1b[M Ā!".as_bytes());
    assert!(
        mouse_event_bytes(
            &MouseEvent::down(2015, 0, MouseButton::Left),
            area,
            MouseProtocolMode::PressRelease,
            MouseProtocolEncoding::Utf8
        )
        .is_none()
    );
}
