use super::*;
use platform::mock::UnsupportedPlatform;
use runtime_log::{LogContext, LogLevel, LogPhase, RuntimeLogEvent};
use std::io::Write;
static ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        let path = std::env::temp_dir().canonicalize().unwrap().join(format!(
            "tundra-runtime-query-{}-{}",
            std::process::id(),
            ID.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(path.join("runtime")).unwrap();
        fs::create_dir(path.join("crashes")).unwrap();
        Self(path)
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn event(owner: &str) -> RuntimeLogEvent {
    RuntimeLogEvent::new(
        LogContext {
            owner_id: Some(owner.into()),
            module: "ux.explorer".into(),
            operation: "copy".into(),
            run_id: Some("run-one".into()),
            ..Default::default()
        },
        LogLevel::Error,
        LogPhase::Failed,
        format!("failure for {owner}"),
    )
}
fn write_events(root: &Path, events: &[RuntimeLogEvent]) -> PathBuf {
    let path = root.join("runtime/runtime-test.jsonl");
    let mut file = fs::File::create(&path).unwrap();
    for event in events {
        serde_json::to_writer(&mut file, event).unwrap();
        writeln!(file).unwrap();
    }
    path
}
fn report(root: &Path, id: &str, owner: Option<&str>) {
    let record = serde_json::json!({"schema_version":1,"incident_id":id,"kind":"error","severity":"error","occurred_at":"2026-09-10T10:00:00Z","owner_id":owner,"run_id":"run-one","boundary":"file.copy","component":"ux.explorer","app":null,"error":{"message":"failure","source_chain":["permission denied","password=secret-value"],"backtrace":"private state"},"recovery":{"status":"recovered","detail":"retry completed"},"runtime":{"password":"hidden-runtime"},"breadcrumbs":[{"message":"copy started","event_id":"event-1","clipboard":"private-clipboard"}]});
    fs::write(
        root.join("crashes").join(format!("{id}.json")),
        serde_json::to_vec(&record).unwrap(),
    )
    .unwrap();
}

#[test]
fn runtime_logs_user_filter_cannot_be_overridden() {
    let root = Temp::new();
    write_events(&root.0, &[event("alice"), event("bob")]);
    let query = LogQuery {
        owner_id: Some("bob".into()),
        ..Default::default()
    };
    let snapshot = query_snapshot(
        &root.0,
        &query,
        &LogAccess::User("alice".into()),
        &UnsupportedPlatform,
        &AtomicBool::new(false),
    );
    assert_eq!(snapshot.result.events.len(), 1);
    assert_eq!(
        snapshot.result.events[0].context.owner_id.as_deref(),
        Some("alice")
    );
}
#[test]
fn runtime_logs_user_file_open_is_sanitized_filtered_snapshot() {
    let root = Temp::new();
    let mut alice = event("alice");
    alice.message = "token=private-value".into();
    let original = write_events(&root.0, &[alice, event("bob")]);
    let path = prepare_log_document(
        &root.0,
        &LogQuery::default(),
        &LogAccess::User("alice".into()),
        &LogDocumentSelection::File(original.clone()),
        &UnsupportedPlatform,
        &AtomicBool::new(false),
    )
    .unwrap();
    assert_ne!(path, original);
    let text = fs::read_to_string(&path).unwrap();
    assert!(text.contains("alice"));
    assert!(!text.contains("bob"));
    assert!(!text.contains("private-value"));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
}
#[test]
fn runtime_logs_file_selection_rejects_outside_and_traversal() {
    let root = Temp::new();
    let original = write_events(&root.0, &[event("alice")]);
    for path in [
        root.0.join("crashes/../runtime/runtime-test.jsonl"),
        root.0.join("../runtime-test.jsonl"),
        root.0.join("crashes/test.json"),
    ] {
        assert!(
            prepare_log_document(
                &root.0,
                &LogQuery::default(),
                &LogAccess::Admin,
                &LogDocumentSelection::File(path),
                &UnsupportedPlatform,
                &AtomicBool::new(false)
            )
            .is_err()
        );
    }
    assert!(original.exists());
}
#[cfg(unix)]
#[test]
fn runtime_logs_symlink_file_and_directory_are_rejected() {
    use std::os::unix::fs::symlink;
    let root = Temp::new();
    let outside = Temp::new();
    let file = write_events(&outside.0, &[event("secret")]);
    let link = root.0.join("runtime/runtime-link.jsonl");
    symlink(&file, &link).unwrap();
    assert!(
        prepare_log_document(
            &root.0,
            &LogQuery::default(),
            &LogAccess::Admin,
            &LogDocumentSelection::File(link),
            &UnsupportedPlatform,
            &AtomicBool::new(false)
        )
        .is_err()
    );
    fs::remove_dir_all(root.0.join("runtime")).unwrap();
    symlink(outside.0.join("runtime"), root.0.join("runtime")).unwrap();
    let result = query_snapshot(
        &root.0,
        &LogQuery::default(),
        &LogAccess::Admin,
        &UnsupportedPlatform,
        &AtomicBool::new(false),
    );
    assert_eq!(result.result.state, LogSourceState::PermissionDenied);
}
#[test]
fn runtime_logs_user_linux_and_full_reports_require_admin() {
    let root = Temp::new();
    report(&root.0, "legacy", None);
    report(&root.0, "alice-report", Some("alice"));
    report(&root.0, "bob-report", Some("bob"));
    let access = LogAccess::User("alice".into());
    let snapshot = query_snapshot(
        &root.0,
        &LogQuery::default(),
        &access,
        &UnsupportedPlatform,
        &AtomicBool::new(false),
    );
    assert_eq!(snapshot.incidents.len(), 1);
    assert_eq!(snapshot.incidents[0].incident_id, "alice-report");
    assert!(
        snapshot.incidents[0]
            .json_report_path
            .as_os_str()
            .is_empty()
    );
    assert!(
        prepare_log_document(
            &root.0,
            &LogQuery::default(),
            &access,
            &LogDocumentSelection::Incident("alice-report".into()),
            &UnsupportedPlatform,
            &AtomicBool::new(false)
        )
        .is_err()
    );
    let result = query_snapshot(
        &root.0,
        &LogQuery {
            source: LogSource::Linux,
            ..Default::default()
        },
        &access,
        &UnsupportedPlatform,
        &AtomicBool::new(false),
    );
    assert_eq!(result.result.state, LogSourceState::PermissionDenied);
}
#[test]
fn runtime_logs_incident_document_uses_catalog_and_whitelist() {
    let root = Temp::new();
    report(&root.0, "incident-one", None);
    let path = prepare_log_document(
        &root.0,
        &LogQuery::default(),
        &LogAccess::Admin,
        &LogDocumentSelection::Incident("incident-one".into()),
        &UnsupportedPlatform,
        &AtomicBool::new(false),
    )
    .unwrap();
    let text = fs::read_to_string(path).unwrap();
    assert!(text.contains("permission denied"));
    assert!(text.contains("copy started"));
    for secret in [
        "secret-value",
        "hidden-runtime",
        "private-clipboard",
        "private state",
    ] {
        assert!(!text.contains(secret));
    }
    assert!(
        prepare_log_document(
            &root.0,
            &LogQuery::default(),
            &LogAccess::Admin,
            &LogDocumentSelection::Incident("../incident-one".into()),
            &UnsupportedPlatform,
            &AtomicBool::new(false)
        )
        .is_err()
    );
}
#[test]
fn runtime_logs_export_manifest_carries_partial_and_reports_without_overwrite() {
    let root = Temp::new();
    write_events(&root.0, &[event("alice")]);
    report(&root.0, "incident-one", None);
    fs::write(root.0.join("runtime/runtime-damaged.jsonl"), "broken\n").unwrap();
    let output = root.0.join("export");
    let result = export_diagnostics(
        &root.0,
        &LogQuery::default(),
        &LogAccess::OsUser,
        &UnsupportedPlatform,
        &AtomicBool::new(false),
        &output,
    )
    .unwrap();
    assert_eq!(result.state, LogSourceState::Partial);
    assert!(output.join("incident-0000.json").exists());
    let manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(output.join("manifest.json")).unwrap()).unwrap();
    assert_eq!(manifest["source_state"], "partial");
    assert_eq!(manifest["damaged_records"], 1);
    assert!(
        export_diagnostics(
            &root.0,
            &LogQuery::default(),
            &LogAccess::OsUser,
            &UnsupportedPlatform,
            &AtomicBool::new(false),
            &output
        )
        .is_err()
    );
}
#[test]
fn runtime_logs_cancelled_queries_and_documents_return_promptly() {
    let root = Temp::new();
    let cancelled = AtomicBool::new(true);
    let snapshot = query_snapshot(
        &root.0,
        &LogQuery::default(),
        &LogAccess::Admin,
        &UnsupportedPlatform,
        &cancelled,
    );
    assert_eq!(snapshot.result.state, LogSourceState::Cancelled);
    assert!(
        prepare_log_document(
            &root.0,
            &LogQuery::default(),
            &LogAccess::Admin,
            &LogDocumentSelection::Events,
            &UnsupportedPlatform,
            &cancelled
        )
        .is_err()
    );
}

#[test]
fn runtime_logs_files_hide_foreign_and_mixed_segments() {
    let root = Temp::new();
    let mixed = write_events(&root.0, &[event("alice"), event("bob")]);
    let owned = root.0.join("runtime/runtime-alice.jsonl");
    let foreign = root.0.join("runtime/runtime-bob.jsonl");
    for (path, event) in [(&owned, event("alice")), (&foreign, event("bob"))] {
        fs::write(
            path,
            format!("{}\n", serde_json::to_string(&event).unwrap()),
        )
        .unwrap();
    }
    let snapshot = query_snapshot(
        &root.0,
        &LogQuery::default(),
        &LogAccess::User("alice".into()),
        &UnsupportedPlatform,
        &AtomicBool::new(false),
    );
    assert_eq!(
        snapshot
            .files
            .iter()
            .map(|file| &file.path)
            .collect::<Vec<_>>(),
        vec![&owned]
    );
    assert!(
        !snapshot
            .files
            .iter()
            .any(|file| file.path == mixed || file.path == foreign)
    );
}

#[test]
fn runtime_logs_snapshot_cache_honors_configured_quota_and_age() {
    let root = Temp::new();
    fs::write(root.0.join("runtime/.retention.limit"), "1").unwrap();
    assert!(
        prepare_log_document(
            &root.0,
            &LogQuery::default(),
            &LogAccess::Admin,
            &LogDocumentSelection::Events,
            &UnsupportedPlatform,
            &AtomicBool::new(false)
        )
        .is_err()
    );
    fs::write(root.0.join("runtime/.retention.limit"), "1048576").unwrap();
    let path = prepare_log_document(
        &root.0,
        &LogQuery::default(),
        &LogAccess::Admin,
        &LogDocumentSelection::Events,
        &UnsupportedPlatform,
        &AtomicBool::new(false),
    )
    .unwrap();
    let old = std::time::SystemTime::now() - std::time::Duration::from_secs(31 * 60);
    fs::File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_times(fs::FileTimes::new().set_modified(old))
        .unwrap();
    prepare_log_document(
        &root.0,
        &LogQuery::default(),
        &LogAccess::Admin,
        &LogDocumentSelection::Events,
        &UnsupportedPlatform,
        &AtomicBool::new(false),
    )
    .unwrap();
    assert!(!path.exists());
}

#[cfg(unix)]
#[test]
fn runtime_logs_snapshot_cache_refuses_symlink_directory() {
    let root = Temp::new();
    let outside = Temp::new();
    std::os::unix::fs::symlink(&outside.0, root.0.join("runtime/snapshots")).unwrap();
    assert!(
        prepare_log_document(
            &root.0,
            &LogQuery::default(),
            &LogAccess::Admin,
            &LogDocumentSelection::Events,
            &UnsupportedPlatform,
            &AtomicBool::new(false)
        )
        .is_err()
    );
    assert_eq!(fs::read_dir(&outside.0).unwrap().count(), 2);
}
