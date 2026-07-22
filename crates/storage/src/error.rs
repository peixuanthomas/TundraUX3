use std::fmt;
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StorageError {
    Io {
        operation: &'static str,
        path: PathBuf,
        message: String,
    },
    Platform {
        message: String,
    },
    MissingParent {
        path: PathBuf,
    },
    InvalidSchemaVersion {
        document: &'static str,
        path: PathBuf,
        found: u32,
        supported: u32,
    },
    UnsupportedSchema {
        document: &'static str,
        path: PathBuf,
        found: u32,
        supported: u32,
    },
    TomlDeserialize {
        document: &'static str,
        path: PathBuf,
        message: String,
    },
    TomlSerialize {
        document: &'static str,
        message: String,
    },
    JsonDeserialize {
        document: &'static str,
        path: PathBuf,
        message: String,
    },
    JsonSerialize {
        document: &'static str,
        message: String,
    },
}

impl fmt::Display for StorageError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io {
                operation,
                path,
                message,
            } => write!(
                formatter,
                "{operation} failed for {}: {message}",
                path.display()
            ),
            Self::Platform { message } => formatter.write_str(message),
            Self::MissingParent { path } => {
                write!(formatter, "{} has no parent directory", path.display())
            }
            Self::InvalidSchemaVersion {
                document,
                path,
                found,
                supported,
            } => write!(
                formatter,
                "{document} at {} uses invalid schema version {found}; supported version is {supported}",
                path.display()
            ),
            Self::UnsupportedSchema {
                document,
                path,
                found,
                supported,
            } => write!(
                formatter,
                "{document} at {} uses future schema version {found}; supported version is {supported}",
                path.display()
            ),
            Self::TomlDeserialize {
                document,
                path,
                message,
            } => write!(
                formatter,
                "could not read TOML {document} at {}: {message}",
                path.display()
            ),
            Self::TomlSerialize { document, message } => {
                write!(formatter, "could not serialize TOML {document}: {message}")
            }
            Self::JsonDeserialize {
                document,
                path,
                message,
            } => write!(
                formatter,
                "could not read JSON {document} at {}: {message}",
                path.display()
            ),
            Self::JsonSerialize { document, message } => {
                write!(formatter, "could not serialize JSON {document}: {message}")
            }
        }
    }
}

impl std::error::Error for StorageError {}
