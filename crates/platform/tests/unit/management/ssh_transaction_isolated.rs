use super::*;
use std::os::unix::ffi::OsStrExt;
use std::process::{Child, Command, Stdio};

struct Server(Child);
impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
fn mount(source: &Path, target: &Path) {
    let source = std::ffi::CString::new(source.as_os_str().as_bytes()).unwrap();
    let target = std::ffi::CString::new(target.as_os_str().as_bytes()).unwrap();
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
fn wait_port(port: u16) {
    let started = Instant::now();
    while std::net::TcpStream::connect(("127.0.0.1", port)).is_err() {
        assert!(started.elapsed() < Duration::from_secs(10));
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
#[ignore = "internal child of real_isolated_ssh_connections"]
fn real_recovery_probe() {
    if let Ok(id) = std::env::var("TUNDRA_SSH_RECOVERY_ID") {
        rollback_transaction(&id).unwrap();
    }
}

pub(crate) fn verify_deadline_exit_and_boot(
    binary: &Path,
    config: &Path,
    settings: &dyn Fn(u16) -> String,
    temp: &Path,
) {
    let var = temp.join("var-lib");
    fs::create_dir(&var).unwrap();
    mount(&var, Path::new("/var/lib"));
    prepare_root().unwrap();
    let with_ufw = std::env::var("TUNDRA_SSH_WITH_UFW").as_deref() == Ok("1");
    let flag = AtomicBool::new(false);
    let original_rules = if with_ufw {
        assert!(
            Path::new("/etc/tundra-firewall-isolated").is_file()
                && Path::new("/run/tundra-firewall-isolated").is_file()
        );
        run("ufw", &["--force", "enable"], &flag).unwrap();
        run("ufw", &["allow", "5555/tcp"], &flag).unwrap();
        run("ufw", &["show", "added"], &flag).unwrap()
    } else {
        String::new()
    };
    let systemctl = temp.join("systemctl");
    fs::write(&systemctl, "#!/bin/sh\nexit 1\n").unwrap();
    fs::set_permissions(&systemctl, fs::Permissions::from_mode(0o755)).unwrap();
    mount(&systemctl, Path::new("/usr/bin/systemctl"));
    let mut cases = vec!["deadline", "owner-exit", "boot", "cancel", "bad-confirm"];
    if with_ufw {
        cases.push("firewall-failure");
    }
    for (index, kind) in cases.into_iter().enumerate() {
        let old_port = 2242 + index as u16 * 2;
        let new_port = old_port + 1;
        fs::write(config, settings(old_port)).unwrap();
        let start = || {
            Server(
                Command::new(binary)
                    .args(["-D", "-e", "-f"])
                    .arg(config)
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn()
                    .unwrap(),
            )
        };
        let mut server = start();
        wait_port(old_port);
        fs::write(
            &systemctl,
            format!(
                r#"#!/bin/sh
case "$1" in
show)
    case " $* " in
    *" firewalld.service "*) printf 'ActiveState=inactive\nMainPID=0\n';;
    *" ssh.service "*) printf 'ActiveState=active\nMainPID={}\nKillMode=process\n';;
    *) exit 1;;
    esac;;
daemon-reload) exit 0;;
*) exit 1;;
esac
"#,
                server.0.id()
            ),
        )
        .unwrap();
        let id = format!("991-{index}");
        let owned = if with_ufw {
            super::super::super::firewall::prepare_ssh_ports(&id, &[new_port], &flag).unwrap()
        } else {
            Vec::new()
        };
        if with_ufw {
            assert!(!owned.is_empty());
        }
        let before = system_config::read(config).unwrap();
        let mut paths = vec![config.into()];
        paths.extend(super::super::super::firewall::ssh_backup_paths(&owned));
        let session = super::super::super::config_backups::begin("ssh", &paths).unwrap();
        super::super::super::firewall::apply_ssh_ports_unrecorded(&owned, &flag).unwrap();
        let after = settings(new_port);
        system_config::install_reviewed(config, &before.version, &after).unwrap();
        let old_listeners = connection::listener_inodes(server.0.id()).unwrap();
        assert_eq!(unsafe { libc::kill(server.0.id() as i32, libc::SIGHUP) }, 0);
        let started = Instant::now();
        while connection::listener_inodes(server.0.id())
            .is_ok_and(|current| current.is_empty() || !current.is_disjoint(&old_listeners))
        {
            assert!(started.elapsed() < Duration::from_secs(10));
            std::thread::sleep(Duration::from_millis(20));
        }
        wait_port(new_port);
        session.finish("awaiting SSH confirmation").unwrap();
        let directory = Path::new(ROOT).join(&id);
        fs::DirBuilder::new()
            .mode(0o700)
            .create(&directory)
            .unwrap();
        let owner_pid = std::process::id();
        let journal = Journal {
            actor_uid: 0,
            owner_pid: if kind == "owner-exit" { 0 } else { owner_pid },
            owner_start_ticks: connection::start_ticks(owner_pid).unwrap(),
            boot_id: if kind == "boot" {
                "previous-boot".into()
            } else {
                boot_id().unwrap()
            },
            state: State::AwaitingConfirmation,
            deadline: if kind == "firewall-failure" {
                now()
            } else if kind == "deadline" {
                now() + 1
            } else {
                now() + 120
            },
            applied_ticks: connection::ticks().unwrap(),
            backup_id: session.id().into(),
            rollback_backup_id: None,
            plan: Plan {
                service: Service {
                    unit: "ssh.service".into(),
                    socket: false,
                    socket_listen: String::new(),
                    main_pid: server.0.id(),
                    reload_reason: None,
                    original_ports: vec![old_port],
                },
                files: vec![Change {
                    before: before.clone(),
                    after,
                    after_existed: true,
                }],
                ports: vec![new_port],
                key_command: None,
                backup_to_restore: None,
                seed_backup: None,
                remove_firewall: Vec::new(),
                add_firewall: Vec::new(),
            },
            firewall: owned.clone(),
            result: String::new(),
        };
        write(&directory, &journal).unwrap();
        if kind == "boot" {
            server.0.kill().unwrap();
            server.0.wait().unwrap();
            if with_ufw {
                let saved = fs::read_to_string("/etc/ufw/ufw.conf").unwrap();
                run("ufw", &["--force", "disable"], &flag).unwrap();
                fs::write("/etc/ufw/ufw.conf", saved).unwrap();
                assert!(
                    run("ufw", &["status"], &flag)
                        .unwrap()
                        .contains("Status: inactive")
                );
            }
        }
        if kind == "firewall-failure" {
            let failure = temp.join("ufw-failure");
            fs::write(&failure, "#!/bin/sh\nexit 99\n").unwrap();
            fs::set_permissions(&failure, fs::Permissions::from_mode(0o755)).unwrap();
            mount(&failure, Path::new("/usr/sbin/ufw"));
        }
        let began = Instant::now();
        if kind == "cancel" || kind == "bad-confirm" {
            let context = ExecutionContext {
                actor_uid: 0,
                helper_path: std::env::current_exe().unwrap(),
                caller_pid: owner_pid,
                caller_start_ticks: connection::start_ticks(owner_pid).unwrap(),
            };
            let result = confirm(
                &id,
                &context,
                kind == "bad-confirm",
                &AtomicBool::new(false),
            );
            if kind == "bad-confirm" {
                assert!(
                    result
                        .unwrap_err()
                        .to_string()
                        .contains("Previous SSH settings were restored")
                );
            } else {
                result.unwrap();
            }
        } else {
            let output = Command::new(std::env::current_exe().unwrap())
                .env("TUNDRA_SSH_RECOVERY_ID", &id)
                .args([
                    "management::ssh::transaction::real_tests::real_recovery_probe",
                    "--exact",
                    "--ignored",
                    "--nocapture",
                ])
                .output()
                .unwrap();
            assert!(
                output.status.success() != (kind == "firewall-failure"),
                "{kind}: {} {}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }
        assert!(began.elapsed() < Duration::from_secs(if with_ufw { 30 } else { 5 }));
        assert_eq!(
            read(&directory).unwrap().state,
            if kind == "firewall-failure" {
                State::RestoreFailed
            } else {
                State::Restored
            }
        );
        assert_eq!(system_config::read(config).unwrap().content, before.content);
        if kind == "boot" {
            server = start();
        }
        wait_port(old_port);
        if kind == "firewall-failure" {
            assert_eq!(unsafe { libc::umount(c"/usr/sbin/ufw".as_ptr()) }, 0);
            rollback_transaction(&id).unwrap();
            assert_eq!(read(&directory).unwrap().state, State::Restored);
            wait_port(old_port);
        }
        if with_ufw {
            let remaining = if kind == "boot" {
                super::super::super::firewall::present_ssh_rules_boot(&owned, &flag).unwrap()
            } else {
                super::super::super::firewall::present_ssh_rules(&owned, &flag).unwrap()
            };
            assert!(remaining.is_empty());
            assert_eq!(
                run("ufw", &["show", "added"], &flag).unwrap(),
                original_rules
            );
            if kind == "boot" {
                run("ufw", &["--force", "enable"], &flag).unwrap();
            }
        }
        drop(server);
    }
}

pub(in crate::management::ssh) fn prepare_systemd_change(
    id: &str,
    port: u16,
    deadline: bool,
) -> u64 {
    let flag = AtomicBool::new(false);
    prepare_root().unwrap();
    let service = discover(&flag).unwrap();
    assert!(service.reload_reason.is_none(), "{service:?}");
    let before = system_config::read(Path::new(MAIN)).unwrap();
    let after = rewrite(&before.content, true, port, "no", "yes", "yes");
    let mut files = vec![Change {
        before,
        after,
        after_existed: true,
    }];
    if service.socket {
        files.push(Change {
            before: system_config::read(Path::new(SOCKET_DROPIN)).unwrap(),
            after: socket_content(&service.socket_listen, port).unwrap(),
            after_existed: true,
        });
    }
    let paths = files
        .iter()
        .map(|file| file.before.path.clone())
        .collect::<Vec<_>>();
    let session = super::super::super::config_backups::begin("ssh", &paths).unwrap();
    session.set_payload(serde_json::json!({"transaction_id":id,"backup_id":session.id(),"firewall_observed":true,"firewall_restore":{"add":[],"remove":[]}})).unwrap();
    let directory = Path::new(ROOT).join(id);
    fs::DirBuilder::new()
        .mode(0o700)
        .create(&directory)
        .unwrap();
    let owner_pid = std::process::id();
    let mut journal = Journal {
        actor_uid: 0,
        owner_pid,
        owner_start_ticks: connection::start_ticks(owner_pid).unwrap(),
        boot_id: boot_id().unwrap(),
        state: State::Applying,
        deadline: now() + 120,
        applied_ticks: 0,
        backup_id: session.id().into(),
        rollback_backup_id: None,
        plan: Plan {
            service,
            files,
            ports: vec![port],
            key_command: None,
            backup_to_restore: None,
            seed_backup: None,
            remove_firewall: Vec::new(),
            add_firewall: Vec::new(),
        },
        firewall: Vec::new(),
        result: String::new(),
    };
    write(&directory, &journal).unwrap();
    for file in &journal.plan.files {
        system_config::install_reviewed(&file.before.path, &file.before.version, &file.after)
            .unwrap();
    }
    load(&journal.plan.service, &flag).unwrap();
    verify_plan(&journal.plan, &flag).unwrap();
    journal.applied_ticks = connection::ticks().unwrap();
    journal.deadline = now() + if deadline { 1 } else { 120 };
    journal.state = State::AwaitingConfirmation;
    session
        .finish("awaiting SSH connection confirmation")
        .unwrap();
    write(&directory, &journal).unwrap();
    journal.applied_ticks
}

pub(in crate::management::ssh) fn restore_systemd_change(id: &str, deadline: bool) {
    if deadline {
        let output = Command::new(std::env::current_exe().unwrap())
            .env("TUNDRA_SSH_RECOVERY_ID", id)
            .args([
                "management::ssh::transaction::real_tests::real_recovery_probe",
                "--exact",
                "--ignored",
                "--nocapture",
            ])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{} {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    } else {
        let pid = std::process::id();
        let context = ExecutionContext {
            actor_uid: 0,
            helper_path: std::env::current_exe().unwrap(),
            caller_pid: pid,
            caller_start_ticks: connection::start_ticks(pid).unwrap(),
        };
        confirm(id, &context, false, &AtomicBool::new(false)).unwrap();
    }
    assert_eq!(
        read(&Path::new(ROOT).join(id)).unwrap().state,
        State::Restored
    );
}
