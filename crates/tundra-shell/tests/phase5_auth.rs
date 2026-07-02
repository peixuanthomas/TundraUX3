use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use tundra_platform::mock::MockPlatform;
use tundra_platform::{PlatformCapabilities, PlatformKind, UserDirs, build_windows_app_paths};
use tundra_shell::{
    HomeModeOverride, InputEvent, PointerButton, ShellComponent, ShellHomeMode, ShellLaunchConfig,
    ShellScreen, ShellState, ShellTerminalMode, prepare_shell_startup,
};

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
fn fresh_startup_requires_bootstrap_and_debug_flag_does_not_bypass_auth() {
    let fixture = FixtureRoot::new("fresh-bootstrap");
    let platform = mock_platform(fixture.path());
    let startup = prepare_shell_startup(&platform, debug_config()).expect("startup");

    let state = ShellState::new_with_startup(debug_config(), (120, 40), startup);

    assert_eq!(state.active_screen(), ShellScreen::BootstrapAdmin);
    assert_eq!(state.auth_session(), None);
}

#[test]
fn bootstrap_admin_signs_in_and_does_not_store_plaintext_password() {
    let fixture = FixtureRoot::new("bootstrap-admin");
    let platform = mock_platform(fixture.path());
    let startup = prepare_shell_startup(&platform, default_config()).expect("startup");
    let manager = startup.storage_manager.clone().expect("storage manager");
    let mut state = ShellState::new_with_startup(default_config(), (120, 40), startup);

    type_text(&mut state, "AdminUser");
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
    let stored = fs::read_to_string(&manager.layout().users_path).expect("users file");
    assert!(!stored.contains("StrongPass123"));
    assert!(stored.contains("$argon2"));
}

#[test]
fn bootstrap_form_can_focus_password_with_enter_tab_down_and_mouse() {
    let fixture = FixtureRoot::new("bootstrap-focus");
    let platform = mock_platform(fixture.path());
    let startup = prepare_shell_startup(&platform, default_config()).expect("startup");
    let mut state = ShellState::new_with_startup(default_config(), (120, 40), startup);

    type_text(&mut state, "AdminUser");
    state.apply_input(InputEvent::from_key_label("Enter"));
    assert_eq!(state.focused_component(), ShellComponent::BootstrapPassword);
    state.apply_input(InputEvent::from_key_label("Shift+Tab"));
    assert_eq!(state.focused_component(), ShellComponent::BootstrapUsername);
    state.apply_input(InputEvent::from_key_label("Down"));
    assert_eq!(state.focused_component(), ShellComponent::BootstrapPassword);
    state.apply_input(InputEvent::from_key_label("Up"));
    assert_eq!(state.focused_component(), ShellComponent::BootstrapUsername);

    state.apply_input(InputEvent::mouse_down(PointerButton::Left, (4, 7)));
    assert_eq!(state.focused_component(), ShellComponent::BootstrapPassword);
}

#[test]
fn restart_requires_login_and_bad_password_stays_on_login() {
    let fixture = FixtureRoot::new("restart-login");
    let platform = mock_platform(fixture.path());
    bootstrap_with_shell(&platform);

    let startup = prepare_shell_startup(&platform, default_config()).expect("restart startup");
    let mut state = ShellState::new_with_startup(default_config(), (120, 40), startup);
    assert_eq!(state.active_screen(), ShellScreen::Login);

    type_text(&mut state, "AdminUser");
    state.apply_input(InputEvent::from_key_label("Tab"));
    type_text(&mut state, "WrongPass123");
    state.apply_input(InputEvent::from_key_label("Enter"));

    assert_eq!(state.active_screen(), ShellScreen::Login);
    assert_eq!(state.auth_session(), None);

    let startup = prepare_shell_startup(&platform, default_config()).expect("second restart");
    let mut state = ShellState::new_with_startup(default_config(), (120, 40), startup);
    type_text(&mut state, "adminuser");
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
    type_text(&mut state, "AdminUser");
    state.apply_input(InputEvent::from_key_label("Tab"));
    type_text(&mut state, "StrongPass123");
    state.apply_input(InputEvent::from_key_label("Enter"));
    assert_eq!(state.active_screen(), ShellScreen::Home);
}

fn login(state: &mut ShellState, username: &str, password: &str) {
    type_text(state, username);
    state.apply_input(InputEvent::from_key_label("Tab"));
    type_text(state, password);
    state.apply_input(InputEvent::from_key_label("Enter"));
}

fn type_text(state: &mut ShellState, text: &str) {
    for character in text.chars() {
        state.apply_input(InputEvent::from_key_label(character.to_string()));
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
