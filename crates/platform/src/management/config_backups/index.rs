use super::*;
use std::collections::{BTreeMap, BTreeSet};
use std::io::{BufReader, Write};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct Stamp {
    device: u64,
    inode: u64,
    bytes: u64,
    modified: i64,
    nanos: i64,
}

impl Stamp {
    fn of(metadata: &fs::Metadata) -> Self {
        Self {
            device: metadata.dev(),
            inode: metadata.ino(),
            bytes: metadata.len(),
            modified: metadata.mtime(),
            nanos: metadata.mtime_nsec(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(super) struct Summary {
    pub id: String,
    pub source: String,
    pub timestamp: u64,
    pub subject: String,
    pub result: String,
    pub retained: bool,
    pub completed: bool,
    pub restorable: bool,
    pub disabled_reason: Option<String>,
    pub legacy: bool,
    pub references: BTreeSet<String>,
    pub files: Vec<(bool, Stamp)>,
}

impl Summary {
    pub fn bytes(&self) -> u64 {
        self.files.iter().map(|(_, stamp)| stamp.bytes).sum()
    }
    pub fn paths(&self, root: &Path) -> Vec<PathBuf> {
        self.files
            .iter()
            .map(|(operation, _)| {
                if *operation {
                    root.join("operations")
                } else {
                    root.to_owned()
                }
                .join(format!("{}.json", self.id))
            })
            .collect()
    }
}

#[derive(Serialize, Deserialize)]
struct Checkpoint {
    directories: [Option<Stamp>; 2],
    journal_bytes: u64,
    #[serde(default)]
    retentions: Option<Stamp>,
}

pub(crate) struct HistoryLock {
    _file: File,
    pub current: bool,
}

impl Drop for HistoryLock {
    fn drop(&mut self) {
        unsafe {
            libc::flock(self._file.as_raw_fd(), libc::LOCK_UN);
        }
    }
}

pub(super) fn private_file(path: &Path, flags: i32) -> Result<File, ManagementError> {
    let (directory, name) = system_config::parent(path, flags & libc::O_CREAT != 0)?;
    let fd = unsafe {
        libc::openat(
            directory.as_raw_fd(),
            name.as_ptr(),
            flags | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            0o600,
        )
    };
    if fd < 0 {
        return Err(io_error(std::io::Error::last_os_error()));
    }
    let file = unsafe { File::from_raw_fd(fd) };
    let metadata = file.metadata().map_err(io_error)?;
    if !metadata.is_file()
        || metadata.uid() != unsafe { libc::geteuid() }
        || metadata.mode() & 0o077 != 0
    {
        return Err(ManagementError::PermissionDenied(i18n::tr!(
            "backup-history-private-file"
        )));
    }
    Ok(file)
}

fn io_error(error: std::io::Error) -> ManagementError {
    if error.kind() == std::io::ErrorKind::PermissionDenied {
        ManagementError::PermissionDenied(error.to_string())
    } else {
        super::error(error)
    }
}

pub(crate) fn lock(root: &Path) -> Result<HistoryLock, ManagementError> {
    let (directory, _) = system_config::parent(&root.join("record"), true)?;
    let metadata = directory.metadata().map_err(io_error)?;
    if !metadata.is_dir()
        || metadata.uid() != unsafe { libc::geteuid() }
        || metadata.mode() & 0o022 != 0
    {
        return Err(ManagementError::PermissionDenied(i18n::tr!(
            "platform-backups-unsafe-directory"
        )));
    }
    directory
        .set_permissions(fs::Permissions::from_mode(0o700))
        .map_err(io_error)?;
    let file = private_file(&root.join("index/lock"), libc::O_RDWR | libc::O_CREAT)?;
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) } != 0 {
        return Err(io_error(std::io::Error::last_os_error()));
    }
    let mut lock = HistoryLock {
        _file: file,
        current: current(root),
    };
    if !lock.current {
        let mut has_records = false;
        for directory in [root.to_owned(), root.join("operations")] {
            let directory = match fs::read_dir(directory) {
                Ok(directory) => directory,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => return Err(io_error(error)),
            };
            for entry in directory {
                let path = entry.map_err(io_error)?.path();
                if path
                    .extension()
                    .is_some_and(|extension| extension == "json")
                    && path
                        .file_stem()
                        .and_then(|id| id.to_str())
                        .is_some_and(|id| record_path(root, id).is_ok())
                {
                    has_records = true;
                    break;
                }
            }
            if has_records {
                break;
            }
        }
        if !has_records {
            rewrite(root, &BTreeMap::new())?;
            lock.current = true;
        }
    }
    Ok(lock)
}

pub(super) fn lease(
    root: &Path,
    id: &str,
    wait: bool,
) -> Result<Option<HistoryLock>, ManagementError> {
    record_path(root, id)?;
    let file = private_file(
        &root.join("index/leases").join(id),
        libc::O_RDWR | libc::O_CREAT,
    )?;
    if unsafe {
        libc::flock(
            file.as_raw_fd(),
            libc::LOCK_EX | if wait { 0 } else { libc::LOCK_NB },
        )
    } != 0
    {
        let error = std::io::Error::last_os_error();
        return if error.kind() == std::io::ErrorKind::WouldBlock {
            Ok(None)
        } else {
            Err(io_error(error))
        };
    }
    Ok(Some(HistoryLock {
        _file: file,
        current: false,
    }))
}

pub(super) struct RestoreLease {
    _gate: HistoryLock,
    _record: HistoryLock,
}

pub(super) fn restore_lease(root: &Path, id: &str) -> Result<RestoreLease, ManagementError> {
    let file = private_file(
        &root.join("index/recovery.lock"),
        libc::O_RDWR | libc::O_CREAT,
    )?;
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_SH) } != 0 {
        return Err(io_error(std::io::Error::last_os_error()));
    }
    let gate = HistoryLock {
        _file: file,
        current: false,
    };
    if !record_path(root, id)?.exists() && !root.join(format!("{id}.json")).exists() {
        return Err(ManagementError::Conflict(i18n::tr!(
            "backup-history-record-changed"
        )));
    }
    let record = lease(root, id, true)?.expect("a blocking record lease is acquired");
    Ok(RestoreLease {
        _gate: gate,
        _record: record,
    })
}

pub(super) fn cleanup_lease(root: &Path) -> Result<Option<HistoryLock>, ManagementError> {
    let file = private_file(
        &root.join("index/recovery.lock"),
        libc::O_RDWR | libc::O_CREAT,
    )?;
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        let error = std::io::Error::last_os_error();
        return if error.kind() == std::io::ErrorKind::WouldBlock {
            Ok(None)
        } else {
            Err(io_error(error))
        };
    }
    Ok(Some(HistoryLock {
        _file: file,
        current: false,
    }))
}

fn directories(root: &Path) -> Result<[Option<Stamp>; 2], ManagementError> {
    let stamp = |path: &Path| match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_dir() => Ok(Some(Stamp::of(&metadata))),
        Ok(_) => Err(ManagementError::PermissionDenied(i18n::tr!(
            "platform-backups-unsafe-directory"
        ))),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(io_error(error)),
    };
    Ok([stamp(root)?, stamp(&root.join("operations"))?])
}

fn current(root: &Path) -> bool {
    let Ok(checkpoint) = private_file(&root.join("index/checkpoint.json"), libc::O_RDONLY) else {
        return false;
    };
    let Ok(checkpoint) = serde_json::from_reader::<_, Checkpoint>(checkpoint.take(8192)) else {
        return false;
    };
    directories(root).is_ok_and(|actual| actual == checkpoint.directories)
        && retention_stamp(root).is_ok_and(|actual| actual == checkpoint.retentions)
        && fs::symlink_metadata(root.join("index/entries.jsonl"))
            .is_ok_and(|metadata| metadata.len() == checkpoint.journal_bytes)
}

fn retention_stamp(root: &Path) -> Result<Option<Stamp>, ManagementError> {
    match fs::symlink_metadata(root.join("index/retained-legacy.json")) {
        Ok(metadata) => Ok(Some(Stamp::of(&metadata))),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(io_error(error)),
    }
}

fn atomic_json(path: &Path, value: &impl Serialize) -> Result<(), ManagementError> {
    if path.exists() {
        private_file(path, libc::O_RDONLY)?;
    }
    crate::document::atomic_write_document_with(path, |writer| {
        serde_json::to_writer(writer, value).map_err(std::io::Error::other)
    })
    .map(|_| ())
    .map_err(super::error)
}

fn checkpoint(root: &Path) -> Result<(), ManagementError> {
    let journal = private_file(&root.join("index/entries.jsonl"), libc::O_RDONLY)?;
    atomic_json(
        &root.join("index/checkpoint.json"),
        &Checkpoint {
            directories: directories(root)?,
            journal_bytes: journal.metadata().map_err(io_error)?.len(),
            retentions: retention_stamp(root)?,
        },
    )
}

fn artifacts(root: &Path, id: &str) -> Result<Vec<(bool, Stamp)>, ManagementError> {
    let mut files = Vec::new();
    for operation in [true, false] {
        let path = if operation {
            record_path(root, id)?
        } else {
            root.join(format!("{id}.json"))
        };
        match fs::symlink_metadata(&path) {
            Ok(metadata) => {
                if !metadata.is_file()
                    || metadata.uid() != unsafe { libc::geteuid() }
                    || metadata.mode() & 0o077 != 0
                {
                    return Err(ManagementError::PermissionDenied(i18n::tr!(
                        "platform-backups-unsafe-record"
                    )));
                }
                files.push((operation, Stamp::of(&metadata)));
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(io_error(error)),
        }
    }
    Ok(files)
}

fn references(value: &Value, result: &mut BTreeSet<String>) {
    match value {
        Value::Object(fields) => {
            for (key, value) in fields {
                if matches!(
                    key.as_str(),
                    "backup_id" | "restore_record" | "seed_backup" | "rollback_backup_id"
                ) && let Some(id) = value.as_str()
                    && record_path(Path::new("/"), id).is_ok()
                {
                    result.insert(id.into());
                }
                references(value, result);
            }
        }
        Value::Array(values) => {
            for value in values {
                references(value, result);
            }
        }
        _ => {}
    }
}

pub(super) fn summary(root: &Path, record: &Record) -> Result<Summary, ManagementError> {
    let mut linked = BTreeSet::new();
    references(&record.payload, &mut linked);
    linked.remove(&record.id);
    Ok(Summary {
        id: record.id.clone(),
        source: record.source.clone(),
        timestamp: record.timestamp,
        subject: operation_subject(record),
        result: record.result.clone(),
        retained: record.retained,
        completed: record.after.is_some()
            && record.result != "pending"
            && !record.result.starts_with("awaiting SSH")
            && record.result != "awaiting network confirmation",
        restorable: can_restore(record),
        disabled_reason: record
            .payload
            .get("restore_disabled_reason")
            .and_then(Value::as_str)
            .map(String::from),
        legacy: false,
        references: linked,
        files: artifacts(root, &record.id)?,
    })
}

pub(super) fn legacy_retained(root: &Path) -> Result<BTreeSet<String>, ManagementError> {
    let path = root.join("index/retained-legacy.json");
    if !path.exists() {
        return Ok(BTreeSet::new());
    }
    serde_json::from_reader(private_file(&path, libc::O_RDONLY)?.take(MAX_RECORD))
        .map_err(super::error)
}

pub(super) fn set_legacy_retained(
    root: &Path,
    id: &str,
    retain: bool,
) -> Result<(), ManagementError> {
    let mut retained = legacy_retained(root)?;
    if retain {
        retained.insert(id.into());
    } else {
        retained.remove(id);
    }
    atomic_json(&root.join("index/retained-legacy.json"), &retained)
}

fn legacy_summary(root: &Path, id: &str, retained: bool) -> Result<Summary, ManagementError> {
    let stored = system_config::read_backup(&root.join(format!("{id}.json")))?;
    Ok(Summary {
        id: id.into(),
        source: "legacy".into(),
        timestamp: id
            .split('-')
            .next()
            .and_then(|time| time.parse::<u128>().ok())
            .map(|nanos| (nanos / 1_000_000_000) as u64)
            .unwrap_or(0),
        subject: stored.document.path.display().to_string(),
        result: String::new(),
        retained,
        completed: true,
        restorable: true,
        disabled_reason: None,
        legacy: true,
        references: BTreeSet::new(),
        files: artifacts(root, id)?,
    })
}

pub(super) fn record_written(
    root: &Path,
    record: &Record,
    was_current: bool,
) -> Result<(), ManagementError> {
    if !was_current {
        return Ok(());
    }
    let mut file = private_file(
        &root.join("index/entries.jsonl"),
        libc::O_WRONLY | libc::O_APPEND,
    )?;
    serde_json::to_writer(&mut file, &summary(root, record)?).map_err(super::error)?;
    file.write_all(b"\n").map_err(io_error)?;
    file.sync_all().map_err(io_error)?;
    checkpoint(root)
}

pub(super) fn rewrite(
    root: &Path,
    entries: &BTreeMap<String, Summary>,
) -> Result<(), ManagementError> {
    let path = root.join("index/entries.jsonl");
    if path.exists() {
        private_file(&path, libc::O_RDONLY)?;
    }
    crate::document::atomic_write_document_with(&path, |writer| {
        for entry in entries.values() {
            serde_json::to_writer(&mut *writer, entry).map_err(std::io::Error::other)?;
            writer.write_all(b"\n")?;
        }
        Ok(())
    })
    .map_err(super::error)?;
    checkpoint(root)
}

pub(super) fn load(
    root: &Path,
    flag: &AtomicBool,
) -> Result<BTreeMap<String, Summary>, ManagementError> {
    if current(root) {
        let file = private_file(&root.join("index/entries.jsonl"), libc::O_RDONLY)?;
        let stream =
            serde_json::Deserializer::from_reader(BufReader::new(file)).into_iter::<Summary>();
        let mut entries = BTreeMap::new();
        let mut valid = true;
        for entry in stream {
            check_cancelled(flag)?;
            match entry {
                Ok(entry) => {
                    entries.insert(entry.id.clone(), entry);
                }
                Err(_) => {
                    valid = false;
                    break;
                }
            }
        }
        if valid {
            return Ok(entries);
        }
    }
    let mut ids = BTreeSet::new();
    for directory in [root.to_owned(), root.join("operations")] {
        let directory = match fs::read_dir(directory) {
            Ok(directory) => directory,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(io_error(error)),
        };
        for entry in directory {
            check_cancelled(flag)?;
            let path = entry.map_err(io_error)?.path();
            if path
                .extension()
                .is_some_and(|extension| extension == "json")
                && let Some(id) = path.file_stem().and_then(|id| id.to_str())
                && record_path(root, id).is_ok()
            {
                ids.insert(id.to_owned());
            }
        }
    }
    let retained = legacy_retained(root)?;
    let mut entries = BTreeMap::new();
    for id in ids {
        check_cancelled(flag)?;
        let entry = if record_path(root, &id)?.exists() {
            summary(root, &read_record(root, &id)?)?
        } else {
            legacy_summary(root, &id, retained.contains(&id))?
        };
        entries.insert(id, entry);
    }
    rewrite(root, &entries)?;
    Ok(entries)
}

pub(super) fn unchanged(root: &Path, summary: &Summary) -> Result<bool, ManagementError> {
    Ok(artifacts(root, &summary.id)? == summary.files)
}
