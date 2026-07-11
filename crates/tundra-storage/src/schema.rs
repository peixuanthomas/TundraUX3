use std::path::Path;

use crate::error::StorageError;

pub const SCHEMA_VERSION: u32 = 1;
pub const USERS_SCHEMA_VERSION: u32 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StorageFormat {
    Toml,
    VersionedJson,
}

pub trait VersionedDocument {
    fn schema_version(&self) -> u32;
}

pub(crate) fn ensure_document_schema(
    document: &'static str,
    path: &Path,
    schema_version: u32,
) -> Result<(), StorageError> {
    let supported = supported_schema_version(document);
    if schema_version == supported {
        return Ok(());
    }

    if schema_version > supported {
        Err(StorageError::UnsupportedSchema {
            document,
            path: path.to_path_buf(),
            found: schema_version,
            supported,
        })
    } else {
        Err(StorageError::InvalidSchemaVersion {
            document,
            path: path.to_path_buf(),
            found: schema_version,
            supported,
        })
    }
}

pub(crate) fn ensure_supported_schema(
    document: &'static str,
    path: &Path,
    schema_version: u32,
) -> Result<(), StorageError> {
    let supported = supported_schema_version(document);
    if schema_version > supported {
        return Err(StorageError::UnsupportedSchema {
            document,
            path: path.to_path_buf(),
            found: schema_version,
            supported,
        });
    }

    if schema_version == 0 {
        return Err(StorageError::InvalidSchemaVersion {
            document,
            path: path.to_path_buf(),
            found: schema_version,
            supported,
        });
    }

    Ok(())
}

pub(crate) fn supported_schema_version(document: &str) -> u32 {
    match document {
        "users" => USERS_SCHEMA_VERSION,
        _ => SCHEMA_VERSION,
    }
}

pub(crate) fn toml_schema_version(
    contents: &str,
    path: &Path,
    document: &'static str,
) -> Result<u32, StorageError> {
    let value: toml::Value =
        toml::from_str(contents).map_err(|error| StorageError::TomlDeserialize {
            document,
            path: path.to_path_buf(),
            message: error.to_string(),
        })?;
    let Some(integer) = value
        .get("schema_version")
        .and_then(|value| value.as_integer())
    else {
        return Err(StorageError::TomlDeserialize {
            document,
            path: path.to_path_buf(),
            message: "missing numeric schema_version".to_string(),
        });
    };
    u32::try_from(integer).map_err(|_| StorageError::TomlDeserialize {
        document,
        path: path.to_path_buf(),
        message: format!("schema_version {integer} is outside the supported range"),
    })
}

pub(crate) fn json_schema_version(
    contents: &str,
    path: &Path,
    document: &'static str,
) -> Result<u32, StorageError> {
    let value: serde_json::Value =
        serde_json::from_str(contents).map_err(|error| StorageError::JsonDeserialize {
            document,
            path: path.to_path_buf(),
            message: error.to_string(),
        })?;
    let Some(integer) = value.get("schema_version").and_then(|value| value.as_u64()) else {
        return Err(StorageError::JsonDeserialize {
            document,
            path: path.to_path_buf(),
            message: "missing numeric schema_version".to_string(),
        });
    };
    u32::try_from(integer).map_err(|_| StorageError::JsonDeserialize {
        document,
        path: path.to_path_buf(),
        message: format!("schema_version {integer} is outside the supported range"),
    })
}
