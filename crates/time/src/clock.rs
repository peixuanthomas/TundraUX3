//! A synchronized UTC anchor projected into the selected timezone.

use crate::TimeSyncResult;
use chrono::{DateTime, Local, NaiveDate, NaiveTime, Utc};
use chrono_tz::Tz;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClockDisplay {
    pub date: NaiveDate,
    pub time: NaiveTime,
    pub warning: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClockSnapshot {
    pub utc: DateTime<Utc>,
    pub date: NaiveDate,
    pub time: NaiveTime,
    pub timezone: Option<Tz>,
    pub warning: Option<String>,
}

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
                let fallback = if self.anchor.is_some() {
                    "continuing last synchronized time"
                } else {
                    "using system time"
                };
                self.sync_error = Some(format!("Time sync failed: {error}; {fallback}"));
            }
        }
    }

    pub fn current(&self) -> ClockDisplay {
        let snapshot = self.snapshot();
        ClockDisplay {
            date: snapshot.date,
            time: snapshot.time,
            warning: snapshot.warning,
        }
    }

    pub fn snapshot(&self) -> ClockSnapshot {
        if let Some(timezone) = self.timezone {
            let utc = self
                .anchor
                .as_ref()
                .map(TimeAnchor::current_utc)
                .unwrap_or_else(Utc::now);
            let local = utc.with_timezone(&timezone);
            return ClockSnapshot {
                utc,
                date: local.date_naive(),
                time: local.time(),
                timezone: Some(timezone),
                warning: self.warning(),
            };
        }

        let local = Local::now();
        ClockSnapshot {
            utc: local.with_timezone(&Utc),
            date: local.date_naive(),
            time: local.time(),
            timezone: None,
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

fn advance_utc(anchor: DateTime<Utc>, elapsed: Duration) -> DateTime<Utc> {
    let elapsed = chrono::Duration::from_std(elapsed).unwrap_or_else(|_| chrono::Duration::zero());
    anchor + elapsed
}

#[cfg(test)]
#[path = "tests/clock.rs"]
mod tests;
