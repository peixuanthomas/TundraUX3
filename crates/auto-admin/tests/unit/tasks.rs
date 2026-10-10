use super::*;
use std::path::PathBuf;
use std::sync::atomic::AtomicUsize;
use std::time::{SystemTime, UNIX_EPOCH};

pub(crate) struct TaskFixture {
    runtime: Option<watchdog::WatchdogRuntime>,
    pub(crate) group: ManagedTaskGroup,
    root: PathBuf,
}

impl TaskFixture {
    pub(crate) fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "tundra-aa-task-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let config = watchdog::WatchdogConfig::new(
            root.join("reports"),
            root.join("fallback"),
            root.join("data"),
            "auto-admin-test",
            env!("CARGO_PKG_VERSION"),
        );
        let (runtime, process) = watchdog::WatchdogRuntime::start_isolated(config).unwrap();
        let app = process
            .register_app(watchdog::AppDescriptor::new(
                watchdog::AppId::from_static("shell"),
                "Shell",
                env!("CARGO_PKG_VERSION"),
                watchdog::AppCriticality::ProcessCritical,
            ))
            .unwrap();
        Self {
            runtime: Some(runtime),
            group: app.task_group("auto-admin-test"),
            root,
        }
    }
}

impl Drop for TaskFixture {
    fn drop(&mut self) {
        self.runtime.take().unwrap().shutdown().unwrap();
        platform::cleanup_temp_path(&self.root).unwrap();
    }
}

fn job(policy: storage::AutoAdminPolicy) -> AutoAdminJob {
    let (responses, _inputs) = mpsc::channel();
    AutoAdminJob::new("Fixture operation".into(), policy, responses)
}

#[test]
fn task_exit_closes_unfinished_aa_and_preserves_verified_results_and_denial() {
    for outcome in ["unfinished", "success", "failure", "denied"] {
        let fixture = TaskFixture::new();
        let job = job(if outcome == "denied" {
            storage::AutoAdminPolicy::Deny
        } else {
            storage::AutoAdminPolicy::Automatic
        });
        let worker_job = job.clone();
        let worker = spawn_task(
            &fixture.group,
            TaskId::from_static("operation"),
            Arc::new(i18n::LanguageSnapshot::embedded(0)),
            Some(&job),
            move || match outcome {
                "success" => worker_job.finish_result(&Ok::<_, String>(()), |()| "Verified".into()),
                "failure" => worker_job.finish_result(&Err::<(), _>("Failed"), |()| unreachable!()),
                _ => {}
            },
        )
        .unwrap();
        assert_eq!(worker.join().unwrap(), Some(()));
        let display = job.0.display.lock().unwrap();
        if outcome == "denied" {
            assert_eq!(job.phase(), DENIED);
            assert_eq!(display.status, i18n::tr!("aa-blocked"));
        } else {
            assert_eq!(job.phase(), FINISHED);
            let expected = match outcome {
                "success" => "Verified".into(),
                "failure" => "Failed".into(),
                _ => platform::service::ServiceError::Unknown.to_string(),
            };
            assert_eq!(display.status, expected);
            assert_eq!(display.parser.screen().contents(), expected);
        }
    }
}

#[test]
fn task_panic_finishes_aa_without_replay_or_exposing_the_payload() {
    let fixture = TaskFixture::new();
    let job = job(storage::AutoAdminPolicy::Automatic);
    job.emit(&OperationEvent::Question {
        id: "sudo-password".into(),
        prompt: "Password:".into(),
        choices: vec![],
        secret: true,
    });
    let runs = Arc::new(AtomicUsize::new(0));
    let worker_runs = runs.clone();
    let worker = spawn_task(
        &fixture.group,
        TaskId::from_static("panic"),
        Arc::new(i18n::LanguageSnapshot::embedded(0)),
        Some(&job),
        move || {
            worker_runs.fetch_add(1, Ordering::Relaxed);
            panic!("secret fixture panic payload");
        },
    )
    .unwrap();
    assert_eq!(worker.join().unwrap(), None);
    assert_eq!(runs.load(Ordering::Relaxed), 1);
    assert_eq!(job.phase(), FINISHED);
    let display = job.0.display.lock().unwrap();
    assert!(display.question.is_none());
    assert_eq!(
        display.status,
        watchdog::WatchdogError::TaskPanicked.to_string()
    );
    assert!(
        !display
            .parser
            .screen()
            .contents()
            .contains("fixture panic payload")
    );
    drop(display);
    let deadline = Instant::now() + Duration::from_secs(2);
    let incident = loop {
        if let Some(incident) = fixture.runtime.as_ref().unwrap().try_recv_incident() {
            break incident;
        }
        assert!(
            Instant::now() < deadline,
            "watchdog must still report the panic"
        );
        std::thread::sleep(Duration::from_millis(5));
    };
    assert_eq!(incident.kind, watchdog::IncidentKind::Panic);
    assert!(!incident.recovery.is_recovered());
    assert_eq!(incident.restart_attempt, 0);
}

#[test]
fn task_start_failure_finishes_aa_without_running_the_operation() {
    let fixture = TaskFixture::new();
    fixture.group.clone().shutdown(Duration::ZERO);
    let job = job(storage::AutoAdminPolicy::Automatic);
    let (started, result) = mpsc::channel();
    let error = spawn_task(
        &fixture.group,
        TaskId::from_static("closed"),
        Arc::new(i18n::LanguageSnapshot::embedded(0)),
        Some(&job),
        move || started.send(()).unwrap(),
    )
    .err()
    .expect("the closed task group must reject the worker");
    assert!(result.try_recv().is_err());
    assert_eq!(job.phase(), FINISHED);
    let display = job.0.display.lock().unwrap();
    assert_eq!(display.status, error.to_string());
    assert_eq!(display.parser.screen().contents(), error.to_string());
}
