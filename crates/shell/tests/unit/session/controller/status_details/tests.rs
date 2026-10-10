use super::*;

fn session() -> ShellSession {
    let mut state = ShellSession::new_for_home_mode(
        ShellLaunchConfig::default(),
        (80, 24),
        ShellHomeMode::User,
    );
    while state.notification_dismiss_active_modal_without_response() {}
    state.set_navigation_path(vec![ShellScreen::Home]);
    state.refresh_hit_map();
    state
}

fn click_status(state: &mut ShellSession) {
    let area = state.frame_layout.unwrap().status_message.unwrap();
    let point = (area.x + 1, area.y + 1);
    state.apply_input(InputEvent::mouse_down(PointerButton::Left, point));
    assert!(
        !state.status_details_visible(),
        "press must wait for release"
    );
    state.apply_input(InputEvent::mouse_up(PointerButton::Left, point));
}

#[test]
fn status_details_keep_the_displayed_toast_and_alert_text_after_updates() {
    for alert in [false, true] {
        let mut state = session();
        state.notify_status("Base status");
        state.notify_toast("Saved 中文\nsecond line 👩‍💻".repeat(40));
        if alert {
            state.notify_alert_with_tone(
                "Failure details\n原文".repeat(40),
                ui::NotificationTone::Error,
            );
        }
        state.displayed_status = Some(state.to_shell_chrome_view_model().status.full_message());
        let expected = state.displayed_status.clone().unwrap();
        click_status(&mut state);
        assert!(state.status_details_visible());
        assert_eq!(
            state.to_notification_view_model().unwrap().message,
            expected
        );
        state.notify_toast("A later toast");
        state.notify_status("A later status");
        state.clear_notification_alert();
        assert_eq!(
            state.to_notification_view_model().unwrap().message,
            expected
        );
        state.apply_input(InputEvent::key(InputKey::Escape));
        assert!(!state.notification_has_active_modal());
        assert_eq!(state.focused_component, ShellComponent::Home);
    }
}

#[test]
fn status_details_scroll_wrapped_unicode_and_close_from_the_shared_button() {
    let mut state = session();
    let text = "中文状态👩‍💻 ".repeat(500);
    state.notify_status(text.clone());
    click_status(&mut state);
    let model = state.to_notification_view_model().unwrap();
    assert_eq!(model.message, text);
    let ui::NotificationLayout::Dialog(layout) =
        ui::notification_layout(state.shell_modal_area(), &model)
    else {
        panic!("status details should fit");
    };
    assert!(layout.scrollbar.is_some());
    assert!(layout.max_scroll_offset > usize::from(layout.message.height));
    state.apply_input(InputEvent::key(InputKey::Down));
    assert_eq!(state.to_notification_view_model().unwrap().scroll_offset, 1);
    state.apply_input(InputEvent::key(InputKey::PageDown));
    assert_eq!(
        state.to_notification_view_model().unwrap().scroll_offset,
        1 + usize::from(layout.message.height)
    );
    state.apply_input(InputEvent::key(InputKey::End));
    assert_eq!(
        state.to_notification_view_model().unwrap().scroll_offset,
        layout.max_scroll_offset
    );
    state.apply_input(InputEvent::key(InputKey::Home));
    assert_eq!(state.to_notification_view_model().unwrap().scroll_offset, 0);
    state.apply_input(InputEvent::mouse_scroll(
        ScrollDirection::Down,
        (layout.message.x, layout.message.y),
    ));
    assert_eq!(state.to_notification_view_model().unwrap().scroll_offset, 1);
    let close = layout.actions[0].area;
    let point = (close.x, close.y);
    state.apply_input(InputEvent::mouse_down(PointerButton::Left, point));
    assert!(state.status_details_visible());
    state.apply_input(InputEvent::mouse_up(PointerButton::Left, point));
    assert!(!state.notification_has_active_modal());
}

#[test]
fn status_click_cancels_after_drag_focus_loss_and_resize() {
    for cancellation in [
        InputEvent::mouse_drag(PointerButton::Left, (0, 0)),
        InputEvent::FocusLost,
        InputEvent::Resize {
            width: 80,
            height: 24,
        },
    ] {
        let mut state = session();
        let area = state.frame_layout.unwrap().status_message.unwrap();
        let point = (area.x + 1, area.y + 1);
        state.apply_input(InputEvent::mouse_down(PointerButton::Left, point));
        state.apply_input(cancellation);
        state.apply_input(InputEvent::mouse_up(PointerButton::Left, point));
        assert!(!state.notification_has_active_modal());
    }
}

#[test]
fn status_details_do_not_bypass_existing_or_new_critical_modals() {
    let mut state = session();
    state.notify_critical_modal("Critical", "Review this first", vec![]);
    let critical = state.notification_active_modal_id();
    state.open_status_details();
    assert_eq!(state.notification_active_modal_id(), critical);
    assert!(!state.status_details_visible());
    while state.notification_dismiss_active_modal_without_response() {}
    state.finish_modal_focus_transition();
    state.open_status_details();
    let details = state.notification_active_modal_id();
    state.notify_critical_modal("Critical", "Interrupt reading", vec![]);
    assert_ne!(state.notification_active_modal_id(), details);
    assert_eq!(
        state.to_notification_view_model().unwrap().tone,
        ui::NotificationTone::Critical
    );
    state.set_navigation_path(vec![ShellScreen::Home, ShellScreen::CommandLine]);
    state.refresh_hit_map();
    assert!(matches!(
        state.route_key_input(&KeyInput::new(InputKey::Enter)).1,
        ShellCommand::NotificationActivateSelected
    ));
}

#[test]
fn status_details_capture_command_line_keys_paste_and_the_back_button() {
    let mut state = session();
    state.set_navigation_path(vec![ShellScreen::Home, ShellScreen::CommandLine]);
    state.refresh_hit_map();
    state.notify_status("Child command output status");
    click_status(&mut state);
    assert!(state.status_details_visible());
    assert!(matches!(
        state.route_key_input(&KeyInput::new(InputKey::Char('x'))).1,
        ShellCommand::CaptureOverlayInput
    ));
    let routed = state.route_input_at(InputEvent::paste("dangerous child input"), Instant::now());
    assert_eq!(routed.command, ShellCommand::CaptureOverlayInput);
    let back = state.frame_layout.unwrap().back_button.unwrap();
    assert_eq!(
        state.normalize_shell_navigation_input(InputEvent::mouse_down(
            PointerButton::Left,
            (back.x, back.y)
        )),
        InputEvent::key(InputKey::Escape)
    );
    state.apply_input(InputEvent::key(InputKey::Escape));
    assert!(!state.notification_has_active_modal());
    assert_eq!(state.active_screen(), ShellScreen::CommandLine);
}
