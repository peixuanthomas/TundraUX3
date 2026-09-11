use runtime_log::*;
use std::{
    fs,
    io::Write,
    time::{Duration, Instant},
};

fn event(owner: &str, phase: LogPhase, text: &str) -> RuntimeLogEvent {
    RuntimeLogEvent::new(
        LogContext {
            owner_id: Some(owner.into()),
            module: "ux.explorer".into(),
            operation: "copy".into(),
            operation_id: Some("operation-1".into()),
            ..Default::default()
        },
        if phase == LogPhase::Failed {
            LogLevel::Error
        } else {
            LogLevel::Info
        },
        phase,
        text,
    )
}
fn setup() -> (tempfile::TempDir, RuntimeLogRuntime) {
    let dir = tempfile::tempdir().unwrap();
    let runtime = RuntimeLogRuntime::start(RuntimeLogConfig::new(
        dir.path().to_path_buf(),
        "run-1".into(),
    ))
    .unwrap();
    (dir, runtime)
}
#[test]
fn shutdown_flushes_and_query_filters_owner_and_source() {
    let (dir, runtime) = setup();
    let h = runtime.handle();
    assert!(h.record(event("alice", LogPhase::Failed, "permission denied")));
    assert!(h.record(event("bob", LogPhase::Succeeded, "copied")));
    let mut kernel = event("alice", LogPhase::Observed, "kernel warning");
    kernel.source = LogSource::Linux;
    assert!(h.record(kernel));
    let health = runtime.shutdown();
    assert_eq!(health.written_events, 3);
    assert_eq!(health.pending_events, 0);
    assert!(!h.record(event("alice", LogPhase::Succeeded, "after shutdown")));
    let query = LogQuery {
        owner_id: Some("alice".into()),
        ..Default::default()
    };
    let results = query_logs(dir.path(), &query);
    assert_eq!(results.events.len(), 1);
    assert_eq!(results.events[0].context.run_id.as_deref(), Some("run-1"));
    let linux = query_logs(
        dir.path(),
        &LogQuery {
            source: LogSource::Linux,
            ..query
        },
    );
    assert_eq!(linux.events.len(), 1);
    assert!(linux.events[0].context.run_id.is_none());
}
#[test]
fn repeated_alerts_keep_stages_counts_and_recovery() {
    let (dir, runtime) = setup();
    let h = runtime.handle();
    for phase in [
        LogPhase::Failed,
        LogPhase::Failed,
        LogPhase::Retry,
        LogPhase::Retry,
        LogPhase::Degraded,
        LogPhase::Recovered,
    ] {
        let mut e = event("alice", phase, "service unavailable");
        e.alert_key = Some("server-1".into());
        h.record(e);
    }
    runtime.shutdown();
    let result = query_logs(dir.path(), &LogQuery::default());
    assert_eq!(result.events.len(), 4);
    let recovered = result
        .events
        .iter()
        .find(|e| e.phase == LogPhase::Recovered)
        .unwrap();
    assert_eq!(recovered.repeat_count, 5);
    assert_eq!(recovered.retry_count, 2);
    assert!(recovered.first_seen <= recovered.last_seen);
}
#[test]
fn unresolved_alert_flush_does_not_invent_recovery() {
    let (dir, runtime) = setup();
    let h = runtime.handle();
    for _ in 0..3 {
        let mut e = event("alice", LogPhase::Failed, "offline");
        e.alert_key = Some("server".into());
        h.record(e);
    }
    runtime.shutdown();
    let results = query_logs(dir.path(), &LogQuery::default());
    assert_eq!(results.events.len(), 2);
    assert!(
        results
            .events
            .iter()
            .any(|e| e.phase == LogPhase::Repeated && e.repeat_count == 3)
    );
    assert!(
        !results
            .events
            .iter()
            .any(|e| e.phase == LogPhase::Recovered)
    );
}
#[test]
fn secret_metadata_is_redacted_and_sizes_bounded() {
    for (text, secret) in [
        ("Authorization: Bearer abc123", "abc123"),
        ("https://u:secretpass@host/file", "secretpass"),
        ("failed https://host/?token=xyz123", "xyz123"),
        (
            "clipboard: private clipboard text",
            "private clipboard text",
        ),
        ("file_body=personal text", "personal text"),
        ("Cookie: auth=xyz", "xyz"),
    ] {
        assert!(!sanitize_text(text).contains(secret), "{text}");
    }
    assert!(!sanitize_text("bad\u{1b}[31m\nerror").contains('\u{1b}'));
    let mut e = event("alice", LogPhase::Failed, &"a".repeat(1_000_000));
    e.error_chain = vec!["Password: a_secret".into(); 500];
    sanitize_event(&mut e);
    assert!(e.message.len() <= 2048);
    assert_eq!(e.error_chain.len(), 8);
    assert!(!serde_json::to_string(&e).unwrap().contains("a_secret"));
}
#[test]
fn latest_query_tolerates_corruption_partial_tail_and_oversize() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("runtime-test.jsonl");
    let mut file = fs::File::create(path).unwrap();
    for i in 0..5 {
        let mut e = event("alice", LogPhase::Succeeded, &format!("event-{i}"));
        e.timestamp = chrono::Utc::now() + chrono::Duration::seconds(i);
        writeln!(file, "{}", serde_json::to_string(&e).unwrap()).unwrap();
    }
    writeln!(file, "broken").unwrap();
    writeln!(file, "{}", "x".repeat(70_000)).unwrap();
    write!(file, "{{\"partial\":").unwrap();
    let result = query_logs(
        dir.path(),
        &LogQuery {
            limit: 2,
            ..Default::default()
        },
    );
    assert_eq!(result.events.len(), 2);
    assert_eq!(result.events[0].message, "event-4");
    assert_eq!(result.damaged_records, 3);
    assert!(result.truncated);
    assert_eq!(result.state, LogSourceState::Partial);
}
#[test]
fn export_is_private_sanitized_and_never_overwrites() {
    let dir = tempfile::tempdir().unwrap();
    let output = dir.path().join("export");
    let mut e = event("alice", LogPhase::Failed, "failed");
    e.error_chain.push("token=extremely-secret".into());
    let result = LogQueryResult {
        events: vec![e],
        notices: vec!["password=huntersecret".into()],
        ..Default::default()
    };
    export_logs(&output, &result, &LogWriterHealth::default()).unwrap();
    assert!(
        !fs::read_to_string(output.join("events.jsonl"))
            .unwrap()
            .contains("extremely-secret")
    );
    assert!(
        !fs::read_to_string(output.join("manifest.json"))
            .unwrap()
            .contains("huntersecret")
    );
    assert!(export_logs(&output, &result, &LogWriterHealth::default()).is_err());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(output.join("events.jsonl"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
}
#[test]
fn rotation_retention_only_remove_closed_runtime_segments() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("incident.json"), "keep").unwrap();
    let mut config = RuntimeLogConfig::new(dir.path().to_path_buf(), "run".into());
    config.segment_bytes = 1000;
    config.max_total_bytes = 2500;
    let runtime = RuntimeLogRuntime::start(config).unwrap();
    for i in 0..20 {
        runtime
            .handle()
            .record(event("alice", LogPhase::Succeeded, &format!("copy-{i}")));
    }
    runtime.shutdown();
    let files: Vec<_> = fs::read_dir(dir.path())
        .unwrap()
        .map(Result::unwrap)
        .filter(|e| e.file_name().to_string_lossy().starts_with("runtime-"))
        .collect();
    assert!(!files.is_empty());
    assert!(
        files
            .iter()
            .map(|e| e.metadata().unwrap().len())
            .sum::<u64>()
            <= 2500
    );
    assert!(dir.path().join("incident.json").exists());
}
#[test]
fn zero_day_retention_does_not_delete_another_active_writer() {
    let dir = tempfile::tempdir().unwrap();
    let a = RuntimeLogRuntime::start(RuntimeLogConfig::new(dir.path().to_path_buf(), "a".into()))
        .unwrap();
    a.handle()
        .record(event("alice", LogPhase::Succeeded, "active"));
    let deadline = Instant::now() + Duration::from_secs(2);
    while a.handle().health().written_events == 0 && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(5));
    }
    let mut config = RuntimeLogConfig::new(dir.path().to_path_buf(), "b".into());
    config.max_age_days = 0;
    let b = RuntimeLogRuntime::start(config).unwrap();
    b.shutdown();
    assert_eq!(query_logs(dir.path(), &LogQuery::default()).events.len(), 1);
    a.shutdown();
}
#[test]
fn tiny_queue_has_nonblocking_backpressure() {
    let dir = tempfile::tempdir().unwrap();
    let mut config = RuntimeLogConfig::new(dir.path().to_path_buf(), "run".into());
    config.queue_capacity = 1;
    let runtime = RuntimeLogRuntime::start(config).unwrap();
    let handle = runtime.handle();
    let start = Instant::now();
    let mut rejected = 0;
    for _ in 0..5000 {
        if !handle.record(event("alice", LogPhase::Succeeded, "copied")) {
            rejected += 1;
        }
    }
    assert!(rejected > 0);
    assert!(start.elapsed() < Duration::from_secs(5));
    let health = runtime.shutdown();
    assert!(health.dropped_events >= rejected);
}
#[cfg(unix)]
#[test]
fn symlink_files_and_directories_are_not_followed() {
    use std::os::unix::fs::symlink;
    let dir = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    fs::write(outside.path().join("private"), "secret").unwrap();
    symlink(
        outside.path().join("private"),
        dir.path().join("runtime-symlink.jsonl"),
    )
    .unwrap();
    let result = query_logs(dir.path(), &LogQuery::default());
    assert!(result.events.is_empty());
    assert_eq!(result.state, LogSourceState::Partial);
    let link = dir.path().join("link");
    symlink(outside.path(), &link).unwrap();
    assert!(RuntimeLogRuntime::start(RuntimeLogConfig::new(link, "run".into())).is_err());
}
#[test]
fn global_can_be_reinstalled_after_shutdown() {
    let (_dir, runtime) = setup();
    assert!(install_global(runtime.handle()).is_ok());
    assert!(global().is_some());
    runtime.shutdown();
    assert!(global().is_none());
    let (_dir2, runtime2) = setup();
    assert!(install_global(runtime2.handle()).is_ok());
    runtime2.shutdown();
}

#[test]
fn write_failure_reports_health_without_blocking_or_recursive_events() {
    let dir = tempfile::tempdir().unwrap();
    let logs = dir.path().join("logs");
    let runtime =
        RuntimeLogRuntime::start(RuntimeLogConfig::new(logs.clone(), "run".into())).unwrap();
    fs::rename(&logs, dir.path().join("previous-logs")).unwrap();
    fs::write(&logs, "not a directory").unwrap();
    let start = Instant::now();
    assert!(
        runtime
            .handle()
            .record(event("alice", LogPhase::Failed, "failed copy"))
    );
    assert!(start.elapsed() < Duration::from_secs(1));
    let health = runtime.shutdown();
    assert_eq!(health.dropped_events, 1);
    assert!(health.write_failures > 0);
    assert!(health.last_error.is_some());
}
#[test]
fn storage_cap_is_enforced_while_writer_is_active() {
    let dir = tempfile::tempdir().unwrap();
    let mut config = RuntimeLogConfig::new(dir.path().to_path_buf(), "run".into());
    config.max_total_bytes = 2500;
    config.segment_bytes = 100_000;
    let runtime = RuntimeLogRuntime::start(config).unwrap();
    for i in 0..20 {
        runtime
            .handle()
            .record(event(&format!("owner-{i}"), LogPhase::Succeeded, "done"));
    }
    let deadline = Instant::now() + Duration::from_secs(3);
    while runtime.handle().health().pending_events > 0 && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(5));
    }
    let total: u64 = fs::read_dir(dir.path())
        .unwrap()
        .map(Result::unwrap)
        .filter(|e| e.file_name().to_string_lossy().starts_with("runtime-"))
        .map(|e| e.metadata().unwrap().len())
        .sum();
    assert!(total <= 2500, "{total}");
    runtime.shutdown();
}
#[test]
fn alert_counts_do_not_cross_owners_or_resources() {
    let (dir, runtime) = setup();
    for owner in ["alice", "bob"] {
        for resource in ["one", "two"] {
            for _ in 0..2 {
                let mut e = event(owner, LogPhase::Failed, "failed");
                e.alert_key = Some("same-alert".into());
                e.source_path = Some(resource.into());
                runtime.handle().record(e);
            }
        }
    }
    runtime.shutdown();
    let result = query_logs(dir.path(), &LogQuery::default());
    assert_eq!(result.events.len(), 8);
    assert_eq!(
        result
            .events
            .iter()
            .filter(|e| e.phase == LogPhase::Repeated && e.repeat_count == 2)
            .count(),
        4
    );
}

#[test]
fn cancelled_query_is_cancelled_even_for_empty_storage() {
    let dir = tempfile::tempdir().unwrap();
    let result = query_logs_cancellable(
        dir.path(),
        &LogQuery::default(),
        &std::sync::atomic::AtomicBool::new(true),
    );
    assert_eq!(result.state, LogSourceState::Cancelled);
    assert!(result.events.is_empty());
}

#[test]
fn snapshots_and_records_share_configured_capacity() {
    let dir = tempfile::tempdir().unwrap();
    let mut config = RuntimeLogConfig::new(dir.path().into(), "small-quota".into());
    config.max_total_bytes = 4096;
    let runtime = RuntimeLogRuntime::start(config).unwrap();
    runtime
        .handle()
        .record(event("alice", LogPhase::Succeeded, "done"));
    runtime.shutdown();
    assert_eq!(storage_limit(dir.path()).unwrap(), 4096);
    let snapshots = dir.path().join("snapshots");
    fs::create_dir(&snapshots).unwrap();
    let guard = reserve_storage_capacity(dir.path(), 4096).unwrap();
    fs::write(snapshots.join("snapshot-first.jsonl"), vec![b' '; 4096]).unwrap();
    drop(guard);
    assert!(
        query_logs(dir.path(), &LogQuery::default())
            .events
            .is_empty()
    );
    let guard = reserve_storage_capacity(dir.path(), 1024).unwrap();
    assert!(!snapshots.join("snapshot-first.jsonl").exists());
    fs::write(snapshots.join("snapshot-next.jsonl"), vec![b' '; 1024]).unwrap();
    drop(guard);
    assert!(reserve_storage_capacity(dir.path(), 4097).is_err());
}

#[test]
fn localization_metadata_is_optional_in_schema_one() {
    let original = event("alice", LogPhase::Succeeded, "Copied 2 items");
    let encoded = serde_json::to_value(&original).unwrap();
    assert_eq!(encoded["schema_version"], 1);
    assert!(encoded.get("message_id").is_none());
    assert!(encoded.get("message_args").is_none());
    let restored: RuntimeLogEvent = serde_json::from_value(encoded.clone()).unwrap();
    assert_eq!(restored, original);

    let mut explicit_null = encoded;
    explicit_null["message_id"] = serde_json::Value::Null;
    explicit_null["message_args"] = serde_json::json!({});
    assert_eq!(
        serde_json::from_value::<RuntimeLogEvent>(explicit_null).unwrap(),
        original
    );
}

#[test]
fn localization_metadata_survives_persistence_with_readable_fallback() {
    let (dir, runtime) = setup();
    let mut original = event("alice", LogPhase::Succeeded, "Copied 2 items");
    original.message_id = Some("app-explorer-copied".into());
    original
        .message_args
        .insert("count".into(), serde_json::json!(2));
    original
        .message_args
        .insert("destination".into(), serde_json::json!("/tmp/文档"));
    assert!(runtime.handle().record(original.clone()));
    runtime.shutdown();
    let result = query_logs(dir.path(), &LogQuery::default());
    assert_eq!(result.events.len(), 1);
    let restored = &result.events[0];
    assert_eq!(restored.schema_version, 1);
    assert_eq!(restored.message, original.message);
    assert_eq!(restored.message_id, original.message_id);
    assert_eq!(restored.message_args, original.message_args);
}

#[test]
fn localization_metadata_obeys_metadata_privacy_limits() {
    let mut original = event("alice", LogPhase::Failed, "Failed");
    original.message_id = Some("app-explorer-clipboard-empty".into());
    original
        .message_args
        .insert("password".into(), serde_json::json!("opaque-secret"));
    original.message_args.insert(
        "details".into(),
        serde_json::json!({
            "nested": ["token=secret-value", {"cookie": "session-value"}],
            "count": 3
        }),
    );
    original
        .message_args
        .insert("oversize".into(), serde_json::json!("x".repeat(10_000)));
    sanitize_event(&mut original);
    let encoded = serde_json::to_string(&original).unwrap();
    for secret in ["opaque-secret", "secret-value", "session-value"] {
        assert!(!encoded.contains(secret));
    }
    assert!(encoded.len() < 4096);
    assert_eq!(original.message_args["details"]["count"], 3);
    assert_eq!(
        original.message_id.as_deref(),
        Some("app-explorer-clipboard-empty")
    );
    original.message_id = Some("password=opaque-secret".into());
    sanitize_event(&mut original);
    assert!(original.message_id.is_none());
}
