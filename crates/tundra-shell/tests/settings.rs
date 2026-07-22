use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use tundra_core::{SessionService, UserRole, UserService};
use tundra_platform::mock::MockPlatform;
use tundra_platform::{
    AppPaths, PlatformCapabilities, PlatformKind, UserDirs, build_windows_app_paths,
    cleanup_temp_path,
};
use tundra_shell::{
    HomeModeOverride, InputEvent, ShellLaunchConfig, ShellLaunchTarget, ShellScreen, ShellState,
    ShellTerminalMode, prepare_shell_startup,
};
use tundra_storage::{BorderShape, StorageManager};
use tundra_ui::{SettingsCategory, SettingsField};

fn default_config() -> ShellLaunchConfig {
    ShellLaunchConfig {
        terminal_mode: ShellTerminalMode::Fullscreen,
        home_mode_override: HomeModeOverride::BuildDefault,
        launch_target: ShellLaunchTarget::Home,
    }
}

#[test]
fn admin_settings_immediately_persist_global_changes_and_confirm_picker_selection() {
    let fixture = FixtureRoot::new("admin-persistence");
    let platform = mock_platform(fixture.path());
    let manager = initialize_users(&platform, false, false);
    let mut state = logged_in_state(&platform, "AdminUser", "StrongPass123");

    open_settings_from_home(&mut state, &platform);
    assert_eq!(state.active_screen(), ShellScreen::Settings);

    // Appearance -> Region & Time. Moving the timezone highlight must not save.
    press(&mut state, &platform, "Right");
    press(&mut state, &platform, "Enter");
    press(&mut state, &platform, "Down");
    assert_eq!(
        state.to_settings_view_model().unwrap().selected_field,
        SettingsField::Timezone
    );
    let original_timezone = manager.load_config().unwrap().timezone;
    press(&mut state, &platform, "Enter");
    press(&mut state, &platform, "Down");
    let highlighted_timezone = state
        .to_settings_view_model()
        .unwrap()
        .picker
        .unwrap()
        .options
        .into_iter()
        .nth(1)
        .and_then(|option| option.timezone_id)
        .expect("second timezone option");
    assert_eq!(manager.load_config().unwrap().timezone, original_timezone);
    press(&mut state, &platform, "Enter");
    assert_eq!(
        manager.load_config().unwrap().timezone,
        highlighted_timezone
    );

    // Return to category navigation, choose File Explorer, and toggle immediately.
    press(&mut state, &platform, "Tab");
    press(&mut state, &platform, "Right");
    press(&mut state, &platform, "Enter");
    assert_eq!(
        state.to_settings_view_model().unwrap().selected_category,
        SettingsCategory::FileExplorer
    );
    assert!(!manager.load_config().unwrap().explorer.show_hidden);
    press(&mut state, &platform, "Enter");
    assert!(manager.load_config().unwrap().explorer.show_hidden);
    assert!(
        state
            .to_settings_view_model()
            .unwrap()
            .status
            .contains("Saved")
    );
}

#[test]
fn weather_location_rejects_non_english_input_and_saves_only_after_warning() {
    let fixture = FixtureRoot::new("weather-location");
    let platform = mock_platform(fixture.path());
    let manager = initialize_users(&platform, false, false);
    let mut state = logged_in_state(&platform, "AdminUser", "StrongPass123");

    open_settings_from_home(&mut state, &platform);
    press(&mut state, &platform, "Right");
    press(&mut state, &platform, "Enter");
    press(&mut state, &platform, "Down");
    press(&mut state, &platform, "Down");
    assert_eq!(
        state.to_settings_view_model().unwrap().selected_field,
        SettingsField::WeatherLocation
    );
    press(&mut state, &platform, "Enter");

    press(&mut state, &platform, "杭");
    let editor = state
        .to_settings_view_model()
        .unwrap()
        .weather_location_editor
        .expect("weather location editor");
    assert!(editor.value.is_empty());
    assert!(editor.error.unwrap().contains("Only English"));

    let location = "Cambridge, Massachusetts, USA";
    for character in location.chars() {
        press(&mut state, &platform, &character.to_string());
    }
    press(&mut state, &platform, "Enter");
    assert_eq!(manager.load_config().unwrap().weather_location, None);
    let warning = state
        .to_notification_view_model()
        .expect("weather search warning");
    assert!(warning.message.contains("may be inaccurate"));
    assert!(warning.message.contains("no results"));

    press(&mut state, &platform, "Enter");
    assert_eq!(
        manager.load_config().unwrap().weather_location.as_deref(),
        Some(location)
    );
    assert!(
        state
            .to_settings_view_model()
            .unwrap()
            .weather_location_editor
            .is_none()
    );
}

#[test]
fn normal_user_can_change_only_their_appearance() {
    let fixture = FixtureRoot::new("user-permissions");
    let platform = mock_platform(fixture.path());
    let manager = initialize_users(&platform, true, false);
    let mut state = logged_in_state(&platform, "NormalUser", "NormalPass123");

    open_settings_from_home(&mut state, &platform);
    let appearance = state.to_settings_view_model().unwrap();
    assert!(appearance.locked_message.is_none());
    assert!(
        appearance
            .cards
            .iter()
            .flat_map(|card| &card.items)
            .all(|item| item.enabled)
    );

    press(&mut state, &platform, "Enter");
    press(&mut state, &platform, "Enter");
    let users = manager.load_users().unwrap();
    assert_eq!(
        users
            .users
            .iter()
            .find(|user| user.username == "NormalUser")
            .unwrap()
            .appearance
            .border_shape,
        BorderShape::Square
    );
    assert_eq!(
        users
            .users
            .iter()
            .find(|user| user.username == "AdminUser")
            .unwrap()
            .appearance
            .border_shape,
        BorderShape::Rounded
    );

    press(&mut state, &platform, "Tab");
    press(&mut state, &platform, "Right");
    let region = state.to_settings_view_model().unwrap();
    assert_eq!(region.selected_category, SettingsCategory::RegionTime);
    assert!(
        region
            .locked_message
            .as_deref()
            .unwrap()
            .contains("administrator")
    );
    assert!(
        region
            .cards
            .iter()
            .flat_map(|card| &card.items)
            .all(|item| !item.enabled)
    );

    let config_before = manager.load_config().unwrap();
    press(&mut state, &platform, "Enter");
    press(&mut state, &platform, "Enter");
    assert_eq!(manager.load_config().unwrap(), config_before);
    assert!(
        state
            .to_settings_view_model()
            .unwrap()
            .status
            .starts_with("Error:")
    );
}

#[test]
fn guest_home_does_not_expose_settings() {
    let fixture = FixtureRoot::new("guest-hidden");
    let platform = mock_platform(fixture.path());
    initialize_users(&platform, false, true);
    let state = logged_in_state(&platform, "GuestUser", "GuestPass123");

    assert_eq!(state.active_screen(), ShellScreen::Home);
    assert!(
        state
            .to_home_view_model()
            .entries()
            .iter()
            .all(|entry| entry.label != "Settings")
    );
}

fn initialize_users(
    platform: &MockPlatform,
    create_user: bool,
    create_guest: bool,
) -> StorageManager {
    let manager = StorageManager::open_from_platform(platform)
        .expect("storage open")
        .manager;
    let users = UserService::new(manager.clone());
    users
        .bootstrap_admin("AdminUser", "StrongPass123")
        .expect("bootstrap admin");
    let admin = SessionService::new(manager.clone())
        .login("AdminUser", "StrongPass123")
        .expect("admin login");
    if create_user {
        users
            .create_user(
                &admin,
                "NormalUser",
                "Normal User",
                UserRole::User,
                "NormalPass123",
            )
            .expect("create normal user");
    }
    if create_guest {
        users
            .create_user(
                &admin,
                "GuestUser",
                "Guest User",
                UserRole::Guest,
                "GuestPass123",
            )
            .expect("create guest user");
    }
    manager
}

fn logged_in_state(platform: &MockPlatform, username: &str, password: &str) -> ShellState {
    let startup = prepare_shell_startup(platform, default_config()).expect("startup");
    let mut state = ShellState::new_with_startup(default_config(), (120, 40), startup);
    assert_eq!(state.active_screen(), ShellScreen::Login);
    let target = state
        .to_login_view_model()
        .users
        .iter()
        .position(|user| user.username == username)
        .expect("login user");
    while state.to_login_view_model().selected_index < target {
        press(&mut state, platform, "Down");
    }
    press(&mut state, platform, "Tab");
    for character in password.chars() {
        state
            .apply_input_with_platform(InputEvent::from_key_label(character.to_string()), platform);
    }
    press(&mut state, platform, "Enter");
    assert_eq!(state.active_screen(), ShellScreen::Home);
    state
}

fn open_settings_from_home(state: &mut ShellState, platform: &MockPlatform) {
    let target = state
        .to_home_view_model()
        .entries()
        .iter()
        .position(|entry| entry.label == "Settings")
        .expect("Settings home entry");
    while state.selected_home_entry_index() < target {
        press(state, platform, "Right");
    }
    press(state, platform, "Enter");
}

fn press(state: &mut ShellState, platform: &MockPlatform, key: &str) {
    state.apply_input_with_platform(InputEvent::from_key_label(key), platform);
}

fn mock_platform(base: &Path) -> MockPlatform {
    for directory in [
        "Desktop",
        "Documents",
        "Downloads",
        "Pictures",
        "Videos",
        "Music",
        "Roaming",
        "Local",
        "Temp",
    ] {
        fs::create_dir_all(base.join(directory)).expect("fixture directory");
    }
    MockPlatform::new(user_dirs(base), app_paths(base))
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
    .expect("fixture user directories")
}

fn app_paths(base: &Path) -> AppPaths {
    build_windows_app_paths(base.join("Roaming"), base.join("Local"), base.join("Temp"))
        .expect("fixture app paths")
}

struct FixtureRoot {
    path: PathBuf,
}

impl FixtureRoot {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "tundra-shell-settings-{name}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        fs::create_dir_all(&path).expect("create fixture root");
        Self {
            path: fs::canonicalize(path).expect("canonicalize fixture root"),
        }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for FixtureRoot {
    fn drop(&mut self) {
        let _ = cleanup_temp_path(&self.path);
    }
}
