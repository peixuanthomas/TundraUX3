use crate::DiagnosticsViewModel;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum LogsCategory {
    #[default]
    Ux,
    Linux,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum LogsSection {
    #[default]
    Events,
    Files,
    Incidents,
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LogsEventViewModel {
    pub id: String,
    pub timestamp: String,
    pub level: String,
    pub module: String,
    pub operation: String,
    pub summary: String,
    pub detail: String,
    pub incident_id: Option<String>,
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LogsViewModel {
    pub category: LogsCategory,
    pub section: LogsSection,
    pub diagnostics: DiagnosticsViewModel,
    pub events: Vec<LogsEventViewModel>,
    pub selected_event: usize,
    pub scroll_offset: usize,
    pub linux_available: bool,
    pub can_view_system: bool,
    pub loading: bool,
    pub filter_summary: String,
    pub feedback: Option<String>,
}
