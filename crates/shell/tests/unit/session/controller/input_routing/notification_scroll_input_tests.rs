use super::*;

#[test]
fn notification_back_waits_for_matching_release_and_cancels_on_drag() {
    let mut session = session_with_long_notification();
    let back = session
        .hit_map
        .regions()
        .iter()
        .find(|region| region.component == ShellComponent::BackButton)
        .unwrap()
        .area;
    // A tall notification may cover the button's left border. Click its
    // exposed edge, just as the hit map requires for any covered control.
    let point = (back.right() - 1, back.y);
    assert_eq!(
        session.hit_map.target_at(point),
        Some(ShellComponent::BackButton)
    );
    session.button_regions.push(ui::components::ButtonRegion {
        id: "shell.back".into(),
        area: back,
        disabled: false,
    });
    session.apply_input(InputEvent::mouse_down(PointerButton::Left, point));
    assert!(session.notification_has_active_modal());
    session.apply_input(InputEvent::mouse_drag(PointerButton::Left, (0, 0)));
    session.apply_input(InputEvent::mouse_up(PointerButton::Left, point));
    assert!(session.notification_has_active_modal());
    session.apply_input(InputEvent::mouse_down(PointerButton::Left, point));
    assert!(session.notification_has_active_modal());
    session.apply_input(InputEvent::mouse_up(PointerButton::Left, point));
    assert!(!session.notification_has_active_modal());
}

#[test]
fn a_new_modal_cancels_dragging_on_the_page_below_it() {
    let mut session = ShellSession::new(ShellLaunchConfig::default(), (80, 24));
    session.logs_state.scrollbar_grab = Some(0);
    session.diagnostics_detail_drag = Some(0);
    session.scrollbar_drag = Some(ScrollbarDragState::Home { grab_offset: 0 });
    session.notify_modal(
        "Notice",
        "Drag ends here",
        ui::NotificationTone::Info,
        vec![],
    );
    assert!(session.logs_state.scrollbar_grab.is_none());
    assert!(session.diagnostics_detail_drag.is_none());
    assert!(session.scrollbar_drag.is_none());
}

#[test]
fn notification_scrollbar_drags_to_end_without_activating_a_dialog_action() {
    let mut session = session_with_long_notification();
    let model = session.to_notification_view_model().unwrap();
    let ui::NotificationLayout::Dialog(layout) =
        ui::notification_layout(Rect::new(0, 0, 80, 24), &model)
    else {
        panic!("dialog");
    };
    let track = layout.scrollbar.unwrap();
    // The renderer still retains page buttons behind the modal.
    session.button_regions.push(ui::components::ButtonRegion {
        id: "home.entry.0".into(),
        area: track,
        disabled: false,
    });
    session.apply_input(InputEvent::mouse_down(
        PointerButton::Left,
        (track.x, track.y),
    ));
    session.apply_input(InputEvent::mouse_drag(
        PointerButton::Left,
        (track.x + 2, track.bottom() + 1),
    ));
    session.apply_input(InputEvent::mouse_up(
        PointerButton::Left,
        (track.x + 2, track.bottom() + 1),
    ));
    assert_eq!(
        session.to_notification_view_model().unwrap().scroll_offset,
        layout.max_scroll_offset
    );
    assert!(session.notification_has_active_modal());
    assert!(session.notification_scrollbar_drag.is_none());
    session.apply_input(InputEvent::mouse_down(
        PointerButton::Left,
        (track.x, track.y),
    ));
    session.apply_input(InputEvent::FocusLost);
    assert!(session.notification_scrollbar_drag.is_none());
}

fn session_with_long_notification() -> ShellSession {
    let mut session = ShellSession::new(ShellLaunchConfig::default(), (80, 24));
    session.notify_modal(
        "Resource recovery",
        (0..80)
            .map(|line| format!("Repaired resource {line:02}: 中文资源.ftl"))
            .collect::<Vec<_>>()
            .join("\n"),
        ui::NotificationTone::Warning,
        vec![
            ShellNotificationAction::new("continue", "Continue"),
            ShellNotificationAction::new("cancel", "Cancel").cancel(),
        ],
    );
    session
}

#[test]
fn notification_pages_and_wheel_scroll_without_changing_action_focus() {
    let mut session = session_with_long_notification();
    let screen = session.active_screen();
    let model = session.to_notification_view_model().unwrap();
    let ui::NotificationLayout::Dialog(layout) =
        ui::notification_layout(Rect::new(0, 0, 80, 24), &model)
    else {
        panic!("notification should render with scrolling");
    };
    let point = (layout.message.x, layout.message.y);
    let page = usize::from(layout.message.height);
    assert!(layout.max_scroll_offset > page);
    session.apply_input(InputEvent::from_key_label("PageDown"));
    assert_eq!(
        session.to_notification_view_model().unwrap().scroll_offset,
        page
    );
    session.apply_input(InputEvent::mouse_scroll(ScrollDirection::Down, point));
    assert_eq!(
        session.to_notification_view_model().unwrap().scroll_offset,
        page + 1
    );
    session.apply_input(InputEvent::mouse_scroll(ScrollDirection::Up, point));
    assert_eq!(
        session.to_notification_view_model().unwrap().scroll_offset,
        page
    );
    assert!(session.to_notification_view_model().unwrap().actions[0].selected);
    session.apply_input(InputEvent::from_key_label("Tab"));
    assert!(session.to_notification_view_model().unwrap().actions[1].selected);
    session.apply_input(InputEvent::from_key_label("PageUp"));
    let model = session.to_notification_view_model().unwrap();
    assert_eq!(model.scroll_offset, 0);
    assert!(model.actions[1].selected);
    assert_eq!(session.active_screen(), screen);
    session.apply_input(InputEvent::key(InputKey::Escape));
    // Closing animations may still expose the outgoing view model after dismissal.
    assert!(!session.notification_has_active_modal());
    assert_eq!(session.active_screen(), screen);
}

#[test]
fn notification_scroll_captures_modified_keys_and_ignores_releases() {
    let mut session = session_with_long_notification();
    session.apply_input(InputEvent::key_with_modifiers(
        InputKey::PageDown,
        InputModifiers::CTRL,
    ));
    session.apply_input(InputEvent::key_with_phase(
        InputKey::PageDown,
        InputModifiers::default(),
        InputPhase::Release,
    ));
    assert_eq!(
        session.to_notification_view_model().unwrap().scroll_offset,
        0
    );
    session.apply_input(InputEvent::key_with_phase(
        InputKey::PageDown,
        InputModifiers::default(),
        InputPhase::Repeat,
    ));
    assert!(session.to_notification_view_model().unwrap().scroll_offset > 0);
    assert!(session.to_notification_view_model().unwrap().actions[0].selected);
}
