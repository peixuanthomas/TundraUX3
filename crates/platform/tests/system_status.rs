use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use platform::mock::{MockCall, MockPlatform};
use platform::{
    AppPaths, NativeSystemMonitor, NetworkInterface, NetworkInterfaceKind, NetworkLinkState,
    NetworkStatus, Platform, SystemMonitor, UserDirs,
};

fn mock_platform() -> MockPlatform {
    let root = std::env::temp_dir();
    MockPlatform::new(
        UserDirs::new(&root, &root, &root, &root, &root, &root, &root).unwrap(),
        AppPaths::from_parts(&root, &root, &root, &root, &root).unwrap(),
    )
}

#[test]
fn native_monitor_establishes_network_rate_baseline_and_bounds_rankings() {
    let mut monitor = NativeSystemMonitor::new().unwrap();
    let first = monitor.sample_fast().unwrap();
    assert!(first.network_interfaces.iter().all(|interface| {
        interface.received_bytes_per_second == 0.0 && interface.transmitted_bytes_per_second == 0.0
    }));
    let second = monitor.sample_fast().unwrap();
    assert!(second.network_interfaces.iter().all(|interface| {
        interface.received_bytes_per_second.is_finite()
            && interface.transmitted_bytes_per_second.is_finite()
    }));
    assert_eq!(second.load.supported, !cfg!(target_os = "windows"));

    let slow = monitor.sample_slow().unwrap();
    assert!(slow.top_cpu.len() <= 20);
    assert!(slow.top_memory.len() <= 20);
    assert!(slow.top_cpu.windows(2).all(|pair| {
        pair[0].cpu_percent > pair[1].cpu_percent
            || (pair[0].cpu_percent == pair[1].cpu_percent && pair[0].pid <= pair[1].pid)
    }));
    assert!(slow.top_memory.windows(2).all(|pair| {
        pair[0].memory_bytes > pair[1].memory_bytes
            || (pair[0].memory_bytes == pair[1].memory_bytes && pair[0].pid <= pair[1].pid)
    }));
    if let Ok(thermal) = slow.thermal {
        assert!(!thermal.is_empty());
    }
    if let Ok(batteries) = slow.batteries {
        assert!(!batteries.is_empty());
    }
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
