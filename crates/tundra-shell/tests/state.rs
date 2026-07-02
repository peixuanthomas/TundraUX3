use std::time::{Duration, Instant};

use tundra_platform::{CapabilityStatus, PlatformCapabilities, PlatformKind};
use tundra_shell::{
    ClickKind, HomeModeOverride, InputEvent, PointerButton, ScrollDirection, ShellAction,
    ShellAppConfig, ShellCommand, ShellComponent, ShellHomeMode, ShellLaunchConfig,
    ShellRestoredSession, ShellScreen, ShellStartupState, ShellState, ShellStorageReport,
    ShellTerminalMode, default_shell_shortcuts, detect_shortcut_conflicts,
};
use tundra_ui::HomeDisplayMode;

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
    assert_eq!(state.focused_component(), ShellComponent::StatusBar);
    assert_eq!(state.last_command(), Some(&ShellCommand::FocusNext));

    state.apply_input(InputEvent::from_key_label("Shift+Tab"));
    assert_eq!(state.focused_component(), ShellComponent::Home);
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
        ShellCommand::Activate {
            target: ShellComponent::Home,
            coordinates: (11, 6),
            click: ClickKind::Double,
        }
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
