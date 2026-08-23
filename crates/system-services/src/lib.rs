//! Process-wide weather and time services.
//!
//! The service owns network I/O, location resolution and its cache. UI crates
//! receive immutable snapshots through `watch` and therefore never need to
//! start a second weather or time worker.

use async_trait::async_trait;
use chrono::{DateTime, FixedOffset, NaiveTime, Utc};
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
/// Data format selected by the caller. Weather values in snapshots are
/// canonical (Celsius, m/s and mm); renderers can format them as desired.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct WeatherUnits {
    pub temperature: TemperatureUnit,
    pub wind_speed: WindSpeedUnit,
    pub precipitation: PrecipitationUnit,
}

impl Default for WeatherUnits {
    fn default() -> Self {
        Self {
            temperature: TemperatureUnit::Celsius,
            wind_speed: WindSpeedUnit::Kmh,
            precipitation: PrecipitationUnit::Mm,
        }
    }
}

impl WeatherUnits {
    pub const fn metric() -> Self {
        Self {
            temperature: TemperatureUnit::Celsius,
            wind_speed: WindSpeedUnit::Kmh,
            precipitation: PrecipitationUnit::Mm,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TemperatureUnit {
    Celsius,
    Fahrenheit,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WindSpeedUnit {
    Kmh,
    Ms,
    Mph,
    Kn,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PrecipitationUnit {
    Mm,
    Inch,
}

pub fn format_temperature(celsius: f64, unit: TemperatureUnit) -> (f64, &'static str) {
    match unit {
        TemperatureUnit::Celsius => (celsius, "°C"),
        TemperatureUnit::Fahrenheit => (celsius * 9.0 / 5.0 + 32.0, "°F"),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WeatherCondition {
    Clear,
    PartlyCloudy,
    Cloudy,
    Overcast,
    Fog,
    Drizzle,
    Rain,
    FreezingRain,
    Snow,
    SnowGrains,
    RainShowers,
    SnowShowers,
    Thunderstorm,
    ThunderstormHail,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RainIntensity {
    Drizzle,
    Light,
    Heavy,
    Storm,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SnowIntensity {
    Light,
    Medium,
    Heavy,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FogIntensity {
    Light,
    Medium,
    Heavy,
}

impl WeatherCondition {
    pub fn rain_intensity(&self) -> RainIntensity {
        match self {
            Self::Drizzle => RainIntensity::Drizzle,
            Self::FreezingRain | Self::Thunderstorm => RainIntensity::Heavy,
            Self::ThunderstormHail => RainIntensity::Storm,
            _ => RainIntensity::Light,
        }
    }
    pub fn snow_intensity(&self) -> SnowIntensity {
        match self {
            Self::SnowGrains => SnowIntensity::Light,
            Self::SnowShowers => SnowIntensity::Medium,
            Self::Snow => SnowIntensity::Heavy,
            _ => SnowIntensity::Light,
        }
    }
    pub fn fog_intensity(&self) -> FogIntensity {
        if matches!(self, Self::Fog) {
            FogIntensity::Medium
        } else {
            FogIntensity::Light
        }
    }
    pub fn is_raining(&self) -> bool {
        matches!(
            self,
            Self::Drizzle
                | Self::Rain
                | Self::RainShowers
                | Self::FreezingRain
                | Self::Thunderstorm
                | Self::ThunderstormHail
        )
    }
    pub fn is_snowing(&self) -> bool {
        matches!(self, Self::Snow | Self::SnowGrains | Self::SnowShowers)
    }
    pub fn is_thunderstorm(&self) -> bool {
        matches!(self, Self::Thunderstorm | Self::ThunderstormHail)
    }
    pub fn is_cloudy(&self) -> bool {
        matches!(self, Self::PartlyCloudy | Self::Cloudy | Self::Overcast)
    }
    pub fn is_foggy(&self) -> bool {
        matches!(self, Self::Fog)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CelestialEvents {
    pub is_day: bool,
    pub begin_twilight: Option<NaiveTime>,
    pub rise: Option<NaiveTime>,
    pub upper_transit: Option<NaiveTime>,
    pub set: Option<NaiveTime>,
    pub end_twilight: Option<NaiveTime>,
}
impl CelestialEvents {
    pub fn from_bool(is_day: bool) -> Self {
        Self {
            is_day,
            begin_twilight: None,
            rise: None,
            upper_transit: None,
            set: None,
            end_twilight: None,
        }
    }
    pub fn only_day(is_day: i32) -> Self {
        Self::from_bool(is_day == 1)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WeatherData {
    pub condition: WeatherCondition,
    pub temperature: f64,
    pub precipitation: f64,
    pub wind_speed: f64,
    pub wind_direction: f64,
    pub sun: CelestialEvents,
    pub moon_phase: Option<f64>,
    pub timestamp: String,
    pub attribution: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct WeatherLocation {
    pub latitude: f64,
    pub longitude: f64,
    pub elevation: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WeatherConditions {
    pub is_raining: bool,
    pub is_snowing: bool,
    pub is_thunderstorm: bool,
    pub is_cloudy: bool,
    pub is_foggy: bool,
    pub sun: CelestialEvents,
}
impl Default for WeatherConditions {
    fn default() -> Self {
        Self {
            is_raining: false,
            is_snowing: false,
            is_thunderstorm: false,
            is_cloudy: false,
            is_foggy: false,
            sun: CelestialEvents::from_bool(true),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GeoLocation {
    pub latitude: f64,
    pub longitude: f64,
    pub city: Option<String>,
}

// Const `Option<String>` is unavailable, so use an internal const adapter for
// the stable default and turn it into an owned value on use.
#[derive(Clone, Copy)]
enum SomeStaticStr {
    Shanghai,
}
impl SomeStaticStr {
    fn as_str(self) -> &'static str {
        match self {
            Self::Shanghai => "Shanghai",
        }
    }
}
struct ShanghaiDefault {
    latitude: f64,
    longitude: f64,
    city: SomeStaticStr,
}
const DEFAULT_LOCATION: ShanghaiDefault = ShanghaiDefault {
    latitude: 31.2304,
    longitude: 121.4737,
    city: SomeStaticStr::Shanghai,
};

impl GeoLocation {
    pub fn weather_location(&self) -> WeatherLocation {
        WeatherLocation {
            latitude: self.latitude,
            longitude: self.longitude,
            elevation: None,
        }
    }
    fn fallback() -> Self {
        Self {
            latitude: DEFAULT_LOCATION.latitude,
            longitude: DEFAULT_LOCATION.longitude,
            city: Some(DEFAULT_LOCATION.city.as_str().to_string()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WeatherSnapshot {
    pub weather: WeatherData,
    pub location: WeatherLocation,
    pub city: Option<String>,
    pub units: WeatherUnits,
    pub sampled_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum WeatherState {
    Loading,
    Ready(WeatherSnapshot),
    Stale {
        last_good: WeatherSnapshot,
        error: String,
    },
    Unavailable {
        reason: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TimeSource {
    OperatingSystem,
    Network(String),
}

pub type LocalTime = DateTime<FixedOffset>;

#[derive(Debug, Clone, PartialEq)]
pub enum TimeState {
    Local {
        local_time: LocalTime,
    },
    Synced {
        utc: DateTime<Utc>,
        local_time: LocalTime,
        source: TimeSource,
        sampled_at: DateTime<Utc>,
    },
    Degraded {
        local_time: LocalTime,
        last_sync: Option<DateTime<Utc>>,
        error: String,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct SystemSnapshot {
    pub revision: u64,
    pub observed_at: DateTime<Utc>,
    pub weather: WeatherState,
    pub time: TimeState,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TimeSyncMode {
    OperatingSystem,
    Network,
}

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

pub fn normalize_open_meteo_code(code: i32) -> WeatherCondition {
    match code {
        0 => WeatherCondition::Clear,
        1 | 2 => WeatherCondition::PartlyCloudy,
        3 => WeatherCondition::Overcast,
        45 | 48 => WeatherCondition::Fog,
        51 | 53 | 55 => WeatherCondition::Drizzle,
        56 | 57 => WeatherCondition::FreezingRain,
        61 | 63 | 65 | 80 | 81 | 82 => WeatherCondition::Rain,
        66 | 67 => WeatherCondition::FreezingRain,
        71 | 73 | 75 | 77 | 85 | 86 => WeatherCondition::Snow,
        95 => WeatherCondition::Thunderstorm,
        96 | 99 => WeatherCondition::ThunderstormHail,
        _ => WeatherCondition::Cloudy,
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
        let (sender, receiver) = std_mpsc::channel();
        self.send(Command::Validate(config, sender))?;
        receiver
            .recv_timeout(Duration::from_secs(12))
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
                local_time: local_time(&config.timezone_id),
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
            current_time_state(&config, None, None),
        );
    }
    let mut weather_failures = 0usize;
    let mut time_failures = 0usize;
    let mut last_sync = None;
    loop {
        let tick = tokio::time::sleep(Duration::from_secs(1));
        tokio::pin!(tick);
        tokio::select! {
            _ = &mut tick => {},
            command = commands.recv() => match command {
                Some(Command::Shutdown) | None => break,
                Some(Command::Reconfigure(next)) => { config = next; weather_due = Instant::now(); time_due = Instant::now(); location_due = Instant::now(); },
                Some(Command::RefreshWeather) => weather_due = Instant::now(),
                Some(Command::SyncTime) => time_due = Instant::now(),
                Some(Command::Validate(candidate, sender)) => { let _ = sender.send(validate_time(&candidate).await); },
            },
        }
        let now = Instant::now();
        if now >= weather_due {
            let location = resolve_location(&config, location_due <= now).await;
            location_due = now + config.location_refresh_interval;
            match provider
                .current_weather(location.weather_location(), config.weather_units)
                .await
            {
                Ok(weather) => {
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
                        current_time_state(&config, last_sync.clone(), None),
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
                        current_time_state(&config, last_sync.clone(), None),
                    );
                    weather_due =
                        now + retry_delay(weather_failures, config.weather_refresh_interval);
                }
            }
        }
        if now >= time_due {
            match synchronize_time(&config).await {
                Ok((utc, source)) => {
                    last_sync = Some((utc, source));
                    time_failures = 0;
                    time_due = now + config.time_sync_interval;
                }
                Err(error) => {
                    time_failures += 1;
                    time_due = now + retry_delay(time_failures, config.time_sync_interval);
                    publish(
                        &snapshot_tx,
                        snapshot_tx.borrow().weather.clone(),
                        current_time_state(&config, last_sync.clone(), Some(error)),
                    );
                }
            }
        }
        let previous = snapshot_tx.borrow().clone();
        publish(
            &snapshot_tx,
            previous.weather,
            current_time_state(&config, last_sync.clone(), None),
        );
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
fn local_time(timezone: &str) -> LocalTime {
    let utc = Utc::now();
    parse_timezone(timezone)
        .map(|tz| utc.with_timezone(&tz).fixed_offset())
        .unwrap_or_else(|| utc.fixed_offset())
}
fn current_time_state(
    config: &SystemServicesConfig,
    last_sync: Option<(DateTime<Utc>, TimeSource)>,
    error: Option<String>,
) -> TimeState {
    let local = local_time(&config.timezone_id);
    if let Some(error) = error {
        return TimeState::Degraded {
            local_time: local,
            last_sync: last_sync.map(|(utc, _)| utc),
            error,
        };
    }
    match last_sync {
        Some((utc, source)) => TimeState::Synced {
            utc,
            local_time: utc
                .with_timezone(&parse_timezone(&config.timezone_id).unwrap_or(chrono_tz::UTC))
                .fixed_offset(),
            source,
            sampled_at: Utc::now(),
        },
        None => TimeState::Local { local_time: local },
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
        let mut config = SystemServicesConfig::default();
        config.time_sync_mode = TimeSyncMode::OperatingSystem;
        config.weather_refresh_interval = Duration::from_secs(3600);
        config.cache_dir = Some(tempfile::tempdir().unwrap().keep());
        config
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
}
