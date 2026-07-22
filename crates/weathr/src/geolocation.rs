use crate::cache;
use crate::error::{GeolocationError, NetworkError};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use watchdog::AppWatchdog;

const IPINFO_URL: &str = "https://ipinfo.io/json";
const NOMINATIM_URL: &str = "https://nominatim.openstreetmap.org/reverse";
const NOMINATIM_SEARCH_URL: &str = "https://nominatim.openstreetmap.org/search";
const NOMINATIM_SEARCH_LANGUAGE: &str = "en";
const FALLBACK_CITY: &str = "Shanghai";
const FALLBACK_LATITUDE: f64 = 31.2304;
const FALLBACK_LONGITUDE: f64 = 121.4737;
const MAX_RETRIES: u32 = 3;
const INITIAL_RETRY_DELAY_MS: u64 = 500;

#[derive(Deserialize, Debug)]
struct IpInfoResponse {
    loc: String,
    city: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeoLocation {
    pub latitude: f64,
    pub longitude: f64,
    pub city: Option<String>,
}

pub async fn detect_location() -> Result<GeoLocation, GeolocationError> {
    detect_location_with_watchdog(None).await
}

pub async fn detect_location_managed(
    watchdog: &AppWatchdog,
) -> Result<GeoLocation, GeolocationError> {
    detect_location_with_watchdog(Some(watchdog)).await
}

async fn detect_location_with_watchdog(
    watchdog: Option<&AppWatchdog>,
) -> Result<GeoLocation, GeolocationError> {
    if let Some(cached) = cache::load_cached_location().await {
        return Ok(cached);
    }

    detect_location_with_retry(watchdog).await
}

pub async fn search_address(query: &str) -> Result<GeoLocation, GeolocationError> {
    search_address_with_watchdog(query, None).await
}

pub async fn search_address_managed(
    watchdog: &AppWatchdog,
    query: &str,
) -> Result<GeoLocation, GeolocationError> {
    search_address_with_watchdog(query, Some(watchdog)).await
}

async fn search_address_with_watchdog(
    query: &str,
    watchdog: Option<&AppWatchdog>,
) -> Result<GeoLocation, GeolocationError> {
    let address_key =
        cache::normalize_address_key(query).ok_or(GeolocationError::EmptyAddressQuery)?;
    let cache_key = localized_address_cache_key(&address_key);

    if let Some(cached) = cache::load_cached_address(&cache_key).await {
        return Ok(cached);
    }

    let location = fetch_address_search(&address_key).await?;
    let _ = match watchdog {
        Some(watchdog) => cache::save_address_cache_managed(watchdog, &cache_key, &location),
        None => cache::save_address_cache(&cache_key, &location),
    };
    Ok(location)
}

pub fn fallback_location() -> GeoLocation {
    GeoLocation {
        latitude: FALLBACK_LATITUDE,
        longitude: FALLBACK_LONGITUDE,
        city: Some(FALLBACK_CITY.to_string()),
    }
}

fn localized_address_cache_key(address_key: &str) -> String {
    format!("{NOMINATIM_SEARCH_LANGUAGE}:{address_key}")
}

async fn detect_location_with_retry(
    watchdog: Option<&AppWatchdog>,
) -> Result<GeoLocation, GeolocationError> {
    let mut last_error = None;

    for attempt in 1..=MAX_RETRIES {
        match fetch_location(watchdog).await {
            Ok(location) => return Ok(location),
            Err(e) => {
                let should_retry = matches!(
                    e,
                    GeolocationError::Unreachable(ref net_err) if net_err.is_retryable()
                );

                if !should_retry || attempt == MAX_RETRIES {
                    return Err(e);
                }

                let delay_ms = INITIAL_RETRY_DELAY_MS * 2_u64.pow(attempt - 1);
                tokio::time::sleep(Duration::from_millis(delay_ms)).await;
                last_error = Some(e);
            }
        }
    }

    Err(
        last_error.unwrap_or_else(|| GeolocationError::RetriesExhausted {
            attempts: MAX_RETRIES,
        }),
    )
}

async fn fetch_location(watchdog: Option<&AppWatchdog>) -> Result<GeoLocation, GeolocationError> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .connect_timeout(Duration::from_secs(5))
        .build()
        .map_err(|e| GeolocationError::Unreachable(NetworkError::ClientCreation(e)))?;

    let response = client
        .get(IPINFO_URL)
        .send()
        .await
        .and_then(|resp| resp.error_for_status())
        .map_err(|e| {
            GeolocationError::Unreachable(NetworkError::from_reqwest(e, IPINFO_URL, 10))
        })?;

    let ip_info: IpInfoResponse = response.json().await.map_err(|e| {
        GeolocationError::Unreachable(NetworkError::from_reqwest(e, IPINFO_URL, 10))
    })?;

    let coords: Vec<&str> = ip_info.loc.split(',').collect();
    if coords.len() != 2 {
        return Err(GeolocationError::ParseError(
            "Invalid location format from ipinfo.io".to_string(),
        ));
    }

    let latitude = coords[0]
        .parse::<f64>()
        .map_err(|_| GeolocationError::ParseError("Invalid latitude format".to_string()))?;

    let longitude = coords[1]
        .parse::<f64>()
        .map_err(|_| GeolocationError::ParseError("Invalid longitude format".to_string()))?;

    let location = GeoLocation {
        latitude,
        longitude,
        city: ip_info.city,
    };

    let _ = match watchdog {
        Some(watchdog) => cache::save_location_cache_managed(watchdog, &location),
        None => cache::save_location_cache(&location),
    };

    Ok(location)
}

#[derive(Deserialize, Debug)]
struct NominatimAddress {
    city: Option<String>,
    town: Option<String>,
    village: Option<String>,
}

#[derive(Deserialize, Debug)]
struct NominatimResponse {
    address: Option<NominatimAddress>,
}

#[derive(Deserialize, Debug)]
struct NominatimSearchAddress {
    city: Option<String>,
    town: Option<String>,
    village: Option<String>,
    municipality: Option<String>,
    hamlet: Option<String>,
    suburb: Option<String>,
    county: Option<String>,
    state: Option<String>,
}

#[derive(Deserialize, Debug)]
struct NominatimSearchResult {
    lat: String,
    lon: String,
    display_name: Option<String>,
    address: Option<NominatimSearchAddress>,
}

/// Best-effort reverse geocode: returns a city/town/village name for the given
/// coordinates, or `None` if the lookup fails or the location doesn't map to a
/// meaningful settlement (e.g. open sea, administrative-only regions).
pub async fn reverse_geocode(latitude: f64, longitude: f64, language: &str) -> Option<String> {
    reverse_geocode_with_watchdog(latitude, longitude, language, None).await
}

pub async fn reverse_geocode_managed(
    watchdog: &AppWatchdog,
    latitude: f64,
    longitude: f64,
    language: &str,
) -> Option<String> {
    reverse_geocode_with_watchdog(latitude, longitude, language, Some(watchdog)).await
}

async fn reverse_geocode_with_watchdog(
    latitude: f64,
    longitude: f64,
    language: &str,
    watchdog: Option<&AppWatchdog>,
) -> Option<String> {
    if let Some(cached) = cache::load_cached_geocode(latitude, longitude, language).await {
        return Some(cached);
    }

    let city = fetch_reverse_geocode(latitude, longitude, language).await?;
    let _ = match watchdog {
        Some(watchdog) => {
            cache::save_geocode_cache_managed(watchdog, &city, latitude, longitude, language)
        }
        None => cache::save_geocode_cache(&city, latitude, longitude, language),
    };
    Some(city)
}

async fn fetch_reverse_geocode(latitude: f64, longitude: f64, language: &str) -> Option<String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .connect_timeout(Duration::from_secs(3))
        .build()
        .ok()?;

    let url = format!(
        "{}?lat={}&lon={}&format=json&zoom=10",
        NOMINATIM_URL, latitude, longitude
    );

    let mut req = client.get(&url).header(
        "User-Agent",
        format!("weathr/{}", env!("CARGO_PKG_VERSION")),
    );

    if language != "auto" {
        req = req.header("Accept-Language", language);
    }

    let resp = req.send().await.ok()?;

    let data: NominatimResponse = resp.json().await.ok()?;

    let addr = data.address?;
    addr.city.or(addr.town).or(addr.village)
}

async fn fetch_address_search(query: &str) -> Result<GeoLocation, GeolocationError> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .connect_timeout(Duration::from_secs(5))
        .build()
        .map_err(|e| GeolocationError::Unreachable(NetworkError::ClientCreation(e)))?;

    let url = nominatim_search_url(query)?;
    let url_for_error = url.as_str().to_string();

    let response = client
        .get(url)
        .header(
            "User-Agent",
            format!("weathr/{}", env!("CARGO_PKG_VERSION")),
        )
        .header("Accept-Language", NOMINATIM_SEARCH_LANGUAGE)
        .send()
        .await
        .and_then(|resp| resp.error_for_status())
        .map_err(|e| {
            GeolocationError::Unreachable(NetworkError::from_reqwest(e, &url_for_error, 10))
        })?;

    let body = response.text().await.map_err(|e| {
        GeolocationError::Unreachable(NetworkError::from_reqwest(e, &url_for_error, 10))
    })?;

    parse_nominatim_search_response(&body, query)
}

fn nominatim_search_url(query: &str) -> Result<reqwest::Url, GeolocationError> {
    let mut url = reqwest::Url::parse(NOMINATIM_SEARCH_URL)
        .map_err(|e| GeolocationError::ParseError(format!("Invalid Nominatim search URL: {e}")))?;
    url.query_pairs_mut()
        .append_pair("q", query)
        .append_pair("format", "json")
        .append_pair("limit", "1")
        .append_pair("addressdetails", "1")
        .append_pair("accept-language", NOMINATIM_SEARCH_LANGUAGE);
    Ok(url)
}

fn parse_nominatim_search_response(
    body: &str,
    query: &str,
) -> Result<GeoLocation, GeolocationError> {
    let results: Vec<NominatimSearchResult> = serde_json::from_str(body).map_err(|e| {
        GeolocationError::ParseError(format!("Invalid address search response: {e}"))
    })?;

    let first = results
        .into_iter()
        .next()
        .ok_or_else(|| GeolocationError::AddressNotFound(query.to_string()))?;

    search_result_to_location(first)
}

fn search_result_to_location(
    result: NominatimSearchResult,
) -> Result<GeoLocation, GeolocationError> {
    let latitude = result.lat.parse::<f64>().map_err(|_| {
        GeolocationError::ParseError(
            "Invalid latitude format in address search response".to_string(),
        )
    })?;

    let longitude = result.lon.parse::<f64>().map_err(|_| {
        GeolocationError::ParseError(
            "Invalid longitude format in address search response".to_string(),
        )
    })?;

    let city = search_city_name(result.address, result.display_name);

    Ok(GeoLocation {
        latitude,
        longitude,
        city,
    })
}

fn search_city_name(
    address: Option<NominatimSearchAddress>,
    display_name: Option<String>,
) -> Option<String> {
    address
        .and_then(|addr| {
            addr.city
                .or(addr.town)
                .or(addr.village)
                .or(addr.municipality)
                .or(addr.hamlet)
                .or(addr.suburb)
                .or(addr.county)
                .or(addr.state)
        })
        .or_else(|| {
            display_name.and_then(|name| {
                name.split(',')
                    .next()
                    .map(str::trim)
                    .filter(|part| !part.is_empty())
                    .map(str::to_string)
            })
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_first_nominatim_search_result() {
        let body = r#"[
            {
                "lat": "51.5074456",
                "lon": "-0.1277653",
                "display_name": "London, Greater London, England, United Kingdom",
                "address": { "city": "London" }
            },
            {
                "lat": "48.8566",
                "lon": "2.3522",
                "display_name": "Paris, Ile-de-France, France",
                "address": { "city": "Paris" }
            }
        ]"#;

        let location =
            parse_nominatim_search_response(body, "london").expect("search response parses");

        assert_eq!(location.latitude, 51.5074456);
        assert_eq!(location.longitude, -0.1277653);
        assert_eq!(location.city, Some("London".to_string()));
    }

    #[test]
    fn search_url_requests_english_results() {
        let url = nominatim_search_url("上海").expect("search URL builds");
        let query = url.query().expect("search URL has query");

        assert!(query.contains("accept-language=en"));
    }

    #[test]
    fn address_cache_key_is_language_scoped() {
        assert_eq!(localized_address_cache_key("shanghai"), "en:shanghai");
    }

    #[test]
    fn fallback_location_is_shanghai() {
        let location = fallback_location();

        assert_eq!(location.latitude, FALLBACK_LATITUDE);
        assert_eq!(location.longitude, FALLBACK_LONGITUDE);
        assert_eq!(location.city, Some(FALLBACK_CITY.to_string()));
    }

    #[test]
    fn parses_search_result_with_display_name_fallback() {
        let body = r#"[
            {
                "lat": "37.4220604",
                "lon": "-122.0841032",
                "display_name": "Googleplex, Amphitheatre Parkway, Mountain View, California",
                "address": {}
            }
        ]"#;

        let location =
            parse_nominatim_search_response(body, "googleplex").expect("search response parses");

        assert_eq!(location.city, Some("Googleplex".to_string()));
    }

    #[test]
    fn empty_search_results_return_not_found_error() {
        let err = parse_nominatim_search_response("[]", "missing place").unwrap_err();

        assert!(matches!(
            err,
            GeolocationError::AddressNotFound(query) if query == "missing place"
        ));
    }

    #[test]
    fn invalid_search_coordinates_return_parse_error() {
        let body = r#"[
            {
                "lat": "not-a-latitude",
                "lon": "-122.0841032",
                "display_name": "Bad Coordinate",
                "address": { "city": "Mountain View" }
            }
        ]"#;

        let err = parse_nominatim_search_response(body, "bad coordinate").unwrap_err();

        assert!(
            matches!(err, GeolocationError::ParseError(message) if message.contains("latitude"))
        );
    }
}
