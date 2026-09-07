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
            .ok()?
            .error_for_status()
            .ok()?;
        let location = response.json::<IpLocationResponse>().await.ok()?;
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
            let _ = save_location_cache(config, query, &resolved);
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
