use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::schema::{SCHEMA_VERSION, VersionedDocument};

pub const SUPPORTED_LANGUAGE: &str = "en-US";

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
    pub appearance: AppearanceConfig,
    #[serde(default)]
    pub explorer: ExplorerConfig,
    #[serde(default)]
    pub editor: EditorConfig,
    #[serde(default)]
    pub launcher: LauncherConfig,
    #[serde(default)]
    pub security: SecurityConfig,
}

impl StorageConfig {
    pub(crate) fn normalize(&mut self) -> bool {
        let mut changed = self.launcher.migrate_legacy_pinned_apps();
        if self.language != SUPPORTED_LANGUAGE {
            self.language = SUPPORTED_LANGUAGE.to_string();
            changed = true;
        }
        changed
    }
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            theme: default_theme(),
            language: default_language(),
            timezone: default_timezone(),
            shortcuts: BTreeMap::new(),
            appearance: AppearanceConfig::default(),
            explorer: ExplorerConfig::default(),
            editor: EditorConfig::default(),
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
#[serde(default)]
pub struct AppearanceConfig {
    pub border_shape: BorderShape,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BorderShape {
    #[default]
    Rounded,
    Square,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct ExplorerConfig {
    pub show_hidden: bool,
    pub show_system: bool,
    pub show_extensions: bool,
    pub folders_first: bool,
    pub case_sensitive_sort: bool,
    pub size_format: ExplorerSizeFormat,
    pub date_zone: ExplorerDateZone,
    pub confirm_delete: bool,
    pub confirm_name_conflicts: bool,
    pub show_sidebar: bool,
    pub sort_field: ExplorerSortField,
    pub sort_direction: ExplorerSortDirection,
}

impl Default for ExplorerConfig {
    fn default() -> Self {
        Self {
            show_hidden: false,
            show_system: false,
            show_extensions: true,
            folders_first: true,
            case_sensitive_sort: false,
            size_format: ExplorerSizeFormat::HumanBinary,
            date_zone: ExplorerDateZone::ConfiguredTimezone,
            confirm_delete: true,
            confirm_name_conflicts: true,
            show_sidebar: true,
            sort_field: ExplorerSortField::Name,
            sort_direction: ExplorerSortDirection::Ascending,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct EditorConfig {
    pub cursor_acceleration_enabled: bool,
    pub cursor_acceleration_delay_ms: u32,
    pub cursor_acceleration_ramp_ms: u32,
    pub cursor_horizontal_max_step: u8,
    pub cursor_vertical_max_step: u8,
}

impl Default for EditorConfig {
    fn default() -> Self {
        Self {
            cursor_acceleration_enabled: true,
            cursor_acceleration_delay_ms: 2_000,
            cursor_acceleration_ramp_ms: 3_000,
            cursor_horizontal_max_step: 8,
            cursor_vertical_max_step: 3,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ExplorerSizeFormat {
    #[default]
    HumanBinary,
    Bytes,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ExplorerDateZone {
    #[default]
    ConfiguredTimezone,
    Utc,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ExplorerSortField {
    #[default]
    Name,
    Type,
    Size,
    Modified,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ExplorerSortDirection {
    #[default]
    Ascending,
    Descending,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct LauncherConfig {
    pub entries: Vec<LauncherEntryRecord>,
    /// Legacy input retained only so schema-1 configurations can be read. Values are moved into
    /// `entries` during normalization and are never emitted again.
    #[serde(skip_serializing)]
    pub pinned_apps: Vec<String>,
    /// Legacy directory pins remain readable for backwards compatibility, but Launcher does not
    /// treat directories as executable entries.
    pub pinned_dirs: Vec<String>,
}

impl LauncherConfig {
    fn migrate_legacy_pinned_apps(&mut self) -> bool {
        if self.pinned_apps.is_empty() {
            return false;
        }

        for path in &self.pinned_apps {
            if self.entries.iter().any(|entry| entry.path == *path) {
                continue;
            }
            self.entries.push(LauncherEntryRecord {
                id: legacy_launcher_entry_id(path),
                path: path.clone(),
                executable_kind: None,
                fingerprint: None,
                added_by_user_id: "legacy".to_string(),
                added_at_epoch_ms: 0,
            });
        }
        self.pinned_apps.clear();
        true
    }
}

/// A globally-managed application approved for Launcher execution.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct LauncherEntryRecord {
    pub id: String,
    /// A canonical, absolute path recorded by the admin approval workflow.
    pub path: String,
    /// Missing only for entries migrated from the obsolete `pinned_apps` setting; such entries
    /// require fresh admin approval before they can be launched.
    pub executable_kind: Option<LauncherExecutableKind>,
    pub fingerprint: Option<LauncherFingerprint>,
    pub added_by_user_id: String,
    pub added_at_epoch_ms: i64,
}

impl Default for LauncherEntryRecord {
    fn default() -> Self {
        Self {
            id: String::new(),
            path: String::new(),
            executable_kind: None,
            fingerprint: None,
            added_by_user_id: String::new(),
            added_at_epoch_ms: 0,
        }
    }
}

/// The executable classification persisted with a Launcher approval.
///
/// This mirrors `tundra_platform::ExecutableKind` without making storage serialization depend on
/// a platform-facing enum. Application code converts between the two at its boundary.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LauncherExecutableKind {
    NativeBinary,
    Installer,
    Script,
    Shortcut,
    ApplicationBundle,
}

/// Content identity captured when an administrator approves a Launcher entry.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct LauncherFingerprint {
    pub sha256: String,
    pub byte_len: u64,
    pub modified_at_epoch_ms: Option<i64>,
}

fn legacy_launcher_entry_id(path: &str) -> String {
    // FNV-1a makes the migration deterministic without introducing a hashing dependency. This
    // ID identifies a record only; it is never used as an integrity check.
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in path.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("legacy-{hash:016x}")
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct SecurityConfig {
    /// Retained for backward-compatible config parsing. Release builds never enable debug mode.
    pub allow_release_debug: bool,
}

fn default_theme() -> String {
    "dark".to_string()
}

fn default_language() -> String {
    SUPPORTED_LANGUAGE.to_string()
}

fn default_timezone() -> String {
    "UTC".to_string()
}
