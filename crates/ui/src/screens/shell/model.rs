use crate::NotificationTone;
use crate::screens::home::HomeDisplayMode;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusViewModel {
    pub status: String,
    pub toast: Option<String>,
    pub error: Option<String>,
    pub alert_tone: NotificationTone,
    pub time_button_label: Option<String>,
    pub time_button_selected: bool,
}

impl StatusViewModel {
    /// The full message chosen for the status bar, before flattening or truncation.
    pub fn full_message(&self) -> String {
        if let Some(alert) = &self.error {
            return format!(
                "{} {alert}",
                crate::screens::notifications::notification_tone_prefix(self.alert_tone)
            );
        }
        self.toast.as_ref().unwrap_or(&self.status).clone()
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TimeSyncDialogViewModel;

impl TimeSyncDialogViewModel {
    pub fn new() -> Self {
        Self
    }

    pub fn message(&self) -> String {
        i18n::tr!("ui-shell-time-sync-failed")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellChromeViewModel {
    pub app_name: String,
    pub build_mode: String,
    pub display_mode: HomeDisplayMode,
    pub terminal_size: (u16, u16),
    pub screen_stack: Vec<String>,
    pub back_button_hovered: bool,
    pub status: StatusViewModel,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExitConfirmViewModel {
    pub title: String,
    pub message: String,
    pub confirm_label: String,
    pub restart_label: String,
    pub cancel_label: String,
}

impl ExitConfirmViewModel {
    pub fn new() -> Self {
        Self {
            title: i18n::tr!("ui-shell-exit-power"),
            message: i18n::tr!("ui-shell-choose-an-action-esc-returns-to-tundraux"),
            confirm_label: i18n::tr!("ui-shell-y-enter-exit-tundraux"),
            restart_label: i18n::tr!("ui-shell-r-restart-tundraux"),
            cancel_label: i18n::tr!("ui-shell-n-esc-cancel"),
        }
    }
}

impl Default for ExitConfirmViewModel {
    fn default() -> Self {
        Self::new()
    }
}
