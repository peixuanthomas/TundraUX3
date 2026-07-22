use std::fs;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use identity::{SessionService, UserRole, UserService};
use platform::mock::{MockCall, MockPlatform};
use platform::{
    AppPaths, PlatformCapabilities, PlatformKind, UserDirs, build_windows_app_paths,
    cleanup_temp_path,
};
use shell::{
    HomeModeOverride, InputEvent, ShellLaunchConfig, ShellScreen, ShellSession,
    prepare_shell_startup,
};
use storage::{BorderShape, StorageManager, TimeSyncSource};
use ui::{SettingsCategory, SettingsField};

fn default_config() -> ShellLaunchConfig {
    ShellLaunchConfig {
        home_mode_override: HomeModeOverride::BuildDefault,
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
    for _ in 0..2 {
        press(&mut state, &platform, "Down");
    }
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
fn operating_system_time_source_uses_platform_boundary_and_persists() {
    let fixture = FixtureRoot::new("system-time-source");
    let platform = mock_platform(fixture.path());
    let manager = initialize_users(&platform, false, false);
    let system_time = UNIX_EPOCH + Duration::from_secs(1_700_000_000);
    platform.set_system_time_result(Ok(system_time));
    let mut state = logged_in_state(&platform, "AdminUser", "StrongPass123");

    open_settings_from_home(&mut state, &platform);
    press(&mut state, &platform, "Right");
    press(&mut state, &platform, "Enter");
    press(&mut state, &platform, "Down");
    press(&mut state, &platform, "Down");
    press(&mut state, &platform, "Down");
    assert_eq!(
        state.to_settings_view_model().unwrap().selected_field,
        SettingsField::TimeSyncSource
    );
    press(&mut state, &platform, "Enter");

    assert_eq!(
        manager.load_config().unwrap().time_sync.source,
        TimeSyncSource::OperatingSystem
    );
    assert!(
        platform
            .calls()
            .iter()
            .any(|call| matches!(call, MockCall::SystemTime))
    );
    assert!(!state.time_sync_failure_dialog_visible());
}

#[test]
fn time_server_is_saved_only_after_successful_synchronization() {
    let fixture = FixtureRoot::new("valid-time-server");
    let platform = mock_platform(fixture.path());
    let manager = initialize_users(&platform, false, false);
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind test time server");
    let address = listener.local_addr().expect("time server address");
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept time request");
        let mut request = [0_u8; 2048];
        let _ = stream.read(&mut request);
        stream
            .write_all(
                b"HTTP/1.1 204 No Content\r\nDate: Tue, 15 Nov 1994 08:12:31 GMT\r\nConnection: close\r\n\r\n",
            )
            .expect("write time response");
    });
    let mut state = logged_in_state(&platform, "AdminUser", "StrongPass123");

    open_time_server_editor(&mut state, &platform);
    let url = format!("http://{address}/clock");
    for character in url.chars() {
        press(&mut state, &platform, &character.to_string());
    }
    press(&mut state, &platform, "Enter");
    assert_eq!(manager.load_config().unwrap().time_sync.server_url, None);

    drive_settings_until(&mut state, &platform, |state| {
        state
            .to_settings_view_model()
            .is_some_and(|model| model.time_sync_server_editor.is_none())
    });
    server.join().expect("time server thread");
    assert_eq!(
        manager
            .load_config()
            .unwrap()
            .time_sync
            .server_url
            .as_deref(),
        Some(format!("http://{address}/clock").as_str())
    );
}

#[test]
fn failed_time_server_validation_shows_error_and_does_not_save() {
    let fixture = FixtureRoot::new("invalid-time-server");
    let platform = mock_platform(fixture.path());
    let manager = initialize_users(&platform, false, false);
    let listener = TcpListener::bind("127.0.0.1:0").expect("reserve unused port");
    let address = listener.local_addr().expect("unused address");
    drop(listener);
    let mut state = logged_in_state(&platform, "AdminUser", "StrongPass123");

    open_time_server_editor(&mut state, &platform);
    for character in format!("http://{address}/clock").chars() {
        press(&mut state, &platform, &character.to_string());
    }
    press(&mut state, &platform, "Enter");
    drive_settings_until(&mut state, &platform, |state| {
        state.time_sync_failure_dialog_visible()
    });

    assert_eq!(manager.load_config().unwrap().time_sync.server_url, None);
    assert!(
        state
            .time_sync_failure_message()
            .is_some_and(|message| message.contains("setting was not saved"))
    );
}

#[test]
fn malformed_time_server_shows_error_and_does_not_save() {
    let fixture = FixtureRoot::new("malformed-time-server");
    let platform = mock_platform(fixture.path());
    let manager = initialize_users(&platform, false, false);
    let mut state = logged_in_state(&platform, "AdminUser", "StrongPass123");

    open_time_server_editor(&mut state, &platform);
    for character in "not a URL".chars() {
        press(&mut state, &platform, &character.to_string());
    }
    press(&mut state, &platform, "Enter");

    assert_eq!(manager.load_config().unwrap().time_sync.server_url, None);
    assert!(state.time_sync_failure_dialog_visible());
    assert!(
        state
            .time_sync_failure_message()
            .is_some_and(|message| message.contains("setting was not saved"))
    );
}

#[test]
fn editor_settings_save_normalized_explorer_open_suffixes() {
    let fixture = FixtureRoot::new("editor-open-suffixes");
    let platform = mock_platform(fixture.path());
    let manager = initialize_users(&platform, false, false);
    let mut state = logged_in_state(&platform, "AdminUser", "StrongPass123");

    open_settings_from_home(&mut state, &platform);
    for _ in 0..3 {
        press(&mut state, &platform, "Right");
    }
    press(&mut state, &platform, "Enter");
    assert_eq!(
        state.to_settings_view_model().unwrap().selected_field,
        SettingsField::ExplorerOpenExtensions
    );

    press(&mut state, &platform, "Enter");
    let existing_len = state
        .to_settings_view_model()
        .unwrap()
        .file_extensions_editor
        .expect("file suffix editor")
        .value
        .chars()
        .count();
    for _ in 0..existing_len {
        press(&mut state, &platform, "Backspace");
    }
    for character in ".RS, rs; .d.ts".chars() {
        press(&mut state, &platform, &character.to_string());
    }
    press(&mut state, &platform, "Enter");

    assert_eq!(
        manager
            .load_config()
            .unwrap()
            .editor
            .explorer_open_extensions,
        vec!["rs".to_string(), "d.ts".to_string()]
    );
    assert!(
        state
            .to_settings_view_model()
            .unwrap()
            .file_extensions_editor
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

fn logged_in_state(platform: &MockPlatform, username: &str, password: &str) -> ShellSession {
    let startup = prepare_shell_startup(platform).expect("startup");
    let mut state = ShellSession::new_with_startup(default_config(), (120, 40), startup);
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

fn open_settings_from_home(state: &mut ShellSession, platform: &MockPlatform) {
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

fn open_time_server_editor(state: &mut ShellSession, platform: &MockPlatform) {
    open_settings_from_home(state, platform);
    press(state, platform, "Right");
    press(state, platform, "Enter");
    for _ in 0..4 {
        press(state, platform, "Down");
    }
    assert_eq!(
        state.to_settings_view_model().unwrap().selected_field,
        SettingsField::TimeSyncServer
    );
    press(state, platform, "Enter");
    assert!(
        state
            .to_settings_view_model()
            .unwrap()
            .time_sync_server_editor
            .is_some()
    );
}

fn drive_settings_until(
    state: &mut ShellSession,
    platform: &MockPlatform,
    done: impl Fn(&ShellSession) -> bool,
) {
    for _ in 0..1_000 {
        state.apply_input_with_platform(InputEvent::Tick, platform);
        if done(state) {
            return;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    panic!("Settings background task did not finish in time");
}

fn press(state: &mut ShellSession, platform: &MockPlatform, key: &str) {
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
