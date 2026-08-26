use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use platform::mock::{MockCall, MockPlatform};
use platform::{
    AppPaths, NetworkInterface, NetworkInterfaceKind, NetworkLinkState, NetworkStatus, Platform,
    UserDirs,
};

fn mock_platform() -> MockPlatform {
    let root = std::env::temp_dir();
    MockPlatform::new(
        UserDirs::new(&root, &root, &root, &root, &root, &root, &root).unwrap(),
        AppPaths::from_parts(&root, &root, &root, &root, &root).unwrap(),
    )
}

#[test]
fn network_status_normalizes_and_ignores_virtual_links_in_summary() {
    let status = NetworkStatus::new(vec![
        NetworkInterface {
            name: "z-virtual".into(),
            display_name: None,
            kind: NetworkInterfaceKind::Virtual,
            link_state: NetworkLinkState::Up,
            addresses: vec![IpAddr::V6(Ipv6Addr::LOCALHOST)],
        },
        NetworkInterface {
            name: "eth0".into(),
            display_name: None,
            kind: NetworkInterfaceKind::Wired,
            link_state: NetworkLinkState::Up,
            addresses: vec![
                IpAddr::V4(Ipv4Addr::new(10, 0, 0, 2)),
                IpAddr::V4(Ipv4Addr::new(10, 0, 0, 2)),
            ],
        },
    ]);
    assert_eq!(status.interfaces[0].name, "eth0");
    assert_eq!(status.interfaces[0].addresses.len(), 1);
    assert_eq!(status.active_link_count(), 1);
    assert!(status.has_active_link());
}

#[test]
fn mock_injects_and_records_network_status() {
    let platform = mock_platform();
    let expected = NetworkStatus::new(vec![]);
    platform.set_network_status_result(Ok(expected.clone()));
    assert_eq!(platform.network_status().unwrap(), expected);
    assert!(platform.calls().contains(&MockCall::NetworkStatus));
}
