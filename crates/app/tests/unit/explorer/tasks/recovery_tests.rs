use super::*;
use std::sync::atomic::{AtomicU64, Ordering};

static SEQUENCE: AtomicU64 = AtomicU64::new(1);

fn fixture(label: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "tundra-explorer-recovery-{label}-{}-{}",
        std::process::id(),
        SEQUENCE.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&path).unwrap();
    path
}

fn record(phase: &str, payload: serde_json::Value) -> OperationRecord {
    serde_json::from_value(serde_json::json!({
        "schema_version": 1,
        "run_id": "test-run",
        "app_id": "explorer",
        "component": "explorer/filesystem",
        "operation_id": "test-operation",
        "kind": "explorer.filesystem.v1",
        "replay_safety": {
            "kind": "checkpointed",
            "operation": "explorer.filesystem.v1"
        },
        "recovery_handler_version": "1",
        "summary": "test",
        "checkpoint_sequence": 2,
        "checkpoint": {
            "phase": phase,
            "payload": payload
        },
        "status": "active",
        "started_at": "2026-07-12T00:00:00Z",
        "updated_at": "2026-07-12T00:00:01Z"
    }))
    .unwrap()
}

#[test]
fn explorer_io_metadata_retains_native_code_and_sanitizes_causes() {
    let error = io_error(
        "copy",
        Path::new("/source"),
        std::io::Error::from_raw_os_error(13),
    );
    let mut event = RuntimeLogEvent::new(
        LogContext::default(),
        LogLevel::Error,
        LogPhase::Failed,
        "copy failed",
    );
    fill_task_error(&mut event, &error);
    assert_eq!(event.os_error_code, Some(13));
    assert_eq!(event.error_code.as_deref(), Some("EXPLORER_IO"));
    assert!(!event.error_chain.is_empty());
    assert_eq!(error.raw_os_error(), Some(13));
}

#[test]
fn recovery_commits_a_synced_stage_after_destination_was_trashed() {
    let root = fixture("forward-commit");
    let target = root.join("target.txt");
    let staging = root.join(".tundra-stage-test-target.txt");
    fs::write(&staging, b"new").unwrap();
    let outcome = ExplorerRecoveryHandler.recover(&record(
        "destination_trashed",
        serde_json::json!({
            "target": target.display().to_string(),
            "staging": staging.display().to_string()
        }),
    ));

    assert!(outcome.is_recovered());
    assert_eq!(fs::read(&target).unwrap(), b"new");
    assert!(!staging.exists());
    let _ = fs::remove_dir_all(root);
}

#[test]
fn recovery_preserves_both_cross_volume_copies_for_manual_review() {
    let root = fixture("cross-volume");
    let source = root.join("source.txt");
    let target = root.join("target.txt");
    fs::write(&source, b"data").unwrap();
    fs::write(&target, b"data").unwrap();
    let outcome = ExplorerRecoveryHandler.recover(&record(
        "cross_volume_target_committed",
        serde_json::json!({
            "source": source.display().to_string(),
            "target": target.display().to_string()
        }),
    ));

    assert!(matches!(outcome, RecoveryOutcome::ManualActionRequired(_)));
    assert!(source.exists());
    assert!(target.exists());
    let _ = fs::remove_dir_all(root);
}

#[test]
fn recovery_removes_only_an_exact_uncommitted_stage_path() {
    let root = fixture("stage-cleanup");
    let staging = root.join(".tundra-stage-test-target.txt");
    fs::write(&staging, b"partial").unwrap();
    let outcome = ExplorerRecoveryHandler.recover(&record(
        "copy_staging_writing",
        serde_json::json!({ "staging": staging.display().to_string() }),
    ));
    assert!(outcome.is_recovered());
    assert!(!staging.exists());

    let ordinary = root.join("ordinary.txt");
    fs::write(&ordinary, b"keep").unwrap();
    let outcome = ExplorerRecoveryHandler.recover(&record(
        "copy_staging_writing",
        serde_json::json!({ "staging": ordinary.display().to_string() }),
    ));
    assert!(matches!(outcome, RecoveryOutcome::ManualActionRequired(_)));
    assert!(ordinary.exists());
    let _ = fs::remove_dir_all(root);
}

#[test]
fn recovery_preserves_a_partially_created_directory_for_manual_review() {
    let root = fixture("partial-directory");
    let target = root.join("target");
    fs::create_dir(&target).unwrap();
    fs::write(target.join("completed-child.txt"), b"keep").unwrap();

    let outcome = ExplorerRecoveryHandler.recover(&record(
        "directory_target_created",
        serde_json::json!({
            "target": target.display().to_string(),
            "replaced": false
        }),
    ));

    assert!(matches!(outcome, RecoveryOutcome::ManualActionRequired(_)));
    assert_eq!(
        fs::read(target.join("completed-child.txt")).unwrap(),
        b"keep"
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn recovery_forward_commits_when_trash_move_finished_before_its_checkpoint() {
    let root = fixture("trash-checkpoint-gap");
    let target = root.join("target.txt");
    let staging = root.join(".tundra-stage-gap-target.txt");
    fs::write(&staging, b"new").unwrap();

    let outcome = ExplorerRecoveryHandler.recover(&record(
        "destination_trash_pending",
        serde_json::json!({
            "target": target.display().to_string(),
            "staging": staging.display().to_string(),
            "replace": true
        }),
    ));

    assert!(outcome.is_recovered());
    assert_eq!(fs::read(&target).unwrap(), b"new");
    assert!(!staging.exists());
    let _ = fs::remove_dir_all(root);
}
