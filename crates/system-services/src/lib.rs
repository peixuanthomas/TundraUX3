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
            condition: normalize_open_meteo_code(current.weather_code),
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

enum Command {
    Reconfigure(SystemServicesConfig),
    RefreshWeather,
    SyncTime,
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
        let initial = snapshot(
            0,
            WeatherState::Loading,
            TimeState::Local {
                local_time: local_time_at(&config.timezone_id, Utc::now()),
            },
        );
        let (snapshot_tx, snapshot_rx) = watch::channel(initial);
        let (command_tx, command_rx) = mpsc::unbounded_channel();
        let tasks = watchdog.task_group("system-services");
        let mut worker_inputs = Some((config, provider, snapshot_tx, command_rx));
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
                    let (config, provider, snapshot_tx, command_rx) = worker_inputs
                        .take()
                        .expect("the non-restartable system services worker runs once");
                    let runtime = tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build();
                    if let Ok(runtime) = runtime {
                        runtime.block_on(run(config, provider, snapshot_tx, command_rx));
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
    provider: Arc<dyn WeatherProvider>,
    snapshot_tx: watch::Sender<SystemSnapshot>,
    mut commands: mpsc::UnboundedReceiver<Command>,
) {
    let mut weather_due = Instant::now();
    let mut time_due = Instant::now();
    let mut location_due = Instant::now();
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
    'main: loop {
        let tick = tokio::time::sleep(Duration::from_secs(1));
        tokio::pin!(tick);
        tokio::select! {
            _ = &mut tick => {},
            command = commands.recv() => if apply_command(command, &mut config, &mut weather_due, &mut time_due, &mut location_due, &mut pending_validation) { break },
        }
        let now = Instant::now();
        if let Some((candidate, sender)) = pending_validation.take() {
            let operation =
                tokio::time::timeout(candidate.request_timeout, validate_time(&candidate));
            tokio::pin!(operation);
            tokio::select! {
                result = &mut operation => {
                    let result = result.unwrap_or(Err(SystemServicesError::Timeout));
                    let _ = sender.send(result);
                }
                command = commands.recv() => {
                    let _ = sender.send(Err(SystemServicesError::Shutdown));
                    if apply_command(command, &mut config, &mut weather_due, &mut time_due, &mut location_due, &mut pending_validation) { break; }
                    continue 'main;
                }
            }
        }
        if now >= weather_due {
            let should_resolve = location_due <= now;
            let operation_config = config.clone();
            let operation = tokio::time::timeout(operation_config.request_timeout, async {
                let location = resolve_location(&operation_config, should_resolve).await;
                let weather = provider
                    .current_weather(location.weather_location(), operation_config.weather_units)
                    .await?;
                Ok::<_, String>((location, weather))
            });
            tokio::pin!(operation);
            let result = tokio::select! {
                result = &mut operation => result.map_err(|_| "weather request timed out".to_string()).and_then(|result| result),
                command = commands.recv() => {
                    if apply_command(command, &mut config, &mut weather_due, &mut time_due, &mut location_due, &mut pending_validation) { break; }
                    continue 'main;
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
            let result = tokio::select! {
                result = &mut operation => result.map_err(|_| "time request timed out".to_string()).and_then(|result| result),
                command = commands.recv() => {
                    if apply_command(command, &mut config, &mut weather_due, &mut time_due, &mut location_due, &mut pending_validation) { break; }
                    continue 'main;
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
                    publish(
                        &snapshot_tx,
                        snapshot_tx.borrow().weather.clone(),
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
    pending_validation: &mut Option<ValidationRequest>,
) -> bool {
    match command {
        Some(Command::Shutdown) | None => true,
        Some(Command::Reconfigure(next)) => {
            *config = next;
            *weather_due = Instant::now();
            *time_due = Instant::now();
            *location_due = Instant::now();
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
    let revision = sender.borrow().revision.saturating_add(1);
    let _ = sender.send(snapshot(revision, weather, time));
}
fn snapshot(revision: u64, weather: WeatherState, time: TimeState) -> SystemSnapshot {
    SystemSnapshot {
        revision,
        observed_at: Utc::now(),
        weather,
        time,
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

async fn resolve_location(config: &SystemServicesConfig, should_resolve_text: bool) -> GeoLocation {
    if let Some(query) = config
        .weather_location
        .as_deref()
        .map(str::trim)
        .filter(|query| !query.is_empty())
    {
        if let Some(cached) = load_location_cache(config, query) {
            return cached;
        }
        if should_resolve_text && let Some(resolved) = geocode(query).await {
            let _ = save_location_cache(config, query, &resolved);
            return resolved;
        }
    }
    config
        .timezone_location
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
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct FakeProvider {
        outcomes: Mutex<Vec<Result<WeatherData, String>>>,
        calls: AtomicUsize,
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
    #[test]
    fn ready_stale_unavailable_and_manual_refresh_are_published() {
        let provider = Arc::new(FakeProvider {
            outcomes: Mutex::new(vec![
                Ok(sample()),
                Err("offline".to_string()),
                Err("offline".to_string()),
            ]),
            calls: AtomicUsize::new(0),
        });
        let (handle, mut receiver) =
            SystemServicesRuntime::start_with_provider(config(), watchdog(), provider);
        wait_until(&mut receiver, |snapshot| {
            matches!(snapshot.weather, WeatherState::Ready(_))
        });
        handle.refresh_weather().unwrap();
        wait_until(&mut receiver, |snapshot| {
            matches!(snapshot.weather, WeatherState::Stale { .. })
        });
        handle.shutdown().unwrap();
        let unavailable = Arc::new(FakeProvider {
            outcomes: Mutex::new(vec![Err("offline".to_string())]),
            calls: AtomicUsize::new(0),
        });
        let (handle, mut receiver) =
            SystemServicesRuntime::start_with_provider(config(), watchdog(), unavailable);
        wait_until(&mut receiver, |snapshot| {
            matches!(snapshot.weather, WeatherState::Unavailable { .. })
        });
        handle.shutdown().unwrap();
    }
    #[test]
    fn reconfiguration_and_shutdown_are_controllable() {
        let provider = Arc::new(FakeProvider {
            outcomes: Mutex::new(vec![Ok(sample()), Ok(sample())]),
            calls: AtomicUsize::new(0),
        });
        let (handle, mut receiver) =
            SystemServicesRuntime::start_with_provider(config(), watchdog(), provider.clone());
        wait_until(&mut receiver, |snapshot| {
            matches!(snapshot.weather, WeatherState::Ready(_))
        });
        let mut changed = config();
        changed.timezone_id = "Asia/Shanghai".to_string();
        handle.reconfigure(changed).unwrap();
        wait_until(&mut receiver, |_| {
            provider.calls.load(Ordering::SeqCst) >= 2
        });
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

    struct PendingProvider;
    #[async_trait]
    impl WeatherProvider for PendingProvider {
        async fn current_weather(
            &self,
            _: WeatherLocation,
            _: WeatherUnits,
        ) -> Result<WeatherData, String> {
            std::future::pending().await
        }
    }

    #[test]
    fn shutdown_and_reconfigure_cancel_pending_provider_wait() {
        let (handle, _) = SystemServicesRuntime::start_with_provider(
            config(),
            watchdog(),
            Arc::new(PendingProvider),
        );
        std::thread::sleep(Duration::from_millis(50));
        let mut changed = config();
        changed.timezone_id = "Asia/Shanghai".into();
        handle.reconfigure(changed).unwrap();
        let started = Instant::now();
        handle.shutdown().unwrap();
        assert!(started.elapsed() < Duration::from_secs(1));
    }
}
