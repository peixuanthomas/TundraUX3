use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fmt;
use std::path::PathBuf;
use std::time::Duration;

macro_rules! identifier {
    ($name:ident) => {
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            pub fn new(value: impl Into<String>) -> Result<Self, crate::WatchdogError> {
                let value = value.into();
                if value.is_empty()
                    || value.len() > 96
                    || !value.bytes().all(|byte| {
                        byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.')
                    })
                {
                    return Err(crate::WatchdogError::InvalidIdentifier(value));
                }
                Ok(Self(value))
            }

            pub fn from_static(value: &'static str) -> Self {
                assert!(
                    !value.is_empty()
                        && value.len() <= 96
                        && value.bytes().all(|byte| {
                            byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.')
                        }),
                    "invalid static watchdog identifier: {value}"
                );
                Self(value.to_string())
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str(&self.0)
            }
        }
    };
}

identifier!(AppId);
identifier!(ComponentId);
identifier!(TaskId);
identifier!(OperationKind);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AppCriticality {
    Optional,
    SessionCritical,
    ProcessCritical,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppDescriptor {
    pub id: AppId,
    pub display_name: String,
    pub version: String,
    pub criticality: AppCriticality,
}

impl AppDescriptor {
    pub fn new(
        id: AppId,
        display_name: impl Into<String>,
        version: impl Into<String>,
        criticality: AppCriticality,
    ) -> Self {
        Self {
            id,
            display_name: display_name.into(),
            version: version.into(),
            criticality,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BoundaryKind {
    UiSession,
    Worker,
    AsyncTask,
    BlockingIo,
    Process,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoundarySpec {
    pub id: String,
    pub kind: BoundaryKind,
    pub owns_terminal: bool,
}

impl BoundarySpec {
    pub fn new(id: impl Into<String>, kind: BoundaryKind) -> Self {
        Self {
            id: id.into(),
            kind,
            owns_terminal: false,
        }
    }

    pub fn terminal_owner(mut self) -> Self {
        self.owns_terminal = true;
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Breadcrumb {
    pub category: String,
    pub message: String,
    pub recorded_at: DateTime<Utc>,
}

impl Breadcrumb {
    pub fn new(category: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            category: category.into(),
            message: message.into(),
            recorded_at: Utc::now(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeSnapshot {
    pub screen: Option<String>,
    pub last_command: Option<String>,
    pub terminal_size: Option<(u16, u16)>,
    pub active_operation: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ErrorContext {
    pub boundary: String,
    pub severity: IncidentSeverity,
}

impl ErrorContext {
    pub fn new(boundary: impl Into<String>, severity: IncidentSeverity) -> Self {
        Self {
            boundary: boundary.into(),
            severity,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IncidentSeverity {
    Warning,
    Error,
    Critical,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IncidentKind {
    Panic,
    Error,
    UncleanExit,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "status", content = "detail")]
pub enum RecoveryOutcome {
    Pending,
    Recovered(String),
    RecoveredWithWarnings(String),
    ManualActionRequired(String),
    Unrecoverable(String),
}

impl RecoveryOutcome {
    pub fn is_recovered(&self) -> bool {
        matches!(self, Self::Recovered(_) | Self::RecoveredWithWarnings(_))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IncidentReceipt {
    pub incident_id: String,
    pub kind: IncidentKind,
    pub severity: IncidentSeverity,
    pub app_id: Option<AppId>,
    pub component: Option<String>,
    pub task_id: Option<TaskId>,
    pub task_group: Option<String>,
    pub boundary: String,
    pub panic_action: Option<PanicAction>,
    pub operation_kind: Option<OperationKind>,
    pub operation_id: Option<String>,
    pub recovery_handler_version: Option<String>,
    pub restart_attempt: usize,
    pub summary: String,
    pub recovery: RecoveryOutcome,
    pub json_report_path: Option<PathBuf>,
    pub text_report_path: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IncidentTicket {
    pub incident_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskKind {
    OneShot,
    LongRunning,
    UiSession,
    BlockingIo,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PanicAction {
    ReportOnly,
    RestartTask,
    RestartAppSession,
    EscalateProcess,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "operation")]
pub enum ReplaySafety {
    Never,
    Idempotent,
    Checkpointed(OperationKind),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RestartPolicy {
    pub max_restarts: usize,
    pub window: Duration,
    pub backoff: Vec<Duration>,
}

impl RestartPolicy {
    pub fn never() -> Self {
        Self {
            max_restarts: 0,
            window: Duration::ZERO,
            backoff: Vec::new(),
        }
    }

    pub fn limited(max_restarts: usize, window: Duration, backoff: Vec<Duration>) -> Self {
        Self {
            max_restarts,
            window,
            backoff,
        }
    }

    pub(crate) fn delay_for(&self, restart_index: usize) -> Duration {
        self.backoff
            .get(restart_index)
            .copied()
            .or_else(|| self.backoff.last().copied())
            .unwrap_or(Duration::ZERO)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskSpec {
    pub id: TaskId,
    pub kind: TaskKind,
    pub panic_action: PanicAction,
    pub replay_safety: ReplaySafety,
    pub restart_policy: RestartPolicy,
}

impl TaskSpec {
    pub fn one_shot(id: TaskId) -> Self {
        Self {
            id,
            kind: TaskKind::OneShot,
            panic_action: PanicAction::ReportOnly,
            replay_safety: ReplaySafety::Never,
            restart_policy: RestartPolicy::never(),
        }
    }

    pub fn idempotent_service(id: TaskId, restart_policy: RestartPolicy) -> Self {
        Self {
            id,
            kind: TaskKind::LongRunning,
            panic_action: PanicAction::RestartTask,
            replay_safety: ReplaySafety::Idempotent,
            restart_policy,
        }
    }

    pub fn validate(&self) -> Result<(), crate::WatchdogError> {
        if self.panic_action == PanicAction::RestartTask
            && self.replay_safety == ReplaySafety::Never
        {
            return Err(crate::WatchdogError::InvalidTaskPolicy(format!(
                "task {} cannot restart because its replay safety is Never",
                self.id
            )));
        }
        if self.panic_action == PanicAction::RestartTask && self.restart_policy.max_restarts == 0 {
            return Err(crate::WatchdogError::InvalidTaskPolicy(format!(
                "task {} requests restart but has a zero restart limit",
                self.id
            )));
        }
        if self.panic_action == PanicAction::RestartTask && self.restart_policy.window.is_zero() {
            return Err(crate::WatchdogError::InvalidTaskPolicy(format!(
                "task {} requests restart but has a zero restart window",
                self.id
            )));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OperationDescriptor {
    pub id: String,
    pub kind: OperationKind,
    pub summary: String,
    pub payload: Value,
}

impl OperationDescriptor {
    pub fn new(kind: OperationKind, summary: impl Into<String>, payload: Value) -> Self {
        Self {
            id: String::new(),
            kind,
            summary: summary.into(),
            payload,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OperationCheckpoint {
    pub phase: String,
    pub payload: Value,
}

impl OperationCheckpoint {
    pub fn new(phase: impl Into<String>, payload: Value) -> Self {
        Self {
            phase: phase.into(),
            payload,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationStatus {
    Active,
    Committed,
    Interrupted,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OperationRecord {
    pub schema_version: u32,
    pub run_id: String,
    pub app_id: AppId,
    pub component: String,
    pub operation_id: String,
    pub kind: OperationKind,
    pub replay_safety: ReplaySafety,
    pub recovery_handler_version: Option<String>,
    pub summary: String,
    #[serde(default)]
    pub original_payload: Value,
    pub checkpoint_sequence: u64,
    pub checkpoint: OperationCheckpoint,
    pub status: OperationStatus,
    pub started_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

pub trait RecoveryHandler: Send + Sync + 'static {
    fn version(&self) -> &str;
    fn recover(&self, record: &OperationRecord) -> RecoveryOutcome;
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TaskGroupShutdown {
    pub completed: usize,
    pub still_running: usize,
}
