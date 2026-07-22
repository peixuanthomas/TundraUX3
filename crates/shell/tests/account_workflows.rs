use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use chrono::Utc;
use platform::mock::MockPlatform;
use platform::{PlatformCapabilities, PlatformKind, UserDirs, build_windows_app_paths};
use shell::{
    HomeModeOverride, InputEvent, InputKey, InputModifiers, InputPhase, KeyInput,
    LOGIN_IDLE_TIMEOUT, PASSWORD_REVEAL_DURATION, PointerButton, ShellAction, ShellComponent,
    ShellHomeMode, ShellLaunchConfig, ShellLaunchTarget, ShellScreen, ShellSession,
    ShellTerminalMode, prepare_shell_startup,
};
use storage::{BorderColor, BorderShape, ClockEntryRecord, ClockProfile};
use ui::NotificationTone;

fn debug_config() -> ShellLaunchConfig {
    ShellLaunchConfig {
        terminal_mode: ShellTerminalMode::Fullscreen,
        home_mode_override: HomeModeOverride::Debug,
        launch_target: ShellLaunchTarget::Home,
    }
}

fn default_config() -> ShellLaunchConfig {
    ShellLaunchConfig {
        terminal_mode: ShellTerminalMode::Fullscreen,
        home_mode_override: HomeModeOverride::BuildDefault,
        launch_target: ShellLaunchTarget::Home,
    }
}

#[test]
fn fresh_startup_requires_first_run_setup_and_debug_flag_does_not_bypass_auth() {
    let fixture = FixtureRoot::new("fresh-bootstrap");
    let platform = mock_platform(fixture.path());
    let startup = prepare_shell_startup(&platform, debug_config()).expect("startup");

    let state = ShellSession::new_with_startup(debug_config(), (120, 40), startup);

    assert_eq!(state.active_screen(), ShellScreen::FirstRunSetup);
    assert_eq!(state.auth_session(), None);
    assert_eq!(state.to_setup_view_model().step, ui::SetupStep::Language);
}

#[test]
fn profile_default_home_mode_survives_admin_authentication() {
    let fixture = FixtureRoot::new("profile-default-home");
    let platform = mock_platform(fixture.path());
    let launch_config = ShellLaunchConfig::default();
    let startup = prepare_shell_startup(&platform, launch_config).expect("startup");
    let mut state = ShellSession::new_with_startup(launch_config, (120, 40), startup);

    complete_first_run_setup(&mut state, 0, 0, "AdminUser", "StrongPass123", "");

    let expected = if cfg!(debug_assertions) {
        ShellHomeMode::Debug
    } else {
        ShellHomeMode::User
    };
    assert_eq!(state.active_screen(), ShellScreen::Home);
    assert_eq!(state.home_mode(), expected);
}

#[test]
fn first_run_setup_signs_in_persists_config_and_hint_without_plaintext_password() {
    let fixture = FixtureRoot::new("first-run-setup");
    let platform = mock_platform(fixture.path());
    let startup = prepare_shell_startup(&platform, default_config()).expect("startup");
    let manager = startup.storage_manager.clone().expect("storage manager");
    let mut state = ShellSession::new_with_startup(default_config(), (120, 40), startup);

    let expected_language = setup_language_code(&ui::setup_language_options(), 0);
    let expected_timezone = setup_timezone_id(&ui::setup_timezone_options(), 2);

    complete_first_run_setup(&mut state, 0, 2, "AdminUser", "StrongPass123", "First pet");

    assert_eq!(state.active_screen(), ShellScreen::Home);
    assert_eq!(
        state
            .auth_session()
            .map(|session| session.username.as_str()),
        Some("AdminUser")
    );
    let config = manager.load_config().expect("config");
    assert_eq!(config.language, expected_language);
    assert_eq!(config.timezone, expected_timezone);
    let stored = fs::read_to_string(&manager.layout().users_path).expect("users file");
    assert!(!stored.contains("StrongPass123"));
    assert!(stored.contains("First pet"));
    assert!(stored.contains("$argon2"));
}

#[test]
fn appearance_setup_validates_custom_colors_and_persists_all_choices() {
    let fixture = FixtureRoot::new("first-run-appearance");
    let platform = mock_platform(fixture.path());
    let startup = prepare_shell_startup(&platform, default_config()).expect("startup");
    let manager = startup.storage_manager.clone().expect("storage manager");
    let mut state = ShellSession::new_with_startup(default_config(), (120, 40), startup);

    complete_first_run_admin(&mut state, 0, 0, "AdminUser", "StrongPass123", "");
    assert_eq!(
        setup_hit_components(&state),
        vec![
            ShellComponent::SetupAppearanceShape,
            ShellComponent::SetupAppearanceThemeColor,
            ShellComponent::SetupAppearanceThemeCustom,
            ShellComponent::SetupAppearanceAccentColor,
            ShellComponent::SetupAppearanceAccentCustom,
            ShellComponent::SetupAppearanceSubmit,
        ]
    );

    state.apply_input(InputEvent::from_key_label("Right"));
    assert_eq!(
        state.to_setup_view_model().border_shape,
        ui::BorderShape::Square
    );

    state.apply_input(InputEvent::from_key_label("Tab"));
    state.apply_input(InputEvent::from_key_label("Tab"));
    state.apply_input(InputEvent::from_key_label("Enter"));
    assert_eq!(
        state.focused_component(),
        ShellComponent::SetupCustomColorDialog
    );
    for _ in 0..5 {
        state.apply_input(InputEvent::from_key_label("Backspace"));
    }
    type_text(&mut state, "#38BDF8");
    assert!(state.to_setup_view_model().custom_color_valid);
    state.apply_input(InputEvent::from_key_label("Enter"));
    assert_eq!(state.to_setup_view_model().theme_color_value, "#38BDF8");

    state.apply_input(InputEvent::from_key_label("Tab"));
    state.apply_input(InputEvent::from_key_label("Tab"));
    state.apply_input(InputEvent::from_key_label("Enter"));
    for _ in 0..4 {
        state.apply_input(InputEvent::from_key_label("Backspace"));
    }
    type_text(&mut state, "#12GG00");
    assert!(!state.to_setup_view_model().custom_color_valid);
    state.apply_input(InputEvent::from_key_label("Enter"));
    assert_eq!(
        state.focused_component(),
        ShellComponent::SetupCustomColorDialog
    );
    assert_eq!(
        state.to_setup_view_model().custom_color_error.as_deref(),
        Some("Invalid color. Use #RRGGBB or a supported color name.")
    );
    for _ in 0..7 {
        state.apply_input(InputEvent::from_key_label("Backspace"));
    }
    type_text(&mut state, "#AABBCC");
    state.apply_input(InputEvent::from_key_label("Enter"));
    state.apply_input(InputEvent::from_key_label("Tab"));
    assert_eq!(
        state.focused_component(),
        ShellComponent::SetupAppearanceSubmit
    );
    state.apply_input(InputEvent::from_key_label("Enter"));

    assert_eq!(state.active_screen(), ShellScreen::Home);
    let users = manager.load_users().expect("users");
    let admin = users
        .users
        .iter()
        .find(|user| user.username == "AdminUser")
        .expect("admin user");
    assert_eq!(admin.appearance.border_shape, BorderShape::Square);
    assert_eq!(
        admin.appearance.border_color,
        BorderColor::Rgb(0x38, 0xBD, 0xF8)
    );
    assert_eq!(
        admin.appearance.accent_color,
        BorderColor::Rgb(0xAA, 0xBB, 0xCC)
    );
}

#[test]
fn appearance_setup_prevents_matching_theme_and_accent_colors() {
    let (_fixture, mut state) = fresh_setup_state("first-run-distinct-colors");

    complete_first_run_admin(&mut state, 0, 0, "AdminUser", "StrongPass123", "");

    state.apply_input(InputEvent::from_key_label("Tab"));
    assert_eq!(
        state.focused_component(),
        ShellComponent::SetupAppearanceThemeColor
    );
    state.apply_input(InputEvent::from_key_label("Right"));
    let model = state.to_setup_view_model();
    assert_eq!(model.theme_color_value, "cyan");
    assert_eq!(model.accent_color_value, "white");

    state.apply_input(InputEvent::from_key_label("Tab"));
    state.apply_input(InputEvent::from_key_label("Tab"));
    assert_eq!(
        state.focused_component(),
        ShellComponent::SetupAppearanceAccentColor
    );
    state.apply_input(InputEvent::from_key_label("Right"));
    assert_eq!(state.to_setup_view_model().accent_color_value, "blue");

    state.apply_input(InputEvent::from_key_label("Tab"));
    state.apply_input(InputEvent::from_key_label("Enter"));
    assert_eq!(
        state.focused_component(),
        ShellComponent::SetupCustomColorDialog
    );
    for _ in 0..4 {
        state.apply_input(InputEvent::from_key_label("Backspace"));
    }
    type_text(&mut state, "cyan");
    let model = state.to_setup_view_model();
    assert!(model.custom_color_conflicts_with_theme);
    assert!(!model.custom_color_valid);

    state.apply_input(InputEvent::from_key_label("Enter"));
    assert_eq!(
        state.focused_component(),
        ShellComponent::SetupCustomColorDialog
    );
    assert_eq!(
        state.to_setup_view_model().custom_color_error.as_deref(),
        Some("Accent color must be different from the theme color.")
    );
}

#[test]
fn first_run_setup_enter_advances_language_timezone_admin_pages() {
    let (_fixture, mut state) = fresh_setup_state("setup-page-transitions");

    assert_eq!(state.to_setup_view_model().step, ui::SetupStep::Language);
    assert_eq!(state.focused_component(), ShellComponent::SetupLanguage);
    assert_eq!(
        setup_hit_components(&state),
        vec![ShellComponent::SetupLanguage]
    );

    state.apply_input(InputEvent::from_key_label("Enter"));
    assert_eq!(state.to_setup_view_model().step, ui::SetupStep::Timezone);
    assert_eq!(state.focused_component(), ShellComponent::SetupTimezone);
    assert_eq!(
        setup_hit_components(&state),
        vec![ShellComponent::SetupTimezone]
    );

    state.apply_input(InputEvent::from_key_label("Enter"));
    assert_eq!(state.to_setup_view_model().step, ui::SetupStep::Admin);
    assert_eq!(
        state.focused_component(),
        ShellComponent::SetupAdminUsername
    );
    assert_eq!(
        setup_hit_components(&state),
        vec![
            ShellComponent::SetupAdminUsername,
            ShellComponent::SetupAdminPassword,
            ShellComponent::SetupAdminPasswordConfirm,
            ShellComponent::SetupAdminHint,
            ShellComponent::SetupSubmit,
        ]
    );
}

#[test]
fn timezone_setup_page_exposes_only_timezone_shell_targets() {
    let (_fixture, mut state) = fresh_setup_state("setup-timezone-targets");

    state.apply_input(InputEvent::from_key_label("Enter"));

    let model = state.to_setup_view_model();
    assert_eq!(model.step, ui::SetupStep::Timezone);
    assert_eq!(model.focused_field, ui::SetupField::TimezoneList);
    assert_setup_admin_empty(&state);

    let setup_components = setup_hit_components(&state);
    assert_eq!(setup_components, vec![ShellComponent::SetupTimezone]);
    assert!(!setup_components.contains(&ShellComponent::SetupLanguage));
    assert!(!setup_components.contains(&ShellComponent::SetupAdminUsername));
    assert!(!setup_components.contains(&ShellComponent::SetupAdminPassword));
    assert!(!setup_components.contains(&ShellComponent::SetupAdminPasswordConfirm));
    assert!(!setup_components.contains(&ShellComponent::SetupAdminHint));
    assert!(!setup_components.contains(&ShellComponent::SetupSubmit));
}

#[test]
fn inactive_setup_pages_do_not_edit_admin_fields_before_admin_step() {
    let (_fixture, mut state) = fresh_setup_state("setup-inactive-admin");

    type_text(&mut state, "AdminUser");
    state.apply_input(InputEvent::from_key_label("Backspace"));
    assert_eq!(state.to_setup_view_model().step, ui::SetupStep::Language);
    assert_setup_admin_empty(&state);

    state.apply_input(InputEvent::from_key_label("Enter"));
    assert_eq!(state.to_setup_view_model().step, ui::SetupStep::Timezone);

    type_text(&mut state, "StrongPass123");
    state.apply_input(InputEvent::from_key_label("Backspace"));
    state.apply_input(InputEvent::mouse_down(
        PointerButton::Left,
        setup_admin_coordinates(&state, ShellComponent::SetupAdminUsername),
    ));

    let model = state.to_setup_view_model();
    assert_eq!(model.step, ui::SetupStep::Timezone);
    assert_eq!(model.focused_field, ui::SetupField::TimezoneList);
    assert_eq!(state.focused_component(), ShellComponent::SetupTimezone);
    assert_setup_admin_empty(&state);
}

#[test]
fn timezone_keyboard_navigation_keeps_selected_timezone_visible() {
    let (_fixture, mut state) = fresh_setup_state("setup-timezone-scroll");

    state.apply_input(InputEvent::from_key_label("Enter"));
    assert_selected_timezone_visible(&state);

    for _ in 0..12 {
        state.apply_input(InputEvent::from_key_label("Down"));
        assert_selected_timezone_visible(&state);
    }

    for _ in 0..3 {
        state.apply_input(InputEvent::from_key_label("PageDown"));
        assert_selected_timezone_visible(&state);
    }

    state.apply_input(InputEvent::from_key_label("End"));
    assert_selected_timezone_visible(&state);
    assert_eq!(
        state.to_setup_view_model().selected_timezone_index,
        ui::setup_timezone_options().len().saturating_sub(1)
    );
}

#[test]
fn timezone_mouse_click_selects_visible_row_from_hit_map() {
    let (_fixture, mut state) = fresh_setup_state("setup-timezone-mouse");

    state.apply_input(InputEvent::from_key_label("Enter"));
    state.apply_input(InputEvent::from_key_label("PageDown"));
    assert_selected_timezone_visible(&state);

    let visible_rows = setup_hit_region_height(&state, ShellComponent::SetupTimezone);
    let clicked_row = visible_rows.min(4).saturating_sub(1);
    let before = state.to_setup_view_model();
    let expected_index = before.timezone_window_start.saturating_add(clicked_row);
    assert!(expected_index < before.timezones.len());

    let coordinates =
        setup_hit_map_row_coordinates(&state, ShellComponent::SetupTimezone, clicked_row as u16);
    assert_eq!(
        state.hit_target_at(coordinates),
        Some(ShellComponent::SetupTimezone)
    );

    state.apply_input(InputEvent::mouse_down(PointerButton::Left, coordinates));

    assert_eq!(
        state.to_setup_view_model().selected_timezone_index,
        expected_index
    );
    assert_selected_timezone_visible(&state);
}

#[test]
fn admin_setup_text_boxes_route_mouse_clicks_inside_box() {
    let (_fixture, mut state) = fresh_setup_state("setup-admin-text-box-mouse");

    state.apply_input(InputEvent::from_key_label("Enter"));
    state.apply_input(InputEvent::from_key_label("Enter"));
    assert_eq!(state.to_setup_view_model().step, ui::SetupStep::Admin);

    for component in [
        ShellComponent::SetupAdminUsername,
        ShellComponent::SetupAdminPassword,
        ShellComponent::SetupAdminPasswordConfirm,
        ShellComponent::SetupAdminHint,
    ] {
        assert_eq!(setup_hit_region_height(&state, component), 3);
        let coordinates = setup_hit_map_row_coordinates(&state, component, 1);
        assert_eq!(state.hit_target_at(coordinates), Some(component));

        state.apply_input(InputEvent::mouse_down(PointerButton::Left, coordinates));

        assert_eq!(state.focused_component(), component);
        assert_eq!(
            state.to_setup_view_model().focused_field,
            setup_field_for_admin_component(component)
        );
    }
}

#[test]
fn admin_setup_up_down_keys_move_between_fields() {
    let (_fixture, mut state) = fresh_setup_state("setup-admin-up-down-focus");

    state.apply_input(InputEvent::from_key_label("Enter"));
    state.apply_input(InputEvent::from_key_label("Enter"));
    assert_eq!(
        state.focused_component(),
        ShellComponent::SetupAdminUsername
    );

    state.apply_input(InputEvent::from_key_label("Down"));
    assert_eq!(
        state.focused_component(),
        ShellComponent::SetupAdminPassword
    );
    state.apply_input(InputEvent::from_key_label("Down"));
    assert_eq!(
        state.focused_component(),
        ShellComponent::SetupAdminPasswordConfirm
    );
    state.apply_input(InputEvent::from_key_label("Down"));
    assert_eq!(state.focused_component(), ShellComponent::SetupAdminHint);
    state.apply_input(InputEvent::from_key_label("Down"));
    assert_eq!(state.focused_component(), ShellComponent::SetupSubmit);
    state.apply_input(InputEvent::from_key_label("Up"));
    assert_eq!(state.focused_component(), ShellComponent::SetupAdminHint);
}

#[test]
fn admin_setup_password_checklist_updates_with_password_input() {
    let (_fixture, mut state) = fresh_setup_state("setup-admin-password-checklist");

    state.apply_input(InputEvent::from_key_label("Enter"));
    state.apply_input(InputEvent::from_key_label("Enter"));
    type_text(&mut state, "AdminUser");
    state.apply_input(InputEvent::from_key_label("Down"));
    type_text(&mut state, "short");

    let requirements = state.to_setup_view_model().password_requirements;
    assert_requirement(&requirements, "At least 10 characters", false);
    assert_requirement(&requirements, "At most 256 characters", true);
    assert_requirement(&requirements, "Not blank", true);
    assert_requirement(&requirements, "Different from username", true);
    assert_requirement(&requirements, "Passwords match", false);

    for _ in 0..5 {
        state.apply_input(InputEvent::from_key_label("Backspace"));
    }
    type_text(&mut state, "AdminUser");

    let requirements = state.to_setup_view_model().password_requirements;
    assert_requirement(&requirements, "At least 10 characters", false);
    assert_requirement(&requirements, "Not blank", true);
    assert_requirement(&requirements, "Different from username", false);
    assert_requirement(&requirements, "Passwords match", false);

    state.apply_input(InputEvent::from_key_label("Down"));
    type_text(&mut state, "AdminUser");

    let requirements = state.to_setup_view_model().password_requirements;
    assert_requirement(&requirements, "Passwords match", true);
}

#[test]
fn admin_setup_rejects_mismatched_reentered_password() {
    let (_fixture, mut state) = fresh_setup_state("setup-admin-password-mismatch");

    state.apply_input(InputEvent::from_key_label("Enter"));
    state.apply_input(InputEvent::from_key_label("Enter"));
    type_text(&mut state, "AdminUser");
    state.apply_input(InputEvent::from_key_label("Enter"));
    type_text(&mut state, "StrongPass123");
    state.apply_input(InputEvent::from_key_label("Enter"));
    type_text(&mut state, "DifferentPass123");
    state.apply_input(InputEvent::from_key_label("Enter"));
    state.apply_input(InputEvent::from_key_label("Enter"));
    state.apply_input(InputEvent::from_key_label("Enter"));

    let model = state.to_setup_view_model();
    assert_eq!(state.active_screen(), ShellScreen::FirstRunSetup);
    assert_eq!(
        state.focused_component(),
        ShellComponent::SetupAdminPasswordConfirm
    );
    assert_eq!(model.focused_field, ui::SetupField::AdminPasswordConfirm);
    assert!(!model.can_submit);
    assert_eq!(model.error.as_deref(), Some("Passwords do not match"));
    assert_requirement(&model.password_requirements, "Passwords match", false);
}

#[test]
fn first_run_setup_routes_keys_focus_and_mouse_before_home_shortcuts() {
    let fixture = FixtureRoot::new("setup-routing");
    let platform = mock_platform(fixture.path());
    let startup = prepare_shell_startup(&platform, default_config()).expect("startup");
    let mut state = ShellSession::new_with_startup(default_config(), (120, 40), startup);

    state.apply_input(InputEvent::from_key_label("e"));
    state.apply_input(InputEvent::from_key_label("u"));
    assert_eq!(state.active_screen(), ShellScreen::FirstRunSetup);
    assert_eq!(state.active_popup(), None);

    state.apply_input(InputEvent::mouse_down(PointerButton::Right, (4, 4)));
    assert_eq!(state.active_popup(), None);

    state.apply_input(InputEvent::from_key_label("Right"));
    assert_eq!(state.to_setup_view_model().selected_language_index, 0);
    state.apply_input(InputEvent::mouse_down(
        PointerButton::Left,
        setup_hit_map_row_coordinates(&state, ShellComponent::SetupLanguage, 0),
    ));
    assert_eq!(state.to_setup_view_model().selected_language_index, 0);

    state.apply_input(InputEvent::from_key_label("Enter"));
    assert_eq!(state.to_setup_view_model().step, ui::SetupStep::Timezone);
    state.apply_input(InputEvent::from_key_label("Down"));
    assert_eq!(state.to_setup_view_model().selected_timezone_index, 1);
    state.apply_input(InputEvent::from_key_label("PageDown"));
    assert!(state.to_setup_view_model().selected_timezone_index > 1);
    state.apply_input(InputEvent::from_key_label("Home"));
    assert_eq!(state.to_setup_view_model().selected_timezone_index, 0);
    state.apply_input(InputEvent::from_key_label("End"));
    let last_timezone = ui::setup_timezone_options().len().saturating_sub(1);
    assert_eq!(
        state.to_setup_view_model().selected_timezone_index,
        last_timezone
    );
    state.apply_input(InputEvent::mouse_down(
        PointerButton::Left,
        setup_hit_map_row_coordinates(&state, ShellComponent::SetupTimezone, 0),
    ));
    assert_eq!(
        state.to_setup_view_model().selected_timezone_index,
        state.to_setup_view_model().timezone_window_start
    );

    state.apply_input(InputEvent::from_key_label("Enter"));
    assert_eq!(
        state.focused_component(),
        ShellComponent::SetupAdminUsername
    );
    type_text(&mut state, "AdminUser");
    state.apply_input(InputEvent::from_key_label("Enter"));
    assert_eq!(
        state.focused_component(),
        ShellComponent::SetupAdminPassword
    );
    type_text(&mut state, "StrongPass123");
    state.apply_input(InputEvent::from_key_label("Tab"));
    assert_eq!(
        state.focused_component(),
        ShellComponent::SetupAdminPasswordConfirm
    );
    type_text(&mut state, "StrongPass123");
    state.apply_input(InputEvent::from_key_label("Tab"));
    assert_eq!(state.focused_component(), ShellComponent::SetupAdminHint);
    type_text(&mut state, "hint");
    state.apply_input(InputEvent::from_key_label("Backspace"));
    assert_eq!(state.to_setup_view_model().password_hint, "hin");
    state.apply_input(InputEvent::from_key_label("Shift+Tab"));
    assert_eq!(
        state.focused_component(),
        ShellComponent::SetupAdminPasswordConfirm
    );
    state.apply_input(InputEvent::mouse_down(
        PointerButton::Left,
        setup_hit_map_row_coordinates(&state, ShellComponent::SetupSubmit, 0),
    ));
    assert_eq!(state.to_setup_view_model().step, ui::SetupStep::Appearance);
    assert_eq!(
        state.focused_component(),
        ShellComponent::SetupAppearanceShape
    );
    state.apply_input(InputEvent::from_key_label("Right"));
    for _ in 0..5 {
        state.apply_input(InputEvent::from_key_label("Tab"));
    }
    state.apply_input(InputEvent::from_key_label("Enter"));
    assert_eq!(state.active_screen(), ShellScreen::Home);
    assert_eq!(
        state
            .auth_session()
            .map(|session| session.username.as_str()),
        Some("AdminUser")
    );
}

#[test]
fn first_run_setup_keeps_global_exit_keys() {
    let fixture = FixtureRoot::new("setup-exit");
    let platform = mock_platform(fixture.path());
    let startup = prepare_shell_startup(&platform, default_config()).expect("startup");
    let mut state = ShellSession::new_with_startup(default_config(), (120, 40), startup);

    state.apply_input(InputEvent::from_key_label("Esc"));
    assert_eq!(state.active_screen(), ShellScreen::ExitConfirm);
    state.apply_input(InputEvent::from_key_label("n"));
    assert_eq!(state.active_screen(), ShellScreen::FirstRunSetup);

    let action = state.apply_input(InputEvent::from_key_label("Ctrl+C"));
    assert_eq!(action, shell::ShellAction::Exit);
    assert!(state.shutdown_requested());
}

#[test]
fn restart_requires_login_and_bad_password_stays_on_login() {
    let fixture = FixtureRoot::new("restart-login");
    let platform = mock_platform(fixture.path());
    bootstrap_with_shell(&platform);

    let startup = prepare_shell_startup(&platform, default_config()).expect("restart startup");
    let mut state = ShellSession::new_with_startup(default_config(), (120, 40), startup);
    assert_eq!(state.active_screen(), ShellScreen::Login);

    select_login_user(&mut state, "AdminUser");
    state.apply_input(InputEvent::from_key_label("Enter"));
    type_text(&mut state, "WrongPass123");
    state.apply_input(InputEvent::from_key_label("Enter"));

    assert_eq!(state.active_screen(), ShellScreen::Login);
    assert_eq!(state.auth_session(), None);
    assert_eq!(
        state.to_login_view_model().error.as_deref(),
        Some("Password hint: Recovery hint")
    );

    let startup = prepare_shell_startup(&platform, default_config()).expect("second restart");
    let mut state = ShellSession::new_with_startup(default_config(), (120, 40), startup);
    select_login_user(&mut state, "AdminUser");
    state.apply_input(InputEvent::from_key_label("Tab"));
    type_text(&mut state, "StrongPass123");
    state.apply_input(InputEvent::from_key_label("Enter"));

    assert_eq!(state.active_screen(), ShellScreen::Home);
    assert_eq!(
        state
            .auth_session()
            .map(|session| session.username.as_str()),
        Some("AdminUser")
    );
}

#[test]
fn login_bad_password_without_hint_shows_invalid_credentials() {
    let fixture = FixtureRoot::new("restart-login-no-hint");
    let platform = mock_platform(fixture.path());
    let startup = prepare_shell_startup(&platform, default_config()).expect("startup");
    let mut state = ShellSession::new_with_startup(default_config(), (120, 40), startup);
    complete_first_run_setup(&mut state, 0, 0, "AdminUser", "StrongPass123", "");
    assert_eq!(state.active_screen(), ShellScreen::Home);

    let startup = prepare_shell_startup(&platform, default_config()).expect("restart startup");
    let mut state = ShellSession::new_with_startup(default_config(), (120, 40), startup);
    select_login_user(&mut state, "AdminUser");
    state.apply_input(InputEvent::from_key_label("Enter"));
    type_text(&mut state, "WrongPass123");
    state.apply_input(InputEvent::from_key_label("Enter"));

    assert_eq!(state.active_screen(), ShellScreen::Login);
    assert_eq!(
        state.to_login_view_model().error.as_deref(),
        Some("Invalid username or password")
    );
}

#[test]
fn login_idle_timeout_uses_exact_sixty_second_boundary_and_only_resets_for_activity() {
    let fixture = FixtureRoot::new("login-idle-timeout");
    let platform = mock_platform(fixture.path());
    bootstrap_with_shell(&platform);

    let startup = prepare_shell_startup(&platform, default_config()).expect("login startup");
    let mut state = ShellSession::new_with_startup(default_config(), (120, 40), startup);
    state.apply_input(InputEvent::from_key_label("Tab"));
    type_text(&mut state, "WrongPass123");
    state.apply_input(InputEvent::from_key_label("Enter"));
    assert!(state.to_login_view_model().error.is_some());

    let mut activity_at = Instant::now();
    state.apply_input_at(InputEvent::FocusGained, activity_at);
    assert_eq!(
        state.login_idle_deadline_for_test(),
        activity_at + LOGIN_IDLE_TIMEOUT
    );

    for activity in [
        InputEvent::from_key_label("z"),
        InputEvent::mouse_moved((2, 2)),
        InputEvent::Resize {
            width: 119,
            height: 39,
        },
        InputEvent::Paste("ignored paste".to_string()),
        InputEvent::FocusGained,
    ] {
        activity_at += Duration::from_secs(1);
        assert_eq!(
            state.apply_input_at(activity, activity_at),
            ShellAction::Redraw
        );
        assert_eq!(
            state.login_idle_deadline_for_test(),
            activity_at + LOGIN_IDLE_TIMEOUT
        );
    }

    let deadline = state.login_idle_deadline_for_test();
    state.apply_input_at(InputEvent::FocusLost, activity_at + Duration::from_secs(1));
    assert_eq!(state.login_idle_deadline_for_test(), deadline);
    state.apply_time_sync_utc_for_test(Utc::now());
    assert_eq!(state.login_idle_deadline_for_test(), deadline);

    assert_eq!(
        state.apply_input_at(InputEvent::Tick, deadline - Duration::from_nanos(1)),
        ShellAction::Redraw
    );
    assert_eq!(state.active_screen(), ShellScreen::Login);
    assert_eq!(state.login_idle_deadline_for_test(), deadline);

    assert_eq!(
        state.apply_input_at(InputEvent::Tick, deadline),
        ShellAction::Exit
    );
    assert!(!state.shutdown_requested());
    assert_eq!(state.to_login_view_model_at(deadline).password_len, 0);
    assert_eq!(state.to_login_view_model_at(deadline).error, None);

    let mut home = ShellSession::new(debug_config(), (120, 40));
    assert_eq!(home.active_screen(), ShellScreen::Home);
    assert_eq!(
        home.apply_input_at(
            InputEvent::Tick,
            Instant::now() + LOGIN_IDLE_TIMEOUT + Duration::from_secs(1),
        ),
        ShellAction::Redraw
    );
}

#[test]
fn login_password_reveal_handles_unicode_expires_at_five_seconds_and_does_not_extend() {
    let fixture = FixtureRoot::new("login-password-reveal");
    let platform = mock_platform(fixture.path());
    bootstrap_with_shell(&platform);

    let startup = prepare_shell_startup(&platform, default_config()).expect("login startup");
    let mut state = ShellSession::new_with_startup(default_config(), (120, 40), startup);
    state.apply_input(InputEvent::from_key_label("Tab"));
    type_text(&mut state, "密碼🙂");

    let shown_at = Instant::now();
    state.apply_input_at(function_key(2), shown_at);
    let reveal_deadline = shown_at + PASSWORD_REVEAL_DURATION;
    assert_eq!(
        state.login_password_visible_until_for_test(),
        Some(reveal_deadline)
    );
    let visible = state.to_login_view_model_at(shown_at);
    assert_eq!(visible.password_len, 3);
    assert_eq!(visible.visible_password(), Some("密碼🙂"));

    state.apply_input_at(
        InputEvent::mouse_moved((1, 1)),
        shown_at + Duration::from_secs(4),
    );
    assert_eq!(
        state.login_password_visible_until_for_test(),
        Some(reveal_deadline),
        "ordinary activity must not extend the fixed reveal window"
    );
    assert_eq!(
        state
            .to_login_view_model_at(reveal_deadline - Duration::from_nanos(1))
            .visible_password(),
        Some("密碼🙂")
    );

    state.apply_input_at(InputEvent::Tick, reveal_deadline);
    assert_eq!(state.login_password_visible_until_for_test(), None);
    let hidden = state.to_login_view_model_at(reveal_deadline);
    assert_eq!(hidden.password_len, 3);
    assert_eq!(hidden.visible_password(), None);

    let shown_again_at = reveal_deadline + Duration::from_secs(1);
    state.apply_input_at(function_key(2), shown_again_at);
    assert_eq!(
        state
            .to_login_view_model_at(shown_again_at)
            .visible_password(),
        Some("密碼🙂")
    );
    state.apply_input_at(function_key(2), shown_again_at + Duration::from_secs(1));
    assert_eq!(
        state
            .to_login_view_model_at(shown_again_at + Duration::from_secs(1))
            .visible_password(),
        None
    );
}

#[test]
fn login_f3_cannot_create_an_anonymous_session_and_focus_skips_guest() {
    let fixture = FixtureRoot::new("login-without-guest");
    let platform = mock_platform(fixture.path());
    bootstrap_with_shell(&platform);

    let startup = prepare_shell_startup(&platform, default_config()).expect("login startup");
    let manager = startup.storage_manager.clone().expect("storage manager");
    let users_before = read_optional_file(&manager.layout().users_path);
    let clock_before = read_optional_file(&manager.layout().clock_path);
    let mut state = ShellSession::new_with_startup(default_config(), (120, 40), startup);

    assert_eq!(state.active_screen(), ShellScreen::Login);
    assert!(state.auth_session().is_none());
    assert_eq!(state.focused_component(), ShellComponent::LoginUserList);
    state.apply_input(InputEvent::from_key_label("Tab"));
    assert_eq!(state.focused_component(), ShellComponent::LoginPassword);
    state.apply_input(InputEvent::from_key_label("Tab"));
    assert_eq!(
        state.focused_component(),
        ShellComponent::LoginPasswordVisibility
    );
    state.apply_input(InputEvent::from_key_label("Tab"));
    assert_eq!(state.focused_component(), ShellComponent::LoginUserList);
    state.apply_input(InputEvent::from_key_label("Shift+Tab"));
    assert_eq!(
        state.focused_component(),
        ShellComponent::LoginPasswordVisibility
    );

    state.apply_input(function_key(3));
    assert_eq!(state.active_screen(), ShellScreen::Login);
    assert!(state.auth_session().is_none());
    assert_eq!(
        state.focused_component(),
        ShellComponent::LoginPasswordVisibility
    );

    assert_eq!(
        read_optional_file(&manager.layout().users_path),
        users_before
    );
    assert_eq!(
        read_optional_file(&manager.layout().clock_path),
        clock_before
    );
}

#[test]
fn l_returns_to_weathr_while_focused_and_mouse_logout_stay_in_shell() {
    let fixture = FixtureRoot::new("logout-inputs");
    let platform = mock_platform(fixture.path());
    bootstrap_with_shell(&platform);

    let startup = prepare_shell_startup(&platform, default_config()).expect("shortcut startup");
    let mut shortcut_state = ShellSession::new_with_startup(default_config(), (120, 40), startup);
    login(&mut shortcut_state, "AdminUser", "StrongPass123");
    let action = shortcut_state.apply_input(InputEvent::from_key_label("l"));
    assert_eq!(action, ShellAction::Exit);
    assert_eq!(
        shortcut_state.last_command(),
        Some(&shell::ShellCommand::LogoutToLockscreen)
    );
    assert_eq!(shortcut_state.active_screen(), ShellScreen::Login);
    assert!(shortcut_state.auth_session().is_none());

    let startup = prepare_shell_startup(&platform, default_config()).expect("focus startup");
    let mut focus_state = ShellSession::new_with_startup(default_config(), (120, 40), startup);
    login(&mut focus_state, "AdminUser", "StrongPass123");
    focus_state.apply_input(InputEvent::from_key_label("Tab"));
    assert_eq!(focus_state.focused_component(), ShellComponent::HomeLogout);
    let action = focus_state.apply_input(InputEvent::from_key_label("Enter"));
    assert_eq!(action, ShellAction::Redraw);
    assert_eq!(focus_state.active_screen(), ShellScreen::Login);
    assert!(focus_state.auth_session().is_none());

    let startup = prepare_shell_startup(&platform, default_config()).expect("mouse startup");
    let mut mouse_state = ShellSession::new_with_startup(default_config(), (120, 40), startup);
    login(&mut mouse_state, "AdminUser", "StrongPass123");
    let logout = component_center(&mouse_state, ShellComponent::HomeLogout);
    assert_eq!(
        mouse_state.hit_target_at(logout),
        Some(ShellComponent::HomeLogout)
    );
    let action = mouse_state.apply_input(InputEvent::mouse_down(PointerButton::Left, logout));
    assert_eq!(action, ShellAction::Redraw);
    assert_eq!(mouse_state.active_screen(), ShellScreen::Login);
    assert!(mouse_state.auth_session().is_none());
}

#[test]
fn login_and_logout_hit_regions_share_render_geometry_at_supported_sizes() {
    let fixture = FixtureRoot::new("auth-hit-geometry");
    let platform = mock_platform(fixture.path());
    bootstrap_with_shell(&platform);

    for terminal_size in [(80, 24), (50, 12)] {
        let startup = prepare_shell_startup(&platform, default_config()).expect("login startup");
        let mut state = ShellSession::new_with_startup(default_config(), terminal_size, startup);
        let password = component_area(&state, ShellComponent::LoginPassword);
        let visibility = component_area(&state, ShellComponent::LoginPasswordVisibility);

        for area in [password, visibility] {
            assert!(
                area.width > 0 && area.height > 0,
                "missing area at {terminal_size:?}"
            );
        }
        assert!(!rects_overlap(password, visibility));
        for component in [
            ShellComponent::LoginPassword,
            ShellComponent::LoginPasswordVisibility,
        ] {
            let center = rect_center(component_area(&state, component));
            assert_eq!(state.hit_target_at(center), Some(component));
        }

        login(&mut state, "AdminUser", "StrongPass123");
        let logout = component_area(&state, ShellComponent::HomeLogout);
        assert!(logout.width > 0 && logout.height > 0);
        assert_eq!(
            state.hit_target_at(rect_center(logout)),
            Some(ShellComponent::HomeLogout),
            "Logout must win over the enclosing Home region"
        );
    }

    let startup = prepare_shell_startup(&platform, default_config()).expect("compact startup");
    let compact = ShellSession::new_with_startup(default_config(), (49, 11), startup);
    assert!(compact.hit_map().regions().iter().all(|region| {
        !matches!(
            region.component,
            ShellComponent::LoginPassword
                | ShellComponent::LoginPasswordVisibility
                | ShellComponent::HomeLogout
        )
    }));
}

#[test]
fn admin_can_manage_users_and_user_can_only_open_own_profile() {
    let fixture = FixtureRoot::new("manage-users");
    let platform = mock_platform(fixture.path());
    bootstrap_with_shell(&platform);

    let startup = prepare_shell_startup(&platform, default_config()).expect("admin startup");
    let manager = startup.storage_manager.clone().expect("storage manager");
    let mut admin_state = ShellSession::new_with_startup(default_config(), (120, 40), startup);
    login(&mut admin_state, "AdminUser", "StrongPass123");
    admin_state.apply_input(InputEvent::from_key_label("u"));
    assert_eq!(admin_state.active_screen(), ShellScreen::UserManagement);

    admin_state.apply_input(InputEvent::from_key_label("n"));
    type_text(&mut admin_state, "user2");
    admin_state.apply_input(InputEvent::from_key_label("Tab"));
    type_text(&mut admin_state, "User Two");
    admin_state.apply_input(InputEvent::from_key_label("Tab"));
    admin_state.apply_input(InputEvent::from_key_label("Tab"));
    type_text(&mut admin_state, "userPass2123!");
    admin_state.apply_input(InputEvent::from_key_label("Enter"));
    assert_eq!(
        admin_state
            .to_user_management_view_model()
            .users
            .iter()
            .filter(|user| user.username == "user2")
            .count(),
        1
    );
    let created_record = manager
        .load_users()
        .expect("users")
        .users
        .into_iter()
        .find(|user| user.username == "user2")
        .expect("created user record");
    assert_eq!(
        created_record.appearance,
        storage::AppearanceConfig::default()
    );

    let startup = prepare_shell_startup(&platform, default_config()).expect("user startup");
    let mut user_state = ShellSession::new_with_startup(default_config(), (120, 40), startup);
    login(&mut user_state, "user2", "userPass2123!");
    assert_eq!(user_state.active_screen(), ShellScreen::Home);
    assert_eq!(user_state.home_mode(), ShellHomeMode::User);

    user_state.apply_input(InputEvent::from_key_label("u"));
    assert_eq!(user_state.active_screen(), ShellScreen::UserManagement);
    let profile = user_state.to_user_management_view_model();
    assert!(!profile.can_manage_all);
    assert_eq!(profile.users.len(), 1);
    assert_eq!(profile.users[0].username, "user2");
}

#[test]
fn user_management_refresh_failure_is_visible_preserves_users_and_resolves_after_recovery() {
    let fixture = FixtureRoot::new("user-management-refresh-alert");
    let platform = mock_platform(fixture.path());
    bootstrap_with_shell(&platform);

    let startup = prepare_shell_startup(&platform, default_config()).expect("admin startup");
    let manager = startup.storage_manager.clone().expect("storage manager");
    let users_path = manager.layout().users_path.clone();
    let mut state = ShellSession::new_with_startup(default_config(), (120, 40), startup);
    login(&mut state, "AdminUser", "StrongPass123");
    state.apply_input(InputEvent::from_key_label("u"));
    assert_eq!(state.active_screen(), ShellScreen::UserManagement);

    let users_before_failure = state.to_user_management_view_model().users;
    let valid_users = fs::read(&users_path).expect("valid users document");
    fs::write(&users_path, b"{ invalid users json").expect("corrupt users fixture");

    state.apply_input(InputEvent::from_key_label("e"));
    state.apply_input(InputEvent::from_key_label("Enter"));

    let failed_management = state.to_user_management_view_model();
    let failed_chrome = state.to_shell_chrome_view_model();
    assert_eq!(failed_management.users, users_before_failure);
    assert!(
        failed_management
            .message
            .as_deref()
            .is_some_and(|message| { failed_chrome.status.error.as_deref() == Some(message) })
    );
    assert_eq!(failed_chrome.status.alert_tone, NotificationTone::Error);

    fs::write(&users_path, valid_users).expect("restore users fixture");
    state.apply_input(InputEvent::from_key_label("e"));
    state.apply_input(InputEvent::from_key_label("Enter"));

    let recovered_management = state.to_user_management_view_model();
    assert_eq!(recovered_management.users.len(), users_before_failure.len());
    assert!(
        recovered_management
            .users
            .iter()
            .any(|user| user.username == "AdminUser" && user.role == "Admin")
    );
    assert_eq!(state.to_shell_chrome_view_model().status.error, None);
}

#[test]
fn login_mouse_click_selects_user_and_focuses_password() {
    let fixture = FixtureRoot::new("login-mouse-user-select");
    let platform = mock_platform(fixture.path());
    bootstrap_with_shell(&platform);

    let startup = prepare_shell_startup(&platform, default_config()).expect("admin startup");
    let mut admin_state = ShellSession::new_with_startup(default_config(), (120, 40), startup);
    login(&mut admin_state, "AdminUser", "StrongPass123");
    admin_state.apply_input(InputEvent::from_key_label("u"));
    admin_state.apply_input(InputEvent::from_key_label("n"));
    type_text(&mut admin_state, "user2");
    admin_state.apply_input(InputEvent::from_key_label("Tab"));
    type_text(&mut admin_state, "User Two");
    admin_state.apply_input(InputEvent::from_key_label("Tab"));
    admin_state.apply_input(InputEvent::from_key_label("Tab"));
    type_text(&mut admin_state, "userPass2123!");
    admin_state.apply_input(InputEvent::from_key_label("Enter"));

    let startup = prepare_shell_startup(&platform, default_config()).expect("user startup");
    let mut state = ShellSession::new_with_startup(default_config(), (120, 40), startup);
    assert_eq!(state.active_screen(), ShellScreen::Login);
    assert_eq!(state.focused_component(), ShellComponent::LoginUserList);
    assert_eq!(
        state
            .to_login_view_model()
            .selected_user()
            .map(|user| user.username.as_str()),
        Some("AdminUser")
    );

    state.apply_input(InputEvent::mouse_down(
        PointerButton::Left,
        login_user_row_coordinates(&state, 1),
    ));

    assert_eq!(state.focused_component(), ShellComponent::LoginPassword);
    assert_eq!(
        state
            .to_login_view_model()
            .selected_user()
            .map(|user| user.username.as_str()),
        Some("user2")
    );

    type_text(&mut state, "userPass2123!");
    state.apply_input(InputEvent::from_key_label("Enter"));

    assert_eq!(state.active_screen(), ShellScreen::Home);
    assert_eq!(
        state
            .auth_session()
            .map(|session| session.username.as_str()),
        Some("user2")
    );
}

#[test]
fn user_management_forms_edit_password_and_delete_accounts() {
    let fixture = FixtureRoot::new("user-management-forms");
    let platform = mock_platform(fixture.path());
    bootstrap_with_shell(&platform);

    let startup = prepare_shell_startup(&platform, default_config()).expect("admin startup");
    let manager = startup.storage_manager.clone().expect("storage manager");
    let mut state = ShellSession::new_with_startup(default_config(), (120, 40), startup);
    login(&mut state, "AdminUser", "StrongPass123");
    state.apply_input(InputEvent::from_key_label("u"));

    state.apply_input(InputEvent::from_key_label("n"));
    type_text(&mut state, "deleteme");
    state.apply_input(InputEvent::from_key_label("Tab"));
    type_text(&mut state, "Delete Me");
    state.apply_input(InputEvent::from_key_label("Tab"));
    state.apply_input(InputEvent::from_key_label("Tab"));
    type_text(&mut state, "deletePass123!");
    state.apply_input(InputEvent::from_key_label("Enter"));
    assert!(
        state
            .to_user_management_view_model()
            .users
            .iter()
            .any(|user| user.username == "deleteme")
    );
    let deleted_user_id = manager
        .load_users()
        .expect("users")
        .users
        .into_iter()
        .find(|user| user.username == "deleteme")
        .expect("managed user")
        .id;
    let mut clock = manager.load_clock().expect("clock document");
    clock
        .profiles
        .insert(deleted_user_id.clone(), ClockProfile::default());
    manager.save_clock(&clock).expect("clock profile saved");

    state.apply_input(InputEvent::from_key_label("e"));
    for _ in 0.."Delete Me".len() {
        state.apply_input(InputEvent::from_key_label("Backspace"));
    }
    type_text(&mut state, "Deleted User");
    state.apply_input(InputEvent::from_key_label("Enter"));
    assert!(
        state
            .to_user_management_view_model()
            .users
            .iter()
            .any(|user| user.username == "deleteme" && user.display_name == "Deleted User")
    );

    state.apply_input(InputEvent::from_key_label("c"));
    assert!(
        state
            .to_user_management_view_model()
            .users
            .iter()
            .any(|user| user.username == "deleteme" && user.role == "Admin")
    );
    state.apply_input(InputEvent::from_key_label("c"));
    assert!(
        state
            .to_user_management_view_model()
            .users
            .iter()
            .any(|user| user.username == "deleteme" && user.role == "User")
    );
    state.apply_input(InputEvent::from_key_label("d"));
    assert!(
        state
            .to_user_management_view_model()
            .users
            .iter()
            .any(|user| user.username == "deleteme" && !user.enabled)
    );
    state.apply_input(InputEvent::from_key_label("u"));
    assert!(
        state
            .to_user_management_view_model()
            .users
            .iter()
            .any(|user| user.username == "deleteme" && user.enabled && !user.locked)
    );

    state.apply_input(InputEvent::from_key_label("r"));
    type_text(&mut state, "ChangedPass123!");
    state.apply_input(InputEvent::from_key_label("Enter"));
    state.apply_input(InputEvent::from_key_label("x"));
    assert_eq!(
        state
            .to_notification_view_model()
            .map(|notification| notification.title),
        Some("Delete user".to_string())
    );
    state.apply_input(InputEvent::from_key_label("x"));
    assert!(
        !state
            .to_user_management_view_model()
            .users
            .iter()
            .any(|user| user.username == "deleteme")
    );
    assert!(
        !manager
            .load_clock()
            .expect("clock document")
            .profiles
            .contains_key(&deleted_user_id)
    );
}

#[test]
fn compact_user_management_captures_hidden_actions_and_only_escape_leaves() {
    let fixture = FixtureRoot::new("user-management-compact");
    let platform = mock_platform(fixture.path());
    bootstrap_with_shell(&platform);

    let startup = prepare_shell_startup(&platform, default_config()).expect("admin startup");
    let mut state = ShellSession::new_with_startup(default_config(), (120, 40), startup);
    login(&mut state, "AdminUser", "StrongPass123");
    state.apply_input(InputEvent::from_key_label("u"));
    let before = state.to_user_management_view_model();

    state.apply_input(InputEvent::Resize {
        width: 49,
        height: 40,
    });
    for key in ["n", "e", "r", "d", "u", "c", "x", "Down", "End"] {
        state.apply_input(InputEvent::from_key_label(key));
    }
    state.apply_input(InputEvent::mouse_down(PointerButton::Left, (10, 10)));

    let compact = state.to_user_management_view_model();
    assert_eq!(state.active_screen(), ShellScreen::UserManagement);
    assert_eq!(compact.users, before.users);
    assert_eq!(compact.selected_index, before.selected_index);
    assert_eq!(compact.form, None);
    assert_eq!(state.to_notification_view_model(), None);

    state.apply_input(InputEvent::from_key_label("Esc"));
    assert_eq!(state.active_screen(), ShellScreen::Home);
}

#[test]
fn user_management_create_role_and_action_focus_use_one_keyboard_flow() {
    let fixture = FixtureRoot::new("user-management-role-focus");
    let platform = mock_platform(fixture.path());
    bootstrap_with_shell(&platform);

    let startup = prepare_shell_startup(&platform, default_config()).expect("admin startup");
    let mut state = ShellSession::new_with_startup(default_config(), (120, 40), startup);
    login(&mut state, "AdminUser", "StrongPass123");
    state.apply_input(InputEvent::from_key_label("u"));

    state.apply_input(InputEvent::from_key_label("n"));
    assert_eq!(
        state
            .to_user_management_view_model()
            .form
            .as_ref()
            .map(|form| (form.kind, form.role.as_str(), form.focused_field)),
        Some((
            ui::UserManagementFormKind::Create,
            "User",
            ui::UserManagementField::Username,
        ))
    );
    type_text(&mut state, "SecondAdmin");
    state.apply_input(InputEvent::from_key_label("Tab"));
    type_text(&mut state, "Second Administrator");
    state.apply_input(InputEvent::from_key_label("Tab"));
    assert_eq!(
        state
            .to_user_management_view_model()
            .form
            .as_ref()
            .map(|form| form.focused_field),
        Some(ui::UserManagementField::Role)
    );
    state.apply_input(InputEvent::from_key_label(" "));
    assert_eq!(
        state
            .to_user_management_view_model()
            .form
            .as_ref()
            .map(|form| form.role.as_str()),
        Some("Admin")
    );
    state.apply_input(InputEvent::from_key_label("Tab"));
    type_text(&mut state, "ManagedPass123!");
    state.apply_input(InputEvent::from_key_label("Enter"));

    let created = state.to_user_management_view_model();
    assert!(created.form.is_none());
    assert!(
        created
            .users
            .iter()
            .any(|user| user.username == "SecondAdmin" && user.role == "Admin")
    );
    assert_eq!(
        created.focus,
        ui::UserManagementFocus::Action(ui::UserManagementAction::NewUser)
    );

    state.apply_input(InputEvent::from_key_label("Shift+Tab"));
    assert_eq!(
        state.to_user_management_view_model().focus,
        ui::UserManagementFocus::UserList
    );
    state.apply_input(InputEvent::from_key_label("Tab"));
    state.apply_input(InputEvent::from_key_label("Enter"));
    assert!(state.to_user_management_view_model().form.is_some());
    state.apply_input(InputEvent::from_key_label("Esc"));
    state.apply_input(InputEvent::from_key_label("Tab"));
    state.apply_input(InputEvent::from_key_label(" "));
    assert_eq!(
        state
            .to_user_management_view_model()
            .form
            .as_ref()
            .map(|form| form.kind),
        Some(ui::UserManagementFormKind::EditInfo)
    );
    state.apply_input(InputEvent::from_key_label("Esc"));

    let user_count = state.to_user_management_view_model().users.len();
    state.apply_input(InputEvent::from_key_label("a"));
    state.apply_input(InputEvent::from_key_label("g"));
    assert_eq!(
        state.to_user_management_view_model().users.len(),
        user_count
    );
    assert!(state.to_user_management_view_model().form.is_none());
}

#[test]
fn user_management_mouse_uses_shared_rows_actions_forms_and_scroll_geometry() {
    let fixture = FixtureRoot::new("user-management-mouse-layout");
    let platform = mock_platform(fixture.path());
    bootstrap_with_shell(&platform);

    let startup = prepare_shell_startup(&platform, default_config()).expect("admin startup");
    let mut state = ShellSession::new_with_startup(default_config(), (120, 40), startup);
    login(&mut state, "AdminUser", "StrongPass123");
    state.apply_input(InputEvent::from_key_label("u"));
    for username in ["mouse1", "mouse2", "mouse3", "mouse4"] {
        create_managed_user(&mut state, username, false);
    }

    state.apply_input(InputEvent::Resize {
        width: 120,
        height: 16,
    });
    state.apply_input(InputEvent::from_key_label("Home"));
    let layout = user_management_layout_for(&state);
    assert!(layout.visible_capacity >= 2);
    let clicked_row = layout.rows.last().copied().expect("visible user row");
    state.apply_input(InputEvent::mouse_down(
        PointerButton::Left,
        rect_center(clicked_row.area),
    ));
    assert_eq!(
        state.to_user_management_view_model().selected_index,
        clicked_row.index
    );

    let before_scroll = state.to_user_management_view_model().selected_index;
    state.apply_input(InputEvent::mouse_scroll(
        shell::ScrollDirection::Down,
        rect_center(layout.rows_area),
    ));
    let after_scroll = state.to_user_management_view_model();
    assert_eq!(after_scroll.selected_index, before_scroll + 1);
    assert!(after_scroll.user_window_start > 0);

    let layout = user_management_layout_for(&state);
    let new_user = layout
        .actions
        .iter()
        .find(|action| action.action == ui::UserManagementAction::NewUser)
        .expect("new user action");
    state.apply_input(InputEvent::mouse_down(
        PointerButton::Left,
        rect_center(new_user.area),
    ));
    assert_eq!(
        state
            .to_user_management_view_model()
            .form
            .as_ref()
            .map(|form| form.kind),
        Some(ui::UserManagementFormKind::Create)
    );

    let layout = user_management_layout_for(&state);
    let form = layout.form.as_ref().expect("create form layout");
    let display_name = form
        .fields
        .iter()
        .find(|field| field.field == ui::UserManagementField::DisplayName)
        .expect("display name field");
    state.apply_input(InputEvent::mouse_down(
        PointerButton::Left,
        rect_center(display_name.area),
    ));
    assert_eq!(
        state
            .to_user_management_view_model()
            .form
            .as_ref()
            .map(|form| form.focused_field),
        Some(ui::UserManagementField::DisplayName)
    );

    let cancel = user_management_layout_for(&state)
        .form
        .expect("create form layout")
        .cancel;
    state.apply_input(InputEvent::mouse_down(
        PointerButton::Left,
        rect_center(cancel),
    ));
    assert!(state.to_user_management_view_model().form.is_none());
}

#[test]
fn user_management_clock_button_opens_clock_and_returns_to_user_management() {
    let fixture = FixtureRoot::new("user-management-clock-button");
    let platform = mock_platform(fixture.path());
    bootstrap_with_shell(&platform);

    let startup = prepare_shell_startup(&platform, default_config()).expect("admin startup");
    let mut state = ShellSession::new_with_startup(default_config(), (120, 40), startup);
    login(&mut state, "AdminUser", "StrongPass123");
    state.apply_input(InputEvent::from_key_label("u"));
    assert_eq!(state.active_screen(), ShellScreen::UserManagement);

    let clock_coordinates = component_center(&state, ShellComponent::ClockButton);
    assert_eq!(
        state.hit_target_at(clock_coordinates),
        Some(ShellComponent::ClockButton)
    );
    state.apply_input(InputEvent::mouse_down(
        PointerButton::Left,
        clock_coordinates,
    ));

    assert_eq!(state.active_screen(), ShellScreen::Clock);
    assert_eq!(state.last_command(), Some(&shell::ShellCommand::OpenClock));

    let clock_coordinates = component_center(&state, ShellComponent::ClockButton);
    state.apply_input(InputEvent::mouse_down(
        PointerButton::Left,
        clock_coordinates,
    ));

    assert_eq!(state.active_screen(), ShellScreen::UserManagement);
    assert_eq!(state.last_command(), Some(&shell::ShellCommand::CloseClock));
}

#[test]
fn last_admin_actions_are_skipped_and_self_delete_defaults_to_cancel() {
    let fixture = FixtureRoot::new("user-management-last-admin-delete");
    let platform = mock_platform(fixture.path());
    bootstrap_with_shell(&platform);

    let startup = prepare_shell_startup(&platform, default_config()).expect("admin startup");
    let mut state = ShellSession::new_with_startup(default_config(), (120, 40), startup);
    login(&mut state, "AdminUser", "StrongPass123");
    state.apply_input(InputEvent::from_key_label("u"));

    let model = state.to_user_management_view_model();
    for protected in [
        ui::UserManagementAction::ToggleEnabled,
        ui::UserManagementAction::ToggleRole,
        ui::UserManagementAction::Delete,
    ] {
        let action = model
            .actions
            .iter()
            .find(|action| action.action == protected)
            .expect("protected action");
        assert!(!action.enabled);
        assert!(action.disabled_reason.is_some());
    }
    for _ in 0..4 {
        state.apply_input(InputEvent::from_key_label("Tab"));
    }
    assert_eq!(
        state.to_user_management_view_model().focus,
        ui::UserManagementFocus::Action(ui::UserManagementAction::Back)
    );
    state.apply_input(InputEvent::from_key_label("x"));
    assert!(state.to_notification_view_model().is_none());

    create_managed_user(&mut state, "BackupAdmin", true);
    state.apply_input(InputEvent::from_key_label("Home"));
    assert_eq!(
        state
            .to_user_management_view_model()
            .users
            .get(state.to_user_management_view_model().selected_index)
            .map(|user| user.username.as_str()),
        Some("AdminUser")
    );
    state.apply_input(InputEvent::from_key_label("x"));
    let notification = state
        .to_notification_view_model()
        .expect("self-delete confirmation");
    assert_eq!(notification.title, "Delete your account");
    assert!(notification.message.contains("signed out"));
    assert_eq!(
        notification
            .actions
            .iter()
            .find(|action| action.selected)
            .map(|action| action.id.as_str()),
        Some("cancel")
    );

    state.apply_input(InputEvent::from_key_label("Enter"));
    assert!(state.to_notification_view_model().is_none());
    assert_eq!(state.active_screen(), ShellScreen::UserManagement);
    assert_eq!(
        state
            .auth_session()
            .map(|session| session.username.as_str()),
        Some("AdminUser")
    );

    state.apply_input(InputEvent::from_key_label("x"));
    state.apply_input(InputEvent::from_key_label("x"));
    assert_eq!(state.active_screen(), ShellScreen::Login);
    assert!(state.auth_session().is_none());
    assert!(
        state
            .to_login_view_model()
            .users
            .iter()
            .all(|user| user.username != "AdminUser")
    );
    assert!(
        state
            .to_login_view_model()
            .users
            .iter()
            .any(|user| user.username == "BackupAdmin")
    );
}

#[test]
fn clock_keyboard_flow_creates_manages_and_persists_entries() {
    let fixture = FixtureRoot::new("clock-keyboard-flow");
    let platform = mock_platform(fixture.path());
    let startup = prepare_shell_startup(&platform, default_config()).expect("startup");
    let manager = startup.storage_manager.clone().expect("storage manager");
    let mut state = ShellSession::new_with_startup(default_config(), (120, 40), startup);
    complete_first_run_setup(&mut state, 0, 0, "AdminUser", "StrongPass123", "Clock hint");

    open_clock(&mut state);
    let new_button = component_center(&state, ShellComponent::ClockNewButton);
    state.apply_input(InputEvent::mouse_down(PointerButton::Left, new_button));
    assert_eq!(state.focused_component(), ShellComponent::ClockCreateInput);
    state.apply_input(InputEvent::from_key_label("Esc"));

    create_clock_entry(&mut state, "07 30 00", false);
    assert_eq!(state.to_clock_view_model().alarms.len(), 1);
    assert!(
        state.to_clock_view_model().alarms[0]
            .label
            .contains("07:30:00")
    );
    let alarm_row = component_center(&state, ShellComponent::ClockEntryList);
    state.apply_input(InputEvent::mouse_down(PointerButton::Left, alarm_row));
    assert_eq!(
        state
            .to_notification_view_model()
            .map(|notification| notification.title),
        Some("Manage Alarm".to_string())
    );
    state.apply_input(InputEvent::from_key_label("Esc"));

    create_clock_entry(&mut state, "00 00 05", true);
    assert_eq!(state.to_clock_view_model().countdowns.len(), 1);
    assert_eq!(state.focused_component(), ShellComponent::ClockEntryList);

    state.apply_input(InputEvent::from_key_label("Enter"));
    assert_eq!(
        state
            .to_notification_view_model()
            .map(|notification| notification.title),
        Some("Manage Countdown".to_string())
    );
    state.apply_input(InputEvent::from_key_label("t"));
    assert!(state.to_clock_view_model().countdowns[0].strong);

    state.apply_input(InputEvent::from_key_label("Enter"));
    state.apply_input(InputEvent::from_key_label("x"));
    assert!(state.to_clock_view_model().countdowns.is_empty());

    let user_id = state.auth_session().expect("signed in").user_id.clone();
    let document = manager.load_clock().expect("clock document");
    let profile = document.profiles.get(&user_id).expect("user clock profile");
    assert_eq!(profile.entries.len(), 1);
    assert!(matches!(
        profile.entries[0],
        ClockEntryRecord::DailyAlarm { .. }
    ));
}

#[test]
fn expired_countdown_waits_for_initial_sync_then_is_delivered_and_removed() {
    let fixture = FixtureRoot::new("clock-expired-on-login");
    let platform = mock_platform(fixture.path());
    bootstrap_with_shell(&platform);

    let startup = prepare_shell_startup(&platform, default_config()).expect("startup");
    let manager = startup.storage_manager.clone().expect("storage manager");
    let user_id = manager
        .load_users()
        .expect("users")
        .users
        .into_iter()
        .find(|user| user.username == "AdminUser")
        .expect("admin user")
        .id;
    let mut document = manager.load_clock().expect("clock document");
    document.profiles.insert(
        user_id.clone(),
        ClockProfile {
            next_id: 2,
            entries: vec![ClockEntryRecord::Countdown {
                id: 1,
                deadline_epoch_ms: 1,
                strong: false,
            }],
        },
    );
    manager.save_clock(&document).expect("clock saved");

    let mut state = ShellSession::new_with_startup(default_config(), (120, 40), startup);
    login(&mut state, "AdminUser", "StrongPass123");

    assert_eq!(
        state.to_shell_chrome_view_model().status.toast.as_deref(),
        Some("Waiting for initial time sync to restore reminders")
    );
    assert_eq!(
        manager
            .load_clock()
            .expect("clock remains pending")
            .profiles
            .get(&user_id)
            .expect("profile retained")
            .entries
            .len(),
        1
    );

    state.apply_time_sync_utc_for_test(Utc::now());

    assert!(state.to_clock_view_model().countdowns.is_empty());
    assert_eq!(
        state.to_shell_chrome_view_model().status.toast.as_deref(),
        Some("Countdown finished")
    );
    assert!(
        manager
            .load_clock()
            .expect("clock reloaded")
            .profiles
            .get(&user_id)
            .expect("profile retained")
            .entries
            .is_empty()
    );
}

fn open_clock(state: &mut ShellSession) {
    assert_eq!(state.active_screen(), ShellScreen::Home);
    state.apply_input(InputEvent::from_key_label("Tab"));
    assert_eq!(state.focused_component(), ShellComponent::HomeLogout);
    state.apply_input(InputEvent::from_key_label("Tab"));
    assert_eq!(state.focused_component(), ShellComponent::ClockButton);
    state.apply_input(InputEvent::from_key_label("Enter"));
    assert_eq!(state.active_screen(), ShellScreen::Clock);
    assert_eq!(state.focused_component(), ShellComponent::ClockNewButton);
}

fn create_clock_entry(state: &mut ShellSession, input: &str, countdown: bool) {
    state.apply_input(InputEvent::from_key_label("n"));
    assert_eq!(state.focused_component(), ShellComponent::ClockCreateInput);
    type_text(state, input);
    state.apply_input(InputEvent::from_key_label("Tab"));
    if countdown {
        state.apply_input(InputEvent::from_key_label("Tab"));
    }
    state.apply_input(InputEvent::from_key_label("Enter"));
    assert_eq!(state.focused_component(), ShellComponent::ClockEntryList);
}

fn component_center(state: &ShellSession, component: ShellComponent) -> (u16, u16) {
    rect_center(component_area(state, component))
}

fn component_area(state: &ShellSession, component: ShellComponent) -> ratatui::layout::Rect {
    state
        .hit_map()
        .regions()
        .iter()
        .rev()
        .find(|region| region.component == component)
        .unwrap_or_else(|| panic!("missing {component:?} hit region"))
        .area
}

fn rects_overlap(first: ratatui::layout::Rect, second: ratatui::layout::Rect) -> bool {
    first.x < second.right()
        && second.x < first.right()
        && first.y < second.bottom()
        && second.y < first.bottom()
}

fn function_key(number: u8) -> InputEvent {
    InputEvent::Key(KeyInput::with_phase(
        InputKey::F(number),
        InputModifiers::none(),
        InputPhase::Press,
    ))
}

fn read_optional_file(path: &Path) -> Option<Vec<u8>> {
    fs::read(path).ok()
}

fn bootstrap_with_shell(platform: &MockPlatform) {
    let startup = prepare_shell_startup(platform, default_config()).expect("startup");
    let mut state = ShellSession::new_with_startup(default_config(), (120, 40), startup);
    complete_first_run_setup(
        &mut state,
        0,
        0,
        "AdminUser",
        "StrongPass123",
        "Recovery hint",
    );
    assert_eq!(state.active_screen(), ShellScreen::Home);
}

fn login(state: &mut ShellSession, username: &str, password: &str) {
    select_login_user(state, username);
    state.apply_input(InputEvent::from_key_label("Tab"));
    type_text(state, password);
    state.apply_input(InputEvent::from_key_label("Enter"));
}

fn select_login_user(state: &mut ShellSession, username: &str) {
    assert_eq!(state.active_screen(), ShellScreen::Login);
    if state.focused_component() != ShellComponent::LoginUserList {
        state.apply_input(InputEvent::from_key_label("Shift+Tab"));
    }

    let model = state.to_login_view_model();
    let target = model
        .users
        .iter()
        .position(|user| user.username.eq_ignore_ascii_case(username))
        .unwrap_or_else(|| panic!("missing login user: {username}"));
    while state.to_login_view_model().selected_index < target {
        state.apply_input(InputEvent::from_key_label("Down"));
    }
    while state.to_login_view_model().selected_index > target {
        state.apply_input(InputEvent::from_key_label("Up"));
    }

    assert_eq!(
        state
            .to_login_view_model()
            .selected_user()
            .map(|user| user.username.as_str()),
        Some(username)
    );
}

fn type_text(state: &mut ShellSession, text: &str) {
    for character in text.chars() {
        state.apply_input(InputEvent::from_key_label(character.to_string()));
    }
}

fn create_managed_user(state: &mut ShellSession, username: &str, admin: bool) {
    state.apply_input(InputEvent::from_key_label("n"));
    type_text(state, username);
    state.apply_input(InputEvent::from_key_label("Tab"));
    type_text(state, username);
    state.apply_input(InputEvent::from_key_label("Tab"));
    if admin {
        state.apply_input(InputEvent::from_key_label(" "));
    }
    state.apply_input(InputEvent::from_key_label("Tab"));
    type_text(state, "ManagedPass123!");
    state.apply_input(InputEvent::from_key_label("Enter"));
    assert!(state.to_user_management_view_model().form.is_none());
    assert!(
        state
            .to_user_management_view_model()
            .users
            .iter()
            .any(|user| user.username == username)
    );
}

fn user_management_layout_for(state: &ShellSession) -> ui::UserManagementLayout {
    let area = ratatui::layout::Rect::new(0, 0, state.terminal_size().0, state.terminal_size().1);
    let ui::ShellLayout::Full { main, .. } = ui::compute_shell_layout(area) else {
        panic!("user management layout requires a full shell");
    };
    ui::user_management_layout(main, &state.to_user_management_view_model())
}

fn rect_center(area: ratatui::layout::Rect) -> (u16, u16) {
    (
        area.x.saturating_add(area.width / 2),
        area.y.saturating_add(area.height / 2),
    )
}

fn fresh_setup_state(case: &str) -> (FixtureRoot, ShellSession) {
    let fixture = FixtureRoot::new(case);
    let platform = mock_platform(fixture.path());
    let startup = prepare_shell_startup(&platform, default_config()).expect("startup");
    let state = ShellSession::new_with_startup(default_config(), (120, 40), startup);
    (fixture, state)
}

fn assert_setup_admin_empty(state: &ShellSession) {
    let model = state.to_setup_view_model();
    assert!(model.admin_username.is_empty());
    assert_eq!(model.admin_password_len, 0);
    assert_eq!(model.admin_password_confirm_len, 0);
    assert!(model.password_hint.is_empty());
    assert!(!model.can_submit);
}

fn assert_selected_timezone_visible(state: &ShellSession) {
    let model = state.to_setup_view_model();
    assert_eq!(model.step, ui::SetupStep::Timezone);

    let visible_rows = setup_hit_region_height(state, ShellComponent::SetupTimezone);
    assert!(visible_rows > 0);

    let visible_start = model.timezone_window_start;
    let visible_end = visible_start.saturating_add(visible_rows);
    assert!(
        model.selected_timezone_index >= visible_start
            && model.selected_timezone_index < visible_end,
        "selected timezone {} should be inside visible window {}..{}",
        model.selected_timezone_index,
        visible_start,
        visible_end
    );
}

fn assert_requirement(
    requirements: &[ui::SetupPasswordRequirementViewModel],
    label: &str,
    expected: bool,
) {
    let requirement = requirements
        .iter()
        .find(|requirement| requirement.label == label)
        .unwrap_or_else(|| panic!("missing password requirement: {label}"));
    assert_eq!(requirement.met, expected, "{label}");
}

fn setup_hit_components(state: &ShellSession) -> Vec<ShellComponent> {
    state
        .hit_map()
        .regions()
        .iter()
        .map(|region| region.component)
        .filter(|component| {
            matches!(
                component,
                ShellComponent::SetupLanguage
                    | ShellComponent::SetupTimezone
                    | ShellComponent::SetupAdminUsername
                    | ShellComponent::SetupAdminPassword
                    | ShellComponent::SetupAdminPasswordConfirm
                    | ShellComponent::SetupAdminHint
                    | ShellComponent::SetupSubmit
                    | ShellComponent::SetupAppearanceShape
                    | ShellComponent::SetupAppearanceThemeColor
                    | ShellComponent::SetupAppearanceThemeCustom
                    | ShellComponent::SetupAppearanceAccentColor
                    | ShellComponent::SetupAppearanceAccentCustom
                    | ShellComponent::SetupAppearanceSubmit
                    | ShellComponent::SetupCustomColorDialog
            )
        })
        .collect()
}

fn setup_hit_region_height(state: &ShellSession, component: ShellComponent) -> usize {
    state
        .hit_map()
        .regions()
        .iter()
        .find(|region| region.component == component)
        .map(|region| region.area.height as usize)
        .unwrap_or_else(|| panic!("missing hit region for {component:?}"))
}

fn setup_hit_map_row_coordinates(
    state: &ShellSession,
    component: ShellComponent,
    row: u16,
) -> (u16, u16) {
    let region = state
        .hit_map()
        .regions()
        .iter()
        .find(|region| region.component == component)
        .unwrap_or_else(|| panic!("missing hit region for {component:?}"));

    assert!(
        row < region.area.height,
        "row {row} outside {component:?} hit region height {}",
        region.area.height
    );

    (
        region.area.x.saturating_add(1),
        region.area.y.saturating_add(row),
    )
}

fn login_user_row_coordinates(state: &ShellSession, row: u16) -> (u16, u16) {
    let region = state
        .hit_map()
        .regions()
        .iter()
        .find(|region| region.component == ShellComponent::LoginUserList)
        .expect("missing login user list hit region");
    let content_height = region.area.height.saturating_sub(2);
    assert!(
        row < content_height,
        "row {row} outside login user list content height {content_height}"
    );

    (
        region.area.x.saturating_add(1),
        region.area.y.saturating_add(1).saturating_add(row),
    )
}

fn complete_first_run_setup(
    state: &mut ShellSession,
    language_steps: usize,
    timezone_steps: usize,
    username: &str,
    password: &str,
    hint: &str,
) {
    complete_first_run_admin(
        state,
        language_steps,
        timezone_steps,
        username,
        password,
        hint,
    );
    for _ in 0..5 {
        state.apply_input(InputEvent::from_key_label("Tab"));
    }
    assert_eq!(
        state.focused_component(),
        ShellComponent::SetupAppearanceSubmit
    );
    state.apply_input(InputEvent::from_key_label("Enter"));
}

fn complete_first_run_admin(
    state: &mut ShellSession,
    language_steps: usize,
    timezone_steps: usize,
    username: &str,
    password: &str,
    hint: &str,
) {
    assert_eq!(state.active_screen(), ShellScreen::FirstRunSetup);
    for _ in 0..language_steps {
        state.apply_input(InputEvent::from_key_label("Right"));
    }
    state.apply_input(InputEvent::from_key_label("Enter"));
    for _ in 0..timezone_steps {
        state.apply_input(InputEvent::from_key_label("Down"));
    }
    state.apply_input(InputEvent::from_key_label("Enter"));
    type_text(state, username);
    state.apply_input(InputEvent::from_key_label("Enter"));
    type_text(state, password);
    state.apply_input(InputEvent::from_key_label("Enter"));
    type_text(state, password);
    state.apply_input(InputEvent::from_key_label("Enter"));
    type_text(state, hint);
    state.apply_input(InputEvent::from_key_label("Enter"));
    assert_eq!(state.focused_component(), ShellComponent::SetupSubmit);
    state.apply_input(InputEvent::from_key_label("Enter"));
    assert_eq!(state.to_setup_view_model().step, ui::SetupStep::Appearance);
    assert_eq!(
        state.focused_component(),
        ShellComponent::SetupAppearanceShape
    );
}

fn setup_language_code(options: &[ui::SetupLanguageOption], requested_index: usize) -> String {
    options
        .get(requested_index)
        .or_else(|| options.first())
        .expect("setup catalog should not be empty")
        .code
        .clone()
}

fn setup_timezone_id(options: &[ui::SetupTimezoneOption], requested_index: usize) -> String {
    options
        .get(requested_index)
        .or_else(|| options.first())
        .expect("setup catalog should not be empty")
        .id
        .clone()
}

fn setup_admin_coordinates(state: &ShellSession, component: ShellComponent) -> (u16, u16) {
    let area = ratatui::layout::Rect::new(0, 0, state.terminal_size().0, state.terminal_size().1);
    let ui::ShellLayout::Full { main, .. } = ui::compute_shell_layout(area) else {
        panic!("phase5 setup tests use a full shell layout");
    };
    let field = match component {
        ShellComponent::SetupAdminUsername => ui::SetupField::AdminUsername,
        ShellComponent::SetupAdminPassword => ui::SetupField::AdminPassword,
        ShellComponent::SetupAdminPasswordConfirm => ui::SetupField::AdminPasswordConfirm,
        ShellComponent::SetupAdminHint => ui::SetupField::PasswordHint,
        ShellComponent::SetupSubmit => ui::SetupField::Submit,
        other => panic!("unexpected setup component: {other:?}"),
    };
    let field_area = ui::setup_admin_field_area(main, field);
    (field_area.x, field_area.y)
}

fn setup_field_for_admin_component(component: ShellComponent) -> ui::SetupField {
    match component {
        ShellComponent::SetupAdminUsername => ui::SetupField::AdminUsername,
        ShellComponent::SetupAdminPassword => ui::SetupField::AdminPassword,
        ShellComponent::SetupAdminPasswordConfirm => ui::SetupField::AdminPasswordConfirm,
        ShellComponent::SetupAdminHint => ui::SetupField::PasswordHint,
        ShellComponent::SetupSubmit => ui::SetupField::Submit,
        other => panic!("unexpected setup component: {other:?}"),
    }
}

fn mock_platform(base: &Path) -> MockPlatform {
    let app_paths =
        build_windows_app_paths(base.join("Roaming"), base.join("Local"), base.join("Temp"))
            .expect("fixture app paths should resolve");
    MockPlatform::new(user_dirs(base), app_paths)
        .with_kind(PlatformKind::Windows)
        .with_capabilities(PlatformCapabilities::native_supported())
}

fn user_dirs(base: &Path) -> UserDirs {
    UserDirs::new(
        base.join("Desktop"),
        base.join("Documents"),
        base.join("Downloads"),
        base.join("Pictures"),
        base.join("Videos"),
        base.join("Music"),
        base.join("Roaming"),
    )
    .expect("fixture user directories should resolve")
}

struct FixtureRoot {
    path: PathBuf,
}

impl FixtureRoot {
    fn new(case: &str) -> Self {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "tundra-shell-phase5-{}-{nanos}-{case}",
            std::process::id()
        ));
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for FixtureRoot {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}
