use crate::config::WatchdogConfig;
use crate::report::{IncidentRecord, REPORT_SCHEMA_VERSION};
use crate::{AppDescriptor, IncidentKind, IncidentSeverity, RecoveryOutcome};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IncidentReportSummary {
    pub incident_id: String,
    pub occurred_at: DateTime<Utc>,
    pub kind: IncidentKind,
    pub severity: IncidentSeverity,
    pub app: Option<AppDescriptor>,
    pub component: Option<String>,
    pub boundary: String,
    pub summary: String,
    pub recovery: RecoveryOutcome,
    pub json_report_path: PathBuf,
    pub text_report_path: Option<PathBuf>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct IncidentReportCatalog {
    pub reports: Vec<IncidentReportSummary>,
    pub warnings: Vec<String>,
}

#[derive(Debug)]
struct Candidate {
    report: IncidentReportSummary,
    is_primary: bool,
    modified_at: Option<SystemTime>,
}

pub(crate) fn list_incident_reports(config: &WatchdogConfig) -> IncidentReportCatalog {
    let now = Utc::now();
    let mut warnings = Vec::new();
    let mut reports_by_id = HashMap::<String, Candidate>::new();

    scan_directory(
        &config.report_dir,
        true,
        config,
        now,
        &mut reports_by_id,
        &mut warnings,
    );
    scan_directory(
        &config.fallback_dir,
        false,
        config,
        now,
        &mut reports_by_id,
        &mut warnings,
    );

    let mut reports = reports_by_id
        .into_values()
        .map(|candidate| candidate.report)
        .collect::<Vec<_>>();
    reports.sort_by(|left, right| {
        right
            .occurred_at
            .cmp(&left.occurred_at)
            .then_with(|| left.incident_id.cmp(&right.incident_id))
    });
    reports.truncate(config.retention.max_incidents);

    IncidentReportCatalog { reports, warnings }
}

fn scan_directory(
    directory: &Path,
    is_primary: bool,
    config: &WatchdogConfig,
    now: DateTime<Utc>,
    reports_by_id: &mut HashMap<String, Candidate>,
    warnings: &mut Vec<String>,
) {
    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return,
        Err(error) => {
            warnings.push(format!(
                "failed to enumerate incident report directory {}: {error}",
                directory.display()
            ));
            return;
        }
    };

    let mut paths = Vec::new();
    for entry in entries {
        match entry {
            Ok(entry) => paths.push(entry.path()),
            Err(error) => warnings.push(format!(
                "failed to inspect an entry in incident report directory {}: {error}",
                directory.display()
            )),
        }
    }
    paths.sort();

    for json_report_path in paths {
        if json_report_path
            .extension()
            .and_then(|value| value.to_str())
            != Some("json")
        {
            continue;
        }
        let bytes = match fs::read(&json_report_path) {
            Ok(bytes) => bytes,
            Err(error) => {
                warnings.push(format!(
                    "failed to read incident report {}: {error}",
                    json_report_path.display()
                ));
                continue;
            }
        };
        let record = match serde_json::from_slice::<IncidentRecord>(&bytes) {
            Ok(record) => record,
            Err(error) => {
                warnings.push(format!(
                    "failed to parse incident report {}: {error}",
                    json_report_path.display()
                ));
                continue;
            }
        };
        if record.schema_version != REPORT_SCHEMA_VERSION {
            warnings.push(format!(
                "ignored incident report {} with unsupported schema version {}",
                json_report_path.display(),
                record.schema_version
            ));
            continue;
        }
        if is_older_than_retention(now, record.occurred_at, config.retention.max_age) {
            continue;
        }

        let text_path = json_report_path.with_extension("txt");
        let incident_id = record.incident_id.clone();
        let summary = record.summary();
        let candidate = Candidate {
            report: IncidentReportSummary {
                incident_id: incident_id.clone(),
                occurred_at: record.occurred_at,
                kind: record.kind,
                severity: record.severity,
                app: record.app,
                component: record.component,
                boundary: record.boundary,
                summary,
                recovery: record.recovery,
                json_report_path: json_report_path.clone(),
                text_report_path: text_path.is_file().then_some(text_path),
            },
            is_primary,
            modified_at: fs::metadata(&json_report_path)
                .and_then(|metadata| metadata.modified())
                .ok(),
        };

        match reports_by_id.get(&incident_id) {
            Some(existing) if !candidate_is_preferred(&candidate, existing) => {}
            _ => {
                reports_by_id.insert(incident_id, candidate);
            }
        }
    }
}

fn candidate_is_preferred(candidate: &Candidate, existing: &Candidate) -> bool {
    if candidate.is_primary != existing.is_primary {
        return candidate.is_primary;
    }
    if candidate.report.occurred_at != existing.report.occurred_at {
        return candidate.report.occurred_at > existing.report.occurred_at;
    }
    candidate.modified_at > existing.modified_at
}

fn is_older_than_retention(
    now: DateTime<Utc>,
    occurred_at: DateTime<Utc>,
    max_age: std::time::Duration,
) -> bool {
    now.signed_duration_since(occurred_at)
        .to_std()
        .is_ok_and(|age| age > max_age)
}

#[cfg(test)]
mod tests {
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

    fn write_report(
        directory: &Path,
        name: &str,
        record: &IncidentRecord,
        with_text: bool,
    ) -> PathBuf {
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
        let (runtime, process, config, root) =
            test_runtime("deduplicate", RetentionPolicy::default());
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
}
