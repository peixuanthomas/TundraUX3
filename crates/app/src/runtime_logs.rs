//! Authorized runtime-log queries shared by CLI and TUI.
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LogAccess {
    User(String),
    Admin,
    /// Standalone CLI: current OS identity and filesystem permissions.
    OsUser,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LogsSnapshot {
    pub result: runtime_log::LogQueryResult,
    pub files: Vec<crate::diagnostics::DiagnosticLogFile>,
    pub incidents: Vec<watchdog::IncidentReportSummary>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LogDocumentSelection {
    Events,
    File(PathBuf),
    Incident(String),
}

mod documents;
mod incidents;
pub use documents::{export_diagnostics, prepare_log_document};
use runtime_log::{LogQuery, LogQueryResult, LogSource, LogSourceState};
use std::{
    fs,
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
};

/// Enforce application ownership before querying, even if a caller supplied a
/// conflicting owner filter. OS CLI access is governed only by OS permissions.
pub fn query_snapshot(
    logs_root: &Path,
    query: &LogQuery,
    access: &LogAccess,
    platform: &dyn platform::Platform,
    cancelled: &AtomicBool,
) -> LogsSnapshot {
    let mut snapshot = LogsSnapshot::default();
    if cancelled.load(Ordering::Relaxed) {
        snapshot.result.state = LogSourceState::Cancelled;
        return snapshot;
    }
    if let Err(error) = validate_root(logs_root) {
        snapshot.result.state = LogSourceState::PermissionDenied;
        snapshot.result.notices.push(error);
        return snapshot;
    }
    let query = authorized_query(query, access);
    if query.source == LogSource::Linux {
        snapshot.result = if matches!(access, LogAccess::User(_))
            && !(platform.kind() == platform::PlatformKind::Linux && platform.is_native_backend()) {
            LogQueryResult {
                state: LogSourceState::PermissionDenied,
                notices: vec!["Linux logs require administrator diagnostics access".into()],
                ..Default::default()
            }
        } else {
            platform.query_linux_logs(&query, cancelled)
        };
        return snapshot;
    }
    let runtime = logs_root.join("runtime");
    snapshot.result = runtime_log::query_logs_cancellable(&runtime, &query, cancelled);
    if cancelled.load(Ordering::Relaxed) {
        snapshot.result.state = LogSourceState::Cancelled;
        snapshot.result.events.clear();
        return snapshot;
    }
    let mut inspected_bytes = 0_u64;
    if let Ok(entries) = fs::read_dir(&runtime) {
        for (index, entry) in entries.enumerate() {
            if cancelled.load(Ordering::Relaxed) {
                snapshot.result.state = LogSourceState::Cancelled;
                break;
            }
            if index >= 4096 {
                snapshot.result.truncated = true;
                partial(
                    &mut snapshot.result,
                    "Runtime file catalog entry limit reached",
                );
                break;
            }
            let Ok(entry) = entry else {
                partial(&mut snapshot.result, "Could not inspect a runtime log file");
                continue;
            };
            if !is_runtime_name(&entry.path()) {
                continue;
            }
            let Ok(meta) = fs::symlink_metadata(entry.path()) else {
                continue;
            };
            if !meta.is_file() || meta.file_type().is_symlink() {
                continue;
            }
            if let LogAccess::User(owner) = access {
                inspected_bytes = inspected_bytes.saturating_add(meta.len());
                if inspected_bytes > 128 * 1024 * 1024 {
                    partial(&mut snapshot.result, "File ownership scan limit reached");
                    break;
                }
                if !file_owned_by(&entry.path(), owner, cancelled) {
                    continue;
                }
            }
            snapshot.files.push(crate::diagnostics::DiagnosticLogFile {
                path: entry.path(),
                relative_path: Path::new("runtime").join(entry.file_name()),
                modified_at: meta.modified().unwrap_or(std::time::UNIX_EPOCH).into(),
                size_bytes: meta.len(),
            });
        }
        snapshot
            .files
            .sort_by(|a, b| b.modified_at.cmp(&a.modified_at));
    }
    snapshot.incidents =
        incidents::catalog(logs_root, &query, access, cancelled, &mut snapshot.result)
            .into_iter()
            .map(|(_, summary)| summary)
            .collect();
    if cancelled.load(Ordering::Relaxed) {
        snapshot.result.state = LogSourceState::Cancelled;
    }
    snapshot
}

fn authorized_query(query: &LogQuery, access: &LogAccess) -> LogQuery {
    let mut query = query.clone();
    if let LogAccess::User(owner) = access {
        query.owner_id = Some(owner.clone());
    }
    query
}
fn validate_root(root: &Path) -> Result<(), String> {
    for path in [
        root.to_path_buf(),
        root.join("runtime"),
        root.join("crashes"),
    ] {
        match fs::symlink_metadata(path) {
            Ok(meta) if meta.file_type().is_symlink() || !meta.is_dir() => {
                return Err("Refusing a non-directory or symlink log root".into());
            }
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
                return Err(runtime_log::sanitize_text(&error.to_string()));
            }
            _ => {}
        }
    }
    Ok(())
}
fn is_runtime_name(path: &Path) -> bool {
    path.file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n.starts_with("runtime-") && n.ends_with(".jsonl"))
}
fn partial(result: &mut LogQueryResult, notice: &str) {
    if result.state == LogSourceState::Ready {
        result.state = LogSourceState::Partial;
    }
    if result.notices.len() < 32 {
        result.notices.push(runtime_log::sanitize_text(notice));
    }
}

#[cfg(test)]
mod tests;

fn file_owned_by(path: &Path, owner: &str, cancelled: &AtomicBool) -> bool {
    let Ok(document) = platform::read_document_prefix_snapshot_limited_with_progress(
        path,
        64 * 1024 * 1024,
        |_, _| !cancelled.load(Ordering::Relaxed),
    ) else {
        return false;
    };
    let mut found = false;
    for line in document
        .bytes
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
    {
        if cancelled.load(Ordering::Relaxed) || line.len() > 64 * 1024 {
            return false;
        }
        let Ok(event) = serde_json::from_slice::<runtime_log::RuntimeLogEvent>(line) else {
            return false;
        };
        if event.schema_version != 1
            || event.source != LogSource::Ux
            || event.context.owner_id.as_deref() != Some(owner)
        {
            return false;
        }
        found = true;
    }
    found
}
