use crate::weather::{WeatherData, WeatherLocation, WeatherUnits};
use crate::{config::Provider, geolocation::GeoLocation};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::future::Future;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::fs;
use watchdog::{
    AppWatchdog, ComponentId, ErrorContext, IncidentSeverity, TaskId, TaskSpec, WatchdogError,
};

const LOCATION_CACHE_DURATION_SECS: u64 = 86400;
const WEATHER_CACHE_DURATION_SECS: u64 = 300;
static NEXT_CACHE_TASK_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, thiserror::Error)]
pub(crate) enum CacheWriteError {
    #[error("the operating system did not provide a cache directory")]
    CacheDirectoryUnavailable,

    #[error("failed to {operation} cache path {path}: {source}")]
    Io {
        operation: &'static str,
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("failed to serialize cache data: {0}")]
    Serialization(#[from] serde_json::Error),
}

fn cache_io_error(
    operation: &'static str,
    path: PathBuf,
    source: std::io::Error,
) -> CacheWriteError {
    CacheWriteError::Io {
        operation,
        path,
        source,
    }
}

fn spawn_cache_write<F, Fut>(
    watchdog: &AppWatchdog,
    kind: &'static str,
    operation: F,
) -> Result<(), WatchdogError>
where
    F: FnOnce() -> Fut + Send + 'static,
    Fut: Future<Output = Result<(), CacheWriteError>> + Send + 'static,
{
    let cache_watchdog = watchdog.child_component(ComponentId::from_static("cache"));
    let group = cache_watchdog.task_group("writes");
    let sequence = NEXT_CACHE_TASK_ID.fetch_add(1, Ordering::Relaxed);
    let task_id = TaskId::new(format!("{kind}-{sequence}"))?;
    let mut operation = Some(operation);

    let _ = group.spawn_async(TaskSpec::one_shot(task_id), move || {
        let operation = operation
            .take()
            .expect("one-shot cache task factory is called only once");
        let cache_watchdog = cache_watchdog.clone();
        async move {
            if let Err(error) = operation().await {
                cache_watchdog.report_error(
                    ErrorContext::new(format!("cache.{kind}"), IncidentSeverity::Warning),
                    &error,
                );
            }
        }
    })?;
    Ok(())
}

#[derive(Serialize, Deserialize)]
struct LocationCache {
    location: GeoLocation,
    cached_at: u64,
}

#[derive(Serialize, Deserialize)]
struct WeatherCache {
    data: WeatherData,
    cached_at: u64,
    location: WeatherLocation,
    units: WeatherUnits,
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
    if now.saturating_sub(cache.cached_at) < LOCATION_CACHE_DURATION_SECS {
        Some(cache.location)
    } else {
        None
    }
}

pub fn save_location_cache(location: &GeoLocation) -> Result<(), WatchdogError> {
    let watchdog = AppWatchdog::current().ok_or(WatchdogError::NotInstalled)?;
    save_location_cache_managed(&watchdog, location)
}

pub fn save_location_cache_managed(
    watchdog: &AppWatchdog,
    location: &GeoLocation,
) -> Result<(), WatchdogError> {
    let location = location.clone();
    spawn_cache_write(watchdog, "location", move || async move {
        let cache_dir = get_cache_dir().ok_or(CacheWriteError::CacheDirectoryUnavailable)?;
        fs::create_dir_all(&cache_dir)
            .await
            .map_err(|error| cache_io_error("create", cache_dir.clone(), error))?;

        let cache = LocationCache {
            location,
            cached_at: current_timestamp(),
        };
        let json = serde_json::to_string(&cache)?;
        let cache_path = cache_dir.join("location.json");
        fs::write(&cache_path, json)
            .await
            .map_err(|error| cache_io_error("write", cache_path, error))
    })
}

pub async fn load_cached_address(address_key: &str) -> Option<GeoLocation> {
    let address_key = normalize_address_key(address_key)?;
    let cache_path = get_cache_dir()?.join("address.json");
    let contents = fs::read_to_string(&cache_path).await.ok()?;
    let cache: AddressCache = serde_json::from_str(&contents).ok()?;

    cached_address_entry(&cache, &address_key, current_timestamp())
}

pub fn save_address_cache(address_key: &str, location: &GeoLocation) -> Result<(), WatchdogError> {
    let watchdog = AppWatchdog::current().ok_or(WatchdogError::NotInstalled)?;
    save_address_cache_managed(&watchdog, address_key, location)
}

pub fn save_address_cache_managed(
    watchdog: &AppWatchdog,
    address_key: &str,
    location: &GeoLocation,
) -> Result<(), WatchdogError> {
    let Some(address_key) = normalize_address_key(address_key) else {
        return Ok(());
    };

    let location = location.clone();
    spawn_cache_write(watchdog, "address", move || async move {
        let cache_dir = get_cache_dir().ok_or(CacheWriteError::CacheDirectoryUnavailable)?;
        fs::create_dir_all(&cache_dir)
            .await
            .map_err(|error| cache_io_error("create", cache_dir.clone(), error))?;

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

        let json = serde_json::to_string(&cache)?;
        fs::write(&cache_path, json)
            .await
            .map_err(|error| cache_io_error("write", cache_path, error))
    })
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
    if now.saturating_sub(cache.cached_at) < LOCATION_CACHE_DURATION_SECS {
        Some(cache.city_name)
    } else {
        None
    }
}

pub fn save_geocode_cache(
    city_name: &str,
    latitude: f64,
    longitude: f64,
    language: &str,
) -> Result<(), WatchdogError> {
    let watchdog = AppWatchdog::current().ok_or(WatchdogError::NotInstalled)?;
    save_geocode_cache_managed(&watchdog, city_name, latitude, longitude, language)
}

pub fn save_geocode_cache_managed(
    watchdog: &AppWatchdog,
    city_name: &str,
    latitude: f64,
    longitude: f64,
    language: &str,
) -> Result<(), WatchdogError> {
    let city_name = city_name.to_string();
    let language = language.to_string();
    spawn_cache_write(watchdog, "geocode", move || async move {
        let cache_dir = get_cache_dir().ok_or(CacheWriteError::CacheDirectoryUnavailable)?;
        fs::create_dir_all(&cache_dir)
            .await
            .map_err(|error| cache_io_error("create", cache_dir.clone(), error))?;

        let cache = GeocodeCache {
            city_name,
            cached_at: current_timestamp(),
            location_key: make_location_key(latitude, longitude),
            language,
        };
        let json = serde_json::to_string(&cache)?;
        let cache_path = cache_dir.join("geocode.json");
        fs::write(&cache_path, json)
            .await
            .map_err(|error| cache_io_error("write", cache_path, error))
    })
}

pub async fn load_cached_weather(
    location: WeatherLocation,
    units: WeatherUnits,
    provider: Provider,
) -> Option<WeatherData> {
    let cache_path = get_cache_dir()?.join("weather.json");
    let contents = fs::read_to_string(&cache_path).await.ok()?;
    let cache: WeatherCache = serde_json::from_str(&contents).ok()?;

    if cache.location != location || cache.units != units || cache.provider != provider {
        return None;
    }

    let now = current_timestamp();
    if now.saturating_sub(cache.cached_at) < WEATHER_CACHE_DURATION_SECS {
        Some(cache.data)
    } else {
        None
    }
}

pub fn save_weather_cache(
    weather: &WeatherData,
    location: WeatherLocation,
    units: WeatherUnits,
    provider: Provider,
) -> Result<(), WatchdogError> {
    let watchdog = AppWatchdog::current().ok_or(WatchdogError::NotInstalled)?;
    save_weather_cache_managed(&watchdog, weather, location, units, provider)
}

pub fn save_weather_cache_managed(
    watchdog: &AppWatchdog,
    weather: &WeatherData,
    location: WeatherLocation,
    units: WeatherUnits,
    provider: Provider,
) -> Result<(), WatchdogError> {
    let weather = weather.clone();
    spawn_cache_write(watchdog, "weather", move || async move {
        save_weather_cache_now(&weather, location, units, provider).await
    })
}

pub(crate) async fn save_weather_cache_now(
    weather: &WeatherData,
    location: WeatherLocation,
    units: WeatherUnits,
    provider: Provider,
) -> Result<(), CacheWriteError> {
    let cache_dir = get_cache_dir().ok_or(CacheWriteError::CacheDirectoryUnavailable)?;
    fs::create_dir_all(&cache_dir)
        .await
        .map_err(|error| cache_io_error("create", cache_dir.clone(), error))?;

    let cache = WeatherCache {
        data: weather.clone(),
        cached_at: current_timestamp(),
        location,
        units,
        provider,
    };
    let json = serde_json::to_string(&cache)?;
    let cache_path = cache_dir.join("weather.json");
    fs::write(&cache_path, json)
        .await
        .map_err(|error| cache_io_error("write", cache_path, error))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::time::Duration;
    use watchdog::{WatchdogConfig, WatchdogRuntime};

    fn test_location() -> GeoLocation {
        GeoLocation {
            latitude: 40.7128,
            longitude: -74.0060,
            city: Some("New York".to_string()),
        }
    }

    #[tokio::test]
    async fn managed_cache_task_uses_an_explicit_non_global_watchdog() {
        let root = std::env::temp_dir().join(format!(
            "tundra-weathr-cache-watchdog-{}-{}",
            std::process::id(),
            current_timestamp()
        ));
        let config = WatchdogConfig::new(
            root.join("crashes"),
            root.join("fallback"),
            root.join("state"),
            "tundra-weathr-cache-test",
            env!("CARGO_PKG_VERSION"),
        );
        let (runtime, process) =
            WatchdogRuntime::start_isolated(config).expect("test watchdog starts");
        let watchdog = process
            .register_app(crate::weathr_watchdog_descriptor())
            .expect("test Weathr watchdog registers");
        let completed = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let task_completed = completed.clone();

        spawn_cache_write(&watchdog, "explicit-test", move || async move {
            task_completed.store(true, Ordering::Release);
            Ok(())
        })
        .expect("explicit managed cache task starts without a global watchdog");

        tokio::time::timeout(Duration::from_secs(1), async {
            while !completed.load(Ordering::Acquire) {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("managed cache task completes");
        runtime.shutdown().expect("test watchdog shuts down");
        let _ = std::fs::remove_dir_all(root);
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
