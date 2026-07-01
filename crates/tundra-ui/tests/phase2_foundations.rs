use ratatui::layout::Rect;
use tundra_ui::{
    Command, ComponentId, FocusDirection, FocusManager, FocusScope, HitKind, HitMap, HitTarget,
    KeyCode, KeyModifiers, KeyStroke, Point, ShortcutBinding, ShortcutRegistry, ShortcutScope,
};

#[test]
fn shortcut_registry_reports_deterministic_conflicts() {
    let mut registry = ShortcutRegistry::new();
    let key = KeyStroke::ctrl_char('k');
    registry
        .register(ShortcutBinding::new(
            ShortcutScope::Global,
            key.clone(),
            Command::OpenCommandPalette,
        ))
        .expect("first binding should register");

    let conflict = registry
        .register(ShortcutBinding::new(
            ShortcutScope::Global,
            key.clone(),
            Command::Shutdown,
        ))
        .expect_err("same scope/key should conflict");

    assert_eq!(conflict.scope, ShortcutScope::Global);
    assert_eq!(conflict.key, key);
    assert_eq!(conflict.existing, Command::OpenCommandPalette);
    assert_eq!(conflict.attempted, Command::Shutdown);
}

#[test]
fn shortcut_registry_resolves_scopes_in_order() {
    let mut registry = ShortcutRegistry::new();
    registry
        .register(ShortcutBinding::new(
            ShortcutScope::Global,
            KeyStroke::char('q'),
            Command::OpenExitConfirm,
        ))
        .unwrap();
    registry
        .register(ShortcutBinding::new(
            ShortcutScope::Overlay("ExitConfirm".to_string()),
            KeyStroke::char('q'),
            Command::Noop,
        ))
        .unwrap();

    let command = registry.command_for(
        &[
            ShortcutScope::Overlay("ExitConfirm".to_string()),
            ShortcutScope::Global,
        ],
        &KeyStroke::char('q'),
    );

    assert_eq!(command, Some(&Command::Noop));
}

#[test]
fn focus_manager_wraps_and_traps_modal_focus() {
    let mut focus = FocusManager::new();
    focus.register("home.explorer").unwrap();
    focus.register("home.launcher").unwrap();

    assert_eq!(
        focus.focused().map(ComponentId::as_str),
        Some("home.explorer")
    );
    focus.move_focus(FocusDirection::Next);
    assert_eq!(
        focus.focused().map(ComponentId::as_str),
        Some("home.launcher")
    );
    focus.move_focus(FocusDirection::Next);
    assert_eq!(
        focus.focused().map(ComponentId::as_str),
        Some("home.explorer")
    );

    let modal = ComponentId::new("exit.confirm");
    focus
        .register_in_scope("exit.yes", FocusScope::Modal(modal.clone()))
        .unwrap();
    focus
        .register_in_scope("exit.no", FocusScope::Modal(modal.clone()))
        .unwrap();
    focus.enter_modal(modal).unwrap();

    assert_eq!(focus.focused().map(ComponentId::as_str), Some("exit.yes"));
    focus.move_focus(FocusDirection::Previous);
    assert_eq!(focus.focused().map(ComponentId::as_str), Some("exit.no"));
}

#[test]
fn hit_map_returns_topmost_latest_target() {
    let mut hit_map = HitMap::new();
    hit_map.register(HitTarget::new(
        "home.explorer",
        Rect::new(0, 0, 20, 1),
        HitKind::ListItem(0),
    ));
    hit_map.register(
        HitTarget::new("menu", Rect::new(0, 0, 10, 4), HitKind::ContextMenu).with_z_index(10),
    );

    let hit = hit_map.hit(Point::new(2, 0)).expect("target under point");

    assert_eq!(hit.id.as_str(), "menu");
    assert_eq!(hit.kind, HitKind::ContextMenu);
}

#[test]
fn keystroke_labels_preserve_modifiers() {
    assert_eq!(KeyStroke::ctrl_char('c').label(), "Ctrl+c");
    assert_eq!(
        KeyStroke::new(KeyCode::Tab, KeyModifiers::SHIFT).label(),
        "Shift+Tab"
    );
}
