use super::*;

pub(in crate::management::firewall) fn daemon_started(
    native: &mut impl Native,
    flag: &AtomicBool,
) -> Result<bool, ManagementError> {
    if !native.installed("firewall-cmd") {
        return Ok(false);
    }
    let state = run(
        native,
        "systemctl",
        &args(&[
            "show",
            "firewalld.service",
            "--property=ActiveState",
            "--property=MainPID",
        ]),
        flag,
    )?;
    let active = state
        .lines()
        .find_map(|line| line.strip_prefix("ActiveState="));
    let pid = state
        .lines()
        .find_map(|line| line.strip_prefix("MainPID="))
        .and_then(|pid| pid.parse::<u32>().ok());
    match (active, pid) {
        (Some("active"), Some(pid)) if pid != 0 => Ok(true),
        (Some("inactive" | "failed"), Some(0)) => Ok(false),
        _ => Err(ManagementError::Unavailable(
            i18n::tr!("platform-firewall-boot-changing").into(),
        )),
    }
}

fn saved_ufw_rule(
    native: &impl Native,
    rule: &OwnedPortRule,
) -> Result<(bool, bool), ManagementError> {
    let address = if rule.family == "ipv4" {
        "0.0.0.0/0"
    } else {
        "::/0"
    };
    let path = if rule.family == "ipv4" {
        "/etc/ufw/user.rules"
    } else {
        "/etc/ufw/user6.rules"
    };
    let text = native.read(path).ok_or_else(|| {
        ManagementError::Unavailable(i18n::tr!(
            "platform-firewall-saved-ufw-read",
            path = path.to_string()
        ))
    })?;
    let expected = format!(
        "### tuple ### allow tcp {} {address} any {address} in",
        rule.port
    );
    let comment = rule
        .value
        .bytes()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let mut owned = false;
    let mut other = false;
    for line in text.lines() {
        if let Some(rest) = line.trim().strip_prefix(&expected) {
            if rest == format!(" comment={comment}") {
                owned = true;
            } else if rest.is_empty() || rest.starts_with(" comment=") {
                other = true;
            }
        }
    }
    Ok((owned, other))
}

pub(in crate::management::firewall) fn boot_state(
    native: &mut impl Native,
    rules: &[OwnedPortRule],
    daemon_running: bool,
    flag: &AtomicBool,
) -> Result<(State, bool), ManagementError> {
    let backend = rules
        .first()
        .ok_or_else(|| invalid(i18n::tr!("platform-firewall-no-ssh-rules")))?
        .backend;
    for rule in rules {
        validate_owned(rule)?;
        if rule.backend != backend {
            return Err(conflict());
        }
    }
    let ufw_status = if native.installed("ufw") {
        run(native, "ufw", &args(&["status", "numbered"]), flag)?
    } else {
        String::new()
    };
    let ufw_active = ufw_status
        .lines()
        .any(|line| line.trim() == "Status: active");
    let fw_active = if daemon_running {
        let (output, error, code) = native.command("firewall-cmd", &args(&["--state"]), flag)?;
        match (code, output.trim()) {
            (0, "running") => true,
            (252, _) => false,
            _ => {
                return Err(ManagementError::Failed(i18n::tr!(
                    "platform-firewall-boot-state-error",
                    code = code.to_string(),
                    error = error.to_string(),
                    output = output.to_string()
                )));
            }
        }
    } else {
        false
    };
    if fw_active {
        if ufw_active || backend != Backend::Firewalld {
            return Err(conflict());
        }
        return Ok((load(native, flag)?, false));
    }
    if backend == Backend::Firewalld && ufw_active {
        return Err(conflict());
    }
    let mut state = State {
        backend,
        active: ufw_active,
        simultaneous: false,
        issue: None,
        fingerprint: String::new(),
        raw: String::new(),
        rules: Vec::new(),
        zones: Vec::new(),
        default_zone: String::new(),
        detail: Vec::new(),
    };
    if backend == Backend::Ufw {
        if !native.installed("ufw") {
            return Err(conflict());
        }
        if ufw_active {
            state.rules = parse_ufw(&ufw_status)?;
        } else {
            run(native, "ufw", &args(&["show", "added"]), flag)?;
            // UFW's saved command display merges address families; its tuple records retain them.
            for rule in rules {
                if saved_ufw_rule(native, rule)?.0 {
                    let suffix = if rule.family == "ipv6" { " (v6)" } else { "" };
                    state.rules.push(Rule {
                        id: String::new(),
                        zone: String::new(),
                        scope: "both".into(),
                        kind: "saved".into(),
                        value: format!(
                            "{}/tcp{suffix} ALLOW IN Anywhere{suffix} # {}",
                            rule.port, rule.value
                        ),
                    });
                }
            }
        }
        return Ok((state, false));
    }
    let zones = rules
        .iter()
        .filter(|rule| rule.scope == "permanent")
        .map(|rule| rule.zone.as_str())
        .collect::<BTreeSet<_>>();
    for zone in zones {
        let text = run(
            native,
            "firewall-offline-cmd",
            &vec![format!("--zone={zone}"), "--list-rich-rules".into()],
            flag,
        )?;
        for value in text.lines().map(str::trim).filter(|line| !line.is_empty()) {
            state.rules.push(Rule {
                id: hash(value),
                zone: zone.into(),
                scope: "permanent".into(),
                kind: "rich".into(),
                value: value.into(),
            });
        }
    }
    Ok((state, true))
}

pub(crate) fn present_ssh_rules_boot(
    rules: &[OwnedPortRule],
    flag: &AtomicBool,
) -> Result<Vec<OwnedPortRule>, ManagementError> {
    if rules.is_empty() {
        return Ok(Vec::new());
    }
    let mut native = System;
    let running = daemon_started(&mut native, flag)?;
    let (state, _) = boot_state(&mut native, rules, running, flag)?;
    Ok(rules
        .iter()
        .filter(|rule| own_rule(&state, rule).is_some())
        .cloned()
        .collect())
}

pub(in crate::management::firewall) fn change_boot_rules(
    native: &mut impl Native,
    rules: &[OwnedPortRule],
    present: bool,
    daemon_running: bool,
    flag: &AtomicBool,
) -> Result<(), ManagementError> {
    for rule in rules {
        let (state, offline) = boot_state(native, rules, daemon_running, flag)?;
        if offline && rule.scope == "runtime" {
            continue;
        }
        let existing = own_rule(&state, rule);
        if existing.is_some() == present {
            continue;
        }
        let (tool, command) = if rule.backend == Backend::Ufw {
            if present {
                if (!state.active && saved_ufw_rule(native, rule)?.1)
                    || (state.active && already_allowed(native, &state, rule, flag)?)
                {
                    return Err(conflict());
                }
                ("ufw", ufw_add(&owned_ufw_input(rule), Some(&rule.value)))
            } else {
                ("ufw", owned_ufw_delete(rule))
            }
        } else {
            let mut command = native_change(&owned_change(rule), present)?;
            if offline {
                command.retain(|argument| argument != "--permanent");
            }
            (
                if offline {
                    "firewall-offline-cmd"
                } else {
                    "firewall-cmd"
                },
                command,
            )
        };
        let enabled_before = rule.backend == Backend::Ufw && ufw_enabled(native);
        crate::management::network::check_cancelled(flag)?;
        let (output, error, code) = native.command(tool, &command, flag)?;
        let saved_only_reload_error = tool == "ufw"
            && !state.active
            && enabled_before
            && code == 1
            && output.trim().is_empty()
            && error
                .lines()
                .filter(|line| !line.is_empty() && !line.starts_with("WARN:"))
                .collect::<Vec<_>>()
                == ["ERROR: problem running"];
        let failed = || {
            ManagementError::Failed(i18n::tr!(
                "platform-firewall-native-failed",
                tool = tool.to_string(),
                code = code.to_string(),
                error = error.to_string(),
                output = output.to_string()
            ))
        };
        if code != 0 && !saved_only_reload_error {
            return Err(failed());
        }
        let (after, _) = boot_state(native, rules, daemon_running, flag)?;
        if saved_only_reload_error
            && (after.active || !ufw_enabled(native) || own_rule(&after, rule).is_some() != present)
        {
            return Err(failed());
        }
        if own_rule(&after, rule).is_some() != present {
            return Err(ManagementError::Failed(
                i18n::tr!("platform-firewall-boot-unverified").into(),
            ));
        }
    }
    Ok(())
}

pub(crate) fn apply_ssh_ports_boot_unrecorded(
    rules: &[OwnedPortRule],
    flag: &AtomicBool,
) -> Result<(), ManagementError> {
    if rules.is_empty() {
        return Ok(());
    }
    let _lock = lock_rules(flag)?;
    let mut native = System;
    let running = daemon_started(&mut native, flag)?;
    change_boot_rules(&mut native, rules, true, running, flag)
}
pub(crate) fn rollback_ssh_ports_boot_unrecorded(
    rules: &[OwnedPortRule],
    flag: &AtomicBool,
) -> Result<(), ManagementError> {
    if rules.is_empty() {
        return Ok(());
    }
    let _lock = lock_rules(flag)?;
    let mut native = System;
    let running = daemon_started(&mut native, flag)?;
    change_boot_rules(&mut native, rules, false, running, flag)
}
