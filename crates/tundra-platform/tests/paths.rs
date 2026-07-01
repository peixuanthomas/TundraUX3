use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use tundra_platform::{
    AppPaths, CheckStatus, WindowsBuildClass, check_directory_read_write, classify_windows_build,
    is_windows_terminal_session,
};

#[test]
fn app_paths_use_fixed_windows_locations() {
    let paths = AppPaths::from_roots(
        PathBuf::from(r"C:\Users\Ada\AppData\Roaming"),
        PathBuf::from(r"C:\Users\Ada\AppData\Local"),
    )
    .expect("absolute fake Windows paths should resolve");

    assert_eq!(
        paths.config_path(),
        Path::new(r"C:\Users\Ada\AppData\Roaming\TundraUX3\config.toml")
    );
    assert_eq!(
        paths.data_path(),
        Path::new(r"C:\Users\Ada\AppData\Local\TundraUX3\state")
    );
    assert_eq!(
        paths.cache_path(),
        Path::new(r"C:\Users\Ada\AppData\Local\TundraUX3\cache")
    );
    assert_eq!(
        AppPaths::CONFIG_TEMPLATE,
        r"%APPDATA%\TundraUX3\config.toml"
    );
    assert_eq!(AppPaths::DATA_TEMPLATE, r"%LOCALAPPDATA%\TundraUX3\state\");
    assert_eq!(AppPaths::CACHE_TEMPLATE, r"%LOCALAPPDATA%\TundraUX3\cache\");
}

#[test]
fn app_paths_reject_relative_roots() {
    let error = AppPaths::from_roots(
        PathBuf::from(r"relative\Roaming"),
        PathBuf::from(r"C:\Users\Ada\AppData\Local"),
    )
    .expect_err("relative APPDATA root should fail");

    assert!(error.to_string().contains("APPDATA"));
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
