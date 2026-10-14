use std::{
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

pub struct ReplWatchdog {
    runtime: Option<watchdog::WatchdogRuntime>,
    pub process: watchdog::ProcessWatchdog,
    pub app: watchdog::AppWatchdog,
    root: PathBuf,
}

impl ReplWatchdog {
    pub fn new(root: Option<PathBuf>) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = root.unwrap_or_else(|| {
            std::env::temp_dir().join(format!(
                "tundra-repl-worker-{}-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ))
        });
        let config = watchdog::WatchdogConfig::new(
            root.join("reports"),
            root.join("fallback"),
            root.join("data"),
            "repl-test",
            env!("CARGO_PKG_VERSION"),
        );
        let (runtime, process) = watchdog::WatchdogRuntime::start_isolated(config).unwrap();
        let app = process
            .register_app(watchdog::AppDescriptor::new(
                watchdog::AppId::from_static("cli"),
                "CLI test",
                env!("CARGO_PKG_VERSION"),
                watchdog::AppCriticality::Optional,
            ))
            .unwrap();
        Self {
            runtime: Some(runtime),
            process,
            app,
            root,
        }
    }
}

impl Drop for ReplWatchdog {
    fn drop(&mut self) {
        self.runtime.take().unwrap().shutdown().unwrap();
        std::fs::remove_dir_all(&self.root).unwrap();
    }
}
