use std::path::Path;

use crate::error::StorageError;
use crate::manager::StorageLoadReport;
use crate::schema::supported_schema_version;

pub(crate) fn migrate_v1_noop(
    report: &mut StorageLoadReport,
    path: &Path,
    document: &'static str,
    schema_version: u32,
) -> Result<(), StorageError> {
    let supported = supported_schema_version(document);
    match schema_version {
        found if found == supported => Ok(()),
        0 => Err(StorageError::InvalidSchemaVersion {
            document,
            path: path.to_path_buf(),
            found: schema_version,
            supported,
        }),
        found if found < supported => {
            report.migrated_files.push(path.to_path_buf());
            Ok(())
        }
        found => Err(StorageError::UnsupportedSchema {
            document,
            path: path.to_path_buf(),
            found,
            supported,
        }),
    }
}
