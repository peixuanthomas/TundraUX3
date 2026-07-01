use tundra_shell::{
    HomeModeOverride, ShellAction, ShellHomeMode, ShellInput, ShellLaunchConfig, ShellScreen,
    ShellState, ShellTerminalMode,
};

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
