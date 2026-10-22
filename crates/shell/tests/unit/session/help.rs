use super::*;

fn session() -> ShellSession {
    let mut state = ShellSession::new_for_home_mode(
        ShellLaunchConfig::default(),
        (120, 40),
        ShellHomeMode::User,
    );
    while state.notification_dismiss_active_modal_without_response() {}
    state.set_navigation_path(vec![ShellScreen::Home]);
    state.refresh_hit_map();
    state
}

#[test]
fn f1_opens_help_and_close_keys_restore_page_focus() {
    for close in [
        InputKey::Escape,
        InputKey::F(1),
        InputKey::Enter,
        InputKey::Char(' '),
    ] {
        let mut state = session();
        let platform = platform::mock::UnsupportedPlatform;
        let before = state.pages.common.focused_component;
        state.apply_input_with_platform(InputEvent::key(InputKey::F(1)), &platform);
        assert!(state.help_open);
        assert_eq!(
            state.pages.common.focused_component,
            ShellComponent::HelpDialog
        );
        state.apply_input_with_platform(InputEvent::key(close), &platform);
        assert!(!state.help_open);
        assert_eq!(state.pages.common.focused_component, before);
        assert_eq!(state.active_screen(), ShellScreen::Home);
    }
}

#[test]
fn help_is_blocked_by_notifications_and_page_dialogs() {
    let mut state = session();
    let platform = platform::mock::UnsupportedPlatform;
    state.notify_modal(
        i18n::LocalizedText::from("Notice"),
        i18n::LocalizedText::from("Body"),
        ui::NotificationTone::Info,
        Vec::new(),
    );
    state.apply_input_with_platform(InputEvent::key(InputKey::F(1)), &platform);
    assert!(!state.help_open);
    while state.notification_dismiss_active_modal_without_response() {}
    state.enter_screen(ShellScreen::Clock);
    state.pages.clock.clock_create_state = Some(ClockCreateState::default());
    state.refresh_hit_map();
    state.apply_input_with_platform(InputEvent::key(InputKey::F(1)), &platform);
    assert!(!state.help_open);
}

#[test]
fn help_scrolls_and_touch_close_requires_matching_release() {
    let mut state = session();
    state.terminal_size = (50, 12);
    state.open_page_help();
    state.scroll_page_help(1, true);
    assert!(state.help_scroll > 0);
    let model = ui::page_help_notification(&state.to_page_help_view_model(), state.help_scroll);
    let ui::NotificationLayout::Dialog(layout) =
        ui::notification_layout(state.shell_modal_area(), &model)
    else {
        panic!("help dialog");
    };
    let point = layout.actions[0].area;
    let platform = platform::mock::UnsupportedPlatform;
    state.apply_input_with_platform(
        InputEvent::mouse_down(PointerButton::Left, (point.x, point.y)),
        &platform,
    );
    assert!(state.help_open);
    state.apply_input_with_platform(
        InputEvent::mouse_up(PointerButton::Left, (point.x, point.y)),
        &platform,
    );
    assert!(!state.help_open);
}
