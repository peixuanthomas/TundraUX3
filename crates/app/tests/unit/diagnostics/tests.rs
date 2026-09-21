use super::*;
use platform::{AppPaths, PlatformKind, UserDirs};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use watchdog::{WatchdogConfig, WatchdogRuntime};

fn diagnostic_test_path(prefix: &str) -> PathBuf {
    let temp_root = std::env::temp_dir()
        .canonicalize()
        .expect("temporary directory should resolve to a link-free path");
    temp_root.join(format!(
        "{prefix}-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ))
}

#[test]
fn overall_status_and_repair_plan_are_deterministic() {
    let snapshot = DiagnosticsSnapshot {
        scanned_at: Utc::now(),
        checks: vec![
            DiagnosticCheck {
                id: "storage.state".to_string(),
                category: DiagnosticCategory::Storage,
                label: "State".to_string(),
                status: DiagnosticStatus::Fail,
                summary: "bad".to_string(),
                detail: "bad".to_string(),
                remediation: None,
                repair: Some(DiagnosticsRepairAction::RepairStorageDocument(
                    StorageDocumentKind::State,
                )),
            },
            DiagnosticCheck {
                id: "path.data".to_string(),
                category: DiagnosticCategory::Paths,
                label: "Data".to_string(),
                status: DiagnosticStatus::Warning,
                summary: "missing".to_string(),
                detail: "missing".to_string(),
                remediation: None,
                repair: Some(DiagnosticsRepairAction::CreateDirectory {
                    label: "Data".to_string(),
                    path: PathBuf::from("z-data"),
                }),
            },
        ],
        incidents: Vec::new(),
        logs: Vec::new(),
        warnings: Vec::new(),
    };

    assert_eq!(snapshot.overall_status(), DiagnosticStatus::Fail);
    let plan = snapshot.repair_plan();
    assert!(matches!(
        plan.first(),
        Some(DiagnosticsRepairAction::CreateDirectory { .. })
    ));
    assert!(matches!(
        plan.last(),
        Some(DiagnosticsRepairAction::RepairStorageDocument(_))
    ));
}

#[test]
fn unsupported_capability_is_counted_without_becoming_a_warning() {
    let snapshot = DiagnosticsSnapshot {
        scanned_at: Utc::now(),
        checks: vec![DiagnosticCheck {
            id: "environment.terminal".to_string(),
            category: DiagnosticCategory::Environment,
            label: "Terminal".to_string(),
            status: DiagnosticStatus::Unsupported,
            summary: "unsupported".to_string(),
            detail: "unsupported".to_string(),
            remediation: None,
            repair: None,
        }],
        incidents: Vec::new(),
        logs: Vec::new(),
        warnings: Vec::new(),
    };

    assert_eq!(snapshot.overall_status(), DiagnosticStatus::Unsupported);
    assert_eq!(snapshot.status_counts(), (0, 1, 0, 0));
}

#[test]
fn asset_diagnostics_validate_the_runtime_theme_instead_of_the_ui_palette() {
    let checks = diagnostic_asset_checks(Path::new(ascii_assets::CANONICAL_ASSETS_DIR));
    let runtime_theme_path = Path::new("themes")
        .join(ascii_assets::DEFAULT_THEME_ID)
        .display()
        .to_string();

    assert_eq!(checks.len(), ascii_assets::default_theme_files().len());
    assert!(
        checks
            .iter()
            .all(|check| check.status == DiagnosticStatus::Pass)
    );
    assert!(
        checks
            .iter()
            .all(|check| check.detail.contains(&runtime_theme_path))
    );
    assert!(checks.iter().all(|check| check.repair.is_none()));
}

#[test]
fn asset_diagnostics_offer_embedded_repairs_when_the_asset_root_is_missing() {
    let root = diagnostic_test_path("tundra-missing-default-assets");
    let checks = diagnostic_asset_checks(&root);

    assert_eq!(checks.len(), ascii_assets::default_theme_files().len());
    assert!(
        checks
            .iter()
            .all(|check| check.status == DiagnosticStatus::Warning)
    );
    assert!(checks.iter().all(|check| {
        matches!(
            &check.repair,
            Some(DiagnosticsRepairAction::RestoreDefaultThemeFile {
                root: repair_root,
                file_key,
            }) if repair_root == &root && file_key == &check.label
        )
    }));

    let action = checks
        .iter()
        .find(|check| check.label == "banner")
        .and_then(|check| check.repair.clone())
        .expect("missing banner should be repairable");
    let image_action = checks
        .iter()
        .find(|check| check.label == "home_icons/explorer.png")
        .and_then(|check| check.repair.clone())
        .expect("missing default theme image should be repairable");
    let app_paths = AppPaths::from_parts(
        root.join("app/config/config.toml"),
        root.join("app/data"),
        root.join("app/cache"),
        root.join("app/logs"),
        root.join("app/temp"),
    )
    .expect("test app paths");
    let storage = StorageManager::open(app_paths.clone())
        .expect("test storage")
        .manager;
    let user_dirs = UserDirs::new(
        root.join("Desktop"),
        root.join("Documents"),
        root.join("Downloads"),
        root.join("Pictures"),
        root.join("Movies"),
        root.join("Music"),
        root.join("AppData"),
    )
    .expect("test user directories");
    let platform =
        platform::mock::MockPlatform::new(user_dirs, app_paths).with_kind(PlatformKind::Macos);

    let result = execute_repair(&platform, &storage, action);
    let image_result = execute_repair(&platform, &storage, image_action);

    assert!(result.success);
    assert!(result.changed);
    assert!(image_result.success);
    assert!(image_result.changed);
    assert!(matches!(
        result.action,
        DiagnosticsRepairAction::RestoreDefaultThemeFile { ref file_key, .. }
            if file_key == "banner"
    ));
    assert!(
        diagnostic_asset_checks(&root)
            .iter()
            .find(|check| check.label == "banner")
            .is_some_and(|check| check.status == DiagnosticStatus::Pass)
    );
    assert!(matches!(
        image_result.action,
        DiagnosticsRepairAction::RestoreDefaultThemeFile { ref file_key, .. }
            if file_key == "home_icons/explorer.png"
    ));
    assert!(
        diagnostic_asset_checks(&root)
            .iter()
            .find(|check| check.label == "home_icons/explorer.png")
            .is_some_and(|check| check.status == DiagnosticStatus::Pass)
    );

    std::fs::remove_dir_all(root).expect("fixture cleanup");
}

#[test]
fn diagnostic_log_scan_recurses_sorts_and_excludes_crashes() {
    let root = diagnostic_test_path("tundra-diagnostic-logs");
    std::fs::create_dir_all(root.join("nested")).expect("nested fixture directory");
    std::fs::create_dir_all(root.join("Crashes")).expect("crash fixture directory");
    std::fs::write(root.join("older.log"), b"old").expect("older log fixture");
    std::thread::sleep(Duration::from_millis(20));
    std::fs::write(root.join("nested/newer.LOG.1"), b"new").expect("newer log fixture");
    std::fs::write(root.join("nested/ignore.txt"), b"ignore").expect("non-log fixture");
    std::fs::write(root.join("Crashes/panic.log"), b"crash").expect("crash log fixture");

    let (logs, warnings) = scan_diagnostic_logs(&root);

    assert!(warnings.is_empty());
    assert_eq!(
        logs.iter()
            .map(|log| log.relative_path.clone())
            .collect::<Vec<_>>(),
        vec![
            PathBuf::from("nested/newer.LOG.1"),
            PathBuf::from("older.log")
        ]
    );
    assert_eq!(logs[0].size_bytes, 3);
    assert!(logs.iter().all(|log| log.path.starts_with(&root)));

    std::fs::remove_dir_all(root).expect("fixture cleanup");
}

#[test]
fn diagnostic_log_scan_reports_unreadable_root() {
    let root = diagnostic_test_path("tundra-diagnostic-log-root-file");
    std::fs::write(&root, b"not a directory").expect("root file fixture");

    let (logs, warnings) = scan_diagnostic_logs(&root);

    assert!(logs.is_empty());
    assert_eq!(warnings.len(), 1);
    assert!(warnings[0].contains("Log history: could not read diagnostic log directory"));
    std::fs::remove_file(root).expect("fixture cleanup");
}

#[test]
fn diagnostic_log_scan_treats_a_missing_root_as_empty() {
    let root = diagnostic_test_path("tundra-diagnostic-log-missing");

    let (logs, warnings) = scan_diagnostic_logs(&root);

    assert!(logs.is_empty());
    assert!(warnings.is_empty());
}

#[cfg(unix)]
#[test]
fn diagnostic_log_scan_does_not_follow_symbolic_links() {
    use std::os::unix::fs::symlink;

    let root = diagnostic_test_path("tundra-diagnostic-log-link");
    let outside = root.with_extension("outside");
    std::fs::create_dir_all(&root).expect("log root fixture");
    std::fs::create_dir_all(&outside).expect("outside fixture");
    std::fs::write(outside.join("secret.log"), b"secret").expect("outside log fixture");
    symlink(&outside, root.join("linked")).expect("directory symlink fixture");

    let (logs, warnings) = scan_diagnostic_logs(&root);

    assert!(logs.is_empty());
    assert!(
        warnings
            .iter()
            .all(|warning| warning.starts_with("Log history:"))
    );
    std::fs::remove_dir_all(root).expect("root cleanup");
    std::fs::remove_dir_all(outside).expect("outside cleanup");
}

#[test]
fn terminal_event_keeps_runtime_busy_until_drain_consumes_it() {
    let (command_tx, command_rx) = mpsc::channel();
    let (event_tx, event_rx) = mpsc::channel();
    let runtime = DiagnosticsTaskRuntime {
        command_tx,
        event_rx,
        busy: Arc::new(AtomicBool::new(true)),
        restart_required: Arc::new(AtomicBool::new(false)),
        worker: None,
    };
    event_tx
        .send(DiagnosticsTaskEvent::ScanCompleted(Err(
            "expected test result".to_string(),
        )))
        .expect("terminal event should queue");

    assert!(runtime.is_busy());
    assert_eq!(runtime.request_scan(), Err(DiagnosticsTaskError::Busy));

    let events = runtime.drain_events();
    assert!(matches!(
        events.as_slice(),
        [DiagnosticsTaskEvent::ScanCompleted(Err(message))]
            if message == "expected test result"
    ));
    assert!(!runtime.is_busy());

    runtime
        .request_scan()
        .expect("a new scan should be accepted after consuming the terminal event");
    assert!(matches!(command_rx.try_recv(), Ok(WorkerCommand::Scan)));
}

#[test]
fn managed_runtime_scans_and_latches_after_changed_storage_repair() {
    let root = diagnostic_test_path("tundra-diagnostics-runtime");
    let paths = AppPaths::from_parts(
        root.join("config/config.toml"),
        root.join("data"),
        root.join("cache"),
        root.join("logs"),
        root.join("temp"),
    )
    .expect("test paths");
    let layout = storage::StorageLayout::from_app_paths(&paths);
    let storage = StorageManager::open(paths.clone())
        .expect("test storage")
        .manager;
    let user_dirs = UserDirs::new(
        root.join("Desktop"),
        root.join("Documents"),
        root.join("Downloads"),
        root.join("Pictures"),
        root.join("Movies"),
        root.join("Music"),
        root.join("AppData"),
    )
    .expect("test user directories");
    let platform: Arc<dyn Platform> = Arc::new(
        platform::mock::MockPlatform::new(user_dirs, paths).with_kind(PlatformKind::Macos),
    );
    let config = WatchdogConfig::new(
        root.join("watchdog/reports"),
        root.join("watchdog/fallback"),
        root.join("watchdog/data"),
        "diagnostics-test",
        "1.0.0",
    );
    let (watchdog_runtime, process) = WatchdogRuntime::start(config).expect("watchdog");
    let app = process
        .register_app(diagnostics_watchdog_descriptor())
        .expect("diagnostics app");
    let runtime = DiagnosticsTaskRuntime::new_managed(platform, storage, process, app)
        .expect("managed runtime");

    runtime.request_scan().expect("scan accepted");
    let mut result = None;
    for _ in 0..200 {
        if let Some(event) = runtime.drain_events().into_iter().next() {
            result = Some(event);
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    let DiagnosticsTaskEvent::ScanCompleted(Ok(snapshot)) = result.expect("scan event arrives")
    else {
        panic!("scan should complete successfully");
    };
    assert!(!snapshot.checks.is_empty());
    assert!(snapshot.checks.iter().any(|check| {
        check.category == DiagnosticCategory::Storage && check.status == DiagnosticStatus::Pass
    }));

    std::fs::remove_file(&layout.state_path).expect("state fixture should be removable");
    runtime
        .request_repair(vec![DiagnosticsRepairAction::RepairStorageDocument(
            StorageDocumentKind::State,
        )])
        .expect("storage repair accepted");
    for _ in 0..200 {
        if runtime.restart_required() {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }

    assert!(runtime.restart_required());
    assert!(runtime.is_busy());
    assert_eq!(
        runtime.request_scan(),
        Err(DiagnosticsTaskError::RestartRequired)
    );
    assert_eq!(
        runtime.request_repair(Vec::new()),
        Err(DiagnosticsTaskError::RestartRequired)
    );

    let mut completion_results = None;
    for _ in 0..200 {
        for event in runtime.drain_events() {
            if let DiagnosticsTaskEvent::RepairCompleted {
                results,
                restart_required: true,
                ..
            } = event
            {
                completion_results = Some(results);
            }
        }
        if completion_results.is_some() {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    let completion_results = completion_results.expect("repair completion should arrive");
    assert!(matches!(
        completion_results.as_slice(),
        [DiagnosticsRepairResult {
            action: DiagnosticsRepairAction::RepairStorageDocument(StorageDocumentKind::State),
            success: true,
            changed: true,
            ..
        }]
    ));
    assert!(!runtime.is_busy());
    assert!(runtime.restart_required());
    assert!(layout.state_path.is_file());
    assert_eq!(
        runtime.request_scan(),
        Err(DiagnosticsTaskError::RestartRequired)
    );

    drop(runtime);
    watchdog_runtime.shutdown().expect("watchdog shutdown");
    let _ = std::fs::remove_dir_all(root);
}
