use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use tundra_platform::mock::MockPlatform;
use tundra_platform::{
    AppPaths, Platform, UserDirs, build_macos_app_paths, build_windows_app_paths, cleanup_temp_path,
};
use tundra_storage::{
    AppearanceConfig, BorderShape, ClockDocument, ClockEntryRecord, ClockProfile, EditorConfig,
    ExplorerConfig, ExplorerDateZone, ExplorerSizeFormat, ExplorerSortDirection, ExplorerSortField,
    LauncherConfig, LauncherEntryRecord, LauncherExecutableKind, LauncherFingerprint,
    RecentFilesDocument, SCHEMA_VERSION, SecurityConfig, SessionsDocument, StateDocument,
    StorageConfig, StorageError, StorageLayout, StorageManager, TrashDocument, TrashRecord,
    USERS_SCHEMA_VERSION, UserRecord, UsersDocument,
};

#[test]
fn first_open_creates_storage_directories_and_default_files() {
    let base = unique_temp_root("first-open");
    let paths = app_paths(&base);
    let layout = StorageLayout::from_app_paths(&paths);

    let opened = StorageManager::open(paths).expect("storage should open from empty paths");

    assert!(layout.config_path.parent().expect("config parent").is_dir());
    assert!(layout.data_path.is_dir());
    assert!(layout.cache_path.is_dir());
    assert!(layout.logs_path.is_dir());
    assert!(layout.temp_path.is_dir());
    assert!(layout.config_path.is_file());
    assert!(layout.users_path.is_file());
    assert!(layout.state_path.is_file());
    assert!(layout.recent_files_path.is_file());
    assert!(layout.sessions_path.is_file());
    assert!(layout.clock_path.is_file());
    assert!(layout.trash_path.is_dir());
    assert!(layout.trash_manifest_path.is_file());
    assert_eq!(
        opened
            .manager
            .load_trash()
            .expect("default trash manifest")
            .records,
        Vec::<TrashRecord>::new()
    );
    assert_eq!(
        sorted_paths(opened.report.created_files),
        sorted_paths(vec![
            layout.config_path,
            layout.users_path,
            layout.state_path,
            layout.recent_files_path,
            layout.sessions_path,
            layout.clock_path,
            layout.trash_manifest_path,
        ])
    );
    assert!(opened.report.migrated_files.is_empty());
    assert!(opened.report.recovered_files.is_empty());
    assert!(opened.report.warnings.is_empty());

    cleanup(&base);
}

#[test]
fn toml_and_json_documents_round_trip() {
    let base = unique_temp_root("round-trip");
    let opened = StorageManager::open(app_paths(&base)).expect("storage should open");
    let manager = opened.manager;

    let mut shortcuts = BTreeMap::new();
    shortcuts.insert("open_launcher".to_string(), "Ctrl+Space".to_string());
    let config = StorageConfig {
        schema_version: SCHEMA_VERSION,
        theme: "light".to_string(),
        language: "zh-Hans".to_string(),
        timezone: "Asia/Shanghai".to_string(),
        shortcuts,
        appearance: AppearanceConfig {
            border_shape: BorderShape::Square,
        },
        explorer: ExplorerConfig {
            show_hidden: true,
            show_system: true,
            show_extensions: false,
            folders_first: false,
            case_sensitive_sort: true,
            size_format: ExplorerSizeFormat::Bytes,
            date_zone: ExplorerDateZone::Utc,
            confirm_delete: false,
            confirm_name_conflicts: false,
            show_sidebar: false,
            sort_field: ExplorerSortField::Modified,
            sort_direction: ExplorerSortDirection::Descending,
        },
        editor: EditorConfig {
            cursor_acceleration_enabled: false,
            cursor_acceleration_delay_ms: 1_500,
            cursor_acceleration_ramp_ms: 2_500,
            cursor_horizontal_max_step: 10,
            cursor_vertical_max_step: 4,
        },
        launcher: LauncherConfig {
            pinned_dirs: vec!["C:/Projects".to_string()],
            entries: vec![LauncherEntryRecord {
                id: "notepad".to_string(),
                path: "C:/Windows/notepad.exe".to_string(),
                executable_kind: Some(LauncherExecutableKind::NativeBinary),
                fingerprint: Some(LauncherFingerprint {
                    sha256: "abc123".to_string(),
                    byte_len: 42,
                    modified_at_epoch_ms: Some(100),
                }),
                added_by_user_id: "admin".to_string(),
                added_at_epoch_ms: 99,
            }],
            ..LauncherConfig::default()
        },
        security: SecurityConfig {
            allow_release_debug: true,
        },
    };
    manager
        .save_config(&config)
        .expect("config should save atomically");
    let mut expected_config = config.clone();
    expected_config.language = "en-US".to_string();
    assert_eq!(
        manager.load_config().expect("config should reload"),
        expected_config
    );
    let config_contents =
        fs::read_to_string(&manager.layout().config_path).expect("config should be readable");
    assert!(config_contents.contains("language = \"en-US\""));
    assert!(config_contents.contains("timezone = \"Asia/Shanghai\""));
    assert!(config_contents.contains("[appearance]"));
    assert!(config_contents.contains("border_shape = \"square\""));
    assert!(config_contents.contains("size_format = \"bytes\""));
    assert!(config_contents.contains("date_zone = \"utc\""));
    assert!(config_contents.contains("sort_field = \"modified\""));
    assert!(config_contents.contains("sort_direction = \"descending\""));

    let users = UsersDocument {
        schema_version: USERS_SCHEMA_VERSION,
        users: vec![UserRecord {
            id: "user-1".to_string(),
            username: "local-user".to_string(),
            display_name: "Local User".to_string(),
            role: "User".to_string(),
            password_hash: "$argon2id$placeholder".to_string(),
            password_hint: Some("project password".to_string()),
            enabled: true,
            failed_login_attempts: 0,
            locked_until_epoch_ms: None,
            created_at_epoch_ms: 1,
            updated_at_epoch_ms: 1,
            last_login_at_epoch_ms: None,
        }],
    };
    manager.save_users(&users).expect("users should save");
    assert_eq!(manager.load_users().expect("users should reload"), users);

    let mut values = BTreeMap::new();
    values.insert("active_view".to_string(), "home".to_string());
    let state = StateDocument {
        schema_version: SCHEMA_VERSION,
        values,
    };
    manager.save_state(&state).expect("state should save");
    assert_eq!(manager.load_state().expect("state should reload"), state);

    let recent = RecentFilesDocument {
        schema_version: SCHEMA_VERSION,
        files: vec!["C:/Projects/readme.md".to_string()],
    };
    manager
        .save_recent_files(&recent)
        .expect("recent files should save");
    assert_eq!(
        manager
            .load_recent_files()
            .expect("recent files should reload"),
        recent
    );

    let sessions = SessionsDocument {
        schema_version: SCHEMA_VERSION,
        sessions: vec!["default".to_string()],
    };
    manager
        .save_sessions(&sessions)
        .expect("sessions should save");
    assert_eq!(
        manager.load_sessions().expect("sessions should reload"),
        sessions
    );

    let mut profiles = BTreeMap::new();
    profiles.insert(
        "user-1".to_string(),
        ClockProfile {
            next_id: 3,
            entries: vec![
                ClockEntryRecord::DailyAlarm {
                    id: 1,
                    hour: 7,
                    minute: 30,
                    second: 15,
                    strong: true,
                    snooze_deadline_epoch_ms: Some(1_725_000_000_000),
                },
                ClockEntryRecord::Countdown {
                    id: 2,
                    deadline_epoch_ms: 1_725_000_300_000,
                    strong: false,
                },
            ],
        },
    );
    let clock = ClockDocument {
        schema_version: SCHEMA_VERSION,
        profiles,
    };
    manager.save_clock(&clock).expect("clock should save");
    assert_eq!(manager.load_clock().expect("clock should reload"), clock);
    let clock_contents =
        fs::read_to_string(&manager.layout().clock_path).expect("clock should be readable");
    assert!(clock_contents.contains("\"kind\": \"daily_alarm\""));
    assert!(clock_contents.contains("\"kind\": \"countdown\""));

    let trash = TrashDocument {
        schema_version: SCHEMA_VERSION,
        records: vec![TrashRecord {
            original_path: PathBuf::from("C:/Projects/readme.md"),
            trash_path: PathBuf::from("C:/TundraUX3/trash/readme.md"),
            actor: "local-user".to_string(),
            timestamp_epoch_ms: 42,
        }],
    };
    manager.save_trash(&trash).expect("trash should save");
    assert_eq!(manager.load_trash().expect("trash should reload"), trash);

    cleanup(&base);
}

#[test]
fn old_config_without_language_or_timezone_loads_with_defaults() {
    let base = unique_temp_root("old-config-defaults");
    let paths = app_paths(&base);
    let layout = StorageLayout::from_app_paths(&paths);
    fs::create_dir_all(layout.config_path.parent().expect("config parent"))
        .expect("config parent should be writable");
    fs::write(
        &layout.config_path,
        "schema_version = 1\ntheme = \"light\"\n\n[shortcuts]\n\n[explorer]\nshow_hidden = true\n\n[launcher]\npinned_apps = []\npinned_dirs = []\n\n[security]\nallow_release_debug = false\n",
    )
    .expect("old config fixture");

    let opened = StorageManager::open(paths).expect("old config should load");
    let config = opened.manager.load_config().expect("config should load");

    assert_eq!(config.theme, "light");
    assert_eq!(config.language, "en-US");
    assert_eq!(config.timezone, "UTC");
    assert_eq!(config.appearance.border_shape, BorderShape::Rounded);
    assert_eq!(config.editor, EditorConfig::default());
    assert_eq!(
        config.explorer,
        ExplorerConfig {
            show_hidden: true,
            ..ExplorerConfig::default()
        }
    );
    assert!(opened.report.recovered_files.is_empty());
    assert!(opened.report.warnings.is_empty());

    cleanup(&base);
}

#[test]
fn legacy_pinned_apps_migrate_to_unapproved_launcher_entries() {
    let base = unique_temp_root("legacy-launcher-pins");
    let paths = app_paths(&base);
    let layout = StorageLayout::from_app_paths(&paths);
    fs::create_dir_all(layout.config_path.parent().expect("config parent"))
        .expect("config parent should be writable");
    fs::write(
        &layout.config_path,
        "schema_version = 1\ntheme = \"dark\"\n\n[launcher]\npinned_apps = [\"C:/Apps/Legacy.exe\", \"C:/Apps/Legacy.exe\"]\npinned_dirs = [\"C:/Projects\"]\n",
    )
    .expect("legacy config fixture");

    let opened = StorageManager::open(paths).expect("legacy config should migrate");
    let config = opened
        .manager
        .load_config()
        .expect("migrated config should load");

    assert_eq!(config.launcher.entries.len(), 1);
    let entry = &config.launcher.entries[0];
    assert_eq!(entry.path, "C:/Apps/Legacy.exe");
    assert_eq!(entry.executable_kind, None);
    assert_eq!(entry.fingerprint, None);
    assert_eq!(entry.added_by_user_id, "legacy");
    assert!(entry.id.starts_with("legacy-"));
    assert!(config.launcher.pinned_apps.is_empty());
    assert_eq!(config.launcher.pinned_dirs, vec!["C:/Projects"]);
    assert!(opened.report.migrated_files.contains(&layout.config_path));

    let contents = fs::read_to_string(&layout.config_path).expect("migrated config is readable");
    assert!(contents.contains("[[launcher.entries]]"));
    assert!(!contents.contains("pinned_apps"));
    assert!(contents.contains("pinned_dirs = [\"C:/Projects\"]"));

    cleanup(&base);
}

#[test]
fn existing_non_english_config_is_migrated_to_english_on_open() {
    let base = unique_temp_root("non-english-config");
    let paths = app_paths(&base);
    let layout = StorageLayout::from_app_paths(&paths);
    fs::create_dir_all(layout.config_path.parent().expect("config parent"))
        .expect("config parent should be writable");
    fs::write(
        &layout.config_path,
        "schema_version = 1\ntheme = \"dark\"\nlanguage = \"zh-Hans\"\ntimezone = \"UTC\"\n",
    )
    .expect("legacy language fixture");

    let opened = StorageManager::open(paths).expect("config migration should succeed");

    assert_eq!(
        opened.manager.load_config().expect("config").language,
        "en-US"
    );
    assert!(opened.report.migrated_files.contains(&layout.config_path));
    let contents = fs::read_to_string(&layout.config_path).expect("migrated config is readable");
    assert!(contents.contains("language = \"en-US\""));
    assert!(!contents.contains("zh-Hans"));

    cleanup(&base);
}

#[test]
fn old_users_without_password_hint_load_with_none() {
    let base = unique_temp_root("old-users-no-hint");
    let paths = app_paths(&base);
    let layout = StorageLayout::from_app_paths(&paths);
    fs::create_dir_all(&layout.data_path).expect("data path should be writable");
    fs::write(
        &layout.users_path,
        "{\n  \"schema_version\": 2,\n  \"users\": [\n    {\n      \"id\": \"user-1\",\n      \"username\": \"AdminUser\",\n      \"display_name\": \"Admin User\",\n      \"role\": \"Admin\",\n      \"password_hash\": \"$argon2id$placeholder\",\n      \"enabled\": true,\n      \"failed_login_attempts\": 0,\n      \"locked_until_epoch_ms\": null,\n      \"created_at_epoch_ms\": 1,\n      \"updated_at_epoch_ms\": 1,\n      \"last_login_at_epoch_ms\": null\n    }\n  ]\n}\n",
    )
    .expect("old users fixture");

    let opened = StorageManager::open(paths).expect("old users should load");
    let users = opened.manager.load_users().expect("users should load");

    assert_eq!(users.users.len(), 1);
    assert_eq!(users.users[0].password_hint, None);
    assert!(opened.report.recovered_files.is_empty());
    assert!(opened.report.warnings.is_empty());

    cleanup(&base);
}

#[test]
fn clock_fields_missing_from_hand_written_files_use_defaults() {
    let base = unique_temp_root("clock-field-defaults");
    let paths = app_paths(&base);
    let layout = StorageLayout::from_app_paths(&paths);
    fs::create_dir_all(&layout.data_path).expect("data path should be writable");
    fs::write(
        &layout.clock_path,
        "{\n  \"schema_version\": 1,\n  \"profiles\": {\n    \"user-1\": {\n      \"entries\": [\n        { \"kind\": \"daily_alarm\" },\n        { \"kind\": \"countdown\" }\n      ]\n    }\n  }\n}\n",
    )
    .expect("hand-written clock fixture");

    let opened = StorageManager::open(paths).expect("clock defaults should load");
    let clock = opened.manager.load_clock().expect("clock should load");
    let profile = clock.profiles.get("user-1").expect("profile should exist");

    assert_eq!(profile.next_id, 1);
    assert_eq!(
        profile.entries,
        vec![
            ClockEntryRecord::DailyAlarm {
                id: 0,
                hour: 0,
                minute: 0,
                second: 0,
                strong: false,
                snooze_deadline_epoch_ms: None,
            },
            ClockEntryRecord::Countdown {
                id: 0,
                deadline_epoch_ms: 0,
                strong: false,
            },
        ]
    );
    assert!(opened.report.recovered_files.is_empty());

    cleanup(&base);
}

#[test]
fn corrupt_toml_and_json_are_backed_up_and_replaced_with_defaults() {
    let base = unique_temp_root("corrupt-recovery");
    let paths = app_paths(&base);
    let layout = StorageLayout::from_app_paths(&paths);
    fs::create_dir_all(layout.config_path.parent().expect("config parent"))
        .expect("config parent should be writable");
    fs::create_dir_all(&layout.data_path).expect("data path should be writable");
    fs::create_dir_all(&layout.trash_path).expect("trash path should be writable");
    fs::write(&layout.config_path, b"schema_version =\n").expect("corrupt TOML fixture");
    fs::write(&layout.users_path, b"{").expect("corrupt JSON fixture");
    fs::write(&layout.clock_path, b"{").expect("corrupt clock fixture");
    fs::write(&layout.trash_manifest_path, b"{").expect("corrupt trash fixture");

    let opened = StorageManager::open(paths).expect("storage should recover corrupt documents");

    assert_eq!(
        opened.manager.load_config().expect("default config").theme,
        "dark"
    );
    assert!(
        opened
            .manager
            .load_users()
            .expect("default users")
            .users
            .is_empty()
    );
    assert!(
        opened
            .manager
            .load_clock()
            .expect("default clock")
            .profiles
            .is_empty()
    );
    assert!(
        opened
            .manager
            .load_trash()
            .expect("default trash")
            .records
            .is_empty()
    );
    assert_eq!(opened.report.recovered_files.len(), 4);
    assert_eq!(opened.report.warnings.len(), 4);
    for recovered in &opened.report.recovered_files {
        assert!(recovered.recovered_path.is_file());
        assert!(
            recovered
                .recovered_path
                .file_name()
                .expect("backup file name")
                .to_string_lossy()
                .contains(".corrupt.")
        );
    }

    cleanup(&base);
}

#[test]
fn future_trash_schema_errors_without_modifying_file() {
    let base = unique_temp_root("future-trash");
    let paths = app_paths(&base);
    let layout = StorageLayout::from_app_paths(&paths);
    fs::create_dir_all(&layout.trash_path).expect("trash path should be writable");
    let original = "{\n  \"schema_version\": 2,\n  \"records\": []\n}\n";
    fs::write(&layout.trash_manifest_path, original).expect("future trash fixture");

    let error = StorageManager::open(paths).expect_err("future trash should fail");

    assert!(matches!(
        error,
        StorageError::UnsupportedSchema {
            document: "trash",
            found: 2,
            supported: SCHEMA_VERSION,
            ..
        }
    ));
    assert_eq!(
        fs::read_to_string(&layout.trash_manifest_path)
            .expect("future trash should remain readable"),
        original
    );
    assert_no_corrupt_backups(&layout.trash_path);

    cleanup(&base);
}

#[test]
fn future_clock_schema_errors_without_modifying_file() {
    let base = unique_temp_root("future-clock");
    let paths = app_paths(&base);
    let layout = StorageLayout::from_app_paths(&paths);
    fs::create_dir_all(&layout.data_path).expect("data path should be writable");
    let original = "{\n  \"schema_version\": 2,\n  \"profiles\": {}\n}\n";
    fs::write(&layout.clock_path, original).expect("future clock fixture");

    let error = StorageManager::open(paths).expect_err("future clock should fail");

    assert!(matches!(
        error,
        StorageError::UnsupportedSchema {
            document: "clock",
            found: 2,
            supported: SCHEMA_VERSION,
            ..
        }
    ));
    assert_eq!(
        fs::read_to_string(&layout.clock_path).expect("future clock should remain readable"),
        original
    );
    assert_no_corrupt_backups(&layout.data_path);

    cleanup(&base);
}

#[test]
fn future_toml_schema_errors_without_modifying_file() {
    let base = unique_temp_root("future-config");
    let paths = app_paths(&base);
    let layout = StorageLayout::from_app_paths(&paths);
    fs::create_dir_all(layout.config_path.parent().expect("config parent"))
        .expect("config parent should be writable");
    let original = "schema_version = 2\ntheme = \"future\"\n";
    fs::write(&layout.config_path, original).expect("future TOML fixture");

    let error = StorageManager::open(paths).expect_err("future config should fail");

    assert!(matches!(
        error,
        StorageError::UnsupportedSchema {
            document: "config",
            found: 2,
            supported: SCHEMA_VERSION,
            ..
        }
    ));
    assert_eq!(
        fs::read_to_string(&layout.config_path).expect("future config should remain readable"),
        original
    );
    assert_no_corrupt_backups(layout.config_path.parent().expect("config parent"));

    cleanup(&base);
}

#[test]
fn future_json_schema_errors_without_modifying_file() {
    let base = unique_temp_root("future-json");
    let paths = app_paths(&base);
    let layout = StorageLayout::from_app_paths(&paths);
    fs::create_dir_all(layout.config_path.parent().expect("config parent"))
        .expect("config parent should be writable");
    fs::create_dir_all(&layout.data_path).expect("data path should be writable");
    fs::write(
        &layout.config_path,
        "schema_version = 1\ntheme = \"dark\"\n\n[shortcuts]\n\n[explorer]\nshow_hidden = false\n\n[launcher]\npinned_apps = []\npinned_dirs = []\n",
    )
    .expect("current TOML fixture");
    let original = "{\n  \"schema_version\": 3,\n  \"users\": []\n}\n";
    fs::write(&layout.users_path, original).expect("future JSON fixture");

    let error = StorageManager::open(paths).expect_err("future JSON should fail");

    assert!(matches!(
        error,
        StorageError::UnsupportedSchema {
            document: "users",
            found: 3,
            supported: USERS_SCHEMA_VERSION,
            ..
        }
    ));
    assert_eq!(
        fs::read_to_string(&layout.users_path).expect("future JSON should remain readable"),
        original
    );
    assert_no_corrupt_backups(&layout.data_path);

    cleanup(&base);
}

#[test]
fn legacy_v1_users_are_migrated_to_disabled_v2_records() {
    let base = unique_temp_root("legacy-users");
    let paths = app_paths(&base);
    let layout = StorageLayout::from_app_paths(&paths);
    fs::create_dir_all(&layout.data_path).expect("data path should be writable");
    fs::write(
        &layout.legacy_users_path,
        "{\n  \"schema_version\": 1,\n  \"users\": [\"Alice\", \"bob\"]\n}\n",
    )
    .expect("legacy users fixture");

    let opened = StorageManager::open(paths).expect("storage should migrate legacy users");
    let users = opened.manager.load_users().expect("migrated users");

    assert_eq!(users.schema_version, USERS_SCHEMA_VERSION);
    assert_eq!(users.users.len(), 2);
    assert_eq!(users.users[0].username, "Alice");
    assert_eq!(users.users[0].role, "User");
    assert!(!users.users[0].enabled);
    assert!(users.users[0].password_hash.is_empty());
    assert_eq!(users.users[0].password_hint, None);
    assert_eq!(opened.report.migrated_files, vec![layout.users_path]);

    cleanup(&base);
}

#[test]
fn open_from_platform_uses_mock_platform_app_paths() {
    let base = unique_temp_root("mock-platform");
    let platform = mock_platform(&base);
    let expected_layout =
        StorageLayout::from_app_paths(&platform.app_paths().expect("mock paths should resolve"));

    let opened =
        StorageManager::open_from_platform(&platform).expect("storage should open from mock paths");

    assert_eq!(opened.manager.layout(), &expected_layout);
    assert!(expected_layout.config_path.is_file());
    assert!(expected_layout.users_path.is_file());

    cleanup(&base);
}

#[test]
fn windows_and_macos_builders_can_be_injected_directly() {
    let windows_base = unique_temp_root("windows-builder");
    let windows_paths = build_windows_app_paths(
        windows_base.join("Roaming"),
        windows_base.join("Local"),
        windows_base.join("Temp"),
    )
    .expect("injected Windows paths should resolve");
    let windows_layout = StorageLayout::from_app_paths(&windows_paths);
    StorageManager::open(windows_paths).expect("storage should open with Windows paths");
    assert_eq!(
        windows_layout.config_path,
        windows_base
            .join("Roaming")
            .join("TundraUX3")
            .join("config.toml")
    );
    assert!(windows_layout.config_path.is_file());
    assert!(windows_layout.users_path.is_file());

    let macos_base = unique_temp_root("macos-builder");
    let macos_paths = build_macos_app_paths(
        macos_base.join("Users").join("tundra"),
        macos_base.join("Tmp"),
    )
    .expect("injected macOS paths should resolve");
    let macos_layout = StorageLayout::from_app_paths(&macos_paths);
    StorageManager::open(macos_paths).expect("storage should open with macOS paths");
    assert_eq!(
        macos_layout.config_path,
        macos_base
            .join("Users")
            .join("tundra")
            .join("Library")
            .join("Application Support")
            .join("TundraUX3")
            .join("config.toml")
    );
    assert!(macos_layout.config_path.is_file());
    assert!(macos_layout.users_path.is_file());

    cleanup(&windows_base);
    cleanup(&macos_base);
}

fn app_paths(base: &Path) -> AppPaths {
    AppPaths::from_parts(
        base.join("config").join("config.toml"),
        base.join("state"),
        base.join("cache"),
        base.join("logs"),
        base.join("temp"),
    )
    .expect("fixture paths should be absolute")
}

fn mock_platform(base: &Path) -> MockPlatform {
    let user_dirs = UserDirs::new(
        base.join("Desktop"),
        base.join("Documents"),
        base.join("Downloads"),
        base.join("Pictures"),
        base.join("Videos"),
        base.join("Music"),
        base.join("Roaming"),
    )
    .expect("fixture user directories should resolve");
    let app_paths =
        build_windows_app_paths(base.join("Roaming"), base.join("Local"), base.join("Temp"))
            .expect("fixture app paths should resolve");

    MockPlatform::new(user_dirs, app_paths)
}

fn unique_temp_root(case: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time should be after unix epoch")
        .as_nanos();

    std::env::temp_dir().join(format!(
        "tundra-storage-test-{}-{}-{case}",
        std::process::id(),
        nanos
    ))
}

fn sorted_paths(mut paths: Vec<PathBuf>) -> Vec<PathBuf> {
    paths.sort();
    paths
}

fn assert_no_corrupt_backups(directory: &Path) {
    if !directory.exists() {
        return;
    }

    for entry in fs::read_dir(directory).expect("directory should be readable") {
        let entry = entry.expect("directory entry should be readable");
        assert!(
            !entry.file_name().to_string_lossy().contains(".corrupt."),
            "unexpected corrupt backup at {}",
            entry.path().display()
        );
    }
}

fn cleanup(path: &Path) {
    cleanup_temp_path(path).expect("test directory should be removable");
}
