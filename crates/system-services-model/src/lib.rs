//! Pure snapshot and weather data shared by service producers and displays.

use chrono::{DateTime, FixedOffset, NaiveTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct WeatherUnits {
    pub temperature: TemperatureUnit,
    pub wind_speed: WindSpeedUnit,
    pub precipitation: PrecipitationUnit,
}
impl Default for WeatherUnits {
    fn default() -> Self {
        Self::metric()
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
impl GeoLocation {
    pub fn weather_location(&self) -> WeatherLocation {
        WeatherLocation {
            latitude: self.latitude,
            longitude: self.longitude,
            elevation: None,
        }
    }
    pub fn fallback() -> Self {
        Self {
            latitude: 31.2304,
            longitude: 121.4737,
            city: Some("Shanghai".to_string()),
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

/// Storage pressure ordered from least actionable to most severe.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum StoragePressure {
    Unknown,
    Normal,
    Low,
    Critical,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StorageThresholds {
    pub low_available_bytes: u64,
    pub low_percentage: u8,
    pub critical_available_bytes: u64,
    pub critical_percentage: u8,
}

impl StorageThresholds {
    pub fn classify(
        self,
        total_bytes: Option<u64>,
        available_bytes: Option<u64>,
    ) -> StoragePressure {
        let (Some(total), Some(available)) = (total_bytes, available_bytes) else {
            return StoragePressure::Unknown;
        };
        if total == 0 || available > total {
            return StoragePressure::Unknown;
        }

        let percentage_at_most = |threshold: u8| {
            u128::from(available) * 100 <= u128::from(total) * u128::from(threshold)
        };
        if available <= self.critical_available_bytes
            || percentage_at_most(self.critical_percentage)
        {
            StoragePressure::Critical
        } else if available <= self.low_available_bytes || percentage_at_most(self.low_percentage) {
            StoragePressure::Low
        } else {
            StoragePressure::Normal
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StorageVolumeKind {
    Fixed,
    Removable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StorageVolumeAccess {
    ReadWrite,
    ReadOnly,
    Unavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SystemVolumeSource {
    Detected,
    FixedVolumeFallback,
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StorageVolumeSnapshot {
    pub identifier: String,
    pub label: Option<String>,
    pub kind: StorageVolumeKind,
    pub is_system: bool,
    pub access: StorageVolumeAccess,
    pub total_bytes: Option<u64>,
    pub available_bytes: Option<u64>,
    pub pressure: StoragePressure,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StorageSnapshot {
    pub volumes: Vec<StorageVolumeSnapshot>,
    pub overall_pressure: StoragePressure,
    pub system_volume_index: Option<usize>,
    pub system_volume_source: SystemVolumeSource,
    pub sampled_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StorageState {
    Loading,
    Ready(StorageSnapshot),
    Stale {
        last_good: StorageSnapshot,
        error: String,
    },
    Unavailable {
        reason: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetworkInterfaceKind {
    Wired,
    Wireless,
    Virtual,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetworkLinkState {
    Up,
    Down,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetworkInterfaceSnapshot {
    pub name: String,
    pub display_name: Option<String>,
    pub kind: NetworkInterfaceKind,
    pub link_state: NetworkLinkState,
    pub addresses: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetworkSnapshot {
    pub interfaces: Vec<NetworkInterfaceSnapshot>,
    pub active_link_count: usize,
    pub has_active_link: bool,
    pub sampled_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NetworkState {
    Loading,
    Ready(NetworkSnapshot),
    Stale {
        last_good: NetworkSnapshot,
        error: String,
    },
    Unavailable {
        reason: String,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum MetricState<T> {
    Loading,
    Ready(T),
    Stale { last_good: T, error: String },
    Unavailable { reason: String },
}

#[derive(Debug, Clone, PartialEq)]
pub struct CpuSnapshot {
    pub usage_percent: f32,
    pub per_core_percent: Vec<f32>,
    pub logical_core_count: usize,
    pub physical_core_count: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemorySnapshot {
    pub total_bytes: u64,
    pub used_bytes: u64,
    pub available_bytes: u64,
    pub swap_total_bytes: u64,
    pub swap_used_bytes: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LoadSnapshot {
    pub one: f64,
    pub five: f64,
    pub fifteen: f64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UptimeSnapshot {
    pub seconds: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct NetworkIoInterfaceSnapshot {
    pub name: String,
    pub received_bytes: u64,
    pub transmitted_bytes: u64,
    pub received_bytes_per_second: f64,
    pub transmitted_bytes_per_second: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct NetworkIoSnapshot {
    pub interfaces: Vec<NetworkIoInterfaceSnapshot>,
    pub total_received_bytes: u64,
    pub total_transmitted_bytes: u64,
    pub total_received_bytes_per_second: f64,
    pub total_transmitted_bytes_per_second: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ThermalSensorSnapshot {
    pub label: String,
    pub temperature_celsius: f32,
    pub critical_celsius: Option<f32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BatteryState {
    Charging,
    Discharging,
    Full,
    Empty,
    Unknown,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BatterySnapshot {
    pub vendor: Option<String>,
    pub model: Option<String>,
    pub state: BatteryState,
    pub charge_percent: f32,
    pub energy_wh: f32,
    pub energy_full_wh: f32,
    pub time_to_empty_seconds: Option<u64>,
    pub time_to_full_seconds: Option<u64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProcessMetricSnapshot {
    pub pid: u32,
    pub name: String,
    pub cpu_percent: f32,
    pub memory_bytes: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProcessRankingsSnapshot {
    pub top_cpu: Vec<ProcessMetricSnapshot>,
    pub top_memory: Vec<ProcessMetricSnapshot>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SystemIdentitySnapshot {
    pub host_name: Option<String>,
    pub os_name: Option<String>,
    pub os_version: Option<String>,
    pub kernel_version: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SystemMetricsSnapshot {
    pub identity: MetricState<SystemIdentitySnapshot>,
    pub cpu: MetricState<CpuSnapshot>,
    pub memory: MetricState<MemorySnapshot>,
    pub load: MetricState<LoadSnapshot>,
    pub uptime: MetricState<UptimeSnapshot>,
    pub network_io: MetricState<NetworkIoSnapshot>,
    pub thermal: MetricState<Vec<ThermalSensorSnapshot>>,
    pub batteries: MetricState<Vec<BatterySnapshot>>,
    pub processes: MetricState<ProcessRankingsSnapshot>,
    pub sampled_at: DateTime<Utc>,
}

impl SystemMetricsSnapshot {
    pub fn loading() -> Self {
        Self {
            identity: MetricState::Loading,
            cpu: MetricState::Loading,
            memory: MetricState::Loading,
            load: MetricState::Loading,
            uptime: MetricState::Loading,
            network_io: MetricState::Loading,
            thermal: MetricState::Loading,
            batteries: MetricState::Loading,
            processes: MetricState::Loading,
            sampled_at: Utc::now(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct SystemSnapshot {
    pub revision: u64,
    pub observed_at: DateTime<Utc>,
    pub weather: WeatherState,
    pub time: TimeState,
    pub storage: StorageState,
    pub network: NetworkState,
    pub metrics: SystemMetricsSnapshot,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TimeSyncMode {
    OperatingSystem,
    Network,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn weather_helpers_preserve_precipitation_semantics() {
        assert!(WeatherCondition::RainShowers.is_raining());
        assert!(WeatherCondition::SnowGrains.is_snowing());
        assert_eq!(
            format_temperature(0.0, TemperatureUnit::Fahrenheit),
            (32.0, "°F")
        );
    }

    #[test]
    fn storage_pressure_classification_covers_boundaries_and_precedence() {
        let thresholds = StorageThresholds {
            low_available_bytes: 200,
            low_percentage: 20,
            critical_available_bytes: 100,
            critical_percentage: 5,
        };

        assert_eq!(
            thresholds.classify(Some(2_000), Some(100)),
            StoragePressure::Critical
        );
        assert_eq!(
            thresholds.classify(Some(3_000), Some(200)),
            StoragePressure::Low
        );
        assert_eq!(
            thresholds.classify(Some(2_000), Some(201)),
            StoragePressure::Low
        );
        assert_eq!(
            thresholds.classify(Some(1_000), Some(50)),
            StoragePressure::Critical
        );
        assert_eq!(
            thresholds.classify(Some(1_000), Some(300)),
            StoragePressure::Normal
        );
    }

    #[test]
    fn storage_pressure_rejects_unknown_and_invalid_capacities() {
        let thresholds = StorageThresholds {
            low_available_bytes: 1,
            low_percentage: 10,
            critical_available_bytes: 1,
            critical_percentage: 5,
        };
        assert_eq!(thresholds.classify(None, Some(1)), StoragePressure::Unknown);
        assert_eq!(thresholds.classify(Some(1), None), StoragePressure::Unknown);
        assert_eq!(
            thresholds.classify(Some(0), Some(0)),
            StoragePressure::Unknown
        );
        assert_eq!(
            thresholds.classify(Some(1), Some(2)),
            StoragePressure::Unknown
        );
    }

    #[test]
    fn storage_pressure_percentage_math_does_not_overflow() {
        let thresholds = StorageThresholds {
            low_available_bytes: 0,
            low_percentage: 50,
            critical_available_bytes: 0,
            critical_percentage: 1,
        };
        assert_eq!(
            thresholds.classify(Some(u64::MAX), Some(u64::MAX / 2)),
            StoragePressure::Low
        );
    }
}
