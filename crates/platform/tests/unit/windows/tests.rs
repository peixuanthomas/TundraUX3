use std::ffi::OsStr;
use std::path::Path;

use super::{
    ComApartment, FILE_READ_ONLY_VOLUME, IpAdapterAddresses, WindowsPlatform,
    create_file_operation, shell_parsing_name_wide, to_wide, windows_volume_access,
};
use crate::{Platform, VolumeAccess};

#[test]
fn adapter_addresses_prefix_matches_windows_abi() {
    assert_eq!(std::mem::offset_of!(IpAdapterAddresses, next), 8);
    assert_eq!(
        std::mem::offset_of!(IpAdapterAddresses, first_unicast_address),
        24
    );
    assert_eq!(std::mem::offset_of!(IpAdapterAddresses, friendly_name), 72);
    assert_eq!(std::mem::offset_of!(IpAdapterAddresses, if_type), 100);
    assert_eq!(std::mem::offset_of!(IpAdapterAddresses, oper_status), 104);
    assert_eq!(std::mem::size_of::<IpAdapterAddresses>(), 112);
}

#[test]
fn volume_access_uses_only_readonly_flag_and_capacity_result() {
    assert_eq!(
        windows_volume_access(FILE_READ_ONLY_VOLUME, true),
        VolumeAccess::ReadOnly
    );
    assert_eq!(
        windows_volume_access(FILE_READ_ONLY_VOLUME, false),
        VolumeAccess::ReadOnly
    );
    assert_eq!(windows_volume_access(0, true), VolumeAccess::ReadWrite);
    assert_eq!(windows_volume_access(0, false), VolumeAccess::Unavailable);
}

#[test]
fn native_network_enumeration_smoke() {
    WindowsPlatform
        .network_status()
        .expect("enumerate Windows adapters");
}

#[test]
fn file_operation_com_class_exposes_the_declared_interface() {
    let _apartment = ComApartment::enter().expect("initialize COM");
    create_file_operation().expect("FileOperation should expose IFileOperation");
}

#[test]
fn shell_parsing_name_removes_verbatim_drive_prefix() {
    assert_eq!(
        shell_parsing_name_wide(Path::new(r"\\?\C:\Users\Example\file.txt"))
            .expect("convert verbatim drive path"),
        to_wide(OsStr::new(r"C:\Users\Example\file.txt"))
    );
}

#[test]
fn shell_parsing_name_converts_verbatim_unc_prefix() {
    assert_eq!(
        shell_parsing_name_wide(Path::new(r"\\?\UNC\server\share\file.txt"))
            .expect("convert verbatim UNC path"),
        to_wide(OsStr::new(r"\\server\share\file.txt"))
    );
}

#[test]
fn shell_parsing_name_preserves_regular_paths() {
    assert_eq!(
        shell_parsing_name_wide(Path::new(r"C:\Users\Example\file.txt"))
            .expect("preserve regular path"),
        to_wide(OsStr::new(r"C:\Users\Example\file.txt"))
    );
}
