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

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TimeSyncDialogViewModel;

impl TimeSyncDialogViewModel {
    pub const MESSAGE: &'static str = "联网校准时间失败";

    pub fn new() -> Self {
        Self
    }

    pub fn message(&self) -> &'static str {
        Self::MESSAGE
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellChromeViewModel {
    pub app_name: String,
    pub build_mode: String,
    pub display_mode: HomeDisplayMode,
    pub terminal_size: (u16, u16),
    pub screen_stack: Vec<String>,
    pub status: StatusViewModel,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExitConfirmViewModel {
    pub title: String,
    pub message: String,
    pub confirm_label: String,
    pub cancel_label: String,
}

impl ExitConfirmViewModel {
    pub fn new() -> Self {
        Self {
            title: "Exit TundraUX 3".to_string(),
            message: "Leave the shell and restore the terminal?".to_string(),
            confirm_label: "Y / Enter: exit".to_string(),
            cancel_label: "N / Esc: cancel".to_string(),
        }
    }
}

impl Default for ExitConfirmViewModel {
    fn default() -> Self {
        Self::new()
    }
}
