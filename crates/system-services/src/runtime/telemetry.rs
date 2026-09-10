//! Metadata-only observability at service result boundaries.
use runtime_log::{LogContext, LogLevel, LogPhase, RuntimeLogEvent};
use std::{cell::RefCell, collections::HashMap, error::Error};

thread_local! {
    // Runtime service work executes on its managed worker. Limit both maps so
    // arbitrary provider errors cannot grow process memory indefinitely.
    static CAUSES: RefCell<HashMap<String, RuntimeLogEvent>> = RefCell::new(HashMap::new());
    static FAILURES: RefCell<HashMap<String, u64>> = RefCell::new(HashMap::new());
}

fn event(
    module: &str,
    operation: &str,
    level: LogLevel,
    phase: LogPhase,
    message: &str,
) -> RuntimeLogEvent {
    let mut context = watchdog::AppWatchdog::current()
        .map(|app| app.log_context(operation))
        .unwrap_or_else(|| LogContext {
            app: "system-services".into(),
            operation: operation.into(),
            ..LogContext::default()
        });
    context.module = format!("ux.services.{module}");
    let mut event = RuntimeLogEvent::new(context, level, phase, message);
    event.alert_key = Some(operation.into());
    event
}
fn key(event: &RuntimeLogEvent) -> String {
    format!(
        "{}:{}:{}",
        event.context.run_id.as_deref().unwrap_or(""),
        event.context.module,
        event.context.operation
    )
}
fn emit(event: RuntimeLogEvent) {
    if let Some(app) = watchdog::AppWatchdog::current() {
        app.record_log(event);
    } else {
        runtime_log::record(event);
    }
}

/// Keep structured metadata until the caller decides retry or fallback. This
/// does not emit a second copy of an error handled by the runtime loop.
pub(super) fn capture(module: &str, operation: &str, error: &(dyn Error + 'static)) -> String {
    let mut item = event(
        module,
        operation,
        LogLevel::Warning,
        LogPhase::Failed,
        "service request failed",
    );
    watchdog::capture_error(&mut item, error);
    if let Some(platform) = error.downcast_ref::<platform::PlatformError>() {
        item.os_error_code = platform.raw_os_error().map(i64::from);
    }
    if let Some(http) = error.downcast_ref::<reqwest::Error>() {
        item.error_code = http.status().map(|code| format!("HTTP_{}", code.as_u16()));
    }
    CAUSES.with(|causes| {
        let mut causes = causes.borrow_mut();
        if causes.len() >= 64 {
            causes.clear();
        }
        causes.insert(key(&item), item);
    });
    runtime_log::sanitize_text(&error.to_string())
}

pub(super) fn failure(module: &str, operation: &str, reason: &str, degraded: bool) {
    let mut item = event(
        module,
        operation,
        if degraded {
            LogLevel::Warning
        } else {
            LogLevel::Error
        },
        if degraded {
            LogPhase::Degraded
        } else {
            LogPhase::Failed
        },
        if degraded {
            "service failed; fallback remains active; retry scheduled"
        } else {
            "service unavailable; retry scheduled"
        },
    );
    let id = key(&item);

    item.error_code = Some(format!("SERVICE_{}", operation.to_ascii_uppercase()));
    CAUSES.with(|causes| {
        if let Some(cause) = causes.borrow_mut().remove(&id) {
            item.error_chain = cause.error_chain;
            item.os_error_code = cause.os_error_code;
            item.error_code = cause.error_code.or(item.error_code.take());
        }
    });
    if item.error_chain.is_empty() {
        item.error_chain.push(runtime_log::sanitize_text(reason));
    }
    FAILURES.with(|failures| {
        let mut failures = failures.borrow_mut();
        if failures.len() >= 64 {
            failures.clear();
        }
        let attempts = failures.entry(id).or_default();
        if *attempts > 0 {
            item.phase = LogPhase::Retry;
            item.retry_count = 1;
        }
        *attempts += 1;
    });
    emit(item);
}

pub(super) fn typed_failure(
    module: &str,
    operation: &str,
    error: &(dyn Error + 'static),
    degraded: bool,
) {
    if matches!(
        error.downcast_ref::<platform::PlatformError>(),
        Some(platform::PlatformError::Unsupported { .. })
    ) {
        return;
    }
    let reason = capture(module, operation, error);
    failure(module, operation, &reason, degraded);
}

pub(super) fn recovered(module: &str, operation: &str) {
    let mut item = event(
        module,
        operation,
        LogLevel::Info,
        LogPhase::Recovered,
        "service operation recovered",
    );
    CAUSES.with(|causes| {
        causes.borrow_mut().remove(&key(&item));
    });
    let attempts = FAILURES.with(|failures| failures.borrow_mut().remove(&key(&item)));
    if let Some(attempts) = attempts {
        item.retry_count = attempts;
        emit(item);
    }
}

pub(super) fn lifecycle(phase: LogPhase) {
    emit(event(
        "runtime",
        "lifecycle",
        LogLevel::Info,
        phase,
        "system services runtime lifecycle",
    ));
}

pub(super) fn begin(module: &str, operation: &str) {
    let item = event(
        module,
        operation,
        LogLevel::Info,
        LogPhase::Started,
        "service operation started",
    );
    CAUSES.with(|causes| {
        causes.borrow_mut().remove(&key(&item));
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn service_failure_and_recovery_preserve_typed_cause_and_condense_retries() {
        let root = tempfile::tempdir().unwrap();
        let config = watchdog::WatchdogConfig::new(
            root.path().join("reports"),
            root.path().join("fallback"),
            root.path().join("data"),
            "services-test",
            "1",
        );
        let (runtime, process) = watchdog::WatchdogRuntime::start_isolated(config).unwrap();
        let app = process
            .register_app(watchdog::AppDescriptor::new(
                watchdog::AppId::from_static("system-services"),
                "Services",
                "1",
                watchdog::AppCriticality::Optional,
            ))
            .unwrap();
        app.run_boundary(
            watchdog::BoundarySpec::new("service", watchdog::BoundaryKind::Worker),
            || {
                begin("weather", "request");
                let reason = capture("weather", "request", &std::io::Error::from_raw_os_error(13));
                failure("weather", "request", &reason, true);
                for _ in 0..4 {
                    typed_failure(
                        "weather",
                        "request",
                        &std::io::Error::from_raw_os_error(13),
                        true,
                    );
                }
                recovered("weather", "request");
                recovered("weather", "request"); // Silence is not another recovery.
            },
        )
        .ok()
        .unwrap();
        runtime.shutdown().unwrap();
        let logs = runtime_log::query_logs(
            &root.path().join("runtime"),
            &runtime_log::LogQuery::default(),
        );
        assert_eq!(logs.events.len(), 3);
        let failed = logs
            .events
            .iter()
            .find(|event| event.phase == LogPhase::Degraded)
            .unwrap();
        assert_eq!(failed.context.module, "ux.services.weather");
        assert_eq!(failed.os_error_code, Some(13));
        let recovered = logs
            .events
            .iter()
            .find(|event| event.phase == LogPhase::Recovered)
            .unwrap();
        assert_eq!(recovered.repeat_count, 5);
        assert_eq!(recovered.retry_count, 4);
    }
}
