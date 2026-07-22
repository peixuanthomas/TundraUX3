//! Cross-cutting shortcut, focus, and hit-testing contracts.

use ratatui::layout::Rect;
use ui::{
    Command, ComponentId, FocusDirection, FocusManager, FocusScope, HitKind, HitLayer, HitMap,
    HitTarget, InputEvent, KeyCode, KeyModifiers, KeyStroke, Point, ShortcutBinding,
    ShortcutRegistry, ShortcutScope, UiId,
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
fn shortcut_registry_stores_typed_intents_and_preserves_resolution_semantics() {
    #[derive(Debug, Clone, PartialEq, Eq)]
    enum UiIntent {
        Quit,
        CloseOverlay,
        OpenHelp,
    }

    let mut registry = ShortcutRegistry::<UiIntent>::new();
    let key = KeyStroke::char('q');
    registry
        .register(ShortcutBinding::global(key.clone(), UiIntent::Quit))
        .expect("global intent should register");
    registry
        .register(ShortcutBinding::local(
            "help.overlay",
            key.clone(),
            UiIntent::CloseOverlay,
        ))
        .expect("local intent should register");

    let conflict = registry
        .register(ShortcutBinding::global(key.clone(), UiIntent::OpenHelp))
        .expect_err("same scope/key should conflict for typed intents");
    assert_eq!(conflict.existing, UiIntent::Quit);
    assert_eq!(conflict.attempted, UiIntent::OpenHelp);

    assert_eq!(
        registry.command_for(
            &[ShortcutScope::Global, ShortcutScope::local("help.overlay")],
            &key,
        ),
        Some(&UiIntent::Quit),
    );
    assert_eq!(
        registry.command_for(
            &[ShortcutScope::local("help.overlay"), ShortcutScope::Global],
            &key,
        ),
        Some(&UiIntent::CloseOverlay),
    );

    let local_scope = UiId::from("help.overlay");
    assert_eq!(
        registry.resolve(Some(&local_scope), &InputEvent::key(KeyCode::Char('q'))),
        Some(&UiIntent::CloseOverlay),
    );
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
