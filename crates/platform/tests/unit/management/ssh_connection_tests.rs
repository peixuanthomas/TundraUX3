use super::*;
#[test]
fn confirmation_rejects_old_sessions_reused_connections_and_wrong_ports() {
    assert!(new_connection(201, 200, 2222, &[2222], true));
    assert!(!new_connection(100, 200, 2222, &[2222], true));
    assert!(!new_connection(200, 200, 2222, &[2222], true));
    assert!(!new_connection(201, 200, 22, &[2222], true));
    assert!(!new_connection(201, 200, 2222, &[2222], false));
}
#[test]
fn proc_parser_uses_start_time_after_names_with_spaces_and_parentheses() {
    let stat = "123 (sshd: name (test)) S 42 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 456 0";
    assert_eq!(
        parse_stat(stat).unwrap(),
        Process {
            parent: 42,
            start_ticks: 456
        }
    );
    assert!(parse_stat("bad").is_err());
}
#[test]
fn socket_parser_reads_kernel_port_inode_and_connection_state() {
    let text = "header\n0: 0100007F:08AE 0200007F:1234 01 00000000:00000000 00:00000000 00000000 1000 0 555 1\n1: 00000000:0016 00000000:0000 0A 00000000:00000000 00:00000000 00000000 0 0 666 1\n";
    assert_eq!(
        parse_tcp(text),
        vec![
            Tcp {
                port: 2222,
                inode: 555,
                established: true
            },
            Tcp {
                port: 22,
                inode: 666,
                established: false
            }
        ]
    );
}
