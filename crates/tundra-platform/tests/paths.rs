use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use tundra_platform::mock::{MockCall, MockPlatform, UnsupportedPlatform};
use tundra_platform::{
    AppPaths, CapabilityStatus, CheckStatus, FileAttributes, Platform, PlatformError, PlatformKind,
    ProcessSpec, StartupPermissionStatus, UserDirs, build_binary_dir_app_paths,
    build_macos_app_paths, build_windows_app_paths, check_directory_read_write,
    classify_windows_build, cleanup_temp_path, create_temp_dir, create_temp_file,
    default_file_attributes, is_windows_terminal_session, run_doctor_with,
    terminal_environment_check_with, validate_process_spec,
};

#[test]
fn windows_app_paths_follow_roaming_local_and_temp_roots() {
    let base = unique_temp_root("windows-native-paths");
    let roaming = base.join("Roaming");
    let local = base.join("Local");
    let temp = base.join("Temp");

    let paths = build_windows_app_paths(&roaming, &local, &temp)
        .expect("absolute Windows roots should resolve");

    assert_eq!(
        paths.config_path(),
        roaming.join("TundraUX3").join("config.toml").as_path()
    );
    assert_eq!(
        paths.data_path(),
        local.join("TundraUX3").join("state").as_path()
    );
    assert_eq!(
        paths.cache_path(),
        local.join("TundraUX3").join("cache").as_path()
    );
    assert_eq!(
        paths.logs_path(),
        local.join("TundraUX3").join("logs").as_path()
    );
    assert_eq!(paths.temp_path(), temp.join("TundraUX3").as_path());
}

#[test]
fn macos_app_paths_follow_library_and_temp_roots() {
    let base = unique_temp_root("macos-native-paths");
    let home = base.join("Users").join("tundra");
    let temp = base.join("Tmp");

    let paths = build_macos_app_paths(&home, &temp).expect("absolute macOS roots should resolve");
    let app_support = home
        .join("Library")
        .join("Application Support")
        .join("TundraUX3");

    assert_eq!(
        paths.config_path(),
        app_support.join("config.toml").as_path()
    );
    assert_eq!(paths.data_path(), app_support.join("state").as_path());
    assert_eq!(
        paths.cache_path(),
        home.join("Library")
            .join("Caches")
            .join("TundraUX3")
            .as_path()
    );
    assert_eq!(
        paths.logs_path(),
        home.join("Library")
            .join("Logs")
            .join("TundraUX3")
            .as_path()
    );
    assert_eq!(paths.temp_path(), temp.join("TundraUX3").as_path());
}

#[test]
fn native_path_builders_reject_relative_roots() {
    let absolute = unique_temp_root("absolute-root");

    let windows_error = build_windows_app_paths("relative-roaming", &absolute, &absolute)
        .expect_err("relative roaming app data should fail");
    assert_relative_error_mentions(windows_error, "roaming app data");

    let macos_error = build_macos_app_paths("relative-home", &absolute)
        .expect_err("relative home directory should fail");
    assert_relative_error_mentions(macos_error, "home directory");
}

#[test]
fn binary_dir_app_paths_remain_available_for_portable_mode() {
    let base = unique_temp_root("binary-dir");
    let binary_dir = base.join("bin");

    let paths = build_binary_dir_app_paths(&binary_dir)
        .expect("absolute fake binary directory should resolve");
    let app_dir = binary_dir.join("TundraUX3");

    assert_eq!(paths.config_path(), app_dir.join("config.toml").as_path());
    assert_eq!(paths.data_path(), app_dir.join("state").as_path());
    assert_eq!(paths.cache_path(), app_dir.join("cache").as_path());
    assert_eq!(paths.logs_path(), app_dir.join("logs").as_path());
    assert_eq!(paths.temp_path(), app_dir.join("temp").as_path());
}

#[test]
fn app_paths_reject_relative_binary_directory() {
    let error = AppPaths::from_binary_dir(PathBuf::from("relative-bin"))
        .expect_err("relative binary directory should fail");

    let message = error.to_string().to_ascii_lowercase();
    assert!(message.contains("absolute"));
    assert!(
        message.contains("binary") || message.contains("executable"),
        "error should name the binary or executable directory: {message}"
    );
    assert!(!message.contains("appdata"));
    assert!(!message.contains("localappdata"));
}

#[cfg(windows)]
#[test]
fn app_path_templates_use_windows_native_placeholders() {
    assert_eq!(
        AppPaths::CONFIG_TEMPLATE,
        r"%APPDATA%\TundraUX3\config.toml"
    );
    assert_eq!(AppPaths::DATA_TEMPLATE, r"%LOCALAPPDATA%\TundraUX3\state");
    assert_eq!(AppPaths::CACHE_TEMPLATE, r"%LOCALAPPDATA%\TundraUX3\cache");
    assert_eq!(AppPaths::LOGS_TEMPLATE, r"%LOCALAPPDATA%\TundraUX3\logs");
    assert_eq!(AppPaths::TEMP_TEMPLATE, r"%TEMP%\TundraUX3");
}

#[cfg(target_os = "macos")]
#[test]
fn app_path_templates_use_macos_native_locations() {
    assert_eq!(
        AppPaths::CONFIG_TEMPLATE,
        "~/Library/Application Support/TundraUX3/config.toml"
    );
    assert_eq!(
        AppPaths::DATA_TEMPLATE,
        "~/Library/Application Support/TundraUX3/state"
    );
    assert_eq!(AppPaths::CACHE_TEMPLATE, "~/Library/Caches/TundraUX3");
    assert_eq!(AppPaths::LOGS_TEMPLATE, "~/Library/Logs/TundraUX3");
    assert_eq!(AppPaths::TEMP_TEMPLATE, "<temp-dir>/TundraUX3");
}

#[cfg(not(any(windows, target_os = "macos")))]
#[test]
fn app_path_templates_mark_unsupported_platforms() {
    assert_eq!(
        AppPaths::CONFIG_TEMPLATE,
        "<unsupported>/TundraUX3/config.toml"
    );
    assert_eq!(AppPaths::DATA_TEMPLATE, "<unsupported>/TundraUX3/state");
    assert_eq!(AppPaths::CACHE_TEMPLATE, "<unsupported>/TundraUX3/cache");
    assert_eq!(AppPaths::LOGS_TEMPLATE, "<unsupported>/TundraUX3/logs");
    assert_eq!(AppPaths::TEMP_TEMPLATE, "<unsupported>/TundraUX3/temp");
}

#[test]
fn mock_platform_drives_doctor_path_checks() {
    let base = unique_temp_root("mock-doctor");
    let platform = mock_platform(&base).with_kind(PlatformKind::Macos);

    let report = run_doctor_with(&platform).expect("mock platform doctor should run");

    assert_eq!(report.platform_kind, PlatformKind::Macos);
    assert_eq!(
        report.app_paths,
        build_windows_app_paths(base.join("Roaming"), base.join("Local"), base.join("Temp"))
            .expect("fixture app paths should resolve")
    );
    assert_eq!(
        report
            .path_checks
            .iter()
            .map(|check| check.label.as_str())
            .collect::<Vec<_>>(),
        vec![
            "Config parent",
            "Data path",
            "Cache path",
            "Logs path",
            "Temp path"
        ]
    );
    assert!(
        report
            .path_checks
            .iter()
            .all(|check| check.status == CheckStatus::Pass),
        "mock platform paths should all pass: {:?}",
        report.path_checks
    );
    assert!(
        report.environment_checks.iter().any(|check| {
            check.label == "Capability: temp" && check.status == CheckStatus::Pass
        }),
        "doctor should report the mock temp capability"
    );
    assert!(!base.exists());
}

#[test]
fn mock_platform_records_process_clipboard_and_open_calls() {
    let base = unique_temp_root("mock-calls");
    let platform = mock_platform(&base);
    let target = base.join("target.txt");
    let application = base.join("viewer.exe");
    let spec = ProcessSpec::new(base.join("program.exe"))
        .arg("--flag")
        .env("TUNDRA_TEST", "1")
        .current_dir(&base);

    platform
        .open_path(&target)
        .expect("mock open_path should pass");
    platform
        .open_with(&target, &application)
        .expect("mock open_with should pass");
    platform
        .open_uri("tundra://test")
        .expect("mock open_uri should pass");
    platform
        .write_clipboard_text("copied")
        .expect("mock clipboard write should pass");
    assert_eq!(
        platform
            .read_clipboard_text()
            .expect("mock clipboard read should pass"),
        "copied"
    );
    platform
        .spawn_detached(&spec)
        .expect("mock detached spawn should pass");
    let exit = platform
        .spawn_wait(&spec)
        .expect("mock wait spawn should pass");

    assert_eq!(exit.code, Some(0));
    assert_eq!(
        platform.calls(),
        vec![
            MockCall::OpenPath(target.clone()),
            MockCall::OpenWith {
                path: target,
                application,
            },
            MockCall::OpenUri("tundra://test".to_string()),
            MockCall::WriteClipboardText("copied".to_string()),
            MockCall::ReadClipboardText,
            MockCall::SpawnDetached(spec.clone()),
            MockCall::SpawnWait(spec),
        ]
    );
}

#[test]
fn doctor_fails_when_a_required_startup_permission_is_missing() {
    let base = unique_temp_root("doctor-startup-permission");
    let platform = mock_platform(&base).with_kind(PlatformKind::Macos);
    platform.set_startup_permission_status(Ok(StartupPermissionStatus::action_required(
        "Full Disk Access",
        "enable it and restart",
    )));

    let report = run_doctor_with(&platform).expect("doctor should report missing permission");
    let check = report
        .environment_checks
        .iter()
        .find(|check| check.label == "Startup permissions")
        .expect("startup permission check");

    assert_eq!(check.status, CheckStatus::Fail);
    assert!(check.message.contains("Full Disk Access"));
    assert!(report.has_failures());
    assert!(
        platform
            .calls()
            .contains(&MockCall::StartupPermissionStatus)
    );
    assert!(
        !platform
            .calls()
            .contains(&MockCall::RequestStartupPermissions)
    );
}

#[test]
fn mock_platform_records_critical_dialog_and_process_liveness_calls() {
    let base = unique_temp_root("mock-watchdog-platform");
    let platform = mock_platform(&base);
    platform.set_process_alive_result(42, Ok(true));

    platform
        .show_critical_error("TundraUX recovered", "incident wd-42")
        .expect("mock critical dialog should pass");
    assert!(
        platform
            .is_process_alive(42)
            .expect("mock process probe should pass")
    );
    assert!(!platform.is_process_alive(7).expect("unknown mock PID"));

    assert_eq!(
        platform.calls(),
        vec![
            MockCall::ShowCriticalError {
                title: "TundraUX recovered".to_string(),
                body: "incident wd-42".to_string(),
            },
            MockCall::IsProcessAlive(42),
            MockCall::IsProcessAlive(7),
        ]
    );
    assert_eq!(
        platform.capabilities().critical_dialog,
        CapabilityStatus::Supported
    );
}

#[test]
fn mock_platform_propagates_injected_critical_dialog_failure() {
    let base = unique_temp_root("mock-critical-error");
    let platform = mock_platform(&base);
    platform.set_critical_error_result(Err(PlatformError::Native {
        operation: "mock critical dialog",
        message: "injected failure".to_string(),
    }));

    let error = platform
        .show_critical_error("title", "body")
        .expect_err("injected critical dialog failure should be returned");
    assert!(error.to_string().contains("injected failure"));
}

#[cfg(windows)]
#[test]
fn windows_platform_reports_the_current_process_as_alive() {
    let platform = tundra_platform::windows::WindowsPlatform;

    assert!(
        platform
            .is_process_alive(std::process::id())
            .expect("current process liveness probe should pass")
    );
    assert!(!platform.is_process_alive(0).expect("PID zero probe"));
}

#[test]
fn mock_platform_returns_injected_file_attributes() {
    let base = unique_temp_root("mock-file-attributes");
    let platform = mock_platform(&base);
    let path = base.join("virtual.lnk");
    let attributes = FileAttributes {
        path: path.clone(),
        is_file: true,
        is_dir: false,
        len: 42,
        readonly: true,
        modified: Some(UNIX_EPOCH),
        hidden: true,
        system: true,
        archive: true,
        symlink: true,
        junction: true,
        reparse_point: true,
        shortcut: true,
    };

    platform.set_file_attributes(path.clone(), attributes.clone());

    assert_eq!(
        platform
            .file_attributes(&path)
            .expect("mock platform should return injected attributes"),
        attributes
    );
}

#[test]
fn default_file_attributes_preserve_basic_metadata_and_dotfile_hidden() {
    let base = unique_temp_root("default-file-attributes");
    fs::create_dir_all(&base).expect("test directory should be creatable");
    let path = base.join(".profile");
    fs::write(&path, b"tux3").expect("test file should be writable");

    let attributes =
        default_file_attributes(&path).expect("default file attributes should read the test file");

    assert_eq!(attributes.path, path);
    assert!(attributes.is_file);
    assert!(!attributes.is_dir);
    assert_eq!(attributes.len, 4);
    assert!(attributes.modified.is_some());
    assert!(attributes.hidden);
    assert!(!attributes.system);
    assert!(!attributes.archive);
    assert!(!attributes.symlink);
    assert!(!attributes.junction);
    assert!(!attributes.reparse_point);
    assert!(!attributes.shortcut);

    fs::remove_dir_all(base).expect("test directory should be removable");
}

#[cfg(unix)]
#[test]
fn default_file_attributes_detect_symlink_while_preserving_target_metadata() {
    use std::os::unix::fs::symlink;

    let base = unique_temp_root("default-file-attributes-symlink");
    fs::create_dir_all(&base).expect("test directory should be creatable");
    let target = base.join("target.txt");
    let link = base.join("link.txt");
    fs::write(&target, b"tundra").expect("target file should be writable");
    symlink(&target, &link).expect("symlink should be creatable");

    let attributes =
        default_file_attributes(&link).expect("default file attributes should read the symlink");

    assert_eq!(attributes.path, link);
    assert!(attributes.is_file);
    assert!(!attributes.is_dir);
    assert_eq!(attributes.len, 6);
    assert!(attributes.symlink);
    assert!(!attributes.junction);
    assert!(!attributes.reparse_point);

    fs::remove_dir_all(base).expect("test directory should be removable");
}

#[test]
fn platform_temp_helpers_create_under_app_temp_path_and_cleanup() {
    let base = unique_temp_root("platform-temp");
    let platform = mock_platform(&base);
    let temp_root = platform
        .app_paths()
        .expect("mock app paths should resolve")
        .temp_path()
        .to_path_buf();

    let file = platform
        .create_temp_file("mock")
        .expect("platform should create temp file");
    let dir = platform
        .create_temp_dir("mock")
        .expect("platform should create temp directory");

    assert_eq!(file.parent(), Some(temp_root.as_path()));
    assert_eq!(dir.parent(), Some(temp_root.as_path()));
    assert!(file.is_file());
    assert!(dir.is_dir());

    platform
        .cleanup_temp_path(&file)
        .expect("platform should clean temp file");
    platform
        .cleanup_temp_path(&dir)
        .expect("platform should clean temp directory");
    assert!(!file.exists());
    assert!(!dir.exists());

    cleanup_temp_path(&base).expect("fixture root should be removable");
}

#[test]
fn temp_file_and_dir_helpers_create_unique_paths_and_cleanup() {
    let base = unique_temp_root("temp-helpers");
    let temp_root = base.join("TundraUX3");

    let first_file =
        create_temp_file(&temp_root, "export").expect("first temp file should be created");
    let second_file =
        create_temp_file(&temp_root, "export").expect("second temp file should be created");
    let temp_dir = create_temp_dir(&temp_root, "work").expect("temp directory should be created");

    assert_ne!(first_file, second_file);
    assert_temp_child(&first_file, &temp_root, "export", "tmp");
    assert_temp_child(&second_file, &temp_root, "export", "tmp");
    assert_temp_child(&temp_dir, &temp_root, "work", "dir");
    assert!(first_file.is_file());
    assert!(second_file.is_file());
    assert!(temp_dir.is_dir());

    cleanup_temp_path(&first_file).expect("first temp file should be removable");
    cleanup_temp_path(&second_file).expect("second temp file should be removable");
    cleanup_temp_path(&temp_dir).expect("temp directory should be removable");
    cleanup_temp_path(&base).expect("temp fixture root should be removable");
    assert!(!base.exists());
}

#[test]
fn unsupported_platform_reports_unsupported_capabilities() {
    let platform = UnsupportedPlatform;

    assert_eq!(platform.kind(), PlatformKind::Unsupported);
    assert!(
        platform
            .capabilities()
            .checks()
            .into_iter()
            .all(|(_, status)| status == CapabilityStatus::Unsupported)
    );

    match platform
        .app_paths()
        .expect_err("unsupported platform should not resolve app paths")
    {
        PlatformError::Unsupported { capability } => assert_eq!(capability, "app_paths"),
        error => panic!("expected unsupported app_paths error, got {error:?}"),
    }

    assert!(matches!(
        platform.show_critical_error("title", "body"),
        Err(PlatformError::Unsupported {
            capability: "critical_dialog"
        })
    ));
    assert!(matches!(
        platform.is_process_alive(42),
        Err(PlatformError::Unsupported {
            capability: "process_liveness"
        })
    ));
}

#[test]
fn process_policy_rejects_windows_scripts_when_enabled() {
    for extension in ["cmd", "BAT", "Ps1"] {
        let spec = ProcessSpec::new(PathBuf::from(format!("script.{extension}")));

        match validate_process_spec(&spec, true)
            .expect_err("Windows scripts should be rejected when policy is enabled")
        {
            PlatformError::ProcessPolicy { message } => {
                assert!(message.contains("refusing to launch script file"));
                assert!(message.contains(&format!("script.{extension}")));
            }
            error => panic!("expected process policy error, got {error:?}"),
        }
    }
}

#[test]
fn process_policy_allows_native_programs_and_optional_windows_scripts() {
    validate_process_spec(&ProcessSpec::new(PathBuf::from("tool.exe")), true)
        .expect("native program should be allowed");
    validate_process_spec(&ProcessSpec::new(PathBuf::from("tool")), true)
        .expect("programs without script extensions should be allowed");
    validate_process_spec(&ProcessSpec::new(PathBuf::from("script.cmd")), false)
        .expect("Windows script should be allowed when policy is disabled");
}

#[test]
fn process_spec_rejects_empty_program() {
    match validate_process_spec(&ProcessSpec::new(PathBuf::new()), false)
        .expect_err("empty process program should fail")
    {
        PlatformError::InvalidInput { message } => {
            assert!(message.contains("must not be empty"));
        }
        error => panic!("expected invalid input error, got {error:?}"),
    }
}

#[test]
fn binary_dir_backed_path_checks_pass_and_cleanup_created_app_directory() {
    let base = unique_temp_root("binary-dir-path-checks");
    let binary_dir = base.join("bin");
    fs::create_dir_all(&binary_dir).expect("binary directory should be creatable");
    let paths = AppPaths::from_binary_dir(&binary_dir)
        .expect("absolute fake binary directory should resolve");
    let app_dir = binary_dir.join("TundraUX3");

    let config_check = check_directory_read_write(
        "Config parent",
        paths
            .config_path()
            .parent()
            .expect("config path has parent"),
    );
    let data_check = check_directory_read_write("Data path", paths.data_path());
    let cache_check = check_directory_read_write("Cache path", paths.cache_path());
    let logs_check = check_directory_read_write("Logs path", paths.logs_path());
    let temp_check = check_directory_read_write("Temp path", paths.temp_path());

    assert_eq!(config_check.status, CheckStatus::Pass);
    assert_eq!(config_check.path, app_dir);
    assert_eq!(data_check.status, CheckStatus::Pass);
    assert_eq!(data_check.path, paths.data_path());
    assert_eq!(cache_check.status, CheckStatus::Pass);
    assert_eq!(cache_check.path, paths.cache_path());
    assert_eq!(logs_check.status, CheckStatus::Pass);
    assert_eq!(logs_check.path, paths.logs_path());
    assert_eq!(temp_check.status, CheckStatus::Pass);
    assert_eq!(temp_check.path, paths.temp_path());
    assert!(!app_dir.exists());

    fs::remove_dir_all(base).expect("test directory should be removable");
}

#[test]
fn windows_11_requires_build_22000() {
    assert_eq!(
        format!("{:?}", classify_windows_build(21_999)),
        "UnsupportedWindows"
    );
    assert_eq!(
        format!("{:?}", classify_windows_build(22_000)),
        "Windows11OrNewer"
    );
    assert_eq!(
        format!("{:?}", classify_windows_build(26_100)),
        "Windows11OrNewer"
    );
}

#[test]
fn terminal_detection_uses_wt_session_value() {
    assert!(is_windows_terminal_session(Some("session-id")));
    assert!(!is_windows_terminal_session(Some("")));
    assert!(!is_windows_terminal_session(None));
}

#[test]
fn terminal_environment_check_is_reported_by_platform_layer() {
    let windows_terminal =
        terminal_environment_check_with(PlatformKind::Windows, Some("session-id"));
    let conhost = terminal_environment_check_with(PlatformKind::Windows, None);
    let macos_terminal = terminal_environment_check_with(PlatformKind::Macos, None);

    assert_eq!(windows_terminal.status, CheckStatus::Pass);
    assert_eq!(windows_terminal.message, "Windows Terminal detected");
    assert_eq!(conhost.status, CheckStatus::Warning);
    assert!(conhost.message.contains("best-effort"));
    assert_eq!(macos_terminal.status, CheckStatus::Pass);
}

#[test]
fn executable_open_policy_applies_on_every_platform() {
    let base = unique_temp_root("windows-external-open-policy");
    let windows_platform = mock_platform(&base).with_kind(PlatformKind::Windows);
    let macos_platform = mock_platform(&base).with_kind(PlatformKind::Macos);
    let program = base.join("tool.exe");
    let attributes = file_attributes(program.clone());

    let windows_policy = windows_platform.external_open_policy(&program, &attributes);
    let macos_policy = macos_platform.external_open_policy(&program, &attributes);

    assert!(!windows_policy.is_allowed());
    assert!(
        windows_policy
            .blocked_reason()
            .unwrap_or_default()
            .contains("Windows")
    );
    assert!(!macos_policy.is_allowed());

    cleanup_temp_path(&base).expect("fixture root should be removable");
}

#[test]
fn directory_permission_check_creates_and_removes_probe_file() {
    let base = unique_temp_root("cleanup");
    let target = base.join("state");

    let check = check_directory_read_write("State path", &target);

    assert_eq!(check.status, CheckStatus::Pass);
    assert!(!target.exists());
    assert!(!base.exists());
}

#[test]
fn directory_permission_check_preserves_preexisting_directory() {
    let base = unique_temp_root("preexisting");
    let target = base.join("state");
    fs::create_dir_all(&target).expect("preexisting test directory should be creatable");

    let check = check_directory_read_write("State path", &target);

    assert_eq!(check.status, CheckStatus::Pass);
    assert!(target.is_dir());
    let probe_files: Vec<_> = fs::read_dir(&target)
        .expect("target should be readable")
        .filter_map(Result::ok)
        .filter(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with(".tundraux3-doctor-probe")
        })
        .collect();
    assert!(probe_files.is_empty());

    fs::remove_dir_all(base).expect("test directory should be removable");
}

#[test]
fn directory_permission_check_fails_when_target_is_a_file() {
    let base = unique_temp_root("file-target");
    fs::create_dir_all(&base).expect("base test directory should be creatable");
    let target = base.join("state");
    fs::write(&target, b"not a directory").expect("file target should be writable");

    let check = check_directory_read_write("State path", &target);

    assert_eq!(check.status, CheckStatus::Fail);
    assert!(check.message.contains("not a directory"));
    assert!(target.is_file());

    fs::remove_dir_all(base).expect("test directory should be removable");
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
        "tundra-platform-test-{}-{}-{case}",
        std::process::id(),
        nanos
    ))
}

fn assert_relative_error_mentions(error: tundra_platform::PathResolutionError, expected: &str) {
    let message = error.to_string().to_ascii_lowercase();

    assert!(message.contains("absolute"));
    assert!(
        message.contains(expected),
        "error should mention {expected}: {message}"
    );
}

fn assert_temp_child(path: &Path, temp_root: &Path, prefix: &str, suffix: &str) {
    assert_eq!(path.parent(), Some(temp_root));

    let file_name = path
        .file_name()
        .expect("temp child should have file name")
        .to_string_lossy();
    assert!(
        file_name.starts_with(&format!(".tundraux3-{prefix}-")),
        "unexpected temp child name: {file_name}"
    );
    assert!(
        file_name.ends_with(&format!("-{suffix}")),
        "unexpected temp child name: {file_name}"
    );
}

fn file_attributes(path: PathBuf) -> FileAttributes {
    FileAttributes {
        path,
        is_file: true,
        is_dir: false,
        len: 0,
        readonly: false,
        modified: None,
        hidden: false,
        system: false,
        archive: false,
        symlink: false,
        junction: false,
        reparse_point: false,
        shortcut: false,
    }
}
