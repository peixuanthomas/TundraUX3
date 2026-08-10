use crate::{
    AppDescriptor, BoundaryKind, Breadcrumb, IncidentKind, IncidentReceipt, IncidentSeverity,
    OperationKind, PanicAction, RecoveryOutcome, ReplaySafety, RestartPolicy, RuntimeSnapshot,
    TaskId, TaskKind,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub(crate) const REPORT_SCHEMA_VERSION: u32 = 1;

#[derive(Clone)]
pub(crate) struct ExecutionContext {
    pub process: Option<crate::ProcessWatchdog>,
    pub incident_id: String,
    pub app: Option<AppDescriptor>,
    pub component: Option<String>,
    pub task_id: Option<TaskId>,
    pub task_group: Option<String>,
    pub boundary: String,
    pub boundary_kind: BoundaryKind,
    pub owns_terminal: bool,
    pub task_kind: Option<TaskKind>,
    pub replay_safety: Option<ReplaySafety>,
    pub operation_kind: Option<OperationKind>,
    pub operation_id: Option<String>,
    pub recovery_handler_version: Option<String>,
    pub panic_action: Option<PanicAction>,
    pub restart_policy: Option<RestartPolicy>,
    pub restart_attempt: usize,
    pub expects_finalize: bool,
}

impl ExecutionContext {
    pub(crate) fn process(incident_id: String, process: crate::ProcessWatchdog) -> Self {
        Self {
            process: Some(process),
            incident_id,
            app: None,
            component: None,
            task_id: None,
            task_group: None,
            boundary: "process.unhandled".to_string(),
            boundary_kind: BoundaryKind::Process,
            owns_terminal: true,
            task_kind: None,
            replay_safety: None,
            operation_kind: None,
            operation_id: None,
            recovery_handler_version: None,
            panic_action: None,
            restart_policy: None,
            restart_attempt: 0,
            expects_finalize: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct PanicDetails {
    pub payload: String,
    pub source_file: Option<String>,
    pub source_line: Option<u32>,
    pub source_column: Option<u32>,
    pub backtrace: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ErrorDetails {
    pub message: String,
    pub source_chain: Vec<String>,
    pub backtrace: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct IncidentRecord {
    pub schema_version: u32,
    pub incident_id: String,
    pub report_stem: String,
    pub kind: IncidentKind,
    pub severity: IncidentSeverity,
    pub occurred_at: DateTime<Utc>,
    pub process_name: String,
    pub process_version: String,
    pub process_id: u32,
    pub run_id: String,
    pub app: Option<AppDescriptor>,
    pub component: Option<String>,
    pub task_id: Option<TaskId>,
    pub task_group: Option<String>,
    pub boundary: String,
    pub boundary_kind: BoundaryKind,
    pub task_kind: Option<TaskKind>,
    pub replay_safety: Option<ReplaySafety>,
    pub operation_kind: Option<OperationKind>,
    pub operation_id: Option<String>,
    pub recovery_handler_version: Option<String>,
    pub panic_action: Option<PanicAction>,
    pub restart_policy: Option<RestartPolicy>,
    pub restart_attempt: usize,
    pub thread_name: Option<String>,
    pub thread_id: String,
    pub panic: Option<PanicDetails>,
    pub error: Option<ErrorDetails>,
    pub runtime: RuntimeSnapshot,
    pub breadcrumbs: Vec<Breadcrumb>,
    pub recovery: RecoveryOutcome,
    pub secondary_errors: Vec<String>,
}

impl IncidentRecord {
    pub(crate) fn summary(&self) -> String {
        if let Some(panic) = &self.panic {
            return panic.payload.clone();
        }
        if let Some(error) = &self.error {
            return error.message.clone();
        }
        "the previous process ended without a clean watchdog shutdown".to_string()
    }

    pub(crate) fn receipt(
        &self,
        json_report_path: Option<PathBuf>,
        text_report_path: Option<PathBuf>,
    ) -> IncidentReceipt {
        IncidentReceipt {
            incident_id: self.incident_id.clone(),
            kind: self.kind,
            severity: self.severity,
            app_id: self.app.as_ref().map(|app| app.id.clone()),
            component: self.component.clone(),
            task_id: self.task_id.clone(),
            task_group: self.task_group.clone(),
            boundary: self.boundary.clone(),
            panic_action: self.panic_action,
            operation_kind: self.operation_kind.clone(),
            operation_id: self.operation_id.clone(),
            recovery_handler_version: self.recovery_handler_version.clone(),
            restart_attempt: self.restart_attempt,
            summary: self.summary(),
            recovery: self.recovery.clone(),
            json_report_path,
            text_report_path,
        }
    }
}
