use runtime_log::{LogQuery, LogSource, LogSourceState};
use std::sync::atomic::AtomicBool;

#[test]
fn cancelled_query_never_starts_a_command() {
    let result = platform::query_linux_logs(
        &LogQuery {
            source: LogSource::Linux,
            ..Default::default()
        },
        &AtomicBool::new(true),
    );
    assert_eq!(result.state, LogSourceState::Cancelled);
    assert!(result.events.is_empty());
}

#[cfg(not(target_os = "linux"))]
#[test]
fn non_linux_query_is_explicitly_unsupported() {
    let result = platform::query_linux_logs(
        &LogQuery {
            source: LogSource::Linux,
            ..Default::default()
        },
        &AtomicBool::new(false),
    );
    assert_eq!(result.state, LogSourceState::Unsupported);
    assert!(result.events.is_empty());
    assert!(!result.notices.is_empty());
}

#[cfg(target_os = "linux")]
#[test]
fn native_query_is_bounded_and_does_not_fabricate_ux_context() {
    let started = std::time::Instant::now();
    let result = platform::query_linux_logs(
        &LogQuery {
            source: LogSource::Linux,
            limit: 5,
            ..Default::default()
        },
        &AtomicBool::new(false),
    );
    assert!(started.elapsed() < std::time::Duration::from_secs(7));
    assert_ne!(result.state, LogSourceState::Unsupported);
    assert!(result.events.len() <= 5);
    for event in result.events {
        assert_eq!(event.source, LogSource::Linux);
        assert!(event.context.run_id.is_none());
        assert!(event.context.task_id.is_none());
    }
}
