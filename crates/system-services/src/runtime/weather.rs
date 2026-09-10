//! Weather providers and their weather-code conversion.

use crate::{CelestialEvents, WeatherCondition, WeatherData, WeatherLocation, WeatherUnits};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::Deserialize;
use std::time::Duration;

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
            .map_err(|error| super::telemetry::capture("weather", "request", &error))?
            .error_for_status()
            .map_err(|error| super::telemetry::capture("weather", "request", &error))?;
        let current = response
            .json::<OpenMeteoResponse>()
            .await
            .map_err(|error| super::telemetry::capture("weather", "request", &error))?
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
        let mut value = HeaderValue::from_str(api_key)
            .map_err(|error| super::telemetry::capture("weather", "request", &error))?;
        value.set_sensitive(true);
        let mut headers = HeaderMap::new();
        headers.insert("apikey", value);
        let client = reqwest::Client::builder()
            .default_headers(headers)
            .build()
            .map_err(|error| super::telemetry::capture("weather", "request", &error))?;
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
            .map_err(|error| super::telemetry::capture("weather", "request", &error))?
            .error_for_status()
            .map_err(|error| super::telemetry::capture("weather", "request", &error))?
            .json::<MetOfficeResponse>()
            .await
            .map_err(|error| super::telemetry::capture("weather", "request", &error))?;
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
pub(super) fn normalize_met_office_code(code: i32) -> WeatherCondition {
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
