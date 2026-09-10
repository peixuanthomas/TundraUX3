//! Service lifecycle, commands and the shared refresh loop.

mod cache;
mod config;
mod location;
mod system_status;
mod telemetry;
mod time_sync;
mod weather;

pub use config::SystemServicesConfig;
pub use weather::{
    MetOfficeProvider, OpenMeteoProvider, WeatherProvider, normalize_open_meteo_code,
};

use crate::model::*;
use cache::{load_weather_cache, save_weather_cache};
use chrono::{DateTime, Utc};
use location::{IpLocationDetector, resolve_location};
use std::sync::{Arc, Mutex, mpsc as std_mpsc};
use std::time::{Duration, Instant};
use system_status::refresh_due_system_sources;
use thiserror::Error;
use time_sync::{TimeAnchor, current_time_state, local_time_at, synchronize_time, validate_time};
use tokio::sync::{mpsc, watch};
use watchdog::{
    AppWatchdog, ManagedThreadHandle, PanicAction, ReplaySafety, RestartPolicy, TaskId, TaskKind,
    TaskSpec,
};

const DEFAULT_BACKOFF: [Duration; 3] = [
    Duration::from_secs(30),
    Duration::from_secs(2 * 60),
    Duration::from_secs(5 * 60),
];

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum SystemServicesError {
    #[error("system services runtime is shut down")]
    Shutdown,
    #[error("system services request was cancelled")]
    Cancelled,
    #[error("system services request timed out")]
    Timeout,
    #[error("time source validation failed: {0}")]
    Validation(String),
}

enum Command {
    Reconfigure(SystemServicesConfig),
    RefreshWeather,
    SyncTime,
    RefreshSystemStatus,
    SetSystemStatusActive(bool),
    Validate(
        SystemServicesConfig,
        std_mpsc::Sender<Result<DateTime<Utc>, SystemServicesError>>,
    ),
    Shutdown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CommandDisposition {
    Shutdown,
    Reconfigured,
    RefreshWeather,
    SyncTime,
    ReplaceValidation,
    Continue,
}

struct RuntimeShared {
    commands: mpsc::UnboundedSender<Command>,
    join: Mutex<Option<ManagedThreadHandle<()>>>,
}

#[derive(Clone)]
pub struct SystemServicesHandle {
    shared: Arc<RuntimeShared>,
    snapshots: watch::Receiver<SystemSnapshot>,
}

impl SystemServicesHandle {
    pub fn subscribe(&self) -> watch::Receiver<SystemSnapshot> {
        self.snapshots.clone()
    }
    pub fn reconfigure(&self, config: SystemServicesConfig) -> Result<(), SystemServicesError> {
        self.send(Command::Reconfigure(config))
    }
    pub fn refresh_weather(&self) -> Result<(), SystemServicesError> {
        self.send(Command::RefreshWeather)
    }
    pub fn sync_time_now(&self) -> Result<(), SystemServicesError> {
        self.send(Command::SyncTime)
    }
    pub fn refresh_system_status(&self) -> Result<(), SystemServicesError> {
        self.send(Command::RefreshSystemStatus)
    }
    pub fn set_system_status_active(&self, active: bool) -> Result<(), SystemServicesError> {
        self.send(Command::SetSystemStatusActive(active))
    }
    pub fn validate_time_source(
        &self,
        config: SystemServicesConfig,
    ) -> Result<DateTime<Utc>, SystemServicesError> {
        let timeout = config.request_timeout + Duration::from_secs(1);
        let (sender, receiver) = std_mpsc::channel();
        self.send(Command::Validate(config, sender))?;
        receiver
            .recv_timeout(timeout)
            .map_err(|_| SystemServicesError::Timeout)?
    }
    pub fn shutdown(&self) -> Result<(), SystemServicesError> {
        let _ = self.shared.commands.send(Command::Shutdown);
        if let Some(join) = self
            .shared
            .join
            .lock()
            .map_err(|_| SystemServicesError::Shutdown)?
            .take()
        {
            let _ = join.join();
        }
        Ok(())
    }
    fn send(&self, command: Command) -> Result<(), SystemServicesError> {
        self.shared
            .commands
            .send(command)
            .map_err(|_| SystemServicesError::Shutdown)
    }
}

impl Drop for RuntimeShared {
    fn drop(&mut self) {
        let _ = self.commands.send(Command::Shutdown);
        if let Ok(join) = self.join.get_mut()
            && let Some(join) = join.take()
        {
            let _ = join.join();
        }
    }
}

pub struct SystemServicesRuntime;

impl SystemServicesRuntime {
    pub fn start(
        config: SystemServicesConfig,
        watchdog: AppWatchdog,
    ) -> (SystemServicesHandle, watch::Receiver<SystemSnapshot>) {
        Self::start_with_provider(config, watchdog, Arc::new(OpenMeteoProvider::new()))
    }
    pub fn start_with_provider(
        config: SystemServicesConfig,
        watchdog: AppWatchdog,
        provider: Arc<dyn WeatherProvider>,
    ) -> (SystemServicesHandle, watch::Receiver<SystemSnapshot>) {
        Self::start_with_platform_and_provider(
            config,
            watchdog,
            Arc::from(platform::native_platform()),
            provider,
        )
    }
    pub fn start_with_platform_and_provider(
        config: SystemServicesConfig,
        watchdog: AppWatchdog,
        platform: Arc<dyn platform::Platform>,
        provider: Arc<dyn WeatherProvider>,
    ) -> (SystemServicesHandle, watch::Receiver<SystemSnapshot>) {
        let initial = snapshot(
            0,
            WeatherState::Loading,
            TimeState::Local {
                local_time: local_time_at(&config.timezone_id, Utc::now()),
            },
            StorageState::Loading,
            NetworkState::Loading,
            SystemMetricsSnapshot::loading(),
        );
        let (snapshot_tx, snapshot_rx) = watch::channel(initial);
        let (command_tx, command_rx) = mpsc::unbounded_channel();
        let tasks = watchdog.task_group("system-services");
        let mut worker_inputs = Some((config, platform, provider, snapshot_tx, command_rx));
        let join = tasks
            .spawn_thread(
                TaskSpec {
                    id: TaskId::from_static("runtime"),
                    kind: TaskKind::LongRunning,
                    panic_action: PanicAction::ReportOnly,
                    replay_safety: ReplaySafety::Never,
                    restart_policy: RestartPolicy::never(),
                },
                move || {
                    let (config, platform, provider, snapshot_tx, command_rx) = worker_inputs
                        .take()
                        .expect("the non-restartable system services worker runs once");
                    let runtime = tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build();
                    match runtime {
                        Ok(runtime) => runtime.block_on(run(
                            config,
                            platform,
                            provider,
                            snapshot_tx,
                            command_rx,
                        )),
                        Err(error) => {
                            telemetry::typed_failure("runtime", "create_runtime", &error, false)
                        }
                    }
                },
            )
            .expect("system services worker thread must start");
        let shared = Arc::new(RuntimeShared {
            commands: command_tx,
            join: Mutex::new(Some(join)),
        });
        let handle = SystemServicesHandle {
            shared,
            snapshots: snapshot_rx.clone(),
        };
        (handle, snapshot_rx)
    }
}

async fn run(
    mut config: SystemServicesConfig,
    platform: Arc<dyn platform::Platform>,
    provider: Arc<dyn WeatherProvider>,
    snapshot_tx: watch::Sender<SystemSnapshot>,
    mut commands: mpsc::UnboundedReceiver<Command>,
) {
    telemetry::lifecycle(runtime_log::LogPhase::Started);
    let mut weather_due = Instant::now();
    let mut time_due = Instant::now();
    let mut location_due = Instant::now();
    let mut system_status_due = Instant::now();
    let mut system_fast_due = Instant::now();
    let mut system_slow_due = Instant::now();
    let mut system_status_active = false;
    let mut system_location = None;
    let mut last_good: Option<WeatherSnapshot> = match load_weather_cache(&config) {
        Ok(cache) => cache,
        Err(error) => {
            telemetry::typed_failure("weather", "cache_read", &error, true);
            None
        }
    };
    if let Some(cached) = last_good.clone() {
        publish(
            &snapshot_tx,
            WeatherState::Ready(cached),
            current_time_state(&config, None, None, Instant::now()),
        );
    }
    let mut weather_failures = 0usize;
    let mut time_failures = 0usize;
    let mut anchor: Option<TimeAnchor> = None;
    let mut time_error: Option<String> = None;
    let mut pending_validation = None;
    let mut system_monitor = platform.create_system_monitor();
    'main: loop {
        let tick = tokio::time::sleep(
            system_status_due
                .min(system_fast_due)
                .min(system_slow_due)
                .saturating_duration_since(Instant::now())
                .min(Duration::from_secs(1)),
        );
        tokio::pin!(tick);
        tokio::select! {
            _ = &mut tick => {},
            command = commands.recv() => if apply_command(command, &mut config, &mut weather_due, &mut time_due, &mut location_due, &mut system_status_due, &mut system_fast_due, &mut system_slow_due, &mut system_status_active, &mut pending_validation) == CommandDisposition::Shutdown { break },
        }
        let now = Instant::now();
        if let Some((candidate, sender)) = pending_validation.take() {
            let operation =
                tokio::time::timeout(candidate.request_timeout, validate_time(&candidate));
            tokio::pin!(operation);
            loop {
                let system_tick = tokio::time::sleep(
                    system_status_due
                        .min(system_fast_due)
                        .min(system_slow_due)
                        .saturating_duration_since(Instant::now()),
                );
                tokio::pin!(system_tick);
                tokio::select! {
                    result = &mut operation => {
                        let result = result.unwrap_or(Err(SystemServicesError::Timeout));
                        let _ = sender.send(result);
                        break;
                    }
                    command = commands.recv() => {
                        match apply_command(command, &mut config, &mut weather_due, &mut time_due, &mut location_due, &mut system_status_due, &mut system_fast_due, &mut system_slow_due, &mut system_status_active, &mut pending_validation) {
                            CommandDisposition::Shutdown => {
                                let _ = sender.send(Err(SystemServicesError::Shutdown));
                                break 'main;
                            }
                            CommandDisposition::Reconfigured | CommandDisposition::ReplaceValidation => {
                                let _ = sender.send(Err(SystemServicesError::Cancelled));
                                continue 'main;
                            }
                            CommandDisposition::RefreshWeather | CommandDisposition::SyncTime | CommandDisposition::Continue => {}
                        }
                    }
                    _ = &mut system_tick => refresh_due_system_sources(
                        Instant::now(), &config, system_status_active, &snapshot_tx,
                        platform.as_ref(), &mut system_monitor, &mut system_status_due,
                        &mut system_fast_due, &mut system_slow_due,
                    ),
                }
            }
        }
        refresh_due_system_sources(
            now,
            &config,
            system_status_active,
            &snapshot_tx,
            platform.as_ref(),
            &mut system_monitor,
            &mut system_status_due,
            &mut system_fast_due,
            &mut system_slow_due,
        );
        if now >= weather_due {
            telemetry::begin("weather", "request");
            let should_refresh_location = location_due <= now;
            let operation_config = config.clone();
            let operation = tokio::time::timeout(operation_config.request_timeout, async {
                let location = resolve_location(
                    &operation_config,
                    should_refresh_location,
                    &mut system_location,
                    &IpLocationDetector,
                )
                .await;
                let weather = provider
                    .current_weather(location.weather_location(), operation_config.weather_units)
                    .await?;
                Ok::<_, String>((location, weather))
            });
            tokio::pin!(operation);
            let result = loop {
                let system_tick = tokio::time::sleep(
                    system_status_due
                        .min(system_fast_due)
                        .min(system_slow_due)
                        .saturating_duration_since(Instant::now()),
                );
                tokio::pin!(system_tick);
                tokio::select! {
                    result = &mut operation => break result.map_err(|_| "weather request timed out".to_string()).and_then(|result| result),
                    command = commands.recv() => {
                        match apply_command(command, &mut config, &mut weather_due, &mut time_due, &mut location_due, &mut system_status_due, &mut system_fast_due, &mut system_slow_due, &mut system_status_active, &mut pending_validation) {
                            CommandDisposition::Shutdown => break 'main,
                            CommandDisposition::Reconfigured | CommandDisposition::RefreshWeather => continue 'main,
                            CommandDisposition::SyncTime | CommandDisposition::ReplaceValidation | CommandDisposition::Continue => {}
                        }
                    }
                    _ = &mut system_tick => refresh_due_system_sources(
                        Instant::now(), &config, system_status_active, &snapshot_tx,
                        platform.as_ref(), &mut system_monitor, &mut system_status_due,
                        &mut system_fast_due, &mut system_slow_due,
                    ),
                }
            };
            location_due = now + config.location_refresh_interval;
            match result {
                Ok((location, weather)) => {
                    let good = WeatherSnapshot {
                        weather,
                        location: location.weather_location(),
                        city: location.city,
                        units: config.weather_units,
                        sampled_at: Utc::now(),
                    };
                    match save_weather_cache(&config, &good) {
                        Ok(()) => telemetry::recovered("weather", "cache_write"),
                        Err(error) => {
                            telemetry::typed_failure("weather", "cache_write", &error, true)
                        }
                    }
                    telemetry::recovered("weather", "request");
                    last_good = Some(good.clone());
                    weather_failures = 0;
                    publish(
                        &snapshot_tx,
                        WeatherState::Ready(good),
                        current_time_state(
                            &config,
                            anchor.as_ref(),
                            time_error.as_deref(),
                            Instant::now(),
                        ),
                    );
                    weather_due = now + config.weather_refresh_interval;
                }
                Err(error) => {
                    telemetry::failure("weather", "request", &error, last_good.is_some());
                    weather_failures += 1;
                    let state = last_good
                        .clone()
                        .map(|last_good| WeatherState::Stale {
                            last_good,
                            error: error.clone(),
                        })
                        .unwrap_or(WeatherState::Unavailable { reason: error });
                    publish(
                        &snapshot_tx,
                        state,
                        current_time_state(
                            &config,
                            anchor.as_ref(),
                            time_error.as_deref(),
                            Instant::now(),
                        ),
                    );
                    weather_due =
                        now + retry_delay(weather_failures, config.weather_refresh_interval);
                }
            }
        }
        if now >= time_due {
            telemetry::begin("time_sync", "request");
            let operation_config = config.clone();
            let operation = tokio::time::timeout(
                operation_config.request_timeout,
                synchronize_time(&operation_config),
            );
            tokio::pin!(operation);
            let result = loop {
                let system_tick = tokio::time::sleep(
                    system_status_due
                        .min(system_fast_due)
                        .min(system_slow_due)
                        .saturating_duration_since(Instant::now()),
                );
                tokio::pin!(system_tick);
                tokio::select! {
                    result = &mut operation => break result.map_err(|_| "time request timed out".to_string()).and_then(|result| result),
                    command = commands.recv() => {
                        match apply_command(command, &mut config, &mut weather_due, &mut time_due, &mut location_due, &mut system_status_due, &mut system_fast_due, &mut system_slow_due, &mut system_status_active, &mut pending_validation) {
                            CommandDisposition::Shutdown => break 'main,
                            CommandDisposition::Reconfigured | CommandDisposition::SyncTime => continue 'main,
                            CommandDisposition::RefreshWeather | CommandDisposition::ReplaceValidation | CommandDisposition::Continue => {}
                        }
                    }
                    _ = &mut system_tick => refresh_due_system_sources(
                        Instant::now(), &config, system_status_active, &snapshot_tx,
                        platform.as_ref(), &mut system_monitor, &mut system_status_due,
                        &mut system_fast_due, &mut system_slow_due,
                    ),
                }
            };
            match result {
                Ok((utc, source)) => {
                    telemetry::recovered("time_sync", "request");
                    anchor = Some(TimeAnchor {
                        utc,
                        sampled_at: utc,
                        instant: Instant::now(),
                        source,
                    });
                    time_error = None;
                    time_failures = 0;
                    time_due = now + config.time_sync_interval;
                }
                Err(error) => {
                    telemetry::failure("time_sync", "request", &error, true);
                    time_error = Some(error);
                    time_failures += 1;
                    time_due = now + retry_delay(time_failures, config.time_sync_interval);
                    let weather = snapshot_tx.borrow().weather.clone();
                    publish(
                        &snapshot_tx,
                        weather,
                        current_time_state(
                            &config,
                            anchor.as_ref(),
                            time_error.as_deref(),
                            Instant::now(),
                        ),
                    );
                }
            }
        }
        let previous = snapshot_tx.borrow().clone();
        publish(
            &snapshot_tx,
            previous.weather,
            current_time_state(
                &config,
                anchor.as_ref(),
                time_error.as_deref(),
                Instant::now(),
            ),
        );
    }
    telemetry::lifecycle(runtime_log::LogPhase::Succeeded);
}

type ValidationRequest = (
    SystemServicesConfig,
    std_mpsc::Sender<Result<DateTime<Utc>, SystemServicesError>>,
);

fn apply_command(
    command: Option<Command>,
    config: &mut SystemServicesConfig,
    weather_due: &mut Instant,
    time_due: &mut Instant,
    location_due: &mut Instant,
    system_status_due: &mut Instant,
    system_fast_due: &mut Instant,
    system_slow_due: &mut Instant,
    system_status_active: &mut bool,
    pending_validation: &mut Option<ValidationRequest>,
) -> CommandDisposition {
    match command {
        Some(Command::Shutdown) | None => {
            if let Some((_, sender)) = pending_validation.take() {
                let _ = sender.send(Err(SystemServicesError::Shutdown));
            }
            CommandDisposition::Shutdown
        }
        Some(Command::Reconfigure(next)) => {
            if let Some((_, sender)) = pending_validation.take() {
                let _ = sender.send(Err(SystemServicesError::Cancelled));
            }
            *config = next;
            *weather_due = Instant::now();
            *time_due = Instant::now();
            *location_due = Instant::now();
            *system_status_due = Instant::now();
            *system_fast_due = Instant::now();
            *system_slow_due = Instant::now();
            CommandDisposition::Reconfigured
        }
        Some(Command::RefreshWeather) => {
            *weather_due = Instant::now();
            CommandDisposition::RefreshWeather
        }
        Some(Command::SyncTime) => {
            *time_due = Instant::now();
            CommandDisposition::SyncTime
        }
        Some(Command::RefreshSystemStatus) => {
            *system_status_due = Instant::now();
            *system_fast_due = Instant::now();
            *system_slow_due = Instant::now();
            CommandDisposition::Continue
        }
        Some(Command::SetSystemStatusActive(active)) => {
            *system_status_active = active;
            *system_status_due = Instant::now();
            *system_fast_due = Instant::now();
            *system_slow_due = Instant::now();
            CommandDisposition::Continue
        }
        Some(Command::Validate(candidate, sender)) => {
            if let Some((_, previous_sender)) = pending_validation.replace((candidate, sender)) {
                let _ = previous_sender.send(Err(SystemServicesError::Cancelled));
            }
            CommandDisposition::ReplaceValidation
        }
    }
}

fn retry_delay(failures: usize, standard: Duration) -> Duration {
    DEFAULT_BACKOFF[failures.saturating_sub(1).min(DEFAULT_BACKOFF.len() - 1)].min(standard)
}
fn publish(sender: &watch::Sender<SystemSnapshot>, weather: WeatherState, time: TimeState) {
    let previous = sender.borrow().clone();
    let _ = sender.send(snapshot(
        previous.revision.saturating_add(1),
        weather,
        time,
        previous.storage,
        previous.network,
        previous.metrics,
    ));
}

fn snapshot(
    revision: u64,
    weather: WeatherState,
    time: TimeState,
    storage: StorageState,
    network: NetworkState,
    metrics: SystemMetricsSnapshot,
) -> SystemSnapshot {
    SystemSnapshot {
        revision,
        observed_at: Utc::now(),
        weather,
        time,
        storage,
        network,
        metrics,
    }
}

#[cfg(test)]
mod tests;
