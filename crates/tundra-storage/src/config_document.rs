use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::schema::{SCHEMA_VERSION, VersionedDocument};

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
pub struct LauncherConfig {
    pub pinned_apps: Vec<String>,
    pub pinned_dirs: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct SecurityConfig {
    pub allow_release_debug: bool,
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
