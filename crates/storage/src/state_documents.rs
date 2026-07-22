use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::schema::{SCHEMA_VERSION, VersionedDocument};

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
