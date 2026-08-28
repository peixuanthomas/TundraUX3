use crate::components::ComponentTone;
use crate::{DiagnosticsTab, DiagnosticsViewModel};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum SystemStatusWidgetKind {
    #[default]
    SystemOverview,
    Cpu,
    Memory,
    Storage,
    Network,
    Temperature,
    Battery,
    UptimeLoad,
    TopProcesses,
    Diagnostics,
    Activity,
}
impl SystemStatusWidgetKind {
    pub const ALL: [Self; 11] = [
        Self::SystemOverview,
        Self::Cpu,
        Self::Memory,
        Self::Storage,
        Self::Network,
        Self::Temperature,
        Self::Battery,
        Self::UptimeLoad,
        Self::TopProcesses,
        Self::Diagnostics,
        Self::Activity,
    ];
    pub const fn label(self) -> &'static str {
        match self {
            Self::SystemOverview => "System Overview",
            Self::Cpu => "CPU",
            Self::Memory => "Memory",
            Self::Storage => "Storage",
            Self::Network => "Network",
            Self::Temperature => "Temperature",
            Self::Battery => "Battery",
            Self::UptimeLoad => "Uptime & Load",
            Self::TopProcesses => "Top Processes",
            Self::Diagnostics => "Diagnostics",
            Self::Activity => "Activity",
        }
    }
    pub const fn detail(self) -> SystemStatusDetail {
        match self {
            Self::SystemOverview => SystemStatusDetail::Overview,
            Self::Cpu => SystemStatusDetail::Cpu,
            Self::Memory => SystemStatusDetail::Memory,
            Self::Storage => SystemStatusDetail::Storage,
            Self::Network => SystemStatusDetail::Network,
            Self::Temperature => SystemStatusDetail::Thermal,
            Self::Battery => SystemStatusDetail::Power,
            Self::UptimeLoad => SystemStatusDetail::UptimeLoad,
            Self::TopProcesses => SystemStatusDetail::Processes,
            Self::Diagnostics => SystemStatusDetail::Diagnostics,
            Self::Activity => SystemStatusDetail::Activity,
        }
    }
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SystemStatusWidgetSize {
    #[default]
    Small,
    Wide,
    Large,
}
impl SystemStatusWidgetSize {
    pub const fn cols(self) -> u16 {
        match self {
            Self::Small => 2,
            _ => 4,
        }
    }
    pub const fn rows(self) -> u16 {
        match self {
            Self::Large => 4,
            _ => 2,
        }
    }
    pub const fn label(self) -> &'static str {
        match self {
            Self::Small => "2x2",
            Self::Wide => "2x4",
            Self::Large => "4x4",
        }
    }
    pub const fn cycle(self) -> Self {
        match self {
            Self::Small => Self::Wide,
            Self::Wide => Self::Large,
            Self::Large => Self::Small,
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum SystemStatusWidgetState {
    #[default]
    Loading,
    Ready,
    Stale {
        message: String,
    },
    Unavailable {
        message: String,
    },
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SystemStatusBarItem {
    pub label: String,
    pub value: u64,
}
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SystemStatusWidgetViewModel {
    pub kind: SystemStatusWidgetKind,
    pub size: SystemStatusWidgetSize,
    pub column: u16,
    pub row: u16,
    pub state: SystemStatusWidgetState,
    pub tone: ComponentTone,
    pub primary: String,
    pub secondary: Vec<String>,
    pub trend: Option<Vec<u64>>,
    pub progress_percent: Option<u16>,
    pub bars: Vec<SystemStatusBarItem>,
    pub compact_rows: Vec<Vec<String>>,
    pub openable: bool,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SystemStatusDashboardProfile {
    #[default]
    Wide,
    Narrow,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SystemStatusDashboardFocus {
    Widget(SystemStatusWidgetKind),
    Edit,
    Refresh,
    Add,
    Size,
    Remove,
    Save,
    Cancel,
}
impl Default for SystemStatusDashboardFocus {
    fn default() -> Self {
        Self::Widget(SystemStatusWidgetKind::SystemOverview)
    }
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SystemStatusActionState {
    pub refresh_disabled: bool,
    pub edit_disabled: bool,
    pub add_disabled: bool,
    pub size_disabled: bool,
    pub remove_disabled: bool,
    pub save_disabled: bool,
    pub cancel_disabled: bool,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SystemStatusPickerItemViewModel {
    pub kind: SystemStatusWidgetKind,
    pub label: String,
    pub detail: String,
    pub enabled: bool,
}
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SystemStatusPickerViewModel {
    pub title: String,
    pub items: Vec<SystemStatusPickerItemViewModel>,
    pub selected: usize,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SystemStatusSizePickerViewModel {
    pub selected: usize,
}
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SystemStatusDialogViewModel {
    pub title: String,
    pub message: String,
    pub confirm_label: String,
    pub cancel_label: String,
    pub selected_action: usize,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SystemStatusDragPreview {
    pub kind: SystemStatusWidgetKind,
    pub column: u16,
    pub row: u16,
    pub valid: bool,
}
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SystemStatusDashboardViewModel {
    pub wide_widgets: Vec<SystemStatusWidgetViewModel>,
    pub narrow_widgets: Vec<SystemStatusWidgetViewModel>,
    pub selected: Option<SystemStatusWidgetKind>,
    pub focus: SystemStatusDashboardFocus,
    pub scroll_row: u16,
    pub editing: bool,
    pub dirty: bool,
    pub feedback: Option<String>,
    pub picker: Option<SystemStatusPickerViewModel>,
    pub size_picker: Option<SystemStatusSizePickerViewModel>,
    pub dialog: Option<SystemStatusDialogViewModel>,
    pub dragging: Option<SystemStatusDragPreview>,
    pub actions: SystemStatusActionState,
    pub updated: String,
}
impl SystemStatusDashboardViewModel {
    pub fn widgets(&self, p: SystemStatusDashboardProfile) -> &[SystemStatusWidgetViewModel] {
        match p {
            SystemStatusDashboardProfile::Wide => &self.wide_widgets,
            SystemStatusDashboardProfile::Narrow => &self.narrow_widgets,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SystemStatusDetail {
    #[default]
    Overview,
    Cpu,
    Memory,
    Storage,
    Network,
    Thermal,
    Power,
    UptimeLoad,
    Processes,
    Diagnostics,
    Activity,
}
impl SystemStatusDetail {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Overview => "Overview",
            Self::Cpu => "CPU",
            Self::Memory => "Memory",
            Self::Storage => "Storage",
            Self::Network => "Network",
            Self::Thermal => "Thermal",
            Self::Power => "Power",
            Self::UptimeLoad => "Uptime & Load",
            Self::Processes => "Processes",
            Self::Diagnostics => "Diagnostics",
            Self::Activity => "Activity",
        }
    }
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SystemStatusRoute {
    #[default]
    Dashboard,
    Detail(SystemStatusDetail),
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SystemStatusTab {
    #[default]
    Overview,
    Storage,
    Network,
    Health,
    Logs,
    Incidents,
}
impl SystemStatusTab {
    pub const ALL: [Self; 6] = [
        Self::Overview,
        Self::Storage,
        Self::Network,
        Self::Health,
        Self::Logs,
        Self::Incidents,
    ];
    pub const USER: [Self; 4] = [Self::Overview, Self::Health, Self::Logs, Self::Incidents];
    pub const fn label(self) -> &'static str {
        match self {
            Self::Overview => "Overview",
            Self::Storage => "Storage",
            Self::Network => "Network",
            Self::Health => "Health",
            Self::Logs => "Logs",
            Self::Incidents => "Incidents",
        }
    }
    pub const fn diagnostics_tab(self) -> Option<DiagnosticsTab> {
        match self {
            Self::Health => Some(DiagnosticsTab::Health),
            Self::Logs => Some(DiagnosticsTab::Logs),
            Self::Incidents => Some(DiagnosticsTab::Incidents),
            _ => None,
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SystemStatusSectionState {
    Loading,
    Ready,
    Stale { message: String },
    Unavailable { message: String },
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SystemStatusOverviewViewModel {
    pub storage_status: String,
    pub storage_tone: ComponentTone,
    pub system_volume_usage: String,
    pub system_volume_used_percentage: Option<u8>,
    pub network_status: String,
    pub network_tone: ComponentTone,
    pub active_link_count: String,
    pub last_refreshed: String,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StorageVolumeRowViewModel {
    pub volume: String,
    pub kind: String,
    pub system_volume: String,
    pub access: String,
    pub usage: String,
    pub used_percentage: String,
    pub pressure: String,
    pub tone: ComponentTone,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetworkInterfaceRowViewModel {
    pub name: String,
    pub display_name: String,
    pub kind: String,
    pub link_state: String,
    pub received_rate: String,
    pub transmitted_rate: String,
    pub addresses: String,
    pub tone: ComponentTone,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdminSystemStatusViewModel {
    pub overview: SystemStatusOverviewViewModel,
    pub storage_state: SystemStatusSectionState,
    pub storage_rows: Vec<StorageVolumeRowViewModel>,
    pub network_state: SystemStatusSectionState,
    pub network_rows: Vec<NetworkInterfaceRowViewModel>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserSystemStatusViewModel {
    pub storage_status: String,
    pub storage_tone: ComponentTone,
    pub system_volume_usage: String,
    pub system_volume_used_percentage: Option<u8>,
    pub network_status: String,
    pub network_tone: ComponentTone,
    pub last_refreshed: String,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SystemStatusContentViewModel {
    Admin(AdminSystemStatusViewModel),
    User(UserSystemStatusViewModel),
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SystemStatusViewModel {
    pub content: SystemStatusContentViewModel,
    pub diagnostics: DiagnosticsViewModel,
    pub route: SystemStatusRoute,
    pub dashboard: SystemStatusDashboardViewModel,
    pub selected_row: usize,
    pub scroll_offset: usize,
    pub refreshing: bool,
    pub feedback: Option<String>,
}
impl SystemStatusViewModel {
    pub const fn activity_tab(&self) -> DiagnosticsTab {
        match self.diagnostics.tab {
            DiagnosticsTab::Incidents => DiagnosticsTab::Incidents,
            DiagnosticsTab::Health | DiagnosticsTab::Logs => DiagnosticsTab::Logs,
        }
    }
    pub fn detail_widget(&self, d: SystemStatusDetail) -> Option<&SystemStatusWidgetViewModel> {
        self.dashboard
            .wide_widgets
            .iter()
            .chain(&self.dashboard.narrow_widgets)
            .find(|w| w.kind.detail() == d)
    }
    pub fn item_count(&self) -> usize {
        match (&self.content, self.route) {
            (
                SystemStatusContentViewModel::Admin(a),
                SystemStatusRoute::Detail(SystemStatusDetail::Storage),
            ) => a.storage_rows.len(),
            (
                SystemStatusContentViewModel::Admin(a),
                SystemStatusRoute::Detail(SystemStatusDetail::Network),
            ) => a.network_rows.len(),
            (
                _,
                SystemStatusRoute::Detail(
                    SystemStatusDetail::Diagnostics | SystemStatusDetail::Activity,
                ),
            ) => self.diagnostics.item_count(),
            (_, SystemStatusRoute::Detail(detail)) => self
                .detail_widget(detail)
                .map(|widget| widget.compact_rows.len())
                .unwrap_or(0),
            _ => 0,
        }
    }
    pub fn selected_index(&self) -> Option<usize> {
        let n = self.item_count();
        (n > 0).then(|| self.selected_row.min(n - 1))
    }
    pub const fn is_admin(&self) -> bool {
        matches!(self.content, SystemStatusContentViewModel::Admin(_))
    }
}
