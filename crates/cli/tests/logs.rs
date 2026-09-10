use cli::{CliCommand, LogsAction, LogsFormat, LogsVerb, parse_args, run_with_platform};
use platform::{Platform, UserDirs, build_macos_app_paths, mock::MockPlatform};
use runtime_log::{LogContext, LogLevel, LogPhase, LogSource, RuntimeLogEvent};
use std::{
    fs,
    io::Write,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};
static ID: AtomicU64 = AtomicU64::new(1);
struct Fixture {
    base: PathBuf,
    root: PathBuf,
    platform: MockPlatform,
}
impl Fixture {
    fn new() -> Self {
        let base = std::env::temp_dir().canonicalize().unwrap().join(format!(
            "tundra-cli-logs-{}-{}",
            std::process::id(),
            ID.fetch_add(1, Ordering::Relaxed)
        ));
        let dirs = UserDirs::new(
            base.join("Desktop"),
            base.join("Documents"),
            base.join("Downloads"),
            base.join("Pictures"),
            base.join("Videos"),
            base.join("Music"),
            base.join("Data"),
        )
        .unwrap();
        let paths = build_macos_app_paths(base.join("Home"), base.join("Temp")).unwrap();
        let platform = MockPlatform::new(dirs, paths);
        let root = platform.app_paths().unwrap().logs_path().to_path_buf();
        fs::create_dir_all(root.join("runtime")).unwrap();
        fs::create_dir(root.join("crashes")).unwrap();
        Self {
            base,
            root,
            platform,
        }
    }
    fn run(&self, args: &[&str]) -> (i32, String, String) {
        let mut stdout = vec![];
        let mut stderr = vec![];
        let code = run_with_platform(args, &self.platform, &mut stdout, &mut stderr);
        (
            code,
            String::from_utf8(stdout).unwrap(),
            String::from_utf8(stderr).unwrap(),
        )
    }
    fn events(&self) {
        let mut file = fs::File::create(self.root.join("runtime/runtime-test.jsonl")).unwrap();
        for (owner, module, level) in [
            ("alice", "ux.explorer", LogLevel::Error),
            ("bob", "ux.editor", LogLevel::Info),
        ] {
            let mut event = RuntimeLogEvent::new(
                LogContext {
                    owner_id: Some(owner.into()),
                    module: module.into(),
                    operation: "save".into(),
                    run_id: Some("run-a".into()),
                    operation_id: Some("op-a".into()),
                    task_id: Some("task-a".into()),
                    ..Default::default()
                },
                level,
                LogPhase::Failed,
                "access denied",
            );
            event.timestamp = chrono::DateTime::parse_from_rfc3339("2026-09-10T10:00:00Z")
                .unwrap()
                .with_timezone(&chrono::Utc);
            serde_json::to_writer(&mut file, &event).unwrap();
            writeln!(file).unwrap();
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.base);
    }
}

#[test]
fn logs_parser_defaults_and_combined_filters() {
    let CliCommand::Logs(LogsAction::Run {
        verb,
        query,
        format,
        output,
    }) = parse_args(["logs", "query"]).unwrap()
    else {
        panic!("logs command")
    };
    assert_eq!(verb, LogsVerb::Query);
    assert_eq!(query.limit, 200);
    assert_eq!(query.source, LogSource::Ux);
    assert_eq!(format, LogsFormat::Text);
    assert!(output.is_none());
    let parsed = parse_args([
        "logs",
        "query",
        "--source",
        "linux",
        "--since",
        "2026-09-10T00:00:00Z",
        "--until",
        "2026-09-11T00:00:00+08:00",
        "--level",
        "warning",
        "--module",
        "linux.kernel",
        "--run-id",
        "run",
        "--operation-id",
        "op",
        "--task-id",
        "task",
        "--incident-id",
        "incident",
        "--limit",
        "12",
        "--format",
        "jsonl",
    ])
    .unwrap();
    let CliCommand::Logs(LogsAction::Run { query, format, .. }) = parsed else {
        panic!()
    };
    assert_eq!(query.source, LogSource::Linux);
    assert_eq!(query.min_level, Some(LogLevel::Warning));
    assert_eq!(query.limit, 12);
    assert_eq!(query.task_id.as_deref(), Some("task"));
    assert_eq!(format, LogsFormat::Jsonl);
}
#[test]
fn logs_invalid_flags_roles_ranges_and_formats_exit_two() {
    let fixture = Fixture::new();
    for args in [
        vec!["logs", "query", "--admin", "true"],
        vec!["logs", "query", "--owner-id", "alice"],
        vec!["logs", "query", "--limit", "0"],
        vec!["logs", "query", "--limit", "10001"],
        vec!["logs", "query", "--source", "windows"],
        vec!["logs", "incidents", "--source", "linux"],
        vec!["logs", "query", "--format", "yaml"],
        vec!["logs", "query", "--since", "bad"],
        vec![
            "logs",
            "query",
            "--since",
            "2026-09-11T00:00:00Z",
            "--until",
            "2026-09-10T00:00:00Z",
        ],
        vec!["logs", "query", "--limit", "2", "--limit", "3"],
        vec!["logs", "export"],
    ] {
        assert!(parse_args(&args).is_err(), "{args:?}");
        assert_eq!(fixture.run(&args).0, 2, "{args:?}");
    }
}
#[test]
fn logs_jsonl_filters_and_empty_results_are_machine_readable() {
    let fixture = Fixture::new();
    fixture.events();
    let (code, stdout, stderr) = fixture.run(&[
        "logs",
        "query",
        "--module",
        "ux.explorer",
        "--level",
        "error",
        "--run-id",
        "run-a",
        "--operation-id",
        "op-a",
        "--task-id",
        "task-a",
        "--format",
        "jsonl",
    ]);
    assert_eq!(code, 0, "{stderr}");
    let values = stdout
        .lines()
        .map(|s| serde_json::from_str::<serde_json::Value>(s).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(values.len(), 1);
    assert_eq!(values[0]["context"]["module"], "ux.explorer");
    let (code, stdout, _) =
        fixture.run(&["logs", "query", "--run-id", "missing", "--format", "jsonl"]);
    assert_eq!(code, 0);
    assert!(stdout.is_empty());
}
#[test]
fn logs_truncation_and_unsupported_source_are_nonzero() {
    let fixture = Fixture::new();
    fixture.events();
    let (code, stdout, stderr) =
        fixture.run(&["logs", "query", "--limit", "1", "--format", "jsonl"]);
    assert_eq!(code, 3);
    assert_eq!(stdout.lines().count(), 1);
    assert!(stderr.contains("truncated"));
    let (code, _, stderr) = fixture.run(&["logs", "query", "--source", "linux"]);
    assert_eq!(code, 1);
    assert!(stderr.contains("Unsupported"));
}
#[test]
fn logs_export_refuses_existing_target_and_records_partial() {
    let fixture = Fixture::new();
    fixture.events();
    fs::write(fixture.root.join("runtime/runtime-bad.jsonl"), "bad\n").unwrap();
    let output = fixture.base.join("package");
    let (code, _, stderr) = fixture.run(&["logs", "export", "--output", output.to_str().unwrap()]);
    assert_eq!(code, 3, "{stderr}");
    let manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(output.join("manifest.json")).unwrap()).unwrap();
    assert_eq!(manifest["source_state"], "partial");
    assert_eq!(manifest["damaged_records"], 1);
    assert!(manifest.get("writer_health").is_some());
    assert!(output.join("events.jsonl").exists());
    assert_eq!(
        fixture
            .run(&["logs", "export", "--output", output.to_str().unwrap()])
            .0,
        1
    );
}
#[test]
fn logs_incident_filters_and_export_preserve_correlation() {
    let fixture = Fixture::new();
    let report = serde_json::json!({"schema_version":1,"incident_id":"incident-one","run_id":"run-a","task_id":"task-a","operation_id":"op-a","occurred_at":"2026-09-10T10:00:00Z","kind":"error","severity":"error","component":"ux.explorer","boundary":"copy","error":{"message":"copy denied","source_chain":["permission denied"]},"recovery":{"status":"recovered","detail":"retry worked"}});
    fs::write(
        fixture.root.join("crashes/report.json"),
        serde_json::to_vec(&report).unwrap(),
    )
    .unwrap();
    let (code, stdout, stderr) = fixture.run(&[
        "logs",
        "incidents",
        "--run-id",
        "run-a",
        "--task-id",
        "task-a",
        "--format",
        "jsonl",
    ]);
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(stdout.trim()).unwrap()["incident_id"],
        "incident-one"
    );
    let output = fixture.base.join("package");
    assert_eq!(
        fixture
            .run(&[
                "logs",
                "export",
                "--incident-id",
                "incident-one",
                "--output",
                output.to_str().unwrap()
            ])
            .0,
        0
    );
    let exported: serde_json::Value =
        serde_json::from_slice(&fs::read(output.join("incident-0000.json")).unwrap()).unwrap();
    assert_eq!(exported["run_id"], "run-a");
    assert_eq!(exported["task_id"], "task-a");
}
#[test]
fn logs_help_explains_access_filters_and_exit_codes() {
    let fixture = Fixture::new();
    let (code, stdout, _) = fixture.run(&["logs", "help"]);
    assert_eq!(code, 0);
    assert!(stdout.contains("--source ux|linux"));
    assert!(stdout.contains("current OS identity"));
    assert!(stdout.contains("3 partial/truncated"));
}
