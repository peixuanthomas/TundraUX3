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
