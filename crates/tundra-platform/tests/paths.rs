use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use tundra_platform::{
    AppPaths, CheckStatus, WindowsBuildClass, check_directory_read_write, classify_windows_build,
    is_windows_terminal_session,
};

#[test]
fn app_paths_are_scoped_to_binary_directory() {
    let base = unique_temp_root("binary-dir");
    let binary_dir = base.join("bin");
    std::fs::create_dir_all(&binary_dir).expect("binary directory should be creatable");

    let paths = AppPaths::from_binary_dir(&binary_dir)
        .expect("absolute fake binary directory should resolve");

    assert_eq!(
        paths.config_path(),
        binary_dir.join("TundraUX3").join("config.toml").as_path()
    );
    assert_eq!(
        paths.data_path(),
        binary_dir.join("TundraUX3").join("state").as_path()
    );
    assert_eq!(
        paths.cache_path(),
        binary_dir.join("TundraUX3").join("cache").as_path()
    );
    assert_binary_dir_template(AppPaths::CONFIG_TEMPLATE, "config.toml");
    assert_binary_dir_template(AppPaths::DATA_TEMPLATE, "state");
    assert_binary_dir_template(AppPaths::CACHE_TEMPLATE, "cache");

    std::fs::remove_dir_all(base).expect("test directory should be removable");
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

#[test]
fn binary_dir_backed_path_checks_pass_and_cleanup_created_app_directory() {
    let base = unique_temp_root("binary-dir-path-checks");
    let binary_dir = base.join("bin");
    std::fs::create_dir_all(&binary_dir).expect("binary directory should be creatable");
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

    assert_eq!(config_check.status, CheckStatus::Pass);
    assert_eq!(config_check.path, app_dir);
    assert_eq!(data_check.status, CheckStatus::Pass);
    assert_eq!(data_check.path, paths.data_path());
    assert_eq!(cache_check.status, CheckStatus::Pass);
    assert_eq!(cache_check.path, paths.cache_path());
    assert!(!app_dir.exists());

    std::fs::remove_dir_all(base).expect("test directory should be removable");
}

#[test]
fn windows_11_requires_build_22000() {
    assert_eq!(
        classify_windows_build(21_999),
        WindowsBuildClass::UnsupportedWindows
    );
    assert_eq!(
        classify_windows_build(22_000),
        WindowsBuildClass::Windows11OrNewer
    );
    assert_eq!(
        classify_windows_build(26_100),
        WindowsBuildClass::Windows11OrNewer
    );
}

#[test]
fn terminal_detection_uses_wt_session_value() {
    assert!(is_windows_terminal_session(Some("session-id")));
    assert!(!is_windows_terminal_session(Some("")));
    assert!(!is_windows_terminal_session(None));
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
    std::fs::create_dir_all(&target).expect("preexisting test directory should be creatable");

    let check = check_directory_read_write("State path", &target);

    assert_eq!(check.status, CheckStatus::Pass);
    assert!(target.is_dir());
    let probe_files: Vec<_> = std::fs::read_dir(&target)
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

    std::fs::remove_dir_all(base).expect("test directory should be removable");
}

#[test]
fn directory_permission_check_fails_when_target_is_a_file() {
    let base = unique_temp_root("file-target");
    std::fs::create_dir_all(&base).expect("base test directory should be creatable");
    let target = base.join("state");
    std::fs::write(&target, b"not a directory").expect("file target should be writable");

    let check = check_directory_read_write("State path", &target);

    assert_eq!(check.status, CheckStatus::Fail);
    assert!(check.message.contains("not a directory"));
    assert!(target.is_file());

    std::fs::remove_dir_all(base).expect("test directory should be removable");
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

fn assert_binary_dir_template(template: &str, leaf: &str) {
    let normalized = template.replace('\\', "/");

    assert!(
        normalized.contains("<binary-dir>") || normalized.contains("<executable-dir>"),
        "template should identify the binary or executable directory: {template}"
    );
    assert!(
        normalized.ends_with(&format!("/TundraUX3/{leaf}")),
        "template should end in the TundraUX3 app path: {template}"
    );
    assert!(!template.contains("APPDATA"));
    assert!(!template.contains("LOCALAPPDATA"));
}
