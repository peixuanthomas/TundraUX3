use super::*;
#[test]
fn only_tcp_listeners_and_unconnected_udp_are_selected() {
    let text = "header\n0: 0100007F:1F90 00000000:0000 0A 0:0 0:0 0 1000 0 42\n1: 00000000:0016 00000000:0000 01 0:0 0:0 0 1000 0 99\n2: 00000000:0035 00000000:0000 07 0:0 0:0 0 1000 0 77\n";
    assert_eq!(
        parse_table(text, "tcp", false)
            .iter()
            .map(|s| s.port)
            .collect::<Vec<_>>(),
        [8080]
    );
    assert_eq!(
        parse_table(text, "udp", false)
            .iter()
            .map(|s| s.port)
            .collect::<Vec<_>>(),
        [53]
    );
    assert_eq!(
        endpoint("00000000000000000000000001000000:0016", true),
        Some("[::1]:22".into())
    );
}
#[test]
fn live_listener_reports_the_real_owner() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap().to_string();
    let (rows, _) = query(Path::new("/proc"), &address, &AtomicBool::new(false)).unwrap();
    assert!(rows.iter().any(
        |row| row.cells[2] == address && row.cells[3].contains(&std::process::id().to_string())
    ));
}
