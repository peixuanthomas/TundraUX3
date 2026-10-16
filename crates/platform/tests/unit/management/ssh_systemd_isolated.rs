use super::super::super::transaction::real_tests::{
    prepare_systemd_change, restore_systemd_change,
};
use super::super::super::{discover, properties};
use super::*;

fn systemctl(arguments: &[&str]) {
    let mut command = Command::new("/usr/bin/systemctl");
    command.args(arguments);
    checked(command);
}
fn old_command(key: &Path, port: u16, control: &Path, command: &str) -> std::process::Output {
    let mut client = client(key, port);
    client
        .args(["-F", "/dev/null"])
        .arg("-S")
        .arg(control)
        .args(["root@127.0.0.1", command]);
    checked(client)
}
fn fresh_probe(key: &Path, port: u16, cutoff: u64) {
    let mut client = client(key, port);
    client
        .args([
            "-F",
            "/dev/null",
            "-o",
            "ControlMaster=no",
            "-o",
            "ControlPath=none",
            "root@127.0.0.1",
        ])
        .arg(probe_command(cutoff, port, true));
    checked(client);
}

#[test]
#[ignore = "requires the private PID1 in scripts/tests/isolated-systemd.sh ssh mode"]
fn real_systemd_ssh_service_and_socket() {
    assert_eq!(unsafe { libc::geteuid() }, 0);
    assert!(Path::new("/etc/tundra-ssh-systemd-isolated").is_file());
    assert!(Path::new("/run/tundra-ssh-systemd-isolated").is_file());
    for (kind, variable) in [
        ("mnt", "TUNDRA_HOST_MNT_NS"),
        ("net", "TUNDRA_HOST_NET_NS"),
        ("pid", "TUNDRA_HOST_PID_NS"),
    ] {
        assert_ne!(
            fs::read_link(format!("/proc/self/ns/{kind}")).unwrap(),
            PathBuf::from(std::env::var_os(variable).unwrap())
        );
    }
    assert!(
        fs::read_to_string("/proc/1/comm")
            .unwrap()
            .starts_with("systemd")
    );
    let binary = PathBuf::from(std::env::var_os("TUNDRA_TEST_SSHD").unwrap());
    let libraries = std::env::var("LD_LIBRARY_PATH").unwrap();
    let temp = tempfile::Builder::new()
        .prefix("tundra-ssh-systemd-")
        .tempdir()
        .unwrap();
    fs::set_permissions(temp.path(), fs::Permissions::from_mode(0o755)).unwrap();
    let mut ip = Command::new("/usr/sbin/ip");
    ip.args(["link", "set", "lo", "up"]);
    checked(ip);
    let host = temp.path().join("host");
    let key = temp.path().join("client");
    for path in [&host, &key] {
        let mut generate = Command::new("/usr/bin/ssh-keygen");
        generate
            .args(["-q", "-t", "ed25519", "-N", "", "-f"])
            .arg(path);
        checked(generate);
    }
    fs::create_dir_all("/etc/systemd/system/ssh.socket.d").unwrap();
    let flag = AtomicBool::new(false);
    for (index, (socket, deadline)) in [(false, false), (false, true), (true, false), (true, true)]
        .into_iter()
        .enumerate()
    {
        if index != 0 {
            systemctl(&["stop", "ssh.service", "ssh.socket"]);
        }
        let port = 2262 + index as u16 * 2;
        let target = port + 1;
        let settings = format!(
            "Port {port}\nListenAddress 127.0.0.1\nHostKey {}\nPidFile /run/tundra-sshd.pid\nAuthorizedKeysFile {}\nStrictModes no\nUsePAM no\nPermitRootLogin yes\nPasswordAuthentication no\nKbdInteractiveAuthentication no\nPubkeyAuthentication yes\nPermitUserRC no\nLogLevel VERBOSE\n",
            host.display(),
            key.with_extension("pub").display()
        );
        fs::write("/etc/ssh/sshd_config", settings).unwrap();
        let mut validate = Command::new(&binary);
        validate.arg("-t");
        checked(validate);
        let dependencies = if socket {
            "Requires=ssh.socket\nAfter=ssh.socket\n"
        } else {
            ""
        };
        fs::write("/etc/systemd/system/ssh.service", format!("[Unit]\nDefaultDependencies=no\n{dependencies}[Service]\nType=simple\nKillMode=process\nEnvironment=LD_LIBRARY_PATH={libraries}\nExecStart={} -D -e\nStandardError=append:/run/tundra-sshd-unit.log\n", binary.display())).unwrap();
        fs::write("/etc/systemd/system/ssh.socket", format!("[Unit]\nDefaultDependencies=no\n[Socket]\nAccept=no\nListenStream=127.0.0.1:{port}\nService=ssh.service\n")).unwrap();
        systemctl(&["daemon-reload"]);
        if socket {
            systemctl(&["start", "ssh.socket"]);
        }
        systemctl(&["start", "ssh.service"]);
        wait_for(|| std::net::TcpStream::connect(("127.0.0.1", port)).is_ok());
        let initial = discover(&flag).unwrap();
        assert_eq!(initial.socket, socket);
        assert!(initial.reload_reason.is_none(), "{initial:?}");
        assert_eq!(
            properties("ssh.service", &flag)
                .unwrap()
                .get("KillMode")
                .map(String::as_str),
            Some("process")
        );
        let control = temp.path().join(format!("control-{index}"));
        let mut master = client(&key, port);
        master
            .args(["-F", "/dev/null"])
            .args(["-M", "-S"])
            .arg(&control)
            .args(["-N", "root@127.0.0.1"])
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let _master = ChildGuard(master.spawn().unwrap());
        wait_for(|| control.exists());
        old_command(&key, port, &control, "true");
        let id = format!("881-{index}");
        let cutoff = prepare_systemd_change(&id, target, deadline);
        let changed = discover(&flag).unwrap();
        assert_eq!(changed.main_pid == initial.main_pid, !socket);
        std::thread::sleep(Duration::from_millis(30));
        old_command(&key, port, &control, &probe_command(cutoff, target, false));
        fresh_probe(&key, target, cutoff);
        assert_eq!(
            old_command(&key, port, &control, "printf old-session-survived").stdout,
            b"old-session-survived"
        );
        restore_systemd_change(&id, deadline);
        let restored = discover(&flag).unwrap();
        assert_eq!(restored.original_ports, vec![port]);
        assert_eq!(restored.main_pid == changed.main_pid, !socket);
        assert_eq!(
            old_command(&key, port, &control, "printf old-session-restored").stdout,
            b"old-session-restored"
        );
        let restored_cutoff = ticks().unwrap();
        std::thread::sleep(Duration::from_millis(30));
        fresh_probe(&key, port, restored_cutoff);
        eprintln!(
            "real systemd SSH: socket={socket} deadline={deadline}, old session and restored listener verified"
        );
    }
    systemctl(&["stop", "ssh.service", "ssh.socket"]);
}
