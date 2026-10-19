use super::ssh_ports::boot::{boot_state, change_boot_rules, daemon_started};
use super::ssh_ports::{apply_ports, own_rule, prepare_ports, rollback_ports};
use super::*;
use std::collections::VecDeque;

#[test]
fn unsupported_firewall_explains_requirements_and_keeps_listener_navigation() {
    for installed_firewalld in [false, true] {
        let mut fixture = Fixture::ufw("", false);
        fixture.ufw = false;
        fixture.firewalld = installed_firewalld;
        fixture.commands.clear();
        let snapshot = snapshot(load(&mut fixture, &flag()).unwrap(), "").unwrap();
        assert!(snapshot.rows.is_empty());
        assert!(!snapshot.needs_authorization);
        assert_eq!(
            snapshot.notices,
            [i18n::tr!("management-firewall-unavailable")]
        );
        assert_eq!(
            snapshot.details[0].1,
            i18n::tr!(if installed_firewalld {
                "management-firewalld-inactive"
            } else {
                "management-firewall-tools-missing"
            })
        );
        assert_eq!(snapshot.actions.len(), 1);
        assert_eq!(snapshot.actions[0].id, "show_listening_ports");
        assert!(fixture.commands.is_empty());
    }
}

struct Fixture {
    ufw: bool,
    firewalld: bool,
    owner: bool,
    enabled: bool,
    files: BTreeMap<String, String>,
    after_write: BTreeMap<String, String>,
    error: String,
    commands: VecDeque<(String, Vec<String>, String, i32)>,
}
impl Fixture {
    fn ufw(status: &str, enabled: bool) -> Self {
        let mut fixture = Self {
            ufw: true,
            firewalld: false,
            owner: false,
            enabled,
            files: BTreeMap::new(),
            after_write: BTreeMap::new(),
            error: String::new(),
            commands: VecDeque::new(),
        };
        fixture.push("ufw", &["status", "numbered"], status, 0);
        fixture.push(
            "ufw",
            &["show", "added"],
            "Added user rules (see 'ufw status' for running firewall):\nufw allow 22/tcp",
            0,
        );
        fixture
    }
    fn push(&mut self, tool: &str, arguments: &[&str], output: &str, status: i32) {
        self.commands
            .push_back((tool.into(), args(arguments), output.into(), status));
    }
    fn firewalld(runtime: &str, permanent: &str) -> Self {
        let mut fixture = Self {
            ufw: false,
            firewalld: true,
            owner: true,
            enabled: false,
            files: BTreeMap::new(),
            after_write: BTreeMap::new(),
            error: String::new(),
            commands: VecDeque::new(),
        };
        fixture.push("firewall-cmd", &["--state"], "running", 0);
        fixture.push("firewall-cmd", &["--get-default-zone"], "public", 0);
        fixture.push("firewall-cmd", &["--list-all-zones"], runtime, 0);
        fixture.push(
            "firewall-cmd",
            &["--permanent", "--list-all-zones"],
            permanent,
            0,
        );
        fixture.push(
            "firewall-cmd",
            &["--get-active-zones"],
            "public\n  interfaces: eth0",
            0,
        );
        fixture
    }
}
impl Native for Fixture {
    fn firewalld_owner(&self, _: &AtomicBool) -> Result<bool, ManagementError> {
        Ok(self.owner)
    }
    fn installed(&self, name: &str) -> bool {
        match name {
            "ufw" => self.ufw,
            "firewall-cmd" => self.firewalld,
            _ => false,
        }
    }
    fn read(&self, path: &str) -> Option<String> {
        if let Some(text) = self.files.get(path) {
            return Some(text.clone());
        }
        (path == "/etc/ufw/ufw.conf")
            .then(|| format!("ENABLED={}\n", if self.enabled { "yes" } else { "no" }))
    }
    fn command(
        &mut self,
        tool: &str,
        arguments: &[String],
        _: &AtomicBool,
    ) -> Result<(String, String, i32), ManagementError> {
        let expected = self
            .commands
            .pop_front()
            .expect("Unexpected native command");
        assert_eq!(
            (tool, arguments),
            (expected.0.as_str(), expected.1.as_slice())
        );
        if tool == "ufw"
            && arguments
                .first()
                .is_some_and(|arg| arg == "--force" || arg == "allow")
            && !self.after_write.is_empty()
        {
            self.files = std::mem::take(&mut self.after_write);
        }
        let error = if expected.3 != 0 {
            std::mem::take(&mut self.error)
        } else {
            String::new()
        };
        Ok((expected.2, error, expected.3))
    }
}
fn flag() -> AtomicBool {
    AtomicBool::new(false)
}
fn command(state: &State) -> ManagementCommand {
    ManagementCommand {
        kind: ManagementKind::Firewall,
        action: "add_rule".into(),
        target: None,
        values: BTreeMap::from([("port".into(), "8000-8010".into())]),
        identity: BTreeMap::from([
            ("backend".into(), state.backend.id().into()),
            ("firewall_state".into(), state.fingerprint.clone()),
        ]),
    }
}

#[test]
fn ufw_number_is_rejected_when_another_rule_takes_its_place() {
    let mut fixture = Fixture::ufw("Status: active\n[ 1] 22/tcp ALLOW IN Anywhere\n", true);
    let first = load(&mut fixture, &flag()).unwrap();
    let old = command(&first);
    let mut fixture = Fixture::ufw("Status: active\n[ 1] 80/tcp ALLOW IN Anywhere\n", true);
    let second = load(&mut fixture, &flag()).unwrap();
    assert!(matches!(
        validate_request(&old, &second),
        Err(ManagementError::Conflict(_))
    ));
    assert!(fixture.commands.is_empty());
}

#[test]
fn normal_ufw_deletion_uses_complete_parameters_and_address_family() {
    let mut fixture = Fixture::ufw(
        "Status: active\n[ 9] 38111:38113/tcp ALLOW IN 127.0.0.1 # original\n[10] 38121/udp (v6) ALLOW IN ::1\n",
        true,
    );
    let state = load(&mut fixture, &flag()).unwrap();
    assert_eq!(
        ufw_delete(&state.rules[0]).unwrap(),
        args(&[
            "--force",
            "delete",
            "allow",
            "in",
            "proto",
            "tcp",
            "from",
            "127.0.0.1",
            "to",
            "0.0.0.0/0",
            "port",
            "38111:38113",
            "comment",
            "original"
        ])
    );
    assert_eq!(
        ufw_delete(&state.rules[1]).unwrap(),
        args(&[
            "--force", "delete", "allow", "in", "proto", "udp", "from", "::1", "to", "::/0",
            "port", "38121"
        ])
    );
    let unsupported = Rule {
        value: "OpenSSH ALLOW IN Anywhere".into(),
        ..state.rules[0].clone()
    };
    assert!(ufw_delete(&unsupported).is_err());
}

#[test]
fn chinese_firewall_labels_preserve_native_rule_identity() {
    let root = tempfile::tempdir().unwrap();
    let language = i18n::LanguageSnapshot::load_startup(root.path(), "zh-CN", 1);
    let language = std::sync::Arc::new(language.snapshot);
    i18n::with_snapshot(&language, || {
        let mut fixture = Fixture::ufw("Status: active\n[ 1] 2222/tcp ALLOW IN Anywhere\n", true);
        let view = snapshot(load(&mut fixture, &flag()).unwrap(), "").unwrap();
        assert_eq!(view.columns[0], "规则 / 后端");
        assert_eq!(
            view.rows[1].identity["rule_value"],
            "2222/tcp ALLOW IN Anywhere"
        );
        assert_eq!(view.rows[1].identity["rule_kind"], "number");
        assert_eq!(view.actions[1].fields[0].value, "tcp");
    });
}
#[test]
fn firewalld_keeps_runtime_permanent_and_ipv6_rules_separate() {
    let runtime = "public (active)\n  services: ssh\n  ports: 443/tcp\n  rich rules:\n    rule family=\"ipv6\" source address=\"2001:db8::/64\" port port=\"9000-9010\" protocol=\"udp\" accept\n";
    let permanent = "public\n  services: ssh\n  ports: 80/tcp\n  rich rules:\n";
    let mut fixture = Fixture::firewalld(runtime, permanent);
    let state = load(&mut fixture, &flag()).unwrap();
    assert_eq!(state.rules.len(), 5);
    assert!(state.rules.iter().any(|rule| rule.kind == "rich"
        && rule.scope == "runtime"
        && rule.value.contains("2001:db8::/64")));
    let result = snapshot(state, "").unwrap();
    assert!(result.rows.iter().any(|row| row.cells[3] == "443/tcp" && row.cells[2] == "Immediate; removed at restart"));
    assert!(
        result
            .rows
            .iter()
            .any(|row| row.cells[3] == "80/tcp" && row.cells[2] == "After reload / restart")
    );
    assert!(fixture.commands.is_empty());
}
#[test]
fn unsupported_backend_explains_requirements_and_rejects_write_actions() {
    let mut fixture = Fixture {
        ufw: false,
        firewalld: false,
        owner: false,
        enabled: false,
        files: BTreeMap::new(),
        after_write: BTreeMap::new(),
        error: String::new(),
        commands: VecDeque::new(),
    };
    let state = load(&mut fixture, &flag()).unwrap();
    assert!(matches!(
        validate_request(&command(&state), &state),
        Err(ManagementError::Unavailable(_))
    ));
    let result = snapshot(state, "").unwrap();
    assert_eq!(result.backend, "Not supported yet");
    assert!(result.rows.is_empty());
    assert_eq!(
        result.notices,
        [i18n::tr!("management-firewall-unavailable")]
    );
    assert_eq!(
        result.details,
        [(
            i18n::tr!("management-requirements"),
            i18n::tr!("management-firewall-tools-missing")
        )]
    );
    assert_eq!(
        result
            .actions
            .iter()
            .map(|action| action.id.as_str())
            .collect::<Vec<_>>(),
        ["show_listening_ports"]
    );
    assert!(result.actions.iter().all(|action| !action.privileged));
    assert!(!result.needs_authorization);
    assert!(fixture.commands.is_empty());
}
#[test]
fn simultaneous_backends_disable_all_changes() {
    let mut fixture = Fixture::firewalld("public\n rich rules:\n", "public\n rich rules:\n");
    fixture.ufw = true;
    fixture.enabled = true;
    fixture.commands.insert(
        1,
        (
            "ufw".into(),
            args(&["status", "numbered"]),
            "Status: active\n".into(),
            0,
        ),
    );
    let state = load(&mut fixture, &flag()).unwrap();
    assert!(matches!(
        validate_request(&command(&state), &state),
        Err(ManagementError::Conflict(_))
    ));
    let result = snapshot(state, "").unwrap();
    assert!(
        result
            .actions
            .iter()
            .filter(|action| action.id == "add_rule")
            .all(|action| action.disabled_reason.is_some())
    );
}

#[test]
fn a_running_failed_firewalld_is_never_treated_as_stopped() {
    let mut fixture = Fixture::firewalld("public\n rich rules:\n", "public\n rich rules:\n");
    fixture.commands[0].2 = "failed".into();
    fixture.commands[0].3 = 251;
    let state = load(&mut fixture, &flag()).unwrap();
    assert_eq!(state.backend, Backend::Firewalld);
    assert!(state.active);
    assert!(state.issue.as_deref().unwrap().contains("failed"));
    assert!(validate_request(&command(&state), &state).is_err());
}
#[test]
fn validated_inputs_become_fixed_native_arguments() {
    let mut fixture = Fixture::ufw("Status: active\n", true);
    let state = load(&mut fixture, &flag()).unwrap();
    let mut command = command(&state);
    command
        .values
        .insert("source".into(), "2001:db8::/64".into());
    let input = input(&command, &state).unwrap();
    assert_eq!(
        ufw_add(&input, None),
        args(&[
            "allow",
            "in",
            "proto",
            "tcp",
            "from",
            "2001:db8::/64",
            "to",
            "any",
            "port",
            "8000:8010"
        ])
    );
    assert_eq!(
        rich_rule(&input),
        "rule family=\"ipv6\" source address=\"2001:db8::/64\" port port=\"8000-8010\" protocol=\"tcp\" accept"
    );
    for value in ["0", "65536", "90-80", "22;reboot", "22\n80", "-22"] {
        assert!(port_range(value).is_err(), "{value}");
    }
    for value in ["--help", "any;reboot", "2001:db8::/129", "127.0.0.1/33"] {
        assert!(source_address(value).is_err(), "{value}");
    }
    assert!(validate_zone("public\n--reload").is_err());
}
#[test]
fn restore_refuses_to_overwrite_external_rule_changes() {
    let mut fixture = Fixture::firewalld(
        "public\n ports: 443/tcp\n rich rules:\n",
        "public\n rich rules:\n",
    );
    let current = load(&mut fixture, &flag()).unwrap();
    let mut saved = BackupPayload {
        missing_zones: Vec::new(),
        version: 1,
        backend: Backend::Firewalld,
        active: true,
        ufw_before: String::new(),
        ufw_after: String::new(),
        changes: vec![Change {
            zone: "public".into(),
            scope: "runtime".into(),
            kind: "port".into(),
            value: "443/tcp".into(),
            before: false,
            after: true,
        }],
    };
    assert!(restore_check(&saved, &current).is_ok());
    saved.changes[0].value = "80/tcp".into();
    assert!(matches!(
        restore_check(&saved, &current),
        Err(ManagementError::Conflict(_))
    ));
}
#[test]
fn ssh_prepare_skips_existing_rules_and_owns_only_transaction_comment() {
    let mut fixture = Fixture::ufw(
        "Status: active\n[ 1] 22/tcp ALLOW IN Anywhere\n[ 2] 22/tcp (v6) ALLOW IN Anywhere (v6)\n",
        true,
    );
    let rules = prepare_ports(&mut fixture, "test-session", &[22, 2222, 2222], &flag()).unwrap();
    assert_eq!(rules.len(), 2);
    assert!(rules.iter().all(|rule| rule.port == 2222));
    let mut fixture = Fixture::ufw(
        "Status: active\n[ 1] 2222/tcp ALLOW IN Anywhere # someone-else\n[ 2] 2222/tcp (v6) ALLOW IN Anywhere (v6) # tundra-ssh-test-session\n",
        true,
    );
    let current = load(&mut fixture, &flag()).unwrap();
    assert!(own_rule(&current, &rules[0]).is_none());
    assert_eq!(own_rule(&current, &rules[1]).unwrap().id, "2");
    let value = serde_json::to_value(&rules).unwrap();
    let restored: Vec<OwnedPortRule> = serde_json::from_value(value).unwrap();
    assert_eq!(restored.len(), 2);
}
#[test]
fn ssh_firewalld_prepares_separate_families_and_scopes_without_deleting_old_ports() {
    let mut fixture = Fixture::firewalld(
        "public\n ports: 22/tcp\n rich rules:\n",
        "public\n ports: 22/tcp\n rich rules:\n",
    );
    let rules = prepare_ports(&mut fixture, "test-session", &[22, 2222], &flag()).unwrap();
    assert_eq!(rules.len(), 4);
    assert!(
        rules
            .iter()
            .all(|rule| rule.port == 2222 && rule.value.starts_with("rule priority="))
    );
    assert_eq!(
        rules.iter().filter(|rule| rule.scope == "runtime").count(),
        2
    );
    assert_eq!(
        rules
            .iter()
            .filter(|rule| rule.scope == "permanent")
            .count(),
        2
    );
    assert!(fixture.commands.is_empty());
}

#[test]
fn inactive_ufw_lists_saved_rules_and_only_offers_allow_deletion() {
    let mut fixture = Fixture::ufw("Status: inactive\n", false);
    fixture.commands[1].2 =
        "Added user rules:\nufw allow 22/tcp comment 'SSH connection'\nufw deny 80/tcp\n".into();
    let state = load(&mut fixture, &flag()).unwrap();
    assert_eq!(state.rules.len(), 2);
    assert_eq!(state.rules[0].kind, "saved");
    let result = snapshot(state, "").unwrap();
    assert_eq!(result.rows[1].actions.len(), 1);
    assert!(result.rows[2].actions.is_empty());
    assert!(result.rows[1].cells[2].contains("inactive"));
}

#[test]
fn ssh_apply_rejects_an_external_rule_added_after_preparation() {
    let mut before = Fixture::ufw("Status: active\n", true);
    let planned = prepare_ports(&mut before, "test-session", &[2222], &flag()).unwrap();
    let mut after = Fixture::ufw(
        "Status: active\n[ 1] 2222/tcp ALLOW IN Anywhere # external-rule\n",
        true,
    );
    assert!(matches!(
        apply_ports(&mut after, &planned, &flag()),
        Err(ManagementError::Conflict(_))
    ));
    assert!(after.commands.is_empty());
}
#[test]
fn ssh_prepare_reads_native_service_ports_instead_of_guessing_the_ssh_service() {
    let mut fixture = Fixture::firewalld(
        "public\n services: ssh\n rich rules:\n",
        "public\n services: ssh\n rich rules:\n",
    );
    for _ in 0..2 {
        fixture.push(
            "firewall-cmd",
            &["--info-service=ssh"],
            "ssh\n ports: 22/tcp\n destination:\n",
            0,
        );
        fixture.push(
            "firewall-cmd",
            &["--permanent", "--info-service=ssh"],
            "ssh\n ports: 22/tcp\n destination:\n",
            0,
        );
    }
    let planned = prepare_ports(&mut fixture, "test-session", &[22], &flag()).unwrap();
    assert!(planned.is_empty());
    assert!(fixture.commands.is_empty());
}

#[test]
fn boot_ufw_uses_family_and_exact_saved_comment_without_dbus() {
    let mut active = Fixture::ufw("Status: active\n", true);
    let rules = prepare_ports(&mut active, "boot-test", &[2222], &flag()).unwrap();
    let mut stopped = Fixture::ufw("Status: inactive\n", true);
    let comment = "tundra-ssh-boot-test"
        .bytes()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    stopped.files.insert(
        "/etc/ufw/user.rules".into(),
        format!("### tuple ### allow tcp 2222 0.0.0.0/0 any 0.0.0.0/0 in comment={comment}\n"),
    );
    stopped.files.insert(
        "/etc/ufw/user6.rules".into(),
        "### tuple ### allow tcp 2222 ::/0 any ::/0 in comment=736f6d656f6e652d656c7365\n".into(),
    );
    let (state, offline) = boot_state(&mut stopped, &rules, false, &flag()).unwrap();
    assert!(!state.active && !offline);
    assert!(own_rule(&state, &rules[0]).is_some());
    assert!(own_rule(&state, &rules[1]).is_none());
    assert!(stopped.commands.is_empty());
}

#[test]
fn ssh_ufw_rollback_deletes_the_complete_owned_rule_instead_of_its_number() {
    let mut before = Fixture::ufw("Status: active\n", true);
    let rules = prepare_ports(&mut before, "test-session", &[2222], &flag()).unwrap();
    let mut fixture = Fixture::ufw(
        "Status: active\n[ 1] 5555/tcp ALLOW IN Anywhere\n[ 2] 2222/tcp ALLOW IN Anywhere # tundra-ssh-test-session\n",
        true,
    );
    fixture.push(
        "ufw",
        &[
            "--force",
            "delete",
            "allow",
            "in",
            "proto",
            "tcp",
            "from",
            "0.0.0.0/0",
            "to",
            "any",
            "port",
            "2222",
            "comment",
            "tundra-ssh-test-session",
        ],
        "Rule deleted",
        0,
    );
    fixture.push(
        "ufw",
        &["status", "numbered"],
        "Status: active\n[ 1] 5555/tcp ALLOW IN Anywhere\n",
        0,
    );
    fixture.push("ufw", &["show", "added"], "ufw allow 5555/tcp", 0);
    rollback_ports(&mut fixture, &rules[..1], &flag()).unwrap();
    assert!(fixture.commands.is_empty());
}

#[test]
fn boot_firewalld_removes_only_owned_permanent_rules_without_dbus() {
    let mut active = Fixture::firewalld("public\n rich rules:\n", "public\n rich rules:\n");
    let rules = prepare_ports(&mut active, "boot-test", &[2222], &flag()).unwrap();
    let rules = &rules[..2];
    let owned = &rules[1].value;
    let other = "rule family=\"ipv4\" port port=\"5555\" protocol=\"tcp\" accept";
    let mut stopped = Fixture {
        ufw: false,
        firewalld: true,
        owner: false,
        enabled: false,
        files: BTreeMap::new(),
        after_write: BTreeMap::new(),
        error: String::new(),
        commands: VecDeque::new(),
    };
    for _ in 0..2 {
        stopped.push(
            "firewall-offline-cmd",
            &["--zone=public", "--list-rich-rules"],
            &format!("{owned}\n{other}\n"),
            0,
        );
    }
    stopped.push(
        "firewall-offline-cmd",
        &["--zone=public", &format!("--remove-rich-rule={owned}")],
        "success",
        0,
    );
    stopped.push(
        "firewall-offline-cmd",
        &["--zone=public", "--list-rich-rules"],
        other,
        0,
    );
    change_boot_rules(&mut stopped, rules, false, false, &flag()).unwrap();
    assert!(stopped.commands.is_empty());
}

#[test]
fn boot_daemon_probe_uses_pid1_and_refuses_an_unready_service() {
    let mut fixture = Fixture {
        ufw: false,
        firewalld: true,
        owner: false,
        enabled: false,
        files: BTreeMap::new(),
        after_write: BTreeMap::new(),
        error: String::new(),
        commands: VecDeque::new(),
    };
    for output in [
        "ActiveState=inactive\nMainPID=0\n",
        "ActiveState=active\nMainPID=42\n",
        "ActiveState=activating\nMainPID=42\n",
    ] {
        fixture.push(
            "systemctl",
            &[
                "show",
                "firewalld.service",
                "--property=ActiveState",
                "--property=MainPID",
            ],
            output,
            0,
        );
    }
    assert!(!daemon_started(&mut fixture, &flag()).unwrap());
    assert!(daemon_started(&mut fixture, &flag()).unwrap());
    assert!(daemon_started(&mut fixture, &flag()).is_err());
    assert!(fixture.commands.is_empty());
}

#[test]
fn firewalld_without_an_owner_never_calls_a_potentially_activating_cli() {
    let mut fixture = Fixture::firewalld("", "");
    fixture.owner = false;
    fixture.commands.clear();
    let state = load(&mut fixture, &flag()).unwrap();
    assert_eq!(state.backend, Backend::Unsupported);
    assert!(!state.active);
    assert!(fixture.commands.is_empty());
}

#[test]
fn boot_ufw_accepts_only_the_known_reload_error_after_exact_saved_readback() {
    let mut active = Fixture::ufw("Status: active\n", true);
    let rules = prepare_ports(&mut active, "boot-test", &[2222], &flag()).unwrap();
    let rules = &rules[..1];
    let comment = "tundra-ssh-boot-test"
        .bytes()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let other = "### tuple ### allow tcp 5555 0.0.0.0/0 any 0.0.0.0/0 in\n";
    let owned = format!(
        "{other}### tuple ### allow tcp 2222 0.0.0.0/0 any 0.0.0.0/0 in comment={comment}\n"
    );
    for present in [false, true] {
        for (error, wrote, succeeds) in [
            ("ERROR: problem running\n", true, true),
            ("ERROR: problem running\n", false, false),
            ("ERROR: permission denied\n", true, false),
            ("ERROR: problem running\nERROR: unknown\n", true, false),
        ] {
            let mut fixture = Fixture::ufw("Status: inactive\n", true);
            fixture.files.insert(
                "/etc/ufw/user.rules".into(),
                if present { other.into() } else { owned.clone() },
            );
            fixture.after_write.insert(
                "/etc/ufw/user.rules".into(),
                if present == wrote {
                    owned.clone()
                } else {
                    other.into()
                },
            );
            fixture.error = error.into();
            let mut command = Vec::new();
            if !present {
                command.extend(args(&["--force", "delete"]));
            }
            command.extend(args(&[
                "allow",
                "in",
                "proto",
                "tcp",
                "from",
                "0.0.0.0/0",
                "to",
                "any",
                "port",
                "2222",
                "comment",
                "tundra-ssh-boot-test",
            ]));
            fixture
                .commands
                .push_back(("ufw".into(), command, String::new(), 1));
            if error == "ERROR: problem running\n" {
                fixture.push("ufw", &["status", "numbered"], "Status: inactive\n", 0);
                fixture.push("ufw", &["show", "added"], "ufw allow 5555/tcp", 0);
            }
            assert_eq!(
                change_boot_rules(&mut fixture, rules, present, false, &flag()).is_ok(),
                succeeds,
                "present={present}, wrote={wrote}, error={error}"
            );
            assert!(fixture.commands.is_empty());
        }
    }
}

#[test]
#[ignore = "requires the private firewalld-boot environment in scripts/tests/firewall-isolated.sh"]
fn real_boot_firewalld_offline_rules() {
    assert_eq!(unsafe { libc::geteuid() }, 0);
    assert_eq!(
        std::fs::read_to_string("/run/tundra-firewall-isolated").unwrap(),
        "private\n"
    );
    for (name, variable) in [
        ("net", "TUNDRA_HOST_NET_NS"),
        ("mnt", "TUNDRA_HOST_MNT_NS"),
        ("pid", "TUNDRA_HOST_PID_NS"),
    ] {
        assert_ne!(
            std::fs::read_link(format!("/proc/self/ns/{name}"))
                .unwrap()
                .to_string_lossy(),
            std::env::var(variable).unwrap()
        );
    }
    assert!(!Path::new("/run/dbus/system_bus_socket").exists());
    let mut fixture = Fixture::firewalld("public\n rich rules:\n", "public\n rich rules:\n");
    let rules = prepare_ports(&mut fixture, "real-boot-test", &[38134], &flag()).unwrap();
    let mut native = System;
    run(
        &mut native,
        "firewall-offline-cmd",
        &args(&["--zone=public", "--add-port=5555/tcp"]),
        &flag(),
    )
    .unwrap();
    change_boot_rules(&mut native, &rules, true, false, &flag()).unwrap();
    let (state, offline) = boot_state(&mut native, &rules, false, &flag()).unwrap();
    assert!(offline);
    assert_eq!(
        rules
            .iter()
            .filter(|rule| own_rule(&state, rule).is_some())
            .count(),
        2
    );
    assert!(
        rules
            .iter()
            .filter(|rule| rule.scope == "runtime")
            .all(|rule| own_rule(&state, rule).is_none())
    );
    change_boot_rules(&mut native, &rules, false, false, &flag()).unwrap();
    let (state, _) = boot_state(&mut native, &rules, false, &flag()).unwrap();
    assert!(rules.iter().all(|rule| own_rule(&state, rule).is_none()));
    assert_eq!(
        run(
            &mut native,
            "firewall-offline-cmd",
            &args(&["--zone=public", "--query-port=5555/tcp"]),
            &flag()
        )
        .unwrap()
        .trim(),
        "yes"
    );
    run(
        &mut native,
        "firewall-offline-cmd",
        &args(&["--check-config"]),
        &flag(),
    )
    .unwrap();
}
