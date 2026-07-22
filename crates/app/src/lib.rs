pub mod application;
pub mod diagnostics;
pub mod editor;
pub mod explorer;
pub mod launcher;

pub use editor::markdown_codec;
pub use editor::recovery as editor_recovery;
pub use editor::rich_document;
pub use editor::rich_edit;
pub use explorer::tasks as explorer_tasks;

pub use application::catalog::{
    SetupLanguageOption, SetupTimezoneOption, setup_language_options, setup_timezone_options,
};
pub use application::notification::{
    DEFAULT_ALERT_KEY, DEFAULT_TOAST_DURATION, MAX_ACTIVE_ALERTS, MAX_NOTIFICATION_RESPONSES,
    Notification, NotificationAction, NotificationCenter, NotificationCommand, NotificationLevel,
    NotificationResponse, NotificationTone,
};
pub use application::{AppAction, AppCommand, AppSnapshot, AppState};

#[derive(Clone)]
pub struct AppRuntimeContext {
    pub watchdog: watchdog::AppWatchdog,
}

impl AppRuntimeContext {
    pub fn new(watchdog: watchdog::AppWatchdog) -> Self {
        Self { watchdog }
    }

    pub fn component(&self, id: &str) -> watchdog::AppWatchdog {
        self.watchdog.child_component(
            watchdog::ComponentId::new(id)
                .expect("App component identifiers must be static, validated identifiers"),
        )
    }

    pub fn task_group(&self, id: &str) -> watchdog::ManagedTaskGroup {
        self.watchdog.task_group(id)
    }
}
