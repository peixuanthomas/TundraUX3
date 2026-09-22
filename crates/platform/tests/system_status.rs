use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use platform::{
    NativeSystemMonitor, NetworkInterface, NetworkInterfaceKind, NetworkLinkState, NetworkStatus,
    SystemMonitor,
};

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
    // A machine without batteries legitimately returns an empty collection.
}

#[cfg(target_os = "macos")]
#[test]
#[ignore = "requires a macOS host with an internal battery"]
fn native_monitor_reads_macos_internal_battery() {
    let output = std::process::Command::new("/usr/bin/pmset")
        .args(["-g", "batt"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let output = String::from_utf8(output.stdout).unwrap();
    assert!(output.contains("InternalBattery"), "{output}");
    let mut monitor = NativeSystemMonitor::new().unwrap();
    let batteries = monitor.sample_slow().unwrap().batteries.unwrap();
    assert!(!batteries.is_empty(), "pmset found a battery: {output}");
    for battery in batteries {
        assert!((0.0..=100.0).contains(&battery.charge_percent));
        assert!(battery.energy_full_wh > 0.0);
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
