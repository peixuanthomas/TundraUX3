use std::fs;
use std::path::Path;

use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::atomic_write::atomic_write;
use crate::error::StorageError;
use crate::schema::{
    StorageFormat, VersionedDocument, ensure_document_schema, ensure_supported_schema,
    json_schema_version, toml_schema_version,
};

pub(crate) fn load_toml_document<T>(path: &Path, document: &'static str) -> Result<T, StorageError>
where
    T: DeserializeOwned,
{
    let contents = read_to_string(path, "read TOML document")?;
    let schema_version = toml_schema_version(&contents, path, document)?;
    ensure_supported_schema(document, path, schema_version)?;

    toml::from_str(&contents).map_err(|error| StorageError::TomlDeserialize {
        document,
        path: path.to_path_buf(),
        message: error.to_string(),
    })
}

pub(crate) fn save_toml_document<T>(
    path: &Path,
    document: &'static str,
    value: &T,
) -> Result<(), StorageError>
where
    T: Serialize + VersionedDocument,
{
    ensure_document_schema(document, path, value.schema_version())?;
    validate_existing_schema(path, document, StorageFormat::Toml)?;
    let contents = toml::to_string_pretty(value).map_err(|error| StorageError::TomlSerialize {
        document,
        message: error.to_string(),
    })?;

    atomic_write(path, contents.as_bytes())
}

pub(crate) fn validate_toml_document<T>(
    path: &Path,
    document: &'static str,
) -> Result<u32, StorageError>
where
    T: DeserializeOwned,
{
    let _: T = load_toml_document(path, document)?;
    let contents = read_to_string(path, "read TOML document")?;
    toml_schema_version(&contents, path, document)
}

pub(crate) fn load_json_document<T>(path: &Path, document: &'static str) -> Result<T, StorageError>
where
    T: DeserializeOwned,
{
    let contents = read_to_string(path, "read JSON document")?;
    let schema_version = json_schema_version(&contents, path, document)?;
    ensure_supported_schema(document, path, schema_version)?;

    serde_json::from_str(&contents).map_err(|error| StorageError::JsonDeserialize {
        document,
        path: path.to_path_buf(),
        message: error.to_string(),
    })
}

pub(crate) fn save_json_document<T>(
    path: &Path,
    document: &'static str,
    value: &T,
) -> Result<(), StorageError>
where
    T: Serialize + VersionedDocument,
{
    ensure_document_schema(document, path, value.schema_version())?;
    validate_existing_schema(path, document, StorageFormat::VersionedJson)?;
    let contents =
        serde_json::to_string_pretty(value).map_err(|error| StorageError::JsonSerialize {
            document,
            message: error.to_string(),
        })?;
    let mut bytes = contents.into_bytes();
    bytes.push(b'\n');

    atomic_write(path, &bytes)
}

pub(crate) fn validate_json_document<T>(
    path: &Path,
    document: &'static str,
) -> Result<u32, StorageError>
where
    T: DeserializeOwned,
{
    let _: T = load_json_document(path, document)?;
    let contents = read_to_string(path, "read JSON document")?;
    json_schema_version(&contents, path, document)
}

pub(crate) fn validate_existing_schema(
    path: &Path,
    document: &'static str,
    format: StorageFormat,
) -> Result<(), StorageError> {
    if !path.exists() {
        return Ok(());
    }

    let contents = read_to_string(path, "read existing document")?;
    let schema_version = match format {
        StorageFormat::Toml => toml_schema_version(&contents, path, document)?,
        StorageFormat::VersionedJson => json_schema_version(&contents, path, document)?,
    };

    ensure_supported_schema(document, path, schema_version)
}

fn read_to_string(path: &Path, operation: &'static str) -> Result<String, StorageError> {
    fs::read_to_string(path).map_err(|error| StorageError::Io {
        operation,
        path: path.to_path_buf(),
        message: error.to_string(),
    })
}
