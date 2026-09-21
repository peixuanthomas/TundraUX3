use std::path::Path;

use super::{MNT_DONTBROWSE, macos_interface_kind, macos_mount_is_visible, macos_volume_kind};
use crate::{NetworkInterfaceKind, VolumeKind};

#[test]
fn volume_kind_uses_native_removable_mount_flag() {
    assert_eq!(macos_volume_kind(0), VolumeKind::Fixed);
    assert_eq!(
        macos_volume_kind(super::MNT_REMOVABLE),
        VolumeKind::Removable
    );
}

#[test]
fn mount_visibility_keeps_root_and_excludes_hidden_auxiliary_mounts() {
    assert!(macos_mount_is_visible(Path::new("/"), MNT_DONTBROWSE));
    assert!(!macos_mount_is_visible(
        Path::new("/System/Volumes/VM"),
        MNT_DONTBROWSE
    ));
    assert!(macos_mount_is_visible(Path::new("/Volumes/External"), 0));
}

#[test]
fn system_configuration_interface_types_map_to_public_kinds() {
    assert_eq!(
        macos_interface_kind(Some("Ethernet"), "en0"),
        NetworkInterfaceKind::Wired
    );
    assert_eq!(
        macos_interface_kind(Some("IEEE80211"), "en1"),
        NetworkInterfaceKind::Wireless
    );
    assert_eq!(
        macos_interface_kind(Some("Bridge"), "bridge0"),
        NetworkInterfaceKind::Virtual
    );
    assert_eq!(
        macos_interface_kind(Some("VendorSpecific"), "custom0"),
        NetworkInterfaceKind::Unknown
    );
}
