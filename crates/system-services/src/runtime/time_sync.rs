//! Time synchronization, timezone conversion and monotonic time tracking.

use super::{SystemServicesConfig, SystemServicesError};
use crate::{LocalTime, TimeSource, TimeState, TimeSyncMode};
use chrono::{DateTime, Utc};
use chrono_tz::Tz;
use std::time::Instant;

fn parse_timezone(timezone: &str) -> Option<Tz> {
    timezone.parse::<Tz>().ok()
}
pub(super) fn local_time_at(timezone: &str, utc: DateTime<Utc>) -> LocalTime {
    parse_timezone(timezone)
        .map(|tz| utc.with_timezone(&tz).fixed_offset())
        .unwrap_or_else(|| utc.fixed_offset())
}
pub(super) fn current_time_state(
    config: &SystemServicesConfig,
    anchor: Option<&TimeAnchor>,
    error: Option<&str>,
    now: Instant,
) -> TimeState {
    let utc = anchor.map_or_else(Utc::now, |anchor| anchor.utc_at(now));
    let local = local_time_at(&config.timezone_id, utc);
    if let Some(error) = error {
        return TimeState::Degraded {
            local_time: local,
            last_sync: anchor.map(|anchor| anchor.utc),
            error: error.to_string(),
        };
    }
    match anchor {
        Some(anchor) => TimeState::Synced {
            utc,
            local_time: local,
            source: anchor.source.clone(),
            sampled_at: anchor.sampled_at,
        },
        None => TimeState::Local { local_time: local },
    }
}

#[derive(Debug, Clone)]
pub(super) struct TimeAnchor {
    pub(super) utc: DateTime<Utc>,
    pub(super) sampled_at: DateTime<Utc>,
    pub(super) instant: Instant,
    pub(super) source: TimeSource,
}
impl TimeAnchor {
    pub(super) fn utc_at(&self, now: Instant) -> DateTime<Utc> {
        self.utc + now.saturating_duration_since(self.instant)
    }
}

pub(super) async fn synchronize_time(
    config: &SystemServicesConfig,
) -> Result<(DateTime<Utc>, TimeSource), String> {
    match config.time_sync_mode {
        TimeSyncMode::OperatingSystem => Ok((Utc::now(), TimeSource::OperatingSystem)),
        TimeSyncMode::Network => {
            let result = match config.time_server_url.as_deref() {
                Some(url) => time::fetch_time_from_server(url).await,
                None => time::fetch_standard_time().await,
            };
            result
                .map(|utc| {
                    (
                        utc,
                        TimeSource::Network(
                            config
                                .time_server_url
                                .clone()
                                .unwrap_or_else(|| "standard".to_string()),
                        ),
                    )
                })
                .map_err(|error| error.to_string())
        }
    }
}
pub(super) async fn validate_time(
    config: &SystemServicesConfig,
) -> Result<DateTime<Utc>, SystemServicesError> {
    synchronize_time(config)
        .await
        .map(|(utc, _)| utc)
        .map_err(SystemServicesError::Validation)
}
