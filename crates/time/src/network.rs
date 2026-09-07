//! HTTP Date requests, server URL validation and synchronization errors.

use chrono::{DateTime, NaiveDateTime, TimeZone, Utc};
use reqwest::header::DATE;
use std::fmt;
use std::time::Duration;

const TIME_SYNC_TIMEOUT: Duration = Duration::from_secs(5);
pub const MAX_TIME_SERVER_URL_LEN: usize = 2_048;
const TIME_SYNC_SOURCES: &[&str] = &[
    "https://www.google.com/generate_204",
    "https://www.cloudflare.com/cdn-cgi/trace",
    "https://www.microsoft.com/",
];

pub type TimeSyncResult = Result<DateTime<Utc>, TimeSyncError>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimeSyncError {
    failures: Vec<String>,
}

impl TimeSyncError {
    pub fn new(failures: Vec<String>) -> Self {
        Self { failures }
    }
}

impl fmt::Display for TimeSyncError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.failures.is_empty() {
            formatter.write_str("all time sources failed")
        } else {
            write!(
                formatter,
                "all time sources failed: {}",
                self.failures.join("; ")
            )
        }
    }
}

impl std::error::Error for TimeSyncError {}

pub async fn fetch_standard_time() -> TimeSyncResult {
    let client = reqwest::Client::builder()
        .timeout(TIME_SYNC_TIMEOUT)
        .connect_timeout(TIME_SYNC_TIMEOUT)
        .build()
        .map_err(|error| TimeSyncError::new(vec![format!("client setup failed: {error}")]))?;

    fetch_standard_time_with_client(&client, TIME_SYNC_SOURCES).await
}

pub async fn fetch_time_from_server(server_url: &str) -> TimeSyncResult {
    let server_url =
        normalize_time_server_url(server_url).map_err(|error| TimeSyncError::new(vec![error]))?;
    let client = reqwest::Client::builder()
        .timeout(TIME_SYNC_TIMEOUT)
        .connect_timeout(TIME_SYNC_TIMEOUT)
        .build()
        .map_err(|error| TimeSyncError::new(vec![format!("client setup failed: {error}")]))?;
    fetch_standard_time_with_client(&client, &[server_url.as_str()]).await
}

pub fn normalize_time_server_url(value: &str) -> Result<String, String> {
    let value = value.trim();
    if value.is_empty() {
        return Err("time server address must not be empty".to_string());
    }
    if value.len() > MAX_TIME_SERVER_URL_LEN {
        return Err(format!(
            "time server address is limited to {MAX_TIME_SERVER_URL_LEN} characters"
        ));
    }
    let parsed =
        reqwest::Url::parse(value).map_err(|error| format!("invalid time server URL: {error}"))?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err("time server URL must use http:// or https://".to_string());
    }
    if parsed.host_str().is_none() {
        return Err("time server URL must include a host".to_string());
    }
    Ok(parsed.to_string())
}

async fn fetch_standard_time_with_client(
    client: &reqwest::Client,
    sources: &[&str],
) -> TimeSyncResult {
    let mut failures = Vec::new();

    for source in sources {
        match fetch_source_time(client, source).await {
            Ok(utc) => return Ok(utc),
            Err(error) => failures.push(format!("{source}: {error}")),
        }
    }

    Err(TimeSyncError::new(failures))
}

async fn fetch_source_time(
    client: &reqwest::Client,
    source: &str,
) -> Result<DateTime<Utc>, String> {
    let response = client
        .get(source)
        .send()
        .await
        .map_err(|error| error.to_string())?
        .error_for_status()
        .map_err(|error| error.to_string())?;

    let value = response
        .headers()
        .get(DATE)
        .ok_or_else(|| "missing Date header".to_string())?
        .to_str()
        .map_err(|error| format!("invalid Date header: {error}"))?;

    parse_http_date(value)
}

fn parse_http_date(value: &str) -> Result<DateTime<Utc>, String> {
    if let Ok(parsed) = DateTime::parse_from_rfc2822(value) {
        return Ok(parsed.with_timezone(&Utc));
    }

    NaiveDateTime::parse_from_str(value, "%a, %d %b %Y %H:%M:%S GMT")
        .map(|naive| Utc.from_utc_datetime(&naive))
        .map_err(|error| format!("could not parse Date header {value:?}: {error}"))
}

#[cfg(test)]
#[path = "tests/network.rs"]
mod tests;
