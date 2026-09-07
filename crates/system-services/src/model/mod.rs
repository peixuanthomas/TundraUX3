//! Shared data types; this module performs no network or platform I/O.

mod metrics;
mod network;
mod storage;
mod time;
mod weather;

pub use metrics::*;
pub use network::*;
pub use storage::*;
pub use time::*;
pub use weather::*;

use chrono::{DateTime, Utc};

#[derive(Debug, Clone, PartialEq)]
pub struct SystemSnapshot {
    pub revision: u64,
    pub observed_at: DateTime<Utc>,
    pub weather: WeatherState,
    pub time: TimeState,
    pub storage: StorageState,
    pub network: NetworkState,
    pub metrics: SystemMetricsSnapshot,
}

#[cfg(test)]
mod tests;
