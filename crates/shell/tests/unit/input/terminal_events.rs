use super::*;

#[test]
fn enhanced_keys_keep_phase_modifiers_and_keyboard_state() {
    let modifiers = CrosstermKeyModifiers::SHIFT
        | CrosstermKeyModifiers::CONTROL
        | CrosstermKeyModifiers::ALT
        | CrosstermKeyModifiers::SUPER
        | CrosstermKeyModifiers::HYPER
        | CrosstermKeyModifiers::META;
    let state =
        CrosstermKeyState::KEYPAD | CrosstermKeyState::CAPS_LOCK | CrosstermKeyState::NUM_LOCK;
    for (kind, phase) in [
        (KeyEventKind::Press, InputPhase::Press),
        (KeyEventKind::Repeat, InputPhase::Repeat),
        (KeyEventKind::Release, InputPhase::Release),
    ] {
        let input =
            crossterm_event_to_input(Event::Key(CrosstermKeyEvent::new_with_kind_and_state(
                CrosstermKeyCode::Char('7'),
                modifiers,
                kind,
                state,
            )));
        let InputEvent::Key(key) = input else {
            panic!("key event required")
        };
        assert_eq!(key.key, InputKey::Char('7'));
        assert_eq!(key.phase, phase);
        assert!(key.modifiers.shift && key.modifiers.is_control() && key.modifiers.alt);
        assert!(key.modifiers.super_key && key.modifiers.hyper && key.modifiers.meta);
        assert_eq!(
            key.state,
            KeyEventState {
                keypad: true,
                caps_lock: true,
                num_lock: true
            }
        );
    }
}

#[test]
fn enhanced_functional_keys_are_not_reduced_to_debug_strings() {
    for (source, expected) in [
        (CrosstermKeyCode::Null, InputKey::Null),
        (CrosstermKeyCode::CapsLock, InputKey::CapsLock),
        (CrosstermKeyCode::ScrollLock, InputKey::ScrollLock),
        (CrosstermKeyCode::NumLock, InputKey::NumLock),
        (CrosstermKeyCode::PrintScreen, InputKey::PrintScreen),
        (CrosstermKeyCode::Pause, InputKey::Pause),
        (CrosstermKeyCode::Menu, InputKey::Menu),
        (CrosstermKeyCode::KeypadBegin, InputKey::KeypadBegin),
        (CrosstermKeyCode::F(35), InputKey::F(35)),
        (
            CrosstermKeyCode::Media(MediaKeyCode::MuteVolume),
            InputKey::Media(MediaKey::MuteVolume),
        ),
        (
            CrosstermKeyCode::Media(MediaKeyCode::Reverse),
            InputKey::Media(MediaKey::Reverse),
        ),
        (
            CrosstermKeyCode::Modifier(ModifierKeyCode::RightMeta),
            InputKey::Modifier(ModifierKey::RightMeta),
        ),
        (
            CrosstermKeyCode::Modifier(ModifierKeyCode::IsoLevel5Shift),
            InputKey::Modifier(ModifierKey::IsoLevel5Shift),
        ),
    ] {
        let input = crossterm_event_to_input(Event::Key(CrosstermKeyEvent::new_with_kind(
            source,
            CrosstermKeyModifiers::NONE,
            KeyEventKind::Release,
        )));
        assert_eq!(input, InputEvent::Key(KeyInput::new(expected).released()));
    }
}

#[test]
fn terminal_focus_and_paste_events_stay_distinct() {
    assert_eq!(
        crossterm_event_to_input(Event::FocusGained),
        InputEvent::FocusGained
    );
    assert_eq!(
        crossterm_event_to_input(Event::FocusLost),
        InputEvent::FocusLost
    );
    assert_eq!(
        crossterm_event_to_input(Event::Paste("中\n文".into())),
        InputEvent::Paste("中\n文".into())
    );
}
