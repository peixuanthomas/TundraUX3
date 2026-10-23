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
