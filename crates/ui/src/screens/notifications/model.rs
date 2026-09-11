pub use app::{NotificationLevel, NotificationTone};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotificationActionViewModel {
    pub id: String,
    pub label: String,
    pub shortcut: Option<String>,
    pub selected: bool,
}

impl NotificationActionViewModel {
    pub fn new(id: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            shortcut: None,
            selected: false,
        }
    }

    pub fn with_shortcut(mut self, shortcut: impl Into<String>) -> Self {
        self.shortcut = Some(shortcut.into());
        self
    }

    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotificationViewModel {
    pub id: String,
    pub level: NotificationLevel,
    pub tone: NotificationTone,
    pub title: String,
    pub message: String,
    pub actions: Vec<NotificationActionViewModel>,
    pub stacked_actions: bool,
    /// First wrapped message line to show; layout clamps it to the visible range.
    pub scroll_offset: usize,
}

impl NotificationViewModel {
    pub fn new(
        id: impl Into<String>,
        level: NotificationLevel,
        tone: NotificationTone,
        title: impl Into<String>,
        message: impl Into<String>,
        actions: Vec<NotificationActionViewModel>,
    ) -> Self {
        Self {
            id: id.into(),
            level,
            tone,
            title: title.into(),
            message: message.into(),
            actions,
            stacked_actions: false,
            scroll_offset: 0,
        }
    }
}

pub(crate) fn notification_title(model: &NotificationViewModel) -> String {
    format!("{} {}", notification_tone_prefix(model.tone), model.title)
}

pub(crate) fn notification_tone_prefix(tone: NotificationTone) -> String {
    match tone {
        NotificationTone::Info => i18n::tr!("ui-notifications-info-button"),
        NotificationTone::Success => i18n::tr!("ui-notifications-success-button"),
        NotificationTone::Warning => i18n::tr!("ui-notifications-warn-button"),
        NotificationTone::Error => i18n::tr!("ui-notifications-error-button"),
        NotificationTone::Critical => i18n::tr!("ui-notifications-critical-button"),
    }
}
