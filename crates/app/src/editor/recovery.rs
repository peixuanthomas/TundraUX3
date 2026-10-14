use std::fmt;
use std::fs::{self, File, OpenOptions, TryLockError};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use platform::{AppPaths, PlatformError, atomic_write_document, read_document_bytes};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::editor::rich_document::{RichDocument, RichPosition, RichRange};

/// Identifies one editor draft for its entire lifetime, independently of its
/// source path, page index, or the account's other open editor windows.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RecoveryId(String);

impl Default for RecoveryId {
    fn default() -> Self {
        static NEXT_ID: AtomicU64 = AtomicU64::new(1);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or_default();
        Self(format!(
            "{:x}-{nanos:x}-{:x}",
            std::process::id(),
            NEXT_ID.fetch_add(1, Ordering::Relaxed)
        ))
    }
}

/// An owned draft and its operating-system lock. Cloning rendering state
/// shares ownership; only dropping the last clone releases the lock. A crash
/// releases it automatically, allowing a later editor to recover the draft.
#[derive(Debug, Clone)]
pub struct EditorRecoveryLease {
    id: RecoveryId,
    path: PathBuf,
    lock: Arc<RecoveryLock>,
}

#[derive(Debug)]
struct RecoveryLock(File);

impl Drop for RecoveryLock {
    fn drop(&mut self) {
        // A concurrently forked child can retain this open file description
        // until exec closes CLOEXEC descriptors. Release our ownership now;
        // merely closing the parent's descriptor leaves the draft locked.
        let _ = self.0.unlock();
    }
}

impl EditorRecoveryLease {
    pub fn id(&self) -> &RecoveryId {
        &self.id
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl PartialEq for EditorRecoveryLease {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id && self.path == other.path && Arc::ptr_eq(&self.lock, &other.lock)
    }
}

impl Eq for EditorRecoveryLease {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditorRecoveryRecord {
    pub source: String,
    pub path: Option<PathBuf>,
    pub markdown: bool,
    pub source_mode: bool,
    pub cursor: usize,
    pub saved_content_hash: Option<u64>,
    pub updated_at_epoch_ms: u64,
}

/// Versioned recovery data written by the Markdown-independent editor.
///
/// This is deliberately an application-private format. It is never written
/// beside the user's Markdown file and is not itself a Markdown save.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EditorRecoveryRecordV2 {
    pub path: Option<PathBuf>,
    pub document_kind: RecoveryDocumentKind,
    pub metadata: RecoveryTextMetadata,
    pub saved_content_hash: Option<u64>,
    pub updated_at_epoch_ms: u64,
    pub payload: EditorRecoveryPayload,
}

impl EditorRecoveryRecordV2 {
    pub fn rich(document: RichDocument, markdown_fallback: impl Into<String>) -> Self {
        Self {
            path: None,
            document_kind: RecoveryDocumentKind::Markdown,
            metadata: RecoveryTextMetadata::default(),
            saved_content_hash: None,
            updated_at_epoch_ms: unix_millis(),
            payload: EditorRecoveryPayload::Rich {
                document,
                cursor: None,
                selection: None,
                markdown_fallback: markdown_fallback.into(),
            },
        }
    }

    pub fn source(text: impl Into<String>, document_kind: RecoveryDocumentKind) -> Self {
        Self {
            path: None,
            document_kind,
            metadata: RecoveryTextMetadata::default(),
            saved_content_hash: None,
            updated_at_epoch_ms: unix_millis(),
            payload: EditorRecoveryPayload::Source {
                text: text.into(),
                cursor: 0,
                selection: None,
            },
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecoveryDocumentKind {
    Markdown,
    PlainText,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecoveryLineEnding {
    Lf,
    CrLf,
    Cr,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecoveryTextMetadata {
    #[serde(default)]
    pub encoding: crate::editor::TextEncoding,
    pub utf8_bom: bool,
    pub preferred_line_ending: RecoveryLineEnding,
    pub mixed_line_endings: bool,
    pub has_final_newline: bool,
}

impl Default for RecoveryTextMetadata {
    fn default() -> Self {
        Self {
            encoding: crate::editor::TextEncoding::Utf8,
            utf8_bom: false,
            preferred_line_ending: RecoveryLineEnding::Lf,
            mixed_line_endings: false,
            has_final_newline: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EditorRecoveryPayload {
    Rich {
        document: RichDocument,
        cursor: Option<RichPosition>,
        selection: Option<RichRange>,
        /// A defensive escape hatch for future schema/model migration errors.
        /// It lives only in the recovery JSON and is never auto-written to the
        /// user's Markdown path.
        markdown_fallback: String,
    },
    Source {
        text: String,
        cursor: usize,
        selection: Option<RecoverySourceSelection>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecoverySourceSelection {
    pub anchor: usize,
    pub focus: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VersionedEditorRecovery {
    V1(EditorRecoveryRecord),
    V2(EditorRecoveryRecordV2),
    /// The schema/header survived but the structured Rich payload did not.
    /// The private Markdown fallback is returned as a Source draft.
    V2Fallback {
        record: EditorRecoveryRecordV2,
        warning: String,
    },
}

impl EditorRecoveryRecord {
    pub fn new(source: impl Into<String>) -> Self {
        Self {
            source: source.into(),
            path: None,
            markdown: true,
            source_mode: false,
            cursor: 0,
            saved_content_hash: None,
            updated_at_epoch_ms: unix_millis(),
        }
    }
}

#[derive(Debug)]
pub enum EditorRecoveryError {
    Platform(PlatformError),
    Invalid(String),
}

impl fmt::Display for EditorRecoveryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Platform(error) => error.fmt(formatter),
            Self::Invalid(message) => formatter.write_str(message),
        }
    }
}

impl std::error::Error for EditorRecoveryError {}

impl From<PlatformError> for EditorRecoveryError {
    fn from(value: PlatformError) -> Self {
        Self::Platform(value)
    }
}

pub fn editor_recovery_path(app_paths: &AppPaths, user_key: &str) -> PathBuf {
    app_paths
        .data_path()
        .join("editor")
        .join("recovery")
        .join(format!(
            "{:016x}.json",
            stable_key_hash(user_key.as_bytes())
        ))
}

fn recovery_directory(app_paths: &AppPaths, user_key: &str) -> PathBuf {
    editor_recovery_path(app_paths, user_key).with_extension("")
}

fn recovery_io_error(
    operation: &'static str,
    path: &Path,
    error: std::io::Error,
) -> EditorRecoveryError {
    EditorRecoveryError::Platform(PlatformError::Io {
        operation,
        path: Some(path.to_owned()),
        message: error.to_string(),
    })
}

fn try_lease(
    id: RecoveryId,
    path: PathBuf,
) -> Result<Option<EditorRecoveryLease>, EditorRecoveryError> {
    let lock_path = path.with_extension("lock");
    let parent = lock_path.parent().ok_or_else(|| {
        EditorRecoveryError::Invalid("editor recovery has no parent directory".into())
    })?;
    fs::create_dir_all(parent)
        .map_err(|error| recovery_io_error("create editor recovery directory", parent, error))?;
    if let Ok(metadata) = fs::symlink_metadata(&lock_path)
        && !metadata.is_file()
    {
        return Err(EditorRecoveryError::Invalid(
            "editor recovery lock is not a regular file".into(),
        ));
    }
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&lock_path)
        .map_err(|error| recovery_io_error("open editor recovery lock", &lock_path, error))?;
    match file.try_lock() {
        Ok(()) => Ok(Some(EditorRecoveryLease {
            id,
            path,
            lock: Arc::new(RecoveryLock(file)),
        })),
        Err(TryLockError::WouldBlock) => Ok(None),
        Err(TryLockError::Error(error)) => {
            Err(recovery_io_error("lock editor recovery", &lock_path, error))
        }
    }
    // Never unlink the lock file: another process may already have opened it.
    // Replacing it would give that process and a new opener different locks.
}

/// Reserves a new instance's path. Existing drafts can only be opened through
/// `claim_next_editor_recovery`, never silently overwritten by a new editor.
pub fn acquire_editor_recovery(
    app_paths: &AppPaths,
    user_key: &str,
    id: &RecoveryId,
) -> Result<EditorRecoveryLease, EditorRecoveryError> {
    if id.0 == "legacy" {
        return Err(EditorRecoveryError::Invalid(
            "legacy editor recovery must be claimed before writing".into(),
        ));
    }
    let path = recovery_directory(app_paths, user_key).join(format!("{}.json", id.0));
    let lease = try_lease(id.clone(), path.clone())?.ok_or_else(|| {
        EditorRecoveryError::Invalid("editor recovery is already owned by an active editor".into())
    })?;
    match fs::symlink_metadata(&path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(lease),
        Err(error) => Err(recovery_io_error("inspect editor recovery", &path, error)),
        Ok(_) => Err(EditorRecoveryError::Invalid(
            "editor recovery already exists; claim it before writing".into(),
        )),
    }
}

fn recovery_paths(
    app_paths: &AppPaths,
    user_key: &str,
) -> Result<Vec<(RecoveryId, PathBuf)>, EditorRecoveryError> {
    let mut paths = Vec::new();
    let legacy = editor_recovery_path(app_paths, user_key);
    match fs::symlink_metadata(&legacy) {
        Ok(_) => paths.push((RecoveryId("legacy".into()), legacy)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(recovery_io_error("inspect editor recovery", &legacy, error)),
    }
    let directory = recovery_directory(app_paths, user_key);
    let entries = match fs::read_dir(&directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(paths),
        Err(error) => return Err(recovery_io_error("list editor recovery", &directory, error)),
    };
    for entry in entries {
        let entry =
            entry.map_err(|error| recovery_io_error("list editor recovery", &directory, error))?;
        let path = entry.path();
        if path.extension().is_none_or(|extension| extension != "json") {
            continue;
        }
        let Some(name) = path.file_stem().and_then(|name| name.to_str()) else {
            continue;
        };
        if name.is_empty()
            || !name
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() || byte == b'-')
        {
            continue;
        }
        paths.push((RecoveryId(name.into()), path));
    }
    paths.sort_by(|left, right| left.0.cmp(&right.0));
    Ok(paths)
}

fn recovery_updated_at(record: &VersionedEditorRecovery) -> u64 {
    match record {
        VersionedEditorRecovery::V1(record) => record.updated_at_epoch_ms,
        VersionedEditorRecovery::V2(record)
        | VersionedEditorRecovery::V2Fallback { record, .. } => record.updated_at_epoch_ms,
    }
}

/// Reads an account's drafts without claiming them. Active drafts are included;
/// callers wishing to restore a draft must use `claim_next_editor_recovery`.
pub fn read_editor_recoveries(
    app_paths: &AppPaths,
    user_key: &str,
) -> Result<Vec<(RecoveryId, VersionedEditorRecovery)>, EditorRecoveryError> {
    let mut records = Vec::new();
    for (id, path) in recovery_paths(app_paths, user_key)? {
        if let Some(record) = read_versioned_recovery_path(&path)? {
            records.push((id, record));
        }
    }
    records.sort_by_key(|(_, record)| std::cmp::Reverse(recovery_updated_at(record)));
    Ok(records)
}

/// Claims just the most recently updated inactive draft. Locks held by other
/// pages or processes are skipped, and malformed files are left untouched.
pub fn claim_next_editor_recovery(
    app_paths: &AppPaths,
    user_key: &str,
) -> Result<Option<(EditorRecoveryLease, VersionedEditorRecovery)>, EditorRecoveryError> {
    let mut newest: Option<(EditorRecoveryLease, VersionedEditorRecovery)> = None;
    let mut first_error = None;
    for (id, path) in recovery_paths(app_paths, user_key)? {
        let candidate = try_lease(id, path).and_then(|lease| {
            let Some(lease) = lease else { return Ok(None) };
            Ok(read_versioned_recovery_path(lease.path())?.map(|record| (lease, record)))
        });
        match candidate {
            Ok(Some(candidate)) => {
                if newest.as_ref().is_none_or(|(_, record)| {
                    recovery_updated_at(&candidate.1) > recovery_updated_at(record)
                }) {
                    newest = Some(candidate);
                }
            }
            Ok(None) => {}
            Err(error) => {
                first_error.get_or_insert(error);
            }
        }
    }
    if newest.is_none()
        && let Some(error) = first_error
    {
        return Err(error);
    }
    Ok(newest)
}

pub fn write_editor_recovery_v2(
    lease: &EditorRecoveryLease,
    record: &EditorRecoveryRecordV2,
) -> Result<(), EditorRecoveryError> {
    let mut value = serde_json::to_value(record)
        .map_err(|error| EditorRecoveryError::Invalid(error.to_string()))?;
    let object = value.as_object_mut().ok_or_else(|| {
        EditorRecoveryError::Invalid("recovery payload is not an object".to_string())
    })?;
    object.insert("schema".to_string(), Value::from(2_u64));
    object.insert(
        "saved_content_hash_algorithm".into(),
        Value::from(platform::DOCUMENT_CONTENT_HASH_ALGORITHM),
    );
    let mut bytes = serde_json::to_vec_pretty(&value)
        .map_err(|error| EditorRecoveryError::Invalid(error.to_string()))?;
    bytes.push(b'\n');
    atomic_write_document(lease.path(), &bytes)?;
    Ok(())
}

pub fn read_editor_recovery(
    app_paths: &AppPaths,
    user_key: &str,
) -> Result<Option<EditorRecoveryRecord>, EditorRecoveryError> {
    let path = editor_recovery_path(app_paths, user_key);
    match fs::symlink_metadata(&path) {
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(EditorRecoveryError::Platform(PlatformError::Io {
                operation: "inspect editor recovery",
                path: Some(path),
                message: error.to_string(),
            }));
        }
    }
    let document = read_document_bytes(&path)?;
    let value: Value = serde_json::from_slice(&document.bytes)
        .map_err(|error| EditorRecoveryError::Invalid(error.to_string()))?;
    legacy_record_from_value(&value).map(Some)
}

pub fn read_versioned_editor_recovery(
    app_paths: &AppPaths,
    user_key: &str,
) -> Result<Option<VersionedEditorRecovery>, EditorRecoveryError> {
    let path = editor_recovery_path(app_paths, user_key);
    read_versioned_recovery_path(&path)
}

fn read_versioned_recovery_path(
    path: &Path,
) -> Result<Option<VersionedEditorRecovery>, EditorRecoveryError> {
    match fs::symlink_metadata(&path) {
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(EditorRecoveryError::Platform(PlatformError::Io {
                operation: "inspect editor recovery",
                path: Some(path.to_owned()),
                message: error.to_string(),
            }));
        }
    }
    let document = read_document_bytes(&path)?;
    let value: Value = serde_json::from_slice(&document.bytes)
        .map_err(|error| EditorRecoveryError::Invalid(error.to_string()))?;
    match value.get("schema").and_then(Value::as_u64) {
        Some(1) => legacy_record_from_value(&value)
            .map(VersionedEditorRecovery::V1)
            .map(Some),
        Some(2) => match serde_json::from_value::<EditorRecoveryRecordV2>(value.clone()) {
            Ok(mut record) => {
                record.saved_content_hash = recovery_content_hash(&value);
                Ok(Some(VersionedEditorRecovery::V2(record)))
            }
            Err(error) => {
                recover_v2_markdown_fallback(&value, error.to_string()).map(|(record, warning)| {
                    Some(VersionedEditorRecovery::V2Fallback { record, warning })
                })
            }
        },
        _ => Err(EditorRecoveryError::Invalid(
            "unsupported editor recovery schema".to_string(),
        )),
    }
}

fn recover_v2_markdown_fallback(
    value: &Value,
    warning: String,
) -> Result<(EditorRecoveryRecordV2, String), EditorRecoveryError> {
    let payload = value
        .get("payload")
        .and_then(Value::as_object)
        .ok_or_else(|| EditorRecoveryError::Invalid(warning.clone()))?;
    if payload.get("type").and_then(Value::as_str) != Some("rich") {
        return Err(EditorRecoveryError::Invalid(warning));
    }
    let fallback = payload
        .get("markdown_fallback")
        .and_then(Value::as_str)
        .ok_or_else(|| EditorRecoveryError::Invalid(warning.clone()))?
        .to_owned();
    let path = value
        .get("path")
        .cloned()
        .map(serde_json::from_value::<Option<PathBuf>>)
        .transpose()
        .map_err(|_| EditorRecoveryError::Invalid(warning.clone()))?
        .flatten();
    let document_kind = value
        .get("document_kind")
        .cloned()
        .and_then(|value| serde_json::from_value(value).ok())
        .unwrap_or(RecoveryDocumentKind::Markdown);
    let metadata = value
        .get("metadata")
        .cloned()
        .and_then(|value| serde_json::from_value(value).ok())
        .unwrap_or_default();
    Ok((
        EditorRecoveryRecordV2 {
            path,
            document_kind,
            metadata,
            saved_content_hash: recovery_content_hash(value),
            updated_at_epoch_ms: value
                .get("updated_at_epoch_ms")
                .and_then(Value::as_u64)
                .unwrap_or_default(),
            payload: EditorRecoveryPayload::Source {
                text: fallback,
                cursor: 0,
                selection: None,
            },
        },
        format!(
            "The structured Rich recovery was damaged and was restored in Source mode: {warning}"
        ),
    ))
}

fn recovery_content_hash(value: &Value) -> Option<u64> {
    // Preserve the draft, but do not bind an old/unknown hash to a disk file:
    // restoring it as an unbound draft retains the existing Save As protection.
    (value
        .get("saved_content_hash_algorithm")
        .and_then(Value::as_str)
        == Some(platform::DOCUMENT_CONTENT_HASH_ALGORITHM))
    .then(|| value.get("saved_content_hash").and_then(Value::as_u64))
    .flatten()
}

fn legacy_record_from_value(value: &Value) -> Result<EditorRecoveryRecord, EditorRecoveryError> {
    if value.get("schema").and_then(Value::as_u64) != Some(1) {
        return Err(EditorRecoveryError::Invalid(
            "unsupported editor recovery schema".to_string(),
        ));
    }
    let source = value
        .get("source")
        .and_then(Value::as_str)
        .ok_or_else(|| EditorRecoveryError::Invalid("recovery source is missing".to_string()))?
        .to_string();
    let path = value.get("path").and_then(Value::as_str).map(PathBuf::from);
    Ok(EditorRecoveryRecord {
        source,
        path,
        markdown: value
            .get("markdown")
            .and_then(Value::as_bool)
            .unwrap_or(true),
        source_mode: value
            .get("source_mode")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        cursor: value
            .get("cursor")
            .and_then(Value::as_u64)
            .and_then(|value| usize::try_from(value).ok())
            .unwrap_or(0),
        saved_content_hash: recovery_content_hash(value),
        updated_at_epoch_ms: value
            .get("updated_at_epoch_ms")
            .and_then(Value::as_u64)
            .unwrap_or(0),
    })
}

pub fn clear_editor_recovery(lease: &EditorRecoveryLease) -> Result<(), EditorRecoveryError> {
    let path = lease.path();
    match fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(EditorRecoveryError::Platform(PlatformError::Io {
            operation: "remove editor recovery",
            path: Some(path.to_owned()),
            message: error.to_string(),
        })),
    }
}

fn unix_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(u128::from(u64::MAX)) as u64)
        .unwrap_or_default()
}

fn stable_key_hash(bytes: &[u8]) -> u64 {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    bytes.iter().fold(OFFSET, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(PRIME)
    })
}

#[cfg(test)]
#[path = "../../tests/unit/editor/recovery/tests.rs"]
mod tests;
