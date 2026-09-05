#![deny(unsafe_code)]

mod config;
#[allow(unsafe_code)]
mod durable;
mod error;
mod journal;
mod model;
mod report;
mod report_catalog;
mod runtime;
mod sanitize;
mod task;
mod writer;

pub use config::{RetentionPolicy, WatchdogConfig};
pub use error::WatchdogError;
pub use journal::OperationGuard;
pub use model::*;
pub use report_catalog::{IncidentReportCatalog, IncidentReportSummary};
pub use runtime::{AppWatchdog, CaughtPanic, EmergencyCleanup, ProcessWatchdog, WatchdogRuntime};
pub use task::{ManagedTaskGroup, ManagedThreadHandle};

#[cfg(feature = "tokio")]
pub use task::{ManagedLocalTaskHandle, ManagedTaskHandle};

#[cfg(all(not(test), panic = "abort"))]
compile_error!("watchdog recovery requires panic=\"unwind\"");

#[cfg(test)]
mod tests;
