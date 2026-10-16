use super::*;
use std::fs::File;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::CommandExt;
use std::process::Child;

struct ChildGuard(Child);
impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
fn checked(mut command: Command) -> std::process::Output {
    let output = command
        .output()
        .unwrap_or_else(|error| panic!("{command:?}: {error}"));
    assert!(
        output.status.success(),
        "{:?}: {}",
        command,
        String::from_utf8_lossy(&output.stderr)
    );
    output
}
fn bind_text(path: &str, text: &str, temp: &Path) {
    use std::os::unix::ffi::OsStrExt;
    let source = temp.join(Path::new(path).file_name().unwrap());
    fs::write(&source, text).unwrap();
    let source = std::ffi::CString::new(source.as_os_str().as_bytes()).unwrap();
    let target = std::ffi::CString::new(path).unwrap();
    assert_eq!(
        unsafe {
            libc::mount(
                source.as_ptr(),
                target.as_ptr(),
                std::ptr::null(),
                libc::MS_BIND,
                std::ptr::null(),
            )
        },
        0,
        "{}",
        std::io::Error::last_os_error()
    );
}
fn client(key: &Path, port: u16) -> Command {
    let mut command = Command::new("/usr/bin/ssh");
    command
        .args([
            "-o",
            "BatchMode=yes",
            "-o",
            "StrictHostKeyChecking=no",
            "-o",
            "UserKnownHostsFile=/dev/null",
            "-o",
            "GlobalKnownHostsFile=/dev/null",
            "-o",
            "IdentitiesOnly=yes",
            "-o",
            "ConnectTimeout=5",
            "-i",
        ])
        .arg(key)
        .args(["-p", &port.to_string()]);
    command
}
fn wait_for(mut predicate: impl FnMut() -> bool) {
    let started = Instant::now();
    while !predicate() {
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "SSH test readiness timed out"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}
fn probe_command(cutoff: u64, port: u16, expected: bool) -> String {
    let binary = std::env::current_exe().unwrap();
    probe_command_from(cutoff, port, expected, &binary)
}
fn probe_command_from(cutoff: u64, port: u16, expected: bool, binary: &Path) -> String {
    assert!(!binary.to_string_lossy().contains('\''));
    format!(
        "TUNDRA_SSH_PROBE='{cutoff},{port},{expected}' '{}' management::ssh::connection::real_tests::real_connection_probe --exact --ignored --nocapture",
        binary.display()
    )
}

#[test]
#[ignore = "internal subprocess of real_isolated_ssh_connections; not a standalone host test"]
fn real_connection_probe() {
    let Some(input) = std::env::var_os("TUNDRA_SSH_PROBE") else {
        return;
    };
    let input = input.to_str().unwrap();
    let fields: Vec<_> = input.split(',').collect();
    let actor_uid = unsafe { libc::getuid() };
    assert_eq!(unsafe { libc::setresuid(0, 0, 0) }, 0);
    let pid = std::process::id();
    let context = ExecutionContext {
        actor_uid,
        helper_path: std::env::current_exe().unwrap(),
        caller_pid: pid,
        caller_start_ticks: process(pid).unwrap().start_ticks,
    };
    let result = verify_new_connection(
        &context,
        fields[0].parse().unwrap(),
        &[fields[1].parse().unwrap()],
    );
    assert_eq!(result.is_ok(), fields[2] == "true", "{result:?}");
}

#[test]
#[ignore = "requires root, private mount/network namespaces and TUNDRA_TEST_SSHD; see ssh-management.md"]
fn real_isolated_ssh_connections() {
    assert_eq!(unsafe { libc::geteuid() }, 0);
    assert_ne!(
        fs::read_link("/proc/self/ns/mnt").unwrap(),
        std::env::var_os("TUNDRA_HOST_MNT_NS")
            .map(PathBuf::from)
            .unwrap_or_else(|| fs::read_link("/proc/1/ns/mnt").unwrap())
    );
    assert_ne!(
        fs::read_link("/proc/self/ns/net").unwrap(),
        std::env::var_os("TUNDRA_HOST_NET_NS")
            .map(PathBuf::from)
            .unwrap_or_else(|| fs::read_link("/proc/1/ns/net").unwrap())
    );
    let binary = PathBuf::from(
        std::env::var_os("TUNDRA_TEST_SSHD").expect("Provide a privately extracted sshd"),
    );
    assert_eq!(
        unsafe {
            libc::mount(
                c"tmpfs".as_ptr(),
                c"/tmp".as_ptr(),
                c"tmpfs".as_ptr(),
                0,
                c"mode=1777".as_ptr().cast(),
            )
        },
        0
    );
    let temp = tempfile::Builder::new()
        .prefix("tundra-real-ssh-")
        .tempdir()
        .unwrap();
    fs::set_permissions(temp.path(), fs::Permissions::from_mode(0o755)).unwrap();
    let privileged_probe = temp.path().join("authorized-test-probe");
    fs::copy(std::env::current_exe().unwrap(), &privileged_probe).unwrap();
    fs::set_permissions(&privileged_probe, fs::Permissions::from_mode(0o4755)).unwrap();
    let mut passwd = fs::read_to_string("/etc/passwd").unwrap();
    if !passwd.lines().any(|line| line.starts_with("sshd:")) {
        passwd.push_str("\nsshd:x:9999:9999:Private SSH test:/nonexistent:/usr/sbin/nologin\n");
    }
    passwd.push_str(&format!(
        "\ntundra-ssh-test:x:1001:1001:Private SSH test:{}:/bin/sh\n",
        temp.path().display()
    ));
    bind_text("/etc/passwd", &passwd, temp.path());
    let shadow = fs::read_to_string("/etc/shadow")
        .unwrap()
        .lines()
        .map(|line| {
            if line.starts_with("root:") {
                let mut fields = line.split(':').map(String::from).collect::<Vec<_>>();
                fields[1] = "private-test-invalid-hash".into();
                fields.join(":")
            } else {
                line.into()
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    bind_text(
        "/etc/shadow",
        &format!("{shadow}\ntundra-ssh-test:private-test-invalid-hash:20000:0:99999:7:::\n"),
        temp.path(),
    );
    if !Path::new("/run/sshd").exists() {
        assert_eq!(
            unsafe {
                libc::mount(
                    c"tmpfs".as_ptr(),
                    c"/run".as_ptr(),
                    c"tmpfs".as_ptr(),
                    0,
                    c"mode=755".as_ptr().cast(),
                )
            },
            0
        );
        fs::create_dir("/run/sshd").unwrap();
    }
    let mut ip = Command::new("/usr/sbin/ip");
    ip.args(["link", "set", "lo", "up"]);
    checked(ip);
    let host = temp.path().join("host");
    let key = temp.path().join("client");
    for path in [&host, &key] {
        let mut command = Command::new("/usr/bin/ssh-keygen");
        command
            .args(["-q", "-t", "ed25519", "-N", "", "-f"])
            .arg(path);
        checked(command);
    }
    let config = temp.path().join("sshd_config");
    let settings = |port: u16| {
        format!(
            "Port {port}\nListenAddress 127.0.0.1\nHostKey {}\nPidFile {}/pid\nAuthorizedKeysFile {}\nStrictModes no\nUsePAM no\nPermitRootLogin yes\nPasswordAuthentication no\nKbdInteractiveAuthentication no\nPubkeyAuthentication yes\nPermitUserRC no\nLogLevel VERBOSE\n",
            host.display(),
            temp.path().display(),
            key.with_extension("pub").display()
        )
    };
    for socket in [false, true] {
        let original = if socket { 2232 } else { 2222 };
        let changed = original + 1;
        fs::write(&config, settings(original)).unwrap();
        let mut check = Command::new(&binary);
        check.args(["-t", "-f"]).arg(&config);
        checked(check);
        let start = |port: u16| {
            let mut command = if socket {
                let mut c = Command::new("/usr/bin/systemd-socket-activate");
                c.args(["-l", &format!("127.0.0.1:{port}"), "-E", "LD_LIBRARY_PATH"]);
                c.arg(&binary);
                c
            } else {
                Command::new(&binary)
            };
            command
                .args(["-D", "-e", "-f"])
                .arg(&config)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::from(
                    File::create(temp.path().join(if socket {
                        "socket.log"
                    } else {
                        "daemon.log"
                    }))
                    .unwrap(),
                ))
                .process_group(0);
            ChildGuard(command.spawn().unwrap())
        };
        let mut server = start(original);
        let control = temp.path().join(if socket {
            "socket-control"
        } else {
            "daemon-control"
        });
        wait_for(|| std::net::TcpStream::connect(("127.0.0.1", original)).is_ok());
        let mut master = client(&key, original);
        master
            .args(["-M", "-S"])
            .arg(&control)
            .args(["-N", "root@127.0.0.1"])
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let _master = ChildGuard(master.spawn().unwrap());
        wait_for(|| control.exists());
        let mut old = client(&key, original);
        old.arg("-S").arg(&control).args(["root@127.0.0.1", "true"]);
        checked(old);
        let user_control = temp.path().join(if socket {
            "socket-user-control"
        } else {
            "daemon-user-control"
        });
        let mut user_master = client(&key, original);
        user_master
            .args(["-M", "-S"])
            .arg(&user_control)
            .args(["-N", "tundra-ssh-test@127.0.0.1"])
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let _user_master = ChildGuard(user_master.spawn().unwrap());
        wait_for(|| user_control.exists());
        fs::write(&config, settings(changed)).unwrap();
        let mut check = Command::new(&binary);
        check.args(["-t", "-f"]).arg(&config);
        checked(check);
        if socket {
            server.0.kill().unwrap();
            server.0.wait().unwrap();
            server = start(changed);
        } else {
            let before = listener_inodes(server.0.id()).unwrap();
            assert_eq!(unsafe { libc::kill(server.0.id() as i32, libc::SIGHUP) }, 0);
            wait_for(|| {
                listener_inodes(server.0.id())
                    .is_ok_and(|current| !current.is_empty() && current.is_disjoint(&before))
            });
        }
        wait_for(|| std::net::TcpStream::connect(("127.0.0.1", changed)).is_ok());
        let cutoff = ticks().unwrap();
        std::thread::sleep(Duration::from_millis(30));
        let mut old = client(&key, original);
        old.arg("-S")
            .arg(&control)
            .arg("root@127.0.0.1")
            .arg(probe_command(cutoff, changed, false));
        checked(old);
        let mut new = client(&key, changed);
        new.args([
            "-o",
            "ControlMaster=no",
            "-o",
            "ControlPath=none",
            "root@127.0.0.1",
        ])
        .arg(probe_command(cutoff, changed, true));
        checked(new);
        let mut wrong = client(&key, changed);
        wrong
            .args([
                "-o",
                "ControlMaster=no",
                "-o",
                "ControlPath=none",
                "root@127.0.0.1",
            ])
            .arg(probe_command(cutoff, changed + 10, false));
        checked(wrong);
        let mut old_user = client(&key, original);
        old_user
            .arg("-S")
            .arg(&user_control)
            .arg("tundra-ssh-test@127.0.0.1")
            .arg(probe_command_from(
                cutoff,
                changed,
                false,
                &privileged_probe,
            ));
        checked(old_user);
        let mut new_user = client(&key, changed);
        new_user
            .args([
                "-o",
                "ControlMaster=no",
                "-o",
                "ControlPath=none",
                "tundra-ssh-test@127.0.0.1",
            ])
            .arg(probe_command_from(cutoff, changed, true, &privileged_probe));
        checked(new_user);
        let mut echo = client(&key, original);
        echo.arg("-S")
            .arg(&control)
            .args(["root@127.0.0.1", "printf old-session-survived"]);
        assert_eq!(
            String::from_utf8_lossy(&checked(echo).stdout),
            "old-session-survived"
        );
        drop(_master);
        drop(_user_master);
        drop(server);
    }
    super::super::transaction::real_tests::verify_deadline_exit_and_boot(
        &binary,
        &config,
        &settings,
        temp.path(),
    );
}

#[path = "ssh_systemd_isolated.rs"]
mod systemd_tests;
