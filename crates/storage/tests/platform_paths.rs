use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use platform::mock::MockPlatform;
use platform::{AppPaths, PlatformKind, UserDirs, build_macos_app_paths, build_windows_app_paths};
use storage::{StorageLayout, StorageManager};

#[test]
fn windows_style_storage_paths_open_from_mock_platform() {
    let fixture = FixtureRoot::new("windows-storage-paths");
    let app_paths = build_windows_app_paths(
        fixture.path().join("Roaming"),
        fixture.path().join("Local"),
        fixture.path().join("Temp"),
    )
    .expect("absolute Windows-style fixture roots should resolve");
    let platform = MockPlatform::new(user_dirs_for(fixture.path()), app_paths.clone())
        .with_kind(PlatformKind::Windows);

    let storage =
        StorageManager::open_from_platform(&platform).expect("storage should open from platform");

    assert_storage_layout(storage.manager.layout(), &app_paths);
}

#[test]
fn macos_style_storage_paths_open_from_mock_platform() {
    let fixture = FixtureRoot::new("macos-storage-paths");
    let app_paths = build_macos_app_paths(
        fixture.path().join("Users").join("tundra"),
        fixture.path().join("Tmp"),
    )
    .expect("absolute macOS-style fixture roots should resolve");
    let platform = MockPlatform::new(user_dirs_for(fixture.path()), app_paths.clone())
        .with_kind(PlatformKind::Macos);

    let storage =
        StorageManager::open_from_platform(&platform).expect("storage should open from platform");

    assert_storage_layout(storage.manager.layout(), &app_paths);
}

fn assert_storage_layout(storage: &StorageLayout, app_paths: &AppPaths) {
    assert_eq!(storage.config_path, app_paths.config_path());
    assert_eq!(storage.data_path, app_paths.data_path());
    assert_eq!(storage.cache_path, app_paths.cache_path());
    assert_eq!(storage.logs_path, app_paths.logs_path());
    assert_eq!(storage.temp_path, app_paths.temp_path());

    assert!(storage.config_path.is_file());
    assert!(storage.data_path.is_dir());
    assert!(storage.cache_path.is_dir());
    assert!(storage.logs_path.is_dir());
    assert!(storage.temp_path.is_dir());

    assert_state_file(&storage.state_path, &storage.data_path, "state.v1.json");
    assert_state_file(
        &storage.recent_files_path,
        &storage.data_path,
        "recent-files.v1.json",
    );
    assert_state_file(
        &storage.sessions_path,
        &storage.data_path,
        "sessions.v1.json",
    );
    assert_state_file(&storage.clock_path, &storage.data_path, "clock.v1.json");
    assert_state_file(&storage.users_path, &storage.data_path, "users.v2.json");
    assert_eq!(storage.trash_path, storage.data_path.join("trash"));
    assert!(storage.trash_path.is_dir());
    assert_state_file(
        &storage.trash_manifest_path,
        &storage.trash_path,
        "trash.v1.json",
    );
}

fn assert_state_file(path: &Path, data_path: &Path, file_name: &str) {
    assert_eq!(path, data_path.join(file_name).as_path());
    assert_eq!(path.parent(), Some(data_path));
    assert!(path.is_file());
}

fn user_dirs_for(base: &Path) -> UserDirs {
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
        let root = std::env::temp_dir();
        assert!(root.is_absolute(), "temp root must be absolute");

        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should be after unix epoch")
            .as_nanos();
        let path = root.join(format!(
            "tundra-storage-test-{}-{nanos}-{case}",
            std::process::id()
        ));

        assert!(path.starts_with(&root), "fixture must live under temp root");
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
