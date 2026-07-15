use std::fmt;
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};
use tundra_platform::{AppPaths, PlatformError, atomic_write_document, read_document_bytes};

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
    Ok(Some(EditorRecoveryRecord {
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
    }))
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
}
