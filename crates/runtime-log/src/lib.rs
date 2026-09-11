//! Structured, bounded runtime logging shared by the shell, watchdog and CLI.
mod model;
mod privacy;
mod reader;
mod writer;
pub use model::*;
pub use privacy::{sanitize_event, sanitize_text};
pub use reader::{export_logs, query_logs, query_logs_cancellable};
pub use writer::{
    RuntimeLogConfig, RuntimeLogHandle, RuntimeLogRuntime, global, install_global,
    is_writer_thread, record, reserve_storage_capacity, storage_limit,
};

use chrono::Utc;
use std::sync::atomic::{AtomicU64, Ordering};
static NEXT_ID: AtomicU64 = AtomicU64::new(1);
pub(crate) fn unique_id() -> String {
    format!(
        "{}-{}-{}",
        std::process::id(),
        Utc::now().timestamp_nanos_opt().unwrap_or_default(),
        NEXT_ID.fetch_add(1, Ordering::Relaxed)
    )
}
impl RuntimeLogEvent {
    /// Construct a metadata-only event. Callers must never pass file bodies,
    /// clipboard contents, command input, or authentication material as metadata.
    pub fn new(
        context: LogContext,
        level: LogLevel,
        phase: LogPhase,
        message: impl Into<String>,
    ) -> Self {
        let mut event = Self {
            schema_version: 1,
            event_id: unique_id(),
            timestamp: Utc::now(),
            process_id: std::process::id(),
            source: LogSource::Ux,
            level,
            phase,
            context,
            message: message.into(),
            message_id: None,
            message_args: Default::default(),
            error_code: None,
            os_error_code: None,
            error_chain: Vec::new(),
            source_path: None,
            target_path: None,
            incident_id: None,
            alert_key: None,
            repeat_count: 1,
            retry_count: u64::from(phase == LogPhase::Retry),
            first_seen: None,
            last_seen: None,
            native_priority: None,
            native_source: None,
            timestamp_note: None,
        };
        sanitize_event(&mut event);
        event
    }
}
