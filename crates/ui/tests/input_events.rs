use ui::{InputEvent, InputPhase, Key, KeyEvent, KeyModifiers, KeyStroke};

#[test]
fn key_events_preserve_all_phases() {
    let modifiers = KeyModifiers::new(true, true, true);
    let press = InputEvent::key_with_phase(Key::F(12), modifiers, InputPhase::Press);
    let repeat = InputEvent::key_with_phase(Key::F(12), modifiers, InputPhase::Repeat);
    let release = KeyEvent::with_phase(Key::F(12), modifiers, InputPhase::Release);

    assert!(matches!(
        press,
        InputEvent::Key(KeyEvent {
            phase: InputPhase::Press,
            ..
        })
    ));
    assert!(matches!(
        repeat,
        InputEvent::Key(KeyEvent {
            phase: InputPhase::Repeat,
            ..
        })
    ));
    assert!(!release.is_press_like());
    assert_eq!(
        KeyEvent::new(Key::Enter).repeated().phase,
        InputPhase::Repeat
    );
    assert_eq!(
        KeyEvent::new(Key::Enter).released().phase,
        InputPhase::Release
    );
}

#[test]
fn key_event_labels_preserve_shell_compatibility() {
    assert_eq!(KeyEvent::from_label("Ctrl+C").label(), "Ctrl+C");
    assert_eq!(KeyEvent::from_label("Shift+Tab").label(), "Shift+Tab");
    assert_eq!(KeyEvent::from_label("F5").label(), "F(5)");
    assert_eq!(KeyStroke::plain(Key::F(5)).label(), "F5");

    let modified_character = KeyEvent::with_modifiers(
        Key::Char('c'),
        KeyModifiers {
            control: true,
            alt: true,
            ..KeyModifiers::NONE
        },
    );
    assert_eq!(modified_character.label(), "Ctrl+Alt+c");
}

#[test]
fn modifiers_include_control_and_platform_modifier_keys() {
    let modifiers = KeyModifiers {
        shift: true,
        control: true,
        ctrl: false,
        alt: true,
        super_key: true,
        hyper: true,
        meta: true,
    };

    assert!(modifiers.is_control());
    assert!(modifiers.has_non_shift_modifier());
    assert_eq!(
        KeyStroke::new(Key::Char('k'), modifiers).label(),
        "Ctrl+Alt+Super+Hyper+Meta+Shift+k"
    );
}
