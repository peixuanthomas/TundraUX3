pub mod diagnostics;
pub mod explorer;
pub mod explorer_tasks;

#[derive(Clone)]
pub struct AppRuntimeContext {
    pub watchdog: tundra_watchdog::AppWatchdog,
}

impl AppRuntimeContext {
    pub fn new(watchdog: tundra_watchdog::AppWatchdog) -> Self {
        Self { watchdog }
    }

    pub fn component(&self, id: &str) -> tundra_watchdog::AppWatchdog {
        self.watchdog.child_component(
            tundra_watchdog::ComponentId::new(id)
                .expect("App component identifiers must be static, validated identifiers"),
        )
    }

    pub fn task_group(&self, id: &str) -> tundra_watchdog::ManagedTaskGroup {
        self.watchdog.task_group(id)
    }
}
