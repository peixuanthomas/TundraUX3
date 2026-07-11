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

fn default_theme() -> String {
    "dark".to_string()
}

fn default_language() -> String {
    "en-US".to_string()
}

fn default_timezone() -> String {
    "UTC".to_string()
}
