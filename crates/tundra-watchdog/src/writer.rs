use crate::config::WatchdogConfig;
use crate::durable;
use crate::report::IncidentRecord;
use crate::sanitize;
use crate::{IncidentReceipt, RecoveryOutcome, RuntimeSnapshot, WatchdogError};
use chrono::Utc;
use serde::Serialize;
use std::collections::HashMap;
use std::fmt::Write as _;
use std::fs::{self, OpenOptions};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::SystemTime;

pub(crate) enum WriterCommand {
    Record {
        incident: IncidentRecord,
        emit: bool,
    },
    RecordAndWait {
        incident: IncidentRecord,
        emit: bool,
        response: mpsc::Sender<Result<IncidentReceipt, String>>,
    },
    Finalize {
        incident_id: String,
        recovery: RecoveryOutcome,
        fallback: IncidentRecord,
        response: mpsc::Sender<Result<IncidentReceipt, String>>,
    },
    Heartbeat(RuntimeSnapshot),
    Shutdown(mpsc::Sender<Result<(), String>>),
}

struct PersistOutcome {
    receipt: IncidentReceipt,
    durable: bool,
}

#[derive(Debug, Clone, Serialize)]
struct RunMarker<'a> {
    schema_version: u32,
    run_id: &'a str,
    process_name: &'a str,
    process_version: &'a str,
    process_id: u32,
    started_at_utc: String,
    last_heartbeat_utc: String,
    snapshot: &'a RuntimeSnapshot,
}

pub(crate) fn writer_loop(
    config: WatchdogConfig,
    run_id: String,
    command_rx: mpsc::Receiver<WriterCommand>,
    incident_tx: mpsc::Sender<IncidentReceipt>,
    ready: mpsc::Sender<Result<(), String>>,
) {
    let marker_path = run_marker_path(&config, &run_id);
    let started_at = Utc::now().to_rfc3339();
    let mut snapshot = RuntimeSnapshot::default();
    match write_marker(&config, &marker_path, &run_id, &started_at, &snapshot) {
        Ok(()) => {
            let _ = ready.send(Ok(()));
        }
        Err(error) => {
            let _ = ready.send(Err(error.to_string()));
            return;
        }
    }
    let mut incidents = HashMap::<String, IncidentRecord>::new();

    while let Ok(command) = command_rx.recv() {
        match command {
            WriterCommand::Record { incident, emit } => {
                let id = incident.incident_id.clone();
                let outcome = persist_incident(&config, &incident);
                if !emit {
                    incidents.insert(id, incident);
                }
                if emit {
                    let _ = incident_tx.send(outcome.receipt);
                }
            }
            WriterCommand::RecordAndWait {
                incident,
                emit,
                response,
            } => {
                let outcome = persist_incident(&config, &incident);
                if emit {
                    let _ = incident_tx.send(outcome.receipt.clone());
                }
                let result = if outcome.durable {
                    Ok(outcome.receipt)
                } else {
                    Err(format!(
                        "incident {} could not be persisted to either report directory",
                        incident.incident_id
                    ))
                };
                let _ = response.send(result);
            }
            WriterCommand::Finalize {
                incident_id,
                recovery,
                fallback,
                response,
            } => {
                let mut incident = incidents.remove(&incident_id).unwrap_or(fallback);
                incident.recovery = recovery;
                let outcome = persist_incident(&config, &incident);
                let _ = incident_tx.send(outcome.receipt.clone());
                let result = if outcome.durable {
                    Ok(outcome.receipt)
                } else {
                    Err(format!(
                        "incident {incident_id} could not be persisted to either report directory"
                    ))
                };
                let _ = response.send(result);
            }
            WriterCommand::Heartbeat(next) => {
                snapshot = sanitize::snapshot(next);
                if let Err(error) =
                    write_marker(&config, &marker_path, &run_id, &started_at, &snapshot)
                {
                    append_emergency(&config, &format!("run marker heartbeat failed: {error}"));
                }
            }
            WriterCommand::Shutdown(response) => {
                let result = durable::remove_file(&marker_path).map_err(|error| error.to_string());
                let _ = response.send(result);
                return;
            }
        }
    }
}

fn persist_incident(config: &WatchdogConfig, incident: &IncidentRecord) -> PersistOutcome {
    let mut record = sanitize_incident(incident.clone());
    match write_report_pair(&config.report_dir, &record) {
        Ok((json, text)) => {
            enforce_retention(config, &config.report_dir);
            PersistOutcome {
                receipt: record.receipt(Some(json), Some(text)),
                durable: true,
            }
        }
        Err(primary_error) => {
            record.secondary_errors.push(primary_error.to_string());
            match write_report_pair(&config.fallback_dir, &record) {
                Ok((json, text)) => {
                    enforce_retention(config, &config.fallback_dir);
                    PersistOutcome {
                        receipt: record.receipt(Some(json), Some(text)),
                        durable: true,
                    }
                }
                Err(fallback_error) => {
                    let message = format!(
                        "watchdog could not persist incident {}: {primary_error}; fallback: {fallback_error}",
                        record.incident_id
                    );
                    let _ = writeln!(std::io::stderr(), "{message}");
                    PersistOutcome {
                        receipt: record.receipt(None, None),
                        durable: false,
                    }
                }
            }
        }
    }
}

fn sanitize_incident(mut incident: IncidentRecord) -> IncidentRecord {
    incident.boundary = sanitize::text(incident.boundary);
    incident.task_group = incident.task_group.map(sanitize::text);
    incident.operation_id = incident.operation_id.map(sanitize::text);
    incident.recovery_handler_version =
        incident.recovery_handler_version.map(sanitize::text);
    if let Some(panic) = &mut incident.panic {
        panic.payload = sanitize::text(&panic.payload);
    }
    if let Some(error) = &mut incident.error {
        error.message = sanitize::text(&error.message);
        error.source_chain = error.source_chain.iter().map(sanitize::text).collect();
    }
    incident.runtime = sanitize::snapshot(incident.runtime);
    incident.breadcrumbs = incident
        .breadcrumbs
        .into_iter()
        .map(sanitize::breadcrumb)
        .collect();
    incident.recovery = sanitize::recovery(incident.recovery);
    incident.secondary_errors = incident
        .secondary_errors
        .iter()
        .map(sanitize::text)
        .collect();
    incident
}

fn write_report_pair(
    directory: &Path,
    incident: &IncidentRecord,
) -> Result<(PathBuf, PathBuf), WatchdogError> {
    fs::create_dir_all(directory).map_err(|source| WatchdogError::Io {
        operation: "create crash report directory",
        path: directory.to_path_buf(),
        source,
    })?;
    let json_path = directory.join(format!("{}.json", incident.report_stem));
    let text_path = directory.join(format!("{}.txt", incident.report_stem));
    let json = serde_json::to_vec_pretty(incident)?;
    atomic_write(&json_path, &json)?;
    atomic_write(&text_path, render_text(incident).as_bytes())?;
    Ok((json_path, text_path))
}

fn render_text(incident: &IncidentRecord) -> String {
    let mut text = String::new();
    let _ = writeln!(text, "TundraUX3 crash report");
    let _ = writeln!(text, "Incident: {}", incident.incident_id);
    let _ = writeln!(text, "Occurred: {}", incident.occurred_at.to_rfc3339());
    let _ = writeln!(text, "Kind: {:?} / {:?}", incident.kind, incident.severity);
    let _ = writeln!(
        text,
        "Process: {} {} (pid {})",
        incident.process_name, incident.process_version, incident.process_id
    );
    let _ = writeln!(text, "Run: {}", incident.run_id);
    if let Some(app) = &incident.app {
        let _ = writeln!(text, "App: {} ({})", app.display_name, app.id);
    }
    if let Some(component) = &incident.component {
        let _ = writeln!(text, "Component: {component}");
    }
    if let Some(task) = &incident.task_id {
        let _ = writeln!(text, "Task: {task}");
    }
    if let Some(group) = &incident.task_group {
        let _ = writeln!(text, "Task group: {group}");
    }
    if let Some(kind) = incident.task_kind {
        let _ = writeln!(text, "Task kind: {kind:?}");
    }
    if let Some(action) = incident.panic_action {
        let _ = writeln!(text, "Panic action: {action:?}");
    }
    if let Some(replay_safety) = &incident.replay_safety {
        let _ = writeln!(text, "Replay safety: {replay_safety:?}");
    }
    if let Some(policy) = &incident.restart_policy {
        let _ = writeln!(
            text,
            "Restart: attempt {}, max {} in {:?}, backoff {:?}",
            incident.restart_attempt, policy.max_restarts, policy.window, policy.backoff
        );
    }
    if let Some(operation) = &incident.operation_kind {
        let _ = writeln!(text, "Operation kind: {operation}");
    }
    if let Some(operation) = &incident.operation_id {
        let _ = writeln!(text, "Operation ID: {operation}");
    }
    if let Some(version) = &incident.recovery_handler_version {
        let _ = writeln!(text, "Recovery handler: {version}");
    }
    let _ = writeln!(
        text,
        "Boundary: {} ({:?})",
        incident.boundary, incident.boundary_kind
    );
    let _ = writeln!(
        text,
        "Thread: {} ({})",
        incident.thread_name.as_deref().unwrap_or("unnamed"),
        incident.thread_id
    );
    if let Some(panic) = &incident.panic {
        let _ = writeln!(text, "\nPanic: {}", panic.payload);
        if let Some(file) = &panic.source_file {
            let _ = writeln!(
                text,
                "Location: {}:{}:{}",
                file,
                panic.source_line.unwrap_or(0),
                panic.source_column.unwrap_or(0)
            );
        }
        let _ = writeln!(text, "\nBacktrace:\n{}", panic.backtrace);
    }
    if let Some(error) = &incident.error {
        let _ = writeln!(text, "\nError: {}", error.message);
        for source in &error.source_chain {
            let _ = writeln!(text, "Caused by: {source}");
        }
        let _ = writeln!(text, "\nBacktrace:\n{}", error.backtrace);
    }
    let _ = writeln!(text, "\nRecovery: {:?}", incident.recovery);
    let _ = writeln!(text, "Runtime snapshot: {:?}", incident.runtime);
    if !incident.breadcrumbs.is_empty() {
        let _ = writeln!(text, "\nRecent breadcrumbs:");
        for breadcrumb in &incident.breadcrumbs {
            let _ = writeln!(
                text,
                "- {} [{}] {}",
                breadcrumb.recorded_at.to_rfc3339(),
                breadcrumb.category,
                breadcrumb.message
            );
        }
    }
    if !incident.secondary_errors.is_empty() {
        let _ = writeln!(text, "\nSecondary errors:");
        for error in &incident.secondary_errors {
            let _ = writeln!(text, "- {error}");
        }
    }
    text
}

fn write_marker(
    config: &WatchdogConfig,
    path: &Path,
    run_id: &str,
    started_at: &str,
    snapshot: &RuntimeSnapshot,
) -> Result<(), WatchdogError> {
    let marker = RunMarker {
        schema_version: 1,
        run_id,
        process_name: &config.process_name,
        process_version: &config.process_version,
        process_id: std::process::id(),
        started_at_utc: started_at.to_string(),
        last_heartbeat_utc: Utc::now().to_rfc3339(),
        snapshot,
    };
    let bytes = serde_json::to_vec_pretty(&marker)?;
    atomic_write(path, &bytes)
}

fn run_marker_path(config: &WatchdogConfig, run_id: &str) -> PathBuf {
    config
        .data_dir
        .join("watchdog")
        .join("runs")
        .join(format!("{run_id}.active.json"))
}

pub(crate) fn emergency_log_path(config: &WatchdogConfig) -> PathBuf {
    config.fallback_dir.join("emergency-panic.log")
}

pub(crate) fn append_emergency(config: &WatchdogConfig, line: &str) {
    let path = emergency_log_path(config);
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if fs::metadata(&path)
        .map(|metadata| metadata.len() >= config.retention.emergency_log_max_bytes)
        .unwrap_or(false)
    {
        let rotated = path.with_extension("log.old");
        let _ = fs::remove_file(&rotated);
        let _ = fs::rename(&path, rotated);
    }
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(&path) {
        let line = sanitize::text(line);
        let _ = writeln!(file, "{} {line}", Utc::now().to_rfc3339());
        let _ = file.flush();
        let _ = file.sync_data();
    }
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), WatchdogError> {
    durable::atomic_write(path, bytes).map_err(|source| WatchdogError::Io {
        operation: "atomically replace watchdog file",
        path: path.to_path_buf(),
        source,
    })
}

fn enforce_retention(config: &WatchdogConfig, directory: &Path) {
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };
    let mut reports = entries
        .filter_map(Result::ok)
        .filter(|entry| entry.path().extension().and_then(|value| value.to_str()) == Some("json"))
        .filter_map(|entry| {
            let metadata = entry.metadata().ok()?;
            let json = entry.path();
            let text_size = fs::metadata(json.with_extension("txt"))
                .map(|metadata| metadata.len())
                .unwrap_or(0);
            Some((json, metadata.modified().ok()?, metadata.len() + text_size))
        })
        .collect::<Vec<_>>();
    reports.sort_by_key(|(_, modified, _)| *modified);
    let now = SystemTime::now();
    let mut total = reports.iter().map(|(_, _, size)| *size).sum::<u64>();
    let mut remaining = reports.len();
    for (json, modified, size) in reports {
        let too_old = now.duration_since(modified).unwrap_or_default() > config.retention.max_age;
        let too_many = remaining > config.retention.max_incidents;
        let too_large = total > config.retention.max_total_bytes;
        if too_old || too_many || too_large {
            let text = json.with_extension("txt");
            let _ = fs::remove_file(&json);
            let _ = fs::remove_file(text);
            total = total.saturating_sub(size);
            remaining = remaining.saturating_sub(1);
        }
    }
}
