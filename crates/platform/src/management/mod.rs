//! Typed requests for the Linux management applications.
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
mod configuration;
pub use configuration::{ConfigCandidate, ConfigCheck, ConfigDocument};

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
    Backups,
    Firewall,
    Ssh,
    ScheduledTasks,
    FilePermissions,
    SystemSettings,
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
            Self::Backups => "backups",
            Self::Firewall => "firewall",
            Self::Ssh => "ssh",
            Self::ScheduledTasks => "scheduled-tasks",
            Self::FilePermissions => "file-permissions",
            Self::SystemSettings => "system-settings",
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

    pub fn inspection_values(&self) -> BTreeMap<String, String> {
        let mut values = BTreeMap::from([
            ("scope".into(), self.scope.clone()),
            ("filter".into(), self.filter.clone()),
        ]);
        if let Some(target) = &self.target {
            values.insert("query_target".into(), target.clone());
        }
        values.extend(
            self.options
                .iter()
                .map(|(key, value)| (format!("__query_option:{key}"), value.clone())),
        );
        values
    }

    pub fn from_inspection(command: &ManagementCommand) -> Self {
        Self {
            kind: command.kind,
            scope: command.values.get("scope").cloned().unwrap_or_default(),
            filter: command.values.get("filter").cloned().unwrap_or_default(),
            target: command.values.get("query_target").cloned(),
            options: command
                .values
                .iter()
                .filter_map(|(key, value)| {
                    key.strip_prefix("__query_option:")
                        .map(|key| (key.into(), value.clone()))
                })
                .collect(),
        }
    }
}

/// Field/action IDs stay stable; labels may use the caller's language.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FieldControl {
    #[default]
    Text,
    Checkbox,
    Permissions,
    SearchableChoice,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManagementField {
    pub id: String,
    pub label: String,
    pub value: String,
    pub secret: bool,
    pub required: bool,
    #[serde(default)]
    pub choices: Vec<String>,
    #[serde(default)]
    pub control: FieldControl,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManagementAction {
    pub id: String,
    pub label: String,
    pub fields: Vec<ManagementField>,
    pub confirm: bool,
    pub privileged: bool,
    #[serde(default)]
    pub requires_approval: bool,
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
    #[serde(default)]
    pub details: Vec<(String, String)>,
    pub backend: String,
    #[serde(default)]
    pub details_complete: bool,
    #[serde(default)]
    pub refresh_action: Option<ManagementAction>,
    #[serde(default)]
    pub needs_authorization: bool,
}

impl ManagementSnapshot {
    pub fn with_authorized_refresh(mut self, command: &ManagementCommand) -> Self {
        self.refresh_action = Some(ManagementAction {
            id: command.action.clone(),
            values: command.values.clone(),
            privileged: true,
            ..Default::default()
        });
        self
    }
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
    #[serde(default)]
    pub caller_pid: u32,
    #[serde(default)]
    pub caller_start_ticks: u64,
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
    Transaction {
        kind: ManagementKind,
        id: String,
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
    /// A graceful stop, or a force-stop after the caller's explicit warning.
    fn stop_requested(&mut self) -> Option<bool> {
        None
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "request", rename_all = "snake_case")]
pub enum OperationInput {
    Answer {
        id: String,
        value: String,
    },
    Terminal {
        bytes: Vec<u8>,
    },
    Resize {
        columns: u16,
        rows: u16,
    },
    Cancel,
    /// Explicit user request, unlike closing/disconnecting from an operation.
    Terminate,
    /// Sent only after a termination timeout and an explicit damage warning.
    Kill,
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
    MissingDependency(String),
    UnsupportedEnvironment(String),
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
            | Self::MissingDependency(s)
            | Self::UnsupportedEnvironment(s)
            | Self::PermissionDenied(s)
            | Self::InvalidInput(s)
            | Self::Conflict(s)
            | Self::Failed(s) => f.write_str(s),
            Self::Cancelled => f.write_str(&i18n::tr!("platform-management-cancelled")),
        }
    }
}
impl std::error::Error for ManagementError {}

#[cfg(target_os = "linux")]
pub mod authorization;
#[cfg(target_os = "linux")]
mod capability;
pub mod client;
#[cfg(target_os = "linux")]
pub mod config_backups;
#[cfg(target_os = "linux")]
pub mod disks;
#[cfg(target_os = "linux")]
pub mod file_permissions;
#[cfg(target_os = "linux")]
pub mod firewall;
#[cfg(target_os = "linux")]
pub mod helper;
#[cfg(target_os = "linux")]
mod listening_ports;
#[cfg(target_os = "linux")]
pub mod network;
#[cfg(target_os = "linux")]
pub mod packages;
#[cfg(target_os = "macos")]
#[path = "homebrew/mod.rs"]
pub mod packages;
pub mod problem;
#[cfg(target_os = "linux")]
pub mod processes;
#[cfg(target_os = "linux")]
pub mod scheduled_tasks;
#[cfg(target_os = "linux")]
pub mod services;
#[cfg(target_os = "linux")]
pub mod ssh;
#[cfg(target_os = "linux")]
pub mod system_config;
#[cfg(target_os = "linux")]
pub mod system_settings;
#[cfg(target_os = "linux")]
pub mod termination;
#[cfg(target_os = "linux")]
pub mod users;
#[cfg(any(windows, test))]
pub mod winget;

pub fn query(
    query: &ManagementQuery,
    cancelled: &AtomicBool,
) -> Result<ManagementSnapshot, ManagementError> {
    #[cfg(target_os = "macos")]
    if query.kind == ManagementKind::Packages {
        return packages::query(query, cancelled);
    }
    #[cfg(target_os = "linux")]
    return match query.kind {
        ManagementKind::Services => services::query(query, cancelled),
        ManagementKind::Processes => processes::query(query, cancelled),
        ManagementKind::Packages => packages::query(query, cancelled),
        ManagementKind::Network => network::query(query, cancelled),
        ManagementKind::Disks => disks::query(query, cancelled),
        ManagementKind::Users => users::query(query, cancelled),
        ManagementKind::SystemConfig => system_config::query(query, cancelled),
        ManagementKind::Backups => config_backups::query(query, cancelled),
        ManagementKind::Firewall => firewall::query(query, cancelled),
        ManagementKind::Ssh => ssh::query(query, cancelled),
        ManagementKind::ScheduledTasks => scheduled_tasks::query(query, cancelled),
        ManagementKind::FilePermissions => file_permissions::query(query, cancelled),
        ManagementKind::SystemSettings => system_settings::query(query, cancelled),
    };
    #[cfg(windows)]
    if query.kind == ManagementKind::Packages {
        return winget::query(query, cancelled);
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (query, cancelled);
        Err(ManagementError::Unavailable(
            i18n::tr!("platform-management-unavailable").into(),
        ))
    }
}

pub fn execute(
    command: &ManagementCommand,
    context: &ExecutionContext,
    interaction: &mut dyn OperationInteraction,
    cancelled: &AtomicBool,
) -> Result<String, ManagementError> {
    #[cfg(target_os = "macos")]
    if command.kind == ManagementKind::Packages {
        return packages::execute(command, context, interaction, cancelled);
    }
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
        ManagementKind::Backups => {
            config_backups::execute(command, context, interaction, cancelled)
        }
        ManagementKind::Firewall => firewall::execute(command, context, interaction, cancelled),
        ManagementKind::Ssh => ssh::execute(command, context, interaction, cancelled),
        ManagementKind::ScheduledTasks => {
            scheduled_tasks::execute(command, context, interaction, cancelled)
        }
        ManagementKind::FilePermissions => {
            file_permissions::execute(command, context, interaction, cancelled)
        }
        ManagementKind::SystemSettings => {
            system_settings::execute(command, context, interaction, cancelled)
        }
    };
    #[cfg(windows)]
    if command.kind == ManagementKind::Packages {
        return winget::execute(command, context, interaction, cancelled);
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (command, context, interaction, cancelled);
        Err(ManagementError::Unavailable(
            i18n::tr!("platform-management-unavailable").into(),
        ))
    }
}
