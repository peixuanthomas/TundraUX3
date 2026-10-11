use crate::session::*;

#[derive(Debug)]
pub(in crate::session) struct SettingsTimeSyncValidationEvent {
    pub(in crate::session) request_id: u64,
    pub(in crate::session) config: storage::TimeSyncConfig,
    pub(in crate::session) result: TimeSyncResult,
}

#[derive(Debug)]
pub(in crate::session) enum SettingsUpdateTaskEvent {
    Progress(app::update::UpdateProgress),
    CheckCompleted(Result<app::update::UpdateCheckResult, i18n::LocalizedText>),
    PrepareCompleted(Result<std::path::PathBuf, i18n::LocalizedText>),
    PreparationFailed(app::update::UpdateError),
    Cancelled,
}

pub(in crate::session) struct SettingsUpdateControl {
    id: u64,
    cancel: app::update::UpdateCancellation,
    staged: Option<std::path::PathBuf>,
    cancellation_error: Option<i18n::LocalizedText>,
}

pub(in crate::session) struct ShellSettingsTaskShared {
    pub(in crate::session) task_group: Option<ManagedTaskGroup>,
    pub(in crate::session) event_tx: mpsc::Sender<SettingsTimeSyncValidationEvent>,
    pub(in crate::session) event_rx: Mutex<mpsc::Receiver<SettingsTimeSyncValidationEvent>>,
    pub(in crate::session) workers: Mutex<BTreeMap<u64, ManagedThreadHandle<()>>>,
    pub(in crate::session) next_request_id: std::sync::atomic::AtomicU64,
    pub(in crate::session) system_services: Option<system_services::SystemServicesHandle>,
    pub(in crate::session) system_services_config: Mutex<system_services::SystemServicesConfig>,
    pub(in crate::session) update_event_tx: mpsc::Sender<SettingsUpdateTaskEvent>,
    pub(in crate::session) update_event_rx: Mutex<mpsc::Receiver<SettingsUpdateTaskEvent>>,
    pub(in crate::session) update_worker: Mutex<Option<ManagedThreadHandle<()>>>,
    update_control: Mutex<Option<SettingsUpdateControl>>,
    pub(in crate::session) platform: Option<std::sync::Arc<dyn Platform>>,
}

pub(in crate::session) static NEXT_SETTINGS_RUNTIME_ID: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(1);

impl Drop for ShellSettingsTaskShared {
    fn drop(&mut self) {
        if let Ok(workers) = self.workers.get_mut() {
            for worker in workers.values() {
                worker.cancel();
            }
        }
        if let Ok(worker) = self.update_worker.get_mut()
            && let Some(worker) = worker.as_ref()
        {
            worker.cancel();
        }
    }
}

#[derive(Clone)]
pub(in crate::session) struct ShellSettingsTaskRuntime {
    pub(in crate::session) shared: Arc<ShellSettingsTaskShared>,
}

impl ShellSettingsTaskRuntime {
    pub(in crate::session) fn unavailable() -> Self {
        let (event_tx, event_rx) = mpsc::channel();
        let (update_event_tx, update_event_rx) = mpsc::channel();
        Self {
            shared: Arc::new(ShellSettingsTaskShared {
                task_group: None,
                event_tx,
                event_rx: Mutex::new(event_rx),
                workers: Mutex::new(BTreeMap::new()),
                next_request_id: std::sync::atomic::AtomicU64::new(1),
                system_services: None,
                system_services_config: Mutex::new(system_services::SystemServicesConfig::default()),
                update_event_tx,
                update_event_rx: Mutex::new(update_event_rx),
                update_worker: Mutex::new(None),
                update_control: Mutex::new(None),
                platform: None,
            }),
        }
    }

    pub(in crate::session) fn new_managed(watchdog: AppWatchdog) -> Self {
        Self::new_managed_with_system_services(
            watchdog,
            None,
            system_services::SystemServicesConfig::default(),
            None,
        )
    }

    pub(in crate::session) fn new_managed_with_system_services(
        watchdog: AppWatchdog,
        system_services: Option<system_services::SystemServicesHandle>,
        system_services_config: system_services::SystemServicesConfig,
        platform: Option<std::sync::Arc<dyn Platform>>,
    ) -> Self {
        use std::sync::atomic::Ordering;

        let (event_tx, event_rx) = mpsc::channel();
        let (update_event_tx, update_event_rx) = mpsc::channel();
        let runtime_id = NEXT_SETTINGS_RUNTIME_ID
            .fetch_add(1, Ordering::Relaxed)
            .max(1);
        Self {
            shared: Arc::new(ShellSettingsTaskShared {
                task_group: Some(
                    watchdog.task_group(&format!("settings-time-sync-validation-{runtime_id}")),
                ),
                event_tx,
                event_rx: Mutex::new(event_rx),
                workers: Mutex::new(BTreeMap::new()),
                next_request_id: std::sync::atomic::AtomicU64::new(1),
                system_services,
                system_services_config: Mutex::new(system_services_config),
                update_event_tx,
                update_event_rx: Mutex::new(update_event_rx),
                update_worker: Mutex::new(None),
                update_control: Mutex::new(None),
                platform,
            }),
        }
    }

    pub(in crate::session) fn update_supported(&self) -> bool {
        self.shared
            .platform
            .as_ref()
            .is_some_and(|platform| app::update::supports_updates(platform.kind()))
    }

    pub(in crate::session) fn update_busy(&self) -> bool {
        self.shared
            .update_worker
            .lock()
            .is_ok_and(|worker| worker.is_some())
            || self.shared.update_control.lock().is_ok_and(|control| {
                control
                    .as_ref()
                    .is_some_and(|control| control.staged.is_some())
            })
    }

    /// Actual child/cleanup work only. A failed cancellation marker remains
    /// busy for new updates, but must not make session shutdown wait forever.
    pub(in crate::session) fn update_cleanup_running(&self) -> bool {
        self.shared
            .update_worker
            .lock()
            .is_ok_and(|worker| worker.as_ref().is_some_and(|worker| !worker.is_finished()))
    }

    /// An unresolved error that leaves staging unsafe to hand off or abandon.
    /// Successful retry clears it; the retained record still blocks new updates.
    pub(in crate::session) fn update_cancel_error(&self) -> Option<i18n::LocalizedText> {
        self.shared
            .update_control
            .lock()
            .ok()?
            .as_ref()?
            .cancellation_error
            .clone()
    }

    pub(in crate::session) fn submit_update_check(
        &self,
        identity: app::update::BuildIdentity,
        mode: storage::LinuxUpdateMode,
    ) -> Result<(), i18n::LocalizedText> {
        let task_group = self.shared.task_group.clone().ok_or_else(|| {
            i18n::LocalizedText::from(i18n::msg!("shell-update-worker-is-unavailable"))
        })?;
        if !self.update_supported() {
            return Err(i18n::LocalizedText::from(i18n::msg!(
                "shell-automatic-updates-are-supported-only-on-windows-and-linux"
            )));
        }
        let mut worker_slot = self.shared.update_worker.lock().map_err(|_| {
            i18n::LocalizedText::from(i18n::msg!("shell-update-task-registry-is-unavailable"))
        })?;
        if worker_slot.is_some()
            || self
                .shared
                .update_control
                .lock()
                .is_ok_and(|control| control.is_some())
        {
            return Err(i18n::LocalizedText::from(i18n::msg!(
                "shell-an-update-task-is-already-running"
            )));
        }
        let request_id = self
            .shared
            .next_request_id
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            .max(1);
        let task_id = TaskId::new(format!("check-update-{}", request_id % 64))
            .map_err(|error| format!("invalid update check task: {error}"))?;
        let events = self.shared.update_event_tx.clone();
        let cancel = app::update::UpdateCancellation::default();
        let worker_cancel = cancel.clone();
        let worker = task_group
            .spawn_thread_with_cancellation(TaskSpec::one_shot(task_id), move |watchdog_cancel| {
                let cancel = worker_cancel.with_watchdog(watchdog_cancel);
                let _ = events.send(SettingsUpdateTaskEvent::Progress(
                    app::update::UpdateProgress {
                        detail: app::update::UpdateProgressDetail::Status,
                        phase: app::update::UpdatePhase::Checking,
                        message: "Checking GitHub default branch".to_string(),
                    },
                ));
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    app::update::check_for_updates_in_mode_cancellable(&identity, mode, &cancel)
                }));
                match result {
                    Ok(result) => {
                        let event = match result {
                            Err(error)
                                if error.is_cancelled() && error.retained_staging().is_none() =>
                            {
                                SettingsUpdateTaskEvent::Cancelled
                            }
                            result => SettingsUpdateTaskEvent::CheckCompleted(
                                result.map_err(|error| error.to_string().into()),
                            ),
                        };
                        let _ = events.send(event);
                    }
                    Err(payload) => {
                        let _ = events.send(SettingsUpdateTaskEvent::CheckCompleted(Err(
                            i18n::LocalizedText::from(i18n::msg!(
                                "shell-update-check-worker-panicked"
                            )),
                        )));
                        std::panic::resume_unwind(payload);
                    }
                }
            })
            .map_err(|error| {
                i18n::LocalizedText::from(i18n::msg!(
                    "shell-could-not-start-update-check-error",
                    error = error.to_string()
                ))
            })?;
        *self
            .shared
            .update_control
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(SettingsUpdateControl {
            id: request_id,
            cancel,
            staged: None,
            cancellation_error: None,
        });
        *worker_slot = Some(worker);
        Ok(())
    }

    pub(in crate::session) fn submit_update_prepare(
        &self,
        check: app::update::UpdateCheckResult,
        install_dir: std::path::PathBuf,
    ) -> Result<(), i18n::LocalizedText> {
        let task_group = self.shared.task_group.clone().ok_or_else(|| {
            i18n::LocalizedText::from(i18n::msg!("shell-update-worker-is-unavailable"))
        })?;
        let platform = self.shared.platform.clone().ok_or_else(|| {
            i18n::LocalizedText::from(i18n::msg!("shell-update-platform-is-unavailable"))
        })?;
        if !app::update::supports_updates(platform.kind()) {
            return Err(i18n::LocalizedText::from(i18n::msg!(
                "shell-automatic-updates-are-supported-only-on-windows-and-linux"
            )));
        }
        let mut worker_slot = self.shared.update_worker.lock().map_err(|_| {
            i18n::LocalizedText::from(i18n::msg!("shell-update-task-registry-is-unavailable"))
        })?;
        if worker_slot.is_some()
            || self
                .shared
                .update_control
                .lock()
                .is_ok_and(|control| control.is_some())
        {
            return Err(i18n::LocalizedText::from(i18n::msg!(
                "shell-an-update-task-is-already-running"
            )));
        }
        let request_id = self
            .shared
            .next_request_id
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            .max(1);
        let task_id = TaskId::new(format!("prepare-update-{}", request_id % 64))
            .map_err(|error| format!("invalid update build task: {error}"))?;
        let events = self.shared.update_event_tx.clone();
        let cancel = app::update::UpdateCancellation::default();
        let worker_cancel = cancel.clone();
        let worker = task_group
            .spawn_thread_with_cancellation(TaskSpec::one_shot(task_id), move |watchdog_cancel| {
                let cancel = worker_cancel.with_watchdog(watchdog_cancel);
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    let mut report = |progress: app::update::UpdateProgress| {
                        let _ = events.send(SettingsUpdateTaskEvent::Progress(progress));
                    };
                    let prepared = app::update::prepare_update_cancellable(
                        platform.as_ref(),
                        &check,
                        &mut report,
                        &cancel,
                    )?;
                    let work_dir = prepared.work_dir.clone();
                    report(app::update::UpdateProgress {
                        detail: app::update::UpdateProgressDetail::Status,
                        phase: app::update::UpdatePhase::PreparingReplacement,
                        message: "Preparing rollback files and restart helper".to_string(),
                    });
                    let staged = app::update::stage_update_for_apply_cancellable(
                        &prepared,
                        &install_dir,
                        &cancel,
                    );
                    let cleanup = app::update::cleanup_update_work(&work_dir);
                    match staged {
                        Ok(staged) => {
                            if cleanup.is_err() || cancel.is_cancelled() {
                                let discard =
                                    app::update::discard_staged_update(&staged.manifest_path);
                                match (cleanup, discard) {
                                    (Err(error), Err(discard)) => {
                                        return Err(error.with_cleanup_failure(discard));
                                    }
                                    (Err(error), _) | (_, Err(error)) => return Err(error),
                                    _ => {}
                                }
                                return Err(app::update::UpdateError::cancelled());
                            }
                            Ok(staged.manifest_path)
                        }
                        Err(error) => Err(match cleanup {
                            Ok(()) => error,
                            Err(cleanup) => error.with_cleanup_failure(cleanup),
                        }),
                    }
                }));
                match result {
                    Ok(result) => {
                        let event = match result {
                            Err(error)
                                if error.is_cancelled() && error.retained_staging().is_none() =>
                            {
                                SettingsUpdateTaskEvent::Cancelled
                            }
                            Err(error) => SettingsUpdateTaskEvent::PreparationFailed(error),
                            Ok(path) => SettingsUpdateTaskEvent::PrepareCompleted(Ok(path)),
                        };
                        let _ = events.send(event);
                    }
                    Err(payload) => {
                        let _ = events.send(SettingsUpdateTaskEvent::PrepareCompleted(Err(
                            i18n::LocalizedText::from(i18n::msg!(
                                "shell-update-build-worker-panicked"
                            )),
                        )));
                        std::panic::resume_unwind(payload);
                    }
                }
            })
            .map_err(|error| {
                i18n::LocalizedText::from(i18n::msg!(
                    "shell-could-not-start-update-build-error",
                    error = error.to_string()
                ))
            })?;
        *self
            .shared
            .update_control
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(SettingsUpdateControl {
            id: request_id,
            cancel,
            staged: None,
            cancellation_error: None,
        });
        *worker_slot = Some(worker);
        Ok(())
    }

    pub(in crate::session) fn active_update_id(&self) -> Option<u64> {
        self.shared
            .update_control
            .lock()
            .ok()?
            .as_ref()
            .map(|control| control.id)
    }

    pub(in crate::session) fn update_cancelling(&self) -> bool {
        self.shared.update_control.lock().is_ok_and(|control| {
            control
                .as_ref()
                .is_some_and(|control| control.cancel.is_cancelled())
        })
    }

    pub(in crate::session) fn cancel_update(&self) -> Result<bool, i18n::LocalizedText> {
        let staged = {
            let mut slot =
                self.shared.update_control.lock().map_err(|_| {
                    i18n::LocalizedText::from("Update task registry is unavailable")
                })?;
            let Some(control) = slot.as_mut() else {
                return Ok(false);
            };
            control.cancel.cancel();
            control.staged.take()
        };
        if let Some(path) = staged {
            self.start_staged_cleanup(path)?;
        }
        Ok(true)
    }

    fn start_staged_cleanup(&self, path: std::path::PathBuf) -> Result<(), i18n::LocalizedText> {
        if let Err(error) = app::update::mark_staged_update_cancelled(&path) {
            let message: i18n::LocalizedText = format!(
                "Could not persist update cancellation; retained {}: {error}",
                path.display()
            )
            .into();
            if let Some(control) = self
                .shared
                .update_control
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .as_mut()
            {
                control.staged = Some(path);
                control.cancellation_error = Some(message.clone());
            }
            return Err(message);
        }
        if let Some(control) = self
            .shared
            .update_control
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .as_mut()
        {
            control.cancellation_error = None;
        }
        let result = self.spawn_staged_cleanup(path.clone()).map_err(|error| {
            i18n::LocalizedText::from(format!(
                "{}; retained {}",
                error.render_current(),
                path.display()
            ))
        });
        if result.is_err() {
            // The cancellation journal is durable, so this failed cleanup cannot
            // launch a replacement later. A fresh update is safe.
            *self
                .shared
                .update_control
                .lock()
                .unwrap_or_else(|p| p.into_inner()) = None;
        }
        result
    }

    fn spawn_staged_cleanup(&self, path: std::path::PathBuf) -> Result<(), i18n::LocalizedText> {
        let task_group = self
            .shared
            .task_group
            .as_ref()
            .ok_or_else(|| i18n::LocalizedText::from("Update worker is unavailable"))?;
        let id = self.active_update_id().unwrap_or(0);
        let events = self.shared.update_event_tx.clone();
        let mut slot = self
            .shared
            .update_worker
            .lock()
            .map_err(|_| i18n::LocalizedText::from("Update task registry is unavailable"))?;
        if let Some(worker) = slot.take() {
            // Completion was observed before taking this slot; never wait for live work on the UI thread.
            if !worker.is_finished() {
                *slot = Some(worker);
                return Err("Update cleanup is still running".into());
            }
            let _ = worker.join();
        }
        let cleanup_path = path.clone();
        let worker = task_group
            .spawn_thread(
                TaskSpec::one_shot(
                    TaskId::new(format!("cancel-update-{}", id % 64))
                        .map_err(|error| error.to_string())?,
                ),
                move || {
                    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        app::update::discard_staged_update(&cleanup_path)
                    }));
                    let event = match result {
                        Ok(Ok(())) => SettingsUpdateTaskEvent::Cancelled,
                        Ok(Err(error)) => SettingsUpdateTaskEvent::PreparationFailed(error),
                        Err(payload) => {
                            let _ = events.send(SettingsUpdateTaskEvent::PrepareCompleted(Err(
                                format!(
                                    "Update cleanup panicked; retained {}",
                                    cleanup_path.display()
                                )
                                .into(),
                            )));
                            std::panic::resume_unwind(payload);
                        }
                    };
                    let _ = events.send(event);
                },
            )
            .map_err(|error| {
                i18n::LocalizedText::from(format!(
                    "Could not start update cleanup; retained {}: {error}",
                    path.display()
                ))
            })?;
        *slot = Some(worker);
        Ok(())
    }

    pub(in crate::session) fn drain_update_events(&self) -> Vec<SettingsUpdateTaskEvent> {
        let Ok(receiver) = self.shared.update_event_rx.lock() else {
            return Vec::new();
        };
        let pending = std::iter::from_fn(|| receiver.try_recv().ok()).collect::<Vec<_>>();
        drop(receiver);
        let mut events = Vec::new();
        for event in pending {
            if matches!(event, SettingsUpdateTaskEvent::Progress(_)) {
                if !self.update_cancelling() {
                    events.push(event);
                }
                continue;
            }
            let Ok(mut worker) = self.shared.update_worker.lock() else {
                continue;
            };
            if worker.as_ref().is_some_and(|worker| !worker.is_finished()) {
                let _ = self.shared.update_event_tx.send(event);
                continue;
            }
            if let Some(worker) = worker.take() {
                let _ = worker.join();
            }
            drop(worker);
            let event = match event {
                SettingsUpdateTaskEvent::PreparationFailed(error) => {
                    let message: i18n::LocalizedText = error.to_string().into();
                    if let Some(path) = error.retained_staging() {
                        if let Some(control) = self
                            .shared
                            .update_control
                            .lock()
                            .unwrap_or_else(|p| p.into_inner())
                            .as_mut()
                        {
                            control.cancel.cancel();
                            control.staged = Some(path.to_owned());
                            control.cancellation_error = Some(message.clone());
                        }
                        events.push(SettingsUpdateTaskEvent::PrepareCompleted(Err(message)));
                        continue;
                    }
                    SettingsUpdateTaskEvent::PrepareCompleted(Err(message))
                }
                event => event,
            };
            if self.update_cancelling() {
                match event {
                    SettingsUpdateTaskEvent::PrepareCompleted(Ok(path)) => {
                        if let Err(error) = self.start_staged_cleanup(path) {
                            events.push(SettingsUpdateTaskEvent::PrepareCompleted(Err(error)));
                        }
                        continue;
                    }
                    SettingsUpdateTaskEvent::CheckCompleted(Ok(_)) => {
                        events.push(SettingsUpdateTaskEvent::Cancelled);
                        *self
                            .shared
                            .update_control
                            .lock()
                            .unwrap_or_else(|p| p.into_inner()) = None;
                        continue;
                    }
                    _ => {}
                }
            }
            if let SettingsUpdateTaskEvent::PrepareCompleted(Ok(path)) = &event {
                if let Some(control) = self
                    .shared
                    .update_control
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .as_mut()
                {
                    control.staged = Some(path.clone());
                }
            } else {
                *self
                    .shared
                    .update_control
                    .lock()
                    .unwrap_or_else(|p| p.into_inner()) = None;
            }
            events.push(event);
        }
        events
    }

    pub(in crate::session) fn update_task_control(&self) -> TaskControl {
        let cancel = self.clone();
        let finished = self.clone();
        TaskControl::new(
            move || {
                let _ = cancel.cancel_update();
            },
            // A finished worker may still have a queued Prepared result that
            // needs cancellation cleanup. Keep it until drain acknowledges it.
            // A failed cancellation marker has no live worker; retain its error
            // for retry without trapping session shutdown in a wait loop.
            move || !finished.update_busy() || finished.update_cancel_error().is_some(),
        )
    }

    pub(in crate::session) fn validation_task_control(&self, request: u64) -> TaskControl {
        let cancel = self.clone();
        let finished = self.clone();
        TaskControl::new(
            move || {
                if let Ok(workers) = cancel.shared.workers.lock()
                    && let Some(worker) = workers.get(&request)
                {
                    worker.cancel();
                }
            },
            move || {
                finished.shared.workers.lock().is_ok_and(|workers| {
                    workers
                        .get(&request)
                        .is_none_or(|worker| worker.is_finished())
                })
            },
        )
    }

    pub(in crate::session) fn submit_time_sync_validation(
        &self,
        config: storage::TimeSyncConfig,
    ) -> Result<u64, i18n::LocalizedText> {
        use std::sync::atomic::Ordering;

        let task_group = self.shared.task_group.clone().ok_or_else(|| {
            i18n::LocalizedText::from(i18n::msg!(
                "shell-time-sync-validation-worker-is-unavailable"
            ))
        })?;
        let mut workers = self.shared.workers.lock().map_err(|_| {
            i18n::LocalizedText::from(i18n::msg!(
                "shell-time-sync-validation-task-registry-is-unavailable"
            ))
        })?;
        if !workers.is_empty() {
            return Err(i18n::LocalizedText::from(i18n::msg!(
                "shell-a-time-sync-validation-is-already-running"
            )));
        }
        let request_id = self
            .shared
            .next_request_id
            .fetch_add(1, Ordering::Relaxed)
            .max(1);
        let task_id = TaskId::new(format!("validate-server-{}", request_id % 64))
            .map_err(|error| format!("invalid time sync validation task: {error}"))?;
        let events = self.shared.event_tx.clone();
        let event_config = config.clone();
        let system_services = self.shared.system_services.clone();
        let base_config = self
            .shared
            .system_services_config
            .lock()
            .map_err(|_| {
                i18n::LocalizedText::from(i18n::msg!(
                    "shell-system-services-configuration-is-unavailable"
                ))
            })?
            .clone();
        let worker = task_group
            .spawn_thread(TaskSpec::one_shot(task_id), move || {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    let runtime = tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build()
                        .map_err(|error| {
                            time::TimeSyncError::new(vec![format!(
                                "could not start validation runtime: {error}"
                            )])
                        })?;
                    if let Some(system_services) = system_services.as_ref() {
                        system_services
                            .validate_time_source(system_services_config_for_time_sync(
                                &base_config,
                                &config,
                            ))
                            .map_err(|error| time::TimeSyncError::new(vec![error.to_string()]))
                    } else {
                        runtime.block_on(async {
                            match config.server_url.as_deref() {
                                Some(server_url) => time::fetch_time_from_server(server_url).await,
                                None => time::fetch_standard_time().await,
                            }
                        })
                    }
                }));
                let result = match result {
                    Ok(result) => result,
                    Err(payload) => {
                        let _ = events.send(SettingsTimeSyncValidationEvent {
                            request_id,
                            config: event_config.clone(),
                            result: Err(time::TimeSyncError::new(vec![
                                "time sync validation worker panicked".to_string(),
                            ])),
                        });
                        std::panic::resume_unwind(payload);
                    }
                };
                let _ = events.send(SettingsTimeSyncValidationEvent {
                    request_id,
                    config: event_config.clone(),
                    result,
                });
            })
            .map_err(|error| {
                i18n::LocalizedText::from(i18n::msg!(
                    "shell-could-not-start-time-sync-validation-error",
                    error = error.to_string()
                ))
            })?;
        workers.insert(request_id, worker);
        Ok(request_id)
    }

    pub(in crate::session) fn drain_time_sync_validation_events(
        &self,
    ) -> Vec<SettingsTimeSyncValidationEvent> {
        let Ok(receiver) = self.shared.event_rx.lock() else {
            return Vec::new();
        };
        let events = std::iter::from_fn(|| receiver.try_recv().ok()).collect::<Vec<_>>();
        drop(receiver);
        if let Ok(mut workers) = self.shared.workers.lock() {
            workers.retain(|_, worker| !worker.is_finished());
        }
        events
    }

    pub(in crate::session) fn reconfigure_system_services(&self, config: &storage::StorageConfig) {
        if let Some(system_services) = self.shared.system_services.as_ref()
            && let Ok(mut base) = self.shared.system_services_config.lock()
        {
            let next = system_services_config_for_storage_config(&base, config);
            let _ = system_services.reconfigure(next.clone());
            *base = next;
        }
    }

    pub(in crate::session) fn refresh_system_status(
        &self,
    ) -> Result<(), system_services::SystemServicesError> {
        self.shared
            .system_services
            .as_ref()
            .ok_or(system_services::SystemServicesError::Shutdown)?
            .refresh_system_status()
    }

    pub(in crate::session) fn set_system_status_active(
        &self,
        active: bool,
    ) -> Result<(), system_services::SystemServicesError> {
        self.shared
            .system_services
            .as_ref()
            .ok_or(system_services::SystemServicesError::Shutdown)?
            .set_system_status_active(active)
    }
}

pub(in crate::session) fn system_status_thresholds_from_storage(
    config: &storage::SystemStatusConfig,
) -> system_services::StorageThresholds {
    const BYTES_PER_GIB: u64 = 1024_u64 * 1024 * 1024;
    system_services::StorageThresholds {
        low_available_bytes: u64::from(config.low_available_gib)
            .checked_mul(BYTES_PER_GIB)
            .expect("bounded GiB threshold fits in u64"),
        low_percentage: config.low_percentage,
        critical_available_bytes: u64::from(config.critical_available_gib)
            .checked_mul(BYTES_PER_GIB)
            .expect("bounded GiB threshold fits in u64"),
        critical_percentage: config.critical_percentage,
    }
}

fn system_services_config_for_time_sync(
    base: &system_services::SystemServicesConfig,
    time_sync: &storage::TimeSyncConfig,
) -> system_services::SystemServicesConfig {
    let mut config = base.clone();
    config.time_sync_mode = match time_sync.source {
        storage::TimeSyncSource::NetworkServer => system_services::TimeSyncMode::Network,
        storage::TimeSyncSource::OperatingSystem => system_services::TimeSyncMode::OperatingSystem,
    };
    config.time_server_url = time_sync.server_url.clone();
    config
}

pub(in crate::session) fn system_services_config_for_storage_config(
    base: &system_services::SystemServicesConfig,
    storage_config: &storage::StorageConfig,
) -> system_services::SystemServicesConfig {
    let mut config = system_services_config_for_time_sync(base, &storage_config.time_sync);
    config.weather_location = storage_config.weather_location.clone();
    config.timezone_id = storage_config.timezone.clone();
    config.timezone_location = app::setup_timezone_options()
        .into_iter()
        .find(|timezone| timezone.id == storage_config.timezone)
        .map(|timezone| system_services::GeoLocation {
            latitude: timezone.latitude,
            longitude: timezone.longitude,
            city: Some(timezone.label),
        });
    config.storage_thresholds =
        system_status_thresholds_from_storage(&storage_config.system_status);
    config
}

impl std::fmt::Debug for ShellSettingsTaskRuntime {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ShellSettingsTaskRuntime")
            .finish_non_exhaustive()
    }
}

impl PartialEq for ShellSettingsTaskRuntime {
    fn eq(&self, _other: &Self) -> bool {
        true
    }
}

impl Eq for ShellSettingsTaskRuntime {}

#[cfg(test)]
#[path = "../../../../tests/unit/session/controller/settings/tasks/tests.rs"]
mod tests;

#[cfg(test)]
#[path = "../../../../tests/unit/session/controller/settings/tasks/update_cancellation_tests.rs"]
mod update_cancellation_tests;
