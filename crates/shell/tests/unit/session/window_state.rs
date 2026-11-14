use super::*;
use crate::InputModifiers;

#[test]
fn opening_replaces_focused_slot_and_background_stays_explicit() {
    let mut windows = WindowSelection::default();
    let [a, b, c] = [SessionId(1), SessionId(2), SessionId(3)];
    windows.open(a);
    windows.show(b);
    windows.switch(a);
    windows.open(c);
    assert_eq!(windows.visible(2), [c, b]);
    assert_eq!(windows.focused(), Some(c));
    assert!(!windows.includes(a));

    windows.minimize(c);
    assert_eq!(windows.visible(2), [b]);
    assert_eq!(windows.focused(), Some(b));
    assert_eq!(windows.visible(4), [b]);
}

#[test]
fn taskbar_restores_minimized_windows_in_their_original_order_when_they_fit() {
    let mut windows = WindowSelection::default();
    let [a, b, c] = [SessionId(1), SessionId(2), SessionId(3)];
    windows.open(a);
    windows.show(b);
    windows.show(c);
    windows.minimize(a);
    windows.minimize(b);
    assert_eq!(windows.visible(3), [c]);
    windows.activate(a, 3);
    windows.activate(b, 3);
    assert_eq!(windows.visible(3), [a, b, c]);
    assert_eq!(windows.focused(), Some(b));
}

#[test]
fn taskbar_with_a_full_split_replaces_only_the_focused_slot() {
    let mut windows = WindowSelection::default();
    let [a, b, c] = [SessionId(1), SessionId(2), SessionId(3)];
    windows.open(a);
    windows.show(b);
    windows.minimize(a);
    windows.show(c);
    windows.activate(a, 2);
    assert_eq!(windows.visible(2), [b, a]);
    assert_eq!(windows.focused(), Some(a));
    assert!(!windows.includes(c));
}

#[test]
fn resize_restores_automatically_hidden_windows_and_keeps_focus() {
    let mut windows = WindowSelection::default();
    let [a, b, c] = [SessionId(1), SessionId(2), SessionId(3)];
    windows.open(a);
    windows.show(b);
    windows.show(c);
    assert_eq!(windows.visible(0), []);
    assert_eq!(windows.visible(1), [c]);
    assert_eq!(windows.visible(2), [a, c]);
    assert_eq!(windows.visible(3), [a, b, c]);
    assert_eq!(windows.focused(), Some(c));
}

#[test]
fn maximize_and_add_restore_the_other_slots() {
    let mut windows = WindowSelection::default();
    let [a, b, c] = [SessionId(1), SessionId(2), SessionId(3)];
    windows.open(a);
    windows.show(b);
    windows.toggle_maximized(a);
    assert_eq!(windows.visible(3), [a]);
    windows.toggle_maximized(a);
    assert_eq!(windows.visible(3), [a, b]);
    windows.add_from_home();
    assert_eq!(windows.visible(3), []);
    windows.open(c);
    assert_eq!(windows.visible(3), [a, b, c]);
}

#[test]
fn double_prefix_sends_one_press_and_lowercase_x_cannot_kill() {
    let mut prefix = WindowPrefix::default();
    let ctrl_b = KeyInput::with_modifiers(InputKey::Char('b'), InputModifiers::CTRL);
    assert_eq!(prefix.route(&ctrl_b), WindowShortcut::Consume);
    assert_eq!(
        prefix.route(&ctrl_b.clone().repeated()),
        WindowShortcut::Consume
    );
    assert_eq!(
        prefix.route(&ctrl_b.clone().released()),
        WindowShortcut::Consume
    );
    assert_eq!(
        prefix.route(&ctrl_b),
        WindowShortcut::SendPrefix(ctrl_b.clone())
    );
    assert_eq!(
        prefix.route(&ctrl_b.clone().released()),
        WindowShortcut::Consume
    );
    assert_eq!(prefix.route(&ctrl_b), WindowShortcut::Consume);
    assert_eq!(
        prefix.route(&KeyInput::new(InputKey::Char('x'))),
        WindowShortcut::Pass
    );
    assert_eq!(prefix.route(&ctrl_b), WindowShortcut::Consume);
    assert_eq!(
        prefix.route(&KeyInput::new(InputKey::Char('X'))),
        WindowShortcut::Kill
    );
}
