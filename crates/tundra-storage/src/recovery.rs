use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::error::StorageError;
use crate::manager::{RecoveredFile, StorageLoadReport};

pub(crate) fn recover_document(
    report: &mut StorageLoadReport,
    path: &Path,
    document: &'static str,
    cause: StorageError,
) -> Result<(), StorageError> {
    let recovered_path = corrupt_backup_path(path);
    fs::rename(path, &recovered_path).map_err(|error| StorageError::Io {
        operation: "recover corrupt document",
        path: path.to_path_buf(),
        message: error.to_string(),
    })?;

    report
        .warnings
        .push(format!("Recovered corrupt {document}: {cause}"));
    report.recovered_files.push(RecoveredFile {
        original_path: path.to_path_buf(),
        recovered_path,
    });

    Ok(())
}

fn corrupt_backup_path(path: &Path) -> PathBuf {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let file_name = path
        .file_name()
        .map(|name| name.to_string_lossy())
        .unwrap_or_else(|| "document".into());

    for offset in 0..64 {
        let path = parent.join(format!(
            "{file_name}.corrupt.{}",
            unix_nanos().saturating_add(offset)
        ));
        if !path.exists() {
            return path;
        }
    }

    parent.join(format!("{file_name}.corrupt.{}", unix_nanos()))
}

fn unix_nanos() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0)
}
