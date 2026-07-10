use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use tundra_platform::mock::MockPlatform;
use tundra_platform::{PlatformCapabilities, PlatformKind, UserDirs, build_windows_app_paths};
use tundra_shell::{
    HomeModeOverride, InputEvent, PointerButton, ShellComponent, ShellHomeMode, ShellLaunchConfig,
    ShellScreen, ShellState, ShellTerminalMode, prepare_shell_startup,
};
use tundra_ui::NotificationTone;

fn debug_config() -> ShellLaunchConfig {
    ShellLaunchConfig {
        terminal_mode: ShellTerminalMode::Fullscreen,
        home_mode_override: HomeModeOverride::Debug,
    }
}

fn default_config() -> ShellLaunchConfig {
    ShellLaunchConfig {
        terminal_mode: ShellTerminalMode::Fullscreen,
        home_mode_override: HomeModeOverride::BuildDefault,
    }
}

#[test]
fn fresh_startup_requires_first_run_setup_and_debug_flag_does_not_bypass_auth() {
    let fixture = FixtureRoot::new("fresh-bootstrap");
    let platform = mock_platform(fixture.path());
    let startup = prepare_shell_startup(&platform, debug_config()).expect("startup");

    let state = ShellState::new_with_startup(debug_config(), (120, 40), startup);

    assert_eq!(state.active_screen(), ShellScreen::FirstRunSetup);
    assert_eq!(state.auth_session(), None);
    assert_eq!(
        state.to_setup_view_model().step,
        tundra_ui::SetupStep::Language
    );
}

#[test]
fn first_run_setup_signs_in_persists_config_and_hint_without_plaintext_password() {
    let fixture = FixtureRoot::new("first-run-setup");
    let platform = mock_platform(fixture.path());
    let startup = prepare_shell_startup(&platform, default_config()).expect("startup");
    let manager = startup.storage_manager.clone().expect("storage manager");
    let mut state = ShellState::new_with_startup(default_config(), (120, 40), startup);

    let expected_language = setup_language_code(&tundra_ui::setup_language_options(), 1);
    let expected_timezone = setup_timezone_id(&tundra_ui::setup_timezone_options(), 2);

    complete_first_run_setup(&mut state, 1, 2, "AdminUser", "StrongPass123", "First pet");

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
fn first_run_setup_enter_advances_language_timezone_admin_pages() {
    let (_fixture, mut state) = fresh_setup_state("setup-page-transitions");

    assert_eq!(
        state.to_setup_view_model().step,
        tundra_ui::SetupStep::Language
    );
    assert_eq!(state.focused_component(), ShellComponent::SetupLanguage);
    assert_eq!(
        setup_hit_components(&state),
        vec![ShellComponent::SetupLanguage]
    );

    state.apply_input(InputEvent::from_key_label("Enter"));
    assert_eq!(
        state.to_setup_view_model().step,
        tundra_ui::SetupStep::Timezone
    );
    assert_eq!(state.focused_component(), ShellComponent::SetupTimezone);
    assert_eq!(
        setup_hit_components(&state),
        vec![ShellComponent::SetupTimezone]
    );

    state.apply_input(InputEvent::from_key_label("Enter"));
    assert_eq!(
        state.to_setup_view_model().step,
        tundra_ui::SetupStep::Admin
    );
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
    assert_eq!(model.step, tundra_ui::SetupStep::Timezone);
    assert_eq!(model.focused_field, tundra_ui::SetupField::TimezoneList);
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
    assert_eq!(
        state.to_setup_view_model().step,
        tundra_ui::SetupStep::Language
    );
    assert_setup_admin_empty(&state);

    state.apply_input(InputEvent::from_key_label("Enter"));
    assert_eq!(
        state.to_setup_view_model().step,
        tundra_ui::SetupStep::Timezone
    );

    type_text(&mut state, "StrongPass123");
    state.apply_input(InputEvent::from_key_label("Backspace"));
    state.apply_input(InputEvent::mouse_down(
        PointerButton::Left,
        setup_admin_coordinates(&state, ShellComponent::SetupAdminUsername),
    ));

    let model = state.to_setup_view_model();
    assert_eq!(model.step, tundra_ui::SetupStep::Timezone);
    assert_eq!(model.focused_field, tundra_ui::SetupField::TimezoneList);
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
        tundra_ui::setup_timezone_options().len().saturating_sub(1)
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
    assert_eq!(
        state.to_setup_view_model().step,
        tundra_ui::SetupStep::Admin
    );

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
    assert_eq!(
        model.focused_field,
        tundra_ui::SetupField::AdminPasswordConfirm
    );
    assert!(!model.can_submit);
    assert_eq!(model.error.as_deref(), Some("Passwords do not match"));
    assert_requirement(&model.password_requirements, "Passwords match", false);
}

#[test]
fn first_run_setup_routes_keys_focus_and_mouse_before_home_shortcuts() {
    let fixture = FixtureRoot::new("setup-routing");
    let platform = mock_platform(fixture.path());
    let startup = prepare_shell_startup(&platform, default_config()).expect("startup");
    let mut state = ShellState::new_with_startup(default_config(), (120, 40), startup);

    state.apply_input(InputEvent::from_key_label("e"));
    state.apply_input(InputEvent::from_key_label("u"));
    assert_eq!(state.active_screen(), ShellScreen::FirstRunSetup);
    assert_eq!(state.active_popup(), None);

    state.apply_input(InputEvent::mouse_down(PointerButton::Right, (4, 4)));
    assert_eq!(state.active_popup(), None);

    state.apply_input(InputEvent::from_key_label("Right"));
    assert_eq!(state.to_setup_view_model().selected_language_index, 1);
    state.apply_input(InputEvent::mouse_down(
        PointerButton::Left,
        setup_hit_map_row_coordinates(&state, ShellComponent::SetupLanguage, 0),
    ));
    assert_eq!(state.to_setup_view_model().selected_language_index, 0);

    state.apply_input(InputEvent::from_key_label("Enter"));
    assert_eq!(
        state.to_setup_view_model().step,
        tundra_ui::SetupStep::Timezone
    );
    state.apply_input(InputEvent::from_key_label("Down"));
    assert_eq!(state.to_setup_view_model().selected_timezone_index, 1);
    state.apply_input(InputEvent::from_key_label("PageDown"));
    assert!(state.to_setup_view_model().selected_timezone_index > 1);
    state.apply_input(InputEvent::from_key_label("Home"));
    assert_eq!(state.to_setup_view_model().selected_timezone_index, 0);
    state.apply_input(InputEvent::from_key_label("End"));
    let last_timezone = tundra_ui::setup_timezone_options().len().saturating_sub(1);
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
    let mut state = ShellState::new_with_startup(default_config(), (120, 40), startup);

    state.apply_input(InputEvent::from_key_label("Esc"));
    assert_eq!(state.active_screen(), ShellScreen::ExitConfirm);
    state.apply_input(InputEvent::from_key_label("n"));
    assert_eq!(state.active_screen(), ShellScreen::FirstRunSetup);

    let action = state.apply_input(InputEvent::from_key_label("Ctrl+C"));
    assert_eq!(action, tundra_shell::ShellAction::Exit);
    assert!(state.shutdown_requested());
}

#[test]
fn restart_requires_login_and_bad_password_stays_on_login() {
    let fixture = FixtureRoot::new("restart-login");
    let platform = mock_platform(fixture.path());
    bootstrap_with_shell(&platform);

    let startup = prepare_shell_startup(&platform, default_config()).expect("restart startup");
    let mut state = ShellState::new_with_startup(default_config(), (120, 40), startup);
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
    let mut state = ShellState::new_with_startup(default_config(), (120, 40), startup);
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
    let mut state = ShellState::new_with_startup(default_config(), (120, 40), startup);
    complete_first_run_setup(&mut state, 0, 0, "AdminUser", "StrongPass123", "");
    assert_eq!(state.active_screen(), ShellScreen::Home);

    let startup = prepare_shell_startup(&platform, default_config()).expect("restart startup");
    let mut state = ShellState::new_with_startup(default_config(), (120, 40), startup);
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
fn admin_can_manage_users_and_user_can_only_open_own_profile() {
    let fixture = FixtureRoot::new("manage-users");
    let platform = mock_platform(fixture.path());
    bootstrap_with_shell(&platform);

    let startup = prepare_shell_startup(&platform, default_config()).expect("admin startup");
    let mut admin_state = ShellState::new_with_startup(default_config(), (120, 40), startup);
    login(&mut admin_state, "AdminUser", "StrongPass123");
    admin_state.apply_input(InputEvent::from_key_label("u"));
    assert_eq!(admin_state.active_screen(), ShellScreen::UserManagement);

    admin_state.apply_input(InputEvent::from_key_label("n"));
    type_text(&mut admin_state, "user2");
    admin_state.apply_input(InputEvent::from_key_label("Tab"));
    type_text(&mut admin_state, "User Two");
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

    let startup = prepare_shell_startup(&platform, default_config()).expect("user startup");
    let mut user_state = ShellState::new_with_startup(default_config(), (120, 40), startup);
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
    let mut state = ShellState::new_with_startup(default_config(), (120, 40), startup);
    login(&mut state, "AdminUser", "StrongPass123");
    state.apply_input(InputEvent::from_key_label("u"));
    assert_eq!(state.active_screen(), ShellScreen::UserManagement);

    let users_before_failure = state.to_user_management_view_model().users;
    let valid_users = fs::read(&users_path).expect("valid users document");
    fs::write(&users_path, b"{ invalid users json").expect("corrupt users fixture");

    state.apply_input(InputEvent::from_key_label("c"));

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
    state.apply_input(InputEvent::from_key_label("c"));

    let recovered_management = state.to_user_management_view_model();
    assert_eq!(recovered_management.users.len(), users_before_failure.len());
    assert!(
        recovered_management
            .users
            .iter()
            .any(|user| user.username == "AdminUser" && user.role == "Debug")
    );
    assert_eq!(state.to_shell_chrome_view_model().status.error, None);
}

#[test]
fn login_mouse_click_selects_user_and_focuses_password() {
    let fixture = FixtureRoot::new("login-mouse-user-select");
    let platform = mock_platform(fixture.path());
    bootstrap_with_shell(&platform);

    let startup = prepare_shell_startup(&platform, default_config()).expect("admin startup");
    let mut admin_state = ShellState::new_with_startup(default_config(), (120, 40), startup);
    login(&mut admin_state, "AdminUser", "StrongPass123");
    admin_state.apply_input(InputEvent::from_key_label("u"));
    admin_state.apply_input(InputEvent::from_key_label("n"));
    type_text(&mut admin_state, "user2");
    admin_state.apply_input(InputEvent::from_key_label("Tab"));
    type_text(&mut admin_state, "User Two");
    admin_state.apply_input(InputEvent::from_key_label("Tab"));
    type_text(&mut admin_state, "userPass2123!");
    admin_state.apply_input(InputEvent::from_key_label("Enter"));

    let startup = prepare_shell_startup(&platform, default_config()).expect("user startup");
    let mut state = ShellState::new_with_startup(default_config(), (120, 40), startup);
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
    let mut state = ShellState::new_with_startup(default_config(), (120, 40), startup);
    login(&mut state, "AdminUser", "StrongPass123");
    state.apply_input(InputEvent::from_key_label("u"));

    state.apply_input(InputEvent::from_key_label("n"));
    type_text(&mut state, "deleteme");
    state.apply_input(InputEvent::from_key_label("Tab"));
    type_text(&mut state, "Delete Me");
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

    state.apply_input(InputEvent::from_key_label("r"));
    type_text(&mut state, "ChangedPass123!");
    state.apply_input(InputEvent::from_key_label("Enter"));
    state.apply_input(InputEvent::from_key_label("x"));
    assert!(
        !state
            .to_user_management_view_model()
            .users
            .iter()
            .any(|user| user.username == "deleteme")
    );
}

fn bootstrap_with_shell(platform: &MockPlatform) {
    let startup = prepare_shell_startup(platform, default_config()).expect("startup");
    let mut state = ShellState::new_with_startup(default_config(), (120, 40), startup);
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

fn login(state: &mut ShellState, username: &str, password: &str) {
    select_login_user(state, username);
    state.apply_input(InputEvent::from_key_label("Tab"));
    type_text(state, password);
    state.apply_input(InputEvent::from_key_label("Enter"));
}

fn select_login_user(state: &mut ShellState, username: &str) {
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

fn type_text(state: &mut ShellState, text: &str) {
    for character in text.chars() {
        state.apply_input(InputEvent::from_key_label(character.to_string()));
    }
}

fn fresh_setup_state(case: &str) -> (FixtureRoot, ShellState) {
    let fixture = FixtureRoot::new(case);
    let platform = mock_platform(fixture.path());
    let startup = prepare_shell_startup(&platform, default_config()).expect("startup");
    let state = ShellState::new_with_startup(default_config(), (120, 40), startup);
    (fixture, state)
}

fn assert_setup_admin_empty(state: &ShellState) {
    let model = state.to_setup_view_model();
    assert!(model.admin_username.is_empty());
    assert_eq!(model.admin_password_len, 0);
    assert_eq!(model.admin_password_confirm_len, 0);
    assert!(model.password_hint.is_empty());
    assert!(!model.can_submit);
}

fn assert_selected_timezone_visible(state: &ShellState) {
    let model = state.to_setup_view_model();
    assert_eq!(model.step, tundra_ui::SetupStep::Timezone);

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
    requirements: &[tundra_ui::SetupPasswordRequirementViewModel],
    label: &str,
    expected: bool,
) {
    let requirement = requirements
        .iter()
        .find(|requirement| requirement.label == label)
        .unwrap_or_else(|| panic!("missing password requirement: {label}"));
    assert_eq!(requirement.met, expected, "{label}");
}

fn setup_hit_components(state: &ShellState) -> Vec<ShellComponent> {
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
            )
        })
        .collect()
}

fn setup_hit_region_height(state: &ShellState, component: ShellComponent) -> usize {
    state
        .hit_map()
        .regions()
        .iter()
        .find(|region| region.component == component)
        .map(|region| region.area.height as usize)
        .unwrap_or_else(|| panic!("missing hit region for {component:?}"))
}

fn setup_hit_map_row_coordinates(
    state: &ShellState,
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

fn login_user_row_coordinates(state: &ShellState, row: u16) -> (u16, u16) {
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
    state: &mut ShellState,
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
}

fn setup_language_code(
    options: &[tundra_ui::SetupLanguageOption],
    requested_index: usize,
) -> String {
    options
        .get(requested_index)
        .or_else(|| options.first())
        .expect("setup catalog should not be empty")
        .code
        .clone()
}

fn setup_timezone_id(options: &[tundra_ui::SetupTimezoneOption], requested_index: usize) -> String {
    options
        .get(requested_index)
        .or_else(|| options.first())
        .expect("setup catalog should not be empty")
        .id
        .clone()
}

fn setup_admin_coordinates(state: &ShellState, component: ShellComponent) -> (u16, u16) {
    let area = ratatui::layout::Rect::new(0, 0, state.terminal_size().0, state.terminal_size().1);
    let tundra_ui::ShellLayout::Full { main, .. } = tundra_ui::compute_shell_layout(area) else {
        panic!("phase5 setup tests use a full shell layout");
    };
    let field = match component {
        ShellComponent::SetupAdminUsername => tundra_ui::SetupField::AdminUsername,
        ShellComponent::SetupAdminPassword => tundra_ui::SetupField::AdminPassword,
        ShellComponent::SetupAdminPasswordConfirm => tundra_ui::SetupField::AdminPasswordConfirm,
        ShellComponent::SetupAdminHint => tundra_ui::SetupField::PasswordHint,
        ShellComponent::SetupSubmit => tundra_ui::SetupField::Submit,
        other => panic!("unexpected setup component: {other:?}"),
    };
    let field_area = tundra_ui::setup_admin_field_area(main, field);
    (field_area.x, field_area.y)
}

fn setup_field_for_admin_component(component: ShellComponent) -> tundra_ui::SetupField {
    match component {
        ShellComponent::SetupAdminUsername => tundra_ui::SetupField::AdminUsername,
        ShellComponent::SetupAdminPassword => tundra_ui::SetupField::AdminPassword,
        ShellComponent::SetupAdminPasswordConfirm => tundra_ui::SetupField::AdminPasswordConfirm,
        ShellComponent::SetupAdminHint => tundra_ui::SetupField::PasswordHint,
        ShellComponent::SetupSubmit => tundra_ui::SetupField::Submit,
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
