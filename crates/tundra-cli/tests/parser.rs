use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use tundra_cli::{CliCommand, CliError, parse_args, run, run_with_platform};
use tundra_platform::mock::{MockPlatform, UnsupportedPlatform};
use tundra_platform::{
    Platform, PlatformKind, UserDirs, build_macos_app_paths, build_windows_app_paths,
};
use tundra_storage::{StorageLayout, StorageManager};

#[test]
fn no_args_dispatches_help() {
    assert_eq!(parse_args(std::iter::empty::<&str>()), Ok(CliCommand::Help));
}

#[test]
fn doctor_arg_dispatches_doctor() {
    assert_eq!(parse_args(["doctor"]), Ok(CliCommand::Doctor));
}

#[test]
fn paths_arg_dispatches_paths() {
    assert_eq!(parse_args(["paths"]), Ok(CliCommand::Paths));
}

#[test]
fn explain_arg_dispatches_explain() {
    assert_eq!(parse_args(["explain"]), Ok(CliCommand::Explain));
}

#[test]
fn new_arg_dispatches_new() {
    assert_eq!(parse_args(["new"]), Ok(CliCommand::New));
}

#[test]
fn unknown_arg_is_an_error() {
    assert_eq!(
        parse_args(["repair"]),
        Err(CliError::UnknownCommand("repair".to_string()))
    );
}

#[test]
fn extra_arg_is_an_error() {
    assert_eq!(
        parse_args(["doctor", "--json"]),
        Err(CliError::UnexpectedArgument("--json".to_string()))
    );
}

#[test]
fn help_command_writes_usage_to_stdout() {
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();

    let exit_code = run(["help"], &mut stdout, &mut stderr);

    assert_eq!(exit_code, 0);
    assert!(stderr.is_empty());
    let stdout = String::from_utf8(stdout).expect("help output should be utf8");
    assert!(stdout.contains("Usage: tundra-cli <doctor|explain|new|paths>"));
    assert!(stdout.contains("new     Clear saved TundraUX3 data"));
    assert!(!stdout.contains("Windows 11"));
    assert!(!stdout.contains("Windows Terminal"));
}

#[test]
fn explain_command_prints_startup_and_boundary_notes() {
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();

    let exit_code = run(["explain"], &mut stdout, &mut stderr);

    assert_eq!(exit_code, 0);
    assert!(stderr.is_empty());
    let stdout = String::from_utf8(stdout).expect("explain output should be utf8");
    assert!(stdout.contains("Startup flow"));
    assert!(stdout.contains("tundra-cli"));
    assert!(stdout.contains("Kernel boundary"));
    assert!(stdout.contains("UI boundary"));
    assert!(stdout.contains("tundra-platform"));
    assert!(stdout.contains("tundra-shell"));
    assert!(stdout.contains("doctor, paths, explain, new"));
    assert!(!stdout.contains("Windows 11"));
    assert!(!stdout.contains("Windows Terminal"));
}

#[test]
fn new_command_clears_saved_content_and_recreates_default_storage() {
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let tree = TempTree::new("new-reset");
    let platform = mock_windows_platform(tree.path());
    let app_paths = platform.app_paths().expect("mock paths should resolve");
    let layout = StorageLayout::from_app_paths(&app_paths);

    StorageManager::open(app_paths.clone()).expect("initial storage should open");
    fs::write(&layout.config_path, "custom config").expect("custom config fixture");
    fs::write(layout.data_path.join("extra-state.txt"), "state").expect("extra state fixture");
    fs::write(layout.cache_path.join("cached.txt"), "cache").expect("cache fixture");
    fs::write(layout.logs_path.join("audit.v1.log"), "audit").expect("audit fixture");
    fs::write(layout.temp_path.join("temp.txt"), "temp").expect("temp fixture");

    let exit_code = run_with_platform(["new"], &platform, &mut stdout, &mut stderr);

    assert_eq!(exit_code, 0);
    assert!(stderr.is_empty());
    let stdout = String::from_utf8(stdout).expect("new output should be utf8");
    assert!(stdout.contains("TundraUX3 storage reset"));
    assert!(stdout.contains("Recreated storage files:"));
    assert!(layout.config_path.is_file());
    assert!(layout.users_path.is_file());
    assert!(layout.state_path.is_file());
    assert!(layout.recent_files_path.is_file());
    assert!(layout.sessions_path.is_file());
    assert!(!layout.data_path.join("extra-state.txt").exists());
    assert!(!layout.cache_path.join("cached.txt").exists());
    assert!(!layout.logs_path.join("audit.v1.log").exists());
    assert!(!layout.temp_path.join("temp.txt").exists());

    let manager = StorageManager::open(app_paths).expect("reset storage should reopen");
    assert!(
        manager
            .manager
            .load_users()
            .expect("users")
            .users
            .is_empty()
    );
    assert_eq!(manager.manager.load_config().expect("config").theme, "dark");
}

#[test]
fn paths_command_prints_injected_windows_resolved_and_storage_paths() {
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let tree = TempTree::new("windows-paths");
    let platform = mock_windows_platform(tree.path());

    let exit_code = run_with_platform(["paths"], &platform, &mut stdout, &mut stderr);

    assert_eq!(exit_code, 0);
    assert!(stderr.is_empty());
    let stdout = String::from_utf8(stdout).expect("paths output should be utf8");
    assert!(stdout.contains("Path templates:"));
    assert!(stdout.contains("Resolved paths:"));
    assert!(stdout.contains("Storage files:"));
    assert_path_labels(&stdout);
    assert_storage_labels(&stdout);
    assert_windows_resolved_path_markers(&stdout);
    assert_windows_storage_file_markers(&stdout);
}

#[test]
fn paths_command_prints_injected_macos_resolved_and_storage_paths() {
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let tree = TempTree::new("macos-paths");
    let platform = mock_macos_platform(tree.path());

    let exit_code = run_with_platform(["paths"], &platform, &mut stdout, &mut stderr);

    assert_eq!(exit_code, 0);
    assert!(stderr.is_empty());
    let stdout = String::from_utf8(stdout).expect("paths output should be utf8");
    assert!(stdout.contains("Path templates:"));
    assert!(stdout.contains("Resolved paths:"));
    assert!(stdout.contains("Storage files:"));
    assert_path_labels(&stdout);
    assert_storage_labels(&stdout);
    assert_macos_resolved_path_markers(&stdout);
    assert_macos_storage_file_markers(&stdout);
}

#[test]
fn paths_command_reports_unsupported_platform_from_injected_platform() {
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let platform = UnsupportedPlatform;

    let exit_code = run_with_platform(["paths"], &platform, &mut stdout, &mut stderr);

    assert_eq!(exit_code, 1);
    let stdout = String::from_utf8(stdout).expect("paths output should be utf8");
    let stderr = String::from_utf8(stderr).expect("paths error output should be utf8");
    assert!(stdout.contains("Path templates:"));
    assert_path_labels(&stdout);
    assert!(stderr.contains("ERROR: platform capability is unsupported: app_paths"));
}

#[test]
fn doctor_command_passes_and_bootstraps_storage_with_injected_macos_platform() {
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let tree = TempTree::new("doctor-macos");
    let platform = mock_macos_platform(tree.path());

    let exit_code = run_with_platform(["doctor"], &platform, &mut stdout, &mut stderr);

    assert_eq!(exit_code, 0);
    assert!(stderr.is_empty());
    let stdout = String::from_utf8(stdout).expect("doctor output should be utf8");
    assert!(stdout.contains("TundraUX3 doctor"));
    assert!(stdout.contains("Platform kind: macOS"));
    assert!(stdout.contains("Path templates:"));
    assert!(stdout.contains("Resolved paths:"));
    assert!(stdout.contains("Checks:"));
    assert!(stdout.contains("Platform checks:"));
    assert!(stdout.contains("Terminal check:"));
    assert!(stdout.contains("Capability checks:"));
    assert!(stdout.contains("Path checks:"));
    assert!(stdout.contains("Storage checks:"));
    assert!(stdout.contains("[PASS] Storage bootstrap: storage initialized and loaded cleanly"));
    assert!(stdout.contains("Doctor result: PASS"));
    assert_path_labels(&stdout);
    assert_macos_resolved_path_markers(&stdout);

    assert!(
        tree.path()
            .join("Home/Library/Application Support/TundraUX3/config.toml")
            .exists()
    );
    assert!(
        tree.path()
            .join("Home/Library/Application Support/TundraUX3/state/state.v1.json")
            .exists()
    );
    assert!(
        !tree
            .path()
            .join("Home/Library/Logs/TundraUX3/audit.v1.log")
            .exists()
    );
}

#[test]
fn doctor_command_reports_checks_and_skips_storage_when_app_paths_fail() {
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let platform = UnsupportedPlatform;

    let exit_code = run_with_platform(["doctor"], &platform, &mut stdout, &mut stderr);

    assert_eq!(exit_code, 1);
    let stdout = String::from_utf8(stdout).expect("doctor output should be utf8");
    let stderr = String::from_utf8(stderr).expect("doctor error output should be utf8");
    assert!(stdout.contains("TundraUX3 doctor"));
    assert!(stdout.contains("Platform kind: Unsupported"));
    assert!(stdout.contains("Path templates:"));
    assert!(stdout.contains("Checks:"));
    assert!(stdout.contains("Terminal check:"));
    assert!(stdout.contains("Capability checks:"));
    assert!(stdout.contains("Path checks:"));
    assert!(stdout.contains("[FAIL] App paths: platform capability is unsupported: app_paths"));
    assert!(!stdout.contains("Storage checks:"));
    assert!(stderr.contains("Doctor result: FAIL"));
}

#[test]
fn unknown_command_exits_two_and_writes_error_to_stderr() {
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();

    let exit_code = run(["repair"], &mut stdout, &mut stderr);

    assert_eq!(exit_code, 2);
    assert!(stdout.is_empty());
    let stderr = String::from_utf8(stderr).expect("error output should be utf8");
    assert!(stderr.contains("ERROR: unknown command: repair"));
    assert!(stderr.contains("Usage: tundra-cli <doctor|explain|new|paths>"));
}

fn assert_path_labels(output: &str) {
    assert!(output.contains("Config path:"));
    assert!(output.contains("Data path:"));
    assert!(output.contains("Cache path:"));
    assert!(output.contains("Logs path:"));
    assert!(output.contains("Temp path:"));
}

fn assert_storage_labels(output: &str) {
    assert!(output.contains("Config file:"));
    assert!(output.contains("State file:"));
    assert!(output.contains("Recent files:"));
    assert!(output.contains("Sessions file:"));
    assert!(output.contains("Users file:"));
    assert!(output.contains("Audit log:"));
}

fn assert_windows_resolved_path_markers(output: &str) {
    let normalized = output.replace('\\', "/");

    assert!(normalized.contains("Roaming/TundraUX3/config.toml"));
    assert!(normalized.contains("Local/TundraUX3/state"));
    assert!(normalized.contains("Local/TundraUX3/cache"));
    assert!(normalized.contains("Local/TundraUX3/logs"));
    assert!(normalized.contains("Temp/TundraUX3"));
}

fn assert_windows_storage_file_markers(output: &str) {
    let normalized = output.replace('\\', "/");

    assert!(normalized.contains("Local/TundraUX3/state/state.v1.json"));
    assert!(normalized.contains("Local/TundraUX3/state/recent-files.v1.json"));
    assert!(normalized.contains("Local/TundraUX3/state/sessions.v1.json"));
    assert!(normalized.contains("Local/TundraUX3/state/users.v2.json"));
    assert!(normalized.contains("Local/TundraUX3/logs/audit.v1.log"));
}

fn assert_macos_resolved_path_markers(output: &str) {
    let normalized = output.replace('\\', "/");

    assert!(normalized.contains("Home/Library/Application Support/TundraUX3/config.toml"));
    assert!(normalized.contains("Home/Library/Application Support/TundraUX3/state"));
    assert!(normalized.contains("Home/Library/Caches/TundraUX3"));
    assert!(normalized.contains("Home/Library/Logs/TundraUX3"));
    assert!(normalized.contains("Temp/TundraUX3"));
}

fn assert_macos_storage_file_markers(output: &str) {
    let normalized = output.replace('\\', "/");

    assert!(normalized.contains("Home/Library/Application Support/TundraUX3/state/state.v1.json"));
    assert!(
        normalized
            .contains("Home/Library/Application Support/TundraUX3/state/recent-files.v1.json")
    );
    assert!(
        normalized.contains("Home/Library/Application Support/TundraUX3/state/sessions.v1.json")
    );
    assert!(normalized.contains("Home/Library/Application Support/TundraUX3/state/users.v2.json"));
    assert!(normalized.contains("Home/Library/Logs/TundraUX3/audit.v1.log"));
}

fn mock_windows_platform(base: &Path) -> MockPlatform {
    let user_dirs = user_dirs(base);
    let app_paths =
        build_windows_app_paths(base.join("Roaming"), base.join("Local"), base.join("Temp"))
            .expect("absolute windows app path roots should resolve");

    MockPlatform::new(user_dirs, app_paths).with_kind(PlatformKind::Windows)
}

fn mock_macos_platform(base: &Path) -> MockPlatform {
    let user_dirs = user_dirs(base);
    let app_paths = build_macos_app_paths(base.join("Home"), base.join("Temp"))
        .expect("absolute macOS app path roots should resolve");

    MockPlatform::new(user_dirs, app_paths).with_kind(PlatformKind::Macos)
}

fn user_dirs(base: &Path) -> UserDirs {
    UserDirs::new(
        base.join("Desktop"),
        base.join("Documents"),
        base.join("Downloads"),
        base.join("Pictures"),
        base.join("Videos"),
        base.join("Music"),
        base.join("AppData"),
    )
    .expect("absolute user directory roots should resolve")
}

#[derive(Debug)]
struct TempTree {
    path: PathBuf,
}

impl TempTree {
    fn new(name: &str) -> Self {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock should be after unix epoch")
            .as_nanos();
        let path =
            std::env::temp_dir().join(format!("tundra-cli-{name}-{}-{suffix}", std::process::id()));

        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempTree {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}
