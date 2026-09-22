//! Platform-neutral contracts for system settings. No operating-system I/O lives here.
//!
//! Backends expose cached snapshots and non-blocking operation submission/polling.
//! A future adapter must schedule I/O through the shell's managed task runtime.
//! `None` means not obtained; `Some(Vec::new())` means a successfully queried empty list.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnavailableReason {
    UnsupportedPlatform,
    NotIntegrated,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Availability {
    Available,
    Unavailable(UnavailableReason),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DeviceId(pub String);

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct NetworkId(pub String);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Device {
    pub id: DeviceId,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WifiNetwork {
    pub id: NetworkId,
    pub name: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SoundSettings {
    pub outputs: Option<Vec<Device>>,
    pub inputs: Option<Vec<Device>>,
    pub selected_output: Option<DeviceId>,
    pub selected_input: Option<DeviceId>,
    pub output_volume_percent: Option<u8>,
    pub input_volume_percent: Option<u8>,
    pub output_muted: Option<bool>,
    pub input_muted: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Display {
    pub device: Device,
    pub brightness_percent: Option<u8>,
    pub automatic_brightness: Option<bool>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DisplaySettings {
    pub displays: Option<Vec<Display>>,
    pub selected_display: Option<DeviceId>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WifiSettings {
    pub enabled: Option<bool>,
    /// None: not obtained; Some(None): queried and disconnected.
    pub connection: Option<Option<WifiNetwork>>,
    pub available_networks: Option<Vec<WifiNetwork>>,
    pub saved_networks: Option<Vec<WifiNetwork>>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BluetoothSettings {
    pub enabled: Option<bool>,
    pub discoverable: Option<bool>,
    pub connected_devices: Option<Vec<Device>>,
    pub paired_devices: Option<Vec<Device>>,
    pub nearby_devices: Option<Vec<Device>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsSection<T> {
    pub availability: Availability,
    pub values: T,
}

impl<T: Default> SettingsSection<T> {
    fn unavailable(reason: UnavailableReason) -> Self {
        Self {
            availability: Availability::Unavailable(reason),
            values: T::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SystemSettingsSnapshot {
    pub sound: SettingsSection<SoundSettings>,
    pub display: SettingsSection<DisplaySettings>,
    pub wifi: SettingsSection<WifiSettings>,
    pub bluetooth: SettingsSection<BluetoothSettings>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioDirection {
    Output,
    Input,
}

/// Stable backend IDs identify targets; display labels and row indices are never IDs.
/// Percentage inputs must be validated in 0..=100 by an implementing backend.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SystemSettingsRequest {
    SelectAudioDevice {
        direction: AudioDirection,
        device: DeviceId,
    },
    SetVolume {
        direction: AudioDirection,
        device: DeviceId,
        percent: u8,
    },
    SetMute {
        direction: AudioDirection,
        device: DeviceId,
        muted: bool,
    },
    SelectDisplay(DeviceId),
    SetBrightness {
        display: DeviceId,
        percent: u8,
    },
    SetAutomaticBrightness {
        display: DeviceId,
        enabled: bool,
    },
    SetWifiEnabled(bool),
    RefreshWifi,
    ConnectWifi(NetworkId),
    DisconnectWifi(NetworkId),
    ForgetWifi(NetworkId),
    SetBluetoothEnabled(bool),
    SetBluetoothDiscoverable(bool),
    DiscoverBluetooth,
    PairBluetooth(DeviceId),
    ConnectBluetooth(DeviceId),
    DisconnectBluetooth(DeviceId),
    UnpairBluetooth(DeviceId),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct OperationId(pub u64);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OperationStatus {
    Unavailable(UnavailableReason),
    InProgress(OperationId),
    Completed,
    Failed(String),
}

pub trait SystemSettingsBackend: Send + Sync {
    /// Returns cached state without blocking on system I/O.
    fn snapshot(&self) -> SystemSettingsSnapshot;
    /// Enqueues work without blocking; unavailable backends must never report success.
    fn submit(&self, request: SystemSettingsRequest) -> OperationStatus;
    fn poll(&self, operation: OperationId) -> OperationStatus;
}

/// Production placeholder, not a simulated implementation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnavailableSystemSettingsBackend {
    pub reason: UnavailableReason,
}

impl SystemSettingsBackend for UnavailableSystemSettingsBackend {
    fn snapshot(&self) -> SystemSettingsSnapshot {
        SystemSettingsSnapshot {
            sound: SettingsSection::unavailable(self.reason),
            display: SettingsSection::unavailable(self.reason),
            wifi: SettingsSection::unavailable(self.reason),
            bluetooth: SettingsSection::unavailable(self.reason),
        }
    }

    fn submit(&self, _request: SystemSettingsRequest) -> OperationStatus {
        OperationStatus::Unavailable(self.reason)
    }

    fn poll(&self, _operation: OperationId) -> OperationStatus {
        OperationStatus::Unavailable(self.reason)
    }
}
