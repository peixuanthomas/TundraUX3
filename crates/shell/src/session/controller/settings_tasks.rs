use super::super::*;

#[derive(Debug)]
pub(in crate::session) struct SettingsTimeSyncValidationEvent {
    pub(in crate::session) request_id: u64,
    pub(in crate::session) config: storage::TimeSyncConfig,
    pub(in crate::session) result: TimeSyncResult,
}

#[derive(Debug)]
pub(in crate::session) enum SettingsUpdateTaskEvent {
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    Rpm(RpmTaskEvent),
    Progress(app::update::UpdateProgress),
    CheckCompleted(Result<app::update::UpdateCheckResult, i18n::LocalizedText>),
    PrepareCompleted(Result<std::path::PathBuf, i18n::LocalizedText>),
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
    #[cfg(target_os = "linux")]
    pub(in crate::session) authorization:
        Mutex<Option<Arc<dyn platform::linux::authorization::Interaction>>>,
    #[cfg(target_os = "linux")]
    pub(in crate::session) rpm_client: Mutex<Option<platform::linux::updates::RpmUpdates>>,
    #[cfg(target_os = "linux")]
    pub(in crate::session) rpm_cancellation:
        Mutex<Option<platform::linux::updates::UpdateCancellation>>,
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
                #[cfg(target_os = "linux")]
                authorization: Mutex::new(None),
                #[cfg(target_os = "linux")]
                rpm_client: Mutex::new(None),
                #[cfg(target_os = "linux")]
                rpm_cancellation: Mutex::new(None),
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
                #[cfg(target_os = "linux")]
                authorization: Mutex::new(None),
                #[cfg(target_os = "linux")]
                rpm_client: Mutex::new(None),
                #[cfg(target_os = "linux")]
                rpm_cancellation: Mutex::new(None),
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
    }

    pub(in crate::session) fn submit_update_check(
        &self,
        identity: app::update::BuildIdentity,
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
        if worker_slot.is_some() {
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
        let worker = task_group
            .spawn_thread(TaskSpec::one_shot(task_id), move || {
                let _ = events.send(SettingsUpdateTaskEvent::Progress(
                    app::update::UpdateProgress {
                        detail: app::update::UpdateProgressDetail::Status,
                        phase: app::update::UpdatePhase::Checking,
                        message: "Checking GitHub default branch".to_string(),
                    },
                ));
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    app::update::check_for_updates(&identity)
                        .map_err(|error| i18n::LocalizedText::from(error.to_string()))
                }));
                match result {
                    Ok(result) => {
                        let _ = events.send(SettingsUpdateTaskEvent::CheckCompleted(result));
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
        if worker_slot.is_some() {
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
        let worker = task_group
            .spawn_thread(TaskSpec::one_shot(task_id), move || {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    let mut report = |progress: app::update::UpdateProgress| {
                        let _ = events.send(SettingsUpdateTaskEvent::Progress(progress));
                    };
                    let prepared =
                        app::update::prepare_update(platform.as_ref(), &check, &mut report)?;
                    let work_dir = prepared.work_dir.clone();
                    report(app::update::UpdateProgress {
                        detail: app::update::UpdateProgressDetail::Status,
                        phase: app::update::UpdatePhase::PreparingReplacement,
                        message: "Preparing rollback files and restart helper".to_string(),
                    });
                    let staged = app::update::stage_update_for_apply(&prepared, &install_dir);
                    let _ = platform.cleanup_temp_path(&work_dir);
                    staged.map(|staged| staged.manifest_path)
                }));
                match result {
                    Ok(result) => {
                        let _ =
                            events
                                .send(SettingsUpdateTaskEvent::PrepareCompleted(result.map_err(
                                    |error| i18n::LocalizedText::from(error.to_string()),
                                )));
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
        *worker_slot = Some(worker);
        Ok(())
    }

    pub(in crate::session) fn drain_update_events(&self) -> Vec<SettingsUpdateTaskEvent> {
        let Ok(receiver) = self.shared.update_event_rx.lock() else {
            return Vec::new();
        };
        let events = std::iter::from_fn(|| receiver.try_recv().ok()).collect::<Vec<_>>();
        drop(receiver);
        if events.iter().any(|event| {
            matches!(
                event,
                SettingsUpdateTaskEvent::Rpm(RpmTaskEvent::Completed(_))
                    | SettingsUpdateTaskEvent::CheckCompleted(_)
                    | SettingsUpdateTaskEvent::PrepareCompleted(_)
            )
        }) && let Ok(mut worker) = self.shared.update_worker.lock()
        {
            *worker = None;
        }
        events
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
            for event in &events {
                workers.remove(&event.request_id);
            }
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
#[path = "../../../tests/unit/session/controller/settings_tasks/tests.rs"]
mod tests;
