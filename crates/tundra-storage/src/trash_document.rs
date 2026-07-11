use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::schema::{SCHEMA_VERSION, VersionedDocument};

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
