//! Network-synchronized clocks and HTTP time sources.

mod clock;
mod network;

pub use clock::{ClockDisplay, ClockSnapshot, NetworkClock};
pub use network::{
    MAX_TIME_SERVER_URL_LEN, TimeSyncError, TimeSyncResult, fetch_standard_time,
    fetch_time_from_server, normalize_time_server_url,
};

pub const TIME_SYNC_INTERVAL: std::time::Duration = std::time::Duration::from_secs(5 * 60);
