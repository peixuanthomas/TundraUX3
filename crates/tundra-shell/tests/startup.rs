use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use tundra_platform::mock::{MockPlatform, UnsupportedPlatform};
use tundra_platform::{
    PlatformCapabilities, PlatformKind, UserDirs, build_macos_app_paths, build_windows_app_paths,
};
use tundra_shell::{ShellLaunchConfig, ShellStartupError, prepare_shell_startup};
use tundra_storage::{BorderShape as StorageBorderShape, StorageError, StorageManager};
use tundra_ui::BorderShape as UiBorderShape;

#[test]
fn prepare_shell_startup_uses_windows_mock_app_paths() {
    let fixture = FixtureRoot::new("windows");
    let base = fixture.path();
    let app_paths =
        build_windows_app_paths(base.join("roaming"), base.join("local"), base.join("temp"))
            .expect("valid Windows app paths");
    let platform = MockPlatform::new(user_dirs(base), app_paths.clone())
        .with_kind(PlatformKind::Windows)
        .with_capabilities(PlatformCapabilities::native_supported());

    let startup =
        prepare_shell_startup(&platform, ShellLaunchConfig::default()).expect("startup state");

    assert_eq!(startup.platform_kind, PlatformKind::Windows);
    assert_eq!(
        startup.platform_capabilities,
        PlatformCapabilities::native_supported()
    );
    assert_eq!(startup.storage_report.app_paths.as_ref(), Some(&app_paths));
    assert!(startup.storage_report.warnings.is_empty());
    assert_eq!(startup.app_config.home_mode, None);
    assert_eq!(startup.app_config.border_shape, UiBorderShape::Rounded);
    assert_eq!(startup.restored_session, None);
}

#[test]
fn prepare_shell_startup_maps_persisted_square_border_shape() {
    let fixture = FixtureRoot::new("square-border");
    let base = fixture.path();
    let app_paths =
        build_windows_app_paths(base.join("roaming"), base.join("local"), base.join("temp"))
            .expect("valid Windows app paths");
    let manager = StorageManager::open(app_paths.clone())
        .expect("storage should initialize")
        .manager;
    let mut config = manager.load_config().expect("config should load");
    config.appearance.border_shape = StorageBorderShape::Square;
    manager.save_config(&config).expect("config should save");
    let platform = MockPlatform::new(user_dirs(base), app_paths)
        .with_kind(PlatformKind::Windows)
        .with_capabilities(PlatformCapabilities::native_supported());

    let startup =
        prepare_shell_startup(&platform, ShellLaunchConfig::default()).expect("startup state");

    assert_eq!(startup.app_config.border_shape, UiBorderShape::Square);
}

#[test]
fn prepare_shell_startup_uses_macos_mock_app_paths() {
    let fixture = FixtureRoot::new("macos");
    let base = fixture.path();
    let app_paths =
        build_macos_app_paths(base.join("home"), base.join("temp")).expect("valid macOS paths");
    let capabilities = PlatformCapabilities::unsupported();
    let platform = MockPlatform::new(user_dirs(base), app_paths.clone())
        .with_kind(PlatformKind::Macos)
        .with_capabilities(capabilities.clone());

    let startup =
        prepare_shell_startup(&platform, ShellLaunchConfig::default()).expect("startup state");

    assert_eq!(startup.platform_kind, PlatformKind::Macos);
    assert_eq!(startup.platform_capabilities, capabilities);
    assert_eq!(startup.storage_report.app_paths.as_ref(), Some(&app_paths));
    assert!(startup.storage_report.warnings.is_empty());
}

#[test]
fn prepare_shell_startup_returns_platform_error_when_app_paths_fail() {
    let error = prepare_shell_startup(&UnsupportedPlatform, ShellLaunchConfig::default())
        .expect_err("unsupported platform cannot resolve app paths");

    assert!(matches!(
        error,
        ShellStartupError::Storage(StorageError::Platform { .. })
    ));
}

fn user_dirs(base: &Path) -> UserDirs {
    UserDirs::new(
        base.join("desktop"),
        base.join("documents"),
        base.join("downloads"),
        base.join("pictures"),
        base.join("videos"),
        base.join("music"),
        base.join("app-data"),
    )
    .expect("absolute user dirs")
}

struct FixtureRoot {
    path: PathBuf,
}

impl FixtureRoot {
    fn new(name: &str) -> Self {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should be after unix epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "tundra-shell-startup-{}-{nanos}-{name}",
            std::process::id()
        ));
        assert!(path.is_absolute());
        let _ = fs::remove_dir_all(&path);
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
