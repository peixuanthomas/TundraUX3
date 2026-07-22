use super::super::*;

#[derive(Debug)]
pub(in crate::session) struct SettingsTimeSyncValidationEvent {
    pub(in crate::session) request_id: u64,
    pub(in crate::session) config: storage::TimeSyncConfig,
    pub(in crate::session) result: TimeSyncResult,
}

pub(in crate::session) struct ShellSettingsTaskShared {
    pub(in crate::session) task_group: Option<ManagedTaskGroup>,
    pub(in crate::session) event_tx: mpsc::Sender<SettingsTimeSyncValidationEvent>,
    pub(in crate::session) event_rx: Mutex<mpsc::Receiver<SettingsTimeSyncValidationEvent>>,
    pub(in crate::session) workers: Mutex<BTreeMap<u64, ManagedThreadHandle<()>>>,
    pub(in crate::session) next_request_id: std::sync::atomic::AtomicU64,
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
    }
}

#[derive(Clone)]
pub(in crate::session) struct ShellSettingsTaskRuntime {
    pub(in crate::session) shared: Arc<ShellSettingsTaskShared>,
}

impl ShellSettingsTaskRuntime {
    pub(in crate::session) fn unavailable() -> Self {
        let (event_tx, event_rx) = mpsc::channel();
        Self {
            shared: Arc::new(ShellSettingsTaskShared {
                task_group: None,
                event_tx,
                event_rx: Mutex::new(event_rx),
                workers: Mutex::new(BTreeMap::new()),
                next_request_id: std::sync::atomic::AtomicU64::new(1),
            }),
        }
    }

    pub(in crate::session) fn new_managed(watchdog: AppWatchdog) -> Self {
        use std::sync::atomic::Ordering;

        let (event_tx, event_rx) = mpsc::channel();
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
            }),
        }
    }

    pub(in crate::session) fn submit_time_sync_validation(
        &self,
        config: storage::TimeSyncConfig,
    ) -> Result<u64, String> {
        use std::sync::atomic::Ordering;

        let task_group = self
            .shared
            .task_group
            .clone()
            .ok_or_else(|| "Time sync validation worker is unavailable".to_string())?;
        let mut workers = self
            .shared
            .workers
            .lock()
            .map_err(|_| "Time sync validation task registry is unavailable".to_string())?;
        if !workers.is_empty() {
            return Err("A time sync validation is already running".to_string());
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
                    runtime.block_on(async {
                        match config.server_url.as_deref() {
                            Some(server_url) => time::fetch_time_from_server(server_url).await,
                            None => time::fetch_standard_time().await,
                        }
                    })
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
            .map_err(|error| format!("Could not start time sync validation: {error}"))?;
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
