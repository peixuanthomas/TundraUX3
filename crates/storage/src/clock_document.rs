use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::schema::{SCHEMA_VERSION, VersionedDocument};

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

fn default_schema_version() -> u32 {
    SCHEMA_VERSION
}

fn default_clock_next_id() -> u64 {
    1
}
