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
    pub(in crate::session) system_services: Option<system_services::SystemServicesHandle>,
    pub(in crate::session) system_services_config: Mutex<system_services::SystemServicesConfig>,
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
                system_services: None,
                system_services_config: Mutex::new(system_services::SystemServicesConfig::default()),
            }),
        }
    }

    pub(in crate::session) fn new_managed(watchdog: AppWatchdog) -> Self {
        Self::new_managed_with_system_services(
            watchdog,
            None,
            system_services::SystemServicesConfig::default(),
        )
    }

    pub(in crate::session) fn new_managed_with_system_services(
        watchdog: AppWatchdog,
        system_services: Option<system_services::SystemServicesHandle>,
        system_services_config: system_services::SystemServicesConfig,
    ) -> Self {
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
                system_services,
                system_services_config: Mutex::new(system_services_config),
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
        let system_services = self.shared.system_services.clone();
        let base_config = self
            .shared
            .system_services_config
            .lock()
            .map_err(|_| "System services configuration is unavailable".to_string())?
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
mod tests {
    use super::*;

    struct UnavailableWeather;

    impl system_services::WeatherProvider for UnavailableWeather {
        fn current_weather<'life0, 'async_trait>(
            &'life0 self,
            _location: system_services::WeatherLocation,
            _units: system_services::WeatherUnits,
        ) -> std::pin::Pin<
            Box<
                dyn std::future::Future<Output = Result<system_services::WeatherData, String>>
                    + Send
                    + 'async_trait,
            >,
        >
        where
            'life0: 'async_trait,
            Self: 'async_trait,
        {
            Box::pin(async { Err("weather unavailable in settings test".into()) })
        }
    }

    fn system_status_platform() -> Arc<platform::mock::MockPlatform> {
        let root = std::env::temp_dir().join(format!(
            "tundra-settings-system-status-platform-{}",
            NEXT_SETTINGS_RUNTIME_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        let user_dirs = platform::UserDirs::new(
            root.join("Desktop"),
            root.join("Documents"),
            root.join("Downloads"),
            root.join("Pictures"),
            root.join("Videos"),
            root.join("Music"),
            root.join("Data"),
        )
        .unwrap();
        let app_paths = platform::build_linux_app_paths(
            root.join("Config"),
            root.join("Data"),
            root.join("Cache"),
            root.join("State"),
            root.join("Temp"),
        )
        .unwrap();
        let platform = Arc::new(platform::mock::MockPlatform::new(user_dirs, app_paths));
        platform.set_local_volumes_result(Ok(vec![platform::LocalVolume {
            root: PathBuf::from("/"),
            label: Some("System".into()),
            kind: platform::VolumeKind::Fixed,
            total_bytes: Some(10 * 1024_u64.pow(3)),
            available_bytes: Some(6 * 1024_u64.pow(3)),
            is_system: true,
            access: platform::VolumeAccess::ReadWrite,
        }]));
        platform
    }

    fn wait_for_system_status_snapshot(
        receiver: &mut tokio::sync::watch::Receiver<system_services::SystemSnapshot>,
        predicate: impl Fn(&system_services::SystemSnapshot) -> bool,
    ) {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .expect("snapshot wait runtime");
        runtime
            .block_on(async {
                tokio::time::timeout(Duration::from_secs(2), receiver.wait_for(predicate)).await
            })
            .expect("system status snapshot wait timed out")
            .expect("system status snapshot sender closed");
    }

    fn system_status_runtime_with_handle(
        handle: system_services::SystemServicesHandle,
        config: system_services::SystemServicesConfig,
    ) -> ShellSettingsTaskRuntime {
        let (event_tx, event_rx) = mpsc::channel();
        ShellSettingsTaskRuntime {
            shared: Arc::new(ShellSettingsTaskShared {
                task_group: None,
                event_tx,
                event_rx: Mutex::new(event_rx),
                workers: Mutex::new(BTreeMap::new()),
                next_request_id: std::sync::atomic::AtomicU64::new(1),
                system_services: Some(handle),
                system_services_config: Mutex::new(config),
            }),
        }
    }

    #[test]
    fn system_status_operations_report_unavailable_runtime() {
        let runtime = ShellSettingsTaskRuntime::unavailable();
        assert_eq!(
            runtime.refresh_system_status(),
            Err(system_services::SystemServicesError::Shutdown)
        );
        assert_eq!(
            runtime.set_system_status_active(true),
            Err(system_services::SystemServicesError::Shutdown)
        );
        assert_eq!(
            runtime.set_system_status_active(false),
            Err(system_services::SystemServicesError::Shutdown)
        );
    }

    #[test]
    fn system_status_operations_forward_and_reconfigure_thresholds() {
        let watchdog = default_editor_watchdog().expect("managed settings test watchdog");
        let platform = system_status_platform();
        let mut service_config = system_services::SystemServicesConfig::default();
        service_config.request_timeout = Duration::from_millis(1);
        service_config.weather_refresh_interval = Duration::from_secs(3600);
        service_config.timezone_location = Some(service_config.fallback_location.clone());
        service_config.system_status_background_refresh_interval = Duration::from_secs(3600);
        service_config.system_status_active_refresh_interval = Duration::from_secs(3600);
        let platform_trait: Arc<dyn platform::Platform> = platform;
        let (handle, mut receiver) =
            system_services::SystemServicesRuntime::start_with_platform_and_provider(
                service_config.clone(),
                watchdog.clone(),
                platform_trait,
                Arc::new(UnavailableWeather),
            );
        let runtime = system_status_runtime_with_handle(handle.clone(), service_config);

        wait_for_system_status_snapshot(&mut receiver, |snapshot| snapshot.revision > 0);
        let initial_revision = receiver.borrow().revision;
        assert_eq!(runtime.set_system_status_active(true), Ok(()));
        assert_eq!(runtime.set_system_status_active(false), Ok(()));
        assert_eq!(runtime.refresh_system_status(), Ok(()));
        wait_for_system_status_snapshot(&mut receiver, |snapshot| {
            snapshot.revision > initial_revision
        });

        let before_reconfigure = receiver.borrow().revision;
        let storage_config = storage::StorageConfig {
            system_status: storage::SystemStatusConfig {
                low_available_gib: 7,
                low_percentage: 1,
                critical_available_gib: 1,
                critical_percentage: 1,
            },
            ..storage::StorageConfig::default()
        };
        runtime.reconfigure_system_services(&storage_config);
        wait_for_system_status_snapshot(&mut receiver, |snapshot| {
            snapshot.revision > before_reconfigure
                && matches!(
                    &snapshot.storage,
                    system_services::StorageState::Ready(storage)
                        if storage.overall_pressure == system_services::StoragePressure::Low
                )
        });
        drop(receiver);
        drop(runtime);
        assert_eq!(handle.shutdown(), Ok(()));
    }

    #[test]
    fn storage_mapping_preserves_runtime_only_configuration() {
        let base = system_services::SystemServicesConfig {
            cache_dir: Some(std::path::PathBuf::from("cache/system-services")),
            weather_refresh_interval: Duration::from_secs(17),
            location_refresh_interval: Duration::from_secs(18),
            time_sync_interval: Duration::from_secs(19),
            request_timeout: Duration::from_secs(20),
            fallback_location: system_services::GeoLocation {
                latitude: 1.0,
                longitude: 2.0,
                city: Some("fallback".into()),
            },
            ..system_services::SystemServicesConfig::default()
        };
        let mapped =
            system_services_config_for_storage_config(&base, &storage::StorageConfig::default());
        assert_eq!(mapped.cache_dir, base.cache_dir);
        assert_eq!(
            mapped.weather_refresh_interval,
            base.weather_refresh_interval
        );
        assert_eq!(
            mapped.location_refresh_interval,
            base.location_refresh_interval
        );
        assert_eq!(mapped.time_sync_interval, base.time_sync_interval);
        assert_eq!(mapped.request_timeout, base.request_timeout);
        assert_eq!(mapped.fallback_location, base.fallback_location);
    }

    #[test]
    fn storage_mapping_maps_thresholds_to_exact_binary_bytes() {
        let storage_config = storage::StorageConfig {
            system_status: storage::SystemStatusConfig {
                low_available_gib: 7,
                low_percentage: 13,
                critical_available_gib: 2,
                critical_percentage: 6,
            },
            ..storage::StorageConfig::default()
        };
        let base = system_services::SystemServicesConfig {
            weather_refresh_interval: Duration::from_secs(5),
            time_sync_interval: Duration::from_secs(30),
            ..system_services::SystemServicesConfig::default()
        };
        let mapped = system_services_config_for_storage_config(&base, &storage_config);
        assert_eq!(
            mapped.storage_thresholds.low_available_bytes,
            7 * 1024_u64.pow(3)
        );
        assert_eq!(mapped.storage_thresholds.low_percentage, 13);
        assert_eq!(
            mapped.storage_thresholds.critical_available_bytes,
            2 * 1024_u64.pow(3)
        );
        assert_eq!(mapped.storage_thresholds.critical_percentage, 6);
        assert_eq!(mapped.weather_refresh_interval, Duration::from_secs(5));
        assert_eq!(mapped.time_sync_interval, Duration::from_secs(30));
    }
}
