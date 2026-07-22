use std::fmt;
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use platform::{AppPaths, PlatformError, atomic_write_document, read_document_bytes};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::editor::rich_document::{RichDocument, RichPosition, RichRange};

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
    pub utf8_bom: bool,
    pub preferred_line_ending: RecoveryLineEnding,
    pub mixed_line_endings: bool,
    pub has_final_newline: bool,
}

impl Default for RecoveryTextMetadata {
    fn default() -> Self {
        Self {
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

pub fn write_editor_recovery(
    app_paths: &AppPaths,
    user_key: &str,
    record: &EditorRecoveryRecord,
) -> Result<(), EditorRecoveryError> {
    let path = editor_recovery_path(app_paths, user_key);
    let value = json!({
        "schema": 1,
        "source": record.source,
        "path": record.path.as_ref().map(|path| path.to_string_lossy()),
        "markdown": record.markdown,
        "source_mode": record.source_mode,
        "cursor": record.cursor,
        "saved_content_hash": record.saved_content_hash,
        "updated_at_epoch_ms": record.updated_at_epoch_ms,
    });
    let mut bytes = serde_json::to_vec_pretty(&value)
        .map_err(|error| EditorRecoveryError::Invalid(error.to_string()))?;
    bytes.push(b'\n');
    atomic_write_document(&path, &bytes)?;
    Ok(())
}

pub fn write_editor_recovery_v2(
    app_paths: &AppPaths,
    user_key: &str,
    record: &EditorRecoveryRecordV2,
) -> Result<(), EditorRecoveryError> {
    let path = editor_recovery_path(app_paths, user_key);
    let mut value = serde_json::to_value(record)
        .map_err(|error| EditorRecoveryError::Invalid(error.to_string()))?;
    let object = value.as_object_mut().ok_or_else(|| {
        EditorRecoveryError::Invalid("recovery payload is not an object".to_string())
    })?;
    object.insert("schema".to_string(), Value::from(2_u64));
    let mut bytes = serde_json::to_vec_pretty(&value)
        .map_err(|error| EditorRecoveryError::Invalid(error.to_string()))?;
    bytes.push(b'\n');
    atomic_write_document(&path, &bytes)?;
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
    match value.get("schema").and_then(Value::as_u64) {
        Some(1) => legacy_record_from_value(&value)
            .map(VersionedEditorRecovery::V1)
            .map(Some),
        Some(2) => match serde_json::from_value::<EditorRecoveryRecordV2>(value.clone()) {
            Ok(record) => Ok(Some(VersionedEditorRecovery::V2(record))),
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
            saved_content_hash: value.get("saved_content_hash").and_then(Value::as_u64),
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
        saved_content_hash: value.get("saved_content_hash").and_then(Value::as_u64),
        updated_at_epoch_ms: value
            .get("updated_at_epoch_ms")
            .and_then(Value::as_u64)
            .unwrap_or(0),
    })
}

pub fn clear_editor_recovery(
    app_paths: &AppPaths,
    user_key: &str,
) -> Result<(), EditorRecoveryError> {
    let path = editor_recovery_path(app_paths, user_key);
    match fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(EditorRecoveryError::Platform(PlatformError::Io {
            operation: "remove editor recovery",
            path: Some(path),
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
mod tests {
    use super::*;

    fn app_paths(root: &std::path::Path) -> AppPaths {
        AppPaths::from_parts(
            root.join("config.toml"),
            root.join("state"),
            root.join("cache"),
            root.join("logs"),
            root.join("temp"),
        )
        .unwrap()
    }

    #[test]
    fn recovery_round_trip_is_scoped_by_user() {
        let root = std::env::temp_dir().join(format!(
            "tundra-editor-recovery-{}-{}",
            std::process::id(),
            unix_millis()
        ));
        fs::create_dir_all(&root).unwrap();
        let root = fs::canonicalize(root).unwrap();
        let paths = app_paths(&root);
        let mut record = EditorRecoveryRecord::new("# recovered\n");
        record.cursor = 4;
        record.path = Some(PathBuf::from("C:/notes/example.md"));
        write_editor_recovery(&paths, "alice", &record).unwrap();
        assert_eq!(read_editor_recovery(&paths, "alice").unwrap(), Some(record));
        assert_eq!(read_editor_recovery(&paths, "bob").unwrap(), None);
        clear_editor_recovery(&paths, "alice").unwrap();
        assert_eq!(read_editor_recovery(&paths, "alice").unwrap(), None);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn schema_two_round_trips_a_rich_document_and_keeps_schema_one_readable() {
        let root = std::env::temp_dir().join(format!(
            "tundra-editor-recovery-v2-{}-{}",
            std::process::id(),
            unix_millis()
        ));
        fs::create_dir_all(&root).unwrap();
        let root = fs::canonicalize(root).unwrap();
        let paths = app_paths(&root);
        let mut record = EditorRecoveryRecordV2::rich(RichDocument::new(), "# fallback\n");
        record.path = Some(PathBuf::from("notes/example.md"));
        record.metadata.utf8_bom = true;
        record.saved_content_hash = Some(42);

        write_editor_recovery_v2(&paths, "alice", &record).unwrap();
        assert_eq!(
            read_versioned_editor_recovery(&paths, "alice").unwrap(),
            Some(VersionedEditorRecovery::V2(record))
        );

        let legacy = EditorRecoveryRecord::new("legacy source");
        write_editor_recovery(&paths, "alice", &legacy).unwrap();
        assert_eq!(
            read_versioned_editor_recovery(&paths, "alice").unwrap(),
            Some(VersionedEditorRecovery::V1(legacy))
        );

        clear_editor_recovery(&paths, "alice").unwrap();
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn damaged_rich_payload_recovers_the_private_markdown_fallback_as_source() {
        let root = std::env::temp_dir().join(format!(
            "tundra-editor-recovery-fallback-{}-{}",
            std::process::id(),
            unix_millis()
        ));
        fs::create_dir_all(&root).unwrap();
        let root = fs::canonicalize(root).unwrap();
        let paths = app_paths(&root);
        let record = EditorRecoveryRecordV2::rich(RichDocument::new(), "# safe fallback\n");
        let mut value = serde_json::to_value(record).unwrap();
        value["schema"] = Value::from(2);
        value["payload"]["document"]["blocks"] = Value::String("corrupt".to_string());
        let bytes = serde_json::to_vec_pretty(&value).unwrap();
        atomic_write_document(&editor_recovery_path(&paths, "alice"), &bytes).unwrap();

        let Some(VersionedEditorRecovery::V2Fallback { record, warning }) =
            read_versioned_editor_recovery(&paths, "alice").unwrap()
        else {
            panic!("expected Source fallback")
        };
        assert!(warning.contains("Source mode"));
        assert!(matches!(
            record.payload,
            EditorRecoveryPayload::Source { ref text, .. } if text == "# safe fallback\n"
        ));

        clear_editor_recovery(&paths, "alice").unwrap();
        fs::remove_dir_all(root).unwrap();
    }
}
