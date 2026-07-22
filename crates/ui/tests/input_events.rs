use ui::{
    InputEvent, InputPhase, Key, KeyEvent, KeyModifiers, KeyStroke, MouseButton, MouseEvent,
    MouseEventKind, ScrollDirection,
};

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

#[test]
fn text_focus_and_shutdown_events_are_first_class() {
    assert_eq!(
        InputEvent::paste("clipboard"),
        InputEvent::Paste("clipboard".into())
    );
    assert!(matches!(InputEvent::FocusGained, InputEvent::FocusGained));
    assert!(matches!(InputEvent::FocusLost, InputEvent::FocusLost));
    assert!(matches!(InputEvent::Shutdown, InputEvent::Shutdown));
}

#[test]
fn mouse_events_retain_button_coordinates_modifiers_drag_and_scroll() {
    let modifiers = KeyModifiers::SHIFT;
    let down = MouseEvent::down(3, 4, MouseButton::Left).with_modifiers(modifiers);
    let up = MouseEvent::up(3, 4, MouseButton::Left);
    let moved = MouseEvent::moved(4, 5);
    let drag = MouseEvent::drag(8, 9, MouseButton::Middle).with_modifiers(modifiers);
    let scroll = MouseEvent::scroll(10, 11, ScrollDirection::Left);

    assert_eq!((down.column(), down.row()), (3, 4));
    assert_eq!(down.kind, MouseEventKind::Down(MouseButton::Left));
    assert!(down.is_primary_down());
    assert!(up.is_primary_up());
    assert_eq!(moved.kind, MouseEventKind::Moved);
    assert_eq!(drag.kind, MouseEventKind::Drag(MouseButton::Middle));
    assert_eq!(drag.modifiers, modifiers);
    assert_eq!(scroll.kind, MouseEventKind::Scroll(ScrollDirection::Left));
}
