use std::fs;
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use chrono::{TimeZone, Utc};
use ratatui::layout::Rect;
use tundra_platform::{
    CapabilityStatus, PlatformCapabilities, PlatformKind, build_windows_app_paths,
};
use tundra_shell::{
    ClickKind, HomeModeOverride, InputEvent, InputKey, InputModifiers, InputPhase, KeyInput,
    PointerButton, ScrollDirection, ShellAction, ShellAppConfig, ShellCommand, ShellComponent,
    ShellHomeMode, ShellLaunchConfig, ShellNotificationAction, ShellRestoredSession, ShellScreen,
    ShellStartupState, ShellState, ShellStorageReport, ShellTerminalMode, default_shell_shortcuts,
    detect_shortcut_conflicts,
};
use tundra_storage::StorageManager;
use tundra_ui::{HomeDisplayMode, NotificationLayout, NotificationTone};

fn debug_config() -> ShellLaunchConfig {
    ShellLaunchConfig {
        terminal_mode: ShellTerminalMode::Fullscreen,
        home_mode_override: HomeModeOverride::Debug,
    }
}

fn build_default_config() -> ShellLaunchConfig {
    ShellLaunchConfig {
        terminal_mode: ShellTerminalMode::Fullscreen,
        home_mode_override: HomeModeOverride::BuildDefault,
    }
}

#[test]
fn debug_override_selects_debug_home() {
    let state = ShellState::new(debug_config(), (120, 40));

    assert_eq!(state.home_mode(), ShellHomeMode::Debug);
    assert_eq!(state.active_screen(), ShellScreen::Home);
    assert_eq!(state.screen_stack(), &[ShellScreen::Home][..]);
    assert_eq!(state.terminal_size(), (120, 40));
    assert_eq!(state.status(), "Ready");
    assert_eq!(state.tick_count(), 0);
    assert_eq!(state.last_key_event(), None);
    assert_eq!(state.last_mouse_event(), None);
    assert_eq!(state.last_resize_event(), None);
    assert_eq!(state.mouse_coordinates(), None);
    assert_eq!(state.mouse_scroll_direction(), None);
    assert_eq!(state.mouse_drag_direction(), None);
    assert_eq!(state.focused_component(), ShellComponent::Home);
    assert_eq!(state.hovered_component(), None);
    assert_eq!(state.active_popup(), None);
    assert_eq!(state.hit_target_at((1, 1)), Some(ShellComponent::TopBar));
    assert_eq!(state.hit_target_at((1, 4)), Some(ShellComponent::Home));
    assert_eq!(
        state.hit_target_at((1, 38)),
        Some(ShellComponent::StatusBar)
    );
    assert!(!state.shutdown_requested());

    let terminal_flags = state.terminal_flags();
    assert!(terminal_flags.raw_mode);
    assert!(terminal_flags.alternate_screen);
    assert!(terminal_flags.mouse_capture);
    assert!(terminal_flags.cursor_restore_enabled);
}

#[test]
fn build_default_selects_expected_home_for_build_profile() {
    let state = ShellState::new(build_default_config(), (120, 40));
    let expected = if cfg!(debug_assertions) {
        ShellHomeMode::Debug
    } else {
        ShellHomeMode::User
    };

    assert_eq!(state.home_mode(), expected);
}

#[test]
fn q_opens_exit_confirmation_from_home() {
    let mut state = ShellState::new(debug_config(), (120, 40));

    let action = state.apply_input(InputEvent::from_key_label("q"));

    assert_eq!(action, ShellAction::Redraw);
    assert_eq!(state.active_screen(), ShellScreen::ExitConfirm);
    assert_eq!(state.status(), "Confirm exit");
    assert_eq!(
        state.screen_stack(),
        &[ShellScreen::Home, ShellScreen::ExitConfirm][..]
    );
    assert!(!state.shutdown_requested());
}

#[test]
fn escape_opens_exit_confirmation_from_home() {
    let mut state = ShellState::new(debug_config(), (120, 40));

    let action = state.apply_input(InputEvent::from_key_label("Esc"));

    assert_eq!(action, ShellAction::Redraw);
    assert_eq!(state.active_screen(), ShellScreen::ExitConfirm);
    assert_eq!(state.status(), "Confirm exit");
    assert_eq!(
        state.screen_stack(),
        &[ShellScreen::Home, ShellScreen::ExitConfirm][..]
    );
    assert!(!state.shutdown_requested());
}

#[test]
fn escape_cancels_exit_confirmation() {
    let mut state = ShellState::new(debug_config(), (120, 40));
    state.apply_input(InputEvent::from_key_label("q"));

    let action = state.apply_input(InputEvent::from_key_label("Esc"));

    assert_eq!(action, ShellAction::Redraw);
    assert_eq!(state.active_screen(), ShellScreen::Home);
    assert_eq!(state.status(), "Ready");
    assert_eq!(state.screen_stack(), &[ShellScreen::Home][..]);
    assert!(!state.shutdown_requested());
}

#[test]
fn enter_confirms_exit_confirmation() {
    let mut state = ShellState::new(debug_config(), (120, 40));
    state.apply_input(InputEvent::from_key_label("q"));

    let action = state.apply_input(InputEvent::from_key_label("Enter"));

    assert_eq!(action, ShellAction::Exit);
    assert_eq!(state.active_screen(), ShellScreen::ExitConfirm);
    assert!(state.shutdown_requested());
}

#[test]
fn y_and_uppercase_y_confirm_exit_confirmation() {
    for key in ["y", "Y"] {
        let mut state = ShellState::new(debug_config(), (120, 40));
        state.apply_input(InputEvent::from_key_label("q"));

        let action = state.apply_input(InputEvent::from_key_label(key));

        assert_eq!(action, ShellAction::Exit);
        assert_eq!(state.active_screen(), ShellScreen::ExitConfirm);
        assert!(state.shutdown_requested());
    }
}

#[test]
fn n_and_uppercase_n_cancel_exit_confirmation() {
    for key in ["n", "N"] {
        let mut state = ShellState::new(debug_config(), (120, 40));
        state.apply_input(InputEvent::from_key_label("q"));

        let action = state.apply_input(InputEvent::from_key_label(key));

        assert_eq!(action, ShellAction::Redraw);
        assert_eq!(state.active_screen(), ShellScreen::Home);
        assert_eq!(state.status(), "Ready");
        assert_eq!(state.screen_stack(), &[ShellScreen::Home][..]);
        assert!(!state.shutdown_requested());
    }
}

#[test]
fn other_key_is_recorded() {
    let mut state = ShellState::new(debug_config(), (120, 40));

    let action = state.apply_input(InputEvent::from_key_label("x"));

    assert_eq!(action, ShellAction::Redraw);
    assert_eq!(state.active_screen(), ShellScreen::Home);
    assert_eq!(state.last_key_event(), Some("x"));
}

#[test]
fn mouse_and_resize_events_are_recorded() {
    let mut state = ShellState::new(debug_config(), (120, 40));

    let mouse_action = state.apply_input(InputEvent::mouse_scroll(ScrollDirection::Down, (12, 7)));
    let resize_action = state.apply_input(InputEvent::Resize {
        width: 80,
        height: 24,
    });

    assert_eq!(mouse_action, ShellAction::Redraw);
    assert_eq!(resize_action, ShellAction::Redraw);
    assert_eq!(state.last_mouse_event(), Some("Mouse Scroll Down"));
    assert_eq!(state.mouse_coordinates(), Some((12, 7)));
    assert_eq!(state.mouse_scroll_direction(), Some("Down"));
    assert_eq!(state.terminal_size(), (80, 24));
    assert_eq!(state.last_resize_event(), Some("80x24"));
}

#[test]
fn mouse_drag_direction_updates_from_each_drag_delta() {
    let mut state = ShellState::new(debug_config(), (120, 40));

    state.apply_input(InputEvent::mouse_down(PointerButton::Left, (10, 10)));

    state.apply_input(InputEvent::mouse_drag(PointerButton::Left, (13, 10)));
    assert_eq!(state.last_mouse_event(), Some("Mouse Drag Left to Right"));
    assert_eq!(state.mouse_drag_direction(), Some("Right"));

    state.apply_input(InputEvent::mouse_drag(PointerButton::Left, (11, 10)));
    assert_eq!(state.last_mouse_event(), Some("Mouse Drag Left to Left"));
    assert_eq!(state.mouse_drag_direction(), Some("Left"));

    state.apply_input(InputEvent::mouse_drag(PointerButton::Left, (11, 8)));
    assert_eq!(state.last_mouse_event(), Some("Mouse Drag Left to Up"));
    assert_eq!(state.mouse_drag_direction(), Some("Up"));

    state.apply_input(InputEvent::mouse_drag(PointerButton::Left, (11, 12)));
    assert_eq!(state.last_mouse_event(), Some("Mouse Drag Left to Down"));
    assert_eq!(state.mouse_drag_direction(), Some("Down"));

    state.apply_input(InputEvent::mouse_up(PointerButton::Left, (11, 12)));
    assert_eq!(state.mouse_drag_direction(), None);
}

#[test]
fn tab_and_shift_tab_route_to_focus_commands() {
    let mut state = ShellState::new(debug_config(), (120, 40));

    state.apply_input(InputEvent::from_key_label("Tab"));
    assert_eq!(state.focused_component(), ShellComponent::ClockButton);
    assert_eq!(state.last_command(), Some(&ShellCommand::FocusNext));

    state.apply_input(InputEvent::from_key_label("Tab"));
    assert_eq!(state.focused_component(), ShellComponent::StatusBar);
    assert_eq!(state.last_command(), Some(&ShellCommand::FocusNext));

    state.apply_input(InputEvent::from_key_label("Shift+Tab"));
    assert_eq!(state.focused_component(), ShellComponent::ClockButton);
    assert_eq!(state.last_command(), Some(&ShellCommand::FocusPrevious));
}

#[test]
fn mouse_move_updates_hover_from_hit_map() {
    let mut state = ShellState::new(debug_config(), (120, 40));

    state.apply_input(InputEvent::mouse_moved((2, 1)));

    assert_eq!(state.hovered_component(), Some(ShellComponent::TopBar));
    assert_eq!(
        state.last_command(),
        Some(&ShellCommand::Hover(Some(ShellComponent::TopBar)))
    );
}

#[test]
fn right_click_opens_context_menu_popup_on_hit_target() {
    let mut state = ShellState::new(debug_config(), (120, 40));

    state.apply_input(InputEvent::mouse_down(PointerButton::Right, (4, 4)));

    assert_eq!(
        state.active_popup().map(|popup| popup.owner),
        Some(Some(ShellComponent::Home))
    );
    assert_eq!(state.focused_component(), ShellComponent::ContextMenu);
    assert_eq!(
        state.last_command(),
        Some(&ShellCommand::OpenContextMenu {
            target: Some(ShellComponent::Home),
            coordinates: (4, 4),
        })
    );
    assert_eq!(
        state.hit_target_at((4, 4)),
        Some(ShellComponent::ContextMenu)
    );
}

#[test]
fn popup_closes_on_outside_click_without_activating_underlying_target() {
    let mut state = ShellState::new(debug_config(), (120, 40));
    state.apply_input(InputEvent::mouse_down(PointerButton::Right, (4, 4)));

    let action = state.apply_input(InputEvent::mouse_down(PointerButton::Left, (80, 20)));

    assert_eq!(action, ShellAction::Redraw);
    assert_eq!(state.active_popup(), None);
    assert_eq!(state.last_command(), Some(&ShellCommand::ClosePopup));
    assert_eq!(state.status(), "Ready");
}

#[test]
fn modal_captures_mouse_without_closing_or_activating_home() {
    let mut state = ShellState::new(debug_config(), (120, 40));
    state.apply_input(InputEvent::from_key_label("q"));

    let action = state.apply_input(InputEvent::mouse_down(PointerButton::Left, (2, 4)));

    assert_eq!(action, ShellAction::Redraw);
    assert_eq!(state.active_screen(), ShellScreen::ExitConfirm);
    assert_eq!(state.focused_component(), ShellComponent::ExitDialog);
    assert_eq!(
        state.last_command(),
        Some(&ShellCommand::CaptureOverlayInput)
    );
    assert_eq!(state.shutdown_requested(), false);
}

#[test]
fn double_click_is_detected_for_same_component_and_nearby_cell() {
    let mut state = ShellState::new(debug_config(), (120, 40));
    let started_at = Instant::now();

    state.route_input_at(
        InputEvent::mouse_down(PointerButton::Left, (10, 5)),
        started_at,
    );

    let routed = state.route_input_at(
        InputEvent::mouse_down(PointerButton::Left, (11, 6)),
        started_at + Duration::from_millis(100),
    );

    assert_eq!(
        routed.command,
        ShellCommand::ActivateHomeEntryAt((11, 6), ClickKind::Double)
    );
}

#[test]
fn resize_refreshes_hit_map_and_keeps_not_fullscreen_unrelated_state_untouched() {
    let mut state = ShellState::new(debug_config(), (120, 40));
    let initial_generation = state.hit_map_generation();

    state.apply_input(InputEvent::Resize {
        width: 40,
        height: 10,
    });

    assert!(state.hit_map_generation() > initial_generation);
    assert_eq!(state.terminal_size(), (40, 10));
    assert_eq!(
        state.hit_target_at((1, 1)),
        Some(ShellComponent::CompactHome)
    );
    assert_eq!(state.focused_component(), ShellComponent::CompactHome);
}

#[test]
fn default_shortcuts_have_no_conflicts() {
    assert!(detect_shortcut_conflicts(&default_shell_shortcuts()).is_empty());
}

#[test]
fn tick_increments_count() {
    let mut state = ShellState::new(debug_config(), (120, 40));

    let first_action = state.apply_input(InputEvent::Tick);
    let second_action = state.apply_input(InputEvent::Tick);

    assert_eq!(first_action, ShellAction::Redraw);
    assert_eq!(second_action, ShellAction::Redraw);
    assert_eq!(state.tick_count(), 2);
}

#[test]
fn shutdown_input_exits_immediately() {
    let mut state = ShellState::new(debug_config(), (120, 40));

    let action = state.apply_input(InputEvent::Shutdown);

    assert_eq!(action, ShellAction::Exit);
    assert!(state.shutdown_requested());
}

#[test]
fn debug_state_builds_debug_home_view_model() {
    let mut state = ShellState::new(debug_config(), (120, 40));
    state.apply_input(InputEvent::from_key_label("x"));
    state.apply_input(InputEvent::Tick);

    let home = state.to_home_view_model();

    assert_eq!(home.display_mode(), HomeDisplayMode::Debug);
    let diagnostics = home.diagnostics().expect("debug diagnostics");
    assert_eq!(diagnostics.tick_count, 1);
    assert_eq!(diagnostics.last_key_event.as_deref(), Some("x"));
    assert_eq!(
        diagnostics.platform_capability_summary,
        state.platform_capability_summary()
    );
    assert!(
        diagnostics
            .platform_capability_summary
            .contains("supported")
    );
}

#[test]
fn terminal_flags_are_visible_in_debug_view_model() {
    let state = ShellState::new(debug_config(), (120, 40));

    let home = state.to_home_view_model();

    let diagnostics = home.diagnostics().expect("debug diagnostics");
    assert_eq!(
        diagnostics.terminal_flags,
        vec![
            "raw mode: enabled".to_string(),
            "alternate screen: enabled".to_string(),
            "mouse capture: enabled".to_string(),
            "cursor restore: enabled".to_string(),
        ]
    );
}

#[test]
fn debug_status_bar_includes_recent_input_diagnostics() {
    let mut state = ShellState::new(debug_config(), (120, 40));

    state.apply_input(InputEvent::from_key_label("x"));
    state.apply_input(InputEvent::mouse_scroll(ScrollDirection::Down, (12, 7)));

    let chrome = state.to_shell_chrome_view_model();

    assert!(chrome.status.status.contains("Key: x"));
    assert!(chrome.status.status.contains("Mouse: Mouse Scroll Down"));
    assert!(chrome.status.status.contains("Resize: none"));
}

#[test]
fn global_notifications_update_status_toast_and_alert() {
    let mut state = ShellState::new(debug_config(), (120, 40));

    state.notify_status("Working");
    state.notify_toast("Saved");
    state.notify_alert("Check configuration");

    let chrome = state.to_shell_chrome_view_model();
    assert!(chrome.status.status.starts_with("Working"));
    assert_eq!(chrome.status.toast.as_deref(), Some("Saved"));
    assert_eq!(chrome.status.error.as_deref(), Some("Check configuration"));
    assert_eq!(state.take_notification_response(), None);

    for _ in 0..16 {
        state.apply_input(InputEvent::Tick);
    }

    let chrome = state.to_shell_chrome_view_model();
    assert_eq!(chrome.status.toast.as_deref(), Some("Saved"));
    assert_eq!(chrome.status.error.as_deref(), Some("Check configuration"));

    state.clear_notification_alert();
    assert_eq!(state.to_shell_chrome_view_model().status.error, None);
}

#[test]
fn modal_shortcuts_reject_ctrl_alt_and_super_modifiers() {
    let mut state = ShellState::new(debug_config(), (120, 40));
    let id = state.notify_modal(
        "Confirm",
        "Continue?",
        NotificationTone::Warning,
        vec![
            ShellNotificationAction::new("confirm", "Confirm")
                .with_shortcut(InputKey::Character('y')),
            ShellNotificationAction::new("cancel", "Cancel").cancel(),
        ],
    );

    let modified_shortcuts = [
        InputModifiers {
            control: true,
            ..InputModifiers::none()
        },
        InputModifiers {
            alt: true,
            ..InputModifiers::none()
        },
        InputModifiers {
            super_key: true,
            ..InputModifiers::none()
        },
    ];

    for modifiers in modified_shortcuts {
        state.apply_input(key_input(
            InputKey::Character('y'),
            modifiers,
            InputPhase::Press,
        ));

        assert!(state.to_notification_view_model().is_some());
        assert_eq!(state.take_notification_response(), None);
        assert_eq!(
            state.last_command(),
            Some(&ShellCommand::CaptureOverlayInput)
        );
    }

    state.apply_input(key_input(
        InputKey::Character('Y'),
        InputModifiers {
            shift: true,
            ..InputModifiers::none()
        },
        InputPhase::Press,
    ));

    assert_eq!(
        state.take_notification_response(),
        Some(tundra_shell::ShellNotificationResponse {
            notification_id: id,
            action_id: "confirm".to_string(),
        })
    );
    assert!(state.to_notification_view_model().is_none());
}

#[test]
fn modal_shortcut_shift_is_only_accepted_for_ascii_letter_case() {
    let mut state = ShellState::new(debug_config(), (120, 40));
    state.notify_modal(
        "Confirm",
        "Continue?",
        NotificationTone::Info,
        vec![ShellNotificationAction::new("space", "Continue")
            .with_shortcut(InputKey::Character(' '))],
    );

    state.apply_input(key_input(
        InputKey::Character(' '),
        InputModifiers {
            shift: true,
            ..InputModifiers::none()
        },
        InputPhase::Press,
    ));

    assert_eq!(state.take_notification_response(), None);
    assert!(state.to_notification_view_model().is_some());
}

#[test]
fn modal_activation_rejects_repeat_and_release_but_repeat_navigation_works() {
    let mut state = ShellState::new(debug_config(), (120, 40));
    let id = state.notify_modal(
        "Confirm",
        "Continue?",
        NotificationTone::Warning,
        vec![
            ShellNotificationAction::new("confirm", "Confirm")
                .with_shortcut(InputKey::Character('y')),
            ShellNotificationAction::new("cancel", "Cancel").cancel(),
        ],
    );

    for (key, phase) in [
        (InputKey::Enter, InputPhase::Repeat),
        (InputKey::Enter, InputPhase::Release),
        (InputKey::Character('y'), InputPhase::Repeat),
        (InputKey::Character('y'), InputPhase::Release),
    ] {
        state.apply_input(key_input(key, InputModifiers::none(), phase));
        assert!(state.to_notification_view_model().is_some());
        assert_eq!(state.take_notification_response(), None);
    }

    state.apply_input(key_input(
        InputKey::Right,
        InputModifiers::none(),
        InputPhase::Repeat,
    ));
    assert_eq!(selected_notification_action(&state), Some(1));

    state.apply_input(key_input(
        InputKey::Enter,
        InputModifiers::none(),
        InputPhase::Press,
    ));
    assert_eq!(
        state.take_notification_response(),
        Some(tundra_shell::ShellNotificationResponse {
            notification_id: id,
            action_id: "cancel".to_string(),
        })
    );
}

#[test]
fn modal_repeat_after_queue_promotion_does_not_activate_next_modal() {
    let mut state = ShellState::new(debug_config(), (120, 40));
    let first = state.notify_modal(
        "First",
        "First message",
        NotificationTone::Info,
        vec![ShellNotificationAction::new("first", "OK")],
    );
    let second = state.notify_modal(
        "Second",
        "Second message",
        NotificationTone::Info,
        vec![ShellNotificationAction::new("second", "OK")],
    );

    state.apply_input(key_input(
        InputKey::Enter,
        InputModifiers::none(),
        InputPhase::Press,
    ));
    assert_eq!(
        state.take_notification_response(),
        Some(tundra_shell::ShellNotificationResponse {
            notification_id: first,
            action_id: "first".to_string(),
        })
    );

    state.apply_input(key_input(
        InputKey::Enter,
        InputModifiers::none(),
        InputPhase::Repeat,
    ));
    assert_eq!(state.take_notification_response(), None);
    assert_eq!(
        state
            .to_notification_view_model()
            .as_ref()
            .map(|model| model.title.as_str()),
        Some("Second")
    );

    state.apply_input(key_input(
        InputKey::Enter,
        InputModifiers::none(),
        InputPhase::Press,
    ));
    assert_eq!(
        state.take_notification_response(),
        Some(tundra_shell::ShellNotificationResponse {
            notification_id: second,
            action_id: "second".to_string(),
        })
    );
}

#[test]
fn capture_overlay_input_preserves_business_status() {
    let mut state = ShellState::new(debug_config(), (120, 40));
    state.notify_status("Background export is running");
    state.notify_modal(
        "Confirm",
        "Continue?",
        NotificationTone::Info,
        vec![ShellNotificationAction::new("ok", "OK")],
    );

    state.apply_input(InputEvent::from_key_label("x"));

    assert_eq!(state.status(), "Background export is running");
    assert_eq!(
        state.last_command(),
        Some(&ShellCommand::CaptureOverlayInput)
    );
    assert!(state.to_notification_view_model().is_some());
}

#[test]
fn modal_actions_require_left_click_release_on_same_action() {
    let (mut accepted, accepted_id) = state_with_two_action_modal();
    let accepted_action = notification_action_coordinates(&accepted, 0);

    accepted.apply_input(InputEvent::mouse_down(PointerButton::Left, accepted_action));
    assert_eq!(accepted.take_notification_response(), None);
    assert!(accepted.to_notification_view_model().is_some());

    accepted.apply_input(InputEvent::mouse_up(PointerButton::Left, accepted_action));
    assert_eq!(
        accepted.take_notification_response(),
        Some(tundra_shell::ShellNotificationResponse {
            notification_id: accepted_id,
            action_id: "confirm".to_string(),
        })
    );

    let (mut rejected, _) = state_with_two_action_modal();
    let first_action = notification_action_coordinates(&rejected, 0);
    let second_action = notification_action_coordinates(&rejected, 1);
    let dialog = notification_dialog_layout(
        &rejected,
        Rect::new(0, 0, rejected.terminal_size().0, rejected.terminal_size().1),
    )
    .dialog;
    let outside_actions = (dialog.x, dialog.y);

    for button in [PointerButton::Right, PointerButton::Middle] {
        rejected.apply_input(InputEvent::mouse_down(button, first_action));
        rejected.apply_input(InputEvent::mouse_up(button, first_action));
        assert_eq!(rejected.take_notification_response(), None);
        assert!(rejected.to_notification_view_model().is_some());
    }

    rejected.apply_input(InputEvent::mouse_down(PointerButton::Left, first_action));
    rejected.apply_input(InputEvent::mouse_up(PointerButton::Left, second_action));
    assert_eq!(rejected.take_notification_response(), None);
    assert!(rejected.to_notification_view_model().is_some());

    rejected.apply_input(InputEvent::mouse_down(PointerButton::Left, first_action));
    rejected.apply_input(InputEvent::mouse_drag(PointerButton::Left, outside_actions));
    rejected.apply_input(InputEvent::mouse_up(PointerButton::Left, outside_actions));
    assert_eq!(rejected.take_notification_response(), None);
    assert!(rejected.to_notification_view_model().is_some());

    rejected.apply_input(InputEvent::mouse_down(PointerButton::Left, first_action));
    rejected.apply_input(InputEvent::mouse_drag(PointerButton::Left, outside_actions));
    rejected.apply_input(InputEvent::mouse_drag(PointerButton::Left, first_action));
    rejected.apply_input(InputEvent::mouse_up(PointerButton::Left, first_action));
    assert_eq!(rejected.take_notification_response(), None);
    assert!(rejected.to_notification_view_model().is_some());
}

#[test]
fn undersized_notification_rejects_keyboard_and_mouse_activation() {
    for shrink_width in [true, false] {
        let mut state = ShellState::new(debug_config(), (120, 40));
        state.notify_modal(
            "Confirm",
            "Continue?",
            NotificationTone::Warning,
            vec![
                ShellNotificationAction::new("confirm", "Confirm")
                    .with_shortcut(InputKey::Character('y')),
            ],
        );

        let large_area = Rect::new(0, 0, 120, 40);
        let large_layout = notification_dialog_layout(&state, large_area);
        let required_area = Rect::new(0, 0, large_layout.dialog.width, large_layout.dialog.height);
        let action_coordinates = notification_action_coordinates_in(&state, required_area, 0);
        let too_small_size = if shrink_width {
            (required_area.width.saturating_sub(1), required_area.height)
        } else {
            (required_area.width, required_area.height.saturating_sub(1))
        };

        state.apply_input(InputEvent::Resize {
            width: too_small_size.0,
            height: too_small_size.1,
        });
        let model = state
            .to_notification_view_model()
            .expect("undersized modal remains active");
        assert!(matches!(
            tundra_ui::notification_layout(
                Rect::new(0, 0, too_small_size.0, too_small_size.1),
                &model,
            ),
            NotificationLayout::TooSmall { .. }
        ));

        state.apply_input(key_input(
            InputKey::Character('y'),
            InputModifiers::none(),
            InputPhase::Press,
        ));
        state.apply_input(key_input(
            InputKey::Enter,
            InputModifiers::none(),
            InputPhase::Press,
        ));
        state.apply_input(InputEvent::mouse_down(
            PointerButton::Left,
            action_coordinates,
        ));
        state.apply_input(InputEvent::mouse_up(
            PointerButton::Left,
            action_coordinates,
        ));

        assert_eq!(state.take_notification_response(), None);
        assert!(state.to_notification_view_model().is_some());

        state.apply_input(key_input(
            InputKey::Escape,
            InputModifiers::none(),
            InputPhase::Press,
        ));
        assert_eq!(state.take_notification_response(), None);
        assert!(state.to_notification_view_model().is_none());
    }
}

#[test]
fn modal_close_restores_previous_focus() {
    let mut state = ShellState::new(debug_config(), (120, 40));
    focus_clock_button(&mut state);
    state.notify_modal(
        "Notice",
        "Acknowledge this message.",
        NotificationTone::Info,
        vec![ShellNotificationAction::new("ok", "OK")],
    );

    assert_eq!(
        state.focused_component(),
        ShellComponent::NotificationDialog
    );
    state.apply_input(InputEvent::from_key_label("Enter"));

    assert_eq!(state.focused_component(), ShellComponent::ClockButton);
}

#[test]
fn queued_modals_restore_focus_after_last_modal_closes() {
    let mut state = ShellState::new(debug_config(), (120, 40));
    focus_clock_button(&mut state);
    state.notify_modal(
        "First",
        "First message",
        NotificationTone::Info,
        vec![ShellNotificationAction::new("first", "First")],
    );
    state.notify_modal(
        "Second",
        "Second message",
        NotificationTone::Info,
        vec![ShellNotificationAction::new("second", "Second")],
    );

    state.apply_input(InputEvent::from_key_label("Enter"));
    assert_eq!(
        state.focused_component(),
        ShellComponent::NotificationDialog
    );
    assert_eq!(
        state
            .to_notification_view_model()
            .as_ref()
            .map(|model| model.title.as_str()),
        Some("Second")
    );

    state.apply_input(InputEvent::from_key_label("Enter"));
    assert_eq!(state.focused_component(), ShellComponent::ClockButton);
    assert!(state.to_notification_view_model().is_none());
}

#[test]
fn modal_follow_up_screen_change_does_not_restore_previous_focus() {
    let mut state = ShellState::new(debug_config(), (120, 40));
    focus_clock_button(&mut state);
    state.notify_modal(
        "Open Clock",
        "Open the clock now?",
        NotificationTone::Info,
        vec![ShellNotificationAction::new("open", "Open").with_follow_up(ShellCommand::OpenClock)],
    );

    state.apply_input(InputEvent::from_key_label("Enter"));

    assert_eq!(state.active_screen(), ShellScreen::Clock);
    assert_eq!(state.focused_component(), ShellComponent::Clock);
    assert_ne!(state.focused_component(), ShellComponent::ClockButton);
}

#[test]
fn modal_follow_up_focus_change_is_not_overwritten_by_focus_restore() {
    let mut state = ShellState::new(debug_config(), (120, 40));
    focus_clock_button(&mut state);
    state.notify_modal(
        "Move Focus",
        "Move to the next component?",
        NotificationTone::Info,
        vec![ShellNotificationAction::new("next", "Next").with_follow_up(ShellCommand::FocusNext)],
    );

    state.apply_input(InputEvent::from_key_label("Enter"));

    assert_eq!(state.active_screen(), ShellScreen::Home);
    assert_eq!(state.focused_component(), ShellComponent::StatusBar);
}

#[test]
fn time_sync_modal_close_and_programmatic_dismiss_restore_previous_focus() {
    let mut closed = ShellState::new(debug_config(), (120, 40));
    focus_clock_button(&mut closed);
    closed.apply_time_sync_failure_for_test("Time sync unavailable");
    closed.apply_input(InputEvent::from_key_label("Esc"));
    assert_eq!(closed.focused_component(), ShellComponent::ClockButton);

    let mut dismissed = ShellState::new(debug_config(), (120, 40));
    focus_clock_button(&mut dismissed);
    dismissed.apply_time_sync_failure_for_test("Time sync unavailable");
    dismissed.apply_time_sync_utc_for_test(
        Utc.with_ymd_and_hms(2026, 7, 10, 0, 0, 0)
            .single()
            .expect("valid UTC timestamp"),
    );
    assert_eq!(dismissed.focused_component(), ShellComponent::ClockButton);
}

#[test]
fn modal_notification_records_response_and_runs_follow_up_command() {
    let mut state = ShellState::new(debug_config(), (120, 40));
    let id = state.notify_modal(
        "Open Clock",
        "Open the clock now?",
        NotificationTone::Info,
        vec![
            ShellNotificationAction::new("open", "Open").with_follow_up(ShellCommand::OpenClock),
            ShellNotificationAction::new("cancel", "Cancel").cancel(),
        ],
    );

    assert_eq!(
        state
            .to_notification_view_model()
            .as_ref()
            .map(|model| model.title.as_str()),
        Some("Open Clock")
    );

    let action = state.apply_input(InputEvent::from_key_label("Enter"));

    assert_eq!(action, ShellAction::Redraw);
    assert_eq!(state.active_screen(), ShellScreen::Clock);
    assert_eq!(state.last_command(), Some(&ShellCommand::OpenClock));
    assert_eq!(
        state.take_notification_response(),
        Some(tundra_shell::ShellNotificationResponse {
            notification_id: id,
            action_id: "open".to_string(),
        })
    );
    assert!(state.to_notification_view_model().is_none());
}

#[test]
fn modal_notification_captures_input_and_escape_uses_cancel_action() {
    let mut state = ShellState::new(debug_config(), (120, 40));
    let id = state.notify_modal(
        "Confirm",
        "Continue?",
        NotificationTone::Warning,
        vec![
            ShellNotificationAction::new("continue", "Continue"),
            ShellNotificationAction::new("cancel", "Cancel").cancel(),
        ],
    );

    state.apply_input(InputEvent::from_key_label("e"));
    assert_eq!(state.active_screen(), ShellScreen::Home);
    assert!(state.to_notification_view_model().is_some());
    assert_eq!(
        state.last_command(),
        Some(&ShellCommand::CaptureOverlayInput)
    );

    state.apply_input(InputEvent::from_key_label("Tab"));
    assert_eq!(
        state
            .to_notification_view_model()
            .expect("active modal")
            .actions
            .iter()
            .position(|action| action.selected),
        Some(1)
    );
    state.apply_input(InputEvent::from_key_label("Shift+Tab"));
    assert_eq!(
        state
            .to_notification_view_model()
            .expect("active modal")
            .actions
            .iter()
            .position(|action| action.selected),
        Some(0)
    );

    state.apply_input(InputEvent::from_key_label("Esc"));

    assert_eq!(
        state.take_notification_response(),
        Some(tundra_shell::ShellNotificationResponse {
            notification_id: id,
            action_id: "cancel".to_string(),
        })
    );
    assert!(state.to_notification_view_model().is_none());
}

#[test]
fn modal_notifications_are_fifo_and_low_priority_notifications_do_not_interrupt() {
    let mut state = ShellState::new(debug_config(), (120, 40));
    let first = state.notify_modal(
        "First",
        "First modal",
        NotificationTone::Info,
        vec![ShellNotificationAction::new("first-ok", "OK")],
    );
    let second = state.notify_modal(
        "Second",
        "Second modal",
        NotificationTone::Info,
        vec![ShellNotificationAction::new("second-ok", "OK")],
    );

    state.notify_status("Background work");
    state.notify_toast("Saved in background");
    state.notify_alert("Background warning");

    let chrome = state.to_shell_chrome_view_model();
    assert!(chrome.status.status.starts_with("Background work"));
    assert_eq!(chrome.status.toast.as_deref(), Some("Saved in background"));
    assert_eq!(chrome.status.error.as_deref(), Some("Background warning"));
    assert_eq!(
        state
            .to_notification_view_model()
            .as_ref()
            .map(|model| model.title.as_str()),
        Some("First")
    );

    state.apply_input(InputEvent::from_key_label(" "));
    assert_eq!(
        state.take_notification_response(),
        Some(tundra_shell::ShellNotificationResponse {
            notification_id: first,
            action_id: "first-ok".to_string(),
        })
    );
    assert_eq!(
        state
            .to_notification_view_model()
            .as_ref()
            .map(|model| model.title.as_str()),
        Some("Second")
    );

    state.apply_input(InputEvent::from_key_label("Enter"));
    assert_eq!(
        state.take_notification_response(),
        Some(tundra_shell::ShellNotificationResponse {
            notification_id: second,
            action_id: "second-ok".to_string(),
        })
    );
    assert!(state.to_notification_view_model().is_none());
}

#[test]
fn user_state_builds_user_home_view_model() {
    let state =
        ShellState::new_for_home_mode(build_default_config(), (120, 40), ShellHomeMode::User);

    let home = state.to_home_view_model();

    assert_eq!(home.display_mode(), HomeDisplayMode::User);
    assert_eq!(home.diagnostics(), None);
    assert_eq!(home.entries().len(), 5);
    assert!(
        home.entries()
            .iter()
            .any(|entry| entry.label == "Diagnostics")
    );
}

#[test]
fn current_time_label_uses_configured_timezone_datetime() {
    let fixture = FixtureRoot::new("clock-timezone");
    let opened = storage_manager_at(&fixture);
    let mut config = opened.load_config().expect("config loads");
    config.timezone = "Asia/Shanghai".to_string();
    opened.save_config(&config).expect("config saves");
    let mut startup = ShellStartupState::clean(
        PlatformKind::Windows,
        PlatformCapabilities::native_supported(),
    );
    startup.storage_manager = Some(opened);
    let mut state = ShellState::new_with_startup(build_default_config(), (120, 40), startup);
    let utc = Utc
        .with_ymd_and_hms(2026, 7, 9, 15, 30, 0)
        .single()
        .unwrap();

    state.apply_time_sync_utc_for_test(utc);

    assert_eq!(state.current_time_label(), "2026-07-09 23:30");
    assert_eq!(state.to_clock_view_model().current_time, "2026-07-09 23:30");
}

#[test]
fn clock_button_click_toggles_clock_placeholder() {
    let mut state =
        ShellState::new_for_home_mode(build_default_config(), (120, 40), ShellHomeMode::User);
    let coordinates = component_coordinates(&state, ShellComponent::ClockButton);

    assert_eq!(
        state.hit_target_at(coordinates),
        Some(ShellComponent::ClockButton)
    );

    state.apply_input(InputEvent::mouse_down(PointerButton::Left, coordinates));

    assert_eq!(state.active_screen(), ShellScreen::Clock);
    assert_eq!(state.focused_component(), ShellComponent::Clock);
    assert_eq!(state.last_command(), Some(&ShellCommand::OpenClock));

    let coordinates = component_coordinates(&state, ShellComponent::ClockButton);
    state.apply_input(InputEvent::mouse_down(PointerButton::Left, coordinates));

    assert_eq!(state.active_screen(), ShellScreen::Home);
    assert_eq!(state.focused_component(), ShellComponent::Home);
    assert_eq!(state.last_command(), Some(&ShellCommand::CloseClock));
}

#[test]
fn keyboard_activation_opens_clock_and_escape_closes_it() {
    let mut state =
        ShellState::new_for_home_mode(build_default_config(), (120, 40), ShellHomeMode::User);

    state.apply_input(InputEvent::from_key_label("Tab"));
    assert_eq!(state.focused_component(), ShellComponent::ClockButton);

    state.apply_input(InputEvent::from_key_label("Enter"));
    assert_eq!(state.active_screen(), ShellScreen::Clock);
    assert_eq!(state.focused_component(), ShellComponent::Clock);
    assert_eq!(state.last_command(), Some(&ShellCommand::OpenClock));

    state.apply_input(InputEvent::from_key_label("Esc"));
    assert_eq!(state.active_screen(), ShellScreen::Home);
    assert_eq!(state.last_command(), Some(&ShellCommand::CloseClock));
}

#[test]
fn time_sync_failure_helper_shows_and_closes_dialog() {
    let mut state = ShellState::new(debug_config(), (120, 40));

    state.apply_time_sync_failure_for_test("联网校准时间失败");

    assert!(state.time_sync_failure_dialog_visible());
    assert_eq!(state.time_sync_failure_message(), Some("联网校准时间失败"));
    assert_eq!(state.status(), "联网校准时间失败");
    assert_eq!(state.focused_component(), ShellComponent::TimeSyncDialog);
    assert!(state.to_time_sync_dialog_view_model().is_some());

    state.apply_input(InputEvent::from_key_label("Esc"));

    assert!(!state.time_sync_failure_dialog_visible());
    assert_eq!(state.time_sync_failure_message(), None);
    assert_eq!(
        state.last_command(),
        Some(&ShellCommand::CloseTimeSyncDialog)
    );
}

#[test]
fn successful_time_sync_clears_failure_dialog_and_updates_anchor() {
    let mut state = ShellState::new(debug_config(), (120, 40));
    let utc = Utc
        .with_ymd_and_hms(2026, 7, 9, 15, 30, 0)
        .single()
        .unwrap();

    state.apply_time_sync_failure_for_test("联网校准时间失败");
    state.apply_time_sync_utc_for_test(utc);

    assert!(!state.time_sync_failure_dialog_visible());
    assert_eq!(state.current_time_label(), "2026-07-09 15:30");
}

#[test]
fn explicit_user_mode_shows_product_entries_without_diagnostics() {
    let config = ShellLaunchConfig {
        terminal_mode: ShellTerminalMode::Fullscreen,
        home_mode_override: HomeModeOverride::BuildDefault,
    };
    let state = ShellState::new_for_home_mode(config, (120, 35), ShellHomeMode::User);

    let home = state.to_home_view_model();

    assert_eq!(home.display_mode(), HomeDisplayMode::User);
    assert_eq!(home.diagnostics(), None);
    let labels: Vec<_> = home
        .entries()
        .iter()
        .map(|entry| entry.label.as_str())
        .collect();
    assert_eq!(
        labels,
        vec!["Explorer", "Launcher", "Editor", "Settings", "Diagnostics"]
    );
}

#[test]
fn home_arrow_keys_update_selected_entry() {
    let mut state =
        ShellState::new_for_home_mode(build_default_config(), (120, 40), ShellHomeMode::User);

    assert_eq!(state.selected_home_entry_index(), 0);

    state.apply_input(InputEvent::from_key_label("Right"));
    assert_eq!(state.selected_home_entry_index(), 1);

    state.apply_input(InputEvent::from_key_label("End"));
    assert_eq!(state.selected_home_entry_index(), 4);
}

#[test]
fn enter_on_home_explorer_entry_routes_activation() {
    let mut state =
        ShellState::new_for_home_mode(build_default_config(), (120, 40), ShellHomeMode::User);

    state.apply_input(InputEvent::from_key_label("Enter"));

    assert_eq!(
        state.last_command(),
        Some(&ShellCommand::ActivateSelectedHomeEntry)
    );
    assert_eq!(state.status(), "Ready");
}

#[test]
fn enter_on_unimplemented_home_entry_shows_placeholder_status() {
    let mut state =
        ShellState::new_for_home_mode(build_default_config(), (120, 40), ShellHomeMode::User);

    state.apply_input(InputEvent::from_key_label("Right"));
    state.apply_input(InputEvent::from_key_label("Enter"));

    assert_eq!(
        state.last_command(),
        Some(&ShellCommand::ActivateSelectedHomeEntry)
    );
    assert_eq!(state.status(), "Launcher is not implemented yet");
}

#[test]
fn mouse_single_click_on_home_entry_selects_that_entry() {
    let mut state =
        ShellState::new_for_home_mode(build_default_config(), (120, 40), ShellHomeMode::User);
    let launcher = home_entry_coordinates(&state, 1);

    state.apply_input(InputEvent::mouse_down(PointerButton::Left, launcher));

    assert_eq!(state.selected_home_entry_index(), 1);
    assert_eq!(
        state.last_command(),
        Some(&ShellCommand::ActivateHomeEntryAt(
            launcher,
            ClickKind::Single
        ))
    );
    assert_eq!(state.status(), "Home: Launcher");
}

#[test]
fn mouse_double_click_on_unimplemented_home_entry_shows_placeholder_status() {
    let mut state =
        ShellState::new_for_home_mode(build_default_config(), (120, 40), ShellHomeMode::User);
    let launcher = home_entry_coordinates(&state, 1);

    state.apply_input(InputEvent::mouse_down(PointerButton::Left, launcher));
    state.apply_input(InputEvent::mouse_down(PointerButton::Left, launcher));

    assert_eq!(state.selected_home_entry_index(), 1);
    assert_eq!(
        state.last_command(),
        Some(&ShellCommand::ActivateHomeEntryAt(
            launcher,
            ClickKind::Double
        ))
    );
    assert_eq!(state.status(), "Launcher is not implemented yet");
}

#[test]
fn state_builds_shell_chrome_view_model() {
    let mut state = ShellState::new(debug_config(), (120, 40));
    state.apply_input(InputEvent::from_key_label("q"));

    let chrome = state.to_shell_chrome_view_model();

    assert_eq!(chrome.app_name, "TundraUX 3");
    assert_eq!(
        chrome.build_mode,
        if cfg!(debug_assertions) {
            "debug"
        } else {
            "release"
        }
    );
    assert_eq!(chrome.display_mode, HomeDisplayMode::Debug);
    assert_eq!(chrome.terminal_size, (120, 40));
    assert_eq!(
        chrome.screen_stack,
        vec!["Home".to_string(), "ExitConfirm".to_string()]
    );
    assert_eq!(chrome.status.status, "Confirm exit");
    assert_eq!(chrome.status.toast, None);
    assert_eq!(chrome.status.error, None);
}

#[test]
fn new_with_startup_clean_storage_starts_ready_without_toast() {
    let startup = ShellStartupState::clean(
        PlatformKind::Windows,
        PlatformCapabilities::native_supported(),
    );

    let state = ShellState::new_with_startup(debug_config(), (120, 40), startup);
    let chrome = state.to_shell_chrome_view_model();

    assert_eq!(state.status(), "Ready");
    assert_eq!(chrome.status.toast, None);
    assert_eq!(chrome.status.error, None);
}

#[test]
fn new_with_startup_recovery_warning_surfaces_toast() {
    let mut startup = ShellStartupState::clean(
        PlatformKind::Windows,
        PlatformCapabilities::native_supported(),
    );
    startup.storage_report = ShellStorageReport::recovered_defaults(None);

    let state = ShellState::new_with_startup(debug_config(), (120, 40), startup);
    let chrome = state.to_shell_chrome_view_model();

    assert_eq!(state.status(), "Ready");
    assert_eq!(
        chrome.status.toast.as_deref(),
        Some("Storage recovered defaults")
    );
}

#[test]
fn debug_diagnostics_use_injected_platform_summary() {
    let mut capabilities = PlatformCapabilities::unsupported();
    capabilities.app_paths = CapabilityStatus::Supported;
    let startup = ShellStartupState::clean(PlatformKind::Macos, capabilities);

    let state = ShellState::new_with_startup(debug_config(), (120, 40), startup);
    let home = state.to_home_view_model();

    let diagnostics = home.diagnostics().expect("debug diagnostics");
    assert_eq!(
        diagnostics.platform_capability_summary,
        "macOS: 1 supported, 0 best-effort, 12 unsupported"
    );
}

#[test]
fn debug_override_wins_over_persisted_config_and_session() {
    let startup = ShellStartupState {
        app_config: ShellAppConfig {
            home_mode: Some(ShellHomeMode::User),
        },
        storage_report: ShellStorageReport::default(),
        platform_kind: PlatformKind::Windows,
        platform_capabilities: PlatformCapabilities::native_supported(),
        restored_session: Some(ShellRestoredSession::new(
            ShellHomeMode::User,
            ShellComponent::StatusBar,
        )),
        storage_manager: None,
        auth_bootstrap_required: false,
        login_users: Vec::new(),
        debug_policy: tundra_core::DebugPolicy::default(),
    };

    let state = ShellState::new_with_startup(debug_config(), (120, 40), startup);

    assert_eq!(state.home_mode(), ShellHomeMode::Debug);
}

#[test]
fn restored_session_is_sanitized_to_stable_home_state() {
    let startup = ShellStartupState {
        app_config: ShellAppConfig::default(),
        storage_report: ShellStorageReport::default(),
        platform_kind: PlatformKind::Windows,
        platform_capabilities: PlatformCapabilities::native_supported(),
        restored_session: Some(ShellRestoredSession {
            active_screen: ShellScreen::ExitConfirm,
            focused_component: ShellComponent::ExitDialog,
            display_mode: ShellHomeMode::User,
            active_popup: Some(tundra_shell::ShellPopup {
                owner: Some(ShellComponent::Home),
                anchor: (4, 4),
            }),
        }),
        storage_manager: None,
        auth_bootstrap_required: false,
        login_users: Vec::new(),
        debug_policy: tundra_core::DebugPolicy::default(),
    };

    let state = ShellState::new_with_startup(build_default_config(), (120, 40), startup);
    let saved = state.sanitized_session_state();

    assert_eq!(state.home_mode(), ShellHomeMode::User);
    assert_eq!(state.active_screen(), ShellScreen::Home);
    assert_eq!(state.screen_stack(), &[ShellScreen::Home][..]);
    assert_eq!(state.focused_component(), ShellComponent::Home);
    assert_eq!(state.active_popup(), None);
    assert_eq!(saved.active_screen, ShellScreen::Home);
    assert_eq!(saved.focused_component, ShellComponent::Home);
    assert_eq!(saved.display_mode, ShellHomeMode::User);
    assert_eq!(saved.active_popup, None);
}

fn home_entry_coordinates(state: &ShellState, index: usize) -> (u16, u16) {
    let area = Rect::new(0, 0, state.terminal_size().0, state.terminal_size().1);
    let tundra_ui::ShellLayout::Full { main, .. } = tundra_ui::compute_shell_layout(area) else {
        panic!("state tests use a full shell layout");
    };
    let tile = tundra_ui::home_entry_tile_areas(main, state.to_home_view_model().entries().len())
        .get(index)
        .copied()
        .unwrap_or_else(|| panic!("missing home entry tile at index {index}"));

    (tile.x.saturating_add(1), tile.y.saturating_add(1))
}

fn component_coordinates(state: &ShellState, component: ShellComponent) -> (u16, u16) {
    let region = state
        .hit_map()
        .regions()
        .iter()
        .find(|region| region.component == component)
        .unwrap_or_else(|| panic!("missing hit region for {component:?}"));

    (
        region
            .area
            .x
            .saturating_add(region.area.width.saturating_sub(1)),
        region.area.y,
    )
}

fn key_input(key: InputKey, modifiers: InputModifiers, phase: InputPhase) -> InputEvent {
    InputEvent::Key(KeyInput::new(key, modifiers, phase))
}

fn selected_notification_action(state: &ShellState) -> Option<usize> {
    state
        .to_notification_view_model()?
        .actions
        .iter()
        .position(|action| action.selected)
}

fn state_with_two_action_modal() -> (ShellState, u64) {
    let mut state = ShellState::new(debug_config(), (120, 40));
    let id = state.notify_modal(
        "Confirm",
        "Continue?",
        NotificationTone::Warning,
        vec![
            ShellNotificationAction::new("confirm", "Confirm"),
            ShellNotificationAction::new("cancel", "Cancel").cancel(),
        ],
    );
    (state, id)
}

fn notification_dialog_layout(
    state: &ShellState,
    area: Rect,
) -> tundra_ui::NotificationDialogLayout {
    let model = state
        .to_notification_view_model()
        .expect("active notification view model");
    match tundra_ui::notification_layout(area, &model) {
        NotificationLayout::Dialog(layout) => layout,
        NotificationLayout::TooSmall {
            required_width,
            required_height,
        } => panic!(
            "notification layout unexpectedly requires {required_width}x{required_height} in {area:?}"
        ),
    }
}

fn notification_action_coordinates(state: &ShellState, index: usize) -> (u16, u16) {
    notification_action_coordinates_in(
        state,
        Rect::new(0, 0, state.terminal_size().0, state.terminal_size().1),
        index,
    )
}

fn notification_action_coordinates_in(state: &ShellState, area: Rect, index: usize) -> (u16, u16) {
    let layout = notification_dialog_layout(state, area);
    let action = layout
        .actions
        .iter()
        .find(|action| action.index == index)
        .unwrap_or_else(|| panic!("missing notification action layout at index {index}"));
    rect_center(action.area)
}

fn rect_center(area: Rect) -> (u16, u16) {
    (
        area.x.saturating_add(area.width / 2),
        area.y.saturating_add(area.height / 2),
    )
}

fn focus_clock_button(state: &mut ShellState) {
    state.apply_input(InputEvent::from_key_label("Tab"));
    assert_eq!(state.focused_component(), ShellComponent::ClockButton);
}

fn storage_manager_at(fixture: &FixtureRoot) -> StorageManager {
    let base = fixture.path();
    let app_paths =
        build_windows_app_paths(base.join("Roaming"), base.join("Local"), base.join("Temp"))
            .expect("valid fixture app paths");
    StorageManager::open(app_paths)
        .expect("storage opens")
        .manager
}

struct FixtureRoot {
    path: PathBuf,
}

impl FixtureRoot {
    fn new(name: &str) -> Self {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "tundra-shell-state-{name}-{}-{nanos}",
            std::process::id()
        ));
        assert!(path.is_absolute());
        let _ = fs::remove_dir_all(&path);
        Self { path }
    }

    fn path(&self) -> &PathBuf {
        &self.path
    }
}

impl Drop for FixtureRoot {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}
