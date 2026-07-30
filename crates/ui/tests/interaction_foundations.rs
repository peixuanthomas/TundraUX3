//! Cross-cutting shortcut, focus, and hit-testing contracts.

use ratatui::layout::Rect;
use ui::{
    Command, ComponentId, FocusDirection, FocusManager, FocusScope, HitKind, HitLayer, HitMap,
    HitTarget, KeyStroke, Point, ShortcutBinding, ShortcutRegistry, ShortcutScope,
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
fn hit_map_prioritizes_typed_layers_over_z_index() {
    let mut hit_map = HitMap::new();
    let rect = Rect::new(0, 0, 10, 4);

    hit_map.register(
        HitTarget::new("content", rect, HitKind::custom("content"))
            .with_layer(HitLayer::AppContent)
            .with_z_index(100),
    );
    hit_map.register(
        HitTarget::new("overlay", rect, HitKind::custom("overlay"))
            .with_layer(HitLayer::AppOverlay)
            .with_z_index(-100),
    );
    hit_map.register(
        HitTarget::new("chrome", rect, HitKind::custom("chrome"))
            .with_layer(HitLayer::ShellChrome)
            .with_z_index(-100),
    );
    hit_map.register(
        HitTarget::new("modal", rect, HitKind::Dialog)
            .with_layer(HitLayer::ShellModal)
            .with_z_index(-100),
    );

    let hit = hit_map.hit(Point::new(2, 1)).expect("target under point");

    assert_eq!(hit.id.as_str(), "modal");
    assert!(HitLayer::ShellModal.z_index() > HitLayer::ShellChrome.z_index());
    assert!(HitLayer::ShellChrome.z_index() > HitLayer::AppOverlay.z_index());
    assert!(HitLayer::AppOverlay.z_index() > HitLayer::AppContent.z_index());
}

#[test]
fn hit_map_prefers_later_registration_within_a_layer() {
    let mut hit_map = HitMap::new();
    let rect = Rect::new(0, 0, 10, 4);

    hit_map
        .register(HitTarget::new("first", rect, HitKind::Button).with_layer(HitLayer::AppOverlay));
    hit_map.register(
        HitTarget::new("later", rect, HitKind::ContextMenu).with_layer(HitLayer::AppOverlay),
    );

    assert_eq!(
        hit_map
            .hit(Point::new(2, 1))
            .map(|target| target.id.as_str()),
        Some("later")
    );
}

#[test]
fn hit_map_skips_disabled_replacements_without_promoting_their_order() {
    let mut hit_map = HitMap::new();
    let rect = Rect::new(0, 0, 10, 4);

    hit_map.register(HitTarget::new("replaced", rect, HitKind::Button));
    hit_map.register(HitTarget::new("later", rect, HitKind::ContextMenu));
    hit_map.register(HitTarget::new("replaced", rect, HitKind::Dialog));

    assert_eq!(
        hit_map
            .hit(Point::new(2, 1))
            .map(|target| target.id.as_str()),
        Some("later"),
        "replacing an id must retain its original registration order"
    );

    hit_map.register(
        HitTarget::new("replaced", rect, HitKind::Dialog)
            .with_z_index(1)
            .disabled(),
    );
    assert_eq!(
        hit_map
            .hit(Point::new(2, 1))
            .map(|target| target.id.as_str()),
        Some("later"),
        "disabled replacements must not receive input"
    );

    hit_map.register(HitTarget::new("replaced", rect, HitKind::Dialog).with_z_index(1));
    assert_eq!(
        hit_map
            .hit(Point::new(2, 1))
            .map(|target| target.id.as_str()),
        Some("replaced")
    );
}
