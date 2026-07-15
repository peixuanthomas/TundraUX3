use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use tundra_cli::{
    CliCommand, CliError, ConfigAction, ConfigField, ConfigUpdate, parse_args, run,
    run_with_platform, run_with_platform_and_asset_root,
    run_with_platform_and_managed_weathr_launcher, run_with_platform_and_weathr_launcher,
};
use tundra_platform::mock::{MockCall, MockPlatform, UnsupportedPlatform};
use tundra_platform::{
    Platform, PlatformKind, UserDirs, build_macos_app_paths, build_windows_app_paths,
};
use tundra_storage::{StorageConfig, StorageLayout, StorageManager};
use tundra_watchdog::{
    BoundaryKind, BoundarySpec, ProcessWatchdog, RecoveryOutcome, WatchdogConfig, WatchdogRuntime,
};

#[test]
fn no_args_dispatches_help() {
    assert_eq!(parse_args(std::iter::empty::<&str>()), Ok(CliCommand::Help));
}

#[test]
fn doctor_arg_dispatches_doctor() {
    assert_eq!(parse_args(["doctor"]), Ok(CliCommand::Doctor));
}

#[test]
fn editor_arg_dispatches_editor() {
    assert_eq!(parse_args(["editor"]), Ok(CliCommand::Editor));
    assert_eq!(
        parse_args(["editor", "note.md"]),
        Err(CliError::UnexpectedArgument("note.md".to_string()))
    );
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
fn weathr_arg_dispatches_weathr() {
    assert_eq!(parse_args(["weathr"]), Ok(CliCommand::Weathr));
}

#[test]
fn config_args_parse_safe_get_and_set_commands() {
    assert_eq!(
        parse_args(["config"]),
        Ok(CliCommand::Config(ConfigAction::Get(None)))
    );
    assert_eq!(
        parse_args(["config", "get", "theme"]),
        Ok(CliCommand::Config(ConfigAction::Get(Some(
            ConfigField::Theme
        ))))
    );
    assert_eq!(
        parse_args(["config", "set", "theme", "light"]),
        Ok(CliCommand::Config(ConfigAction::Set(ConfigUpdate::Theme(
            "light".to_string()
        ))))
    );
    assert_eq!(
        parse_args(["config", "set", "address", "New", "York"]),
        Ok(CliCommand::Config(ConfigAction::Set(
            ConfigUpdate::Address("New York".to_string())
        )))
    );
}

#[test]
fn config_args_reject_username_and_password_updates() {
    assert_eq!(
        parse_args(["config", "set", "username", "admin2"]),
        Err(CliError::ForbiddenConfigField("username".to_string()))
    );
    assert_eq!(
        parse_args(["config", "set", "password", "secret"]),
        Err(CliError::ForbiddenConfigField("password".to_string()))
    );
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
    assert!(stdout.contains("Usage: tundra-cli <config|doctor|editor|explain|new|paths|weathr>"));
    assert!(stdout.contains("config  View or update user config"));
    assert!(stdout.contains("new     Clear saved TundraUX3 data"));
    assert!(stdout.contains("editor  Launch the shell directly into the Markdown editor"));
    assert!(stdout.contains("weathr  Launch the terminal weather scene"));
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
    assert!(stdout.contains("doctor, editor, paths, explain, new, weathr"));
    assert!(!stdout.contains("Windows 11"));
    assert!(!stdout.contains("Windows Terminal"));
}

#[test]
fn weathr_command_launches_injected_runner() {
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let tree = TempTree::new("weathr-launch");
    let platform = mock_windows_platform(tree.path());

    let exit_code = run_with_platform_and_weathr_launcher(
        ["weathr"],
        &platform,
        &mut stdout,
        &mut stderr,
        |_options| Ok::<(), &'static str>(()),
    );

    assert_eq!(exit_code, 0);
    assert!(stdout.is_empty());
    assert!(stderr.is_empty());
}

#[test]
fn weathr_command_reports_injected_runner_error() {
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let tree = TempTree::new("weathr-launch-error");
    let platform = mock_windows_platform(tree.path());

    let exit_code = run_with_platform_and_weathr_launcher(
        ["weathr"],
        &platform,
        &mut stdout,
        &mut stderr,
        |_options| Err::<(), &'static str>("terminal unavailable"),
    );

    assert_eq!(exit_code, 1);
    assert!(stdout.is_empty());
    let stderr = String::from_utf8(stderr).expect("weathr error output should be utf8");
    assert!(stderr.contains("ERROR: could not launch weathr: terminal unavailable"));
}

#[test]
fn managed_weathr_launcher_receives_the_explicit_app_watchdog() {
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let tree = TempTree::new("managed-weathr-launch");
    let platform = mock_windows_platform(tree.path());
    let watchdog = test_weathr_watchdog();
    let mut received_app_id = None;

    let exit_code = run_with_platform_and_managed_weathr_launcher(
        ["weathr"],
        &platform,
        &mut stdout,
        &mut stderr,
        &watchdog.process,
        watchdog.weathr.clone(),
        |_options, watchdog| {
            received_app_id = Some(watchdog.descriptor().id.as_str().to_string());
            Ok(())
        },
    );

    assert_eq!(exit_code, 0);
    assert_eq!(received_app_id.as_deref(), Some("weathr"));
    assert!(stderr.is_empty());
}

#[test]
fn unrecoverable_managed_weathr_panic_routes_to_stderr_and_critical_dialog() {
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let tree = TempTree::new("managed-weathr-panic");
    let platform = mock_windows_platform(tree.path());
    let watchdog = test_weathr_watchdog();

    let exit_code = run_with_platform_and_managed_weathr_launcher(
        ["weathr"],
        &platform,
        &mut stdout,
        &mut stderr,
        &watchdog.process,
        watchdog.weathr.clone(),
        |_options, _watchdog| {
            Err(tundra_weathr::WeathrRunError::Panic {
                incident_id: "test-weathr-panic".to_string(),
                reason: "render failed".to_string(),
            })
        },
    );

    assert_eq!(exit_code, 1);
    let stderr = String::from_utf8(stderr).expect("managed Weathr error output is UTF-8");
    assert!(stderr.contains("render failed"));
    assert!(stderr.contains("test-weathr-panic"));
    assert!(stderr.contains("report path unavailable"));
    assert!(platform.calls().iter().any(|call| matches!(
        call,
        MockCall::ShowCriticalError { title, body }
            if title.contains("Weathr") && body.contains("render failed")
    )));
}

#[test]
fn managed_cli_routes_pending_watchdog_incidents_for_non_weathr_commands() {
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let tree = TempTree::new("managed-watchdog-drain");
    let platform = mock_windows_platform(tree.path());
    let watchdog = test_weathr_watchdog();
    let weathr = watchdog.weathr.clone();
    let caught = weathr
        .run_boundary(
            BoundarySpec::new("test.recovered", BoundaryKind::Worker),
            std::panic::AssertUnwindSafe(|| -> () { panic!("managed incident") }),
        )
        .expect_err("test boundary should catch panic");
    caught
        .finalize(RecoveryOutcome::Recovered(
            "test recovery completed".to_string(),
        ))
        .expect("test incident report finalizes");

    let exit_code = run_with_platform_and_managed_weathr_launcher(
        ["help"],
        &platform,
        &mut stdout,
        &mut stderr,
        &watchdog.process,
        weathr,
        |_options, _watchdog| Ok(()),
    );

    assert_eq!(exit_code, 0);
    let stderr = String::from_utf8(stderr).expect("watchdog route output is UTF-8");
    assert!(stderr.contains("WATCHDOG CRITICAL:"));
    assert!(stderr.contains("test recovery completed"));
    assert!(
        !platform
            .calls()
            .iter()
            .any(|call| matches!(call, MockCall::ShowCriticalError { .. }))
    );
}

#[test]
fn weathr_command_passes_setup_timezone_location_to_launcher() {
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let tree = TempTree::new("weathr-launch-timezone");
    let platform = mock_windows_platform(tree.path());
    let app_paths = platform.app_paths().expect("mock app paths");
    let opened = StorageManager::open(app_paths).expect("storage initializes");
    let config = StorageConfig {
        timezone: "Asia/Shanghai".to_string(),
        ..StorageConfig::default()
    };
    opened.manager.save_config(&config).expect("config saves");

    let mut captured = None;
    let exit_code = run_with_platform_and_weathr_launcher(
        ["weathr"],
        &platform,
        &mut stdout,
        &mut stderr,
        |options| {
            captured = Some(options);
            Ok::<(), &'static str>(())
        },
    );

    assert_eq!(exit_code, 0);
    assert!(stdout.is_empty());
    assert!(stderr.is_empty());

    let options = captured.expect("launcher receives options");
    assert_eq!(options.timezone_id.as_deref(), Some("Asia/Shanghai"));
    let location = options
        .location_override
        .expect("setup timezone should map to location");
    assert_eq!(location.latitude, 31.2304);
    assert_eq!(location.longitude, 121.4737);
    assert_eq!(location.city.as_deref(), Some("Shanghai"));
}

#[test]
fn weathr_command_uses_default_options_when_storage_config_is_missing() {
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let tree = TempTree::new("weathr-launch-no-config");
    let platform = mock_windows_platform(tree.path());

    let mut captured = None;
    let exit_code = run_with_platform_and_weathr_launcher(
        ["weathr"],
        &platform,
        &mut stdout,
        &mut stderr,
        |options| {
            captured = Some(options);
            Ok::<(), &'static str>(())
        },
    );

    assert_eq!(exit_code, 0);
    assert!(stdout.is_empty());
    assert!(stderr.is_empty());
    assert_eq!(captured, Some(tundra_weathr::LaunchOptions::default()));
}

#[test]
fn weathr_command_uses_default_options_when_storage_config_is_corrupt() {
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let tree = TempTree::new("weathr-launch-corrupt-config");
    let platform = mock_windows_platform(tree.path());
    let app_paths = platform.app_paths().expect("mock app paths");
    let config_parent = app_paths
        .config_path()
        .parent()
        .expect("config path has parent");
    fs::create_dir_all(config_parent).expect("config parent can be created");
    fs::write(app_paths.config_path(), b"schema_version =\n").expect("corrupt config fixture");

    let mut captured = None;
    let exit_code = run_with_platform_and_weathr_launcher(
        ["weathr"],
        &platform,
        &mut stdout,
        &mut stderr,
        |options| {
            captured = Some(options);
            Ok::<(), &'static str>(())
        },
    );

    assert_eq!(exit_code, 0);
    assert!(stdout.is_empty());
    assert!(stderr.is_empty());
    assert_eq!(captured, Some(tundra_weathr::LaunchOptions::default()));
}

#[test]
fn config_set_theme_updates_config_without_changing_users() {
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let tree = TempTree::new("config-set-theme");
    let platform = mock_windows_platform(tree.path());
    let app_paths = platform.app_paths().expect("mock app paths");
    let opened = StorageManager::open(app_paths).expect("storage initializes");
    let users_before = opened.manager.load_users().expect("users load");

    let exit_code = run_with_platform(
        ["config", "set", "theme", "light"],
        &platform,
        &mut stdout,
        &mut stderr,
    );

    assert_eq!(exit_code, 0);
    assert!(stderr.is_empty());
    let stdout = String::from_utf8(stdout).expect("config output should be utf8");
    assert!(stdout.contains("Updated theme: light"));
    assert_eq!(opened.manager.load_config().expect("config").theme, "light");
    assert_eq!(
        opened.manager.load_users().expect("users reload"),
        users_before
    );
}

#[test]
fn config_set_address_by_label_updates_timezone() {
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let tree = TempTree::new("config-set-address");
    let platform = mock_windows_platform(tree.path());
    let app_paths = platform.app_paths().expect("mock app paths");
    let opened = StorageManager::open(app_paths).expect("storage initializes");

    let exit_code = run_with_platform(
        ["config", "set", "address", "New", "York"],
        &platform,
        &mut stdout,
        &mut stderr,
    );

    assert_eq!(exit_code, 0);
    assert!(stderr.is_empty());
    let stdout = String::from_utf8(stdout).expect("config output should be utf8");
    assert!(stdout.contains("Updated address: New York (America/New_York"));
    assert_eq!(
        opened.manager.load_config().expect("config").timezone,
        "America/New_York"
    );
}

#[test]
fn config_get_address_prints_resolved_location() {
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let tree = TempTree::new("config-get-address");
    let platform = mock_windows_platform(tree.path());
    let app_paths = platform.app_paths().expect("mock app paths");
    let opened = StorageManager::open(app_paths).expect("storage initializes");
    let config = StorageConfig {
        timezone: "Asia/Shanghai".to_string(),
        ..StorageConfig::default()
    };
    opened.manager.save_config(&config).expect("config saves");

    let exit_code = run_with_platform(
        ["config", "get", "address"],
        &platform,
        &mut stdout,
        &mut stderr,
    );

    assert_eq!(exit_code, 0);
    assert!(stderr.is_empty());
    let stdout = String::from_utf8(stdout).expect("config output should be utf8");
    assert!(stdout.contains("address = Shanghai (Asia/Shanghai, 31.2304, 121.4737)"));
}

#[test]
fn config_set_password_is_rejected_and_leaves_users_unchanged() {
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let tree = TempTree::new("config-set-password-denied");
    let platform = mock_windows_platform(tree.path());
    let app_paths = platform.app_paths().expect("mock app paths");
    let opened = StorageManager::open(app_paths).expect("storage initializes");
    let users_before = opened.manager.load_users().expect("users load");

    let exit_code = run_with_platform(
        ["config", "set", "password", "new-password"],
        &platform,
        &mut stdout,
        &mut stderr,
    );

    assert_eq!(exit_code, 2);
    assert!(stdout.is_empty());
    let stderr = String::from_utf8(stderr).expect("config error output should be utf8");
    assert!(
        stderr.contains("username and password changes must use authenticated user management")
    );
    assert_eq!(
        opened.manager.load_users().expect("users reload"),
        users_before
    );
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
    let asset_root = copy_complete_assets(&tree);

    let exit_code = run_with_platform_and_asset_root(
        ["doctor"],
        &platform,
        &mut stdout,
        &mut stderr,
        &asset_root,
    );

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
    assert!(stdout.contains("Asset checks:"));
    assert!(stdout.contains("[PASS] Required ASCII assets (theme default):"));
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
fn doctor_command_warns_for_missing_ascii_asset_without_failing() {
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let tree = TempTree::new("doctor-missing-asset");
    let platform = mock_macos_platform(tree.path());
    let asset_root = copy_complete_assets(&tree);
    fs::remove_file(asset_root.join("themes/default/weathr/animation/cloud_0.txt"))
        .expect("missing asset fixture can be removed");

    let exit_code = run_with_platform_and_asset_root(
        ["doctor"],
        &platform,
        &mut stdout,
        &mut stderr,
        &asset_root,
    );

    assert_eq!(exit_code, 0);
    assert!(stderr.is_empty());
    let stdout = String::from_utf8(stdout).expect("doctor output should be utf8");
    assert!(stdout.contains("Asset checks:"));
    assert!(stdout.contains("[WARN] Required ASCII assets (theme default):"));
    assert!(stdout.contains("1 missing asset"));
    assert!(stdout.contains("missing: weathr/animation/cloud_0"));
    assert!(stdout.contains("Doctor result: PASS"));
}

#[test]
fn doctor_command_reports_checks_and_skips_storage_when_app_paths_fail() {
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let platform = UnsupportedPlatform;
    let tree = TempTree::new("doctor-unsupported-assets");
    let asset_root = copy_complete_assets(&tree);

    let exit_code = run_with_platform_and_asset_root(
        ["doctor"],
        &platform,
        &mut stdout,
        &mut stderr,
        &asset_root,
    );

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
    assert!(stdout.contains("Asset checks:"));
    assert!(stdout.contains("[PASS] Required ASCII assets (theme default):"));
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
    assert!(stderr.contains("Usage: tundra-cli <config|doctor|editor|explain|new|paths|weathr>"));
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

struct TestWeathrWatchdog {
    _tree: TempTree,
    _runtime: WatchdogRuntime,
    process: ProcessWatchdog,
    weathr: tundra_watchdog::AppWatchdog,
}

fn test_weathr_watchdog() -> std::sync::MutexGuard<'static, TestWeathrWatchdog> {
    static WATCHDOG: std::sync::OnceLock<std::sync::Mutex<TestWeathrWatchdog>> =
        std::sync::OnceLock::new();

    WATCHDOG
        .get_or_init(|| {
            let tree = TempTree::new("managed-weathr-watchdog");
            let root = tree.path().join("watchdog");
            let config = WatchdogConfig::new(
                root.join("crashes"),
                root.join("fallback"),
                root.join("state"),
                "tundra-cli-test",
                env!("CARGO_PKG_VERSION"),
            );
            let (runtime, process) = WatchdogRuntime::start(config).expect("test watchdog starts");
            let weathr = process
                .register_app(tundra_weathr::weathr_watchdog_descriptor())
                .expect("test Weathr app registers");
            std::sync::Mutex::new(TestWeathrWatchdog {
                _tree: tree,
                _runtime: runtime,
                process,
                weathr,
            })
        })
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
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

fn copy_complete_assets(tree: &TempTree) -> PathBuf {
    let out_dir = tree.path().join("target/debug/build/tundra-cli-test/out");
    fs::create_dir_all(&out_dir).expect("asset test OUT_DIR can be created");
    tundra_ascii_assets::copy_canonical_assets_to_profile_dir(&out_dir)
        .expect("canonical assets copy into temp profile dir")
}

impl Drop for TempTree {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}
