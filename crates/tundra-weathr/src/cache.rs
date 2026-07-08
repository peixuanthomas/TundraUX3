use crate::weather::WeatherData;
use crate::{config::Provider, geolocation::GeoLocation};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use tokio::fs;

const LOCATION_CACHE_DURATION_SECS: u64 = 86400;
const WEATHER_CACHE_DURATION_SECS: u64 = 300;

#[derive(Serialize, Deserialize)]
struct LocationCache {
    location: GeoLocation,
    cached_at: u64,
}

#[derive(Serialize, Deserialize)]
struct WeatherCache {
    data: WeatherData,
    cached_at: u64,
    location_key: String,
    provider: Provider,
}

#[derive(Serialize, Deserialize, Default)]
struct AddressCache {
    entries: HashMap<String, AddressCacheEntry>,
}

#[derive(Serialize, Deserialize)]
struct AddressCacheEntry {
    location: GeoLocation,
    cached_at: u64,
}

fn get_cache_dir() -> Option<PathBuf> {
    Some(dirs::cache_dir()?.join("weathr"))
}

fn current_timestamp() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn make_location_key(latitude: f64, longitude: f64) -> String {
    format!("{:.2},{:.2}", latitude, longitude)
}

pub(crate) fn normalize_address_key(address: &str) -> Option<String> {
    let normalized = address
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase();

    if normalized.is_empty() {
        None
    } else {
        Some(normalized)
    }
}

fn cached_address_entry(cache: &AddressCache, address_key: &str, now: u64) -> Option<GeoLocation> {
    let entry = cache.entries.get(address_key)?;
    if now.saturating_sub(entry.cached_at) < LOCATION_CACHE_DURATION_SECS {
        Some(entry.location.clone())
    } else {
        None
    }
}

fn prune_expired_address_entries(cache: &mut AddressCache, now: u64) {
    cache
        .entries
        .retain(|_, entry| now.saturating_sub(entry.cached_at) < LOCATION_CACHE_DURATION_SECS);
}

pub async fn load_cached_location() -> Option<GeoLocation> {
    let cache_path = get_cache_dir()?.join("location.json");
    let contents = fs::read_to_string(&cache_path).await.ok()?;
    let cache: LocationCache = serde_json::from_str(&contents).ok()?;

    let now = current_timestamp();
    if now - cache.cached_at < LOCATION_CACHE_DURATION_SECS {
        Some(cache.location)
    } else {
        None
    }
}

pub fn save_location_cache(location: &GeoLocation) {
    let location = location.clone();
    tokio::spawn(async move {
        if let Some(cache_dir) = get_cache_dir() {
            let _ = fs::create_dir_all(&cache_dir).await;

            let cache = LocationCache {
                location,
                cached_at: current_timestamp(),
            };

            if let Ok(json) = serde_json::to_string(&cache) {
                let _ = fs::write(cache_dir.join("location.json"), json).await;
            }
        }
    });
}

pub async fn load_cached_address(address_key: &str) -> Option<GeoLocation> {
    let address_key = normalize_address_key(address_key)?;
    let cache_path = get_cache_dir()?.join("address.json");
    let contents = fs::read_to_string(&cache_path).await.ok()?;
    let cache: AddressCache = serde_json::from_str(&contents).ok()?;

    cached_address_entry(&cache, &address_key, current_timestamp())
}

pub fn save_address_cache(address_key: &str, location: &GeoLocation) {
    let Some(address_key) = normalize_address_key(address_key) else {
        return;
    };

    let location = location.clone();
    tokio::spawn(async move {
        if let Some(cache_dir) = get_cache_dir() {
            let _ = fs::create_dir_all(&cache_dir).await;

            let cache_path = cache_dir.join("address.json");
            let mut cache: AddressCache = match fs::read_to_string(&cache_path).await {
                Ok(contents) => serde_json::from_str(&contents).unwrap_or_default(),
                Err(_) => AddressCache::default(),
            };

            let now = current_timestamp();
            prune_expired_address_entries(&mut cache, now);
            cache.entries.insert(
                address_key,
                AddressCacheEntry {
                    location,
                    cached_at: now,
                },
            );

            if let Ok(json) = serde_json::to_string(&cache) {
                let _ = fs::write(cache_path, json).await;
            }
        }
    });
}

#[derive(Serialize, Deserialize)]
struct GeocodeCache {
    city_name: String,
    cached_at: u64,
    location_key: String,
    language: String,
}

pub async fn load_cached_geocode(latitude: f64, longitude: f64, language: &str) -> Option<String> {
    let cache_path = get_cache_dir()?.join("geocode.json");
    let contents = fs::read_to_string(&cache_path).await.ok()?;
    let cache: GeocodeCache = serde_json::from_str(&contents).ok()?;

    let location_key = make_location_key(latitude, longitude);
    if cache.location_key != location_key || cache.language != language {
        return None;
    }

    let now = current_timestamp();
    if now - cache.cached_at < LOCATION_CACHE_DURATION_SECS {
        Some(cache.city_name)
    } else {
        None
    }
}

pub fn save_geocode_cache(city_name: &str, latitude: f64, longitude: f64, language: &str) {
    let city_name = city_name.to_string();
    let language = language.to_string();
    tokio::spawn(async move {
        if let Some(cache_dir) = get_cache_dir() {
            let _ = fs::create_dir_all(&cache_dir).await;

            let cache = GeocodeCache {
                city_name,
                cached_at: current_timestamp(),
                location_key: make_location_key(latitude, longitude),
                language,
            };

            if let Ok(json) = serde_json::to_string(&cache) {
                let _ = fs::write(cache_dir.join("geocode.json"), json).await;
            }
        }
    });
}

pub async fn load_cached_weather(
    latitude: f64,
    longitude: f64,
    provider: Provider,
) -> Option<WeatherData> {
    let cache_path = get_cache_dir()?.join("weather.json");
    let contents = fs::read_to_string(&cache_path).await.ok()?;
    let cache: WeatherCache = serde_json::from_str(&contents).ok()?;

    let location_key = make_location_key(latitude, longitude);
    if cache.location_key != location_key || cache.provider != provider {
        return None;
    }

    let now = current_timestamp();
    if now - cache.cached_at < WEATHER_CACHE_DURATION_SECS {
        Some(cache.data)
    } else {
        None
    }
}

pub fn save_weather_cache(
    weather: &WeatherData,
    latitude: f64,
    longitude: f64,
    provider: Provider,
) {
    let weather = weather.clone();
    tokio::spawn(async move {
        if let Some(cache_dir) = get_cache_dir() {
            let _ = fs::create_dir_all(&cache_dir).await;

            let cache = WeatherCache {
                data: weather,
                cached_at: current_timestamp(),
                location_key: make_location_key(latitude, longitude),
                provider,
            };

            if let Ok(json) = serde_json::to_string(&cache) {
                let _ = fs::write(cache_dir.join("weather.json"), json).await;
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_location() -> GeoLocation {
        GeoLocation {
            latitude: 40.7128,
            longitude: -74.0060,
            city: Some("New York".to_string()),
        }
    }

    #[test]
    fn normalizes_address_key_for_cache_lookup() {
        assert_eq!(
            normalize_address_key("  350   Fifth Avenue\nNew   York  "),
            Some("350 fifth avenue new york".to_string())
        );
        assert_eq!(normalize_address_key("\t \n"), None);
    }

    #[test]
    fn cached_address_entry_returns_matching_fresh_location() {
        let now = 1_000_000;
        let location = test_location();
        let mut cache = AddressCache::default();
        cache.entries.insert(
            "new york".to_string(),
            AddressCacheEntry {
                location: location.clone(),
                cached_at: now - 60,
            },
        );

        let cached = cached_address_entry(&cache, "new york", now).expect("fresh cache hit");
        assert_eq!(cached.latitude, location.latitude);
        assert_eq!(cached.longitude, location.longitude);
        assert_eq!(cached.city, location.city);
        assert!(cached_address_entry(&cache, "newark", now).is_none());
    }

    #[test]
    fn cached_address_entry_expires_after_24_hours() {
        let now = 1_000_000;
        let mut cache = AddressCache::default();
        cache.entries.insert(
            "new york".to_string(),
            AddressCacheEntry {
                location: test_location(),
                cached_at: now - LOCATION_CACHE_DURATION_SECS,
            },
        );

        assert!(cached_address_entry(&cache, "new york", now).is_none());
    }

    #[test]
    fn prune_expired_address_entries_keeps_only_fresh_entries() {
        let now = 1_000_000;
        let mut cache = AddressCache::default();
        cache.entries.insert(
            "fresh".to_string(),
            AddressCacheEntry {
                location: test_location(),
                cached_at: now - 60,
            },
        );
        cache.entries.insert(
            "expired".to_string(),
            AddressCacheEntry {
                location: test_location(),
                cached_at: now - LOCATION_CACHE_DURATION_SECS,
            },
        );

        prune_expired_address_entries(&mut cache, now);

        assert!(cache.entries.contains_key("fresh"));
        assert!(!cache.entries.contains_key("expired"));
    }
}
