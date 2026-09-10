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

/// Forward metadata-only renderer fallbacks to the host's runtime logger. A
/// data-only consumer has no logging side effects or additional dependencies.
pub fn record_render_fallback(operation: &str, message: &str) {
    #[cfg(feature = "runtime")]
    {
        let current = watchdog::AppWatchdog::current();
        let mut context = current
            .as_ref()
            .map(|app| app.log_context(operation))
            .unwrap_or_default();
        context.module = "ux.weathr".into();
        context.operation = operation.into();
        let mut event = runtime_log::RuntimeLogEvent::new(
            context,
            runtime_log::LogLevel::Warning,
            runtime_log::LogPhase::Degraded,
            message,
        );
        event.alert_key = Some(operation.into());
        if let Some(app) = current {
            app.record_log(event);
        } else {
            runtime_log::record(event);
        }
    }
    #[cfg(not(feature = "runtime"))]
    let _ = (operation, message);
}
