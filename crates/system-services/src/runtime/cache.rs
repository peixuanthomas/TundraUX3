//! Weather and location cache files.

use super::SystemServicesConfig;
use crate::{GeoLocation, WeatherSnapshot};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

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
pub(super) fn load_weather_cache(
    config: &SystemServicesConfig,
) -> std::io::Result<Option<WeatherSnapshot>> {
    let path = cache_root(config)
        .ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::NotFound, "cache directory unavailable")
        })?
        .join("weather.json");
    let contents = match fs::read_to_string(path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    let cached: CachedWeather = serde_json::from_str(&contents).map_err(std::io::Error::other)?;
    Ok(Some(cached.snapshot))
}
pub(super) fn save_weather_cache(
    config: &SystemServicesConfig,
    snapshot: &WeatherSnapshot,
) -> std::io::Result<()> {
    let root = cache_root(config).ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::NotFound, "cache directory unavailable")
    })?;
    fs::create_dir_all(&root)?;
    let body = serde_json::to_string(&CachedWeather {
        saved_at: Utc::now(),
        snapshot: snapshot.clone(),
    })
    .map_err(std::io::Error::other)?;
    fs::write(root.join("weather.json"), body)
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
pub(super) fn load_location_cache(
    config: &SystemServicesConfig,
    query: &str,
) -> Option<GeoLocation> {
    let contents = fs::read_to_string(
        cache_root(config)?.join(format!("location-{}.json", location_key(query))),
    )
    .map_err(|error| {
        if error.kind() != std::io::ErrorKind::NotFound {
            super::telemetry::typed_failure("location", "cache_read", &error, true);
        }
    })
    .ok()?;
    let cache: CachedLocation = serde_json::from_str(&contents)
        .map_err(|error| super::telemetry::typed_failure("location", "cache_read", &error, true))
        .ok()?;
    super::telemetry::recovered("location", "cache_read");
    (Utc::now() - cache.saved_at)
        .to_std()
        .ok()?
        .lt(&config.location_refresh_interval)
        .then_some(cache.location)
}
pub(super) fn save_location_cache(
    config: &SystemServicesConfig,
    query: &str,
    location: &GeoLocation,
) -> std::io::Result<()> {
    let root = cache_root(config).ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::NotFound, "cache directory unavailable")
    })?;
    fs::create_dir_all(&root)?;
    let body = serde_json::to_string(&CachedLocation {
        saved_at: Utc::now(),
        location: location.clone(),
    })
    .map_err(std::io::Error::other)?;
    fs::write(
        root.join(format!("location-{}.json", location_key(query))),
        body,
    )
}
