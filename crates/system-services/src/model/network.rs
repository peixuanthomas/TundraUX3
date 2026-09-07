//! Network interface and link snapshots.

use chrono::{DateTime, Utc};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetworkInterfaceKind {
    Wired,
    Wireless,
    Virtual,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetworkLinkState {
    Up,
    Down,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetworkInterfaceSnapshot {
    pub name: String,
    pub display_name: Option<String>,
    pub kind: NetworkInterfaceKind,
    pub link_state: NetworkLinkState,
    pub addresses: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetworkSnapshot {
    pub interfaces: Vec<NetworkInterfaceSnapshot>,
    pub active_link_count: usize,
    pub has_active_link: bool,
    pub sampled_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NetworkState {
    Loading,
    Ready(NetworkSnapshot),
    Stale {
        last_good: NetworkSnapshot,
        error: String,
    },
    Unavailable {
        reason: String,
    },
}
