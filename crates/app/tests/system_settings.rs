use app::system_settings::*;

#[test]
fn unavailable_backend_never_fabricates_state_or_success() {
    for reason in [
        UnavailableReason::UnsupportedPlatform,
        UnavailableReason::NotIntegrated,
    ] {
        let backend = UnavailableSystemSettingsBackend { reason };
        let snapshot = backend.snapshot();
        assert_eq!(
            snapshot.sound.availability,
            Availability::Unavailable(reason)
        );
        assert_eq!(
            snapshot.display.availability,
            Availability::Unavailable(reason)
        );
        assert_eq!(
            snapshot.wifi.availability,
            Availability::Unavailable(reason)
        );
        assert_eq!(
            snapshot.bluetooth.availability,
            Availability::Unavailable(reason)
        );
        assert_eq!(snapshot.sound.values, SoundSettings::default());
        assert_eq!(snapshot.display.values, DisplaySettings::default());
        assert_eq!(snapshot.wifi.values, WifiSettings::default());
        assert_eq!(snapshot.bluetooth.values, BluetoothSettings::default());
        assert!(snapshot.wifi.values.available_networks.is_none());
        assert!(snapshot.wifi.values.connection.is_none());
        assert!(snapshot.bluetooth.values.connected_devices.is_none());
        assert!(snapshot.sound.values.output_volume_percent.is_none());
        assert!(snapshot.sound.values.output_muted.is_none());

        let device = DeviceId("backend-device-id".into());
        let network = NetworkId("backend-network-id".into());
        for request in [
            SystemSettingsRequest::SelectAudioDevice {
                direction: AudioDirection::Output,
                device: device.clone(),
            },
            SystemSettingsRequest::SetVolume {
                direction: AudioDirection::Input,
                device: device.clone(),
                percent: 50,
            },
            SystemSettingsRequest::SetMute {
                direction: AudioDirection::Output,
                device: device.clone(),
                muted: true,
            },
            SystemSettingsRequest::SelectDisplay(device.clone()),
            SystemSettingsRequest::SetBrightness {
                display: device.clone(),
                percent: 50,
            },
            SystemSettingsRequest::SetAutomaticBrightness {
                display: device.clone(),
                enabled: true,
            },
            SystemSettingsRequest::SetWifiEnabled(true),
            SystemSettingsRequest::RefreshWifi,
            SystemSettingsRequest::ConnectWifi(network.clone()),
            SystemSettingsRequest::DisconnectWifi(network.clone()),
            SystemSettingsRequest::ForgetWifi(network),
            SystemSettingsRequest::SetBluetoothEnabled(true),
            SystemSettingsRequest::SetBluetoothDiscoverable(true),
            SystemSettingsRequest::DiscoverBluetooth,
            SystemSettingsRequest::PairBluetooth(device.clone()),
            SystemSettingsRequest::ConnectBluetooth(device.clone()),
            SystemSettingsRequest::DisconnectBluetooth(device.clone()),
            SystemSettingsRequest::UnpairBluetooth(device),
        ] {
            assert_eq!(
                backend.submit(request),
                OperationStatus::Unavailable(reason)
            );
            assert_eq!(backend.snapshot(), snapshot);
        }
        assert_eq!(
            backend.poll(OperationId(1)),
            OperationStatus::Unavailable(reason)
        );
    }
}
