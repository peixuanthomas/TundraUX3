use super::*;
use ui::{InputPhase, KeyModifiers};

#[test]
fn kitty_disambiguation_preserves_text_and_recovery_keys() {
    for (key, expected) in [
        (KeyInput::new(InputKey::Char('中')), "中"),
        (KeyInput::new(InputKey::Enter), "\r"),
        (KeyInput::new(InputKey::Tab), "\t"),
        (KeyInput::new(InputKey::Backspace), "\x7f"),
        (KeyInput::new(InputKey::Escape), "\x1b[27;1u"),
        (
            KeyInput::with_modifiers(InputKey::Char('j'), KeyModifiers::CTRL),
            "\x1b[106;5u",
        ),
        (
            KeyInput::with_modifiers(InputKey::Enter, KeyModifiers::CTRL),
            "\r",
        ),
    ] {
        assert_eq!(
            kitty_key_event_bytes(&key, 1),
            Some(expected.as_bytes().to_vec())
        );
        assert_eq!(kitty_key_event_bytes(&key.released(), 1), None);
    }
    for key in [
        InputKey::Enter,
        InputKey::Tab,
        InputKey::Backspace,
        InputKey::Char('a'),
    ] {
        assert_eq!(
            kitty_key_event_bytes(&KeyInput::new(key).released(), 3),
            None
        );
    }
    assert_eq!(
        kitty_key_event_bytes(&KeyInput::new(InputKey::Escape), 0),
        Some(vec![27])
    );
}

#[test]
fn kitty_reports_repeat_and_release_only_when_requested() {
    let key = KeyInput::new(InputKey::Char('a'));
    for (key, expected) in [
        (key.clone(), "\x1b[97;1u"),
        (key.clone().repeated(), "\x1b[97;1:2u"),
        (key.clone().released(), "\x1b[97;1:3u"),
        (KeyInput::new(InputKey::Up).released(), "\x1b[1;1:3A"),
        (KeyInput::new(InputKey::F(3)).repeated(), "\x1b[13;1:2~"),
        (KeyInput::new(InputKey::F(35)), "\x1b[57398;1u"),
    ] {
        assert_eq!(
            kitty_key_event_bytes(&key, 11),
            Some(expected.as_bytes().to_vec())
        );
    }
    assert_eq!(
        kitty_key_event_bytes(&key.clone().repeated(), 9),
        Some(b"\x1b[97;1u".to_vec())
    );
    assert_eq!(kitty_key_event_bytes(&key.released(), 9), None);
    assert_eq!(
        key_event_bytes(&KeyInput::new(InputKey::Char('a')).released(), false),
        None
    );
}

#[test]
fn kitty_keeps_keypad_locks_and_extended_modifiers() {
    let key = KeyInput::with_modifiers(
        InputKey::Char('7'),
        KeyModifiers {
            shift: true,
            alt: true,
            super_key: true,
            hyper: true,
            meta: true,
            ..KeyModifiers::CTRL
        },
    )
    .with_state(ui::KeyEventState {
        keypad: true,
        caps_lock: true,
        num_lock: true,
    });
    assert_eq!(
        kitty_key_event_bytes(&key.released(), 11),
        Some(b"\x1b[57406;256:3u".to_vec())
    );
    let enter = KeyInput::new(InputKey::Enter).with_state(ui::KeyEventState {
        keypad: true,
        ..Default::default()
    });
    assert_eq!(
        kitty_key_event_bytes(&enter.released(), 3),
        Some(b"\x1b[57414;1:3u".to_vec())
    );
    assert_eq!(
        kitty_key_event_bytes(&KeyInput::new(InputKey::BackTab), 11),
        Some(b"\x1b[9;2u".to_vec())
    );
}

#[test]
fn kitty_functional_keys_keep_their_distinct_identities() {
    for (key, expected) in [
        (InputKey::CapsLock, "\x1b[57358;1u"),
        (
            InputKey::Media(ui::MediaKey::TrackPrevious),
            "\x1b[57436;1u",
        ),
        (
            InputKey::Modifier(ui::ModifierKey::RightHyper),
            "\x1b[57451;1u",
        ),
        (
            InputKey::Modifier(ui::ModifierKey::IsoLevel5Shift),
            "\x1b[57454;1u",
        ),
    ] {
        assert_eq!(
            kitty_key_event_bytes(&KeyInput::new(key), 11),
            Some(expected.as_bytes().to_vec())
        );
    }
    assert_eq!(
        kitty_key_event_bytes(
            &KeyInput::new(InputKey::Modifier(ui::ModifierKey::LeftShift)),
            3
        ),
        None
    );
}

#[test]
fn kitty_uses_unshifted_ascii_letter_codes_and_preserves_modifiers() {
    let control_shift = KeyInput::with_modifiers(InputKey::Char('A'), KeyModifiers::CTRL_SHIFT);
    assert_eq!(
        kitty_key_event_bytes(&control_shift, 1),
        Some(b"\x1b[97;6u".to_vec())
    );
    assert_eq!(key_event_bytes(&control_shift, false), Some(vec![1]));
    let shift = KeyInput::with_modifiers(InputKey::Char('A'), KeyModifiers::SHIFT);
    assert_eq!(
        kitty_key_event_bytes(&shift, 11),
        Some(b"\x1b[97;2u".to_vec())
    );
    assert_eq!(key_event_bytes(&shift, false), Some(b"A".to_vec()));
    let caps = KeyInput::new(InputKey::Char('A')).with_state(ui::KeyEventState {
        caps_lock: true,
        ..Default::default()
    });
    assert_eq!(
        kitty_key_event_bytes(&caps, 11),
        Some(b"\x1b[97;65u".to_vec())
    );
    assert_eq!(
        kitty_key_event_bytes(&KeyInput::new(InputKey::Char('A')), 11),
        Some(b"\x1b[65;1u".to_vec())
    );
}

#[test]
fn mouse_coordinates_follow_the_actual_viewport_after_keyboard_and_window_resizes() {
    let event = MouseEvent::drag(14, 9, MouseButton::Left).with_modifiers(KeyModifiers::CTRL);
    for (area, expected) in [
        (Rect::new(10, 5, 40, 20), Some((4, 4))),
        (Rect::new(12, 8, 20, 8), Some((2, 1))),
        (Rect::new(10, 5, 40, 4), None),
        (Rect::new(15, 5, 20, 20), None),
        (Rect::new(10, 5, 0, 20), None),
    ] {
        let translated = mouse_to_terminal(&event, area);
        assert_eq!(translated.map(|mouse| mouse.coordinates()), expected);
        if let Some(mouse) = translated {
            assert_eq!(mouse.kind, event.kind);
            assert_eq!(mouse.modifiers, event.modifiers);
        }
    }
}

#[test]
fn vim_control_keys_and_function_keys_keep_their_terminal_bytes() {
    for key in [InputKey::Space, InputKey::Char(' ')] {
        assert_eq!(
            key_event_bytes(&KeyInput::with_modifiers(key, KeyModifiers::CTRL), false),
            Some(vec![0])
        );
    }
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
