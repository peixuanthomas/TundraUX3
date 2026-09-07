//! Time snapshots shared with displays.

use chrono::{DateTime, FixedOffset, Utc};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TimeSource {
    OperatingSystem,
    Network(String),
}
pub type LocalTime = DateTime<FixedOffset>;
#[derive(Debug, Clone, PartialEq)]
pub enum TimeState {
    Local {
        local_time: LocalTime,
    },
    Synced {
        utc: DateTime<Utc>,
        local_time: LocalTime,
        source: TimeSource,
        sampled_at: DateTime<Utc>,
    },
    Degraded {
        local_time: LocalTime,
        last_sync: Option<DateTime<Utc>>,
        error: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TimeSyncMode {
    OperatingSystem,
    Network,
}
