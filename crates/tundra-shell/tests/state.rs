use tundra_shell::{
    HomeModeOverride, ShellAction, ShellHomeMode, ShellInput, ShellLaunchConfig, ShellScreen,
    ShellState, ShellTerminalMode,
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

    let action = state.apply_input(ShellInput::Key("q".to_string()));

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

    let action = state.apply_input(ShellInput::Key("Esc".to_string()));

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
    state.apply_input(ShellInput::Key("q".to_string()));

    let action = state.apply_input(ShellInput::Key("Esc".to_string()));

    assert_eq!(action, ShellAction::Redraw);
    assert_eq!(state.active_screen(), ShellScreen::Home);
    assert_eq!(state.status(), "Ready");
    assert_eq!(state.screen_stack(), &[ShellScreen::Home][..]);
    assert!(!state.shutdown_requested());
}

#[test]
fn enter_confirms_exit_confirmation() {
    let mut state = ShellState::new(debug_config(), (120, 40));
    state.apply_input(ShellInput::Key("q".to_string()));

    let action = state.apply_input(ShellInput::Key("Enter".to_string()));

    assert_eq!(action, ShellAction::Exit);
    assert_eq!(state.active_screen(), ShellScreen::ExitConfirm);
    assert!(state.shutdown_requested());
}

#[test]
fn y_and_uppercase_y_confirm_exit_confirmation() {
    for key in ["y", "Y"] {
        let mut state = ShellState::new(debug_config(), (120, 40));
        state.apply_input(ShellInput::Key("q".to_string()));

        let action = state.apply_input(ShellInput::Key(key.to_string()));

        assert_eq!(action, ShellAction::Exit);
        assert_eq!(state.active_screen(), ShellScreen::ExitConfirm);
        assert!(state.shutdown_requested());
    }
}

#[test]
fn n_and_uppercase_n_cancel_exit_confirmation() {
    for key in ["n", "N"] {
        let mut state = ShellState::new(debug_config(), (120, 40));
        state.apply_input(ShellInput::Key("q".to_string()));

        let action = state.apply_input(ShellInput::Key(key.to_string()));

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

    let action = state.apply_input(ShellInput::Key("Tab".to_string()));

    assert_eq!(action, ShellAction::Redraw);
    assert_eq!(state.active_screen(), ShellScreen::Home);
    assert_eq!(state.last_key_event(), Some("Tab"));
}

#[test]
fn mouse_and_resize_events_are_recorded() {
    let mut state = ShellState::new(debug_config(), (120, 40));

    let mouse_action = state.apply_input(ShellInput::Mouse {
        summary: "left press".to_string(),
        coordinates: Some((12, 7)),
        scroll_direction: Some("down".to_string()),
    });
    let resize_action = state.apply_input(ShellInput::Resize {
        width: 80,
        height: 24,
    });

    assert_eq!(mouse_action, ShellAction::Redraw);
    assert_eq!(resize_action, ShellAction::Redraw);
    assert_eq!(state.last_mouse_event(), Some("left press"));
    assert_eq!(state.mouse_coordinates(), Some((12, 7)));
    assert_eq!(state.mouse_scroll_direction(), Some("down"));
    assert_eq!(state.terminal_size(), (80, 24));
    assert_eq!(state.last_resize_event(), Some("80x24"));
}

#[test]
fn tick_increments_count() {
    let mut state = ShellState::new(debug_config(), (120, 40));

    let first_action = state.apply_input(ShellInput::Tick);
    let second_action = state.apply_input(ShellInput::Tick);

    assert_eq!(first_action, ShellAction::Redraw);
    assert_eq!(second_action, ShellAction::Redraw);
    assert_eq!(state.tick_count(), 2);
}

#[test]
fn shutdown_input_exits_immediately() {
    let mut state = ShellState::new(debug_config(), (120, 40));

    let action = state.apply_input(ShellInput::Shutdown);

    assert_eq!(action, ShellAction::Exit);
    assert!(state.shutdown_requested());
}

#[test]
fn debug_state_builds_debug_home_view_model() {
    let mut state = ShellState::new(debug_config(), (120, 40));
    state.apply_input(ShellInput::Key("x".to_string()));
    state.apply_input(ShellInput::Tick);

    let home = state.to_home_view_model();

    assert_eq!(home.display_mode(), HomeDisplayMode::Debug);
    let diagnostics = home.diagnostics().expect("debug diagnostics");
    assert_eq!(diagnostics.tick_count, 1);
    assert_eq!(diagnostics.last_key_event.as_deref(), Some("x"));
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
    state.apply_input(ShellInput::Key("q".to_string()));

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
