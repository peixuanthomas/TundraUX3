use std::path::{Path, PathBuf};

use crate::atomic_write::create_dir;
use crate::clock_document::ClockDocument;
use crate::config_document::StorageConfig;
use crate::descriptors::{CLOCK_DESCRIPTOR, CONFIG_DESCRIPTOR};
use crate::document_io::{
    save_json_document, save_toml_document, validate_json_document, validate_toml_document,
};
use crate::error::StorageError;
use crate::manager::{StorageLoadReport, StorageManager};
use crate::recovery::recover_document;
use crate::state_documents::{RecentFilesDocument, SessionsDocument, StateDocument};
use crate::trash_document::TrashDocument;
use crate::user_document::UsersDocument;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StorageDocumentKind {
    Config,
    Users,
    State,
    RecentFiles,
    Sessions,
    Clock,
    TrashManifest,
}

impl StorageDocumentKind {
    const ALL: [Self; 7] = [
        Self::Config,
        Self::Users,
        Self::State,
        Self::RecentFiles,
        Self::Sessions,
        Self::Clock,
        Self::TrashManifest,
    ];
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StorageDocumentStatus {
    Healthy,
    Missing,
    Corrupt,
    UnsupportedSchema,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StorageDocumentCheck {
    pub kind: StorageDocumentKind,
    pub path: PathBuf,
    pub status: StorageDocumentStatus,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StorageRepairReport {
    pub kind: StorageDocumentKind,
    pub path: PathBuf,
    pub backup_path: Option<PathBuf>,
    pub created: bool,
    pub rebuilt: bool,
}

impl StorageManager {
    /// Checks every managed storage document without creating directories, files, or backups.
    pub fn check_documents(&self) -> Result<Vec<StorageDocumentCheck>, StorageError> {
        Ok(StorageDocumentKind::ALL
            .into_iter()
            .map(|kind| self.check_document(kind))
            .collect())
    }

    /// Repairs one missing or corrupt document using its current default representation.
    ///
    /// Healthy documents are left untouched. Documents with a future schema version are
    /// rejected before any directory or file is changed.
    pub fn repair_document(
        &self,
        kind: StorageDocumentKind,
    ) -> Result<StorageRepairReport, StorageError> {
        let path = self.document_path(kind).to_path_buf();
        if !path.exists() {
            self.create_document_parent(&path)?;
            self.save_default_document(kind)?;
            return Ok(StorageRepairReport {
                kind,
                path,
                backup_path: None,
                created: true,
                rebuilt: false,
            });
        }

        match self.validate_document(kind) {
            Ok(()) => Ok(StorageRepairReport {
                kind,
                path,
                backup_path: None,
                created: false,
                rebuilt: false,
            }),
            Err(error @ StorageError::UnsupportedSchema { .. }) => Err(error),
            Err(error) => {
                self.create_document_parent(&path)?;
                let mut recovery = StorageLoadReport::default();
                recover_document(&mut recovery, &path, self.document_name(kind), error)?;
                let backup_path = recovery
                    .recovered_files
                    .into_iter()
                    .next()
                    .map(|file| file.recovered_path);
                self.save_default_document(kind)?;

                Ok(StorageRepairReport {
                    kind,
                    path,
                    backup_path,
                    created: false,
                    rebuilt: true,
                })
            }
        }
    }

    fn check_document(&self, kind: StorageDocumentKind) -> StorageDocumentCheck {
        let path = self.document_path(kind).to_path_buf();
        let (status, message) = if !path.exists() {
            (
                StorageDocumentStatus::Missing,
                format!("{} document is missing", self.document_name(kind)),
            )
        } else {
            match self.validate_document(kind) {
                Ok(()) => (
                    StorageDocumentStatus::Healthy,
                    format!("{} document is healthy", self.document_name(kind)),
                ),
                Err(error @ StorageError::UnsupportedSchema { .. }) => {
                    (StorageDocumentStatus::UnsupportedSchema, error.to_string())
                }
                Err(error) => (StorageDocumentStatus::Corrupt, error.to_string()),
            }
        };

        StorageDocumentCheck {
            kind,
            path,
            status,
            message,
        }
    }

    fn document_path(&self, kind: StorageDocumentKind) -> &Path {
        match kind {
            StorageDocumentKind::Config => &self.layout.config_path,
            StorageDocumentKind::Users => &self.layout.users_path,
            StorageDocumentKind::State => &self.layout.state_path,
            StorageDocumentKind::RecentFiles => &self.layout.recent_files_path,
            StorageDocumentKind::Sessions => &self.layout.sessions_path,
            StorageDocumentKind::Clock => &self.layout.clock_path,
            StorageDocumentKind::TrashManifest => &self.layout.trash_manifest_path,
        }
    }

    fn document_name(&self, kind: StorageDocumentKind) -> &'static str {
        match kind {
            StorageDocumentKind::Config => CONFIG_DESCRIPTOR.name,
            StorageDocumentKind::Users => "users",
            StorageDocumentKind::State => "state",
            StorageDocumentKind::RecentFiles => "recent-files",
            StorageDocumentKind::Sessions => "sessions",
            StorageDocumentKind::Clock => CLOCK_DESCRIPTOR.name,
            StorageDocumentKind::TrashManifest => "trash",
        }
    }

    fn validate_document(&self, kind: StorageDocumentKind) -> Result<(), StorageError> {
        let path = self.document_path(kind);
        match kind {
            StorageDocumentKind::Config => {
                validate_toml_document::<StorageConfig>(path, CONFIG_DESCRIPTOR.name)?;
            }
            StorageDocumentKind::Users => {
                validate_json_document::<UsersDocument>(path, "users")?;
            }
            StorageDocumentKind::State => {
                validate_json_document::<StateDocument>(path, "state")?;
            }
            StorageDocumentKind::RecentFiles => {
                validate_json_document::<RecentFilesDocument>(path, "recent-files")?;
            }
            StorageDocumentKind::Sessions => {
                validate_json_document::<SessionsDocument>(path, "sessions")?;
            }
            StorageDocumentKind::Clock => {
                validate_json_document::<ClockDocument>(path, CLOCK_DESCRIPTOR.name)?;
            }
            StorageDocumentKind::TrashManifest => {
                validate_json_document::<TrashDocument>(path, "trash")?;
            }
        }
        Ok(())
    }

    fn save_default_document(&self, kind: StorageDocumentKind) -> Result<(), StorageError> {
        let path = self.document_path(kind);
        match kind {
            StorageDocumentKind::Config => {
                save_toml_document(path, CONFIG_DESCRIPTOR.name, &StorageConfig::default())
            }
            StorageDocumentKind::Users => {
                save_json_document(path, "users", &UsersDocument::default())
            }
            StorageDocumentKind::State => {
                save_json_document(path, "state", &StateDocument::default())
            }
            StorageDocumentKind::RecentFiles => {
                save_json_document(path, "recent-files", &RecentFilesDocument::default())
            }
            StorageDocumentKind::Sessions => {
                save_json_document(path, "sessions", &SessionsDocument::default())
            }
            StorageDocumentKind::Clock => {
                save_json_document(path, CLOCK_DESCRIPTOR.name, &ClockDocument::default())
            }
            StorageDocumentKind::TrashManifest => {
                save_json_document(path, "trash", &TrashDocument::default())
            }
        }
    }

    fn create_document_parent(&self, path: &Path) -> Result<(), StorageError> {
        let parent = path.parent().ok_or_else(|| StorageError::MissingParent {
            path: path.to_path_buf(),
        })?;
        create_dir(parent, "create document repair directory")
    }
}
