#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HomeDisplayMode {
    Debug,
    User,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusViewModel {
    pub status: String,
    pub toast: Option<String>,
    pub error: Option<String>,
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
pub struct ShellEntry {
    pub label: String,
    pub description: String,
}

impl ShellEntry {
    pub fn new(label: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            description: description.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DebugDiagnosticsViewModel {
    pub tick_count: u64,
    pub last_key_event: Option<String>,
    pub last_mouse_event: Option<String>,
    pub last_resize_event: Option<String>,
    pub mouse_coordinates: Option<(u16, u16)>,
    pub scroll_direction: Option<String>,
    pub terminal_flags: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HomeViewModel {
    display_mode: HomeDisplayMode,
    diagnostics: Option<DebugDiagnosticsViewModel>,
    pub(crate) current_user: Option<String>,
    pub(crate) current_time: Option<String>,
    entries: Vec<ShellEntry>,
}

impl HomeViewModel {
    pub fn debug(diagnostics: DebugDiagnosticsViewModel) -> Self {
        Self {
            display_mode: HomeDisplayMode::Debug,
            diagnostics: Some(diagnostics),
            current_user: None,
            current_time: None,
            entries: Vec::new(),
        }
    }

    pub fn user(
        current_user: impl Into<String>,
        current_time: impl Into<String>,
        entries: Vec<ShellEntry>,
    ) -> Self {
        Self {
            display_mode: HomeDisplayMode::User,
            diagnostics: None,
            current_user: Some(current_user.into()),
            current_time: Some(current_time.into()),
            entries,
        }
    }

    pub fn display_mode(&self) -> HomeDisplayMode {
        self.display_mode
    }

    pub fn diagnostics(&self) -> Option<&DebugDiagnosticsViewModel> {
        self.diagnostics.as_ref()
    }

    pub fn entries(&self) -> &[ShellEntry] {
        &self.entries
    }
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
