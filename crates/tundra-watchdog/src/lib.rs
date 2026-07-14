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
compile_error!("tundra-watchdog recovery requires panic=\"unwind\"");

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::fs;
    use std::process::Command;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
    use std::thread;
    use std::time::Duration;

    static NEXT_TEST_DIRECTORY: AtomicU64 = AtomicU64::new(1);

    fn test_runtime(label: &str) -> (WatchdogRuntime, ProcessWatchdog, std::path::PathBuf) {
        let root = std::env::temp_dir().join(format!(
            "tundra-watchdog-test-{label}-{}-{}",
            std::process::id(),
            NEXT_TEST_DIRECTORY.fetch_add(1, Ordering::Relaxed)
        ));
        let mut config = WatchdogConfig::new(
            root.join("reports"),
            root.join("fallback"),
            root.join("data"),
            "watchdog-test",
            env!("CARGO_PKG_VERSION"),
        );
        config.heartbeat_flush_interval = Duration::ZERO;
        let (runtime, process) = WatchdogRuntime::start_isolated(config).unwrap();
        (runtime, process, root)
    }

    fn test_app(process: &ProcessWatchdog) -> AppWatchdog {
        process
            .register_app(AppDescriptor::new(
                AppId::new("test.app").unwrap(),
                "Test App",
                "1.0.0",
                AppCriticality::Optional,
            ))
            .unwrap()
    }

    fn receive_incident(runtime: &WatchdogRuntime) -> IncidentReceipt {
        for _ in 0..200 {
            if let Some(receipt) = runtime.try_recv_incident() {
                return receipt;
            }
            thread::sleep(Duration::from_millis(5));
        }
        panic!("watchdog did not emit an incident in time")
    }

    fn cleanup(runtime: WatchdogRuntime, root: &std::path::Path) {
        runtime.shutdown().unwrap();
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn stale_run_marker_becomes_an_unclean_exit_incident() {
        let (runtime, process, root) = test_runtime("stale-run");
        let marker_directory = root.join("data").join("watchdog").join("runs");
        fs::create_dir_all(&marker_directory).unwrap();
        let marker = marker_directory.join("previous.active.json");
        fs::write(
            &marker,
            serde_json::to_vec_pretty(&json!({
                "schema_version": 1,
                "run_id": "previous-run",
                "process_name": "watchdog-test",
                "process_version": "1.0.0",
                "process_id": 424242,
                "started_at_utc": "2026-07-12T00:00:00Z",
                "last_heartbeat_utc": "2026-07-12T00:00:05Z",
                "snapshot": {
                    "screen": "Explorer",
                    "last_command": "Copy",
                    "terminal_size": [120, 40],
                    "active_operation": "copy-1"
                }
            }))
            .unwrap(),
        )
        .unwrap();

        assert_eq!(process.report_stale_runs(|_| false).unwrap(), 1);
        let receipt = receive_incident(&runtime);
        assert!(receipt.summary.contains("previous run previous-run"));
        assert!(matches!(receipt.recovery, RecoveryOutcome::Unrecoverable(_)));
        assert!(!marker.exists());

        cleanup(runtime, &root);
    }

    #[test]
    fn restart_requires_replay_safe_task() {
        let spec = TaskSpec {
            id: TaskId::from_static("unsafe-restart"),
            kind: TaskKind::LongRunning,
            panic_action: PanicAction::RestartTask,
            replay_safety: ReplaySafety::Never,
            restart_policy: RestartPolicy::limited(
                1,
                Duration::from_secs(60),
                vec![Duration::ZERO],
            ),
        };
        assert!(spec.validate().is_err());
    }

    #[test]
    fn identifiers_reject_paths_and_empty_values() {
        assert!(AppId::new("").is_err());
        assert!(AppId::new("../shell").is_err());
        assert_eq!(AppId::new("shell.weathr").unwrap().as_str(), "shell.weathr");
    }

    #[test]
    #[should_panic(expected = "invalid static watchdog identifier")]
    fn static_identifiers_cannot_bypass_validation() {
        let _ = AppId::from_static("../shell");
    }

    #[test]
    fn panic_boundary_persists_a_final_report() {
        let (runtime, process, root) = test_runtime("boundary-report");
        let app = test_app(&process).child_component(ComponentId::new("render").unwrap());
        app.breadcrumb(Breadcrumb::new("input", "safe test breadcrumb"));

        let caught = match app.run_boundary(
            BoundarySpec::new("render.frame", BoundaryKind::UiSession),
            || panic!("render exploded"),
        ) {
            Ok(()) => panic!("panic boundary unexpectedly returned successfully"),
            Err(caught) => caught,
        };
        let receipt = caught
            .finalize(RecoveryOutcome::Recovered("session rebuilt".to_string()))
            .unwrap();
        assert_eq!(receipt.summary, "render exploded");
        assert!(receipt.recovery.is_recovered());
        assert_eq!(receipt.component.as_deref(), Some("test.app/render"));

        let json_path = receipt.json_report_path.as_ref().unwrap();
        let report: serde_json::Value =
            serde_json::from_slice(&fs::read(json_path).unwrap()).unwrap();
        assert_eq!(report["panic"]["payload"], "render exploded");
        assert_eq!(report["app"]["id"], "test.app");
        assert_eq!(report["component"], "test.app/render");
        assert_eq!(report["breadcrumbs"][0]["message"], "safe test breadcrumb");
        assert!(report["panic"]["backtrace"].as_str().is_some());

        let routed = receive_incident(&runtime);
        assert_eq!(routed.incident_id, receipt.incident_id);
        cleanup(runtime, &root);
    }

    #[test]
    fn dropping_caught_panic_still_emits_a_report() {
        let (runtime, process, root) = test_runtime("dropped-panic");
        let app = test_app(&process);
        let caught = app
            .run_boundary(
                BoundarySpec::new("worker.dropped", BoundaryKind::Worker),
                || panic!("forgotten panic"),
            )
            .expect_err("the boundary should catch the panic");
        drop(caught);

        let receipt = receive_incident(&runtime);
        assert_eq!(receipt.summary, "forgotten panic");
        assert!(matches!(
            receipt.recovery,
            RecoveryOutcome::Unrecoverable(_)
        ));
        cleanup(runtime, &root);
    }

    #[test]
    fn zero_breadcrumb_capacity_drops_breadcrumbs_without_looping() {
        let root = std::env::temp_dir().join(format!(
            "tundra-watchdog-test-zero-breadcrumb-{}-{}",
            std::process::id(),
            NEXT_TEST_DIRECTORY.fetch_add(1, Ordering::Relaxed)
        ));
        let mut config = WatchdogConfig::new(
            root.join("reports"),
            root.join("fallback"),
            root.join("data"),
            "watchdog-test",
            "1",
        );
        config.breadcrumb_capacity = 0;
        let (runtime, process) = WatchdogRuntime::start_isolated(config).unwrap();
        test_app(&process).breadcrumb(Breadcrumb::new("ignored", "ignored"));
        cleanup(runtime, &root);
    }

    #[test]
    fn idempotent_thread_restarts_within_its_limit() {
        let (runtime, process, root) = test_runtime("thread-restart");
        let app = test_app(&process);
        let attempts = Arc::new(AtomicUsize::new(0));
        let worker_attempts = attempts.clone();
        let spec = TaskSpec::idempotent_service(
            TaskId::new("refresh").unwrap(),
            RestartPolicy::limited(1, Duration::from_secs(60), vec![Duration::ZERO]),
        );
        let handle = app
            .task_group("services")
            .spawn_thread(spec, move || {
                if worker_attempts.fetch_add(1, Ordering::SeqCst) == 0 {
                    panic!("first attempt failed");
                }
                42_u8
            })
            .unwrap();
        assert_eq!(handle.join().unwrap(), Some(42));
        assert_eq!(attempts.load(Ordering::SeqCst), 2);
        assert!(receive_incident(&runtime).recovery.is_recovered());
        cleanup(runtime, &root);
    }

    #[test]
    fn checkpointed_restart_requires_a_registered_handler() {
        let (runtime, process, root) = test_runtime("checkpoint-policy");
        let app = test_app(&process);
        let spec = TaskSpec {
            id: TaskId::new("filesystem").unwrap(),
            kind: TaskKind::LongRunning,
            panic_action: PanicAction::RestartTask,
            replay_safety: ReplaySafety::Checkpointed(OperationKind::new("filesystem.v1").unwrap()),
            restart_policy: RestartPolicy::limited(
                1,
                Duration::from_secs(60),
                vec![Duration::ZERO],
            ),
        };
        let result = app.task_group("workers").spawn_thread(spec, || ());
        assert!(matches!(result, Err(WatchdogError::InvalidTaskPolicy(_))));
        cleanup(runtime, &root);
    }

    struct CountingRecovery(Arc<AtomicUsize>);

    impl RecoveryHandler for CountingRecovery {
        fn version(&self) -> &str {
            "1"
        }

        fn recover(&self, _record: &OperationRecord) -> RecoveryOutcome {
            self.0.fetch_add(1, Ordering::SeqCst);
            RecoveryOutcome::Recovered("journal reconciled".to_string())
        }
    }

    #[test]
    fn operation_journal_rejects_traversal_and_recovers_pending_work() {
        let (runtime, process, root) = test_runtime("journal");
        let app = test_app(&process);
        let kind = OperationKind::new("filesystem.v1").unwrap();

        let mut invalid = OperationDescriptor::new(kind.clone(), "invalid", json!({}));
        invalid.id = "../escape".to_string();
        assert!(matches!(
            app.begin_operation(invalid),
            Err(WatchdogError::InvalidIdentifier(_))
        ));

        let guard = app
            .begin_operation(OperationDescriptor::new(
                kind.clone(),
                "copy a file",
                json!({ "source": "safe" }),
            ))
            .unwrap();
        let journal_directory = root
            .join("data")
            .join("watchdog")
            .join("operations")
            .join("test.app");
        assert!(
            journal_directory
                .join(format!("{}.json", guard.operation_id()))
                .is_file()
        );
        let recovered = Arc::new(AtomicUsize::new(0));
        app.register_recovery_handler(kind, Arc::new(CountingRecovery(recovered.clone())))
            .unwrap();
        assert_eq!(recovered.load(Ordering::SeqCst), 0);
        drop(guard);
        assert!(app
            .reconcile_checkpointed(&OperationKind::new("filesystem.v1").unwrap())
            .is_recovered());
        assert_eq!(recovered.load(Ordering::SeqCst), 1);
        assert_eq!(
            fs::read_dir(&journal_directory)
                .unwrap()
                .filter_map(Result::ok)
                .count(),
            0
        );
        cleanup(runtime, &root);
    }

    #[test]
    fn explicit_operation_id_never_overwrites_a_pending_journal() {
        let (runtime, process, root) = test_runtime("operation-create-new");
        let app = test_app(&process);
        let kind = OperationKind::new("filesystem.v1").unwrap();
        let mut first = OperationDescriptor::new(kind.clone(), "first", json!({ "source": "a" }));
        first.id = "fixed-operation".to_string();
        let guard = app.begin_operation(first).unwrap();

        let mut duplicate = OperationDescriptor::new(kind, "second", json!({ "source": "b" }));
        duplicate.id = "fixed-operation".to_string();
        assert!(matches!(
            app.begin_operation(duplicate),
            Err(WatchdogError::OperationAlreadyExists(id)) if id == "fixed-operation"
        ));
        drop(guard);
        cleanup(runtime, &root);
    }

    #[test]
    fn current_uses_the_explicit_runtime_context_without_a_global_install() {
        let (runtime, process, root) = test_runtime("explicit-current");
        let app = test_app(&process);
        let expected_id = app.descriptor().id.clone();
        let result = app.run_boundary(
            BoundarySpec::new("explicit.current", BoundaryKind::Worker),
            || {
                let current = AppWatchdog::current().expect("explicit managed context");
                assert_eq!(current.descriptor().id, expected_id);
            },
        );
        assert!(result.is_ok());
        cleanup(runtime, &root);
    }

    #[test]
    fn reported_errors_have_a_terminal_recovery_state() {
        let (runtime, process, root) = test_runtime("reported-error-state");
        let app = test_app(&process);
        app.report_error(
            ErrorContext::new("cache.save", IncidentSeverity::Error),
            &std::io::Error::other("cache write failed"),
        );
        let receipt = receive_incident(&runtime);
        assert!(matches!(receipt.recovery, RecoveryOutcome::Unrecoverable(_)));
        cleanup(runtime, &root);
    }

    #[cfg(feature = "tokio")]
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn async_task_restarts_and_keeps_its_context_across_await() {
        let (runtime, process, root) = test_runtime("async-restart");
        let app = test_app(&process).child_component(ComponentId::new("weather").unwrap());
        let attempts = Arc::new(AtomicUsize::new(0));
        let factory_attempts = attempts.clone();
        let spec = TaskSpec::idempotent_service(
            TaskId::new("refresh").unwrap(),
            RestartPolicy::limited(1, Duration::from_secs(60), vec![Duration::ZERO]),
        );
        let handle = app
            .task_group("network")
            .spawn_async(spec, move || {
                let attempt = factory_attempts.fetch_add(1, Ordering::SeqCst);
                async move {
                    tokio::task::yield_now().await;
                    if attempt == 0 {
                        panic!("weather refresh failed");
                    }
                    7_u8
                }
            })
            .unwrap();
        assert_eq!(handle.join().await.unwrap(), Some(7));
        assert_eq!(attempts.load(Ordering::SeqCst), 2);
        let receipt = receive_incident(&runtime);
        assert_eq!(receipt.component.as_deref(), Some("test.app/weather"));
        assert_eq!(
            receipt.task_id.as_ref().map(TaskId::as_str),
            Some("refresh")
        );
        assert!(receipt.recovery.is_recovered());
        cleanup(runtime, &root);
    }

    #[cfg(feature = "tokio")]
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn cancelled_async_task_is_not_reported_as_a_panic() {
        let (runtime, process, root) = test_runtime("async-cancel");
        let app = test_app(&process);
        let handle = app
            .task_group("cancel")
            .spawn_async(
                TaskSpec::one_shot(TaskId::new("pending").unwrap()),
                || async { std::future::pending::<u8>().await },
            )
            .unwrap();
        handle.cancel();
        assert_eq!(handle.join().await.unwrap(), None);
        tokio::time::sleep(Duration::from_millis(20)).await;
        assert!(runtime.try_recv_incident().is_none());
        cleanup(runtime, &root);
    }

    #[cfg(feature = "tokio")]
    #[tokio::test(flavor = "current_thread")]
    async fn async_group_shutdown_closes_and_reaps_pending_tasks_without_blocking() {
        let (runtime, process, root) = test_runtime("async-group-shutdown");
        let app = test_app(&process);
        let group = app.task_group("session");
        let handle = group
            .spawn_async(
                TaskSpec::one_shot(TaskId::new("pending").unwrap()),
                || async { std::future::pending::<u8>().await },
            )
            .unwrap();
        let closed = group.clone();
        let shutdown = group.shutdown_async(Duration::from_secs(1)).await;
        assert_eq!(shutdown.still_running, 0);
        assert_eq!(handle.join().await.unwrap(), None);
        assert!(closed
            .spawn_async(
                TaskSpec::one_shot(TaskId::new("after-close").unwrap()),
                || async { 1_u8 },
            )
            .is_err());
        cleanup(runtime, &root);
    }

    #[test]
    fn global_hook_is_installed_once_in_an_isolated_process() {
        let output = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "tests::global_install_child", "--nocapture"])
            .env("TUNDRA_WATCHDOG_GLOBAL_CHILD", "1")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "child failed:\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[test]
    fn global_install_child() {
        if std::env::var_os("TUNDRA_WATCHDOG_GLOBAL_CHILD").is_none() {
            return;
        }
        let (runtime, process, root) = test_runtime("global-child-primary");
        let installed = process.install_global().unwrap();
        let (second_runtime, second_process, second_root) = test_runtime("global-child-secondary");
        assert!(matches!(
            second_process.install_global(),
            Err(WatchdogError::AlreadyInstalled)
        ));

        let cleanup_calls = Arc::new(AtomicUsize::new(0));
        let cleanup_counter = cleanup_calls.clone();
        installed
            .register_emergency_cleanup(Arc::new(move || {
                cleanup_counter.fetch_add(1, Ordering::SeqCst);
            }))
            .unwrap();
        let app = test_app(&installed);
        let caught = app
            .run_boundary(
                BoundarySpec::new("global.caught", BoundaryKind::Worker).terminal_owner(),
                || panic!("hook-captured panic"),
            )
            .expect_err("the panic should be caught");
        let receipt = caught
            .finalize(RecoveryOutcome::Recovered("continued".to_string()))
            .unwrap();
        let report: serde_json::Value =
            serde_json::from_slice(&fs::read(receipt.json_report_path.as_ref().unwrap()).unwrap())
                .unwrap();
        assert!(report["panic"]["source_file"].as_str().is_some());
        assert_eq!(cleanup_calls.load(Ordering::SeqCst), 1);

        second_runtime.shutdown().unwrap();
        runtime.shutdown().unwrap();
        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_dir_all(second_root);
    }
}
