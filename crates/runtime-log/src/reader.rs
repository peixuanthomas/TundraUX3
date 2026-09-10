use crate::{
    LogQuery, LogQueryResult, LogSourceState, LogWriterHealth, RuntimeLogEvent, sanitize_event,
    sanitize_text,
};
use std::{
    fs::{self, OpenOptions},
    io::{self, BufRead, BufReader, Write},
    path::{Path, PathBuf},
};

fn notice(result: &mut LogQueryResult, message: String) {
    result.state = LogSourceState::Partial;
    if result.notices.len() < 32 {
        result.notices.push(sanitize_text(&message));
    }
}
fn matches(event: &RuntimeLogEvent, query: &LogQuery) -> bool {
    event.source == query.source
        && query.since.is_none_or(|time| event.timestamp >= time)
        && query.until.is_none_or(|time| event.timestamp <= time)
        && query.min_level.is_none_or(|level| event.level >= level)
        && query
            .module
            .as_ref()
            .is_none_or(|module| event.context.module == *module)
        && query
            .run_id
            .as_ref()
            .is_none_or(|id| event.context.run_id.as_ref() == Some(id))
        && query
            .operation_id
            .as_ref()
            .is_none_or(|id| event.context.operation_id.as_ref() == Some(id))
        && query
            .task_id
            .as_ref()
            .is_none_or(|id| event.context.task_id.as_ref() == Some(id))
        && query
            .incident_id
            .as_ref()
            .is_none_or(|id| event.incident_id.as_ref() == Some(id))
        && query
            .owner_id
            .as_ref()
            .is_none_or(|id| event.context.owner_id.as_ref() == Some(id))
}
/// Query newest matching records with bounded per-record and result memory.
/// Call from a CLI or background task, never from a terminal drawing callback.
pub fn query_logs(directory: &Path, query: &LogQuery) -> LogQueryResult {
    query_logs_cancellable(directory, query, &std::sync::atomic::AtomicBool::new(false))
}

pub fn query_logs_cancellable(
    directory: &Path,
    query: &LogQuery,
    cancelled: &std::sync::atomic::AtomicBool,
) -> LogQueryResult {
    let mut result = LogQueryResult::default();
    if cancelled.load(std::sync::atomic::Ordering::Relaxed) {
        result.state = LogSourceState::Cancelled;
        return result;
    }
    if query.limit == 0 {
        return result;
    }
    if fs::symlink_metadata(directory).is_ok_and(|meta| meta.file_type().is_symlink()) {
        result.state = LogSourceState::PermissionDenied;
        result.notices.push("Refusing symlink log directory".into());
        return result;
    }
    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return result,
        Err(error) => {
            result.state = if error.kind() == io::ErrorKind::PermissionDenied {
                LogSourceState::PermissionDenied
            } else {
                LogSourceState::Unavailable
            };
            result.notices.push(sanitize_text(&error.to_string()));
            return result;
        }
    };
    let limit = query.limit.min(10_000);
    if query.limit > limit {
        notice(&mut result, "Result limit capped at 10000 records".into());
        result.truncated = true;
    }
    let mut scanned_bytes = 0_u64;
    for entry in entries {
        if cancelled.load(std::sync::atomic::Ordering::Relaxed) {
            result.events.clear();
            result.state = LogSourceState::Cancelled;
            return result;
        }
        let entry = match entry {
            Ok(entry) => entry,
            Err(e) => {
                notice(&mut result, e.to_string());
                continue;
            }
        };
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !name.starts_with("runtime-") || !name.ends_with(".jsonl") {
            continue;
        }
        match entry.file_type() {
            Ok(kind) if kind.is_file() && !kind.is_symlink() => {}
            Ok(_) => {
                notice(&mut result, "Skipped non-regular runtime log file".into());
                continue;
            }
            Err(error) => {
                notice(&mut result, error.to_string());
                continue;
            }
        }
        let mut opts = OpenOptions::new();
        opts.read(true);
        crate::writer::nofollow(&mut opts);
        let file = match opts.open(entry.path()) {
            Ok(file) => file,
            Err(e) => {
                notice(&mut result, e.to_string());
                continue;
            }
        };
        if !file.metadata().is_ok_and(|m| m.is_file()) {
            notice(&mut result, "Skipped non-regular runtime log file".into());
            continue;
        }
        let mut reader = BufReader::new(file);
        let mut line = Vec::new();
        let mut oversized = false;
        loop {
            if cancelled.load(std::sync::atomic::Ordering::Relaxed) {
                result.events.clear();
                result.state = LogSourceState::Cancelled;
                return result;
            }
            let available = match reader.fill_buf() {
                Ok(data) => data,
                Err(error) => {
                    notice(&mut result, error.to_string());
                    break;
                }
            };
            if available.is_empty() {
                if !line.is_empty() || oversized {
                    result.damaged_records += 1;
                    notice(&mut result, "Ignored incomplete trailing log record".into());
                }
                break;
            }
            let end = available.iter().position(|byte| *byte == b'\n');
            let consumed = end.map_or(available.len(), |i| i + 1);
            scanned_bytes += consumed as u64;
            if scanned_bytes > 512 * 1024 * 1024 {
                result.truncated = true;
                notice(&mut result, "Query scan capped at 512 MiB".into());
                return sorted(result);
            }
            if line.len() + consumed <= crate::privacy::MAX_RECORD_BYTES {
                line.extend_from_slice(&available[..consumed]);
            } else {
                oversized = true;
            }
            reader.consume(consumed);
            if end.is_none() {
                continue;
            }
            if oversized {
                result.damaged_records += 1;
                notice(&mut result, "Skipped oversized log record".into());
            } else {
                match serde_json::from_slice::<RuntimeLogEvent>(&line) {
                    Ok(mut event) if event.schema_version == 1 => {
                        if matches(&event, query) {
                            sanitize_event(&mut event);
                            result.events.push(event);
                            if result.events.len() > limit {
                                let oldest = result
                                    .events
                                    .iter()
                                    .enumerate()
                                    .min_by(|(_, a), (_, b)| {
                                        a.timestamp
                                            .cmp(&b.timestamp)
                                            .then_with(|| a.event_id.cmp(&b.event_id))
                                    })
                                    .map(|(i, _)| i)
                                    .unwrap_or(0);
                                result.events.swap_remove(oldest);
                                result.truncated = true;
                            }
                        }
                    }
                    Ok(_) => {
                        result.damaged_records += 1;
                        notice(&mut result, "Unsupported log schema version".into());
                    }
                    Err(_) => {
                        result.damaged_records += 1;
                        notice(&mut result, "Skipped damaged log record".into());
                    }
                }
            }
            line.clear();
            oversized = false;
        }
    }
    sorted(result)
}
fn sorted(mut result: LogQueryResult) -> LogQueryResult {
    result.events.sort_by(|a, b| {
        b.timestamp
            .cmp(&a.timestamp)
            .then_with(|| b.event_id.cmp(&a.event_id))
    });
    result
}
/// Export already-authorized query results into a new private directory.
/// Permission filtering and attachment selection belong to the caller.
pub fn export_logs(
    output: &Path,
    result: &LogQueryResult,
    health: &LogWriterHealth,
) -> io::Result<PathBuf> {
    // create_dir, unlike create_dir_all, refuses existing files or directories.
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        fs::DirBuilder::new().mode(0o700).create(output)?;
    }
    #[cfg(not(unix))]
    fs::create_dir(output)?;
    let mut log = crate::writer::private_create(&output.join("events.jsonl"))?;
    for event in &result.events {
        let mut event = event.clone();
        sanitize_event(&mut event);
        serde_json::to_writer(&mut log, &event)?;
        log.write_all(b"\n")?;
    }
    log.sync_all()?;
    let mut health = health.clone();
    health.last_error = health.last_error.map(|text| sanitize_text(&text));
    let notices: Vec<_> = result
        .notices
        .iter()
        .take(32)
        .map(|text| sanitize_text(text))
        .collect();
    let manifest = serde_json::json!({ "schema_version": 1, "created_at": chrono::Utc::now(), "event_count": result.events.len(), "source_state": result.state, "truncated": result.truncated, "damaged_records": result.damaged_records, "notices": notices, "writer_health": health, "files": ["events.jsonl"] });
    let mut file = crate::writer::private_create(&output.join("manifest.json"))?;
    serde_json::to_writer_pretty(&mut file, &manifest)?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    Ok(output.to_path_buf())
}
