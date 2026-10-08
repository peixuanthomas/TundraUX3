//! Typed requests for the Linux management applications. No caller-supplied shell commands.
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
mod configuration;
pub use configuration::{ConfigCheck, ConfigDocument};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ManagementKind {
    Services,
    Processes,
    Packages,
    Network,
    Disks,
    Users,
    SystemConfig,
}

impl ManagementKind {
    pub fn id(self) -> &'static str {
        match self {
            Self::Services => "services",
            Self::Processes => "processes",
            Self::Packages => "packages",
            Self::Network => "network",
            Self::Disks => "disks",
            Self::Users => "users",
            Self::SystemConfig => "system-config",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManagementQuery {
    pub kind: ManagementKind,
    pub filter: String,
    pub scope: String,
    pub target: Option<String>,
    #[serde(default)]
    pub options: BTreeMap<String, String>,
}

impl ManagementQuery {
    pub fn new(kind: ManagementKind) -> Self {
        Self {
            kind,
            filter: String::new(),
            scope: String::new(),
            target: None,
            options: BTreeMap::new(),
        }
    }
}

/// Labels are English fallbacks; the Shell translates stable field/action IDs.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManagementField {
    pub id: String,
    pub label: String,
    pub value: String,
    pub secret: bool,
    pub required: bool,
    #[serde(default)]
    pub choices: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManagementAction {
    pub id: String,
    pub label: String,
    pub fields: Vec<ManagementField>,
    pub confirm: bool,
    pub privileged: bool,
    pub disabled_reason: Option<String>,
    #[serde(default)]
    pub values: BTreeMap<String, String>,
    #[serde(default)]
    pub group: String,
    #[serde(default)]
    pub primary: bool,
}

/// A proposed configuration, never an instruction to write it without review.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConfigDraft {
    /// Source used to build a draft. The editor compares this before applying it.
    #[serde(default)]
    pub expected_content: Option<String>,
    pub path: PathBuf,
    pub content: String,
    pub validator: String,
    pub service: Option<String>,
    pub scope: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManagementRow {
    pub id: String,
    pub cells: Vec<String>,
    pub detail: Vec<(String, String)>,
    pub actions: Vec<ManagementAction>,
    /// Stable identity information, revalidated by the backend before any modification.
    pub identity: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManagementSnapshot {
    pub columns: Vec<String>,
    pub rows: Vec<ManagementRow>,
    pub actions: Vec<ManagementAction>,
    pub notices: Vec<String>,
    pub backend: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManagementCommand {
    pub kind: ManagementKind,
    pub action: String,
    pub target: Option<String>,
    #[serde(default)]
    pub values: BTreeMap<String, String>,
    #[serde(default)]
    pub identity: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionContext {
    pub actor_uid: u32,
    pub helper_path: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum OperationEvent {
    ConfigDocument {
        document: ConfigDocument,
    },
    Connected {
        operation_id: String,
    },
    Problem {
        problem: problem::OperationProblem,
    },
    Disconnected {
        message: String,
    },
    Started {
        kind: ManagementKind,
        action: String,
        target: Option<String>,
    },
    Snapshot {
        snapshot: ManagementSnapshot,
    },
    TerminalOutput {
        bytes: Vec<u8>,
    },
    Progress {
        message: String,
        percent: Option<u8>,
    },
    Output {
        text: String,
    },
    Question {
        id: String,
        prompt: String,
        choices: Vec<String>,
        secret: bool,
    },
    Completed {
        message: String,
    },
    Failed {
        message: String,
    },
}

pub trait OperationInteraction {
    fn emit(&mut self, event: OperationEvent);
    fn ask(
        &mut self,
        id: &str,
        prompt: &str,
        choices: &[String],
        secret: bool,
    ) -> Result<String, ManagementError>;
    fn terminal_input(
        &mut self,
        _timeout: std::time::Duration,
    ) -> Result<Option<Vec<u8>>, ManagementError> {
        Ok(None)
    }
    fn terminal_size(&mut self) -> Option<(u16, u16)> {
        None
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "request", rename_all = "snake_case")]
pub enum OperationInput {
    Answer { id: String, value: String },
    Terminal { bytes: Vec<u8> },
    Resize { columns: u16, rows: u16 },
    Cancel,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperationRecord {
    pub sequence: u64,
    pub event: OperationEvent,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HelperReady {
    pub socket: PathBuf,
    pub process_id: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "error", content = "message", rename_all = "snake_case")]
pub enum ManagementError {
    Unavailable(String),
    PermissionDenied(String),
    InvalidInput(String),
    Conflict(String),
    Cancelled,
    Failed(String),
}

impl std::fmt::Display for ManagementError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unavailable(s)
            | Self::PermissionDenied(s)
            | Self::InvalidInput(s)
            | Self::Conflict(s)
            | Self::Failed(s) => f.write_str(s),
            Self::Cancelled => f.write_str("Operation cancelled"),
        }
    }
}
impl std::error::Error for ManagementError {}

#[cfg(target_os = "linux")]
pub mod authorization;
pub mod client;
#[cfg(target_os = "linux")]
pub mod disks;
#[cfg(target_os = "linux")]
pub mod helper;
#[cfg(target_os = "linux")]
pub mod network;
#[cfg(target_os = "linux")]
pub mod packages;
pub mod problem;
#[cfg(target_os = "linux")]
pub mod processes;
#[cfg(target_os = "linux")]
pub mod services;
#[cfg(target_os = "linux")]
pub mod system_config;
#[cfg(target_os = "linux")]
pub mod users;

pub fn query(
    query: &ManagementQuery,
    cancelled: &AtomicBool,
) -> Result<ManagementSnapshot, ManagementError> {
    #[cfg(target_os = "linux")]
    return match query.kind {
        ManagementKind::Services => services::query(query, cancelled),
        ManagementKind::Processes => processes::query(query, cancelled),
        ManagementKind::Packages => packages::query(query, cancelled),
        ManagementKind::Network => network::query(query, cancelled),
        ManagementKind::Disks => disks::query(query, cancelled),
        ManagementKind::Users => users::query(query, cancelled),
        ManagementKind::SystemConfig => system_config::query(query, cancelled),
    };
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (query, cancelled);
        Err(ManagementError::Unavailable(
            "Linux system management is unavailable on this platform".into(),
        ))
    }
}

pub fn execute(
    command: &ManagementCommand,
    context: &ExecutionContext,
    interaction: &mut dyn OperationInteraction,
    cancelled: &AtomicBool,
) -> Result<String, ManagementError> {
    #[cfg(target_os = "linux")]
    return match command.kind {
        ManagementKind::Services => services::execute(command, context, interaction, cancelled),
        ManagementKind::Processes => processes::execute(command, context, interaction, cancelled),
        ManagementKind::Packages => packages::execute(command, context, interaction, cancelled),
        ManagementKind::Network => network::execute(command, context, interaction, cancelled),
        ManagementKind::Disks => disks::execute(command, context, interaction, cancelled),
        ManagementKind::Users => users::execute(command, context, interaction, cancelled),
        ManagementKind::SystemConfig => {
            system_config::execute(command, context, interaction, cancelled)
        }
    };
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (command, context, interaction, cancelled);
        Err(ManagementError::Unavailable(
            "Linux system management is unavailable on this platform".into(),
        ))
    }
}
