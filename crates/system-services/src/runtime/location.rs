//! Location selection, system detection and text geocoding.

use super::SystemServicesConfig;
use super::cache::{load_location_cache, save_location_cache};
use crate::GeoLocation;
use async_trait::async_trait;
use serde::Deserialize;
use std::time::Duration;

const SYSTEM_LOCATION_DETECTION_TIMEOUT: Duration = Duration::from_secs(3);

#[async_trait]
pub(super) trait SystemLocationDetector: Send + Sync {
    async fn detect(&self) -> Option<GeoLocation>;
}

pub(super) struct IpLocationDetector;

#[derive(Deserialize)]
struct IpLocationResponse {
    latitude: f64,
    longitude: f64,
    city: Option<String>,
}

#[async_trait]
impl SystemLocationDetector for IpLocationDetector {
    async fn detect(&self) -> Option<GeoLocation> {
        let response = reqwest::Client::new()
            .get("https://ipapi.co/json/")
            .header(
                "User-Agent",
                format!("tundra-system-services/{}", env!("CARGO_PKG_VERSION")),
            )
            .send()
            .await
            .map_err(|error| super::telemetry::typed_failure("location", "detect", &error, true))
            .ok()?
            .error_for_status()
            .map_err(|error| super::telemetry::typed_failure("location", "detect", &error, true))
            .ok()?;
        let location = response
            .json::<IpLocationResponse>()
            .await
            .map_err(|error| super::telemetry::typed_failure("location", "detect", &error, true))
            .ok()?;
        super::telemetry::recovered("location", "detect");
        Some(GeoLocation {
            latitude: location.latitude,
            longitude: location.longitude,
            city: location.city,
        })
    }
}

pub(super) async fn resolve_location(
    config: &SystemServicesConfig,
    should_refresh_location: bool,
    system_location: &mut Option<GeoLocation>,
    detector: &dyn SystemLocationDetector,
) -> GeoLocation {
    if let Some(query) = config
        .weather_location
        .as_deref()
        .map(str::trim)
        .filter(|query| !query.is_empty())
    {
        if let Some(cached) = load_location_cache(config, query) {
            return cached;
        }
        if should_refresh_location && let Some(resolved) = geocode(query).await {
            match save_location_cache(config, query, &resolved) {
                Ok(()) => super::telemetry::recovered("location", "cache_write"),
                Err(error) => {
                    super::telemetry::typed_failure("location", "cache_write", &error, true)
                }
            }
            return resolved;
        }
    }
    if let Some(location) = config.timezone_location.clone() {
        return location;
    }
    if (should_refresh_location || system_location.is_none())
        && let Some(location) = tokio::time::timeout(
            config
                .request_timeout
                .min(SYSTEM_LOCATION_DETECTION_TIMEOUT),
            detector.detect(),
        )
        .await
        .map_err(|error| super::telemetry::typed_failure("location", "detect", &error, true))
        .ok()
        .flatten()
    {
        *system_location = Some(location);
    }
    system_location
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
        .map_err(|error| super::telemetry::typed_failure("location", "geocode", &error, true))
        .ok()?
        .error_for_status()
        .map_err(|error| super::telemetry::typed_failure("location", "geocode", &error, true))
        .ok()?;
    let item = response
        .json::<Vec<GeocodeResponse>>()
        .await
        .map_err(|error| super::telemetry::typed_failure("location", "geocode", &error, true))
        .ok()?
        .into_iter()
        .next()
        .or_else(|| {
            super::telemetry::failure(
                "location",
                "geocode",
                "geocoding returned no matching location",
                true,
            );
            None
        })?;
    let latitude = item
        .lat
        .parse()
        .map_err(|error: std::num::ParseFloatError| {
            super::telemetry::typed_failure("location", "geocode", &error, true)
        })
        .ok()?;
    let longitude = item
        .lon
        .parse()
        .map_err(|error: std::num::ParseFloatError| {
            super::telemetry::typed_failure("location", "geocode", &error, true)
        })
        .ok()?;
    super::telemetry::recovered("location", "geocode");
    Some(GeoLocation {
        latitude,
        longitude,
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
