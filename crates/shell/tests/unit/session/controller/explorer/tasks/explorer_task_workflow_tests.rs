use super::*;

#[test]
fn explorer_task_error_detail_reports_the_failed_file_and_cause() {
    use app::explorer_tasks::{ExplorerItemFailure, ExplorerTaskError};
    let failures = [ExplorerItemFailure {
        source: PathBuf::from("/home/user/Documents/alpha.txt"),
        target: None,
        error: ExplorerTaskError::Platform(platform::PlatformError::InvalidInput {
            message: "Linux Trash ownership mismatch".into(),
        }),
    }];
    let detail = explorer_task_error_detail(None, &failures)
        .unwrap()
        .render_current();
    assert!(detail.contains("/home/user/Documents/alpha.txt"));
    assert!(detail.contains("Linux Trash ownership mismatch"));
    let fatal = ExplorerTaskError::Journal {
        message: "journal unavailable".into(),
    };
    assert_eq!(
        explorer_task_error_detail(Some(&fatal), &failures)
            .unwrap()
            .render_current(),
        fatal.to_string()
    );
    assert!(explorer_task_error_detail(None, &[]).is_none());
}

fn test_explorer_watchdog() -> watchdog::AppWatchdog {
    static WATCHDOG: std::sync::OnceLock<watchdog::AppWatchdog> = std::sync::OnceLock::new();
    WATCHDOG
        .get_or_init(|| {
            let _ = default_editor_watchdog();
            if let Some(process) = watchdog::ProcessWatchdog::global() {
                return process
                    .register_app(app::explorer_tasks::explorer_watchdog_descriptor())
                    .expect("Explorer workflow watchdog registration");
            }
            let root = std::env::temp_dir().join(format!(
                "tundra-shell-explorer-watchdog-tests-{}",
                std::process::id()
            ));
            let config = watchdog::WatchdogConfig::new(
                root.join("reports"),
                root.join("fallback"),
                root.join("state"),
                "tundra-shell-tests",
                env!("CARGO_PKG_VERSION"),
            );
            let (runtime, process) =
                watchdog::WatchdogRuntime::start(config).expect("Explorer workflow test watchdog");
            let process = process
                .install_global()
                .expect("install Explorer workflow test watchdog");
            let _runtime = Box::leak(Box::new(runtime));
            process
                .register_app(app::explorer_tasks::explorer_watchdog_descriptor())
                .expect("Explorer workflow watchdog registration")
        })
        .clone()
}

#[test]
fn shell_runtime_executes_copy_on_real_temporary_paths() {
    let fixture = std::env::temp_dir().join(format!(
        "tundra-shell-explorer-task-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    let source_root = fixture.join("source");
    let destination = fixture.join("destination");
    std::fs::create_dir_all(&source_root).expect("source directory");
    std::fs::create_dir_all(&destination).expect("destination directory");
    let source = source_root.join("note.txt");
    std::fs::write(&source, b"background copy").expect("source file");

    let storage = storage_at(&fixture);
    let runtime = ShellExplorerTaskRuntime::new_managed(storage, test_explorer_watchdog());
    let plan =
        ExplorerTransferPlan::new(ExplorerTransferOperation::Copy, vec![source], &destination);
    runtime
        .submit(
            ExplorerTaskPlan::Transfer(plan),
            ShellExplorerTaskKind::Copy,
            "Tester".to_string(),
        )
        .expect("task accepted");

    let summary = wait_for_summary(&runtime);

    assert_eq!(summary.succeeded_items, 1);
    assert_eq!(summary.failed_items, 0);
    assert_eq!(
        std::fs::read(destination.join("note.txt")).expect("copied file"),
        b"background copy"
    );
    drop(runtime);
    let _ = std::fs::remove_dir_all(fixture);
}

#[cfg(any())]
#[test]
fn storage_trash_adapter_indexes_background_delete_once() {
    let fixture = std::env::temp_dir().join(format!(
        "tundra-shell-explorer-trash-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    let source_root = fixture.join("source");
    std::fs::create_dir_all(&source_root).expect("source directory");
    let source = source_root.join("obsolete.txt");
    std::fs::write(&source, b"trash me").expect("source file");

    let storage = storage_at(&fixture);
    let runtime = ShellExplorerTaskRuntime::new_managed(storage.clone(), test_explorer_watchdog());
    runtime
        .submit(
            ExplorerTaskPlan::DeleteToTrash(ExplorerDeletePlan::new(vec![source.clone()])),
            ShellExplorerTaskKind::Delete,
            "TestActor".to_string(),
        )
        .expect("task accepted");
    let summary = wait_for_summary(&runtime);

    assert_eq!(summary.succeeded_items, 1);
    assert!(!source.exists());
    let trash = storage.load_trash().expect("trash manifest");
    assert_eq!(trash.records.len(), 1);
    assert_eq!(
        trash.records[0].original_path.file_name(),
        source.file_name()
    );
    assert_eq!(trash.records[0].actor, "TestActor");
    assert!(trash.records[0].trash_path.exists());

    drop(runtime);
    let _ = std::fs::remove_dir_all(fixture);
}

#[cfg(any())]
#[test]
fn replacement_is_indexed_in_storage_trash() {
    let fixture = std::env::temp_dir().join(format!(
        "tundra-shell-explorer-replace-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    let source_root = fixture.join("source");
    let destination = fixture.join("destination");
    std::fs::create_dir_all(&source_root).expect("source directory");
    std::fs::create_dir_all(&destination).expect("destination directory");
    let source = source_root.join("same.txt");
    let replaced = destination.join("same.txt");
    std::fs::write(&source, b"new contents").expect("source file");
    std::fs::write(&replaced, b"old contents").expect("destination file");

    let storage = storage_at(&fixture);
    let runtime = ShellExplorerTaskRuntime::new_managed(storage.clone(), test_explorer_watchdog());
    let mut plan =
        ExplorerTransferPlan::new(ExplorerTransferOperation::Copy, vec![source], &destination);
    plan.collisions = ExplorerCollisionPolicy::replace();
    runtime
        .submit(
            ExplorerTaskPlan::Transfer(plan),
            ShellExplorerTaskKind::Copy,
            "ReplaceActor".to_string(),
        )
        .expect("task accepted");
    let summary = wait_for_summary(&runtime);

    assert_eq!(summary.succeeded_items, 1);
    assert_eq!(
        std::fs::read(&replaced).expect("replacement"),
        b"new contents"
    );
    let trash = storage.load_trash().expect("trash manifest");
    assert_eq!(trash.records.len(), 1);
    assert_eq!(trash.records[0].actor, "ReplaceActor");
    assert_eq!(
        std::fs::read(&trash.records[0].trash_path).expect("replaced file in trash"),
        b"old contents"
    );

    drop(runtime);
    let _ = std::fs::remove_dir_all(fixture);
}

#[cfg(any())]
#[test]
fn trash_manifest_failure_rolls_back_filesystem_move() {
    let fixture = std::env::temp_dir().join(format!(
        "tundra-shell-explorer-trash-rollback-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    let source_root = fixture.join("source");
    std::fs::create_dir_all(&source_root).expect("source directory");
    let source = source_root.join("preserved.txt");
    std::fs::write(&source, b"preserve me").expect("source file");
    let storage = storage_at(&fixture);
    std::fs::write(&storage.layout().trash_manifest_path, b"not-json")
        .expect("corrupt trash manifest");
    let actor = Arc::new(Mutex::new("RollbackActor".to_string()));
    let adapter = StorageExplorerTrash::new(storage, actor);
    let platform = platform::native_platform();

    let error = adapter
        .move_to_trash(platform.as_ref(), &source)
        .expect_err("manifest failure should fail the trash operation");

    assert!(error.to_string().contains("rolled back"));
    assert_eq!(
        std::fs::read(&source).expect("restored source"),
        b"preserve me"
    );
    let _ = std::fs::remove_dir_all(fixture);
}

#[cfg(any())]
#[test]
fn cross_volume_trash_move_commits_copy_before_removing_source() {
    let fixture = std::env::temp_dir().join(format!(
        "tundra-shell-explorer-trash-cross-volume-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    let source_root = fixture.join("source");
    let target_root = fixture.join("target");
    std::fs::create_dir_all(&source_root).expect("source directory");
    std::fs::create_dir_all(&target_root).expect("target directory");
    let source = source_root.join("cross-volume.txt");
    let target = target_root.join("cross-volume.txt");
    std::fs::write(&source, b"cross volume").expect("source file");
    let user_dirs = platform::UserDirs::new(
        fixture.join("Desktop"),
        fixture.join("Documents"),
        fixture.join("Downloads"),
        fixture.join("Pictures"),
        fixture.join("Videos"),
        fixture.join("Music"),
        fixture.join("AppData"),
    )
    .expect("absolute user dirs");
    let platform = platform::mock::MockPlatform::new(user_dirs, app_paths_at(&fixture));
    platform.set_cross_device_rename(source.clone(), target.clone(), "simulated different volume");

    move_to_trash_path(&platform, &source, &target).expect("cross-volume fallback");

    assert!(!source.exists());
    assert_eq!(
        std::fs::read(&target).expect("committed trash target"),
        b"cross volume"
    );
    assert!(
        std::fs::read_dir(&target_root)
            .expect("target listing")
            .all(|entry| !entry
                .expect("target entry")
                .file_name()
                .to_string_lossy()
                .contains("tundra-trash-stage"))
    );
    let _ = std::fs::remove_dir_all(fixture);
}

#[cfg(any())]
#[test]
fn reconciliation_removes_manifest_record_restored_by_engine_rollback() {
    let fixture = std::env::temp_dir().join(format!(
        "tundra-shell-explorer-trash-reconcile-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    let storage = storage_at(&fixture);
    let original = fixture.join("destination").join("restored.txt");
    std::fs::create_dir_all(original.parent().expect("destination parent"))
        .expect("destination directory");
    std::fs::write(&original, b"restored").expect("restored destination");
    let missing_trash_path = storage.layout().trash_path.join("rolled-back.txt");
    let mut trash = storage.load_trash().expect("trash manifest");
    trash.records.push(TrashRecord {
        original_path: original,
        trash_path: missing_trash_path,
        actor: "TestActor".to_string(),
        timestamp_epoch_ms: explorer_unix_millis(),
    });
    storage.save_trash(&trash).expect("save stale record");

    reconcile_storage_trash_manifest(&storage).expect("reconcile trash manifest");

    assert!(
        storage
            .load_trash()
            .expect("reconciled manifest")
            .records
            .is_empty()
    );
    let _ = std::fs::remove_dir_all(fixture);
}

#[test]
fn recursive_conflict_preflight_reports_only_non_merge_targets() {
    let fixture = std::env::temp_dir().join(format!(
        "tundra-shell-explorer-conflicts-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    let source = fixture.join("source").join("folder");
    let destination = fixture.join("destination");
    let target = destination.join("folder");
    std::fs::create_dir_all(source.join("sub")).expect("source directories");
    std::fs::create_dir_all(target.join("sub")).expect("target directories");
    std::fs::write(source.join("same.txt"), b"new").expect("source collision");
    std::fs::write(target.join("same.txt"), b"old").expect("target collision");
    std::fs::write(source.join("only.txt"), b"only source").expect("source-only file");
    std::fs::write(source.join("sub").join("deep.txt"), b"new deep")
        .expect("deep source collision");
    std::fs::write(target.join("sub").join("deep.txt"), b"old deep")
        .expect("deep target collision");
    let canonical_destination = std::fs::canonicalize(&destination).expect("canonical destination");
    let canonical_target = canonical_destination.join("folder");
    let platform = platform::native_platform();
    let mut conflicts = Vec::new();

    collect_explorer_conflicts_no_follow(
        platform.as_ref(),
        &source,
        &canonical_target,
        &mut conflicts,
    )
    .expect("recursive conflict scan");

    assert_eq!(conflicts.len(), 2);
    assert!(
        conflicts
            .iter()
            .any(|(_, target)| target == &canonical_target.join("same.txt"))
    );
    assert!(
        conflicts
            .iter()
            .any(|(_, target)| { target == &canonical_target.join("sub").join("deep.txt") })
    );
    assert!(
        !conflicts
            .iter()
            .any(|(_, target)| target == &canonical_target)
    );
    let _ = std::fs::remove_dir_all(fixture);
}

fn storage_at(fixture: &Path) -> StorageManager {
    StorageManager::open(app_paths_at(fixture))
        .expect("test storage opens")
        .manager
}

fn app_paths_at(fixture: &Path) -> platform::AppPaths {
    platform::AppPaths::from_parts(
        fixture.join("config.toml"),
        fixture.join("data"),
        fixture.join("cache"),
        fixture.join("logs"),
        fixture.join("temp"),
    )
    .expect("absolute test app paths")
}

fn wait_for_summary(
    runtime: &ShellExplorerTaskRuntime,
) -> app::explorer_tasks::ExplorerTaskSummary {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(summary) = runtime.drain_events().into_iter().find_map(|event| {
            if let ExplorerTaskEvent::Finished { summary, .. } = event {
                Some(summary)
            } else {
                None
            }
        }) {
            return summary;
        }
        assert!(Instant::now() < deadline, "background task timed out");
        std::thread::sleep(Duration::from_millis(5));
    }
}
