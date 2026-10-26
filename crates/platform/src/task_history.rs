//! Durable task summaries and append-only output, independent of the UI lifetime.
use serde::{Deserialize, Serialize};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

#[cfg(unix)]
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};

use crate::management::ManagementError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    WaitingApproval,
    WaitingInput,
    Running,
    Cancelling,
    Succeeded,
    Failed,
    Cancelled,
    Unknown,
    Submitted,
}

impl TaskStatus {
    pub fn active(self) -> bool {
        matches!(
            self,
            Self::WaitingApproval | Self::WaitingInput | Self::Running | Self::Cancelling
        )
    }

    pub fn id(self) -> &'static str {
        match self {
            Self::WaitingApproval => "waiting-approval",
            Self::WaitingInput => "waiting-input",
            Self::Running => "running",
            Self::Cancelling => "cancelling",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
            Self::Unknown => "unknown",
            Self::Submitted => "submitted",
        }
    }

    pub fn label(self) -> String {
        i18n::tr!(format!("task-status-{}", self.id()))
    }
}

fn complete_output() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskSummary {
    pub format_version: u32,
    pub id: String,
    pub name: String,
    pub source: String,
    pub started_at: u64,
    pub finished_at: Option<u64>,
    pub status: TaskStatus,
    #[serde(default)]
    pub origin: String,
    #[serde(default)]
    pub process_id: u32,
    #[serde(default)]
    pub process_started_at: Option<u64>,
    #[serde(default)]
    pub boot_id: Option<String>,
    #[serde(default)]
    pub operation_id: Option<String>,
    #[serde(default)]
    pub result: String,
    #[serde(default)]
    pub action: String,
    #[serde(default)]
    pub has_personal_copy: bool,
    #[serde(default = "complete_output")]
    pub output_complete: bool,
}

impl TaskSummary {
    pub fn new(name: String, source: String, origin: String) -> Self {
        static SEQUENCE: AtomicU64 = AtomicU64::new(0);
        let time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default();
        Self {
            format_version: 1,
            id: format!(
                "{}-{}-{}",
                time.as_nanos(),
                std::process::id(),
                SEQUENCE.fetch_add(1, Ordering::Relaxed)
            ),
            name,
            source,
            origin,
            started_at: time.as_secs(),
            finished_at: None,
            status: TaskStatus::Running,
            process_id: std::process::id(),
            process_started_at: process_start(std::process::id()),
            boot_id: boot_id(),
            operation_id: None,
            result: String::new(),
            action: String::new(),
            has_personal_copy: false,
            output_complete: true,
        }
    }
}

pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn error(error: impl std::fmt::Display) -> ManagementError {
    ManagementError::Failed(error.to_string())
}

#[cfg(target_os = "linux")]
fn boot_id() -> Option<String> {
    fs::read_to_string("/proc/sys/kernel/random/boot_id")
        .ok()
        .map(|id| id.trim().to_owned())
}
#[cfg(not(target_os = "linux"))]
fn boot_id() -> Option<String> {
    None
}

#[cfg(target_os = "linux")]
fn process_start(pid: u32) -> Option<u64> {
    fs::read_to_string(format!("/proc/{pid}/stat"))
        .ok()?
        .rsplit_once(')')?
        .1
        .split_whitespace()
        .nth(19)?
        .parse()
        .ok()
}
#[cfg(not(target_os = "linux"))]
fn process_start(pid: u32) -> Option<u64> {
    static START: std::sync::OnceLock<u64> = std::sync::OnceLock::new();
    (pid == std::process::id()).then(|| {
        *START.get_or_init(|| {
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos() as u64
        })
    })
}

pub fn owner_running(summary: &TaskSummary) -> bool {
    #[cfg(target_os = "linux")]
    {
        summary.process_id != 0
            && summary.boot_id.is_some()
            && summary.boot_id == boot_id()
            && summary.process_started_at.is_some()
            && process_start(summary.process_id) == summary.process_started_at
    }
    #[cfg(not(target_os = "linux"))]
    {
        summary.process_id == std::process::id()
            && summary.process_started_at.is_some()
            && summary.process_started_at == process_start(summary.process_id)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryStore {
    root: PathBuf,
}

#[derive(Debug, Clone, Default)]
pub struct HistoryQuery {
    pub filter: String,
    pub status: String,
    pub before: Option<String>,
    pub limit: usize,
}

#[derive(Debug, Clone, Default)]
pub struct HistoryPage {
    pub records: Vec<TaskSummary>,
    pub next: Option<String>,
    pub total: usize,
    pub bytes: u64,
    pub notices: Vec<String>,
    pub active_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryUsage {
    pub source: String,
    pub records: usize,
    pub bytes: u64,
    #[serde(default)]
    pub active_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputWindow {
    pub offset: u64,
    pub end: u64,
    pub total: u64,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CleanupEntry {
    pub id: String,
    pub name: String,
    pub started_at: u64,
    pub status: TaskStatus,
    pub version: String,
    pub bytes: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CleanupPreview {
    pub entries: Vec<CleanupEntry>,
    pub bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskRecorder {
    store: HistoryStore,
    id: String,
}

impl TaskRecorder {
    pub fn new(store: HistoryStore, summary: TaskSummary) -> Result<Self, ManagementError> {
        store.save(&summary)?;
        Ok(Self {
            store,
            id: summary.id,
        })
    }
    pub fn id(&self) -> &str {
        &self.id
    }
    pub fn store(&self) -> &HistoryStore {
        &self.store
    }
    pub fn status(&self, status: TaskStatus) {
        let _ = self.store.update(&self.id, |summary| {
            summary.status = status;
        });
    }
    pub fn finish(&self, status: TaskStatus, result: String) {
        let _ = self.store.update(&self.id, |summary| {
            summary.status = if summary.status == TaskStatus::Cancelling
                && matches!(status, TaskStatus::Failed | TaskStatus::Succeeded)
            {
                TaskStatus::Cancelled
            } else {
                status
            };
            summary.finished_at = Some(now());
            summary.result = result;
        });
    }
    pub fn output(&self, bytes: &[u8]) {
        if self.store.append_output(&self.id, bytes).is_err() {
            let _ = self.store.update(&self.id, |summary| {
                summary.output_complete = false;
            });
        }
    }
    pub fn event(&self, event: &crate::management::OperationEvent) {
        use crate::management::OperationEvent;
        match event {
            OperationEvent::Connected { operation_id } => {
                let _ = self.store.update(&self.id, |summary| {
                    summary.operation_id = Some(operation_id.clone())
                });
            }
            OperationEvent::Started { kind, action, .. } => {
                let _ = self.store.update(&self.id, |summary| {
                    summary.source = format!("management:{}", kind.id());
                    summary.origin = kind.id().into();
                    summary.action = action.clone();
                });
            }
            OperationEvent::TerminalOutput { bytes } => {
                self.status(TaskStatus::Running);
                self.output(bytes);
            }
            OperationEvent::Output { text } => {
                self.output(text.as_bytes());
                self.output(b"\n");
            }
            OperationEvent::Progress { .. } => self.status(TaskStatus::Running),
            OperationEvent::Question { .. } => self.status(TaskStatus::WaitingInput),
            OperationEvent::Completed { message } => {
                self.output(message.as_bytes());
                self.output(b"\n");
                let status = if self
                    .store
                    .read(&self.id)
                    .is_ok_and(|summary| summary.action == "run_now")
                {
                    TaskStatus::Submitted
                } else {
                    TaskStatus::Succeeded
                };
                self.finish(status, message.clone());
            }
            OperationEvent::Failed { message } => {
                self.output(message.as_bytes());
                self.output(b"\n");
                self.finish(TaskStatus::Failed, message.clone());
            }
            OperationEvent::Disconnected { message } => {
                self.finish(TaskStatus::Unknown, message.clone())
            }
            _ => {}
        }
    }
}

impl HistoryStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }
    pub fn root(&self) -> &Path {
        &self.root
    }

    fn path(&self, id: &str, extension: &str) -> Result<PathBuf, ManagementError> {
        if id.is_empty()
            || id.len() > 200
            || !id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-_".contains(&b))
        {
            return Err(ManagementError::InvalidInput(i18n::tr!(
                "task-history-invalid-id"
            )));
        }
        Ok(self.root.join(format!("{id}.{extension}")))
    }

    fn prepare(&self) -> Result<(), ManagementError> {
        crate::validate_no_follow_path(&self.root, false).map_err(error)?;
        fs::create_dir_all(&self.root).map_err(error)?;
        crate::validate_no_follow_path(&self.root, true).map_err(error)?;
        #[cfg(unix)]
        {
            let metadata = fs::metadata(&self.root).map_err(error)?;
            if metadata.uid() != unsafe { libc::geteuid() } {
                return Err(ManagementError::PermissionDenied(i18n::tr!(
                    "task-history-owner"
                )));
            }
            fs::set_permissions(&self.root, fs::Permissions::from_mode(0o700)).map_err(error)?;
        }
        Ok(())
    }

    fn lock(&self) -> Result<File, ManagementError> {
        self.prepare()?;
        let path = self.root.join("history.lock");
        let mut options = OpenOptions::new();
        options.create(true).read(true).write(true).truncate(false);
        #[cfg(unix)]
        options
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
        let lock = options.open(path).map_err(error)?;
        if !lock.metadata().map_err(error)?.is_file() {
            return Err(ManagementError::InvalidInput(i18n::tr!(
                "task-history-format"
            )));
        }
        lock.lock().map_err(error)?;
        Ok(lock)
    }

    fn save_unlocked(&self, summary: &TaskSummary) -> Result<(), ManagementError> {
        let path = self.path(&summary.id, "json")?;
        let bytes = serde_json::to_vec(summary).map_err(error)?;
        crate::atomic_write_document(&path, &bytes).map_err(error)?;
        #[cfg(unix)]
        fs::set_permissions(path, fs::Permissions::from_mode(0o600)).map_err(error)?;
        Ok(())
    }

    pub fn save(&self, summary: &TaskSummary) -> Result<(), ManagementError> {
        let _lock = self.lock()?;
        self.save_unlocked(summary)
    }

    pub fn read(&self, id: &str) -> Result<TaskSummary, ManagementError> {
        let bytes = crate::read_document_bytes_limited(&self.path(id, "json")?, 64 * 1024)
            .map_err(error)?;
        let summary: TaskSummary = serde_json::from_slice(&bytes.bytes).map_err(error)?;
        if summary.format_version != 1 || summary.id != id {
            return Err(ManagementError::InvalidInput(i18n::tr!(
                "task-history-format"
            )));
        }
        Ok(summary)
    }

    pub fn update(
        &self,
        id: &str,
        update: impl FnOnce(&mut TaskSummary),
    ) -> Result<(), ManagementError> {
        let _lock = self.lock()?;
        let mut summary = self.read(id)?;
        let before = summary.clone();
        update(&mut summary);
        if summary == before {
            Ok(())
        } else {
            self.save_unlocked(&summary)
        }
    }

    pub fn append_output(&self, id: &str, bytes: &[u8]) -> Result<(), ManagementError> {
        let _lock = self.lock()?;
        let path = self.path(id, "output")?;
        crate::validate_no_follow_path(&path, false).map_err(error)?;
        match fs::symlink_metadata(&path) {
            Ok(metadata) if !metadata.is_file() => {
                return Err(ManagementError::InvalidInput(i18n::tr!(
                    "task-history-format"
                )));
            }
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
                return Err(ManagementError::Failed(error.to_string()));
            }
            _ => {}
        }
        let mut options = OpenOptions::new();
        options.create(true).append(true);
        #[cfg(unix)]
        options
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK);
        let mut file = options.open(path).map_err(error)?;
        if !file.metadata().map_err(error)?.is_file() {
            return Err(ManagementError::InvalidInput(i18n::tr!(
                "task-history-format"
            )));
        }
        file.write_all(bytes).map_err(error)
    }

    pub fn output(
        &self,
        id: &str,
        offset: u64,
        limit: usize,
    ) -> Result<(Vec<u8>, u64), ManagementError> {
        let path = self.path(id, "output")?;
        if !path.try_exists().map_err(error)? {
            return Ok((Vec::new(), 0));
        }
        crate::validate_no_follow_path(&path, true).map_err(error)?;
        if !fs::symlink_metadata(&path).map_err(error)?.is_file() {
            return Err(ManagementError::InvalidInput(i18n::tr!(
                "task-history-format"
            )));
        }
        let mut options = OpenOptions::new();
        options.read(true);
        #[cfg(unix)]
        options.custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK);
        let mut file = options.open(path).map_err(error)?;
        let metadata = file.metadata().map_err(error)?;
        if !metadata.is_file() {
            return Err(ManagementError::InvalidInput(i18n::tr!(
                "task-history-format"
            )));
        }
        file.seek(SeekFrom::Start(offset.min(metadata.len())))
            .map_err(error)?;
        let mut output = Vec::new();
        file.take(limit.min(256 * 1024) as u64)
            .read_to_end(&mut output)
            .map_err(error)?;
        Ok((output, metadata.len()))
    }

    pub fn output_text(
        &self,
        id: &str,
        offset: u64,
        limit: usize,
    ) -> Result<OutputWindow, ManagementError> {
        let (bytes, total) = self.output(id, offset, limit.min(256 * 1024).saturating_add(3))?;
        let mut start = 0;
        while start < bytes.len() && bytes[start] & 0xc0 == 0x80 {
            start += 1;
        }
        let mut end = limit.min(bytes.len());
        while end < bytes.len() && bytes[end] & 0xc0 == 0x80 {
            end += 1;
        }
        let offset = offset.min(total);
        Ok(OutputWindow {
            offset: offset + start as u64,
            end: offset + end as u64,
            total,
            text: runtime_log::sanitize_text(&String::from_utf8_lossy(
                &bytes[start..end.max(start)],
            )),
        })
    }

    fn bytes(&self, id: &str) -> Result<u64, ManagementError> {
        let mut bytes = 0;
        for extension in ["json", "output"] {
            let path = self.path(id, extension)?;
            match fs::symlink_metadata(path) {
                Ok(m) if m.is_file() => bytes += m.len(),
                Ok(_) => {
                    return Err(ManagementError::InvalidInput(i18n::tr!(
                        "task-history-format"
                    )));
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(error(e)),
            }
        }
        Ok(bytes)
    }

    pub fn list(&self, query: &HistoryQuery) -> Result<HistoryPage, ManagementError> {
        let mut page = HistoryPage::default();
        let entries = match fs::read_dir(&self.root) {
            Ok(entries) => entries,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(page),
            Err(e) => return Err(error(e)),
        };
        crate::validate_no_follow_path(&self.root, true).map_err(error)?;
        let needle = query.filter.to_lowercase();
        for entry in entries {
            let entry = entry.map_err(error)?;
            if entry.path().extension().and_then(|s| s.to_str()) != Some("json") {
                continue;
            }
            let Some(id) = entry
                .path()
                .file_stem()
                .and_then(|s| s.to_str())
                .map(str::to_owned)
            else {
                continue;
            };
            let mut summary = match self.read(&id) {
                Ok(summary) => summary,
                Err(e) => {
                    page.notices.push(format!("{id}: {e}"));
                    continue;
                }
            };
            page.total += 1;
            match self.bytes(&id) {
                Ok(bytes) => page.bytes += bytes,
                Err(error) => {
                    summary.output_complete = false;
                    page.notices.push(format!("{id}: {error}"));
                }
            }
            if summary.status.active() && !owner_running(&summary) {
                summary.status = TaskStatus::Unknown;
            }
            if summary.status.active() {
                page.active_ids.push(summary.id.clone());
            }
            if !needle.is_empty()
                && !format!("{} {} {}", summary.name, summary.source, summary.result)
                    .to_lowercase()
                    .contains(&needle)
            {
                continue;
            }
            if match query.status.as_str() {
                "active" => !summary.status.active(),
                "attention" => !matches!(
                    summary.status,
                    TaskStatus::WaitingInput
                        | TaskStatus::WaitingApproval
                        | TaskStatus::Failed
                        | TaskStatus::Unknown
                ),
                "finished" => summary.status.active(),
                "" | "all" => false,
                status => summary.status.id() != status,
            } {
                continue;
            }
            page.records.push(summary);
        }
        page.records.sort_by(|a, b| {
            b.started_at
                .cmp(&a.started_at)
                .then_with(|| b.id.cmp(&a.id))
        });
        if let Some(before) = &query.before {
            if let Some((at, id)) = before.split_once('/') {
                if let Ok(at) = at.parse::<u64>() {
                    page.records
                        .retain(|record| (record.started_at, record.id.as_str()) < (at, id));
                }
            } else if let Some(index) = page.records.iter().position(|record| &record.id == before)
            {
                page.records.drain(..=index);
            }
        }
        let limit = if query.limit == 0 {
            50
        } else {
            query.limit.min(200)
        };
        if page.records.len() > limit {
            page.records.truncate(limit);
            page.next = page
                .records
                .last()
                .map(|record| format!("{}/{}", record.started_at, record.id));
        }
        Ok(page)
    }

    fn version(&self, id: &str) -> Result<String, ManagementError> {
        let document = crate::read_document_bytes_limited(&self.path(id, "json")?, 64 * 1024)
            .map_err(error)?;
        let output = self.path(id, "output")?;
        let modified = match fs::symlink_metadata(output) {
            Ok(metadata) => metadata
                .modified()
                .ok()
                .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
                .map(|time| time.as_nanos())
                .unwrap_or_default(),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => 0,
            Err(error) => return Err(ManagementError::Failed(error.to_string())),
        };
        Ok(format!(
            "{}:{}:{modified}",
            document.fingerprint.content_hash,
            self.bytes(id)?
        ))
    }

    pub fn preview_cleanup(&self, ids: &[String]) -> Result<CleanupPreview, ManagementError> {
        let mut preview = CleanupPreview::default();
        for id in ids {
            let summary = self.read(id)?;
            if summary.status.active() && owner_running(&summary) {
                return Err(ManagementError::Conflict(i18n::tr!("task-history-running")));
            }
            let bytes = self.bytes(id)?;
            preview.entries.push(CleanupEntry {
                id: id.clone(),
                name: summary.name,
                started_at: summary.started_at,
                status: summary.status,
                version: self.version(id)?,
                bytes,
            });
            preview.bytes += bytes;
        }
        Ok(preview)
    }

    pub fn cleanup(&self, preview: &CleanupPreview) -> Result<(), ManagementError> {
        let _lock = self.lock()?;
        for entry in &preview.entries {
            let summary = self.read(&entry.id)?;
            if (summary.status.active() && owner_running(&summary))
                || self.version(&entry.id)? != entry.version
            {
                return Err(ManagementError::Conflict(i18n::tr!("task-history-changed")));
            }
        }
        for entry in &preview.entries {
            for extension in ["output", "json"] {
                match fs::remove_file(self.path(&entry.id, extension)?) {
                    Ok(()) => {}
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                    Err(e) => return Err(error(e)),
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "../tests/unit/task_history.rs"]
mod tests;
