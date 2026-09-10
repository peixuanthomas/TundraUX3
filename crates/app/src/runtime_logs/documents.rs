use super::*;
use serde_json::json;
use std::{
    fs::OpenOptions,
    io::Write,
    sync::{Mutex, atomic::AtomicU64},
    time::{Duration, SystemTime},
};
const MAX_DOCUMENT: usize = 5 * 1024 * 1024;
const MAX_CACHE: u64 = 20 * 1024 * 1024;
static SNAPSHOT_LOCK: Mutex<()> = Mutex::new(());
static NEXT_SNAPSHOT: AtomicU64 = AtomicU64::new(1);

/// Produce a private sanitized document; never hand an original log/report
/// path to the editor. Callers retain their existing read-only editor policy.
pub fn prepare_log_document(
    logs_root: &Path,
    query: &LogQuery,
    access: &LogAccess,
    selection: &LogDocumentSelection,
    platform: &dyn platform::Platform,
    cancelled: &AtomicBool,
) -> Result<PathBuf, String> {
    validate_root(logs_root)?;
    let query = authorized_query(query, access);
    let (content, extension) = match selection {
        LogDocumentSelection::Events => {
            let snapshot = query_snapshot(logs_root, &query, access, platform, cancelled);
            ensure_readable(&snapshot.result)?;
            (event_document(&snapshot.result)?, "jsonl")
        }
        LogDocumentSelection::File(path) => {
            if query.source != LogSource::Ux
                || path.parent() != Some(logs_root.join("runtime").as_path())
                || path
                    .components()
                    .any(|c| matches!(c, std::path::Component::ParentDir))
                || !is_runtime_name(path)
            {
                return Err("Log selection must be a catalogued runtime file".into());
            }
            if !fs::symlink_metadata(path).is_ok_and(|m| m.is_file() && !m.file_type().is_symlink())
            {
                return Err("Selected log is unavailable or is not a regular file".into());
            }
            let document = platform::read_document_prefix_snapshot_limited_with_progress(
                path,
                64 * 1024 * 1024,
                |_, _| !cancelled.load(Ordering::Relaxed),
            )
            .map_err(|error| runtime_log::sanitize_text(&error.to_string()))?;
            let mut result = LogQueryResult::default();
            for line in document
                .bytes
                .split_inclusive(|b| *b == b'\n')
                .filter(|line| !line.is_empty())
            {
                if cancelled.load(Ordering::Relaxed) {
                    return Err("Log document cancelled".into());
                }
                if !line.ends_with(b"\n") {
                    result.damaged_records += 1;
                    partial(&mut result, "Ignored incomplete trailing record");
                    continue;
                }
                if line.len() > 64 * 1024 {
                    result.damaged_records += 1;
                    partial(&mut result, "Skipped oversized record");
                    continue;
                }
                match serde_json::from_slice::<runtime_log::RuntimeLogEvent>(line) {
                    Ok(mut event) if event.schema_version == 1 && event_matches(&event, &query) => {
                        runtime_log::sanitize_event(&mut event);
                        result.events.push(event);
                    }
                    Ok(event) if event.schema_version != 1 => {
                        result.damaged_records += 1;
                        partial(&mut result, "Unsupported record schema");
                    }
                    Ok(_) => {}
                    Err(_) => {
                        result.damaged_records += 1;
                        partial(&mut result, "Skipped damaged record");
                    }
                }
            }
            result.events.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
            if result.events.len() > query.limit.min(10_000) {
                result.events.truncate(query.limit.min(10_000));
                result.truncated = true;
            }
            (event_document(&result)?, "jsonl")
        }
        LogDocumentSelection::Incident(id) => {
            if matches!(access, LogAccess::User(_)) {
                return Err(
                    "Full incident details require administrator diagnostics access".into(),
                );
            }
            let mut selected = query.clone();
            selected.incident_id = Some(id.clone());
            let mut status = LogQueryResult::default();
            let records = incidents::catalog(logs_root, &selected, access, cancelled, &mut status);
            if cancelled.load(Ordering::Relaxed) {
                return Err("Incident document cancelled".into());
            }
            let record = records
                .into_iter()
                .find(|(_, summary)| summary.incident_id == *id)
                .ok_or("Incident is not present in the authorized catalog")?
                .0;
            (
                serde_json::to_vec_pretty(&record).map_err(|e| e.to_string())?,
                "json",
            )
        }
    };
    if cancelled.load(Ordering::Relaxed) {
        return Err("Log document cancelled".into());
    }
    write_snapshot(logs_root, &content, extension)
}

fn event_matches(event: &runtime_log::RuntimeLogEvent, query: &LogQuery) -> bool {
    event.source == query.source
        && query.since.is_none_or(|v| event.timestamp >= v)
        && query.until.is_none_or(|v| event.timestamp <= v)
        && query.min_level.is_none_or(|v| event.level >= v)
        && query
            .module
            .as_deref()
            .is_none_or(|v| event.context.module == v)
        && [
            (&query.owner_id, &event.context.owner_id),
            (&query.run_id, &event.context.run_id),
            (&query.operation_id, &event.context.operation_id),
            (&query.task_id, &event.context.task_id),
            (&query.incident_id, &event.incident_id),
        ]
        .iter()
        .all(|(wanted, actual)| {
            wanted
                .as_deref()
                .is_none_or(|v| actual.as_deref() == Some(v))
        })
}
fn ensure_readable(result: &LogQueryResult) -> Result<(), String> {
    match result.state {
        LogSourceState::Ready | LogSourceState::Partial => Ok(()),
        state => Err(format!(
            "Log source is {state:?}: {}",
            result.notices.join("; ")
        )),
    }
}
fn event_document(result: &LogQueryResult) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    // A structured metadata row keeps partial-source state visible in the
    // read-only document without introducing invalid JSONL comments.
    serde_json::to_writer(&mut bytes,&json!({"document":"runtime_log_snapshot","source_state":result.state,"truncated":result.truncated,"damaged_records":result.damaged_records,"notices":result.notices.iter().map(|n| runtime_log::sanitize_text(n)).collect::<Vec<_>>()})).map_err(|e|e.to_string())?;
    bytes.push(b'\n');
    for event in &result.events {
        let mut event = event.clone();
        runtime_log::sanitize_event(&mut event);
        let line = serde_json::to_vec(&event).map_err(|e| e.to_string())?;
        if bytes.len() + line.len() + 1 > MAX_DOCUMENT {
            return Err("Log document exceeds 5 MiB; narrow the query".into());
        }
        bytes.extend(line);
        bytes.push(b'\n');
    }
    Ok(bytes)
}
fn private_dir(path: &Path) -> Result<(), String> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.is_dir() && !meta.file_type().is_symlink() => {}
        Ok(_) => return Err("Refusing non-directory or symlink snapshot directory".into()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            let mut builder = fs::DirBuilder::new();
            #[cfg(unix)]
            {
                use std::os::unix::fs::DirBuilderExt;
                builder.mode(0o700);
            }
            builder.create(path).map_err(|e| e.to_string())?;
        }
        Err(e) => return Err(e.to_string()),
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700)).map_err(|e| e.to_string())?;
    }
    Ok(())
}
fn private_create(path: &Path) -> Result<fs::File, String> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options
        .open(path)
        .map_err(|e| runtime_log::sanitize_text(&e.to_string()))
}
fn write_snapshot(root: &Path, bytes: &[u8], extension: &str) -> Result<PathBuf, String> {
    if bytes.len() > MAX_DOCUMENT {
        return Err("Log document exceeds 5 MiB; narrow the query".into());
    }
    let _guard = SNAPSHOT_LOCK
        .lock()
        .map_err(|_| "Snapshot cache lock unavailable")?;
    validate_root(root)?;
    // The logs root is application-owned; do not create arbitrary ancestors.
    if !root.exists() {
        private_dir(root)?;
    }
    let runtime = root.join("runtime");
    private_dir(&runtime)?;
    let _quota_guard = runtime_log::reserve_storage_capacity(&runtime, bytes.len() as u64)
        .map_err(|e| e.to_string())?;
    let root_budget = runtime_log::storage_limit(&runtime).map_err(|e| e.to_string())?;
    let directory = runtime.join("snapshots");
    private_dir(&directory)?;
    let mut retained = Vec::new();
    let mut total = 0u64;
    let now = SystemTime::now();
    for (index, entry) in fs::read_dir(&directory)
        .map_err(|e| e.to_string())?
        .enumerate()
    {
        if index >= 4096 {
            return Err("Snapshot cache entry limit reached".into());
        }
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();
        if !entry.file_name().to_string_lossy().starts_with("snapshot-") {
            continue;
        }
        let metadata = fs::symlink_metadata(&path).map_err(|e| e.to_string())?;
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            continue;
        }
        let modified = metadata.modified().unwrap_or(std::time::UNIX_EPOCH);
        if now
            .duration_since(modified)
            .is_ok_and(|age| age > Duration::from_secs(30 * 60))
        {
            fs::remove_file(path).map_err(|e| e.to_string())?;
            continue;
        }
        total = total.saturating_add(metadata.len());
        retained.push((modified, metadata.len(), path));
    }
    let runtime_bytes = fs::read_dir(&runtime)
        .map_err(|e| e.to_string())?
        .filter_map(Result::ok)
        .filter(|e| is_runtime_name(&e.path()))
        .filter_map(|e| e.metadata().ok())
        .filter(|m| m.is_file())
        .map(|m| m.len())
        .fold(0u64, u64::saturating_add);
    let budget = MAX_CACHE.min(root_budget.saturating_sub(runtime_bytes));
    retained.sort_by_key(|item| item.0);
    for (_, size, path) in retained {
        if total.saturating_add(bytes.len() as u64) <= budget {
            break;
        }
        fs::remove_file(path).map_err(|e| e.to_string())?;
        total = total.saturating_sub(size);
    }
    if total.saturating_add(bytes.len() as u64) > budget {
        return Err("Runtime log capacity has no space for this snapshot".into());
    }
    let id = NEXT_SNAPSHOT.fetch_add(1, Ordering::Relaxed);
    let path = directory.join(format!(
        "snapshot-{}-{}-{id}.{extension}",
        std::process::id(),
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
    ));
    let mut file = private_create(&path)?;
    if let Err(error) = file.write_all(bytes).and_then(|_| file.sync_all()) {
        let _ = fs::remove_file(&path);
        return Err(error.to_string());
    }
    Ok(path)
}

/// Export the same authorized view as the TUI. The caller receives source
/// status so partial diagnostic packages never imply complete success.
pub fn export_diagnostics(
    root: &Path,
    query: &LogQuery,
    access: &LogAccess,
    platform: &dyn platform::Platform,
    cancelled: &AtomicBool,
    output: &Path,
) -> Result<LogQueryResult, String> {
    let query = authorized_query(query, access);
    let snapshot = query_snapshot(root, &query, access, platform, cancelled);
    if cancelled.load(Ordering::Relaxed) {
        return Err("Diagnostic export cancelled".into());
    }
    let mut result = snapshot.result;
    let health = runtime_log::global()
        .map(|handle| handle.health())
        .unwrap_or_default();
    runtime_log::export_logs(output, &result, &health)
        .map_err(|e| runtime_log::sanitize_text(&e.to_string()))?;
    let mut files = vec!["events.jsonl".to_string()];
    if query.source == LogSource::Ux && !matches!(access, LogAccess::User(_)) {
        let reports = incidents::catalog(root, &query, access, cancelled, &mut result);
        for (index, (record, _)) in reports.into_iter().enumerate() {
            if cancelled.load(Ordering::Relaxed) {
                result.state = LogSourceState::Cancelled;
                partial(&mut result, "Export cancelled after partial output");
                break;
            }
            let name = format!("incident-{index:04}.json");
            let write = (|| {
                let mut file = private_create(&output.join(&name))?;
                serde_json::to_writer_pretty(&mut file, &record).map_err(|e| e.to_string())?;
                file.sync_all().map_err(|e| e.to_string())
            })();
            match write {
                Ok(()) => files.push(name),
                Err(_) => partial(&mut result, "An associated incident could not be exported"),
            }
        }
    }
    let manifest = json!({"schema_version":1,"created_at":chrono::Utc::now(),"source":query.source,"source_state":result.state,"event_count":result.events.len(),"truncated":result.truncated,"damaged_records":result.damaged_records,"notices":result.notices.iter().map(|n|runtime_log::sanitize_text(n)).collect::<Vec<_>>(),"writer_health":health,"files":files});
    // The directory was just privately created by export_logs; write a new
    // manifest then atomically replace only our own initial manifest.
    let mut file = private_create(&output.join("manifest.final.json"))?;
    serde_json::to_writer_pretty(&mut file, &manifest).map_err(|e| e.to_string())?;
    file.sync_all().map_err(|e| e.to_string())?;
    fs::rename(
        output.join("manifest.final.json"),
        output.join("manifest.json"),
    )
    .map_err(|e| e.to_string())?;
    Ok(result)
}
