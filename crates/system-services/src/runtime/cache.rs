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
) -> Result<Option<WeatherSnapshot>, ()> {
    let path = cache_root(config).ok_or(())?.join("weather.json");
    let contents = fs::read_to_string(path).map_err(|_| ())?;
    let cached: CachedWeather = serde_json::from_str(&contents).map_err(|_| ())?;
    Ok(Some(cached.snapshot))
}
pub(super) fn save_weather_cache(
    config: &SystemServicesConfig,
    snapshot: &WeatherSnapshot,
) -> Result<(), ()> {
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
pub(super) fn load_location_cache(
    config: &SystemServicesConfig,
    query: &str,
) -> Option<GeoLocation> {
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
pub(super) fn save_location_cache(
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
