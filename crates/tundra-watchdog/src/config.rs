use std::path::PathBuf;
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetentionPolicy {
    pub max_incidents: usize,
    pub max_age: Duration,
    pub max_total_bytes: u64,
    pub emergency_log_max_bytes: u64,
}

impl Default for RetentionPolicy {
    fn default() -> Self {
        Self {
            max_incidents: 30,
            max_age: Duration::from_secs(30 * 24 * 60 * 60),
            max_total_bytes: 50 * 1024 * 1024,
            emergency_log_max_bytes: 1024 * 1024,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WatchdogConfig {
    pub report_dir: PathBuf,
    pub fallback_dir: PathBuf,
    pub data_dir: PathBuf,
    pub process_name: String,
    pub process_version: String,
    pub breadcrumb_capacity: usize,
    pub heartbeat_flush_interval: Duration,
    pub task_shutdown_timeout: Duration,
    pub retention: RetentionPolicy,
}

impl WatchdogConfig {
    pub fn new(
        report_dir: impl Into<PathBuf>,
        fallback_dir: impl Into<PathBuf>,
        data_dir: impl Into<PathBuf>,
        process_name: impl Into<String>,
        process_version: impl Into<String>,
    ) -> Self {
        Self {
            report_dir: report_dir.into(),
            fallback_dir: fallback_dir.into(),
            data_dir: data_dir.into(),
            process_name: process_name.into(),
            process_version: process_version.into(),
            breadcrumb_capacity: 128,
            heartbeat_flush_interval: Duration::from_secs(5),
            task_shutdown_timeout: Duration::from_secs(2),
            retention: RetentionPolicy::default(),
        }
    }
}

impl Default for WatchdogConfig {
    fn default() -> Self {
        let root = std::env::temp_dir().join("TundraUX3").join("watchdog");
        Self::new(
            root.join("crashes"),
            root.join("fallback"),
            root.join("state"),
            "tundra-process",
            env!("CARGO_PKG_VERSION"),
        )
    }
}
