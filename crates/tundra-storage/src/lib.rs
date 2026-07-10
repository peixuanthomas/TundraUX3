use std::collections::BTreeMap;
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Deserialize;
use serde::Serialize;
use serde::de::DeserializeOwned;
use tundra_platform::{AppPaths, Platform};

pub const SCHEMA_VERSION: u32 = 1;
pub const USERS_SCHEMA_VERSION: u32 = 2;
const USERS_V1_FILE_NAME: &str = "users.v1.json";
const USERS_V2_FILE_NAME: &str = "users.v2.json";
const TRASH_DIR_NAME: &str = "trash";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StorageFormat {
    Toml,
    VersionedJson,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StorageDescriptor {
    pub name: &'static str,
    pub file_name: &'static str,
    pub format: StorageFormat,
    pub schema_version: u32,
}

pub const CONFIG_DESCRIPTOR: StorageDescriptor = StorageDescriptor {
    name: "config",
    file_name: "config.toml",
    format: StorageFormat::Toml,
    schema_version: SCHEMA_VERSION,
};

pub const CLOCK_DESCRIPTOR: StorageDescriptor = StorageDescriptor {
    name: "clock",
    file_name: "clock.v1.json",
    format: StorageFormat::VersionedJson,
    schema_version: SCHEMA_VERSION,
};

pub const VERSIONED_JSON_DESCRIPTORS: &[StorageDescriptor] = &[
    StorageDescriptor {
        name: "users",
        file_name: USERS_V2_FILE_NAME,
        format: StorageFormat::VersionedJson,
        schema_version: USERS_SCHEMA_VERSION,
    },
    StorageDescriptor {
        name: "state",
        file_name: "state.v1.json",
        format: StorageFormat::VersionedJson,
        schema_version: SCHEMA_VERSION,
    },
    StorageDescriptor {
        name: "recent-files",
        file_name: "recent-files.v1.json",
        format: StorageFormat::VersionedJson,
        schema_version: SCHEMA_VERSION,
    },
    StorageDescriptor {
        name: "sessions",
        file_name: "sessions.v1.json",
        format: StorageFormat::VersionedJson,
        schema_version: SCHEMA_VERSION,
    },
    CLOCK_DESCRIPTOR,
    StorageDescriptor {
        name: "trash",
        file_name: "trash.v1.json",
        format: StorageFormat::VersionedJson,
        schema_version: SCHEMA_VERSION,
    },
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StorageLayout {
    pub config_path: PathBuf,
    pub data_path: PathBuf,
    pub cache_path: PathBuf,
    pub logs_path: PathBuf,
    pub temp_path: PathBuf,
    pub users_path: PathBuf,
    pub legacy_users_path: PathBuf,
    pub state_path: PathBuf,
    pub recent_files_path: PathBuf,
    pub sessions_path: PathBuf,
    pub clock_path: PathBuf,
    pub trash_path: PathBuf,
    pub trash_manifest_path: PathBuf,
    pub audit_log_path: PathBuf,
}

impl StorageLayout {
    pub fn from_app_paths(app_paths: &AppPaths) -> Self {
        let data_path = app_paths.data_path().to_path_buf();
        let trash_path = data_path.join(TRASH_DIR_NAME);

        Self {
            config_path: app_paths.config_path().to_path_buf(),
            data_path: data_path.clone(),
            cache_path: app_paths.cache_path().to_path_buf(),
            logs_path: app_paths.logs_path().to_path_buf(),
            temp_path: app_paths.temp_path().to_path_buf(),
            users_path: data_path.join(VERSIONED_JSON_DESCRIPTORS[0].file_name),
            legacy_users_path: data_path.join(USERS_V1_FILE_NAME),
            state_path: data_path.join(VERSIONED_JSON_DESCRIPTORS[1].file_name),
            recent_files_path: data_path.join(VERSIONED_JSON_DESCRIPTORS[2].file_name),
            sessions_path: data_path.join(VERSIONED_JSON_DESCRIPTORS[3].file_name),
            clock_path: data_path.join(CLOCK_DESCRIPTOR.file_name),
            trash_manifest_path: trash_path.join(VERSIONED_JSON_DESCRIPTORS[5].file_name),
            trash_path,
            audit_log_path: app_paths.logs_path().join("audit.v1.log"),
        }
    }

    pub fn audit_path(&self) -> &Path {
        &self.audit_log_path
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StorageManager {
    layout: StorageLayout,
}

impl StorageManager {
    pub fn open(app_paths: AppPaths) -> Result<StorageOpen, StorageError> {
        let layout = StorageLayout::from_app_paths(&app_paths);
        let manager = Self { layout };
        let report = manager.initialize()?;

        Ok(StorageOpen { manager, report })
    }

    pub fn open_from_platform(platform: &dyn Platform) -> Result<StorageOpen, StorageError> {
        let app_paths = platform
            .app_paths()
            .map_err(|error| StorageError::Platform {
                message: error.to_string(),
            })?;

        Self::open(app_paths)
    }

    pub fn from_layout(layout: StorageLayout) -> Self {
        Self { layout }
    }

    pub fn layout(&self) -> &StorageLayout {
        &self.layout
    }

    pub fn load_config(&self) -> Result<StorageConfig, StorageError> {
        load_toml_document(&self.layout.config_path, CONFIG_DESCRIPTOR.name)
    }

    pub fn save_config(&self, config: &StorageConfig) -> Result<(), StorageError> {
        save_toml_document(&self.layout.config_path, CONFIG_DESCRIPTOR.name, config)
    }

    pub fn load_users(&self) -> Result<UsersDocument, StorageError> {
        load_json_document(&self.layout.users_path, "users")
    }

    pub fn save_users(&self, users: &UsersDocument) -> Result<(), StorageError> {
        save_json_document(&self.layout.users_path, "users", users)
    }

    pub fn load_state(&self) -> Result<StateDocument, StorageError> {
        load_json_document(&self.layout.state_path, "state")
    }

    pub fn save_state(&self, state: &StateDocument) -> Result<(), StorageError> {
        save_json_document(&self.layout.state_path, "state", state)
    }

    pub fn load_recent_files(&self) -> Result<RecentFilesDocument, StorageError> {
        load_json_document(&self.layout.recent_files_path, "recent-files")
    }

    pub fn load_recent(&self) -> Result<RecentFilesDocument, StorageError> {
        self.load_recent_files()
    }

    pub fn save_recent_files(
        &self,
        recent_files: &RecentFilesDocument,
    ) -> Result<(), StorageError> {
        save_json_document(&self.layout.recent_files_path, "recent-files", recent_files)
    }

    pub fn save_recent(&self, recent_files: &RecentFilesDocument) -> Result<(), StorageError> {
        self.save_recent_files(recent_files)
    }

    pub fn load_sessions(&self) -> Result<SessionsDocument, StorageError> {
        load_json_document(&self.layout.sessions_path, "sessions")
    }

    pub fn save_sessions(&self, sessions: &SessionsDocument) -> Result<(), StorageError> {
        save_json_document(&self.layout.sessions_path, "sessions", sessions)
    }

    pub fn load_clock(&self) -> Result<ClockDocument, StorageError> {
        load_json_document(&self.layout.clock_path, CLOCK_DESCRIPTOR.name)
    }

    pub fn save_clock(&self, clock: &ClockDocument) -> Result<(), StorageError> {
        save_json_document(&self.layout.clock_path, CLOCK_DESCRIPTOR.name, clock)
    }

    pub fn load_trash(&self) -> Result<TrashDocument, StorageError> {
        load_json_document(&self.layout.trash_manifest_path, "trash")
    }

    pub fn save_trash(&self, trash: &TrashDocument) -> Result<(), StorageError> {
        save_json_document(&self.layout.trash_manifest_path, "trash", trash)
    }

    pub fn append_audit_line(&self, line: &str) -> Result<(), StorageError> {
        let parent =
            self.layout
                .audit_log_path
                .parent()
                .ok_or_else(|| StorageError::MissingParent {
                    path: self.layout.audit_log_path.clone(),
                })?;
        create_dir(parent, "create audit log directory")?;

        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.layout.audit_log_path)
            .map_err(|error| StorageError::Io {
                operation: "open audit log",
                path: self.layout.audit_log_path.clone(),
                message: error.to_string(),
            })?;
        file.write_all(line.as_bytes())
            .and_then(|_| file.write_all(b"\n"))
            .and_then(|_| file.sync_all())
            .map_err(|error| StorageError::Io {
                operation: "append audit log",
                path: self.layout.audit_log_path.clone(),
                message: error.to_string(),
            })
    }

    pub fn read_audit_lines(&self) -> Result<Vec<String>, StorageError> {
        if !self.layout.audit_log_path.exists() {
            return Ok(Vec::new());
        }

        let file = File::open(&self.layout.audit_log_path).map_err(|error| StorageError::Io {
            operation: "open audit log",
            path: self.layout.audit_log_path.clone(),
            message: error.to_string(),
        })?;
        BufReader::new(file)
            .lines()
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| StorageError::Io {
                operation: "read audit log",
                path: self.layout.audit_log_path.clone(),
                message: error.to_string(),
            })
    }

    fn initialize(&self) -> Result<StorageLoadReport, StorageError> {
        let mut report = StorageLoadReport::default();

        self.check_existing_future_schemas()?;
        self.create_directories()?;
        self.ensure_toml_document(
            &mut report,
            &self.layout.config_path,
            CONFIG_DESCRIPTOR.name,
            &StorageConfig::default(),
        )?;
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StorageOpen {
    pub manager: StorageManager,
    pub report: StorageLoadReport,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StorageLoadReport {
    pub warnings: Vec<String>,
    pub created_files: Vec<PathBuf>,
    pub migrated_files: Vec<PathBuf>,
    pub recovered_files: Vec<RecoveredFile>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecoveredFile {
    pub original_path: PathBuf,
    pub recovered_path: PathBuf,
}

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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StorageConfig {
    pub schema_version: u32,
    #[serde(default = "default_theme")]
    pub theme: String,
    #[serde(default = "default_language")]
    pub language: String,
    #[serde(default = "default_timezone")]
    pub timezone: String,
    #[serde(default)]
    pub shortcuts: BTreeMap<String, String>,
    #[serde(default)]
    pub explorer: ExplorerConfig,
    #[serde(default)]
    pub launcher: LauncherConfig,
    #[serde(default)]
    pub security: SecurityConfig,
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            theme: default_theme(),
            language: default_language(),
            timezone: default_timezone(),
            shortcuts: BTreeMap::new(),
            explorer: ExplorerConfig::default(),
            launcher: LauncherConfig::default(),
            security: SecurityConfig::default(),
        }
    }
}

impl VersionedDocument for StorageConfig {
    fn schema_version(&self) -> u32 {
        self.schema_version
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExplorerConfig {
    pub show_hidden: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct LauncherConfig {
    pub pinned_apps: Vec<String>,
    pub pinned_dirs: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct SecurityConfig {
    pub allow_release_debug: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UsersDocument {
    pub schema_version: u32,
    #[serde(default)]
    pub users: Vec<UserRecord>,
}

impl Default for UsersDocument {
    fn default() -> Self {
        Self {
            schema_version: USERS_SCHEMA_VERSION,
            users: Vec::new(),
        }
    }
}

impl VersionedDocument for UsersDocument {
    fn schema_version(&self) -> u32 {
        self.schema_version
    }
}

impl UsersDocument {
    fn from_legacy_v1(legacy: UsersV1Document) -> Self {
        let now = unix_millis();
        let users = legacy
            .users
            .into_iter()
            .enumerate()
            .map(|(index, username)| {
                let id = format!("legacy-user-{}", index + 1);
                UserRecord {
                    id,
                    username: username.clone(),
                    display_name: username,
                    role: "User".to_string(),
                    password_hash: String::new(),
                    password_hint: None,
                    enabled: false,
                    failed_login_attempts: 0,
                    locked_until_epoch_ms: None,
                    created_at_epoch_ms: now,
                    updated_at_epoch_ms: now,
                    last_login_at_epoch_ms: None,
                }
            })
            .collect();

        Self {
            schema_version: USERS_SCHEMA_VERSION,
            users,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UserRecord {
    pub id: String,
    pub username: String,
    pub display_name: String,
    pub role: String,
    pub password_hash: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub password_hint: Option<String>,
    pub enabled: bool,
    pub failed_login_attempts: u32,
    pub locked_until_epoch_ms: Option<u64>,
    pub created_at_epoch_ms: u64,
    pub updated_at_epoch_ms: u64,
    pub last_login_at_epoch_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct UsersV1Document {
    pub schema_version: u32,
    #[serde(default)]
    pub users: Vec<String>,
}

impl VersionedDocument for UsersV1Document {
    fn schema_version(&self) -> u32 {
        self.schema_version
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StateDocument {
    pub schema_version: u32,
    #[serde(default)]
    pub values: BTreeMap<String, String>,
}

impl Default for StateDocument {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            values: BTreeMap::new(),
        }
    }
}

impl VersionedDocument for StateDocument {
    fn schema_version(&self) -> u32 {
        self.schema_version
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RecentFilesDocument {
    pub schema_version: u32,
    #[serde(default)]
    pub files: Vec<String>,
}

impl Default for RecentFilesDocument {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            files: Vec::new(),
        }
    }
}

impl VersionedDocument for RecentFilesDocument {
    fn schema_version(&self) -> u32 {
        self.schema_version
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionsDocument {
    pub schema_version: u32,
    #[serde(default)]
    pub sessions: Vec<String>,
}

impl Default for SessionsDocument {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            sessions: Vec::new(),
        }
    }
}

impl VersionedDocument for SessionsDocument {
    fn schema_version(&self) -> u32 {
        self.schema_version
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ClockDocument {
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    #[serde(default)]
    pub profiles: BTreeMap<String, ClockProfile>,
}

impl Default for ClockDocument {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            profiles: BTreeMap::new(),
        }
    }
}

impl VersionedDocument for ClockDocument {
    fn schema_version(&self) -> u32 {
        self.schema_version
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ClockProfile {
    #[serde(default = "default_clock_next_id")]
    pub next_id: u64,
    #[serde(default)]
    pub entries: Vec<ClockEntryRecord>,
}

impl Default for ClockProfile {
    fn default() -> Self {
        Self {
            next_id: default_clock_next_id(),
            entries: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ClockEntryRecord {
    DailyAlarm {
        #[serde(default)]
        id: u64,
        #[serde(default)]
        hour: u8,
        #[serde(default)]
        minute: u8,
        #[serde(default)]
        second: u8,
        #[serde(default)]
        strong: bool,
        #[serde(default)]
        snooze_deadline_epoch_ms: Option<u64>,
    },
    Countdown {
        #[serde(default)]
        id: u64,
        #[serde(default)]
        deadline_epoch_ms: u64,
        #[serde(default)]
        strong: bool,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TrashDocument {
    pub schema_version: u32,
    #[serde(default)]
    pub records: Vec<TrashRecord>,
}

impl Default for TrashDocument {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            records: Vec::new(),
        }
    }
}

impl VersionedDocument for TrashDocument {
    fn schema_version(&self) -> u32 {
        self.schema_version
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TrashRecord {
    pub original_path: PathBuf,
    pub trash_path: PathBuf,
    pub actor: String,
    pub timestamp_epoch_ms: u64,
}

pub trait VersionedDocument {
    fn schema_version(&self) -> u32;
}

fn default_theme() -> String {
    "dark".to_string()
}

fn default_language() -> String {
    "en-US".to_string()
}

fn default_timezone() -> String {
    "UTC".to_string()
}

fn default_schema_version() -> u32 {
    SCHEMA_VERSION
}

fn default_clock_next_id() -> u64 {
    1
}

fn create_dir(path: &Path, operation: &'static str) -> Result<(), StorageError> {
    fs::create_dir_all(path).map_err(|error| StorageError::Io {
        operation,
        path: path.to_path_buf(),
        message: error.to_string(),
    })
}

fn migrate_v1_noop(
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

fn load_toml_document<T>(path: &Path, document: &'static str) -> Result<T, StorageError>
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

fn save_toml_document<T>(path: &Path, document: &'static str, value: &T) -> Result<(), StorageError>
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

fn validate_toml_document<T>(path: &Path, document: &'static str) -> Result<u32, StorageError>
where
    T: DeserializeOwned,
{
    let _: T = load_toml_document(path, document)?;
    let contents = read_to_string(path, "read TOML document")?;
    toml_schema_version(&contents, path, document)
}

fn load_json_document<T>(path: &Path, document: &'static str) -> Result<T, StorageError>
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

fn save_json_document<T>(path: &Path, document: &'static str, value: &T) -> Result<(), StorageError>
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

fn validate_json_document<T>(path: &Path, document: &'static str) -> Result<u32, StorageError>
where
    T: DeserializeOwned,
{
    let _: T = load_json_document(path, document)?;
    let contents = read_to_string(path, "read JSON document")?;
    json_schema_version(&contents, path, document)
}

fn validate_existing_schema(
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

fn ensure_document_schema(
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

fn ensure_supported_schema(
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

fn supported_schema_version(document: &str) -> u32 {
    match document {
        "users" => USERS_SCHEMA_VERSION,
        _ => SCHEMA_VERSION,
    }
}

fn toml_schema_version(
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

fn json_schema_version(
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

fn recover_document(
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

fn read_to_string(path: &Path, operation: &'static str) -> Result<String, StorageError> {
    fs::read_to_string(path).map_err(|error| StorageError::Io {
        operation,
        path: path.to_path_buf(),
        message: error.to_string(),
    })
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), StorageError> {
    let parent = path.parent().ok_or_else(|| StorageError::MissingParent {
        path: path.to_path_buf(),
    })?;
    create_dir(parent, "create storage parent directory")?;

    for _ in 0..64 {
        let temp_path = temp_write_path(path);
        let mut file = match OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temp_path)
        {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(StorageError::Io {
                    operation: "create temporary storage file",
                    path: temp_path,
                    message: error.to_string(),
                });
            }
        };

        if let Err(error) = write_and_sync(&mut file, bytes) {
            let _ = fs::remove_file(&temp_path);
            return Err(StorageError::Io {
                operation: "write temporary storage file",
                path: temp_path,
                message: error.to_string(),
            });
        }

        drop(file);

        if let Err(error) = replace_file(&temp_path, path) {
            let _ = fs::remove_file(&temp_path);
            return Err(StorageError::Io {
                operation: "replace storage file",
                path: path.to_path_buf(),
                message: error.to_string(),
            });
        }

        return Ok(());
    }

    Err(StorageError::Io {
        operation: "create temporary storage file",
        path: parent.to_path_buf(),
        message: "could not create a unique temporary file".to_string(),
    })
}

fn write_and_sync(file: &mut File, bytes: &[u8]) -> Result<(), std::io::Error> {
    file.write_all(bytes)?;
    file.sync_all()
}

fn temp_write_path(path: &Path) -> PathBuf {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let file_name = path
        .file_name()
        .map(|name| name.to_string_lossy())
        .unwrap_or_else(|| "document".into());

    parent.join(format!(
        ".{file_name}.tmp.{}.{}",
        process::id(),
        unix_nanos()
    ))
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

fn unix_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .ok()
        .and_then(|millis| u64::try_from(millis).ok())
        .unwrap_or(0)
}

#[cfg(not(windows))]
fn replace_file(from: &Path, to: &Path) -> Result<(), std::io::Error> {
    fs::rename(from, to)
}

#[cfg(windows)]
fn replace_file(from: &Path, to: &Path) -> Result<(), std::io::Error> {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;

    const MOVEFILE_REPLACE_EXISTING: u32 = 0x1;
    const MOVEFILE_WRITE_THROUGH: u32 = 0x8;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn MoveFileExW(
            existing_file_name: *const u16,
            new_file_name: *const u16,
            flags: u32,
        ) -> i32;
    }

    fn wide_null(value: &OsStr) -> Vec<u16> {
        value.encode_wide().chain(std::iter::once(0)).collect()
    }

    let from = wide_null(from.as_os_str());
    let to = wide_null(to.as_os_str());
    let result = unsafe {
        MoveFileExW(
            from.as_ptr(),
            to.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };

    if result == 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}
