//! Authorized runtime-log queries shared by CLI and TUI.
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LogAccess {
    User(String),
    Admin,
    /// Standalone CLI: current OS identity and filesystem permissions.
    OsUser,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LogsSnapshot {
    pub result: runtime_log::LogQueryResult,
    pub files: Vec<crate::diagnostics::DiagnosticLogFile>,
    pub incidents: Vec<watchdog::IncidentReportSummary>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LogDocumentSelection {
    Events,
    File(PathBuf),
    Incident(String),
}
