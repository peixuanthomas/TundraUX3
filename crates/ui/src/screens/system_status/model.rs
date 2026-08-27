use crate::components::ComponentTone;
use crate::{DiagnosticsTab, DiagnosticsViewModel};

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

    pub const fn compact_label(self) -> &'static str {
        match self {
            Self::Overview => "Info",
            Self::Storage => "Disk",
            Self::Network => "Net",
            Self::Health => "Health",
            Self::Logs => "Logs",
            Self::Incidents => "Events",
        }
    }

    pub const fn diagnostics_tab(self) -> Option<DiagnosticsTab> {
        match self {
            Self::Health => Some(DiagnosticsTab::Health),
            Self::Logs => Some(DiagnosticsTab::Logs),
            Self::Incidents => Some(DiagnosticsTab::Incidents),
            Self::Overview | Self::Storage | Self::Network => None,
        }
    }

    pub const fn from_diagnostics(tab: DiagnosticsTab) -> Self {
        match tab {
            DiagnosticsTab::Health => Self::Health,
            DiagnosticsTab::Logs => Self::Logs,
            DiagnosticsTab::Incidents => Self::Incidents,
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
    pub tab: SystemStatusTab,
    pub selected_row: usize,
    pub scroll_offset: usize,
    pub refreshing: bool,
    pub feedback: Option<String>,
}

impl SystemStatusViewModel {
    pub fn item_count(&self) -> usize {
        match (&self.content, self.tab) {
            (SystemStatusContentViewModel::Admin(admin), SystemStatusTab::Storage) => {
                admin.storage_rows.len()
            }
            (SystemStatusContentViewModel::Admin(admin), SystemStatusTab::Network) => {
                admin.network_rows.len()
            }
            (_, SystemStatusTab::Health | SystemStatusTab::Logs | SystemStatusTab::Incidents) => {
                self.diagnostics.item_count()
            }
            _ => 0,
        }
    }

    pub fn selected_index(&self) -> Option<usize> {
        let count = self.item_count();
        (count > 0).then(|| {
            if self.is_diagnostics() {
                self.diagnostics.selected_index().min(count - 1)
            } else {
                self.selected_row.min(count - 1)
            }
        })
    }

    pub const fn is_admin(&self) -> bool {
        matches!(self.content, SystemStatusContentViewModel::Admin(_))
    }

    pub fn tabs(&self) -> &'static [SystemStatusTab] {
        if self.is_admin() {
            &SystemStatusTab::ALL
        } else {
            &SystemStatusTab::USER
        }
    }

    pub const fn is_diagnostics(&self) -> bool {
        self.tab.diagnostics_tab().is_some()
    }
}
