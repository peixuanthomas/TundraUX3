use super::*;
use crate::report::{ErrorDetails, PanicDetails};
use crate::{
    AppCriticality, AppId, BoundaryKind, Breadcrumb, RetentionPolicy, RuntimeSnapshot,
    WatchdogRuntime,
};
use chrono::Duration as ChronoDuration;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(1);

fn test_runtime(
    label: &str,
    retention: RetentionPolicy,
) -> (
    WatchdogRuntime,
    crate::ProcessWatchdog,
    WatchdogConfig,
    PathBuf,
) {
    let root = std::env::temp_dir().join(format!(
        "tundra-watchdog-catalog-{label}-{}-{}",
        std::process::id(),
        NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed)
    ));
    let mut config = WatchdogConfig::new(
        root.join("reports"),
        root.join("fallback"),
        root.join("data"),
        "catalog-test",
        env!("CARGO_PKG_VERSION"),
    );
    config.retention = retention;
    let (runtime, process) = WatchdogRuntime::start_isolated(config.clone()).unwrap();
    (runtime, process, config, root)
}

fn incident(incident_id: &str, occurred_at: DateTime<Utc>, message: &str) -> IncidentRecord {
    IncidentRecord {
        schema_version: REPORT_SCHEMA_VERSION,
        incident_id: incident_id.to_string(),
        report_stem: incident_id.to_string(),
        kind: IncidentKind::Error,
        severity: IncidentSeverity::Error,
        occurred_at,
        process_name: "catalog-test".to_string(),
        process_version: "1.0.0".to_string(),
        process_id: 42,
        run_id: "catalog-run".to_string(),
        app: Some(AppDescriptor::new(
            AppId::new("catalog.app").unwrap(),
            "Catalog App",
            "1.0.0",
            AppCriticality::Optional,
        )),
        component: Some("catalog.app/worker".to_string()),
        task_id: None,
        task_group: None,
        boundary: "catalog.scan".to_string(),
        boundary_kind: BoundaryKind::Worker,
        task_kind: None,
        replay_safety: None,
        operation_kind: None,
        operation_id: None,
        recovery_handler_version: None,
        panic_action: None,
        restart_policy: None,
        restart_attempt: 0,
        thread_name: Some("catalog-test".to_string()),
        thread_id: "ThreadId(42)".to_string(),
        panic: None,
        error: Some(ErrorDetails {
            message: message.to_string(),
            source_chain: vec!["private source detail".to_string()],
            backtrace: "private backtrace".to_string(),
        }),
        runtime: RuntimeSnapshot::default(),
        breadcrumbs: Vec::<Breadcrumb>::new(),
        recovery: RecoveryOutcome::Recovered("continued".to_string()),
        secondary_errors: Vec::new(),
    }
}

fn write_report(directory: &Path, name: &str, record: &IncidentRecord, with_text: bool) -> PathBuf {
    fs::create_dir_all(directory).unwrap();
    let path = directory.join(format!("{name}.json"));
    fs::write(&path, serde_json::to_vec_pretty(record).unwrap()).unwrap();
    if with_text {
        fs::write(path.with_extension("txt"), "incident report").unwrap();
    }
    path
}

fn cleanup(runtime: WatchdogRuntime, root: &Path) {
    runtime.shutdown().unwrap();
    let _ = fs::remove_dir_all(root);
}

#[test]
fn catalog_deduplicates_prefers_primary_and_sorts_newest_first() {
    let (runtime, process, config, root) = test_runtime("deduplicate", RetentionPolicy::default());
    let now = Utc::now();
    write_report(
        &config.fallback_dir,
        "duplicate-fallback",
        &incident("duplicate", now, "fallback copy"),
        true,
    );
    let primary_path = write_report(
        &config.report_dir,
        "duplicate-primary",
        &incident(
            "duplicate",
            now - ChronoDuration::minutes(3),
            "primary copy",
        ),
        true,
    );
    write_report(
        &config.report_dir,
        "repeated-old",
        &incident("repeated", now - ChronoDuration::minutes(4), "old copy"),
        false,
    );
    let repeated_path = write_report(
        &config.report_dir,
        "repeated-new",
        &incident("repeated", now - ChronoDuration::minutes(1), "new copy"),
        false,
    );
    write_report(
        &config.fallback_dir,
        "latest",
        &incident("latest", now - ChronoDuration::seconds(1), "latest report"),
        true,
    );

    let catalog = process.list_incident_reports();
    assert!(catalog.warnings.is_empty());
    assert_eq!(
        catalog
            .reports
            .iter()
            .map(|report| report.incident_id.as_str())
            .collect::<Vec<_>>(),
        vec!["latest", "repeated", "duplicate"]
    );
    let duplicate = catalog
        .reports
        .iter()
        .find(|report| report.incident_id == "duplicate")
        .unwrap();
    assert_eq!(duplicate.summary, "primary copy");
    assert_eq!(duplicate.json_report_path, primary_path);
    assert!(duplicate.text_report_path.is_some());
    assert_eq!(duplicate.app.as_ref().unwrap().id.as_str(), "catalog.app");
    assert_eq!(duplicate.component.as_deref(), Some("catalog.app/worker"));
    assert_eq!(duplicate.boundary, "catalog.scan");
    assert!(duplicate.recovery.is_recovered());

    let repeated = catalog
        .reports
        .iter()
        .find(|report| report.incident_id == "repeated")
        .unwrap();
    assert_eq!(repeated.summary, "new copy");
    assert_eq!(repeated.json_report_path, repeated_path);
    assert!(repeated.text_report_path.is_none());
    let public_json = serde_json::to_value(repeated).unwrap();
    assert!(public_json.get("backtrace").is_none());
    assert!(public_json.get("error").is_none());
    assert!(public_json.get("panic").is_none());

    cleanup(runtime, &root);
}

#[test]
fn catalog_warns_about_bad_reports_and_keeps_usable_entries() {
    let (runtime, process, config, root) = test_runtime("warnings", RetentionPolicy::default());
    fs::create_dir_all(&config.report_dir).unwrap();
    fs::write(config.report_dir.join("corrupt.json"), b"{not-json").unwrap();
    fs::create_dir_all(config.report_dir.join("unreadable.json")).unwrap();
    let mut unsupported = incident("unsupported", Utc::now(), "future schema");
    unsupported.schema_version = REPORT_SCHEMA_VERSION + 1;
    write_report(&config.report_dir, "unsupported", &unsupported, false);
    write_report(
        &config.fallback_dir,
        "usable",
        &incident("usable", Utc::now(), "usable report"),
        false,
    );

    let catalog = process.list_incident_reports();
    assert_eq!(catalog.reports.len(), 1);
    assert_eq!(catalog.reports[0].incident_id, "usable");
    assert_eq!(catalog.warnings.len(), 3);
    assert!(
        catalog
            .warnings
            .iter()
            .any(|warning| warning.contains("corrupt.json"))
    );
    assert!(
        catalog
            .warnings
            .iter()
            .any(|warning| warning.contains("unreadable.json"))
    );
    assert!(
        catalog
            .warnings
            .iter()
            .any(|warning| warning.contains("unsupported schema"))
    );

    cleanup(runtime, &root);
}

#[test]
fn catalog_enforces_max_age_and_max_incidents() {
    let retention = RetentionPolicy {
        max_incidents: 2,
        max_age: Duration::from_secs(60 * 60),
        ..RetentionPolicy::default()
    };
    let (runtime, process, config, root) = test_runtime("retention", retention);
    let now = Utc::now();
    for (id, minutes) in [("first", 1), ("second", 2), ("third", 3)] {
        write_report(
            &config.report_dir,
            id,
            &incident(id, now - ChronoDuration::minutes(minutes), id),
            false,
        );
    }
    write_report(
        &config.report_dir,
        "expired",
        &incident("expired", now - ChronoDuration::hours(2), "expired"),
        false,
    );

    let catalog = process.list_incident_reports();
    assert!(catalog.warnings.is_empty());
    assert_eq!(
        catalog
            .reports
            .iter()
            .map(|report| report.incident_id.as_str())
            .collect::<Vec<_>>(),
        vec!["first", "second"]
    );

    cleanup(runtime, &root);
}

#[test]
fn panic_details_are_reduced_to_the_public_summary() {
    let (runtime, process, config, root) =
        test_runtime("panic-projection", RetentionPolicy::default());
    let mut record = incident("panic", Utc::now(), "unused error");
    record.kind = IncidentKind::Panic;
    record.error = None;
    record.panic = Some(PanicDetails {
        payload: "safe panic summary".to_string(),
        source_file: Some("private/source.rs".to_string()),
        source_line: Some(42),
        source_column: Some(7),
        backtrace: "private panic backtrace".to_string(),
    });
    write_report(&config.report_dir, "panic", &record, false);

    let catalog = process.list_incident_reports();
    assert_eq!(catalog.reports[0].summary, "safe panic summary");
    let public_json = serde_json::to_value(&catalog.reports[0]).unwrap();
    assert!(public_json.get("backtrace").is_none());
    assert!(public_json.get("source_file").is_none());
    assert!(public_json.get("source_line").is_none());

    cleanup(runtime, &root);
}
