use std::{error::Error, fmt, path::PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LanguageErrorKind {
    Io,
    Manifest,
    UnknownLanguage,
    Syntax,
    Duplicate,
    Reference,
    Parameters,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LanguageError {
    pub kind: LanguageErrorKind,
    pub path: Option<PathBuf>,
    pub message: String,
}

impl LanguageError {
    pub(crate) fn new(
        kind: LanguageErrorKind,
        path: impl Into<Option<PathBuf>>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            kind,
            path: path.into(),
            message: message.into(),
        }
    }
}

impl fmt::Display for LanguageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(path) = &self.path {
            write!(f, "{}: ", path.display())?;
        }
        f.write_str(&self.message)
    }
}
impl Error for LanguageError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepairKind {
    MissingFile,
    CorruptFile,
    MissingMessages,
    MissingTranslations,
    InvalidManifest,
    WriteFailed,
    StartupFallback,
    InvalidResource,
}

/// `repaired` records persistence; false means the disk still needs attention.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepairDiagnostic {
    pub kind: RepairKind,
    pub path: PathBuf,
    pub message: String,
    pub repaired: bool,
}
