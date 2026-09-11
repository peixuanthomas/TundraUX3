use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LogSource {
    #[default]
    Ux,
    Linux,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LogLevel {
    Trace,
    Debug,
    #[default]
    Info,
    Warning,
    Error,
    Critical,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LogPhase {
    Started,
    Succeeded,
    Failed,
    Retry,
    Degraded,
    Recovered,
    Cancelled,
    Repeated,
    Incident,
    #[default]
    Observed,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LogContext {
    pub run_id: Option<String>,
    pub app: String,
    pub module: String,
    pub operation: String,
    pub operation_id: Option<String>,
    pub task_id: Option<String>,
    pub owner_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeLogEvent {
    pub schema_version: u32,
    pub event_id: String,
    pub timestamp: DateTime<Utc>,
    pub process_id: u32,
    pub source: LogSource,
    pub level: LogLevel,
    pub phase: LogPhase,
    pub context: LogContext,
    pub message: String,
    /// Optional localization metadata; `message` remains the readable fallback.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message_id: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub message_args: BTreeMap<String, serde_json::Value>,
    pub error_code: Option<String>,
    pub os_error_code: Option<i64>,
    pub error_chain: Vec<String>,
    pub source_path: Option<PathBuf>,
    pub target_path: Option<PathBuf>,
    pub incident_id: Option<String>,
    pub alert_key: Option<String>,
    pub repeat_count: u64,
    pub retry_count: u64,
    pub first_seen: Option<DateTime<Utc>>,
    pub last_seen: Option<DateTime<Utc>>,
    pub native_priority: Option<u8>,
    pub native_source: Option<String>,
    pub timestamp_note: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LogQuery {
    pub source: LogSource,
    pub since: Option<DateTime<Utc>>,
    pub until: Option<DateTime<Utc>>,
    pub min_level: Option<LogLevel>,
    pub module: Option<String>,
    pub run_id: Option<String>,
    pub operation_id: Option<String>,
    pub task_id: Option<String>,
    pub incident_id: Option<String>,
    pub owner_id: Option<String>,
    pub limit: usize,
}
impl Default for LogQuery {
    fn default() -> Self {
        Self {
            source: LogSource::Ux,
            since: None,
            until: None,
            min_level: None,
            module: None,
            run_id: None,
            operation_id: None,
            task_id: None,
            incident_id: None,
            owner_id: None,
            limit: 200,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LogSourceState {
    #[default]
    Ready,
    Unsupported,
    PermissionDenied,
    Unavailable,
    Partial,
    Cancelled,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LogQueryResult {
    pub events: Vec<RuntimeLogEvent>,
    pub state: LogSourceState,
    pub notices: Vec<String>,
    pub truncated: bool,
    pub damaged_records: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LogWriterHealth {
    pub dropped_events: u64,
    pub written_events: u64,
    pub write_failures: u64,
    pub last_error: Option<String>,
    pub pending_events: usize,
}
