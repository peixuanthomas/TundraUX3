use super::super::*;
use app::system_settings::{Availability, SystemSettingsBackend, UnavailableReason};

pub(super) const SOUND_FIELDS: &[ui::SettingsField] = &[
    ui::SettingsField::SoundOutputDevice,
    ui::SettingsField::SoundOutputVolume,
    ui::SettingsField::SoundOutputMute,
    ui::SettingsField::SoundInputDevice,
    ui::SettingsField::SoundInputVolume,
    ui::SettingsField::SoundInputMute,
];

pub(super) const DISPLAY_FIELDS: &[ui::SettingsField] = &[
    ui::SettingsField::DisplayDevice,
    ui::SettingsField::DisplayBrightness,
    ui::SettingsField::DisplayAutomaticBrightness,
];

pub(super) const WIFI_FIELDS: &[ui::SettingsField] = &[
    ui::SettingsField::WifiEnabled,
    ui::SettingsField::WifiConnection,
    ui::SettingsField::WifiAvailableNetworks,
    ui::SettingsField::WifiSavedNetworks,
    ui::SettingsField::WifiRefresh,
    ui::SettingsField::WifiConnect,
    ui::SettingsField::WifiDisconnect,
    ui::SettingsField::WifiForget,
];

pub(super) const BLUETOOTH_FIELDS: &[ui::SettingsField] = &[
    ui::SettingsField::BluetoothEnabled,
    ui::SettingsField::BluetoothDiscoverable,
    ui::SettingsField::BluetoothConnectedDevices,
    ui::SettingsField::BluetoothPairedDevices,
    ui::SettingsField::BluetoothNearbyDevices,
    ui::SettingsField::BluetoothSearch,
    ui::SettingsField::BluetoothPair,
    ui::SettingsField::BluetoothConnect,
    ui::SettingsField::BluetoothDisconnect,
    ui::SettingsField::BluetoothUnpair,
];

impl ShellSession {
    pub(super) fn system_settings_unavailable_reason(
        &self,
        category: ui::SettingsCategory,
    ) -> Option<String> {
        let snapshot = self.system_settings_backend.snapshot();
        let availability = match category {
            ui::SettingsCategory::Sound => snapshot.sound.availability,
            ui::SettingsCategory::Display => snapshot.display.availability,
            ui::SettingsCategory::Wifi => snapshot.wifi.availability,
            ui::SettingsCategory::Bluetooth => snapshot.bluetooth.availability,
            _ => return None,
        };
        match availability {
            Availability::Unavailable(UnavailableReason::UnsupportedPlatform) => {
                Some(i18n::tr!("settings-device-unsupported"))
            }
            Availability::Unavailable(UnavailableReason::NotIntegrated) => {
                Some(i18n::tr!("settings-device-not-integrated"))
            }
            Availability::Available => None,
        }
    }

    /// All keyboard and pointer activation paths share this availability guard.
    pub(super) fn block_unavailable_system_setting(&mut self) -> bool {
        let Some(category) = self.settings_state.as_ref().map(|state| state.category) else {
            return false;
        };
        let Some(reason) = self.system_settings_unavailable_reason(category) else {
            return false;
        };
        if let Some(state) = self.settings_state.as_mut() {
            state.status = reason.into();
        }
        true
    }
}

pub(super) fn system_device_cards(
    category: ui::SettingsCategory,
    reason: &str,
) -> Vec<ui::SettingsCardViewModel> {
    use ui::{
        SettingsCardViewModel as Card, SettingsControlKind as Kind, SettingsField as Field,
        SettingsItemViewModel as Item,
    };
    let item = |field, label, description, kind| {
        let value = if kind == Kind::Action {
            i18n::tr!("settings-device-unavailable")
        } else {
            i18n::tr!("settings-device-not-obtained")
        };
        Item::new(field, label, value, description, kind).unavailable(reason)
    };
    match category {
        ui::SettingsCategory::Sound => vec![
            Card::new(
                i18n::tr!("settings-device-card-sound-output"),
                vec![
                    item(
                        Field::SoundOutputDevice,
                        i18n::tr!("settings-device-sound-output-device"),
                        i18n::tr!("settings-device-sound-output-device-help"),
                        Kind::Picker,
                    ),
                    item(
                        Field::SoundOutputVolume,
                        i18n::tr!("settings-device-sound-output-volume"),
                        i18n::tr!("settings-device-sound-output-volume-help"),
                        Kind::Stepper,
                    ),
                    item(
                        Field::SoundOutputMute,
                        i18n::tr!("settings-device-sound-output-mute"),
                        i18n::tr!("settings-device-sound-output-mute-help"),
                        Kind::Toggle,
                    ),
                ],
            ),
            Card::new(
                i18n::tr!("settings-device-card-sound-input"),
                vec![
                    item(
                        Field::SoundInputDevice,
                        i18n::tr!("settings-device-sound-input-device"),
                        i18n::tr!("settings-device-sound-input-device-help"),
                        Kind::Picker,
                    ),
                    item(
                        Field::SoundInputVolume,
                        i18n::tr!("settings-device-sound-input-volume"),
                        i18n::tr!("settings-device-sound-input-volume-help"),
                        Kind::Stepper,
                    ),
                    item(
                        Field::SoundInputMute,
                        i18n::tr!("settings-device-sound-input-mute"),
                        i18n::tr!("settings-device-sound-input-mute-help"),
                        Kind::Toggle,
                    ),
                ],
            ),
        ],
        ui::SettingsCategory::Display => vec![Card::new(
            i18n::tr!("settings-device-card-display"),
            vec![
                item(
                    Field::DisplayDevice,
                    i18n::tr!("settings-device-display-device"),
                    i18n::tr!("settings-device-display-device-help"),
                    Kind::Picker,
                ),
                item(
                    Field::DisplayBrightness,
                    i18n::tr!("settings-device-display-brightness"),
                    i18n::tr!("settings-device-display-brightness-help"),
                    Kind::Stepper,
                ),
                item(
                    Field::DisplayAutomaticBrightness,
                    i18n::tr!("settings-device-display-automatic-brightness"),
                    i18n::tr!("settings-device-display-automatic-brightness-help"),
                    Kind::Toggle,
                ),
            ],
        )],
        ui::SettingsCategory::Wifi => vec![
            Card::new(
                i18n::tr!("settings-device-card-wifi-status"),
                vec![
                    item(
                        Field::WifiEnabled,
                        i18n::tr!("settings-device-wifi-enabled"),
                        i18n::tr!("settings-device-wifi-enabled-help"),
                        Kind::Toggle,
                    ),
                    item(
                        Field::WifiConnection,
                        i18n::tr!("settings-device-wifi-connection"),
                        i18n::tr!("settings-device-wifi-connection-help"),
                        Kind::ReadOnly,
                    ),
                ],
            ),
            Card::new(
                i18n::tr!("settings-device-card-wifi-networks"),
                vec![
                    item(
                        Field::WifiAvailableNetworks,
                        i18n::tr!("settings-device-wifi-available-networks"),
                        i18n::tr!("settings-device-wifi-available-networks-help"),
                        Kind::Picker,
                    ),
                    item(
                        Field::WifiSavedNetworks,
                        i18n::tr!("settings-device-wifi-saved-networks"),
                        i18n::tr!("settings-device-wifi-saved-networks-help"),
                        Kind::Picker,
                    ),
                ],
            ),
            Card::new(
                i18n::tr!("settings-device-card-wifi-actions"),
                vec![
                    item(
                        Field::WifiRefresh,
                        i18n::tr!("settings-device-wifi-refresh"),
                        i18n::tr!("settings-device-wifi-refresh-help"),
                        Kind::Action,
                    ),
                    item(
                        Field::WifiConnect,
                        i18n::tr!("settings-device-wifi-connect"),
                        i18n::tr!("settings-device-wifi-connect-help"),
                        Kind::Action,
                    ),
                    item(
                        Field::WifiDisconnect,
                        i18n::tr!("settings-device-wifi-disconnect"),
                        i18n::tr!("settings-device-wifi-disconnect-help"),
                        Kind::Action,
                    ),
                    item(
                        Field::WifiForget,
                        i18n::tr!("settings-device-wifi-forget"),
                        i18n::tr!("settings-device-wifi-forget-help"),
                        Kind::Action,
                    ),
                ],
            ),
        ],
        ui::SettingsCategory::Bluetooth => vec![
            Card::new(
                i18n::tr!("settings-device-card-bluetooth-status"),
                vec![
                    item(
                        Field::BluetoothEnabled,
                        i18n::tr!("settings-device-bluetooth-enabled"),
                        i18n::tr!("settings-device-bluetooth-enabled-help"),
                        Kind::Toggle,
                    ),
                    item(
                        Field::BluetoothDiscoverable,
                        i18n::tr!("settings-device-bluetooth-discoverable"),
                        i18n::tr!("settings-device-bluetooth-discoverable-help"),
                        Kind::Toggle,
                    ),
                ],
            ),
            Card::new(
                i18n::tr!("settings-device-card-bluetooth-devices"),
                vec![
                    item(
                        Field::BluetoothConnectedDevices,
                        i18n::tr!("settings-device-bluetooth-connected-devices"),
                        i18n::tr!("settings-device-bluetooth-connected-devices-help"),
                        Kind::Picker,
                    ),
                    item(
                        Field::BluetoothPairedDevices,
                        i18n::tr!("settings-device-bluetooth-paired-devices"),
                        i18n::tr!("settings-device-bluetooth-paired-devices-help"),
                        Kind::Picker,
                    ),
                    item(
                        Field::BluetoothNearbyDevices,
                        i18n::tr!("settings-device-bluetooth-nearby-devices"),
                        i18n::tr!("settings-device-bluetooth-nearby-devices-help"),
                        Kind::Picker,
                    ),
                ],
            ),
            Card::new(
                i18n::tr!("settings-device-card-bluetooth-actions"),
                vec![
                    item(
                        Field::BluetoothSearch,
                        i18n::tr!("settings-device-bluetooth-search"),
                        i18n::tr!("settings-device-bluetooth-search-help"),
                        Kind::Action,
                    ),
                    item(
                        Field::BluetoothPair,
                        i18n::tr!("settings-device-bluetooth-pair"),
                        i18n::tr!("settings-device-bluetooth-pair-help"),
                        Kind::Action,
                    ),
                    item(
                        Field::BluetoothConnect,
                        i18n::tr!("settings-device-bluetooth-connect"),
                        i18n::tr!("settings-device-bluetooth-connect-help"),
                        Kind::Action,
                    ),
                    item(
                        Field::BluetoothDisconnect,
                        i18n::tr!("settings-device-bluetooth-disconnect"),
                        i18n::tr!("settings-device-bluetooth-disconnect-help"),
                        Kind::Action,
                    ),
                    item(
                        Field::BluetoothUnpair,
                        i18n::tr!("settings-device-bluetooth-unpair"),
                        i18n::tr!("settings-device-bluetooth-unpair-help"),
                        Kind::Action,
                    ),
                ],
            ),
        ],
        _ => Vec::new(),
    }
}
