//! Refresh intervals and service configuration.

use crate::{GeoLocation, StorageThresholds, TimeSyncMode, WeatherUnits};
use std::path::PathBuf;
use std::time::Duration;

const DEFAULT_WEATHER_REFRESH: Duration = Duration::from_secs(5 * 60);
const DEFAULT_LOCATION_REFRESH: Duration = Duration::from_secs(24 * 60 * 60);
const DEFAULT_TIME_REFRESH: Duration = Duration::from_secs(5 * 60);
const DEFAULT_SYSTEM_STATUS_BACKGROUND_REFRESH: Duration = Duration::from_secs(30);
const DEFAULT_SYSTEM_STATUS_ACTIVE_REFRESH: Duration = Duration::from_secs(1);
const DEFAULT_SYSTEM_STATUS_ACTIVE_SLOW_REFRESH: Duration = Duration::from_secs(5);

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
