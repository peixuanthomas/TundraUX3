use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use platform::mock::{MockCall, MockPlatform, UnsupportedPlatform};
use platform::{
    PlatformCapabilities, PlatformError, PlatformKind, StartupPermissionStatus, UserDirs,
    build_macos_app_paths, build_windows_app_paths,
};
use shell::{ShellStartupError, prepare_shell_startup};
use storage::{
    BorderColor as StorageBorderColor, BorderShape as StorageBorderShape, StorageError,
    StorageManager,
};
use ui::BorderShape as UiBorderShape;

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

    let startup = prepare_shell_startup(&platform).expect("startup state");

    assert_eq!(startup.platform_kind, PlatformKind::Windows);
    assert_eq!(
        startup.platform_capabilities,
        PlatformCapabilities::native_supported()
    );
    assert_eq!(startup.storage_report.app_paths.as_ref(), Some(&app_paths));
    assert!(startup.storage_report.warnings.is_empty());
    assert_eq!(startup.app_config.home_mode, None);
    assert_eq!(startup.app_config.border_shape, UiBorderShape::Rounded);
    assert_eq!(startup.app_config.accent_color, ratatui::style::Color::Cyan);
    assert_eq!(startup.restored_session, None);
}

#[test]
fn prepare_shell_startup_uses_default_theme_before_any_user_logs_in() {
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
    config.appearance.border_color = StorageBorderColor::Rgb(0x38, 0xBD, 0xF8);
    config.appearance.accent_color = StorageBorderColor::LightMagenta;
    manager.save_config(&config).expect("config should save");
    let platform = MockPlatform::new(user_dirs(base), app_paths)
        .with_kind(PlatformKind::Windows)
        .with_capabilities(PlatformCapabilities::native_supported());

    let startup = prepare_shell_startup(&platform).expect("startup state");

    assert_eq!(startup.app_config.border_shape, UiBorderShape::Rounded);
    assert_eq!(
        startup.app_config.border_color,
        ratatui::style::Color::White
    );
    assert_eq!(startup.app_config.accent_color, ratatui::style::Color::Cyan);
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

    let startup = prepare_shell_startup(&platform).expect("startup state");

    assert_eq!(startup.platform_kind, PlatformKind::Macos);
    assert_eq!(startup.platform_capabilities, capabilities);
    assert_eq!(startup.storage_report.app_paths.as_ref(), Some(&app_paths));
    assert!(startup.storage_report.warnings.is_empty());
}

#[test]
fn prepare_shell_startup_requests_missing_platform_permission_before_storage_opens() {
    let fixture = FixtureRoot::new("missing-startup-permission");
    let base = fixture.path();
    let app_paths =
        build_macos_app_paths(base.join("home"), base.join("temp")).expect("valid macOS paths");
    let platform =
        MockPlatform::new(user_dirs(base), app_paths.clone()).with_kind(PlatformKind::Macos);
    platform.set_startup_permission_status(Ok(StartupPermissionStatus::action_required(
        "Full Disk Access",
        "enable it and restart",
    )));

    let error = prepare_shell_startup(&platform)
        .expect_err("startup must stop while required permission is missing");

    assert!(matches!(
        error,
        ShellStartupError::Platform(PlatformError::Native {
            operation: "startup permission check",
            ..
        })
    ));
    assert_eq!(
        platform.calls(),
        vec![
            MockCall::StartupPermissionStatus,
            MockCall::RequestStartupPermissions
        ]
    );
    assert!(
        !app_paths.data_path().exists(),
        "storage must not initialize before startup permission is granted"
    );
}

#[test]
fn prepare_shell_startup_returns_platform_error_when_app_paths_fail() {
    let error = prepare_shell_startup(&UnsupportedPlatform)
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
