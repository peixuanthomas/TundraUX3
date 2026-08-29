//! Process-wide weather and time services.
//!
//! The service owns network I/O, location resolution and its cache. UI crates
//! receive immutable snapshots through `watch` and therefore never need to
//! start a second weather or time worker.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use chrono_tz::Tz;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, mpsc as std_mpsc};
use std::time::{Duration, Instant};
use thiserror::Error;
use tokio::sync::{mpsc, watch};
use watchdog::{
    AppWatchdog, ManagedThreadHandle, PanicAction, ReplaySafety, RestartPolicy, TaskId, TaskKind,
    TaskSpec,
};

const DEFAULT_WEATHER_REFRESH: Duration = Duration::from_secs(5 * 60);
const DEFAULT_LOCATION_REFRESH: Duration = Duration::from_secs(24 * 60 * 60);
const DEFAULT_TIME_REFRESH: Duration = Duration::from_secs(5 * 60);
const DEFAULT_SYSTEM_STATUS_BACKGROUND_REFRESH: Duration = Duration::from_secs(30);
const DEFAULT_SYSTEM_STATUS_ACTIVE_REFRESH: Duration = Duration::from_secs(1);
const DEFAULT_SYSTEM_STATUS_ACTIVE_SLOW_REFRESH: Duration = Duration::from_secs(5);
const MIN_SYSTEM_STATUS_REFRESH_INTERVAL: Duration = Duration::from_millis(10);
const SYSTEM_LOCATION_DETECTION_TIMEOUT: Duration = Duration::from_secs(3);
const DEFAULT_BACKOFF: [Duration; 3] = [
    Duration::from_secs(30),
    Duration::from_secs(2 * 60),
    Duration::from_secs(5 * 60),
];
pub use system_services_model::*;

#[derive(Debug, Clone)]
pub struct SystemServicesConfig {
    pub weather_location: Option<String>,
    pub timezone_id: String,
    pub timezone_location: Option<GeoLocation>,
    pub fallback_location: GeoLocation,
    pub weather_units: WeatherUnits,
    pub time_sync_mode: TimeSyncMode,
    pub time_server_url: Option<String>,
    pub weather_refresh_interval: Duration,
    pub location_refresh_interval: Duration,
    pub time_sync_interval: Duration,
    pub cache_dir: Option<PathBuf>,
    pub request_timeout: Duration,
    pub storage_thresholds: StorageThresholds,
    pub system_status_background_refresh_interval: Duration,
    pub system_status_active_refresh_interval: Duration,
    pub system_status_active_slow_refresh_interval: Duration,
}

impl Default for SystemServicesConfig {
    fn default() -> Self {
        Self {
            weather_location: None,
            timezone_id: "UTC".to_string(),
            timezone_location: None,
            fallback_location: GeoLocation::fallback(),
            weather_units: WeatherUnits::default(),
            time_sync_mode: TimeSyncMode::Network,
            time_server_url: None,
            weather_refresh_interval: DEFAULT_WEATHER_REFRESH,
            location_refresh_interval: DEFAULT_LOCATION_REFRESH,
            time_sync_interval: DEFAULT_TIME_REFRESH,
            cache_dir: None,
            request_timeout: Duration::from_secs(10),
            storage_thresholds: StorageThresholds {
                low_available_bytes: 5 * 1024 * 1024 * 1024,
                low_percentage: 10,
                critical_available_bytes: 1024 * 1024 * 1024,
                critical_percentage: 5,
            },
            system_status_background_refresh_interval: DEFAULT_SYSTEM_STATUS_BACKGROUND_REFRESH,
            system_status_active_refresh_interval: DEFAULT_SYSTEM_STATUS_ACTIVE_REFRESH,
            system_status_active_slow_refresh_interval: DEFAULT_SYSTEM_STATUS_ACTIVE_SLOW_REFRESH,
        }
    }
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum SystemServicesError {
    #[error("system services runtime is shut down")]
    Shutdown,
    #[error("system services request timed out")]
    Timeout,
    #[error("time source validation failed: {0}")]
    Validation(String),
}

#[async_trait]
pub trait WeatherProvider: Send + Sync + 'static {
    async fn current_weather(
        &self,
        location: WeatherLocation,
        units: WeatherUnits,
    ) -> Result<WeatherData, String>;
}

#[derive(Default)]
pub struct OpenMeteoProvider {
    client: reqwest::Client,
}

impl OpenMeteoProvider {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::new(),
        }
    }
}

#[derive(Deserialize)]
struct OpenMeteoResponse {
    current: OpenMeteoCurrent,
}
#[derive(Deserialize)]
struct OpenMeteoCurrent {
    temperature_2m: f64,
    is_day: i32,
    precipitation: f64,
    weather_code: i32,
    wind_speed_10m: f64,
    wind_direction_10m: f64,
    time: String,
}

#[async_trait]
impl WeatherProvider for OpenMeteoProvider {
    async fn current_weather(
        &self,
        location: WeatherLocation,
        _units: WeatherUnits,
    ) -> Result<WeatherData, String> {
        let url = format!(
            "https://api.open-meteo.com/v1/forecast?latitude={}&longitude={}&current=temperature_2m,is_day,precipitation,weather_code,wind_speed_10m,wind_direction_10m&wind_speed_unit=ms&timezone=auto",
            location.latitude, location.longitude
        );
        let response = self
            .client
            .get(&url)
            .timeout(Duration::from_secs(10))
            .send()
            .await
            .map_err(|error| error.to_string())?
            .error_for_status()
            .map_err(|error| error.to_string())?;
        let current = response
            .json::<OpenMeteoResponse>()
            .await
            .map_err(|error| error.to_string())?
            .current;
        Ok(WeatherData {
            condition: normalize_open_meteo_code(current.weather_code),
            temperature: current.temperature_2m,
            precipitation: current.precipitation,
            wind_speed: current.wind_speed_10m,
            wind_direction: current.wind_direction_10m,
            sun: CelestialEvents::only_day(current.is_day),
            moon_phase: Some(0.5),
            timestamp: current.time,
            attribution: String::new(),
        })
    }
}

/// Optional Met Office Weather DataHub provider. Callers must opt in by
/// constructing it with an API key and passing it to `start_with_provider`.
pub struct MetOfficeProvider {
    client: reqwest::Client,
    data_source: String,
}
impl MetOfficeProvider {
    pub fn new(api_key: &str, data_source: Option<&str>) -> Result<Self, String> {
        use reqwest::header::{HeaderMap, HeaderValue};
        if api_key.is_empty() {
            return Err("Met Office API key is empty".to_string());
        }
        let mut value = HeaderValue::from_str(api_key).map_err(|error| error.to_string())?;
        value.set_sensitive(true);
        let mut headers = HeaderMap::new();
        headers.insert("apikey", value);
        let client = reqwest::Client::builder()
            .default_headers(headers)
            .build()
            .map_err(|error| error.to_string())?;
        Ok(Self {
            client,
            data_source: data_source
                .filter(|value| !value.is_empty())
                .unwrap_or("BD1")
                .to_string(),
        })
    }
}
#[derive(Deserialize)]
struct MetOfficeResponse {
    features: Vec<MetOfficeFeature>,
}
#[derive(Deserialize)]
struct MetOfficeFeature {
    properties: MetOfficeProperties,
}
#[derive(Deserialize)]
struct MetOfficeProperties {
    #[serde(rename = "timeSeries")]
    time_series: Vec<MetOfficeSeries>,
}
#[derive(Deserialize)]
struct MetOfficeSeries {
    #[serde(rename = "precipitationRate")]
    precipitation: f64,
    #[serde(rename = "screenTemperature")]
    temperature: f64,
    #[serde(rename = "significantWeatherCode")]
    weather_code: i32,
    time: String,
    #[serde(rename = "windDirectionFrom10m")]
    wind_direction: f64,
    #[serde(rename = "windSpeed10m")]
    wind_speed: f64,
}
#[async_trait]
impl WeatherProvider for MetOfficeProvider {
    async fn current_weather(
        &self,
        location: WeatherLocation,
        _units: WeatherUnits,
    ) -> Result<WeatherData, String> {
        let url = format!(
            "https://data.hub.api.metoffice.gov.uk/sitespecific/v0/point/hourly?latitude={}&longitude={}&includeLocationName=true&dataSource={}",
            location.latitude, location.longitude, self.data_source
        );
        let response = self
            .client
            .get(url)
            .send()
            .await
            .map_err(|error| error.to_string())?
            .error_for_status()
            .map_err(|error| error.to_string())?
            .json::<MetOfficeResponse>()
            .await
            .map_err(|error| error.to_string())?;
        let current = response
            .features
            .into_iter()
            .next()
            .and_then(|feature| {
                feature.properties.time_series.into_iter().find(|series| {
                    let time = format!("{}:00Z", series.time.trim_end_matches('Z'));
                    time.parse::<DateTime<Utc>>().is_ok_and(|start| {
                        Utc::now() >= start && Utc::now() <= start + chrono::Duration::hours(1)
                    })
                })
            })
            .ok_or_else(|| "Met Office returned no current weather".to_string())?;
        Ok(WeatherData {
            condition: normalize_met_office_code(current.weather_code),
            temperature: current.temperature,
            precipitation: current.precipitation,
            wind_speed: current.wind_speed,
            wind_direction: current.wind_direction,
            sun: CelestialEvents::from_bool(true),
            moon_phase: Some(0.5),
            timestamp: current.time,
            attribution: "Data supplied by the Met Office".to_string(),
        })
    }
}

pub fn normalize_open_meteo_code(code: i32) -> WeatherCondition {
    match code {
        0 => WeatherCondition::Clear,
        1 | 2 => WeatherCondition::PartlyCloudy,
        3 => WeatherCondition::Overcast,
        45 | 48 => WeatherCondition::Fog,
        51 | 53 | 55 => WeatherCondition::Drizzle,
        56 | 57 => WeatherCondition::FreezingRain,
        61 | 63 | 65 => WeatherCondition::Rain,
        66 | 67 => WeatherCondition::FreezingRain,
        71 | 73 | 75 => WeatherCondition::Snow,
        77 => WeatherCondition::SnowGrains,
        80..=82 => WeatherCondition::RainShowers,
        85 | 86 => WeatherCondition::SnowShowers,
        95 => WeatherCondition::Thunderstorm,
        96 | 99 => WeatherCondition::ThunderstormHail,
        _ => WeatherCondition::Clear,
    }
}

/// Converts Met Office DataHub significant weather codes, which are distinct
/// from the WMO codes used by Open-Meteo.
fn normalize_met_office_code(code: i32) -> WeatherCondition {
    match code {
        0 | 1 => WeatherCondition::Clear,
        2 | 3 => WeatherCondition::PartlyCloudy,
        5 | 6 => WeatherCondition::Fog,
        7 => WeatherCondition::Cloudy,
        8 => WeatherCondition::Overcast,
        -1 | 11 => WeatherCondition::Drizzle,
        9 | 10 | 13 | 14 => WeatherCondition::RainShowers,
        12 | 15 => WeatherCondition::Rain,
        // The shared model has no sleet or hail-only variants. Preserve their
        // frozen-precipitation semantics instead of misclassifying them as rain.
        16 | 17 | 22 | 23 | 25 | 26 => WeatherCondition::SnowShowers,
        18 => WeatherCondition::SnowGrains,
        19..=21 => WeatherCondition::ThunderstormHail,
        24 | 27 => WeatherCondition::Snow,
        28..=31 => WeatherCondition::Thunderstorm,
        _ => WeatherCondition::Clear,
    }
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
                    if let Ok(runtime) = runtime {
                        runtime.block_on(run(config, platform, provider, snapshot_tx, command_rx));
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
    let mut weather_due = Instant::now();
    let mut time_due = Instant::now();
    let mut location_due = Instant::now();
    let mut system_status_due = Instant::now();
    let mut system_fast_due = Instant::now();
    let mut system_slow_due = Instant::now();
    let mut system_status_active = false;
    let mut system_location = None;
    let mut last_good: Option<WeatherSnapshot> = load_weather_cache(&config).ok().flatten();
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
            command = commands.recv() => if apply_command(command, &mut config, &mut weather_due, &mut time_due, &mut location_due, &mut system_status_due, &mut system_fast_due, &mut system_slow_due, &mut system_status_active, &mut pending_validation) { break },
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
                        let _ = sender.send(Err(SystemServicesError::Shutdown));
                        if apply_command(command, &mut config, &mut weather_due, &mut time_due, &mut location_due, &mut system_status_due, &mut system_fast_due, &mut system_slow_due, &mut system_status_active, &mut pending_validation) { break 'main; }
                        continue 'main;
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
                        if apply_command(command, &mut config, &mut weather_due, &mut time_due, &mut location_due, &mut system_status_due, &mut system_fast_due, &mut system_slow_due, &mut system_status_active, &mut pending_validation) { break 'main; }
                        continue 'main;
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
                    let _ = save_weather_cache(&config, &good);
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
                        if apply_command(command, &mut config, &mut weather_due, &mut time_due, &mut location_due, &mut system_status_due, &mut system_fast_due, &mut system_slow_due, &mut system_status_active, &mut pending_validation) { break 'main; }
                        continue 'main;
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
}

#[allow(clippy::too_many_arguments)]
fn refresh_due_system_sources(
    now: Instant,
    config: &SystemServicesConfig,
    active: bool,
    sender: &watch::Sender<SystemSnapshot>,
    platform: &dyn platform::Platform,
    monitor: &mut Result<Box<dyn platform::SystemMonitor>, platform::PlatformError>,
    system_status_due: &mut Instant,
    system_fast_due: &mut Instant,
    system_slow_due: &mut Instant,
) {
    if now >= *system_status_due {
        refresh_system_status(sender, platform, config.storage_thresholds);
        *system_status_due = now + system_status_refresh_interval(config, active);
    }
    if now >= *system_fast_due {
        refresh_fast_metrics(sender, monitor);
        *system_fast_due = now
            + if active {
                config
                    .system_status_active_refresh_interval
                    .max(MIN_SYSTEM_STATUS_REFRESH_INTERVAL)
            } else {
                config
                    .system_status_background_refresh_interval
                    .max(MIN_SYSTEM_STATUS_REFRESH_INTERVAL)
            };
    }
    if now >= *system_slow_due {
        refresh_slow_metrics(sender, monitor);
        *system_slow_due = now
            + if active {
                config
                    .system_status_active_slow_refresh_interval
                    .max(MIN_SYSTEM_STATUS_REFRESH_INTERVAL)
            } else {
                config
                    .system_status_background_refresh_interval
                    .max(MIN_SYSTEM_STATUS_REFRESH_INTERVAL)
            };
    }
}

fn system_status_refresh_interval(config: &SystemServicesConfig, active: bool) -> Duration {
    let configured = if active {
        config.system_status_active_slow_refresh_interval
    } else {
        config.system_status_background_refresh_interval
    };
    configured.max(MIN_SYSTEM_STATUS_REFRESH_INTERVAL)
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
) -> bool {
    match command {
        Some(Command::Shutdown) | None => true,
        Some(Command::Reconfigure(next)) => {
            *config = next;
            *weather_due = Instant::now();
            *time_due = Instant::now();
            *location_due = Instant::now();
            *system_status_due = Instant::now();
            *system_fast_due = Instant::now();
            *system_slow_due = Instant::now();
            false
        }
        Some(Command::RefreshWeather) => {
            *weather_due = Instant::now();
            false
        }
        Some(Command::SyncTime) => {
            *time_due = Instant::now();
            false
        }
        Some(Command::RefreshSystemStatus) => {
            *system_status_due = Instant::now();
            *system_fast_due = Instant::now();
            *system_slow_due = Instant::now();
            false
        }
        Some(Command::SetSystemStatusActive(active)) => {
            *system_status_active = active;
            *system_status_due = Instant::now();
            *system_fast_due = Instant::now();
            *system_slow_due = Instant::now();
            false
        }
        Some(Command::Validate(candidate, sender)) => {
            *pending_validation = Some((candidate, sender));
            false
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

fn refresh_system_status(
    sender: &watch::Sender<SystemSnapshot>,
    platform: &dyn platform::Platform,
    thresholds: StorageThresholds,
) {
    let previous = sender.borrow().clone();
    let storage = match platform.local_volumes() {
        Ok(volumes) => StorageState::Ready(map_storage(volumes, thresholds)),
        Err(error) => match previous.storage {
            StorageState::Ready(last_good) | StorageState::Stale { last_good, .. } => {
                StorageState::Stale {
                    last_good,
                    error: error.to_string(),
                }
            }
            StorageState::Loading | StorageState::Unavailable { .. } => StorageState::Unavailable {
                reason: error.to_string(),
            },
        },
    };
    let network = match platform.network_status() {
        Ok(status) => NetworkState::Ready(map_network(status)),
        Err(error) => match previous.network {
            NetworkState::Ready(last_good) | NetworkState::Stale { last_good, .. } => {
                NetworkState::Stale {
                    last_good,
                    error: error.to_string(),
                }
            }
            NetworkState::Loading | NetworkState::Unavailable { .. } => NetworkState::Unavailable {
                reason: error.to_string(),
            },
        },
    };
    let _ = sender.send(snapshot(
        previous.revision.saturating_add(1),
        previous.weather,
        previous.time,
        storage,
        network,
        previous.metrics,
    ));
}

fn unavailable_or_stale<T: Clone>(previous: &MetricState<T>, error: String) -> MetricState<T> {
    match previous {
        MetricState::Ready(last_good) | MetricState::Stale { last_good, .. } => {
            MetricState::Stale {
                last_good: last_good.clone(),
                error,
            }
        }
        MetricState::Loading | MetricState::Unavailable { .. } => {
            MetricState::Unavailable { reason: error }
        }
    }
}

fn refresh_fast_metrics(
    sender: &watch::Sender<SystemSnapshot>,
    monitor: &mut Result<Box<dyn platform::SystemMonitor>, platform::PlatformError>,
) {
    let previous = sender.borrow().clone();
    let mut metrics = previous.metrics.clone();
    match monitor {
        Ok(monitor) => match monitor.sample_fast() {
            Ok(sample) => {
                metrics.cpu = MetricState::Ready(CpuSnapshot {
                    usage_percent: sample.cpu.usage_percent,
                    per_core_percent: sample.cpu.per_core_percent,
                    logical_core_count: sample.cpu.logical_core_count,
                    physical_core_count: sample.cpu.physical_core_count,
                });
                metrics.memory = MetricState::Ready(MemorySnapshot {
                    total_bytes: sample.memory.total_bytes,
                    used_bytes: sample.memory.used_bytes,
                    available_bytes: sample.memory.available_bytes,
                    swap_total_bytes: sample.memory.swap_total_bytes,
                    swap_used_bytes: sample.memory.swap_used_bytes,
                });
                metrics.uptime = MetricState::Ready(UptimeSnapshot {
                    seconds: sample.uptime_seconds,
                });
                metrics.load = if sample.load.supported {
                    MetricState::Ready(LoadSnapshot {
                        one: sample.load.one,
                        five: sample.load.five,
                        fifteen: sample.load.fifteen,
                    })
                } else {
                    MetricState::Unavailable {
                        reason: "load average is unsupported on this platform".into(),
                    }
                };
                let interfaces = sample
                    .network_interfaces
                    .into_iter()
                    .map(|value| NetworkIoInterfaceSnapshot {
                        name: value.name,
                        received_bytes: value.received_bytes,
                        transmitted_bytes: value.transmitted_bytes,
                        received_bytes_per_second: value.received_bytes_per_second,
                        transmitted_bytes_per_second: value.transmitted_bytes_per_second,
                    })
                    .collect::<Vec<_>>();
                let aggregate = interfaces
                    .iter()
                    .filter(|value| !is_loopback_interface(&value.name));
                let (mut rx, mut tx, mut rx_rate, mut tx_rate) = (0, 0, 0.0, 0.0);
                for value in aggregate {
                    rx += value.received_bytes;
                    tx += value.transmitted_bytes;
                    rx_rate += value.received_bytes_per_second;
                    tx_rate += value.transmitted_bytes_per_second;
                }
                metrics.network_io = MetricState::Ready(NetworkIoSnapshot {
                    interfaces,
                    total_received_bytes: rx,
                    total_transmitted_bytes: tx,
                    total_received_bytes_per_second: rx_rate,
                    total_transmitted_bytes_per_second: tx_rate,
                });
            }
            Err(error) => {
                let error = error.to_string();
                metrics.cpu = unavailable_or_stale(&metrics.cpu, error.clone());
                metrics.memory = unavailable_or_stale(&metrics.memory, error.clone());
                metrics.load = unavailable_or_stale(&metrics.load, error.clone());
                metrics.uptime = unavailable_or_stale(&metrics.uptime, error.clone());
                metrics.network_io = unavailable_or_stale(&metrics.network_io, error);
            }
        },
        Err(error) => {
            let error = error.to_string();
            metrics.cpu = unavailable_or_stale(&metrics.cpu, error.clone());
            metrics.memory = unavailable_or_stale(&metrics.memory, error.clone());
            metrics.load = unavailable_or_stale(&metrics.load, error.clone());
            metrics.uptime = unavailable_or_stale(&metrics.uptime, error.clone());
            metrics.network_io = unavailable_or_stale(&metrics.network_io, error);
        }
    }
    metrics.sampled_at = Utc::now();
    let _ = sender.send(snapshot(
        previous.revision.saturating_add(1),
        previous.weather,
        previous.time,
        previous.storage,
        previous.network,
        metrics,
    ));
}

fn refresh_slow_metrics(
    sender: &watch::Sender<SystemSnapshot>,
    monitor: &mut Result<Box<dyn platform::SystemMonitor>, platform::PlatformError>,
) {
    let previous = sender.borrow().clone();
    let mut metrics = previous.metrics.clone();
    match monitor {
        Ok(monitor) => match monitor.sample_slow() {
            Ok(sample) => {
                metrics.identity = match sample.identity {
                    Ok(value) => MetricState::Ready(SystemIdentitySnapshot {
                        host_name: value.host_name,
                        os_name: value.os_name,
                        os_version: value.os_version,
                        kernel_version: value.kernel_version,
                    }),
                    Err(reason) => unavailable_or_stale(&metrics.identity, reason),
                };
                metrics.thermal = match sample.thermal {
                    Ok(values) => MetricState::Ready(
                        values
                            .into_iter()
                            .map(|v| ThermalSensorSnapshot {
                                label: v.label,
                                temperature_celsius: v.temperature_celsius,
                                critical_celsius: v.critical_celsius,
                            })
                            .collect(),
                    ),
                    Err(reason) => unavailable_or_stale(&metrics.thermal, reason),
                };
                metrics.batteries = match sample.batteries {
                    Ok(values) => MetricState::Ready(
                        values
                            .into_iter()
                            .map(|v| BatterySnapshot {
                                vendor: v.vendor,
                                model: v.model,
                                state: match v.state {
                                    platform::BatterySampleState::Charging => {
                                        BatteryState::Charging
                                    }
                                    platform::BatterySampleState::Discharging => {
                                        BatteryState::Discharging
                                    }
                                    platform::BatterySampleState::Full => BatteryState::Full,
                                    platform::BatterySampleState::Empty => BatteryState::Empty,
                                    platform::BatterySampleState::Unknown => BatteryState::Unknown,
                                },
                                charge_percent: v.charge_percent,
                                energy_wh: v.energy_wh,
                                energy_full_wh: v.energy_full_wh,
                                time_to_empty_seconds: v.time_to_empty_seconds,
                                time_to_full_seconds: v.time_to_full_seconds,
                            })
                            .collect(),
                    ),
                    Err(reason) => unavailable_or_stale(&metrics.batteries, reason),
                };
                metrics.processes = MetricState::Ready(ProcessRankingsSnapshot {
                    top_cpu: sample.top_cpu.into_iter().map(map_process).collect(),
                    top_memory: sample.top_memory.into_iter().map(map_process).collect(),
                });
            }
            Err(error) => {
                let error = error.to_string();
                metrics.identity = unavailable_or_stale(&metrics.identity, error.clone());
                metrics.thermal = unavailable_or_stale(&metrics.thermal, error.clone());
                metrics.batteries = unavailable_or_stale(&metrics.batteries, error.clone());
                metrics.processes = unavailable_or_stale(&metrics.processes, error);
            }
        },
        Err(error) => {
            let error = error.to_string();
            metrics.identity = unavailable_or_stale(&metrics.identity, error.clone());
            metrics.thermal = unavailable_or_stale(&metrics.thermal, error.clone());
            metrics.batteries = unavailable_or_stale(&metrics.batteries, error.clone());
            metrics.processes = unavailable_or_stale(&metrics.processes, error);
        }
    }
    metrics.sampled_at = Utc::now();
    let _ = sender.send(snapshot(
        previous.revision.saturating_add(1),
        previous.weather,
        previous.time,
        previous.storage,
        previous.network,
        metrics,
    ));
}

fn map_process(value: platform::ProcessMetricSample) -> ProcessMetricSnapshot {
    ProcessMetricSnapshot {
        pid: value.pid,
        name: value.name,
        cpu_percent: value.cpu_percent,
        memory_bytes: value.memory_bytes,
    }
}

fn is_loopback_interface(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    name == "lo"
        || name
            .strip_prefix("lo")
            .is_some_and(|suffix| suffix.chars().all(|character| character.is_ascii_digit()))
        || name.starts_with("loopback")
}

fn map_storage(
    volumes: Vec<platform::LocalVolume>,
    thresholds: StorageThresholds,
) -> StorageSnapshot {
    let volumes: Vec<_> = volumes
        .into_iter()
        .map(|volume| StorageVolumeSnapshot {
            identifier: volume.root.to_string_lossy().into_owned(),
            label: volume.label,
            kind: match volume.kind {
                platform::VolumeKind::Fixed => StorageVolumeKind::Fixed,
                platform::VolumeKind::Removable => StorageVolumeKind::Removable,
            },
            is_system: volume.is_system,
            access: match volume.access {
                platform::VolumeAccess::ReadWrite => StorageVolumeAccess::ReadWrite,
                platform::VolumeAccess::ReadOnly => StorageVolumeAccess::ReadOnly,
                platform::VolumeAccess::Unavailable => StorageVolumeAccess::Unavailable,
            },
            total_bytes: volume.total_bytes,
            available_bytes: volume.available_bytes,
            pressure: thresholds.classify(volume.total_bytes, volume.available_bytes),
        })
        .collect();
    let detected = volumes.iter().position(|volume| volume.is_system);
    let fallback = volumes
        .iter()
        .position(|volume| volume.kind == StorageVolumeKind::Fixed);
    let (system_volume_index, system_volume_source) = if let Some(index) = detected {
        (Some(index), SystemVolumeSource::Detected)
    } else if let Some(index) = fallback {
        (Some(index), SystemVolumeSource::FixedVolumeFallback)
    } else {
        (None, SystemVolumeSource::Unavailable)
    };
    let overall_pressure = volumes
        .iter()
        .map(|volume| volume.pressure)
        .filter(|pressure| *pressure != StoragePressure::Unknown)
        .max()
        .unwrap_or(StoragePressure::Unknown);
    StorageSnapshot {
        volumes,
        overall_pressure,
        system_volume_index,
        system_volume_source,
        sampled_at: Utc::now(),
    }
}

fn map_network(status: platform::NetworkStatus) -> NetworkSnapshot {
    let active_link_count = status.active_link_count();
    let has_active_link = status.has_active_link();
    let interfaces = status
        .interfaces
        .into_iter()
        .map(|interface| NetworkInterfaceSnapshot {
            name: interface.name,
            display_name: interface.display_name,
            kind: match interface.kind {
                platform::NetworkInterfaceKind::Wired => NetworkInterfaceKind::Wired,
                platform::NetworkInterfaceKind::Wireless => NetworkInterfaceKind::Wireless,
                platform::NetworkInterfaceKind::Virtual => NetworkInterfaceKind::Virtual,
                platform::NetworkInterfaceKind::Unknown => NetworkInterfaceKind::Unknown,
            },
            link_state: match interface.link_state {
                platform::NetworkLinkState::Up => NetworkLinkState::Up,
                platform::NetworkLinkState::Down => NetworkLinkState::Down,
                platform::NetworkLinkState::Unknown => NetworkLinkState::Unknown,
            },
            addresses: interface
                .addresses
                .into_iter()
                .map(|address| address.to_string())
                .collect(),
        })
        .collect();
    NetworkSnapshot {
        interfaces,
        active_link_count,
        has_active_link,
        sampled_at: Utc::now(),
    }
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

fn parse_timezone(timezone: &str) -> Option<Tz> {
    timezone.parse::<Tz>().ok()
}
fn local_time_at(timezone: &str, utc: DateTime<Utc>) -> LocalTime {
    parse_timezone(timezone)
        .map(|tz| utc.with_timezone(&tz).fixed_offset())
        .unwrap_or_else(|| utc.fixed_offset())
}
fn current_time_state(
    config: &SystemServicesConfig,
    anchor: Option<&TimeAnchor>,
    error: Option<&str>,
    now: Instant,
) -> TimeState {
    let utc = anchor.map_or_else(Utc::now, |anchor| anchor.utc_at(now));
    let local = local_time_at(&config.timezone_id, utc);
    if let Some(error) = error {
        return TimeState::Degraded {
            local_time: local,
            last_sync: anchor.map(|anchor| anchor.utc),
            error: error.to_string(),
        };
    }
    match anchor {
        Some(anchor) => TimeState::Synced {
            utc,
            local_time: local,
            source: anchor.source.clone(),
            sampled_at: anchor.sampled_at,
        },
        None => TimeState::Local { local_time: local },
    }
}

#[derive(Debug, Clone)]
struct TimeAnchor {
    utc: DateTime<Utc>,
    sampled_at: DateTime<Utc>,
    instant: Instant,
    source: TimeSource,
}
impl TimeAnchor {
    fn utc_at(&self, now: Instant) -> DateTime<Utc> {
        self.utc + now.saturating_duration_since(self.instant)
    }
}

async fn synchronize_time(
    config: &SystemServicesConfig,
) -> Result<(DateTime<Utc>, TimeSource), String> {
    match config.time_sync_mode {
        TimeSyncMode::OperatingSystem => Ok((Utc::now(), TimeSource::OperatingSystem)),
        TimeSyncMode::Network => {
            let result = match config.time_server_url.as_deref() {
                Some(url) => time::fetch_time_from_server(url).await,
                None => time::fetch_standard_time().await,
            };
            result
                .map(|utc| {
                    (
                        utc,
                        TimeSource::Network(
                            config
                                .time_server_url
                                .clone()
                                .unwrap_or_else(|| "standard".to_string()),
                        ),
                    )
                })
                .map_err(|error| error.to_string())
        }
    }
}
async fn validate_time(
    config: &SystemServicesConfig,
) -> Result<DateTime<Utc>, SystemServicesError> {
    synchronize_time(config)
        .await
        .map(|(utc, _)| utc)
        .map_err(SystemServicesError::Validation)
}

#[async_trait]
trait SystemLocationDetector: Send + Sync {
    async fn detect(&self) -> Option<GeoLocation>;
}

struct IpLocationDetector;

#[derive(Deserialize)]
struct IpLocationResponse {
    latitude: f64,
    longitude: f64,
    city: Option<String>,
}

#[async_trait]
impl SystemLocationDetector for IpLocationDetector {
    async fn detect(&self) -> Option<GeoLocation> {
        let response = reqwest::Client::new()
            .get("https://ipapi.co/json/")
            .header(
                "User-Agent",
                format!("tundra-system-services/{}", env!("CARGO_PKG_VERSION")),
            )
            .send()
            .await
            .ok()?
            .error_for_status()
            .ok()?;
        let location = response.json::<IpLocationResponse>().await.ok()?;
        Some(GeoLocation {
            latitude: location.latitude,
            longitude: location.longitude,
            city: location.city,
        })
    }
}

async fn resolve_location(
    config: &SystemServicesConfig,
    should_refresh_location: bool,
    system_location: &mut Option<GeoLocation>,
    detector: &dyn SystemLocationDetector,
) -> GeoLocation {
    if let Some(query) = config
        .weather_location
        .as_deref()
        .map(str::trim)
        .filter(|query| !query.is_empty())
    {
        if let Some(cached) = load_location_cache(config, query) {
            return cached;
        }
        if should_refresh_location && let Some(resolved) = geocode(query).await {
            let _ = save_location_cache(config, query, &resolved);
            return resolved;
        }
    }
    if let Some(location) = config.timezone_location.clone() {
        return location;
    }
    if (should_refresh_location || system_location.is_none())
        && let Some(location) = tokio::time::timeout(
            config
                .request_timeout
                .min(SYSTEM_LOCATION_DETECTION_TIMEOUT),
            detector.detect(),
        )
        .await
        .ok()
        .flatten()
    {
        *system_location = Some(location);
    }
    system_location
        .clone()
        .unwrap_or_else(|| config.fallback_location.clone())
}

#[derive(Deserialize)]
struct GeocodeResponse {
    lat: String,
    lon: String,
    display_name: Option<String>,
    address: Option<GeocodeAddress>,
}
#[derive(Deserialize)]
struct GeocodeAddress {
    city: Option<String>,
    town: Option<String>,
    village: Option<String>,
    municipality: Option<String>,
}
async fn geocode(query: &str) -> Option<GeoLocation> {
    let mut url = reqwest::Url::parse("https://nominatim.openstreetmap.org/search").ok()?;
    url.query_pairs_mut()
        .append_pair("q", query)
        .append_pair("format", "json")
        .append_pair("limit", "1")
        .append_pair("addressdetails", "1");
    let client = reqwest::Client::new();
    let response = client
        .get(url)
        .header(
            "User-Agent",
            format!("tundra-system-services/{}", env!("CARGO_PKG_VERSION")),
        )
        .timeout(Duration::from_secs(10))
        .send()
        .await
        .ok()?
        .error_for_status()
        .ok()?;
    let item = response
        .json::<Vec<GeocodeResponse>>()
        .await
        .ok()?
        .into_iter()
        .next()?;
    Some(GeoLocation {
        latitude: item.lat.parse().ok()?,
        longitude: item.lon.parse().ok()?,
        city: item
            .address
            .and_then(|address| {
                address
                    .city
                    .or(address.town)
                    .or(address.village)
                    .or(address.municipality)
            })
            .or_else(|| {
                item.display_name.and_then(|name| {
                    name.split(',')
                        .next()
                        .map(str::trim)
                        .filter(|part| !part.is_empty())
                        .map(str::to_string)
                })
            }),
    })
}

#[derive(Serialize, Deserialize)]
struct CachedWeather {
    saved_at: DateTime<Utc>,
    snapshot: WeatherSnapshot,
}
fn cache_root(config: &SystemServicesConfig) -> Option<PathBuf> {
    config
        .cache_dir
        .clone()
        .or_else(|| dirs::cache_dir().map(|dir| dir.join("system-services")))
}
fn load_weather_cache(config: &SystemServicesConfig) -> Result<Option<WeatherSnapshot>, ()> {
    let path = cache_root(config).ok_or(())?.join("weather.json");
    let contents = fs::read_to_string(path).map_err(|_| ())?;
    let cached: CachedWeather = serde_json::from_str(&contents).map_err(|_| ())?;
    Ok(Some(cached.snapshot))
}
fn save_weather_cache(config: &SystemServicesConfig, snapshot: &WeatherSnapshot) -> Result<(), ()> {
    let root = cache_root(config).ok_or(())?;
    fs::create_dir_all(&root).map_err(|_| ())?;
    let body = serde_json::to_string(&CachedWeather {
        saved_at: Utc::now(),
        snapshot: snapshot.clone(),
    })
    .map_err(|_| ())?;
    fs::write(root.join("weather.json"), body).map_err(|_| ())
}
#[derive(Serialize, Deserialize)]
struct CachedLocation {
    saved_at: DateTime<Utc>,
    location: GeoLocation,
}
fn location_key(query: &str) -> String {
    query
        .bytes()
        .map(|byte| {
            if byte.is_ascii_alphanumeric() {
                byte as char
            } else {
                '_'
            }
        })
        .collect()
}
fn load_location_cache(config: &SystemServicesConfig, query: &str) -> Option<GeoLocation> {
    let contents = fs::read_to_string(
        cache_root(config)?.join(format!("location-{}.json", location_key(query))),
    )
    .ok()?;
    let cache: CachedLocation = serde_json::from_str(&contents).ok()?;
    (Utc::now() - cache.saved_at)
        .to_std()
        .ok()?
        .lt(&config.location_refresh_interval)
        .then_some(cache.location)
}
fn save_location_cache(
    config: &SystemServicesConfig,
    query: &str,
    location: &GeoLocation,
) -> Result<(), ()> {
    let root = cache_root(config).ok_or(())?;
    fs::create_dir_all(&root).map_err(|_| ())?;
    let body = serde_json::to_string(&CachedLocation {
        saved_at: Utc::now(),
        location: location.clone(),
    })
    .map_err(|_| ())?;
    fs::write(
        root.join(format!("location-{}.json", location_key(query))),
        body,
    )
    .map_err(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn mock_platform() -> platform::mock::MockPlatform {
        let root = std::env::temp_dir().join("system-services-platform-test");
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
        platform::mock::MockPlatform::new(user_dirs, app_paths)
    }

    struct ScriptedSlowMonitor {
        samples: VecDeque<platform::SlowSystemSample>,
    }

    impl platform::SystemMonitor for ScriptedSlowMonitor {
        fn sample_fast(&mut self) -> Result<platform::FastSystemSample, platform::PlatformError> {
            Err(platform::PlatformError::Unsupported {
                capability: "test.fast_system_sample",
            })
        }

        fn sample_slow(&mut self) -> Result<platform::SlowSystemSample, platform::PlatformError> {
            Ok(self.samples.pop_front().expect("scripted slow sample"))
        }
    }

    fn slow_sample(
        thermal: Result<Vec<platform::ThermalSensorSample>, &str>,
        batteries: Result<Vec<platform::BatterySample>, &str>,
    ) -> platform::SlowSystemSample {
        platform::SlowSystemSample {
            identity: Ok(platform::SystemIdentitySample {
                host_name: Some("host".into()),
                os_name: Some("os".into()),
                os_version: Some("1".into()),
                kernel_version: Some("1".into()),
            }),
            thermal: thermal.map_err(str::to_string),
            batteries: batteries.map_err(str::to_string),
            top_cpu: Vec::new(),
            top_memory: Vec::new(),
        }
    }

    fn metrics_channel() -> (
        watch::Sender<SystemSnapshot>,
        watch::Receiver<SystemSnapshot>,
    ) {
        let observed_at = Utc::now();
        watch::channel(SystemSnapshot {
            revision: 0,
            observed_at,
            weather: WeatherState::Loading,
            time: TimeState::Local {
                local_time: observed_at.fixed_offset(),
            },
            storage: StorageState::Loading,
            network: NetworkState::Loading,
            metrics: SystemMetricsSnapshot::loading(),
        })
    }

    #[test]
    fn thermal_inner_failures_retain_last_good_and_recover_independently() {
        let first_good = vec![platform::ThermalSensorSample {
            label: "CPU".into(),
            temperature_celsius: 42.0,
            critical_celsius: Some(100.0),
        }];
        let recovered = vec![platform::ThermalSensorSample {
            label: "CPU".into(),
            temperature_celsius: 39.0,
            critical_celsius: Some(100.0),
        }];
        let battery = vec![platform::BatterySample {
            vendor: Some("Vendor".into()),
            model: Some("Model".into()),
            state: platform::BatterySampleState::Charging,
            charge_percent: 50.0,
            energy_wh: 20.0,
            energy_full_wh: 40.0,
            time_to_empty_seconds: None,
            time_to_full_seconds: Some(600),
        }];
        let samples = VecDeque::from([
            slow_sample(Err("thermal unavailable"), Ok(battery.clone())),
            slow_sample(Ok(first_good.clone()), Ok(battery.clone())),
            slow_sample(Err("thermal read failed"), Ok(battery.clone())),
            slow_sample(Err("thermal retry failed"), Ok(battery.clone())),
            slow_sample(Ok(recovered.clone()), Ok(battery)),
        ]);
        let mut monitor: Result<Box<dyn platform::SystemMonitor>, _> =
            Ok(Box::new(ScriptedSlowMonitor { samples }));
        let (sender, receiver) = metrics_channel();

        refresh_slow_metrics(&sender, &mut monitor);
        assert!(
            matches!(receiver.borrow().metrics.thermal, MetricState::Unavailable { ref reason } if reason == "thermal unavailable")
        );
        assert!(matches!(
            receiver.borrow().metrics.batteries,
            MetricState::Ready(_)
        ));
        refresh_slow_metrics(&sender, &mut monitor);
        let expected_first = vec![ThermalSensorSnapshot {
            label: "CPU".into(),
            temperature_celsius: 42.0,
            critical_celsius: Some(100.0),
        }];
        assert_eq!(
            receiver.borrow().metrics.thermal,
            MetricState::Ready(expected_first.clone())
        );
        refresh_slow_metrics(&sender, &mut monitor);
        assert_eq!(
            receiver.borrow().metrics.thermal,
            MetricState::Stale {
                last_good: expected_first.clone(),
                error: "thermal read failed".into()
            }
        );
        assert!(matches!(
            receiver.borrow().metrics.batteries,
            MetricState::Ready(_)
        ));
        refresh_slow_metrics(&sender, &mut monitor);
        assert_eq!(
            receiver.borrow().metrics.thermal,
            MetricState::Stale {
                last_good: expected_first,
                error: "thermal retry failed".into()
            }
        );
        refresh_slow_metrics(&sender, &mut monitor);
        assert!(
            matches!(receiver.borrow().metrics.thermal, MetricState::Ready(ref values) if values[0].temperature_celsius == 39.0)
        );
    }

    #[test]
    fn battery_inner_failures_retain_last_good_and_recover_independently() {
        let thermal = vec![platform::ThermalSensorSample {
            label: "CPU".into(),
            temperature_celsius: 42.0,
            critical_celsius: None,
        }];
        let battery = |charge_percent| platform::BatterySample {
            vendor: Some("Vendor".into()),
            model: Some("Model".into()),
            state: platform::BatterySampleState::Discharging,
            charge_percent,
            energy_wh: 20.0,
            energy_full_wh: 40.0,
            time_to_empty_seconds: Some(900),
            time_to_full_seconds: None,
        };
        let samples = VecDeque::from([
            slow_sample(Ok(thermal.clone()), Err("battery unavailable")),
            slow_sample(Ok(thermal.clone()), Ok(vec![battery(50.0)])),
            slow_sample(Ok(thermal.clone()), Err("battery read failed")),
            slow_sample(Ok(thermal.clone()), Err("battery retry failed")),
            slow_sample(Ok(thermal), Ok(vec![battery(75.0)])),
        ]);
        let mut monitor: Result<Box<dyn platform::SystemMonitor>, _> =
            Ok(Box::new(ScriptedSlowMonitor { samples }));
        let (sender, receiver) = metrics_channel();

        refresh_slow_metrics(&sender, &mut monitor);
        assert!(
            matches!(receiver.borrow().metrics.batteries, MetricState::Unavailable { ref reason } if reason == "battery unavailable")
        );
        assert!(matches!(
            receiver.borrow().metrics.thermal,
            MetricState::Ready(_)
        ));
        refresh_slow_metrics(&sender, &mut monitor);
        let expected_first = match receiver.borrow().metrics.batteries.clone() {
            MetricState::Ready(values) => values,
            state => panic!("expected ready battery state, got {state:?}"),
        };
        refresh_slow_metrics(&sender, &mut monitor);
        assert_eq!(
            receiver.borrow().metrics.batteries,
            MetricState::Stale {
                last_good: expected_first.clone(),
                error: "battery read failed".into()
            }
        );
        assert!(matches!(
            receiver.borrow().metrics.thermal,
            MetricState::Ready(_)
        ));
        refresh_slow_metrics(&sender, &mut monitor);
        assert_eq!(
            receiver.borrow().metrics.batteries,
            MetricState::Stale {
                last_good: expected_first,
                error: "battery retry failed".into()
            }
        );
        refresh_slow_metrics(&sender, &mut monitor);
        assert!(
            matches!(receiver.borrow().metrics.batteries, MetricState::Ready(ref values) if values[0].charge_percent == 75.0)
        );
    }

    #[test]
    fn system_status_failures_are_independent_stale_and_recoverable() {
        let platform = mock_platform();
        platform.set_local_volumes_result(Ok(vec![platform::LocalVolume {
            root: PathBuf::from("/"),
            label: None,
            kind: platform::VolumeKind::Fixed,
            total_bytes: Some(1_000),
            available_bytes: Some(500),
            is_system: true,
            access: platform::VolumeAccess::ReadWrite,
        }]));
        let observed_at = Utc::now();
        let (sender, receiver) = watch::channel(SystemSnapshot {
            revision: 0,
            observed_at,
            weather: WeatherState::Loading,
            time: TimeState::Local {
                local_time: observed_at.fixed_offset(),
            },
            storage: StorageState::Loading,
            network: NetworkState::Loading,
            metrics: SystemMetricsSnapshot::loading(),
        });
        let thresholds = SystemServicesConfig::default().storage_thresholds;
        refresh_system_status(&sender, &platform, thresholds);
        assert!(matches!(receiver.borrow().storage, StorageState::Ready(_)));
        assert!(matches!(receiver.borrow().network, NetworkState::Ready(_)));

        platform.set_local_volumes_result(Err(platform::PlatformError::Unsupported {
            capability: "local_volumes",
        }));
        platform.set_network_status_result(Ok(platform::NetworkStatus::default()));
        refresh_system_status(&sender, &platform, thresholds);
        assert!(matches!(
            receiver.borrow().storage,
            StorageState::Stale { .. }
        ));
        assert!(matches!(receiver.borrow().network, NetworkState::Ready(_)));

        platform.set_local_volumes_result(Ok(Vec::new()));
        platform.set_network_status_result(Err(platform::PlatformError::Unsupported {
            capability: "network_status",
        }));
        refresh_system_status(&sender, &platform, thresholds);
        assert!(matches!(receiver.borrow().storage, StorageState::Ready(_)));
        assert!(matches!(
            receiver.borrow().network,
            NetworkState::Stale { .. }
        ));
        assert!(
            platform
                .calls()
                .iter()
                .filter(|call| matches!(call, platform::mock::MockCall::LocalVolumes))
                .count()
                >= 3
        );
    }

    #[test]
    fn platform_storage_mapping_preserves_rows_pressure_and_fixed_fallback() {
        let thresholds = StorageThresholds {
            low_available_bytes: 500,
            low_percentage: 20,
            critical_available_bytes: 100,
            critical_percentage: 5,
        };
        let mapped = map_storage(
            vec![
                platform::LocalVolume {
                    root: PathBuf::from("/fixed"),
                    label: Some("Fixed".to_string()),
                    kind: platform::VolumeKind::Fixed,
                    total_bytes: Some(10_000),
                    available_bytes: Some(1_000),
                    is_system: false,
                    access: platform::VolumeAccess::ReadWrite,
                },
                platform::LocalVolume {
                    root: PathBuf::from("/media/removable"),
                    label: None,
                    kind: platform::VolumeKind::Removable,
                    total_bytes: Some(1_000),
                    available_bytes: Some(50),
                    is_system: false,
                    access: platform::VolumeAccess::ReadOnly,
                },
            ],
            thresholds,
        );
        assert_eq!(mapped.system_volume_index, Some(0));
        assert_eq!(
            mapped.system_volume_source,
            SystemVolumeSource::FixedVolumeFallback
        );
        assert!(!mapped.volumes[0].is_system);
        assert_eq!(mapped.overall_pressure, StoragePressure::Critical);
    }

    #[test]
    fn platform_network_mapping_keeps_virtual_rows_but_excludes_them_from_active_count() {
        let mapped = map_network(platform::NetworkStatus::new(vec![
            platform::NetworkInterface {
                name: "eth0".to_string(),
                display_name: None,
                kind: platform::NetworkInterfaceKind::Wired,
                link_state: platform::NetworkLinkState::Up,
                addresses: vec!["192.0.2.1".parse().unwrap()],
            },
            platform::NetworkInterface {
                name: "vm0".to_string(),
                display_name: Some("Virtual".to_string()),
                kind: platform::NetworkInterfaceKind::Virtual,
                link_state: platform::NetworkLinkState::Up,
                addresses: vec!["2001:db8::1".parse().unwrap()],
            },
        ]));
        assert_eq!(mapped.interfaces.len(), 2);
        assert_eq!(mapped.active_link_count, 1);
        assert!(mapped.has_active_link);
        assert_eq!(mapped.interfaces[1].addresses, ["2001:db8::1"]);
    }

    struct FakeProvider {
        outcomes: Mutex<Vec<Result<WeatherData, String>>>,
        calls: AtomicUsize,
    }

    struct GatedProvider {
        started: std_mpsc::Sender<()>,
        release: tokio::sync::Notify,
    }

    #[async_trait]
    impl WeatherProvider for GatedProvider {
        async fn current_weather(
            &self,
            _location: WeatherLocation,
            _units: WeatherUnits,
        ) -> Result<WeatherData, String> {
            let _ = self.started.send(());
            self.release.notified().await;
            Ok(sample())
        }
    }

    struct CountingMonitor {
        fast_calls: Arc<AtomicUsize>,
        slow_calls: Arc<AtomicUsize>,
    }

    impl platform::SystemMonitor for CountingMonitor {
        fn sample_fast(&mut self) -> Result<platform::FastSystemSample, platform::PlatformError> {
            self.fast_calls.fetch_add(1, Ordering::SeqCst);
            Ok(platform::FastSystemSample {
                cpu: platform::CpuSample {
                    usage_percent: 1.0,
                    per_core_percent: vec![1.0],
                    logical_core_count: 1,
                    physical_core_count: Some(1),
                },
                memory: platform::MemorySample {
                    total_bytes: 1,
                    used_bytes: 0,
                    available_bytes: 1,
                    swap_total_bytes: 0,
                    swap_used_bytes: 0,
                },
                uptime_seconds: 1,
                load: platform::LoadSample {
                    supported: true,
                    one: 0.0,
                    five: 0.0,
                    fifteen: 0.0,
                },
                network_interfaces: Vec::new(),
            })
        }

        fn sample_slow(&mut self) -> Result<platform::SlowSystemSample, platform::PlatformError> {
            self.slow_calls.fetch_add(1, Ordering::SeqCst);
            Ok(platform::SlowSystemSample {
                identity: Ok(platform::SystemIdentitySample {
                    host_name: Some("host".into()),
                    os_name: Some("os".into()),
                    os_version: Some("1".into()),
                    kernel_version: Some("1".into()),
                }),
                thermal: Err("no thermal sensors".into()),
                batteries: Err("no batteries".into()),
                top_cpu: Vec::new(),
                top_memory: Vec::new(),
            })
        }
    }
    #[async_trait]
    impl WeatherProvider for FakeProvider {
        async fn current_weather(
            &self,
            _location: WeatherLocation,
            _units: WeatherUnits,
        ) -> Result<WeatherData, String> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.outcomes.lock().unwrap().remove(0)
        }
    }

    struct FakeSystemLocationDetector {
        outcomes: Mutex<Vec<Option<GeoLocation>>>,
        calls: AtomicUsize,
    }

    #[async_trait]
    impl SystemLocationDetector for FakeSystemLocationDetector {
        async fn detect(&self) -> Option<GeoLocation> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.outcomes.lock().unwrap().remove(0)
        }
    }

    struct PendingSystemLocationDetector;

    #[async_trait]
    impl SystemLocationDetector for PendingSystemLocationDetector {
        async fn detect(&self) -> Option<GeoLocation> {
            std::future::pending().await
        }
    }
    fn sample() -> WeatherData {
        WeatherData {
            condition: WeatherCondition::Clear,
            temperature: 20.0,
            precipitation: 0.0,
            wind_speed: 1.0,
            wind_direction: 0.0,
            sun: CelestialEvents::from_bool(true),
            moon_phase: None,
            timestamp: "now".to_string(),
            attribution: String::new(),
        }
    }
    fn watchdog() -> AppWatchdog {
        let config = watchdog::WatchdogConfig::new(
            std::env::temp_dir().join("ss-reports"),
            std::env::temp_dir().join("ss-fallback"),
            std::env::temp_dir().join("ss-data"),
            "ss-test",
            "1",
        )
        .with_unclean_exit_tracking(false);
        let (_runtime, process) = watchdog::WatchdogRuntime::start_isolated(config).unwrap();
        process
            .register_app(watchdog::AppDescriptor::new(
                watchdog::AppId::from_static("system-services-test"),
                "test",
                "1",
                watchdog::AppCriticality::SessionCritical,
            ))
            .unwrap()
    }
    fn config() -> SystemServicesConfig {
        SystemServicesConfig {
            time_sync_mode: TimeSyncMode::OperatingSystem,
            weather_refresh_interval: Duration::from_secs(3600),
            cache_dir: Some(tempfile::tempdir().unwrap().keep()),
            ..SystemServicesConfig::default()
        }
    }

    #[test]
    fn network_time_failure_publishes_degraded_state_and_shutdown_completes() {
        let (result_tx, result_rx) = std_mpsc::channel();
        std::thread::spawn(move || {
            let result = (|| -> Result<(), String> {
                let mut runtime_config = config();
                runtime_config.time_sync_mode = TimeSyncMode::Network;
                runtime_config.time_server_url = Some("not a valid URL".to_string());
                runtime_config.request_timeout = Duration::from_millis(100);
                runtime_config.timezone_location = Some(runtime_config.fallback_location.clone());
                let provider = Arc::new(FakeProvider {
                    outcomes: Mutex::new(vec![Err("weather unavailable".to_string())]),
                    calls: AtomicUsize::new(0),
                });
                let (handle, mut receiver) = SystemServicesRuntime::start_with_provider(
                    runtime_config,
                    watchdog(),
                    provider,
                );
                let wait_runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_time()
                    .build()
                    .map_err(|error| error.to_string())?;
                let observed = wait_runtime.block_on(async {
                    tokio::time::timeout(Duration::from_secs(3), async {
                        loop {
                            let snapshot = receiver.borrow_and_update();
                            if snapshot.revision > 0
                                && matches!(snapshot.time, TimeState::Degraded { .. })
                            {
                                return Ok::<(), watch::error::RecvError>(());
                            }
                            drop(snapshot);
                            receiver.changed().await?;
                        }
                    })
                    .await
                });
                observed
                    .map_err(|_| "degraded time snapshot timed out".to_string())?
                    .map_err(|error| error.to_string())?;
                handle.shutdown().map_err(|error| error.to_string())
            })();
            let _ = result_tx.send(result);
        });

        let result = result_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("time failure lifecycle must finish within five seconds");
        assert_eq!(result, Ok(()));
    }
    fn wait_until(
        receiver: &mut watch::Receiver<SystemSnapshot>,
        predicate: impl Fn(&SystemSnapshot) -> bool,
    ) {
        for _ in 0..200 {
            if predicate(&receiver.borrow()) {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("snapshot predicate was not reached")
    }

    async fn wait_for_snapshot_event(
        receiver: &mut watch::Receiver<SystemSnapshot>,
        predicate: impl Fn(&SystemSnapshot) -> bool,
    ) {
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if predicate(&receiver.borrow_and_update()) {
                    return;
                }
                receiver
                    .changed()
                    .await
                    .expect("system service snapshot sender must remain open");
            }
        })
        .await
        .expect("system service snapshot condition timed out");
    }
    fn status_call_count(platform: &platform::mock::MockPlatform) -> usize {
        platform
            .calls()
            .iter()
            .filter(|call| matches!(call, platform::mock::MockCall::LocalVolumes))
            .count()
    }

    fn network_status_call_count(platform: &platform::mock::MockPlatform) -> usize {
        platform
            .calls()
            .iter()
            .filter(|call| matches!(call, platform::mock::MockCall::NetworkStatus))
            .count()
    }

    fn start_with_zero_status_intervals() -> (
        SystemServicesHandle,
        watch::Receiver<SystemSnapshot>,
        Arc<platform::mock::MockPlatform>,
    ) {
        let platform = Arc::new(mock_platform());
        let provider = Arc::new(FakeProvider {
            outcomes: Mutex::new(vec![Ok(sample()), Ok(sample())]),
            calls: AtomicUsize::new(0),
        });
        let mut config = config();
        config.timezone_location = Some(config.fallback_location.clone());
        config.system_status_background_refresh_interval = Duration::ZERO;
        config.system_status_active_refresh_interval = Duration::ZERO;
        config.system_status_active_slow_refresh_interval = Duration::ZERO;
        let platform_trait: Arc<dyn platform::Platform> = platform.clone();
        let (handle, receiver) = SystemServicesRuntime::start_with_platform_and_provider(
            config,
            watchdog(),
            platform_trait,
            provider,
        );
        (handle, receiver, platform)
    }

    #[test]
    fn zero_background_interval_is_bounded_and_commands_remain_responsive() {
        let (handle, mut receiver, platform) = start_with_zero_status_intervals();
        wait_until(&mut receiver, |snapshot| {
            matches!(snapshot.storage, StorageState::Ready(_))
        });
        std::thread::sleep(Duration::from_millis(75));
        let storage_calls = status_call_count(&platform);
        let network_calls = network_status_call_count(&platform);
        assert!(
            (1..=16).contains(&storage_calls),
            "storage calls: {storage_calls}"
        );
        assert!(
            (1..=16).contains(&network_calls),
            "network calls: {network_calls}"
        );

        handle.refresh_system_status().unwrap();
        wait_until(&mut receiver, |_| {
            status_call_count(&platform) > storage_calls
        });
        let after_refresh = status_call_count(&platform);
        let mut changed = config();
        changed.timezone_location = Some(changed.fallback_location.clone());
        changed.system_status_background_refresh_interval = Duration::ZERO;
        changed.system_status_active_refresh_interval = Duration::ZERO;
        changed.system_status_active_slow_refresh_interval = Duration::ZERO;
        handle.reconfigure(changed).unwrap();
        wait_until(&mut receiver, |_| {
            status_call_count(&platform) > after_refresh
        });

        let (shutdown_tx, shutdown_rx) = std_mpsc::channel();
        std::thread::spawn(move || {
            let _ = shutdown_tx.send(handle.shutdown());
        });
        assert_eq!(
            shutdown_rx.recv_timeout(Duration::from_secs(1)).unwrap(),
            Ok(())
        );
    }

    #[test]
    fn zero_active_interval_is_bounded() {
        let (handle, mut receiver, platform) = start_with_zero_status_intervals();
        wait_until(&mut receiver, |snapshot| {
            matches!(snapshot.network, NetworkState::Ready(_))
        });
        handle.set_system_status_active(true).unwrap();
        let storage_baseline = status_call_count(&platform);
        let network_baseline = network_status_call_count(&platform);
        std::thread::sleep(Duration::from_millis(75));
        let storage_calls = status_call_count(&platform) - storage_baseline;
        let network_calls = network_status_call_count(&platform) - network_baseline;
        assert!(
            (1..=16).contains(&storage_calls),
            "storage calls: {storage_calls}"
        );
        assert!(
            (1..=16).contains(&network_calls),
            "network calls: {network_calls}"
        );
        handle.shutdown().unwrap();
    }

    #[test]
    fn injected_platform_refreshes_immediately_manually_and_at_active_interval() {
        let platform = Arc::new(mock_platform());
        let provider = Arc::new(FakeProvider {
            outcomes: Mutex::new(vec![Ok(sample())]),
            calls: AtomicUsize::new(0),
        });
        let mut config = config();
        config.timezone_location = Some(config.fallback_location.clone());
        config.system_status_background_refresh_interval = Duration::from_secs(60);
        config.system_status_active_refresh_interval = Duration::from_millis(30);
        config.system_status_active_slow_refresh_interval = Duration::from_millis(30);
        let platform_trait: Arc<dyn platform::Platform> = platform.clone();
        let (handle, mut receiver) = SystemServicesRuntime::start_with_platform_and_provider(
            config,
            watchdog(),
            platform_trait,
            provider,
        );
        wait_until(&mut receiver, |snapshot| {
            matches!(snapshot.storage, StorageState::Ready(_))
                && matches!(snapshot.network, NetworkState::Ready(_))
        });
        let initial_calls = status_call_count(&platform);
        handle.refresh_system_status().unwrap();
        wait_until(&mut receiver, |_| {
            status_call_count(&platform) > initial_calls
        });
        let manual_calls = status_call_count(&platform);
        handle.set_system_status_active(true).unwrap();
        wait_until(&mut receiver, |_| {
            status_call_count(&platform) >= manual_calls + 2
        });
        handle.set_system_status_active(false).unwrap();
        let before_background = status_call_count(&platform);
        wait_until(&mut receiver, |_| {
            status_call_count(&platform) > before_background
        });
        handle.shutdown().unwrap();
    }

    #[test]
    fn active_metrics_progress_while_weather_provider_is_pending() {
        let platform = Arc::new(mock_platform());
        let fast_calls = Arc::new(AtomicUsize::new(0));
        let slow_calls = Arc::new(AtomicUsize::new(0));
        platform.set_system_monitor_result(Ok(Box::new(CountingMonitor {
            fast_calls: fast_calls.clone(),
            slow_calls: slow_calls.clone(),
        })));
        let (started_tx, started_rx) = std_mpsc::channel();
        let provider = Arc::new(GatedProvider {
            started: started_tx,
            release: tokio::sync::Notify::new(),
        });
        let mut runtime_config = config();
        runtime_config.timezone_location = Some(runtime_config.fallback_location.clone());
        runtime_config.request_timeout = Duration::from_secs(5);
        runtime_config.system_status_background_refresh_interval = Duration::from_secs(60);
        runtime_config.system_status_active_refresh_interval = Duration::from_millis(20);
        runtime_config.system_status_active_slow_refresh_interval = Duration::from_millis(40);
        let platform_trait: Arc<dyn platform::Platform> = platform.clone();
        let (handle, _receiver) = SystemServicesRuntime::start_with_platform_and_provider(
            runtime_config.clone(),
            watchdog(),
            platform_trait,
            provider,
        );

        started_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("weather provider must enter its initial pending operation");
        handle.set_system_status_active(true).unwrap();
        started_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("activation must restart the pending weather operation");
        let deadline = std::time::Instant::now() + Duration::from_secs(1);
        while (fast_calls.load(Ordering::SeqCst) < 3
            || slow_calls.load(Ordering::SeqCst) < 2
            || status_call_count(&platform) < 2)
            && std::time::Instant::now() < deadline
        {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(fast_calls.load(Ordering::SeqCst) >= 3);
        assert!(slow_calls.load(Ordering::SeqCst) >= 2);
        assert!(status_call_count(&platform) >= 2);

        handle.reconfigure(runtime_config).unwrap();
        started_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("reconfigure must cancel and restart pending weather promptly");
        let (shutdown_tx, shutdown_rx) = std_mpsc::channel();
        std::thread::spawn(move || {
            let _ = shutdown_tx.send(handle.shutdown());
        });
        assert_eq!(
            shutdown_rx.recv_timeout(Duration::from_secs(1)).unwrap(),
            Ok(())
        );
    }
    #[tokio::test]
    async fn ready_stale_unavailable_and_manual_refresh_are_published() {
        let provider = Arc::new(FakeProvider {
            outcomes: Mutex::new(vec![
                Ok(sample()),
                Err("offline".to_string()),
                Err("offline".to_string()),
            ]),
            calls: AtomicUsize::new(0),
        });
        let platform: Arc<dyn platform::Platform> = Arc::new(mock_platform());
        let mut runtime_config = config();
        runtime_config.timezone_location = Some(runtime_config.fallback_location.clone());
        let (handle, mut receiver) = SystemServicesRuntime::start_with_platform_and_provider(
            runtime_config,
            watchdog(),
            platform,
            provider,
        );
        wait_for_snapshot_event(&mut receiver, |snapshot| {
            matches!(snapshot.weather, WeatherState::Ready(_))
        })
        .await;
        handle.refresh_weather().unwrap();
        wait_for_snapshot_event(&mut receiver, |snapshot| {
            matches!(snapshot.weather, WeatherState::Stale { .. })
        })
        .await;
        handle.shutdown().unwrap();
        let unavailable = Arc::new(FakeProvider {
            outcomes: Mutex::new(vec![Err("offline".to_string())]),
            calls: AtomicUsize::new(0),
        });
        let platform: Arc<dyn platform::Platform> = Arc::new(mock_platform());
        let mut runtime_config = config();
        runtime_config.timezone_location = Some(runtime_config.fallback_location.clone());
        let (handle, mut receiver) = SystemServicesRuntime::start_with_platform_and_provider(
            runtime_config,
            watchdog(),
            platform,
            unavailable,
        );
        wait_for_snapshot_event(&mut receiver, |snapshot| {
            matches!(snapshot.weather, WeatherState::Unavailable { .. })
        })
        .await;
        handle.shutdown().unwrap();
    }
    #[tokio::test]
    async fn reconfiguration_and_shutdown_are_controllable() {
        let provider = Arc::new(FakeProvider {
            outcomes: Mutex::new(vec![Ok(sample()), Ok(sample())]),
            calls: AtomicUsize::new(0),
        });
        let platform: Arc<dyn platform::Platform> = Arc::new(mock_platform());
        let mut runtime_config = config();
        runtime_config.timezone_location = Some(runtime_config.fallback_location.clone());
        let (handle, mut receiver) = SystemServicesRuntime::start_with_platform_and_provider(
            runtime_config,
            watchdog(),
            platform,
            provider.clone(),
        );
        wait_for_snapshot_event(&mut receiver, |snapshot| {
            matches!(snapshot.weather, WeatherState::Ready(_))
        })
        .await;
        let mut changed = config();
        changed.timezone_id = "Asia/Shanghai".to_string();
        changed.timezone_location = Some(changed.fallback_location.clone());
        handle.reconfigure(changed).unwrap();
        wait_for_snapshot_event(&mut receiver, |_| {
            provider.calls.load(Ordering::SeqCst) >= 2
        })
        .await;
        handle.shutdown().unwrap();
        assert!(matches!(
            handle.refresh_weather(),
            Err(SystemServicesError::Shutdown)
        ));
    }

    #[test]
    fn wmo_codes_match_mature_normalizer() {
        let cases = [
            (0, WeatherCondition::Clear),
            (1, WeatherCondition::PartlyCloudy),
            (2, WeatherCondition::PartlyCloudy),
            (3, WeatherCondition::Overcast),
            (45, WeatherCondition::Fog),
            (48, WeatherCondition::Fog),
            (51, WeatherCondition::Drizzle),
            (53, WeatherCondition::Drizzle),
            (55, WeatherCondition::Drizzle),
            (56, WeatherCondition::FreezingRain),
            (57, WeatherCondition::FreezingRain),
            (61, WeatherCondition::Rain),
            (63, WeatherCondition::Rain),
            (65, WeatherCondition::Rain),
            (66, WeatherCondition::FreezingRain),
            (67, WeatherCondition::FreezingRain),
            (71, WeatherCondition::Snow),
            (73, WeatherCondition::Snow),
            (75, WeatherCondition::Snow),
            (77, WeatherCondition::SnowGrains),
            (80, WeatherCondition::RainShowers),
            (81, WeatherCondition::RainShowers),
            (82, WeatherCondition::RainShowers),
            (85, WeatherCondition::SnowShowers),
            (86, WeatherCondition::SnowShowers),
            (95, WeatherCondition::Thunderstorm),
            (96, WeatherCondition::ThunderstormHail),
            (99, WeatherCondition::ThunderstormHail),
            (-1, WeatherCondition::Clear),
        ];
        for (code, expected) in cases {
            assert_eq!(normalize_open_meteo_code(code), expected, "code {code}");
        }
    }

    #[test]
    fn met_office_codes_use_the_provider_specific_normalizer() {
        let cases = [
            (0, WeatherCondition::Clear),
            (1, WeatherCondition::Clear),
            (2, WeatherCondition::PartlyCloudy),
            (3, WeatherCondition::PartlyCloudy),
            (5, WeatherCondition::Fog),
            (6, WeatherCondition::Fog),
            (7, WeatherCondition::Cloudy),
            (8, WeatherCondition::Overcast),
            (9, WeatherCondition::RainShowers),
            (10, WeatherCondition::RainShowers),
            (11, WeatherCondition::Drizzle),
            (12, WeatherCondition::Rain),
            (13, WeatherCondition::RainShowers),
            (14, WeatherCondition::RainShowers),
            (15, WeatherCondition::Rain),
            (16, WeatherCondition::SnowShowers),
            (17, WeatherCondition::SnowShowers),
            (18, WeatherCondition::SnowGrains),
            (19, WeatherCondition::ThunderstormHail),
            (20, WeatherCondition::ThunderstormHail),
            (21, WeatherCondition::ThunderstormHail),
            (22, WeatherCondition::SnowShowers),
            (23, WeatherCondition::SnowShowers),
            (24, WeatherCondition::Snow),
            (25, WeatherCondition::SnowShowers),
            (26, WeatherCondition::SnowShowers),
            (27, WeatherCondition::Snow),
            (28, WeatherCondition::Thunderstorm),
            (29, WeatherCondition::Thunderstorm),
            (30, WeatherCondition::Thunderstorm),
            (31, WeatherCondition::Thunderstorm),
            (4, WeatherCondition::Clear),
            (-1, WeatherCondition::Drizzle),
        ];
        for (code, expected) in cases {
            assert_eq!(normalize_met_office_code(code), expected, "code {code}");
        }
    }

    #[tokio::test]
    async fn weather_location_prioritizes_text_then_timezone_before_system_detection() {
        let configured = GeoLocation {
            latitude: 1.0,
            longitude: 2.0,
            city: Some("configured".into()),
        };
        let timezone = GeoLocation {
            latitude: 3.0,
            longitude: 4.0,
            city: Some("timezone".into()),
        };
        let detector = FakeSystemLocationDetector {
            outcomes: Mutex::new(vec![]),
            calls: AtomicUsize::new(0),
        };
        let mut text_config = config();
        text_config.weather_location = Some("configured city".into());
        text_config.timezone_location = Some(timezone.clone());
        save_location_cache(&text_config, "configured city", &configured).unwrap();
        let mut system_location = None;
        assert_eq!(
            resolve_location(&text_config, true, &mut system_location, &detector).await,
            configured
        );
        assert_eq!(detector.calls.load(Ordering::SeqCst), 0);

        let mut timezone_config = config();
        timezone_config.timezone_location = Some(timezone.clone());
        assert_eq!(
            resolve_location(&timezone_config, true, &mut system_location, &detector).await,
            timezone
        );
        assert_eq!(detector.calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn weather_location_uses_successful_system_detection() {
        let detected = GeoLocation {
            latitude: 51.5072,
            longitude: -0.1276,
            city: Some("London".into()),
        };
        let detector = FakeSystemLocationDetector {
            outcomes: Mutex::new(vec![Some(detected.clone())]),
            calls: AtomicUsize::new(0),
        };
        let mut system_location = None;
        assert_eq!(
            resolve_location(&config(), true, &mut system_location, &detector).await,
            detected
        );
        assert_eq!(detector.calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn weather_location_uses_default_after_system_detection_fails() {
        let mut config = config();
        config.fallback_location = GeoLocation {
            latitude: 31.2304,
            longitude: 121.4737,
            city: Some("Shanghai".into()),
        };
        let detector = FakeSystemLocationDetector {
            outcomes: Mutex::new(vec![None]),
            calls: AtomicUsize::new(0),
        };
        let mut system_location = None;
        assert_eq!(
            resolve_location(&config, true, &mut system_location, &detector).await,
            config.fallback_location
        );
        assert_eq!(detector.calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn system_location_detection_obeys_the_request_timeout() {
        let mut config = config();
        config.request_timeout = Duration::from_millis(10);
        let mut system_location = None;
        let started = Instant::now();
        let resolved = resolve_location(
            &config,
            true,
            &mut system_location,
            &PendingSystemLocationDetector,
        )
        .await;
        assert_eq!(resolved, config.fallback_location);
        assert!(started.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn monotonic_anchor_advances_and_error_remains_degraded() {
        let instant = Instant::now();
        let utc = DateTime::parse_from_rfc3339("2026-01-01T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let anchor = TimeAnchor {
            utc,
            sampled_at: utc,
            instant,
            source: TimeSource::Network("test".into()),
        };
        assert_eq!(
            anchor.utc_at(instant + Duration::from_secs(7)),
            utc + Duration::from_secs(7)
        );
        let state = current_time_state(
            &config(),
            Some(&anchor),
            Some("offline"),
            instant + Duration::from_secs(7),
        );
        assert!(
            matches!(state, TimeState::Degraded { last_sync: Some(value), ref error, .. } if value == utc && error == "offline")
        );
        let next = current_time_state(
            &config(),
            Some(&anchor),
            Some("offline"),
            instant + Duration::from_secs(8),
        );
        assert!(matches!(next, TimeState::Degraded { ref error, .. } if error == "offline"));
    }

    struct PendingProvider {
        entered: Mutex<Option<std_mpsc::Sender<()>>>,
    }
    #[async_trait]
    impl WeatherProvider for PendingProvider {
        async fn current_weather(
            &self,
            _: WeatherLocation,
            _: WeatherUnits,
        ) -> Result<WeatherData, String> {
            if let Some(entered) = self.entered.lock().unwrap().take() {
                let _ = entered.send(());
            }
            std::future::pending().await
        }
    }

    #[test]
    fn shutdown_and_reconfigure_cancel_pending_provider_wait() {
        let mut initial = config();
        initial.timezone_location = Some(initial.fallback_location.clone());
        let (entered_tx, entered_rx) = std_mpsc::channel();
        let (handle, _) = SystemServicesRuntime::start_with_provider(
            initial.clone(),
            watchdog(),
            Arc::new(PendingProvider {
                entered: Mutex::new(Some(entered_tx)),
            }),
        );
        entered_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("pending provider must be entered before reconfigure");
        let mut changed = initial;
        changed.timezone_id = "Asia/Shanghai".into();
        handle.reconfigure(changed).unwrap();
        let started = Instant::now();
        handle.shutdown().unwrap();
        assert!(started.elapsed() < Duration::from_secs(1));
    }
}
