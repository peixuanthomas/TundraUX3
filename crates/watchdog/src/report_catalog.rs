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
#[path = "tests/report_catalog.rs"]
mod tests;
