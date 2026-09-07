use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

use platform::mock::UnsupportedPlatform;
use watchdog::{AppCriticality, AppDescriptor, AppId, WatchdogConfig, WatchdogRuntime};

#[test]
fn watchdog_debug_reports_use_fallback_and_fail_when_both_directories_are_blocked() {
    let root = std::env::temp_dir().join(format!(
        "tundra-cli-debug-report-test-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&root).unwrap();
    let primary = root.join("blocked-primary");
    let fallback = root.join("fallback");
    fs::write(&primary, "a file cannot contain reports").unwrap();
    let (runtime, process) = WatchdogRuntime::start(WatchdogConfig::new(
        &primary,
        &fallback,
        root.join("state"),
        "cli-debug-report-test",
        "test",
    ))
    .unwrap();
    let app = process
        .register_app(AppDescriptor::new(
            AppId::from_static("cli"),
            "CLI test",
            "test",
            AppCriticality::ProcessCritical,
        ))
        .unwrap();
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    assert_eq!(
        cli::run_with_platform_and_watchdog(
            ["debug", "test-watchdog-error"],
            &UnsupportedPlatform,
            &mut stdout,
            &mut stderr,
            &process,
            app.clone(),
        ),
        0,
        "{}",
        String::from_utf8_lossy(&stderr)
    );
    let reports = process.list_incident_reports().reports;
    assert_eq!(reports.len(), 1);
    assert!(reports[0].json_report_path.starts_with(&fallback));
    assert!(reports[0].text_report_path.as_ref().unwrap().is_file());

    // Keep the generated fallback reports while replacing their directory with
    // a file, so neither configured destination can accept the next report.
    fs::rename(&fallback, root.join("saved-reports")).unwrap();
    fs::write(&fallback, "a file cannot contain reports").unwrap();
    for command in ["test-watchdog-error", "test-watchdog-critical"] {
        stdout.clear();
        stderr.clear();
        assert_eq!(
            cli::run_with_platform_and_watchdog(
                ["debug", command],
                &UnsupportedPlatform,
                &mut stdout,
                &mut stderr,
                &process,
                app.clone(),
            ),
            1
        );
        assert!(!String::from_utf8_lossy(&stdout).contains("test completed"));
        assert!(String::from_utf8_lossy(&stderr).contains("ERROR:"));
    }
    runtime.shutdown().unwrap();
    fs::remove_dir_all(root).unwrap();
}
