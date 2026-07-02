use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use tundra_platform::mock::MockPlatform;
use tundra_platform::{
    AppPaths, Platform, UserDirs, build_macos_app_paths, build_windows_app_paths, cleanup_temp_path,
};
use tundra_storage::{
    ExplorerConfig, LauncherConfig, RecentFilesDocument, SCHEMA_VERSION, SecurityConfig,
    SessionsDocument, StateDocument, StorageConfig, StorageError, StorageLayout, StorageManager,
    TrashDocument, TrashRecord, USERS_SCHEMA_VERSION, UserRecord, UsersDocument,
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
    assert!(layout.trash_path.is_dir());
    assert!(layout.trash_manifest_path.is_file());
    assert!(!layout.audit_log_path.exists());
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
        shortcuts,
        explorer: ExplorerConfig { show_hidden: true },
        launcher: LauncherConfig {
            pinned_apps: vec!["notepad.exe".to_string()],
            pinned_dirs: vec!["C:/Projects".to_string()],
        },
        security: SecurityConfig {
            allow_release_debug: true,
        },
    };
    manager
        .save_config(&config)
        .expect("config should save atomically");
    assert_eq!(manager.load_config().expect("config should reload"), config);

    let users = UsersDocument {
        schema_version: USERS_SCHEMA_VERSION,
        users: vec![UserRecord {
            id: "user-1".to_string(),
            username: "local-user".to_string(),
            display_name: "Local User".to_string(),
            role: "User".to_string(),
            password_hash: "$argon2id$placeholder".to_string(),
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
            .load_trash()
            .expect("default trash")
            .records
            .is_empty()
    );
    assert_eq!(opened.report.recovered_files.len(), 3);
    assert_eq!(opened.report.warnings.len(), 3);
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
    assert_eq!(opened.report.migrated_files, vec![layout.users_path]);

    cleanup(&base);
}

#[test]
fn audit_lines_append_and_read_round_trip() {
    let base = unique_temp_root("audit-lines");
    let opened = StorageManager::open(app_paths(&base)).expect("storage should open");
    let manager = opened.manager;

    manager
        .append_audit_line("{\"sequence\":1}")
        .expect("first line should append");
    manager
        .append_audit_line("{\"sequence\":2}")
        .expect("second line should append");

    assert_eq!(
        manager.read_audit_lines().expect("audit should read"),
        vec![
            "{\"sequence\":1}".to_string(),
            "{\"sequence\":2}".to_string()
        ]
    );

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
