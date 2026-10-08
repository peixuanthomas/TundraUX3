use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Delivered over the private operation connection, never rendered as a log.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConfigDocument {
    pub path: PathBuf,
    pub content: String,
    pub version: String,
    pub existed: bool,
    pub uid: u32,
    pub gid: u32,
    pub mode: u32,
    pub validator: String,
    pub check: ConfigCheck,
    pub backup_id: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfigCheck {
    #[default]
    NotChecked,
    Passed,
    Unavailable(String),
    Failed(String),
}
