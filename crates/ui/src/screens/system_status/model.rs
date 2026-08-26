use crate::components::ComponentTone;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SystemStatusTab {
    #[default]
    Overview,
    Storage,
    Network,
}

impl SystemStatusTab {
    pub const ALL: [Self; 3] = [Self::Overview, Self::Storage, Self::Network];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Overview => "Overview",
            Self::Storage => "Storage",
            Self::Network => "Network",
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
            _ => 0,
        }
    }

    pub fn selected_index(&self) -> Option<usize> {
        let count = self.item_count();
        (count > 0).then(|| self.selected_row.min(count - 1))
    }

    pub const fn is_admin(&self) -> bool {
        matches!(self.content, SystemStatusContentViewModel::Admin(_))
    }
}
