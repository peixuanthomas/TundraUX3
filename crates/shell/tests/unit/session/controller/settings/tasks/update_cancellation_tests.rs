use super::*;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

#[cfg(any(windows, target_os = "linux"))]
#[path = "../../../../../../../app/tests/unit/update_marker_failure.rs"]
mod marker_write_failure;

struct Fixture {
    runtime: ShellSettingsTaskRuntime,
    watchdog: Option<watchdog::WatchdogRuntime>,
    root: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().canonicalize().unwrap().join(format!(
            "tundra-update-cancel-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let (watchdog, process) =
            watchdog::WatchdogRuntime::start_isolated(watchdog::WatchdogConfig::new(
                root.join("reports"),
                root.join("fallback"),
                root.join("state"),
                "update-cancel-tests",
                "test",
            ))
            .unwrap();
        let app = process
            .register_app(watchdog::AppDescriptor::new(
                watchdog::AppId::from_static("update-cancel-tests"),
                "Update",
                "test",
                watchdog::AppCriticality::ProcessCritical,
            ))
            .unwrap();
        let runtime = ShellSettingsTaskRuntime::new_managed(app);
        *runtime.shared.update_control.lock().unwrap() = Some(SettingsUpdateControl {
            id: 42,
            cancel: app::update::UpdateCancellation::default(),
            staged: None,
            cancellation_error: None,
        });
        Self {
            runtime,
            watchdog: Some(watchdog),
            root,
        }
    }

    fn terminal_event(&self) -> SettingsUpdateTaskEvent {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(event) = self.runtime.drain_update_events().into_iter().next() {
                return event;
            }
            assert!(
                Instant::now() < deadline,
                "update worker did not finish cleanup"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.watchdog.take().unwrap().shutdown().unwrap();
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn staged_manifest(root: &Path) -> PathBuf {
    let install = root.join("install");
    let transaction = install.join(".tundra-update/this-task");
    std::fs::create_dir_all(transaction.join("new")).unwrap();
    #[cfg(windows)]
    let names = ["tundra-shell.exe", "tundra-cli.exe"];
    #[cfg(not(windows))]
    let names = ["tundra-shell", "tundra-cli"];
    for name in names {
        std::fs::write(install.join(name), b"installed").unwrap();
        std::fs::write(transaction.join("new").join(name), b"new").unwrap();
    }
    #[cfg(target_os = "linux")]
    std::fs::write(
        install.join(platform::installation::PORTABLE_MARKER),
        platform::installation::PORTABLE_MARKER_CONTENT,
    )
    .unwrap();
    let manifest = transaction.join("transaction.json");
    std::fs::write(&manifest, serde_json::to_vec(&serde_json::json!({
        "protocol": app::update::UPDATE_PROTOCOL_VERSION, "target_sha": "a".repeat(40), "install_dir": install,
        "transaction_dir": transaction, "state": "prepared", "cli_replaced": false, "shell_replaced": false,
    })).unwrap()).unwrap();
    manifest
}

#[test]
fn update_task_record_survives_until_worker_cleanup_has_finished() {
    let fixture = Fixture::new();
    let release = Arc::new(AtomicBool::new(false));
    let sent = Arc::new(AtomicBool::new(false));
    let events = fixture.runtime.shared.update_event_tx.clone();
    let release_worker = release.clone();
    let sent_worker = sent.clone();
    let worker = fixture
        .runtime
        .shared
        .task_group
        .as_ref()
        .unwrap()
        .spawn_thread(
            TaskSpec::one_shot(TaskId::new("cancel-cleanup-test").unwrap()),
            move || {
                events.send(SettingsUpdateTaskEvent::Cancelled).unwrap();
                sent_worker.store(true, Ordering::Release);
                while !release_worker.load(Ordering::Acquire) {
                    std::thread::sleep(Duration::from_millis(5));
                }
            },
        )
        .unwrap();
    *fixture.runtime.shared.update_worker.lock().unwrap() = Some(worker);
    while !sent.load(Ordering::Acquire) {
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(fixture.runtime.drain_update_events().is_empty());
    assert_eq!(fixture.runtime.active_update_id(), Some(42));
    assert!(fixture.runtime.update_busy());
    release.store(true, Ordering::Release);
    assert!(matches!(
        fixture.terminal_event(),
        SettingsUpdateTaskEvent::Cancelled
    ));
    assert!(!fixture.runtime.update_busy());
    assert_eq!(fixture.runtime.active_update_id(), None);
}

#[test]
fn update_cancel_wins_over_a_queued_success_and_cleans_staging() {
    let fixture = Fixture::new();
    let manifest = staged_manifest(&fixture.root);
    assert!(fixture.runtime.cancel_update().unwrap());
    fixture
        .runtime
        .shared
        .update_event_tx
        .send(SettingsUpdateTaskEvent::PrepareCompleted(Ok(
            manifest.clone()
        )))
        .unwrap();
    assert!(fixture.runtime.drain_update_events().is_empty());
    assert_eq!(fixture.runtime.active_update_id(), Some(42));
    assert!(matches!(
        fixture.terminal_event(),
        SettingsUpdateTaskEvent::Cancelled
    ));
    assert!(!manifest.parent().unwrap().exists());
    assert!(fixture.root.join("install").exists());
}

#[test]
fn update_cancelled_check_does_not_deliver_queued_success() {
    let fixture = Fixture::new();
    assert!(fixture.runtime.cancel_update().unwrap());
    fixture
        .runtime
        .shared
        .update_event_tx
        .send(SettingsUpdateTaskEvent::CheckCompleted(Ok(
            app::update::UpdateCheckResult {
                release: None,
                default_branch: "master".into(),
                head_sha: "a".repeat(40),
                relation: app::update::UpdateRelation::Identical,
                commits: Vec::new(),
            },
        )))
        .unwrap();
    assert!(matches!(
        fixture.terminal_event(),
        SettingsUpdateTaskEvent::Cancelled
    ));
    assert_eq!(fixture.runtime.active_update_id(), None);
}

#[test]
fn update_cleanup_spawn_failure_still_persists_cancellation() {
    let fixture = Fixture::new();
    let manifest = staged_manifest(&fixture.root);
    let unavailable = ShellSettingsTaskRuntime::unavailable();
    *unavailable.shared.update_control.lock().unwrap() = Some(SettingsUpdateControl {
        id: 7,
        cancel: app::update::UpdateCancellation::default(),
        staged: Some(manifest.clone()),
        cancellation_error: None,
    });
    assert!(
        unavailable
            .cancel_update()
            .unwrap_err()
            .render_current()
            .contains(&manifest.display().to_string())
    );
    let saved: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&manifest).unwrap()).unwrap();
    assert_eq!(saved["state"], "cancelled");
    assert_eq!(unavailable.active_update_id(), None);
    assert!(!unavailable.update_busy());
    app::update::discard_staged_update(&manifest).unwrap();
}

#[test]
fn update_cancel_after_preparation_never_restarts_when_auto_admin_finishes() {
    let fixture = Fixture::new();
    let manifest = staged_manifest(&fixture.root);
    let mut state = ShellSession::new_for_home_mode(
        ShellLaunchConfig::default(),
        (120, 40),
        ShellHomeMode::User,
    );
    state.settings_task_runtime = fixture.runtime.clone();
    let (responses, _inputs) = mpsc::channel();
    let job = state
        .begin_auto_admin("System operation".into(), true, responses)
        .unwrap();
    fixture
        .runtime
        .shared
        .update_event_tx
        .send(SettingsUpdateTaskEvent::PrepareCompleted(Ok(
            manifest.clone()
        )))
        .unwrap();
    state.poll_settings_background_tasks();
    assert!(!state.shutdown_requested());
    assert_eq!(state.update_apply_manifest.as_ref(), Some(&manifest));
    state.cancel_update();
    assert!(state.update_apply_manifest.is_none());
    let deadline = Instant::now() + Duration::from_secs(5);
    while state.settings_update_state.busy {
        state.poll_settings_background_tasks();
        assert!(!state.shutdown_requested());
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(
        state.settings_update_state.phase,
        Some(app::update::UpdatePhase::Cancelled)
    );
    job.finish(Ok("Done".into()));
    state.poll_settings_background_tasks();
    assert!(!state.shutdown_requested());
    assert!(state.update_apply_manifest.is_none());
    assert!(!manifest.parent().unwrap().exists());
}

#[test]
fn update_cancel_marker_failure_blocks_new_updates_without_claiming_a_live_worker() {
    let fixture = Fixture::new();
    let manifest = staged_manifest(&fixture.root);
    let valid_manifest = std::fs::read(&manifest).unwrap();
    fixture
        .runtime
        .shared
        .update_control
        .lock()
        .unwrap()
        .as_mut()
        .unwrap()
        .staged = Some(manifest.clone());
    std::fs::write(&manifest, b"invalid transaction").unwrap();
    let error = fixture.runtime.cancel_update().unwrap_err();
    assert!(
        error
            .render_current()
            .contains(&manifest.display().to_string())
    );
    assert_eq!(fixture.runtime.update_cancel_error(), Some(error));
    assert!(fixture.runtime.update_busy());
    assert!(!fixture.runtime.update_cleanup_running());
    assert_eq!(fixture.runtime.active_update_id(), Some(42));
    std::fs::write(&manifest, valid_manifest).unwrap();
    assert!(fixture.runtime.cancel_update().unwrap());
    assert!(fixture.runtime.update_cancel_error().is_none());
    assert!(matches!(
        fixture.terminal_event(),
        SettingsUpdateTaskEvent::Cancelled
    ));
    assert!(!fixture.runtime.update_busy());
    assert!(fixture.runtime.update_cancel_error().is_none());
}

#[test]
fn cancel_prepared_update_retracts_only_the_update_exit_request() {
    for user_exit in [false, true] {
        let fixture = Fixture::new();
        let manifest = staged_manifest(&fixture.root);
        let mut state = ShellSession::new_for_home_mode(
            ShellLaunchConfig::default(),
            (100, 30),
            ShellHomeMode::User,
        );
        state.settings_task_runtime = fixture.runtime.clone();
        fixture
            .runtime
            .shared
            .update_event_tx
            .send(SettingsUpdateTaskEvent::PrepareCompleted(Ok(
                manifest.clone()
            )))
            .unwrap();
        state.poll_settings_background_tasks();
        assert!(state.shutdown_requested());
        state.shutdown_requested = user_exit;
        state.cancel_update();
        assert_eq!(state.shutdown_requested(), user_exit);
        assert!(state.update_apply_manifest.is_none());
        assert!(matches!(
            fixture.terminal_event(),
            SettingsUpdateTaskEvent::Cancelled
        ));
    }
}

#[test]
fn closing_page_waits_for_queued_preparation_and_keeps_marker_failure_for_retry() {
    let fixture = Fixture::new();
    let manifest = staged_manifest(&fixture.root);
    let original = std::fs::read(&manifest).unwrap();
    std::fs::write(&manifest, b"invalid transaction").unwrap();
    let mut state = ShellSession::new_for_home_mode(
        ShellLaunchConfig::default(),
        (100, 30),
        ShellHomeMode::User,
    );
    state.enter_new_page(ShellScreen::Settings);
    let page = state.active_page_id();
    state.settings_task_runtime = fixture.runtime.clone();
    state.settings_update_request = Some(42);
    state.register_page_task(
        TaskDomain::Update,
        42,
        TaskClass::Operation,
        fixture.runtime.update_task_control(),
    );
    let events = fixture.runtime.shared.update_event_tx.clone();
    let queued_manifest = manifest.clone();
    let (release, released) = std::sync::mpsc::channel();
    let worker = fixture
        .runtime
        .shared
        .task_group
        .as_ref()
        .unwrap()
        .spawn_thread(
            TaskSpec::one_shot(TaskId::new("queued-preparation-close-test").unwrap()),
            move || {
                released.recv_timeout(Duration::from_secs(5)).unwrap();
                events
                    .send(SettingsUpdateTaskEvent::PrepareCompleted(Ok(
                        queued_manifest.clone(),
                    )))
                    .unwrap();
            },
        )
        .unwrap();
    *fixture.runtime.shared.update_worker.lock().unwrap() = Some(worker);
    release.send(()).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while fixture.runtime.update_cleanup_running() {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(5));
    }
    // Finished workers stay registered until their queued result has been handled.
    assert!(fixture.runtime.update_busy());
    assert!(!fixture.runtime.update_cleanup_running());
    state.cancel_page_tasks_and_close(page);
    assert_eq!(state.page_screen(page), Some(ShellScreen::Settings));
    state.poll_settings_background_tasks();
    state.poll_page_closes();
    assert!(fixture.runtime.update_cancel_error().is_some());
    assert!(state.settings_update_state.error.is_some());
    assert_eq!(state.page_screen(page), Some(ShellScreen::Settings));
    assert!(!state.pending_page_closes.contains(&page));
    assert_eq!(state.settings_update_request, Some(42));
    std::fs::write(&manifest, original).unwrap();
    state.cancel_page_tasks_and_close(page);
    let deadline = Instant::now() + Duration::from_secs(5);
    while state.page_screen(page).is_some() {
        state.poll_settings_background_tasks();
        state.poll_page_closes();
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(!manifest.parent().unwrap().exists());
    assert!(!state.shutdown_requested());
}

#[test]
fn update_cleanup_spawn_failure_completes_registered_task_before_page_close() {
    let fixture = Fixture::new();
    let manifest = staged_manifest(&fixture.root);
    let unavailable = ShellSettingsTaskRuntime::unavailable();
    *unavailable.shared.update_control.lock().unwrap() = Some(SettingsUpdateControl {
        id: 7,
        cancel: app::update::UpdateCancellation::default(),
        staged: Some(manifest.clone()),
        cancellation_error: None,
    });
    let mut state = ShellSession::new_for_home_mode(
        ShellLaunchConfig::default(),
        (120, 40),
        ShellHomeMode::User,
    );
    state.enter_screen(ShellScreen::Settings);
    let page = state.active_page_id();
    state.settings_task_runtime = unavailable.clone();
    state.settings_update_request = Some(7);
    state.register_page_task(
        TaskDomain::Update,
        7,
        TaskClass::Operation,
        unavailable.update_task_control(),
    );
    state.update_apply_manifest = Some(manifest.clone());
    state.update_restart_requested = true;
    state.cancel_page_tasks_and_close(page);
    assert_eq!(state.page_screen(page), None);
    assert_eq!(state.settings_update_request, None);
    assert!(!state.tasks.has_work());
    assert!(!state.update_restart_requested);
    assert!(state.update_apply_manifest.is_none());
    assert!(
        state
            .settings_update_state
            .error
            .as_ref()
            .unwrap()
            .render_current()
            .contains(&manifest.display().to_string())
    );
    let saved: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&manifest).unwrap()).unwrap();
    assert_eq!(saved["state"], "cancelled");
    app::update::discard_staged_update(&manifest).unwrap();
}

#[cfg(any(windows, target_os = "linux"))]
#[test]
fn worker_marker_write_failure_retains_registered_staging_until_cancel_retry() {
    // Exercise both cancellation and a preparation failure that also triggers
    // discard; neither may lose the Prepared journal in its worker error.
    for cancelled in [false, true] {
        let fixture = Fixture::new();
        let manifest = staged_manifest(&fixture.root);
        let mut state = ShellSession::new_for_home_mode(
            ShellLaunchConfig::default(),
            (120, 40),
            ShellHomeMode::User,
        );
        state.enter_new_page(ShellScreen::Settings);
        let page = state.active_page_id();
        state.settings_task_runtime = fixture.runtime.clone();
        state.settings_update_request = Some(42);
        state.register_page_task(
            TaskDomain::Update,
            42,
            TaskClass::Operation,
            fixture.runtime.update_task_control(),
        );
        if cancelled {
            assert!(fixture.runtime.cancel_update().unwrap());
        }
        let root = fixture.root.clone();
        let path = manifest.clone();
        let events = fixture.runtime.shared.update_event_tx.clone();
        let worker = fixture
            .runtime
            .shared
            .task_group
            .as_ref()
            .unwrap()
            .spawn_thread(
                TaskSpec::one_shot(TaskId::new("worker-marker-failure-test").unwrap()),
                move || {
                    let error = marker_write_failure::with_blocked_write(&root, &path, || {
                        app::update::discard_staged_update(&path).unwrap_err()
                    });
                    assert!(
                        error
                            .to_string()
                            .contains("could not save update transaction")
                    );
                    assert_eq!(error.retained_staging(), Some(path.as_path()));
                    // This is the same typed event used by prepare and cleanup workers.
                    events
                        .send(SettingsUpdateTaskEvent::PreparationFailed(error))
                        .unwrap();
                },
            )
            .unwrap();
        *fixture.runtime.shared.update_worker.lock().unwrap() = Some(worker);
        let deadline = Instant::now() + Duration::from_secs(5);
        while fixture.runtime.update_cancel_error().is_none() {
            state.poll_settings_background_tasks();
            assert!(!state.shutdown_requested());
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(fixture.runtime.active_update_id(), Some(42));
        assert_eq!(state.settings_update_request, Some(42));
        assert_eq!(
            state.tasks.owner(TaskDomain::Update, 42),
            Some(TaskOwner::Page(page))
        );
        assert!(fixture.runtime.update_busy());
        assert!(!fixture.runtime.update_cleanup_running());
        assert!(state.settings_update_state.error.is_some());
        assert!(state.update_apply_manifest.is_none());
        assert!(!state.update_restart_requested);
        let saved: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&manifest).unwrap()).unwrap();
        assert_eq!(saved["state"], "prepared");
        state.cancel_page_tasks_and_close(page);
        let deadline = Instant::now() + Duration::from_secs(5);
        while state.page_screen(page).is_some() {
            state.poll_settings_background_tasks();
            state.poll_page_closes();
            assert!(!state.shutdown_requested());
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(state.settings_update_request, None);
        assert!(!state.tasks.has_work());
        assert!(!manifest.parent().unwrap().exists());
        assert!(fixture.runtime.update_cancel_error().is_none());
    }
}
