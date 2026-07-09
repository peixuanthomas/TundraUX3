use chrono::{DateTime, Local, NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Utc};
use chrono_tz::Tz;
use reqwest::header::DATE;
use std::fmt;
use std::time::{Duration, Instant};

pub const TIME_SYNC_INTERVAL: Duration = Duration::from_secs(5 * 60);

const TIME_SYNC_TIMEOUT: Duration = Duration::from_secs(5);
const TIME_SYNC_SOURCES: &[&str] = &[
    "https://www.google.com/generate_204",
    "https://www.cloudflare.com/cdn-cgi/trace",
    "https://www.microsoft.com/",
];

pub type TimeSyncResult = Result<DateTime<Utc>, TimeSyncError>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClockDisplay {
    pub date: NaiveDate,
    pub time: NaiveTime,
    pub warning: Option<String>,
}

#[derive(Debug)]
pub struct TimeSyncError {
    failures: Vec<String>,
}

impl TimeSyncError {
    fn new(failures: Vec<String>) -> Self {
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

#[derive(Debug, Clone)]
struct TimeAnchor {
    utc: DateTime<Utc>,
    instant: Instant,
}

impl TimeAnchor {
    fn new(utc: DateTime<Utc>) -> Self {
        Self {
            utc,
            instant: Instant::now(),
        }
    }

    fn current_utc(&self) -> DateTime<Utc> {
        advance_utc(self.utc, self.instant.elapsed())
    }
}

#[derive(Debug, Clone)]
pub struct NetworkClock {
    timezone: Option<Tz>,
    timezone_error: Option<String>,
    sync_error: Option<String>,
    anchor: Option<TimeAnchor>,
}

impl NetworkClock {
    pub fn new(timezone_id: Option<String>) -> Self {
        let mut timezone = Some(chrono_tz::UTC);
        let mut timezone_error = None;

        if let Some(timezone_id) = timezone_id
            .as_deref()
            .map(str::trim)
            .filter(|id| !id.is_empty())
        {
            match timezone_id.parse::<Tz>() {
                Ok(parsed) => timezone = Some(parsed),
                Err(error) => {
                    timezone = None;
                    timezone_error = Some(format!(
                        "Invalid timezone {timezone_id}: {error}; using system time"
                    ));
                }
            }
        }

        Self {
            timezone,
            timezone_error,
            sync_error: None,
            anchor: None,
        }
    }

    pub fn apply_sync(&mut self, result: TimeSyncResult) {
        match result {
            Ok(utc) => {
                self.anchor = Some(TimeAnchor::new(utc));
                self.sync_error = None;
            }
            Err(error) => {
                self.anchor = None;
                self.sync_error = Some(format!("Time sync failed: {error}; using system time"));
            }
        }
    }

    pub fn current(&self) -> ClockDisplay {
        if let Some(timezone) = self.timezone {
            let utc = self
                .anchor
                .as_ref()
                .map(TimeAnchor::current_utc)
                .unwrap_or_else(Utc::now);
            let local = utc.with_timezone(&timezone);
            return ClockDisplay {
                date: local.date_naive(),
                time: local.time(),
                warning: self.warning(),
            };
        }

        let local = Local::now();
        ClockDisplay {
            date: local.date_naive(),
            time: local.time(),
            warning: self.warning(),
        }
    }

    fn warning(&self) -> Option<String> {
        let mut warnings = Vec::new();
        if let Some(error) = &self.timezone_error {
            warnings.push(error.as_str());
        }
        if let Some(error) = &self.sync_error {
            warnings.push(error.as_str());
        }

        (!warnings.is_empty()).then(|| warnings.join(" | "))
    }
}

pub async fn fetch_standard_time() -> TimeSyncResult {
    let client = reqwest::Client::builder()
        .timeout(TIME_SYNC_TIMEOUT)
        .connect_timeout(TIME_SYNC_TIMEOUT)
        .build()
        .map_err(|error| TimeSyncError::new(vec![format!("client setup failed: {error}")]))?;

    fetch_standard_time_with_client(&client, TIME_SYNC_SOURCES).await
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

fn advance_utc(anchor: DateTime<Utc>, elapsed: Duration) -> DateTime<Utc> {
    let elapsed = chrono::Duration::from_std(elapsed).unwrap_or_else(|_| chrono::Duration::zero());
    anchor + elapsed
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Datelike, Timelike};

    #[test]
    fn parses_http_date_header_as_utc() {
        let parsed = parse_http_date("Tue, 15 Nov 1994 08:12:31 GMT").expect("date parses");

        assert_eq!(parsed.year(), 1994);
        assert_eq!(parsed.month(), 11);
        assert_eq!(parsed.day(), 15);
        assert_eq!(parsed.hour(), 8);
        assert_eq!(parsed.minute(), 12);
        assert_eq!(parsed.second(), 31);
    }

    #[test]
    fn advances_anchor_by_elapsed_duration() {
        let anchor = Utc.with_ymd_and_hms(2026, 7, 9, 10, 0, 0).single().unwrap();

        let advanced = advance_utc(anchor, Duration::from_secs(125));

        assert_eq!(advanced.hour(), 10);
        assert_eq!(advanced.minute(), 2);
        assert_eq!(advanced.second(), 5);
    }

    #[test]
    fn converts_synced_utc_to_target_timezone() {
        let mut clock = NetworkClock::new(Some("Asia/Shanghai".to_string()));
        let utc = Utc
            .with_ymd_and_hms(2026, 7, 9, 15, 30, 0)
            .single()
            .unwrap();

        clock.apply_sync(Ok(utc));
        let display = clock.current();

        assert_eq!(display.date.to_string(), "2026-07-09");
        assert_eq!(display.time.hour(), 23);
        assert_eq!(display.time.minute(), 30);
        assert!(display.warning.is_none());
    }

    #[test]
    fn unsynced_clock_uses_target_timezone_instead_of_utc_default() {
        let clock = NetworkClock::new(Some("Asia/Shanghai".to_string()));
        let expected = Utc::now().with_timezone(&chrono_tz::Asia::Shanghai);

        let display = clock.current();

        assert_eq!(display.date, expected.date_naive());
        let delta = display
            .time
            .signed_duration_since(expected.time())
            .num_seconds()
            .abs();
        assert!(delta <= 2, "display time differed by {delta} seconds");
        assert!(display.warning.is_none());
    }

    #[test]
    fn failed_sync_clears_anchor_and_reports_system_time_fallback() {
        let mut clock = NetworkClock::new(Some("UTC".to_string()));
        let utc = Utc
            .with_ymd_and_hms(2026, 7, 9, 15, 30, 0)
            .single()
            .unwrap();
        clock.apply_sync(Ok(utc));

        clock.apply_sync(Err(TimeSyncError::new(vec!["example failed".to_string()])));
        let display = clock.current();

        assert!(clock.anchor.is_none());
        assert!(
            display
                .warning
                .as_deref()
                .is_some_and(|warning| warning.contains("Time sync failed"))
        );
    }

    #[test]
    fn invalid_timezone_reports_system_time_fallback() {
        let clock = NetworkClock::new(Some("Not/AZone".to_string()));
        let display = clock.current();

        assert!(
            display
                .warning
                .as_deref()
                .is_some_and(|warning| warning.contains("Invalid timezone Not/AZone"))
        );
    }
}
