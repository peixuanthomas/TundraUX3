use runtime_log::*;
use std::{
    fs,
    path::Path,
    time::{Duration, Instant},
};

fn fixture() -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    for path in [
        "runtime/runtime-old.jsonl",
        "crashes/crash-one.json",
        "crashes/crash-one.txt",
        "runtime/snapshots/snapshot-one.jsonl",
        "application.log",
        "runtime/.retention.limit",
        "watchdog/runs/run.active.json",
        "export/events.jsonl",
        "notes.txt",
    ] {
        let path = root.path().join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, "keep or clear\n").unwrap();
    }
    root
}
#[test]
fn preview_and_each_type_only_select_its_files() {
    for (kind, paths) in [
        (LogFileType::Runtime, vec!["runtime/runtime-old.jsonl"]),
        (
            LogFileType::Incidents,
            vec!["crashes/crash-one.json", "crashes/crash-one.txt"],
        ),
        (
            LogFileType::Snapshots,
            vec!["runtime/snapshots/snapshot-one.jsonl"],
        ),
        (LogFileType::Legacy, vec!["application.log"]),
    ] {
        let root = fixture();
        let target = LogClearTarget::Type(kind);
        let preview = clear_logs(root.path(), &target, false).unwrap();
        assert!(preview.failures.is_empty());
        assert_eq!(preview.files.len(), paths.len());
        assert!(
            preview
                .files
                .iter()
                .all(|entry| entry.action == LogClearAction::Preview && entry.path.exists())
        );
        let report = clear_logs(root.path(), &target, true).unwrap();
        assert!(report.failures.is_empty(), "{:?}", report.failures);
        for path in paths {
            assert!(!root.path().join(path).exists());
        }
        let remaining = clear_logs(root.path(), &LogClearTarget::All, false).unwrap();
        assert_eq!(remaining.files.len(), 5 - report.files.len());
    }
}
#[test]
fn all_preserves_state_policies_exports_and_unrecognized_files() {
    let root = fixture();
    let report = clear_logs(root.path(), &LogClearTarget::All, true).unwrap();
    assert!(report.failures.is_empty(), "{:?}", report.failures);
    assert_eq!(report.files.len(), 5);
    for path in [
        "runtime/.retention.limit",
        "watchdog/runs/run.active.json",
        "export/events.jsonl",
        "notes.txt",
    ] {
        assert_eq!(
            fs::read_to_string(root.path().join(path)).unwrap(),
            "keep or clear\n"
        );
    }
    assert!(
        clear_logs(root.path(), &LogClearTarget::All, true)
            .unwrap()
            .files
            .is_empty()
    );
}
#[test]
fn exact_file_accepts_relative_and_absolute_paths_without_deleting_siblings() {
    let root = fixture();
    for path in [
        "crashes/crash-one.json".into(),
        root.path().join("runtime/runtime-old.jsonl"),
    ] {
        let report = clear_logs(root.path(), &LogClearTarget::File(path), true).unwrap();
        assert_eq!(report.files.len(), 1);
        assert!(report.failures.is_empty());
    }
    assert!(root.path().join("crashes/crash-one.txt").exists());
    let report = clear_logs(
        root.path(),
        &LogClearTarget::File("crashes/missing.json".into()),
        true,
    )
    .unwrap();
    assert_eq!(report.failures.len(), 1);
    for path in [
        "../outside.log",
        "runtime/../application.log",
        "runtime/.retention.limit",
        "watchdog/runs/run.active.json",
        "/outside.log",
    ] {
        assert!(
            clear_logs(root.path(), &LogClearTarget::File(path.into()), true).is_err(),
            "{path}"
        );
    }
}
#[test]
fn missing_root_is_empty_without_creating_directories() {
    let root = tempfile::tempdir().unwrap();
    let missing = root.path().join("absent");
    let report = clear_logs(&missing, &LogClearTarget::All, true).unwrap();
    assert!(report.files.is_empty() && report.failures.is_empty());
    assert!(!missing.exists());
}
#[test]
fn directory_disguised_as_report_is_reported_as_partial_failure() {
    let root = fixture();
    fs::create_dir(root.path().join("crashes/not-a-file.json")).unwrap();
    let report = clear_logs(
        root.path(),
        &LogClearTarget::Type(LogFileType::Incidents),
        true,
    )
    .unwrap();
    assert_eq!(report.files.len(), 2);
    assert_eq!(report.failures.len(), 1);
    assert!(root.path().join("crashes/not-a-file.json").is_dir());
}
#[cfg(unix)]
#[test]
fn symlinked_files_directories_and_roots_never_clear_external_logs() {
    use std::os::unix::fs::symlink;
    let outside = tempfile::tempdir().unwrap();
    fs::write(outside.path().join("crash-one.json"), "external").unwrap();
    for kind in ["file", "directory", "root"] {
        let root = tempfile::tempdir().unwrap();
        let logs = root.path().join("logs");
        fs::create_dir(&logs).unwrap();
        match kind {
            "file" => {
                fs::create_dir(logs.join("crashes")).unwrap();
                symlink(
                    outside.path().join("crash-one.json"),
                    logs.join("crashes/crash-one.json"),
                )
                .unwrap();
            }
            "directory" => symlink(outside.path(), logs.join("crashes")).unwrap(),
            _ => {
                fs::remove_dir(&logs).unwrap();
                symlink(outside.path(), &logs).unwrap();
            }
        }
        let report =
            clear_logs(&logs, &LogClearTarget::Type(LogFileType::Incidents), true).unwrap();
        assert!(!report.failures.is_empty());
        assert_eq!(
            fs::read_to_string(outside.path().join("crash-one.json")).unwrap(),
            "external"
        );
    }
}
fn event(text: &str) -> RuntimeLogEvent {
    RuntimeLogEvent::new(
        LogContext {
            module: "ux.test".into(),
            operation: "sample".into(),
            ..Default::default()
        },
        LogLevel::Info,
        LogPhase::Observed,
        text,
    )
}
fn wait_written(handle: &RuntimeLogHandle, count: u64) {
    let until = Instant::now() + Duration::from_secs(5);
    while handle.health().written_events < count {
        assert!(Instant::now() < until, "writer timed out");
        std::thread::sleep(Duration::from_millis(10));
    }
}
#[test]
fn clearing_active_segments_preserves_future_events_without_zero_padding() {
    let root = tempfile::tempdir().unwrap();
    let directory = root.path().join("runtime");
    let runtime =
        RuntimeLogRuntime::start(RuntimeLogConfig::new(directory.clone(), "live-run".into()))
            .unwrap();
    let handle = runtime.handle();
    assert!(handle.record(event("before clear")));
    wait_written(&handle, 1);
    let report = clear_logs(
        root.path(),
        &LogClearTarget::Type(LogFileType::Runtime),
        true,
    )
    .unwrap();
    assert!(report.failures.is_empty(), "{:?}", report.failures);
    assert_eq!(report.files.len(), 1);
    assert_eq!(report.files[0].action, LogClearAction::Truncated);
    assert_eq!(fs::metadata(&report.files[0].path).unwrap().len(), 0);
    assert!(handle.record(event("after clear")));
    let health = runtime.shutdown();
    assert_eq!(health.written_events, 2);
    let result = query_logs(&directory, &LogQuery::default());
    assert_eq!(result.damaged_records, 0);
    assert_eq!(result.events.len(), 1);
    assert_eq!(result.events[0].message, "after clear");
    assert!(!fs::read(&report.files[0].path).unwrap().contains(&0));
}
#[cfg(unix)]
#[test]
fn selecting_incidents_does_not_inspect_unrelated_runtime_directory() {
    let root = tempfile::tempdir().unwrap();
    std::os::unix::fs::symlink(Path::new("/does/not/exist"), root.path().join("runtime")).unwrap();
    fs::create_dir(root.path().join("crashes")).unwrap();
    fs::write(root.path().join("crashes/crash-one.json"), "report").unwrap();
    let report = clear_logs(
        root.path(),
        &LogClearTarget::Type(LogFileType::Incidents),
        true,
    )
    .unwrap();
    assert!(report.failures.is_empty());
    assert_eq!(report.files.len(), 1);
}
