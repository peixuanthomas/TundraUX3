#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DebugDiagnosticsViewModel {
    pub tick_count: u64,
    pub last_key_event: Option<String>,
    pub last_mouse_event: Option<String>,
    pub last_resize_event: Option<String>,
    pub mouse_coordinates: Option<(u16, u16)>,
    pub scroll_direction: Option<String>,
    pub drag_direction: Option<String>,
    pub terminal_flags: Vec<String>,
    pub platform_capability_summary: String,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum DiagnosticsTab {
    #[default]
    Health,
    Logs,
    Incidents,
}

impl DiagnosticsTab {
    pub const ALL: [Self; 3] = [Self::Health, Self::Logs, Self::Incidents];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Health => "Health",
            Self::Logs => "Logs",
            Self::Incidents => "Incidents",
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum DiagnosticsStatus {
    #[default]
    Pass,
    Warning,
    Fail,
}

impl DiagnosticsStatus {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Pass => "Pass",
            Self::Warning => "Warning",
            Self::Fail => "Failure",
        }
    }

    pub const fn marker(self) -> &'static str {
        match self {
            Self::Pass => "[OK]",
            Self::Warning => "[!]",
            Self::Fail => "[X]",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiagnosticsCheckViewModel {
    pub id: String,
    pub label: String,
    pub category: String,
    pub status: DiagnosticsStatus,
    pub summary: String,
    pub detail: String,
    pub remediation: String,
    pub repairable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiagnosticsIncidentViewModel {
    pub id: String,
    pub occurred_at: String,
    pub app: String,
    pub severity: DiagnosticsStatus,
    pub recovery: String,
    pub summary: String,
    pub detail: String,
    pub report_path: String,
    pub restricted: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiagnosticsLogViewModel {
    pub relative_path: String,
    pub path: String,
    pub modified_at: String,
    pub size_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiagnosticsRepairItemViewModel {
    pub id: String,
    pub label: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DiagnosticsRepairDialogViewModel {
    pub items: Vec<DiagnosticsRepairItemViewModel>,
    pub selected: usize,
    pub confirm_selected: bool,
    pub scroll_offset: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DiagnosticsViewModel {
    pub tab: DiagnosticsTab,
    pub checks: Vec<DiagnosticsCheckViewModel>,
    pub incidents: Vec<DiagnosticsIncidentViewModel>,
    pub logs: Vec<DiagnosticsLogViewModel>,
    pub selected_check: usize,
    pub selected_incident: usize,
    pub selected_log: usize,
    pub list_window_start: usize,
    /// Keeps a pointer-scrolled viewport from snapping back to the selected row.
    pub list_window_is_explicit: bool,
    pub scanning: bool,
    pub can_view_details: bool,
    pub can_repair: bool,
    pub restart_required: bool,
    pub repair_dialog: Option<DiagnosticsRepairDialogViewModel>,
    pub feedback: Option<String>,
    pub scanned_at: Option<String>,
}

impl DiagnosticsViewModel {
    pub fn selected_check(&self) -> Option<&DiagnosticsCheckViewModel> {
        self.checks.get(self.selected_check)
    }

    pub fn selected_incident(&self) -> Option<&DiagnosticsIncidentViewModel> {
        self.incidents.get(self.selected_incident)
    }

    pub fn selected_log(&self) -> Option<&DiagnosticsLogViewModel> {
        self.logs.get(self.selected_log)
    }

    pub fn item_count(&self) -> usize {
        match self.tab {
            DiagnosticsTab::Health => self.checks.len(),
            DiagnosticsTab::Incidents => self.incidents.len(),
            DiagnosticsTab::Logs if self.can_view_details => self.logs.len(),
            DiagnosticsTab::Logs => 0,
        }
    }

    pub fn selected_index(&self) -> usize {
        match self.tab {
            DiagnosticsTab::Health => self.selected_check,
            DiagnosticsTab::Incidents => self.selected_incident,
            DiagnosticsTab::Logs => self.selected_log,
        }
    }
}
