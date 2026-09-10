//! Bounded, on-demand Linux journal and kernel log access.
use runtime_log::{LogQuery, LogQueryResult, LogSourceState};
use std::sync::atomic::AtomicBool;

pub fn query_linux_logs(_query: &LogQuery, _cancelled: &AtomicBool) -> LogQueryResult {
    LogQueryResult {
        state: LogSourceState::Unsupported,
        notices: vec!["Linux logs are unavailable on this platform".into()],
        ..Default::default()
    }
}
