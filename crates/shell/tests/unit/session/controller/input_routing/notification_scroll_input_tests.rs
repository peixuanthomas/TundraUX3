use super::*;

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
