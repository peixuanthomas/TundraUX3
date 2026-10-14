use super::*;
use serde_json::json;

struct RecoveryFixture(PathBuf);

impl RecoveryFixture {
    fn new(label: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "tundra-editor-recovery-{label}-{}",
            RecoveryId::default().0
        ));
        fs::create_dir_all(&root).unwrap();
        Self(fs::canonicalize(root).unwrap())
    }

    fn paths(&self) -> AppPaths {
        app_paths(&self.0)
    }
}

impl Drop for RecoveryFixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn app_paths(root: &Path) -> AppPaths {
    AppPaths::from_parts(
        root.join("config.toml"),
        root.join("state"),
        root.join("cache"),
        root.join("logs"),
        root.join("temp"),
    )
    .unwrap()
}

fn draft(text: &str, updated: u64) -> EditorRecoveryRecordV2 {
    let mut record = EditorRecoveryRecordV2::source(text, RecoveryDocumentKind::PlainText);
    record.updated_at_epoch_ms = updated;
    record
}

fn new_draft(paths: &AppPaths, text: &str, updated: u64) -> EditorRecoveryLease {
    let lease = acquire_editor_recovery(paths, "alice", &RecoveryId::default()).unwrap();
    write_editor_recovery_v2(&lease, &draft(text, updated)).unwrap();
    lease
}

fn write_legacy(paths: &AppPaths, record: &EditorRecoveryRecord) {
    let value = json!({
        "schema": 1, "source": record.source, "path": record.path,
        "markdown": record.markdown, "source_mode": record.source_mode,
        "cursor": record.cursor, "saved_content_hash": record.saved_content_hash,
        "updated_at_epoch_ms": record.updated_at_epoch_ms,
    });
    atomic_write_document(
        &editor_recovery_path(paths, "alice"),
        &serde_json::to_vec(&value).unwrap(),
    )
    .unwrap();
}

#[test]
fn recovery_round_trip_is_scoped_by_user_and_document() {
    let fixture = RecoveryFixture::new("scoped");
    let paths = fixture.paths();
    let first = new_draft(&paths, "first", 1);
    let second = new_draft(&paths, "second", 2);
    assert_ne!(first.id(), second.id());
    assert_ne!(first.path(), second.path());
    assert_eq!(read_editor_recoveries(&paths, "alice").unwrap().len(), 2);
    assert!(read_editor_recoveries(&paths, "bob").unwrap().is_empty());
    write_editor_recovery_v2(&first, &draft("updated first", 3)).unwrap();
    clear_editor_recovery(&first).unwrap();
    assert_eq!(
        read_editor_recoveries(&paths, "alice").unwrap(),
        vec![(
            second.id().clone(),
            VersionedEditorRecovery::V2(draft("second", 2))
        )]
    );
    assert!(
        claim_next_editor_recovery(&paths, "alice")
            .unwrap()
            .is_none()
    );
}

#[test]
fn active_drafts_cannot_be_claimed_or_overwritten() {
    let fixture = RecoveryFixture::new("active");
    let paths = fixture.paths();
    let lease = new_draft(&paths, "active draft", 1);
    let id = lease.id().clone();
    assert!(acquire_editor_recovery(&paths, "alice", &id).is_err());
    assert!(
        claim_next_editor_recovery(&paths, "alice")
            .unwrap()
            .is_none()
    );
    drop(lease);
    assert!(
        acquire_editor_recovery(&paths, "alice", &id).is_err(),
        "an abandoned draft must also be claimed before writing"
    );
    let (claimed, record) = claim_next_editor_recovery(&paths, "alice")
        .unwrap()
        .unwrap();
    assert_eq!(claimed.id(), &id);
    assert_eq!(
        record,
        VersionedEditorRecovery::V2(draft("active draft", 1))
    );
}

#[test]
fn cloned_render_state_retains_ownership_until_its_last_copy_is_dropped() {
    let fixture = RecoveryFixture::new("clone");
    let paths = fixture.paths();
    let lease = new_draft(&paths, "shared lock", 1);
    let snapshot = lease.clone();
    assert_eq!(lease, snapshot);
    drop(lease);
    assert!(
        claim_next_editor_recovery(&paths, "alice")
            .unwrap()
            .is_none()
    );
    drop(snapshot);
    assert!(
        claim_next_editor_recovery(&paths, "alice")
            .unwrap()
            .is_some()
    );
}

#[test]
fn duplicate_descriptor_does_not_extend_draft_ownership() {
    let fixture = RecoveryFixture::new("duplicate-descriptor");
    let paths = fixture.paths();
    let lease = new_draft(&paths, "draft", 1);
    // A duplicate shares the same lock, just like the descriptor briefly
    // inherited by a child between fork and exec on Linux.
    let duplicate = lease.lock.0.try_clone().unwrap();
    drop(lease);
    let claimed = claim_next_editor_recovery(&paths, "alice").unwrap();
    drop(duplicate);
    assert!(claimed.is_some());
}

#[test]
fn inactive_drafts_are_claimed_one_at_a_time_in_updated_order() {
    let fixture = RecoveryFixture::new("ordered");
    let paths = fixture.paths();
    let older = new_draft(&paths, "older", 1);
    let newer = new_draft(&paths, "newer", 20);
    let active = new_draft(&paths, "active newest", 100);
    let older_id = older.id().clone();
    let newer_id = newer.id().clone();
    drop(older);
    drop(newer);
    let (first, _) = claim_next_editor_recovery(&paths, "alice")
        .unwrap()
        .unwrap();
    let (second, _) = claim_next_editor_recovery(&paths, "alice")
        .unwrap()
        .unwrap();
    assert_eq!(first.id(), &newer_id);
    assert_eq!(second.id(), &older_id);
    assert!(
        claim_next_editor_recovery(&paths, "alice")
            .unwrap()
            .is_none()
    );
    clear_editor_recovery(&first).unwrap();
    assert!(second.path().is_file());
    assert!(active.path().is_file());
    assert_eq!(read_editor_recoveries(&paths, "alice").unwrap().len(), 2);
}

#[test]
fn schema_two_round_trips_a_rich_document_and_keeps_schema_one_readable() {
    let fixture = RecoveryFixture::new("legacy");
    let paths = fixture.paths();
    let lease = acquire_editor_recovery(&paths, "alice", &RecoveryId::default()).unwrap();
    let mut record = EditorRecoveryRecordV2::rich(RichDocument::new(), "# fallback\n");
    record.path = Some(PathBuf::from("notes/example.md"));
    record.metadata.utf8_bom = true;
    record.saved_content_hash = Some(42);
    write_editor_recovery_v2(&lease, &record).unwrap();
    assert_eq!(
        read_versioned_recovery_path(lease.path()).unwrap(),
        Some(VersionedEditorRecovery::V2(record))
    );
    let mut legacy = EditorRecoveryRecord::new("legacy source");
    legacy.cursor = 4;
    legacy.path = Some(PathBuf::from("C:/notes/example.md"));
    write_legacy(&paths, &legacy);
    assert_eq!(
        read_editor_recovery(&paths, "alice").unwrap(),
        Some(legacy.clone())
    );
    assert_eq!(read_editor_recovery(&paths, "bob").unwrap(), None);
    let (claimed, restored) = claim_next_editor_recovery(&paths, "alice")
        .unwrap()
        .unwrap();
    assert_eq!(restored, VersionedEditorRecovery::V1(legacy));
    assert!(acquire_editor_recovery(&paths, "alice", claimed.id()).is_err());
    assert!(
        claim_next_editor_recovery(&paths, "alice")
            .unwrap()
            .is_none()
    );
    write_editor_recovery_v2(&claimed, &draft("legacy edited", 50)).unwrap();
    clear_editor_recovery(&claimed).unwrap();
    assert!(
        lease.path().is_file(),
        "clearing the legacy draft must retain other drafts"
    );
    assert!(read_editor_recovery(&paths, "alice").unwrap().is_none());
}

#[test]
fn old_schema_two_file_can_be_claimed_and_cleared_separately() {
    let fixture = RecoveryFixture::new("legacy-v2");
    let paths = fixture.paths();
    let record = draft("old v2", 10);
    let mut value = serde_json::to_value(&record).unwrap();
    value["schema"] = Value::from(2);
    atomic_write_document(
        &editor_recovery_path(&paths, "alice"),
        &serde_json::to_vec(&value).unwrap(),
    )
    .unwrap();
    let other = new_draft(&paths, "new v2", 20);
    let (claimed, restored) = claim_next_editor_recovery(&paths, "alice")
        .unwrap()
        .unwrap();
    assert_eq!(restored, VersionedEditorRecovery::V2(record));
    clear_editor_recovery(&claimed).unwrap();
    assert!(other.path().exists());
}

#[test]
fn unknown_or_legacy_fingerprint_keeps_the_draft_without_binding_to_disk() {
    let fixture = RecoveryFixture::new("hash-version");
    let paths = fixture.paths();
    let mut record = draft("unsaved text must survive", 10);
    record.path = Some(fixture.0.join("source.txt"));
    record.saved_content_hash = Some(42);
    for algorithm in [Value::Null, Value::from("future-hash")] {
        let mut value = serde_json::to_value(&record).unwrap();
        value["schema"] = Value::from(2);
        value["saved_content_hash_algorithm"] = algorithm;
        let path = editor_recovery_path(&paths, "alice");
        atomic_write_document(&path, &serde_json::to_vec(&value).unwrap()).unwrap();
        let Some(VersionedEditorRecovery::V2(restored)) =
            read_versioned_recovery_path(&path).unwrap()
        else {
            panic!("draft must remain readable")
        };
        assert_eq!(restored.saved_content_hash, None);
        assert_eq!(restored.payload, record.payload);
        assert_eq!(restored.path, record.path);
        assert_eq!(
            fs::read(&path).unwrap(),
            serde_json::to_vec(&value).unwrap()
        );
    }
    let mut legacy = EditorRecoveryRecord::new("legacy unsaved text");
    legacy.saved_content_hash = Some(42);
    write_legacy(&paths, &legacy);
    legacy.saved_content_hash = None;
    assert_eq!(read_editor_recovery(&paths, "alice").unwrap(), Some(legacy));
}

#[test]
fn damaged_rich_payload_recovers_the_private_markdown_fallback_as_source() {
    let fixture = RecoveryFixture::new("fallback");
    let paths = fixture.paths();
    let record = EditorRecoveryRecordV2::rich(RichDocument::new(), "# safe fallback\n");
    let mut value = serde_json::to_value(record).unwrap();
    value["schema"] = Value::from(2);
    value["payload"]["document"]["blocks"] = Value::String("corrupt".to_string());
    atomic_write_document(
        &editor_recovery_path(&paths, "alice"),
        &serde_json::to_vec_pretty(&value).unwrap(),
    )
    .unwrap();
    let (lease, VersionedEditorRecovery::V2Fallback { record, warning }) =
        claim_next_editor_recovery(&paths, "alice")
            .unwrap()
            .unwrap()
    else {
        panic!("expected Source fallback")
    };
    assert!(warning.contains("Source mode"));
    assert!(
        matches!(record.payload, EditorRecoveryPayload::Source { ref text, .. } if text == "# safe fallback\n")
    );
    clear_editor_recovery(&lease).unwrap();
}

#[test]
fn corrupt_record_does_not_hide_a_valid_draft_or_get_deleted() {
    let fixture = RecoveryFixture::new("corrupt");
    let paths = fixture.paths();
    let corrupt = new_draft(&paths, "bad", 20);
    let corrupt_path = corrupt.path().to_owned();
    fs::write(&corrupt_path, b"invalid json").unwrap();
    drop(corrupt);
    let valid = new_draft(&paths, "good", 1);
    let valid_id = valid.id().clone();
    drop(valid);
    let (claimed, _) = claim_next_editor_recovery(&paths, "alice")
        .unwrap()
        .unwrap();
    assert_eq!(claimed.id(), &valid_id);
    clear_editor_recovery(&claimed).unwrap();
    assert_eq!(fs::read(corrupt_path).unwrap(), b"invalid json");
    assert!(claim_next_editor_recovery(&paths, "alice").is_err());
}

#[test]
fn clearing_a_draft_keeps_the_lock_until_its_editor_is_gone() {
    let fixture = RecoveryFixture::new("clear-lock");
    let paths = fixture.paths();
    let lease = new_draft(&paths, "before save", 1);
    clear_editor_recovery(&lease).unwrap();
    assert!(acquire_editor_recovery(&paths, "alice", lease.id()).is_err());
    write_editor_recovery_v2(&lease, &draft("new edits", 2)).unwrap();
    assert!(
        claim_next_editor_recovery(&paths, "alice")
            .unwrap()
            .is_none()
    );
    assert_eq!(
        read_versioned_recovery_path(lease.path()).unwrap(),
        Some(VersionedEditorRecovery::V2(draft("new edits", 2)))
    );
}

#[test]
fn recovery_child_process() {
    let Some(root) = std::env::var_os("TUNDRA_RECOVERY_CHILD_ROOT") else {
        return;
    };
    let paths = app_paths(Path::new(&root));
    let _lease = new_draft(&paths, "draft from another process", 1);
    use std::io::Write;
    println!("RECOVERY_READY");
    std::io::stdout().flush().unwrap();
    let mut input = String::new();
    std::io::stdin().read_line(&mut input).unwrap();
}

#[test]
fn other_process_is_excluded_and_abnormal_exit_releases_its_draft() {
    use std::io::BufRead;
    use std::process::{Child, Command, Stdio};
    struct ChildGuard(Child);
    impl Drop for ChildGuard {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let fixture = RecoveryFixture::new("process");
    let paths = fixture.paths();
    let mut child = ChildGuard(
        Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "editor::recovery::tests::recovery_child_process",
                "--nocapture",
            ])
            .env("TUNDRA_RECOVERY_CHILD_ROOT", &fixture.0)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap(),
    );
    let output = std::io::BufReader::new(child.0.stdout.take().unwrap());
    assert!(
        output
            .lines()
            .any(|line| line.unwrap().contains("RECOVERY_READY"))
    );
    assert_eq!(read_editor_recoveries(&paths, "alice").unwrap().len(), 1);
    assert!(
        claim_next_editor_recovery(&paths, "alice")
            .unwrap()
            .is_none()
    );
    child.0.kill().unwrap();
    child.0.wait().unwrap();
    let (lease, record) = claim_next_editor_recovery(&paths, "alice")
        .unwrap()
        .unwrap();
    assert_eq!(
        record,
        VersionedEditorRecovery::V2(draft("draft from another process", 1))
    );
    clear_editor_recovery(&lease).unwrap();
}
