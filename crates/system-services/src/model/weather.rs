//! Weather data, units and location snapshots.

use chrono::{DateTime, NaiveTime, Utc};
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
