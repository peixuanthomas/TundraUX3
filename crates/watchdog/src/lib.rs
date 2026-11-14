#![deny(unsafe_code)]

mod config;
#[allow(unsafe_code)]
mod durable;
mod error;
mod journal;
mod log_error;
mod model;
mod report;
mod report_catalog;
mod resource;
mod runtime;
mod sanitize;
mod supervisor;
mod task;
mod writer;

pub use config::{RetentionPolicy, WatchdogConfig};
pub use error::WatchdogError;
pub use journal::OperationGuard;
pub use log_error::capture_error;
pub use model::*;
pub use report_catalog::{IncidentReportCatalog, IncidentReportSummary};
pub use resource::ManagedResource;
pub use runtime::{AppWatchdog, CaughtPanic, EmergencyCleanup, ProcessWatchdog, WatchdogRuntime};
#[cfg(target_os = "macos")]
pub use supervisor::OwnedProcessSession;
#[cfg(target_os = "linux")]
pub use supervisor::register_delegated_session;
#[cfg(windows)]
pub use supervisor::register_process_job;
pub use supervisor::{
    ProcessResourceGuard, SupervisionEvent, Supervisor, WORKER_PROGRESS_INTERVAL,
    process_session_registration_enabled, set_process_resource_registrar,
};
#[cfg(unix)]
pub use supervisor::{
    register_process_group, register_process_session, spawn_registered_process_group,
};
pub use task::{ManagedTaskGroup, ManagedThreadHandle, ThreadCancellation};

#[cfg(feature = "tokio")]
pub use task::{ManagedLocalTaskHandle, ManagedTaskHandle};

#[cfg(all(not(test), panic = "abort"))]
compile_error!("watchdog recovery requires panic=\"unwind\"");

#[cfg(test)]
#[path = "../tests/unit/root.rs"]
mod tests;
