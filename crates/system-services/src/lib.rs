//! Shared weather, time and system-status snapshots.
//!
//! Enable the `runtime` feature to start the process-wide services that own
//! network I/O, platform sampling and cache files. Data-only consumers can use
//! this crate without that feature or its service dependencies.

mod model;
pub use model::*;

#[cfg(feature = "runtime")]
mod runtime;
#[cfg(feature = "runtime")]
pub use runtime::{
    MetOfficeProvider, OpenMeteoProvider, SystemServicesConfig, SystemServicesError,
    SystemServicesHandle, SystemServicesRuntime, WeatherProvider, normalize_open_meteo_code,
};
