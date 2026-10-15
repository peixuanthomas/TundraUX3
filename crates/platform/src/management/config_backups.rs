//! Protected operation history shared by configuration writers and recovery.
use super::system_config::{self, StoredFile};
use super::*;
use serde_json::Value;
use std::fs::{self, File};
use std::io::Read;
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::Path;
use std::sync::atomic::Ordering;
use std::time::{SystemTime, UNIX_EPOCH};

const ROOT: &str = "/var/lib/tundraux3/config-backups";
const MAX_RECORD: u64 = 16 * 1024 * 1024;

#[derive(Clone, Serialize, Deserialize)]
struct Record {
    format_version: u32,
    id: String,
    source: String,
    timestamp: u64,
    before: Vec<StoredFile>,
    after: Option<Vec<StoredFile>>,
    result: String,
    #[serde(default)]
    payload: Value,
}

pub struct BackupSession {
    id: String,
    root: PathBuf,
}

fn error(e: impl std::fmt::Display) -> ManagementError {
    ManagementError::Failed(e.to_string())
}
fn check_cancelled(flag: &AtomicBool) -> Result<(), ManagementError> {
    if flag.load(Ordering::Acquire) {
        Err(ManagementError::Cancelled)
    } else {
        Ok(())
    }
}
fn record_path(root: &Path, id: &str) -> Result<PathBuf, ManagementError> {
    if id.is_empty() || id.len() > 100 || !id.bytes().all(|b| b.is_ascii_digit() || b == b'-') {
        return Err(ManagementError::InvalidInput("Invalid backup ID".into()));
    }
    Ok(root.join("operations").join(format!("{id}.json")))
}
fn write_record(root: &Path, record: &Record) -> Result<(), ManagementError> {
    let (base, _) = system_config::parent(&root.join("record"), true)?;
    let m = base.metadata().map_err(error)?;
    if m.uid() != unsafe { libc::geteuid() } || m.mode() & 0o022 != 0 {
        return Err(ManagementError::PermissionDenied(
            "Unsafe backup directory".into(),
        ));
    }
    base.set_permissions(fs::Permissions::from_mode(0o700))
        .map_err(error)?;
    let path = record_path(root, &record.id)?;
    let (directory, name) = system_config::parent(&path, true)?;
    let metadata = directory.metadata().map_err(error)?;
    if metadata.uid() != unsafe { libc::geteuid() } || metadata.mode() & 0o022 != 0 {
        return Err(ManagementError::PermissionDenied(
            "Unsafe backup directory".into(),
        ));
    }
    directory
        .set_permissions(fs::Permissions::from_mode(0o700))
        .map_err(error)?;
    let mut temp =
        tempfile::NamedTempFile::new_in(format!("/proc/self/fd/{}", directory.as_raw_fd()))
            .map_err(error)?;
    temp.as_file()
        .set_permissions(fs::Permissions::from_mode(0o600))
        .map_err(error)?;
    serde_json::to_writer(&mut temp, record).map_err(error)?;
    temp.as_file().sync_all().map_err(error)?;
    let temporary = std::ffi::CString::new(temp.path().file_name().unwrap().as_encoded_bytes())
        .map_err(error)?;
    if unsafe {
        libc::renameat(
            directory.as_raw_fd(),
            temporary.as_ptr(),
            directory.as_raw_fd(),
            name.as_ptr(),
        )
    } != 0
    {
        return Err(error(std::io::Error::last_os_error()));
    }
    directory.sync_all().map_err(error)
}
fn read_record(root: &Path, id: &str) -> Result<Record, ManagementError> {
    let path = record_path(root, id)?;
    let (directory, name) = system_config::parent(&path, false)?;
    let fd = unsafe {
        libc::openat(
            directory.as_raw_fd(),
            name.as_ptr(),
            libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if fd < 0 {
        return Err(error(std::io::Error::last_os_error()));
    }
    let file = unsafe { File::from_raw_fd(fd) };
    let m = file.metadata().map_err(error)?;
    if !m.is_file()
        || m.uid() != unsafe { libc::geteuid() }
        || m.mode() & 0o077 != 0
        || m.len() > MAX_RECORD
    {
        return Err(ManagementError::PermissionDenied(
            "Unsafe backup record".into(),
        ));
    }
    let record: Record = serde_json::from_reader(file.take(MAX_RECORD)).map_err(error)?;
    if record.id != id || record.format_version != 1 {
        return Err(ManagementError::InvalidInput(
            "Unsupported backup record".into(),
        ));
    }
    Ok(record)
}
fn capture(paths: &[PathBuf]) -> Result<Vec<StoredFile>, ManagementError> {
    paths
        .iter()
        .map(|path| system_config::read_stored(path))
        .collect()
}
pub fn begin(source: &str, paths: &[PathBuf]) -> Result<BackupSession, ManagementError> {
    begin_at(Path::new(ROOT), source, paths)
}
pub(crate) fn begin_from_backup(
    source: &str,
    id: &str,
    extra_paths: &[PathBuf],
) -> Result<BackupSession, ManagementError> {
    let root = Path::new(ROOT);
    let selected = read_record(root, id)?;
    verify_current(&selected, None)?;
    let mut before = selected.before;
    for path in extra_paths {
        if !before.iter().any(|file| &file.document.path == path) {
            before.push(system_config::read_stored(path)?);
        }
    }
    let time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(error)?;
    let id = format!("{}-{}", time.as_nanos(), std::process::id());
    let after = Some(capture(
        &before
            .iter()
            .map(|file| file.document.path.clone())
            .collect::<Vec<_>>(),
    )?);
    write_record(
        root,
        &Record {
            format_version: 1,
            id: id.clone(),
            source: source.into(),
            timestamp: time.as_secs(),
            before,
            after,
            result: "pending".into(),
            payload: Value::Null,
        },
    )?;
    Ok(BackupSession {
        id,
        root: root.into(),
    })
}
fn begin_at(
    root: &Path,
    source: &str,
    paths: &[PathBuf],
) -> Result<BackupSession, ManagementError> {
    let time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(error)?;
    let id = format!("{}-{}", time.as_nanos(), std::process::id());
    let record = Record {
        format_version: 1,
        id: id.clone(),
        source: source.into(),
        timestamp: time.as_secs(),
        before: capture(paths)?,
        after: None,
        result: "pending".into(),
        payload: Value::Null,
    };
    write_record(root, &record)?;
    Ok(BackupSession {
        id,
        root: root.into(),
    })
}
impl BackupSession {
    pub fn id(&self) -> &str {
        &self.id
    }
    pub fn set_payload(&self, payload: Value) -> Result<(), ManagementError> {
        let mut record = read_record(&self.root, &self.id)?;
        record.payload = payload;
        write_record(&self.root, &record)
    }
    pub fn finish(&self, result: &str) -> Result<(), ManagementError> {
        finish_at(&self.root, &self.id, result)
    }
}
pub fn finish(id: &str, result: &str) -> Result<(), ManagementError> {
    finish_at(Path::new(ROOT), id, result)
}
pub(crate) fn set_result(id: &str, result: &str) -> Result<(), ManagementError> {
    let root = Path::new(ROOT);
    let mut record = read_record(root, id)?;
    record.result = result.into();
    write_record(root, &record)
}
pub(crate) fn payload(id: &str) -> Result<Value, ManagementError> {
    Ok(read_record(Path::new(ROOT), id)?.payload)
}
pub(crate) fn set_payload(id: &str, payload: Value) -> Result<(), ManagementError> {
    let root = Path::new(ROOT);
    let mut record = read_record(root, id)?;
    record.payload = payload;
    write_record(root, &record)
}
pub(crate) fn snapshots(
    id: &str,
) -> Result<Vec<(ConfigDocument, ConfigDocument)>, ManagementError> {
    let record = read_record(Path::new(ROOT), id)?;
    let after = record.after.ok_or_else(|| {
        ManagementError::Conflict("The configuration operation has no verified result".into())
    })?;
    Ok(record
        .before
        .into_iter()
        .zip(after)
        .map(|(before, after)| (before.document, after.document))
        .collect())
}
pub(crate) fn validate_current(id: &str) -> Result<(), ManagementError> {
    verify_current(&read_record(Path::new(ROOT), id)?, None).map(|_| ())
}
pub(crate) fn append_created_path(id: &str, path: &Path) -> Result<(), ManagementError> {
    if !path.starts_with("/etc/NetworkManager/system-connections")
        || path.extension().and_then(|s| s.to_str()) != Some("nmconnection")
    {
        return Err(ManagementError::InvalidInput(
            "Unexpected new network profile path".into(),
        ));
    }
    let root = Path::new(ROOT);
    let mut record = read_record(root, id)?;
    if record.before.iter().any(|file| file.document.path == path) {
        return Ok(());
    }
    let mut missing = system_config::read_stored(path)?;
    missing.document.content.clear();
    missing.document.version = "missing".into();
    missing.document.existed = false;
    missing.attributes.clear();
    record.before.push(missing);
    write_record(root, &record)
}
pub(super) fn finish_at(root: &Path, id: &str, result: &str) -> Result<(), ManagementError> {
    let mut record = read_record(root, id)?;
    record.after = Some(capture(
        &record
            .before
            .iter()
            .map(|s| s.document.path.clone())
            .collect::<Vec<_>>(),
    )?);
    record.result = result.into();
    write_record(root, &record)
}
pub(super) fn start_legacy(
    root: &Path,
    id: &str,
    before: &StoredFile,
) -> Result<(), ManagementError> {
    write_record(
        root,
        &Record {
            format_version: 1,
            id: id.into(),
            source: "system-config".into(),
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(error)?
                .as_secs(),
            before: vec![before.clone()],
            after: None,
            result: "pending".into(),
            payload: Value::Null,
        },
    )
}

fn verify_current(
    record: &Record,
    candidates: Option<&BTreeMap<PathBuf, (String, bool)>>,
) -> Result<Vec<StoredFile>, ManagementError> {
    let current = capture(
        &record
            .before
            .iter()
            .map(|s| s.document.path.clone())
            .collect::<Vec<_>>(),
    )?;
    for (index, actual) in current.iter().enumerate() {
        let before = &record.before[index];
        let after = record.after.as_ref().and_then(|files| {
            files
                .iter()
                .find(|file| file.document.path == actual.document.path)
        });
        let properties_match = |known: &StoredFile| {
            actual.document.uid == known.document.uid
                && actual.document.gid == known.document.gid
                && actual.document.mode == known.document.mode
                && actual.attributes == known.attributes
        };
        let permitted = if let Some(candidates) = candidates {
            actual.document.existed == before.document.existed
                && actual.document.content == before.document.content
                && actual.document.uid == before.document.uid
                && actual.document.gid == before.document.gid
                && actual.document.mode == before.document.mode
                && actual.attributes == before.attributes
                || after.is_some_and(|after| actual.document.version == after.document.version)
                || candidates
                    .get(&actual.document.path)
                    .is_some_and(|(content, existed)| {
                        actual.document.existed == *existed
                            && actual.document.content == *content
                            && (!*existed
                                || before.document.existed && properties_match(before)
                                || after.is_some_and(|after| {
                                    after.document.existed && properties_match(after)
                                })
                                || !before.document.existed
                                    && after.is_none_or(|after| !after.document.existed)
                                    && actual.document.uid == before.document.uid
                                    && actual.document.gid == before.document.gid
                                    && matches!(actual.document.mode, 0o600 | 0o644))
                    })
        } else {
            record
                .after
                .as_ref()
                .and_then(|files| files.get(index))
                .is_some_and(|after| actual.document.version == after.document.version)
        };
        if !permitted {
            return Err(ManagementError::Conflict(format!(
                "Configuration changed after this operation: {}",
                actual.document.path.display()
            )));
        }
    }
    Ok(current)
}
fn restore_record(
    root: &Path,
    record: &Record,
    candidates: Option<&BTreeMap<PathBuf, (String, bool)>>,
) -> Result<(), ManagementError> {
    let current = verify_current(record, candidates)?;
    let session = begin_at(
        root,
        "restore",
        &current
            .iter()
            .map(|s| s.document.path.clone())
            .collect::<Vec<_>>(),
    )?;
    let outcome = (|| {
        for (before, actual) in record.before.iter().zip(&current) {
            system_config::restore_snapshot(before, &actual.document.version)?;
        }
        Ok(())
    })();
    if outcome.is_ok() {
        session.finish("restored")?;
    } else {
        let mut failed = read_record(root, session.id())?;
        failed.result = "restore failed".into();
        write_record(root, &failed)?;
    }
    outcome
}
pub fn restore(id: &str) -> Result<(), ManagementError> {
    let root = Path::new(ROOT);
    restore_record(root, &read_record(root, id)?, None)
}
pub(crate) fn restore_files_unrecorded(id: &str) -> Result<(), ManagementError> {
    let record = read_record(Path::new(ROOT), id)?;
    let current = verify_current(&record, None)?;
    for (before, actual) in record.before.iter().zip(current) {
        system_config::restore_snapshot(before, &actual.document.version)?;
    }
    Ok(())
}
pub fn restore_pending(
    id: &str,
    candidates: &BTreeMap<PathBuf, String>,
) -> Result<(), ManagementError> {
    let files = candidates
        .iter()
        .map(|(path, content)| (path.clone(), (content.clone(), true)))
        .collect();
    restore_pending_files(id, &files)
}
pub fn restore_pending_files(
    id: &str,
    candidates: &BTreeMap<PathBuf, (String, bool)>,
) -> Result<(), ManagementError> {
    let root = Path::new(ROOT);
    restore_record(root, &read_record(root, id)?, Some(candidates))
}
pub(crate) fn restore_pending_files_unrecorded(
    id: &str,
    candidates: &BTreeMap<PathBuf, (String, bool)>,
) -> Result<(), ManagementError> {
    let record = read_record(Path::new(ROOT), id)?;
    let current = verify_current(&record, Some(candidates))?;
    for (before, actual) in record.before.iter().zip(current) {
        system_config::restore_snapshot(before, &actual.document.version)?;
    }
    Ok(())
}
pub(crate) fn restore_selected_files_unrecorded(
    id: &str,
    paths: &[PathBuf],
) -> Result<(), ManagementError> {
    restore_selected(id, paths, None)
}
pub(crate) fn restore_pending_selected_files_unrecorded(
    id: &str,
    candidates: &BTreeMap<PathBuf, (String, bool)>,
) -> Result<(), ManagementError> {
    restore_selected(
        id,
        &candidates.keys().cloned().collect::<Vec<_>>(),
        Some(candidates),
    )
}
fn restore_selected(
    id: &str,
    paths: &[PathBuf],
    candidates: Option<&BTreeMap<PathBuf, (String, bool)>>,
) -> Result<(), ManagementError> {
    let mut record = read_record(Path::new(ROOT), id)?;
    if paths
        .iter()
        .any(|path| !record.before.iter().any(|file| &file.document.path == path))
    {
        return Err(ManagementError::InvalidInput(
            "Recovery contains an unrecorded configuration file".into(),
        ));
    }
    record
        .before
        .retain(|file| paths.contains(&file.document.path));
    if let Some(after) = &mut record.after {
        after.retain(|file| paths.contains(&file.document.path));
    }
    let current = verify_current(&record, candidates)?;
    for (before, actual) in record.before.iter().zip(current) {
        system_config::restore_snapshot(before, &actual.document.version)?;
    }
    Ok(())
}

fn safe_text(path: &Path, content: &str) -> String {
    if matches!(
        path.file_name().and_then(|s| s.to_str()),
        Some("shadow" | "gshadow")
    ) {
        content
            .lines()
            .map(|line| {
                let mut fields = line.split(':').map(String::from).collect::<Vec<_>>();
                if fields.len() > 1 {
                    fields[1] = "[protected]".into();
                }
                fields.join(":")
            })
            .collect::<Vec<_>>()
            .join("\n")
    } else if path.starts_with("/etc/NetworkManager/system-connections")
        || path.starts_with("/etc/sysconfig/network-scripts")
    {
        "[protected network configuration]".into()
    } else {
        content.into()
    }
}
fn differences(record: &Record) -> String {
    let mut text = String::new();
    for (i, before) in record.before.iter().enumerate() {
        let after = record.after.as_ref().and_then(|v| v.get(i));
        let original = safe_text(&before.document.path, &before.document.content);
        let updated = after
            .map(|s| safe_text(&s.document.path, &s.document.content))
            .unwrap_or_else(|| "[result not recorded]".into());
        text.push_str(&format!(
            "{}\n--- before\n+++ after\n",
            before.document.path.display()
        ));
        for line in original.lines() {
            text.push_str(&format!("-{line}\n"));
        }
        for line in updated.lines() {
            text.push_str(&format!("+{line}\n"));
        }
        if let Some(after) = after {
            for (label, old, new) in [
                ("UID", before.document.uid, after.document.uid),
                ("GID", before.document.gid, after.document.gid),
                ("Mode", before.document.mode, after.document.mode),
            ] {
                if old != new {
                    text.push_str(&format!("{label}: {old} -> {new}\n"));
                }
            }
            if original == updated && before.document.content != after.document.content {
                text.push_str("Protected credentials changed\n");
            }
            if before.attributes != after.attributes {
                text.push_str("File access attributes changed\n");
            }
            if before.document.existed != after.document.existed {
                text.push_str(if after.document.existed {
                    "File created\n"
                } else {
                    "File removed\n"
                });
            }
        }
    }
    if record.source == "users" {
        text.push_str(&super::users::describe_backup(&record.payload));
    }
    if record.source == "firewall" {
        if let Some(changes) = record.payload["changes"].as_array() {
            for change in changes {
                text.push_str(&format!(
                    "{} / {} / {} {}: {} -> {}\n",
                    change["zone"].as_str().unwrap_or(""),
                    change["scope"].as_str().unwrap_or(""),
                    change["kind"].as_str().unwrap_or(""),
                    change["value"].as_str().unwrap_or(""),
                    change["before"],
                    change["after"]
                ));
            }
        }
        if record.payload["ufw_before"] != record.payload["ufw_after"] {
            text.push_str(&format!(
                "UFW\n--- before\n{}\n+++ after\n{}\n",
                record.payload["ufw_before"].as_str().unwrap_or(""),
                record.payload["ufw_after"].as_str().unwrap_or("")
            ));
        }
    }
    if record.source == "scheduled-tasks" {
        let render = |value: &Value| {
            value
                .as_str()
                .map(String::from)
                .unwrap_or_else(|| value.to_string())
        };
        text.push_str(&format!(
            "{} {} {}\n--- before\n{}\n+++ after\n{}\n",
            record.payload["type"].as_str().unwrap_or(""),
            record.payload["timer"].as_str().unwrap_or(""),
            record.payload["user"].as_str().unwrap_or(""),
            render(&record.payload["before"]),
            render(&record.payload["after"])
        ));
    }
    text
}
fn operation_subject(record: &Record) -> String {
    let paths = record
        .before
        .iter()
        .map(|s| s.document.path.display().to_string())
        .collect::<Vec<_>>()
        .join(", ");
    if !paths.is_empty() {
        return paths;
    }
    ["target", "timer", "user", "backend", "transaction_id"]
        .iter()
        .filter_map(|key| record.payload[*key].as_str())
        .collect::<Vec<_>>()
        .join(", ")
}
fn can_restore(record: &Record) -> bool {
    record.after.is_some()
        && record.result != "pending"
        && !record.result.starts_with("awaiting SSH")
}
pub fn query(
    query: &ManagementQuery,
    flag: &AtomicBool,
) -> Result<ManagementSnapshot, ManagementError> {
    query_at(Path::new(ROOT), query, flag)
}
fn query_at(
    root: &Path,
    query: &ManagementQuery,
    flag: &AtomicBool,
) -> Result<ManagementSnapshot, ManagementError> {
    let mut snapshot = ManagementSnapshot {
        backend: "Configuration backups".into(),
        columns: vec![
            "Recovery".into(),
            "Time".into(),
            "Source".into(),
            "File".into(),
            "Result".into(),
        ],
        ..Default::default()
    };
    let directory = root.join("operations");
    let entries = match fs::read_dir(&directory) {
        Ok(v) => v,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return legacy_rows(root, query, snapshot, flag);
        }
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
            snapshot.actions.push(ManagementAction {
                id: "inspect_backups".into(),
                label: "Authorize backup inspection".into(),
                privileged: true,
                ..Default::default()
            });
            return Ok(snapshot);
        }
        Err(e) => return Err(error(e)),
    };
    let mut ids = Vec::new();
    for entry in entries {
        check_cancelled(flag)?;
        let entry = entry.map_err(error)?;
        if entry.path().extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        let id = entry
            .path()
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        let record = read_record(root, &id)?;
        ids.push(id.clone());
        let paths = operation_subject(&record);
        if query.target.as_ref().is_some_and(|t| t != &id)
            || !format!("{} {} {}", record.source, paths, record.result)
                .to_lowercase()
                .contains(&query.filter.to_lowercase())
        {
            continue;
        }
        let time = chrono::DateTime::from_timestamp(record.timestamp as i64, 0)
            .map(|t| {
                t.with_timezone(&chrono::Local)
                    .format("%Y-%m-%d %H:%M:%S")
                    .to_string()
            })
            .unwrap_or_default();
        let mut actions = vec![ManagementAction {
            id: "preview_restore".into(),
            label: "View differences".into(),
            privileged: true,
            ..Default::default()
        }];
        actions.push(ManagementAction {
            id: "restore".into(),
            label: "Restore".into(),
            privileged: true,
            confirm: true,
            primary: true,
            disabled_reason: (!can_restore(&record))
                .then(|| "Operation is unfinished; inspect or confirm it first".into()),
            fields: vec![ManagementField {
                id: "allow_unvalidated".into(),
                label: "Restore without an available checker".into(),
                value: "false".into(),
                choices: vec!["false".into(), "true".into()],
                ..Default::default()
            }],
            ..Default::default()
        });
        let difference = differences(&record);
        snapshot.rows.push(ManagementRow {
            id: id.clone(),
            cells: vec![id.clone(), time, record.source, paths, record.result],
            detail: vec![("Difference".into(), difference)],
            identity: BTreeMap::from([("backup_id".into(), id)]),
            actions,
        });
    }
    snapshot.rows.sort_by(|a, b| b.id.cmp(&a.id));
    legacy_rows_excluding(root, query, snapshot, flag, &ids)
}
fn legacy_rows(
    root: &Path,
    query: &ManagementQuery,
    snapshot: ManagementSnapshot,
    flag: &AtomicBool,
) -> Result<ManagementSnapshot, ManagementError> {
    legacy_rows_excluding(root, query, snapshot, flag, &[])
}
fn legacy_rows_excluding(
    root: &Path,
    query: &ManagementQuery,
    mut snapshot: ManagementSnapshot,
    flag: &AtomicBool,
    ids: &[String],
) -> Result<ManagementSnapshot, ManagementError> {
    let entries = match fs::read_dir(root) {
        Ok(v) => v,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(snapshot),
        Err(e) => return Err(error(e)),
    };
    for entry in entries {
        check_cancelled(flag)?;
        let path = entry.map_err(error)?.path();
        if path.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        let id = path
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        if ids.contains(&id) || query.target.as_ref().is_some_and(|t| t != &id) {
            continue;
        }
        let stored = system_config::read_backup(&path)?;
        let file = stored.document.path.display().to_string();
        if !file.to_lowercase().contains(&query.filter.to_lowercase()) {
            continue;
        }
        let current = system_config::read(&stored.document.path)?;
        snapshot.rows.push(ManagementRow {
            id: id.clone(),
            cells: vec![
                id.clone(),
                "Unknown".into(),
                "system-config (legacy)".into(),
                file.clone(),
                "Unknown".into(),
            ],
            detail: vec![
                ("File".into(), file.clone()),
                (
                    "Original".into(),
                    safe_text(&stored.document.path, &stored.document.content),
                ),
                ("Current".into(), safe_text(&current.path, &current.content)),
            ],
            identity: BTreeMap::from([
                ("path".into(), file.clone()),
                ("legacy".into(), "true".into()),
            ]),
            actions: vec![ManagementAction {
                id: "restore_legacy".into(),
                label: "Restore old version".into(),
                privileged: true,
                confirm: true,
                values: BTreeMap::from([
                    ("path".into(), file),
                    ("backup_id".into(), id),
                    ("expected_version".into(), current.version),
                ]),
                fields: vec![ManagementField {
                    id: "allow_unvalidated".into(),
                    label: "Restore without an available checker".into(),
                    value: "false".into(),
                    choices: vec!["false".into(), "true".into()],
                    ..Default::default()
                }],
                ..Default::default()
            }],
        });
    }
    Ok(snapshot)
}
pub fn execute(
    command: &ManagementCommand,
    context: &ExecutionContext,
    io: &mut dyn OperationInteraction,
    flag: &AtomicBool,
) -> Result<String, ManagementError> {
    check_cancelled(flag)?;
    if command.action == "inspect_backups" {
        io.emit(OperationEvent::Snapshot {
            snapshot: query(&ManagementQuery::new(ManagementKind::Backups), flag)?,
        });
        return Ok("Backup history loaded".into());
    }
    if command.action == "restore_legacy" {
        let path = command
            .values
            .get("path")
            .ok_or_else(|| ManagementError::InvalidInput("Select a configuration backup".into()))?;
        let restore = ManagementCommand {
            kind: ManagementKind::SystemConfig,
            action: "restore".into(),
            target: Some(path.clone()),
            values: command.values.clone(),
            identity: command.identity.clone(),
        };
        return system_config::execute(&restore, context, io, flag);
    }
    let id = command
        .target
        .as_deref()
        .ok_or_else(|| ManagementError::InvalidInput("Select a backup".into()))?;
    let record = read_record(Path::new(ROOT), id)?;
    if command.action == "preview_restore" {
        io.emit(OperationEvent::Output {
            text: differences(&record),
        });
        return Ok("Recovery differences ready".into());
    }
    if command.action != "restore" {
        return Err(ManagementError::InvalidInput(
            "Unknown backup operation".into(),
        ));
    }
    if !can_restore(&record) {
        return Err(ManagementError::Conflict(
            "The operation is unfinished; inspect or confirm it before recovery".into(),
        ));
    }
    if record.source == "ssh" {
        return super::ssh::restore_backup(&record.payload, context, io, flag);
    }
    if record.source == "users" {
        return super::users::restore_backup(&record.payload, context, io, flag);
    }
    if record.source == "network" {
        super::network::restore_preflight(&record.payload, context, flag)?;
        return super::network::restore_backup(&record.payload, context, io, flag);
    }
    let _firewall_lock = if record.source == "firewall" {
        Some(super::firewall::lock_rules(flag)?)
    } else {
        None
    };
    if record.source == "firewall" {
        super::firewall::restore_preflight(&record.payload, flag)?;
    }
    if record.source == "scheduled-tasks" {
        super::scheduled_tasks::restore_preflight(&record.payload, context, flag)?;
    }
    verify_current(&record, None)?;
    for file in &record.before {
        check_cancelled(flag)?;
        if !file.document.existed {
            continue;
        }
        let result =
            system_config::check(&file.document.path, &file.document.content, "auto", flag)?;
        match result {
            ConfigCheck::Failed(message) => return Err(ManagementError::Failed(message)),
            ConfigCheck::Unavailable(_) | ConfigCheck::NotChecked
                if command.values.get("allow_unvalidated").map(String::as_str) != Some("true") =>
            {
                return Err(ManagementError::Unavailable(
                    "No checker is available; explicitly choose unvalidated recovery".into(),
                ));
            }
            _ => {}
        }
    }
    let result = match record.source.as_str() {
        "firewall" => super::firewall::restore_backup(id, &record.payload, context, io, flag)?,
        "scheduled-tasks" => {
            super::scheduled_tasks::restore_backup(id, &record.payload, context, io, flag)?
        }
        _ => {
            restore(id)?;
            "Configuration content restored and verified; services have not been reloaded".into()
        }
    };
    Ok(result)
}

#[cfg(test)]
#[path = "../../tests/unit/management/config_backups_tests.rs"]
mod tests;
