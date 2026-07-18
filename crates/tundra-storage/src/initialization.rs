use std::path::Path;

use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::atomic_write::create_dir;
use crate::clock_document::ClockDocument;
use crate::config_document::StorageConfig;
use crate::descriptors::{CLOCK_DESCRIPTOR, CONFIG_DESCRIPTOR};
use crate::document_io::{
    load_json_document, load_toml_document, save_json_document, save_toml_document,
    validate_existing_schema, validate_json_document, validate_toml_document,
};
use crate::error::StorageError;
use crate::manager::{StorageLoadReport, StorageManager};
use crate::migration::migrate_v1_noop;
use crate::recovery::recover_document;
use crate::schema::{StorageFormat, VersionedDocument};
use crate::state_documents::{RecentFilesDocument, SessionsDocument, StateDocument};
use crate::trash_document::TrashDocument;
use crate::user_document::{UsersDocument, UsersV1Document};

impl StorageManager {
    pub(crate) fn initialize(&self) -> Result<StorageLoadReport, StorageError> {
        let mut report = StorageLoadReport::default();

        self.check_existing_future_schemas()?;
        self.create_directories()?;
        self.ensure_toml_document(
            &mut report,
            &self.layout.config_path,
            CONFIG_DESCRIPTOR.name,
            &StorageConfig::default(),
        )?;
        self.normalize_config_language(&mut report)?;
        self.ensure_users_document(&mut report)?;
        self.ensure_json_document(
            &mut report,
            &self.layout.state_path,
            "state",
            &StateDocument::default(),
        )?;
        self.ensure_json_document(
            &mut report,
            &self.layout.recent_files_path,
            "recent-files",
            &RecentFilesDocument::default(),
        )?;
        self.ensure_json_document(
            &mut report,
            &self.layout.sessions_path,
            "sessions",
            &SessionsDocument::default(),
        )?;
        self.ensure_json_document(
            &mut report,
            &self.layout.clock_path,
            CLOCK_DESCRIPTOR.name,
            &ClockDocument::default(),
        )?;
        self.ensure_json_document(
            &mut report,
            &self.layout.trash_manifest_path,
            "trash",
            &TrashDocument::default(),
        )?;

        Ok(report)
    }

    fn normalize_config_language(
        &self,
        report: &mut StorageLoadReport,
    ) -> Result<(), StorageError> {
        let mut config =
            load_toml_document::<StorageConfig>(&self.layout.config_path, CONFIG_DESCRIPTOR.name)?;
        if !config.normalize_language() {
            return Ok(());
        }

        save_toml_document(&self.layout.config_path, CONFIG_DESCRIPTOR.name, &config)?;
        if !report.migrated_files.contains(&self.layout.config_path) {
            report.migrated_files.push(self.layout.config_path.clone());
        }
        Ok(())
    }

    fn check_existing_future_schemas(&self) -> Result<(), StorageError> {
        let documents = [
            (
                self.layout.config_path.as_path(),
                CONFIG_DESCRIPTOR.name,
                StorageFormat::Toml,
            ),
            (
                self.layout.users_path.as_path(),
                "users",
                StorageFormat::VersionedJson,
            ),
            (
                self.layout.state_path.as_path(),
                "state",
                StorageFormat::VersionedJson,
            ),
            (
                self.layout.recent_files_path.as_path(),
                "recent-files",
                StorageFormat::VersionedJson,
            ),
            (
                self.layout.sessions_path.as_path(),
                "sessions",
                StorageFormat::VersionedJson,
            ),
            (
                self.layout.clock_path.as_path(),
                CLOCK_DESCRIPTOR.name,
                StorageFormat::VersionedJson,
            ),
            (
                self.layout.trash_manifest_path.as_path(),
                "trash",
                StorageFormat::VersionedJson,
            ),
        ];

        for (path, document, format) in documents {
            if !path.exists() {
                continue;
            }

            if let Err(error @ StorageError::UnsupportedSchema { .. }) =
                validate_existing_schema(path, document, format)
            {
                return Err(error);
            }
        }

        Ok(())
    }

    fn ensure_users_document(&self, report: &mut StorageLoadReport) -> Result<(), StorageError> {
        if self.layout.users_path.exists() {
            return match validate_json_document::<UsersDocument>(&self.layout.users_path, "users") {
                Ok(schema_version) => {
                    migrate_v1_noop(report, &self.layout.users_path, "users", schema_version)
                }
                Err(error @ StorageError::UnsupportedSchema { .. }) => Err(error),
                Err(error) => {
                    recover_document(report, &self.layout.users_path, "users", error)?;
                    save_json_document(
                        &self.layout.users_path,
                        "users",
                        &UsersDocument::default(),
                    )?;
                    report.created_files.push(self.layout.users_path.clone());
                    Ok(())
                }
            };
        }

        if self.layout.legacy_users_path.exists() {
            let legacy =
                load_json_document::<UsersV1Document>(&self.layout.legacy_users_path, "users")?;
            let users = UsersDocument::from_legacy_v1(legacy);
            save_json_document(&self.layout.users_path, "users", &users)?;
            report.migrated_files.push(self.layout.users_path.clone());
            return Ok(());
        }

        save_json_document(&self.layout.users_path, "users", &UsersDocument::default())?;
        report.created_files.push(self.layout.users_path.clone());
        Ok(())
    }

    fn create_directories(&self) -> Result<(), StorageError> {
        let config_parent =
            self.layout
                .config_path
                .parent()
                .ok_or_else(|| StorageError::MissingParent {
                    path: self.layout.config_path.clone(),
                })?;

        create_dir(config_parent, "create config directory")?;
        create_dir(&self.layout.data_path, "create data directory")?;
        create_dir(&self.layout.cache_path, "create cache directory")?;
        create_dir(&self.layout.logs_path, "create logs directory")?;
        create_dir(&self.layout.temp_path, "create temp directory")?;
        create_dir(&self.layout.trash_path, "create trash directory")?;

        Ok(())
    }

    fn ensure_toml_document<T>(
        &self,
        report: &mut StorageLoadReport,
        path: &Path,
        document: &'static str,
        default_document: &T,
    ) -> Result<(), StorageError>
    where
        T: Serialize + DeserializeOwned + VersionedDocument,
    {
        if !path.exists() {
            save_toml_document(path, document, default_document)?;
            report.created_files.push(path.to_path_buf());
            return Ok(());
        }

        match validate_toml_document::<T>(path, document) {
            Ok(schema_version) => {
                migrate_v1_noop(report, path, document, schema_version)?;
                Ok(())
            }
            Err(error @ StorageError::UnsupportedSchema { .. }) => Err(error),
            Err(error) => {
                recover_document(report, path, document, error)?;
                save_toml_document(path, document, default_document)?;
                report.created_files.push(path.to_path_buf());
                Ok(())
            }
        }
    }

    fn ensure_json_document<T>(
        &self,
        report: &mut StorageLoadReport,
        path: &Path,
        document: &'static str,
        default_document: &T,
    ) -> Result<(), StorageError>
    where
        T: Serialize + DeserializeOwned + VersionedDocument,
    {
        if !path.exists() {
            save_json_document(path, document, default_document)?;
            report.created_files.push(path.to_path_buf());
            return Ok(());
        }

        match validate_json_document::<T>(path, document) {
            Ok(schema_version) => {
                migrate_v1_noop(report, path, document, schema_version)?;
                Ok(())
            }
            Err(error @ StorageError::UnsupportedSchema { .. }) => Err(error),
            Err(error) => {
                recover_document(report, path, document, error)?;
                save_json_document(path, document, default_document)?;
                report.created_files.push(path.to_path_buf());
                Ok(())
            }
        }
    }
}
