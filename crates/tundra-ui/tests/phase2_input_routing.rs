use ratatui::layout::Rect;
use tundra_ui::{
    FocusDirection, FocusManager, HitKind, HitMap, HitTarget, InputEvent, Key, KeyModifiers,
    MouseButton, MouseEvent, RoutedEvent,
};

#[test]
fn typed_mouse_event_routes_to_hit_target() {
    let mut hit_map = HitMap::new();
    hit_map.register(HitTarget::new(
        "home.explorer",
        Rect::new(2, 1, 10, 3),
        HitKind::Button,
    ));

    let event = InputEvent::mouse(MouseEvent::double_click(4, 2, MouseButton::Left));
    let routed = hit_map.route_input(event.clone());

    assert_eq!(routed, RoutedEvent::hit(event, "home.explorer"));
}

#[test]
fn focus_manager_handles_tab_and_shift_tab_input() {
    let mut focus = FocusManager::with_order(["first", "second", "third"]).unwrap();

    let next = focus.handle_input(&InputEvent::key(Key::Tab)).unwrap();
    assert_eq!(next.direction, FocusDirection::Next);
    assert_eq!(focus.focused().map(|id| id.as_str()), Some("second"));

    let previous = focus
        .handle_input(&InputEvent::key_with_modifiers(
            Key::Tab,
            KeyModifiers::SHIFT,
        ))
        .unwrap();
    assert_eq!(previous.direction, FocusDirection::Previous);
    assert_eq!(focus.focused().map(|id| id.as_str()), Some("first"));
}
