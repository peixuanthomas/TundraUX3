use super::*;
use chrono::{DateTime, Utc};
use serde_json::{Value, json};
use watchdog::{IncidentReportSummary, IncidentSeverity};

pub(super) fn catalog(
    root: &Path,
    query: &LogQuery,
    access: &LogAccess,
    cancelled: &AtomicBool,
    result: &mut LogQueryResult,
) -> Vec<(Value, IncidentReportSummary)> {
    let mut found = Vec::new();
    let entries = match fs::read_dir(root.join("crashes")) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return found,
        Err(_) => {
            partial(result, "Could not read incident catalog");
            return found;
        }
    };
    let mut bytes_read = 0;
    for (index, entry) in entries.enumerate() {
        if cancelled.load(Ordering::Relaxed) {
            result.state = LogSourceState::Cancelled;
            break;
        }
        if index >= 256 || bytes_read >= 32 * 1024 * 1024 {
            result.truncated = true;
            partial(result, "Incident catalog scan limit reached");
            break;
        }
        let Ok(entry) = entry else {
            partial(result, "Could not inspect incident entry");
            continue;
        };
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        if !fs::symlink_metadata(&path).is_ok_and(|m| m.is_file() && !m.file_type().is_symlink()) {
            partial(result, "Skipped non-regular incident report");
            continue;
        }
        let bytes = match platform::read_document_bytes_limited_with_progress(
            &path,
            2 * 1024 * 1024,
            |_, _| !cancelled.load(Ordering::Relaxed),
        ) {
            Ok(value) => value.bytes,
            Err(_) => {
                partial(result, "Could not safely read incident report");
                continue;
            }
        };
        bytes_read += bytes.len();
        let record: Value = match serde_json::from_slice(&bytes) {
            Ok(record) => record,
            Err(_) => {
                result.damaged_records += 1;
                partial(result, "Skipped damaged incident report");
                continue;
            }
        };
        if record.get("schema_version").and_then(Value::as_u64) != Some(1) {
            result.damaged_records += 1;
            partial(result, "Unsupported incident report schema");
            continue;
        }
        if let LogAccess::User(owner) = access {
            if record.get("owner_id").and_then(Value::as_str) != Some(owner.as_str()) {
                continue;
            }
        }
        let Some(mut summary) = summary(&record, &path) else {
            result.damaged_records += 1;
            partial(result, "Skipped incomplete incident report");
            continue;
        };
        if !matches_query(&record, &summary, query) {
            continue;
        }
        if matches!(access, LogAccess::User(_)) {
            // Report-level metadata is visible to its owner; global breadcrumbs
            // and full legacy details require administrator diagnostics access.
            summary.json_report_path = PathBuf::new();
            summary.text_report_path = None;
        }
        found.push((safe_report(&record), summary));
    }
    found.sort_by(|a, b| b.1.occurred_at.cmp(&a.1.occurred_at));
    found.dedup_by(|a, b| a.1.incident_id == b.1.incident_id);
    let limit = query.limit.min(10_000);
    if found.len() > limit {
        found.truncate(limit);
        result.truncated = true;
    }
    found
}

fn matches_query(record: &Value, summary: &IncidentReportSummary, query: &LogQuery) -> bool {
    let level = match summary.severity {
        IncidentSeverity::Warning => runtime_log::LogLevel::Warning,
        IncidentSeverity::Error => runtime_log::LogLevel::Error,
        IncidentSeverity::Critical => runtime_log::LogLevel::Critical,
    };
    query.source == LogSource::Ux
        && query.since.is_none_or(|time| summary.occurred_at >= time)
        && query.until.is_none_or(|time| summary.occurred_at <= time)
        && query.min_level.is_none_or(|min| level >= min)
        && query
            .incident_id
            .as_deref()
            .is_none_or(|id| summary.incident_id == id)
        && query.module.as_deref().is_none_or(|module| {
            module == "ux.watchdog"
                || record.get("module").and_then(Value::as_str) == Some(module)
                || summary.component.as_deref() == Some(module)
        })
        && [
            (&query.run_id, "run_id"),
            (&query.operation_id, "operation_id"),
            (&query.task_id, "task_id"),
            (&query.owner_id, "owner_id"),
        ]
        .iter()
        .all(|(expected, key)| {
            expected
                .as_deref()
                .is_none_or(|id| record.get(*key).and_then(Value::as_str) == Some(id))
        })
}
fn summary(record: &Value, path: &Path) -> Option<IncidentReportSummary> {
    let timestamp = DateTime::parse_from_rfc3339(record.get("occurred_at")?.as_str()?)
        .ok()?
        .with_timezone(&Utc);
    let message = record
        .pointer("/error/message")
        .or_else(|| record.pointer("/panic/payload"))
        .and_then(Value::as_str)
        .unwrap_or("Process did not complete a clean shutdown");
    Some(IncidentReportSummary {
        incident_id: runtime_log::sanitize_text(record.get("incident_id")?.as_str()?),
        occurred_at: timestamp,
        kind: serde_json::from_value(record.get("kind")?.clone()).ok()?,
        severity: serde_json::from_value(record.get("severity")?.clone()).ok()?,
        app: record
            .get("app")
            .filter(|v| !v.is_null())
            .and_then(|v| serde_json::from_value(sanitize_value(v, 0)).ok()),
        component: record
            .get("component")
            .and_then(Value::as_str)
            .map(runtime_log::sanitize_text),
        boundary: runtime_log::sanitize_text(record.get("boundary")?.as_str()?),
        summary: runtime_log::sanitize_text(message),
        recovery: serde_json::from_value(sanitize_value(record.get("recovery")?, 0)).ok()?,
        json_report_path: path.to_path_buf(),
        text_report_path: None,
    })
}
fn pick(record: &Value, keys: &[&str]) -> Value {
    Value::Object(
        keys.iter()
            .filter_map(|key| {
                record
                    .get(*key)
                    .filter(|v| !v.is_array() && !v.is_object())
                    .map(|v| ((*key).into(), sanitize_value(v, 0)))
            })
            .collect(),
    )
}
fn safe_report(record: &Value) -> Value {
    let mut result = pick(
        record,
        &[
            "schema_version",
            "incident_id",
            "kind",
            "severity",
            "occurred_at",
            "process_name",
            "process_version",
            "process_id",
            "run_id",
            "app",
            "component",
            "module",
            "task_id",
            "task_group",
            "boundary",
            "operation_id",
            "owner_id",
            "log_event_id",
            "restart_attempt",
            "recovery",
        ],
    );
    if let Some(app) = record.get("app").filter(|v| v.is_object()) {
        result["app"] = pick(app, &["id", "display_name", "version", "criticality"]);
    }
    if let Some(recovery) = record.get("recovery") {
        result["recovery"] = pick(recovery, &["status", "detail"]);
    }
    if let Some(error) = record.get("error") {
        result["error"] = pick(error, &["message", "os_error_code"]);
        if let Some(chain) = error.get("source_chain").and_then(Value::as_array) {
            result["error"]["source_chain"] = Value::Array(
                chain
                    .iter()
                    .filter_map(Value::as_str)
                    .take(32)
                    .map(|text| Value::String(runtime_log::sanitize_text(text)))
                    .collect(),
            );
        }
    }
    if let Some(panic) = record.get("panic") {
        result["panic"] = pick(
            panic,
            &["payload", "source_file", "source_line", "source_column"],
        );
    }
    if let Some(crumbs) = record.get("breadcrumbs").and_then(Value::as_array) {
        result["breadcrumbs"] = Value::Array(
            crumbs
                .iter()
                .rev()
                .take(128)
                .rev()
                .map(|c| {
                    pick(
                        c,
                        &[
                            "recorded_at",
                            "occurred_at",
                            "timestamp",
                            "app_id",
                            "component",
                            "category",
                            "message",
                            "run_id",
                            "task_id",
                            "operation_id",
                            "event_id",
                            "incident_id",
                            "owner_id",
                        ],
                    )
                })
                .collect(),
        );
    }
    if let Some(errors) = record.get("secondary_errors").and_then(Value::as_array) {
        result["secondary_errors"] = Value::Array(
            errors
                .iter()
                .filter_map(Value::as_str)
                .take(32)
                .map(|s| Value::String(runtime_log::sanitize_text(s)))
                .collect(),
        );
    }
    result["notice"] = json!(
        "Sanitized diagnostic fields; opaque runtime state, raw payload attachments and backtraces omitted"
    );
    result
}
fn sanitize_value(value: &Value, depth: usize) -> Value {
    if depth > 8 {
        return Value::Null;
    }
    match value {
        Value::String(text) => Value::String(runtime_log::sanitize_text(text)),
        Value::Array(items) => Value::Array(
            items
                .iter()
                .take(128)
                .map(|v| sanitize_value(v, depth + 1))
                .collect(),
        ),
        Value::Object(fields) => Value::Object(
            fields
                .iter()
                .take(64)
                .filter(|(key, _)| {
                    ![
                        "password",
                        "token",
                        "clipboard",
                        "file_body",
                        "file_content",
                        "authorization",
                    ]
                    .iter()
                    .any(|marker| key.to_ascii_lowercase().contains(marker))
                })
                .map(|(key, value)| {
                    (
                        runtime_log::sanitize_text(key),
                        sanitize_value(value, depth + 1),
                    )
                })
                .collect(),
        ),
        value => value.clone(),
    }
}
