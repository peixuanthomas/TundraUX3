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
        }
    }
}
