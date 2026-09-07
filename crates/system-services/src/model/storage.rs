use chrono::{DateTime, Utc};

/// Storage pressure ordered from least actionable to most severe.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum StoragePressure {
    Unknown,
    Normal,
    Low,
    Critical,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StorageThresholds {
    pub low_available_bytes: u64,
    pub low_percentage: u8,
    pub critical_available_bytes: u64,
    pub critical_percentage: u8,
}

impl StorageThresholds {
    pub fn classify(
        self,
        total_bytes: Option<u64>,
        available_bytes: Option<u64>,
    ) -> StoragePressure {
        let (Some(total), Some(available)) = (total_bytes, available_bytes) else {
            return StoragePressure::Unknown;
        };
        if total == 0 || available > total {
            return StoragePressure::Unknown;
        }

        let percentage_at_most = |threshold: u8| {
            u128::from(available) * 100 <= u128::from(total) * u128::from(threshold)
        };
        if available <= self.critical_available_bytes
            || percentage_at_most(self.critical_percentage)
        {
            StoragePressure::Critical
        } else if available <= self.low_available_bytes || percentage_at_most(self.low_percentage) {
            StoragePressure::Low
        } else {
            StoragePressure::Normal
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StorageVolumeKind {
    Fixed,
    Removable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StorageVolumeAccess {
    ReadWrite,
    ReadOnly,
    Unavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SystemVolumeSource {
    Detected,
    FixedVolumeFallback,
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StorageVolumeSnapshot {
    pub identifier: String,
    pub label: Option<String>,
    pub kind: StorageVolumeKind,
    pub is_system: bool,
    pub access: StorageVolumeAccess,
    pub total_bytes: Option<u64>,
    pub available_bytes: Option<u64>,
    pub pressure: StoragePressure,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StorageSnapshot {
    pub volumes: Vec<StorageVolumeSnapshot>,
    pub overall_pressure: StoragePressure,
    pub system_volume_index: Option<usize>,
    pub system_volume_source: SystemVolumeSource,
    pub sampled_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StorageState {
    Loading,
    Ready(StorageSnapshot),
    Stale {
        last_good: StorageSnapshot,
        error: String,
    },
    Unavailable {
        reason: String,
    },
}
