use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use cli::{
    AssetAction, AssetOutput, CliCommand, CliError, ConfigAction, ConfigField, ConfigUpdate,
    parse_args, run, run_with_platform, run_with_platform_and_asset_root,
    run_with_platform_and_watchdog,
};
use platform::mock::{MockCall, MockPlatform, UnsupportedPlatform};
use platform::{Platform, PlatformKind, UserDirs, build_macos_app_paths, build_windows_app_paths};
use storage::{BorderColor, BorderShape, StorageConfig, StorageLayout, StorageManager};
use watchdog::{
    BoundaryKind, BoundarySpec, ProcessWatchdog, RecoveryOutcome, WatchdogConfig, WatchdogRuntime,
};

#[test]
fn simple_commands_dispatch_from_a_table() {
    let cases: &[(&[&str], CliCommand)] = &[
        (&[], CliCommand::Help),
        (&["debug", "doctor"], CliCommand::Doctor),
        (&["debug", "paths"], CliCommand::Paths),
        (&["debug", "explain"], CliCommand::Explain),
        (&["new"], CliCommand::New),
        (&["debug"], CliCommand::DebugHelp),
        (&["debug", "help"], CliCommand::DebugHelp),
        (&["debug", "--help"], CliCommand::DebugHelp),
        (&["debug", "-h"], CliCommand::DebugHelp),
        (&["debug", "test-frost"], CliCommand::TestFrost),
        (&["debug", "test-matrix"], CliCommand::TestMatrix),
        (
            &["debug", "test-watchdog-error"],
            CliCommand::TestWatchdogError,
        ),
        (
            &["debug", "test-watchdog-critical"],
            CliCommand::TestWatchdogCritical,
        ),
        (
            &["debug", "test-watchdog-panic"],
            CliCommand::TestWatchdogPanic,
        ),
    ];

    for (args, expected) in cases {
        assert_eq!(
            parse_args(args.iter().copied()),
            Ok(expected.clone()),
            "unexpected dispatch for {args:?}"
        );
    }
}

#[test]
fn debug_commands_reject_old_entries_unknown_names_and_extra_arguments() {
    for name in [
        "asset",
        "doctor",
        "paths",
        "explain",
        "test-frost",
        "test-matrix",
        "weathr",
        "sudo",
    ] {
        assert_eq!(
            parse_args([name]),
            Err(CliError::UnknownCommand(name.to_string()))
        );
    }
    for name in ["weathr", "unknown", "new"] {
        assert_eq!(
            parse_args(["debug", name]),
            Err(CliError::UnknownDebugCommand(name.to_string()))
        );
    }
    for name in [
        "help",
        "test-frost",
        "test-matrix",
        "doctor",
        "paths",
        "explain",
        "test-watchdog-error",
        "test-watchdog-critical",
        "test-watchdog-panic",
    ] {
        assert_eq!(
            parse_args(["debug", name, "extra"]),
            Err(CliError::UnexpectedArgument("extra".to_string()))
        );
    }
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    assert_eq!(run(["weathr"], &mut stdout, &mut stderr), 2);
    assert!(String::from_utf8_lossy(&stderr).contains("unknown command: weathr"));
}

#[test]
fn debug_help_lists_diagnostics_and_report_tests() {
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    assert_eq!(run(["debug"], &mut stdout, &mut stderr), 0);
    let help = String::from_utf8(stdout).unwrap();
    for name in [
        "asset",
        "doctor",
        "paths",
        "explain",
        "test-frost",
        "test-matrix",
        "test-watchdog-error",
        "test-watchdog-critical",
        "test-watchdog-panic",
    ] {
        assert!(help.contains(name), "missing {name}");
    }
    assert!(stderr.is_empty());
    assert!(!help.contains("weathr"));
}

#[test]
fn watchdog_tests_write_real_reports_and_allow_the_next_command() {
    use watchdog::{IncidentKind, IncidentSeverity};
    let tree = TempTree::new("debug-watchdog");
    let platform = mock_windows_platform(tree.path());
    let watchdog = test_cli_watchdog();
    for (name, kind, severity) in [
        (
            "test-watchdog-error",
            IncidentKind::Error,
            IncidentSeverity::Error,
        ),
        (
            "test-watchdog-critical",
            IncidentKind::Error,
            IncidentSeverity::Critical,
        ),
    ] {
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        assert_eq!(
            run_with_platform_and_watchdog(
                ["debug", name],
                &platform,
                &mut stdout,
                &mut stderr,
                &watchdog.process,
                watchdog.cli.clone(),
            ),
            0,
            "{name}: {}",
            String::from_utf8_lossy(&stderr)
        );
        let output = String::from_utf8(stdout).unwrap();
        let catalog = watchdog.process.list_incident_reports();
        let report = catalog
            .reports
            .iter()
            .find(|report| output.contains(&report.incident_id))
            .expect("persisted report");
        assert_eq!(report.kind, kind);
        assert_eq!(report.severity, severity);
        assert_eq!(report.app.as_ref().unwrap().id.as_str(), "cli");
        assert!(report.component.as_ref().unwrap().contains("debug"));
        let json = fs::read_to_string(&report.json_report_path).unwrap();
        let text = fs::read_to_string(report.text_report_path.as_ref().unwrap()).unwrap();
        assert!(json.contains("Intentional"));
        assert!(text.contains("Intentional"));
        assert!(output.contains(&report.json_report_path.display().to_string()));
        assert!(
            output.contains(
                &report
                    .text_report_path
                    .as_ref()
                    .unwrap()
                    .display()
                    .to_string()
            )
        );
        if kind == IncidentKind::Panic {
            assert!(report.recovery.is_recovered());
        }
        assert!(output.contains("Command Line can continue"));
        let mut next_output = Vec::new();
        assert_eq!(
            run_with_platform_and_watchdog(
                ["cls"],
                &platform,
                &mut next_output,
                &mut stderr,
                &watchdog.process,
                watchdog.cli.clone(),
            ),
            0
        );
        assert_eq!(next_output, b"\x1b[3J\x1b[2J\x1b[H");
    }
}

#[test]
fn watchdog_tests_without_a_runtime_fail_clearly() {
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    assert_eq!(
        run(["debug", "test-watchdog-error"], &mut stdout, &mut stderr),
        1
    );
    assert!(stdout.is_empty());
    assert!(String::from_utf8_lossy(&stderr).contains("require the managed tundra-cli runtime"));
}

#[test]
fn watchdog_panic_test_unwinds_to_the_real_cli_boundary() {
    let watchdog = test_cli_watchdog();
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let caught = watchdog
        .cli
        .run_boundary(
            BoundarySpec::new("cli.command", BoundaryKind::Process),
            std::panic::AssertUnwindSafe(|| {
                run_with_platform_and_watchdog(
                    ["debug", "test-watchdog-panic"],
                    &UnsupportedPlatform,
                    &mut stdout,
                    &mut stderr,
                    &watchdog.process,
                    watchdog.cli.clone(),
                )
            }),
        )
        .expect_err("panic must escape the command dispatcher");
    assert!(caught.payload().contains("Intentional watchdog panic"));
    let receipt = caught
        .finalize(RecoveryOutcome::Unrecoverable(
            "CLI commands are never replayed after panic".into(),
        ))
        .unwrap();
    assert_eq!(receipt.kind, watchdog::IncidentKind::Panic);
    assert!(!receipt.recovery.is_recovered());
    assert!(receipt.text_report_path.unwrap().is_file());
    assert!(!String::from_utf8_lossy(&stdout).contains("Command Line can continue"));
    watchdog.process.drain_incidents();
}

#[test]
fn asset_args_select_help_rendered_source_and_named_item_output() {
    assert_eq!(
        parse_args(["debug", "asset"]),
        Ok(CliCommand::Asset(AssetAction::Help))
    );
    assert_eq!(
        parse_args(["debug", "asset", "--help"]),
        Ok(CliCommand::Asset(AssetAction::Help))
    );
    assert_eq!(
        parse_args(["debug", "asset", "explorer_icons"]),
        Ok(CliCommand::Asset(AssetAction::Show {
            name: "explorer_icons".to_string(),
            output: AssetOutput::RenderAll,
        }))
    );
    assert_eq!(
        parse_args(["debug", "asset", "explorer_icons", "-a"]),
        Ok(CliCommand::Asset(AssetAction::Show {
            name: "explorer_icons".to_string(),
            output: AssetOutput::Source,
        }))
    );
    assert_eq!(
        parse_args(["debug", "asset", "home_icons", "--launcher"]),
        Ok(CliCommand::Asset(AssetAction::Show {
            name: "home_icons".to_string(),
            output: AssetOutput::Item("launcher".to_string()),
        }))
    );
    assert_eq!(
        parse_args(["debug", "asset", "-a"]),
        Err(CliError::MissingArgument("asset name"))
    );
    assert_eq!(
        parse_args(["debug", "asset", "banner", "unexpected"]),
        Err(CliError::UnexpectedArgument("unexpected".to_string()))
    );
}

#[test]
fn cls_arg_dispatches_without_extra_arguments() {
    assert_eq!(parse_args(["cls"]), Ok(CliCommand::Cls));
    assert_eq!(
        parse_args(["cls", "extra"]),
        Err(CliError::UnexpectedArgument("extra".to_string()))
    );
}

#[test]
fn editor_command_is_not_a_shell_launch_bypass() {
    assert_eq!(
        parse_args(["editor"]),
        Err(CliError::UnknownCommand("editor".to_string()))
    );
}

#[test]
fn repl_arg_dispatches_with_an_internal_embedded_mode() {
    assert_eq!(
        parse_args(["repl"]),
        Ok(CliCommand::Repl { embedded: false })
    );
    assert_eq!(
        parse_args(["repl", "--embedded"]),
        Ok(CliCommand::Repl { embedded: true })
    );
    assert_eq!(
        parse_args(["repl", "--not-a-mode"]),
        Err(CliError::InvalidReplArgument("--not-a-mode".to_string()))
    );
}

#[test]
fn internal_update_commands_are_parsed_but_not_advertised() {
    assert_eq!(parse_args(["__update-probe"]), Ok(CliCommand::UpdateProbe));
    assert_eq!(
        parse_args(["__apply-update", "C:\\stage\\transaction.json", "42"]),
        Ok(CliCommand::ApplyUpdate {
            manifest: PathBuf::from("C:\\stage\\transaction.json"),
            parent_pid: 42,
            recover_only: false,
        })
    );
    assert_eq!(
        parse_args(["__recover-update", "C:\\stage\\transaction.json", "43"]),
        Ok(CliCommand::ApplyUpdate {
            manifest: PathBuf::from("C:\\stage\\transaction.json"),
            parent_pid: 43,
            recover_only: true,
        })
    );
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
        Err(CliError::ReadOnlyConfigField("theme".to_string()))
    );
    assert_eq!(
        parse_args(["config", "set", "border-shape", "square"]),
        Ok(CliCommand::Config(ConfigAction::Set(
            ConfigUpdate::BorderShape("square".to_string())
        )))
    );
    assert_eq!(
        parse_args(["config", "set", "border_color", "#aBc123"]),
        Ok(CliCommand::Config(ConfigAction::Set(
            ConfigUpdate::BorderColor("#aBc123".to_string())
        )))
    );
    assert_eq!(
        parse_args(["config", "set", "accent_color", "light-magenta"]),
        Ok(CliCommand::Config(ConfigAction::Set(
            ConfigUpdate::AccentColor("light-magenta".to_string())
        )))
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
fn unknown_and_extra_arguments_are_errors() {
    assert_eq!(
        parse_args(["repair"]),
        Err(CliError::UnknownCommand("repair".to_string()))
    );
    assert_eq!(
        parse_args(["debug", "doctor", "--json"]),
        Err(CliError::UnexpectedArgument("--json".to_string()))
    );
}

#[test]
fn help_command_writes_usage_to_stdout() {
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();

    let exit_code = run(["help"], &mut stdout, &mut stderr);

    assert_eq!(exit_code, 0, "{}", String::from_utf8_lossy(&stderr));
    assert!(stderr.is_empty());
    let stdout = String::from_utf8(stdout).expect("help output should be utf8");
    assert!(stdout.contains("<cls|config|debug|new|repl|help>"));
    assert!(stdout.contains("cls     Clear terminal history and screen"));
    assert!(stdout.contains("config  View or update user config"));
    assert!(stdout.contains("new     Clear saved TundraUX3 data"));
    assert!(!stdout.contains("Launch the shell directly"));
    assert!(!stdout.contains("Launch the terminal weather scene"));
    assert!(!stdout.contains("Windows 11"));
    assert!(!stdout.contains("Windows Terminal"));
}

#[test]
fn asset_without_a_name_prints_asset_specific_help() {
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();

    let exit_code = run(["debug", "asset"], &mut stdout, &mut stderr);

    assert_eq!(exit_code, 0);
    assert!(stderr.is_empty());
    let stdout = String::from_utf8(stdout).expect("asset help output should be utf8");
    assert!(stdout.contains("TundraUX3 asset test command"));
    assert!(stdout.contains("asset <name> -a"));
    assert!(stdout.contains("asset <name> --<item>"));
    assert!(stdout.contains("explorer_icons"));
    assert!(stdout.contains("weathr/world/house"));
}

#[test]
fn asset_command_renders_art_sets_and_individual_items() {
    let tree = TempTree::new("asset-render");
    let asset_root = copy_complete_assets(&tree);
    let platform = UnsupportedPlatform;

    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let exit_code = run_with_platform_and_asset_root(
        ["debug", "asset", "banner"],
        &platform,
        &mut stdout,
        &mut stderr,
        &asset_root,
    );
    assert_eq!(exit_code, 0, "{}", String::from_utf8_lossy(&stderr));
    assert!(stderr.is_empty());
    let banner = String::from_utf8(stdout).expect("banner output should be utf8");
    assert!(banner.contains("ooooooooooooo"));
    assert!(!banner.contains("schema_version"));
    assert!(!banner.contains("[items.tundraux3]"));

    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let exit_code = run_with_platform_and_asset_root(
        ["debug", "asset", "explorer_icons"],
        &platform,
        &mut stdout,
        &mut stderr,
        &asset_root,
    );
    assert_eq!(exit_code, 0);
    assert!(stderr.is_empty());
    let icons = String::from_utf8(stdout).expect("icon output should be utf8");
    assert!(icons.contains("--folder\n[+]\n"));
    assert!(icons.contains("--cancel\nx\n"));

    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let exit_code = run_with_platform_and_asset_root(
        ["debug", "asset", "home_icons", "--launcher"],
        &platform,
        &mut stdout,
        &mut stderr,
        &asset_root,
    );
    assert_eq!(exit_code, 0);
    assert!(stderr.is_empty());
    assert_eq!(
        String::from_utf8(stdout).expect("launcher icon output should be utf8"),
        "  / \\  \n /===\\ \n | o | \n /___\\ \n"
    );

    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let exit_code = run_with_platform_and_asset_root(
        ["debug", "asset", "launcher_icons", "--builtin.command-line"],
        &platform,
        &mut stdout,
        &mut stderr,
        &asset_root,
    );
    assert_eq!(exit_code, 0);
    assert!(stderr.is_empty());
    assert_eq!(
        String::from_utf8(stdout).expect("Command Line Launcher icon output should be utf8"),
        " ______ \n|cmd>  |\n|      |\n|______|\n"
    );

    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let exit_code = run_with_platform_and_asset_root(
        ["debug", "asset", "clock_font", "--0"],
        &platform,
        &mut stdout,
        &mut stderr,
        &asset_root,
    );
    assert_eq!(exit_code, 0);
    assert!(stderr.is_empty());
    assert_eq!(
        String::from_utf8(stdout).expect("clock glyph output should be utf8"),
        "  .oooo.\n d8P'`Y8b\n888    888\n888    888\n888    888\n`88b  d88'\n `Y8bd8P'\n"
    );
}

#[test]
fn asset_source_mode_prints_the_complete_original_file() {
    let tree = TempTree::new("asset-source");
    let asset_root = copy_complete_assets(&tree);
    let expected =
        fs::read(asset_root.join("themes/default/explorer_icons.toml")).expect("asset source");
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();

    let exit_code = run_with_platform_and_asset_root(
        ["debug", "asset", "explorer_icons", "-a"],
        &UnsupportedPlatform,
        &mut stdout,
        &mut stderr,
        &asset_root,
    );

    assert_eq!(exit_code, 0);
    assert!(stderr.is_empty());
    assert_eq!(stdout, expected);
}

#[test]
fn asset_command_supports_unique_file_names_and_reports_missing_values() {
    let tree = TempTree::new("asset-errors");
    let asset_root = copy_complete_assets(&tree);
    let expected =
        fs::read(asset_root.join("themes/default/weathr/world/house.txt")).expect("house asset");

    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let exit_code = run_with_platform_and_asset_root(
        ["debug", "asset", "house"],
        &UnsupportedPlatform,
        &mut stdout,
        &mut stderr,
        &asset_root,
    );
    assert_eq!(exit_code, 0);
    assert!(stderr.is_empty());
    assert_eq!(stdout, expected);

    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let exit_code = run_with_platform_and_asset_root(
        ["debug", "asset", "not_present"],
        &UnsupportedPlatform,
        &mut stdout,
        &mut stderr,
        &asset_root,
    );
    assert_eq!(exit_code, 1, "{}", String::from_utf8_lossy(&stderr));
    assert!(stdout.is_empty());
    let error = String::from_utf8(stderr).expect("unknown asset error should be utf8");
    assert!(error.contains("ERROR: unknown asset \"not_present\""));

    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let exit_code = run_with_platform_and_asset_root(
        ["debug", "asset", "home_icons", "--not_present"],
        &UnsupportedPlatform,
        &mut stdout,
        &mut stderr,
        &asset_root,
    );
    assert_eq!(exit_code, 1);
    assert!(stdout.is_empty());
    let error = String::from_utf8(stderr).expect("unknown item error should be utf8");
    assert!(
        error.contains("asset \"home_icons\" has no item \"not_present\""),
        "unexpected error: {error}"
    );

    fs::remove_file(asset_root.join("themes/default/banner.toml"))
        .expect("banner fixture can be removed");
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let exit_code = run_with_platform_and_asset_root(
        ["debug", "asset", "banner"],
        &UnsupportedPlatform,
        &mut stdout,
        &mut stderr,
        &asset_root,
    );
    assert_eq!(exit_code, 1);
    assert!(stdout.is_empty());
    let error = String::from_utf8(stderr).expect("missing file error should be utf8");
    assert!(
        error.contains("ERROR: could not read asset \"banner\""),
        "unexpected error: {error}"
    );
}

#[test]
fn cls_command_clears_history_and_screen_then_moves_the_cursor_home() {
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();

    let exit_code = run(["cls"], &mut stdout, &mut stderr);

    assert_eq!(exit_code, 0);
    assert_eq!(stdout, b"\x1b[3J\x1b[2J\x1b[H");
    assert!(stderr.is_empty());
}

#[test]
fn explain_command_prints_startup_and_boundary_notes() {
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();

    let exit_code = run(["debug", "explain"], &mut stdout, &mut stderr);

    assert_eq!(exit_code, 0);
    assert!(stderr.is_empty());
    let stdout = String::from_utf8(stdout).expect("explain output should be utf8");
    assert!(stdout.contains("Startup flow"));
    assert!(stdout.contains("tundra-cli"));
    assert!(stdout.contains("Kernel boundary"));
    assert!(stdout.contains("UI boundary"));
    assert!(stdout.contains("platform"));
    assert!(stdout.contains("tundra-shell"));
    assert!(stdout.contains("diagnostics and tests are under debug"));
    assert!(!stdout.contains("Windows 11"));
    assert!(!stdout.contains("Windows Terminal"));
}

#[test]
fn managed_cli_routes_pending_watchdog_incidents_for_regular_commands() {
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let tree = TempTree::new("managed-watchdog-drain");
    let platform = mock_windows_platform(tree.path());
    let watchdog = test_cli_watchdog();
    let cli = watchdog.cli.clone();
    let caught = cli
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

    let exit_code = run_with_platform_and_watchdog(
        ["help"],
        &platform,
        &mut stdout,
        &mut stderr,
        &watchdog.process,
        cli,
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
fn config_set_border_theme_fields_normalizes_and_preserves_users() {
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let tree = TempTree::new("config-set-border-theme-fields");
    let platform = mock_windows_platform(tree.path());
    let app_paths = platform.app_paths().expect("mock app paths");
    let opened = StorageManager::open(app_paths).expect("storage initializes");
    let users_before = opened.manager.load_users().expect("users load");

    let exit_code = run_with_platform(
        ["config", "set", "border-shape", "square"],
        &platform,
        &mut stdout,
        &mut stderr,
    );

    assert_eq!(exit_code, 0);
    assert!(stderr.is_empty());
    let stdout = String::from_utf8(stdout).expect("config output should be utf8");
    assert!(stdout.contains("Updated border shape: square"));
    assert_eq!(
        opened
            .manager
            .load_config()
            .expect("config")
            .appearance
            .border_shape,
        BorderShape::Square
    );

    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let exit_code = run_with_platform(
        ["config", "set", "border-color", "LiGhT-CyAn"],
        &platform,
        &mut stdout,
        &mut stderr,
    );

    assert_eq!(exit_code, 0);
    assert!(stderr.is_empty());
    assert_eq!(
        String::from_utf8(stdout).expect("config output should be utf8"),
        "Updated border color: light-cyan\n"
    );
    assert_eq!(
        opened
            .manager
            .load_config()
            .expect("config")
            .appearance
            .border_color,
        BorderColor::LightCyan
    );
    assert_eq!(
        opened.manager.load_users().expect("users reload"),
        users_before
    );
}

#[test]
fn config_border_color_accepts_hex_and_default_then_reports_canonical_values() {
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let tree = TempTree::new("config-border-color-canonical-values");
    let platform = mock_windows_platform(tree.path());
    let app_paths = platform.app_paths().expect("mock app paths");
    let opened = StorageManager::open(app_paths).expect("storage initializes");

    assert_eq!(
        run_with_platform(
            ["config", "set", "border-color", "#aBc123"],
            &platform,
            &mut stdout,
            &mut stderr,
        ),
        0
    );
    assert!(stderr.is_empty());
    assert_eq!(
        String::from_utf8(stdout).expect("config output should be utf8"),
        "Updated border color: #ABC123\n"
    );
    assert_eq!(
        opened
            .manager
            .load_config()
            .expect("config")
            .appearance
            .border_color,
        BorderColor::Rgb(0xAB, 0xC1, 0x23)
    );

    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    assert_eq!(
        run_with_platform(
            ["config", "set", "border-color", "default"],
            &platform,
            &mut stdout,
            &mut stderr,
        ),
        0
    );
    assert!(stderr.is_empty());
    assert_eq!(
        String::from_utf8(stdout).expect("config output should be utf8"),
        "Updated border color: white\n"
    );
    assert_eq!(
        opened
            .manager
            .load_config()
            .expect("config")
            .appearance
            .border_color,
        BorderColor::White
    );
}

#[test]
fn config_accent_color_accepts_hex_and_default_then_reports_canonical_values() {
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let tree = TempTree::new("config-accent-color-canonical-values");
    let platform = mock_windows_platform(tree.path());
    let app_paths = platform.app_paths().expect("mock app paths");
    let opened = StorageManager::open(app_paths).expect("storage initializes");

    assert_eq!(
        run_with_platform(
            ["config", "set", "accent-color", "#aBc123"],
            &platform,
            &mut stdout,
            &mut stderr,
        ),
        0
    );
    assert!(stderr.is_empty());
    assert_eq!(
        String::from_utf8(stdout).expect("config output should be utf8"),
        "Updated accent color: #ABC123\n"
    );
    assert_eq!(
        opened
            .manager
            .load_config()
            .expect("config")
            .appearance
            .accent_color,
        BorderColor::Rgb(0xAB, 0xC1, 0x23)
    );

    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    assert_eq!(
        run_with_platform(
            ["config", "set", "accent-color", "default"],
            &platform,
            &mut stdout,
            &mut stderr,
        ),
        0
    );
    assert!(stderr.is_empty());
    assert_eq!(
        String::from_utf8(stdout).expect("config output should be utf8"),
        "Updated accent color: cyan\n"
    );
    assert_eq!(
        opened
            .manager
            .load_config()
            .expect("config")
            .appearance
            .accent_color,
        BorderColor::Cyan
    );

    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    assert_eq!(
        run_with_platform(
            ["config", "set", "accent-color", "LiGhT-MaGeNtA"],
            &platform,
            &mut stdout,
            &mut stderr,
        ),
        0
    );
    assert!(stderr.is_empty());
    assert_eq!(
        String::from_utf8(stdout).expect("config output should be utf8"),
        "Updated accent color: light-magenta\n"
    );

    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    assert_eq!(
        run_with_platform(
            ["config", "get", "accent-color"],
            &platform,
            &mut stdout,
            &mut stderr,
        ),
        0
    );
    assert!(stderr.is_empty());
    assert_eq!(
        String::from_utf8(stdout).expect("config output should be utf8"),
        "accent-color = light-magenta\n"
    );
}

#[test]
fn config_get_theme_and_full_config_include_border_summary() {
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let tree = TempTree::new("config-get-theme-summary");
    let platform = mock_windows_platform(tree.path());

    assert_eq!(
        run_with_platform(
            ["config", "get", "theme"],
            &platform,
            &mut stdout,
            &mut stderr,
        ),
        0
    );
    assert!(stderr.is_empty());
    assert_eq!(
        String::from_utf8(stdout).expect("config output should be utf8"),
        "border-shape = rounded\nborder-color = #29434E\naccent-color = #63D3E5\n"
    );

    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    assert_eq!(
        run_with_platform(["config", "get"], &platform, &mut stdout, &mut stderr),
        0
    );
    assert!(stderr.is_empty());
    let stdout = String::from_utf8(stdout).expect("config output should be utf8");
    assert!(
        stdout.starts_with(
            "border-shape = rounded\nborder-color = #29434E\naccent-color = #63D3E5\n"
        )
    );
}

#[test]
fn config_rejects_invalid_border_values_without_writing_config() {
    let tree = TempTree::new("config-reject-invalid-border-values");
    let platform = mock_windows_platform(tree.path());
    let app_paths = platform.app_paths().expect("mock app paths");
    let opened = StorageManager::open(app_paths).expect("storage initializes");
    let config_before = fs::read(&opened.manager.layout().config_path).expect("config file reads");

    for args in [
        ["config", "set", "border-shape", "pill"],
        ["config", "set", "border-color", "#12345G"],
        ["config", "set", "accent-color", "#12345G"],
    ] {
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let exit_code = run_with_platform(args, &platform, &mut stdout, &mut stderr);

        assert_ne!(exit_code, 0);
        assert!(stdout.is_empty());
        assert!(!stderr.is_empty());
        assert_eq!(
            fs::read(&opened.manager.layout().config_path).expect("config file remains readable"),
            config_before
        );
    }
}

#[test]
fn config_set_theme_is_read_only_and_does_not_write_config() {
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let tree = TempTree::new("config-set-theme-read-only");
    let platform = mock_windows_platform(tree.path());
    let app_paths = platform.app_paths().expect("mock app paths");
    let opened = StorageManager::open(app_paths).expect("storage initializes");
    let config_before = fs::read(&opened.manager.layout().config_path).expect("config file reads");

    let exit_code = run_with_platform(
        ["config", "set", "theme", "light"],
        &platform,
        &mut stdout,
        &mut stderr,
    );

    assert_eq!(exit_code, 2);
    assert!(stdout.is_empty());
    let stderr = String::from_utf8(stderr).expect("config error should be utf8");
    assert!(stderr.contains("read-only summary"));
    assert!(stderr.contains("accent-color instead"));
    assert_eq!(
        fs::read(&opened.manager.layout().config_path).expect("config file remains readable"),
        config_before
    );
}

#[test]
fn config_set_rejects_non_english_language() {
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let tree = TempTree::new("config-set-non-english");
    let platform = mock_windows_platform(tree.path());
    let app_paths = platform.app_paths().expect("mock app paths");
    let opened = StorageManager::open(app_paths).expect("storage initializes");

    let exit_code = run_with_platform(
        ["config", "set", "language", "zh-Hans"],
        &platform,
        &mut stdout,
        &mut stderr,
    );

    assert_ne!(exit_code, 0);
    assert!(stdout.is_empty());
    let stderr = String::from_utf8(stderr).expect("config error should be utf8");
    assert!(stderr.contains("unsupported language"));
    assert!(stderr.contains("available values: en-US"));
    assert_eq!(
        opened.manager.load_config().expect("config").language,
        "en-US"
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
    fs::write(layout.logs_path.join("application.log"), "log").expect("log fixture");
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
    assert!(!layout.logs_path.join("application.log").exists());
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

    let exit_code = run_with_platform(["debug", "paths"], &platform, &mut stdout, &mut stderr);

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

    let exit_code = run_with_platform(["debug", "paths"], &platform, &mut stdout, &mut stderr);

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

    let exit_code = run_with_platform(["debug", "paths"], &platform, &mut stdout, &mut stderr);

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
        ["debug", "doctor"],
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
    assert_eq!(stdout.matches("] Terminal:").count(), 1);
    assert!(!stdout.contains("Terminal image protocol"));
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
        ["debug", "doctor"],
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
        ["debug", "doctor"],
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
    assert!(stderr.contains("<cls|config|debug|new|repl|help>"));
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

struct TestCliWatchdog {
    _tree: TempTree,
    _runtime: WatchdogRuntime,
    process: ProcessWatchdog,
    cli: watchdog::AppWatchdog,
}

fn test_cli_watchdog() -> std::sync::MutexGuard<'static, TestCliWatchdog> {
    static WATCHDOG: std::sync::OnceLock<std::sync::Mutex<TestCliWatchdog>> =
        std::sync::OnceLock::new();

    WATCHDOG
        .get_or_init(|| {
            let tree = TempTree::new("managed-cli-watchdog");
            let root = tree.path().join("watchdog");
            let config = WatchdogConfig::new(
                root.join("crashes"),
                root.join("fallback"),
                root.join("state"),
                "tundra-cli-test",
                env!("CARGO_PKG_VERSION"),
            );
            let (runtime, process) = WatchdogRuntime::start(config).expect("test watchdog starts");
            let process = process
                .install_global()
                .expect("test watchdog hook installs");
            let cli = process
                .register_app(watchdog::AppDescriptor::new(
                    watchdog::AppId::from_static("cli"),
                    "Tundra CLI test",
                    env!("CARGO_PKG_VERSION"),
                    watchdog::AppCriticality::ProcessCritical,
                ))
                .expect("test CLI app registers");
            std::sync::Mutex::new(TestCliWatchdog {
                _tree: tree,
                _runtime: runtime,
                process,
                cli,
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
    ascii_assets::copy_canonical_assets_to_profile_dir(&out_dir)
        .expect("canonical assets copy into temp profile dir")
}

impl Drop for TempTree {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}
