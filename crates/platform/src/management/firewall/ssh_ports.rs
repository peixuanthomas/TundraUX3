use super::*;
pub(super) mod boot;
pub(crate) use boot::{
    apply_ssh_ports_boot_unrecorded, present_ssh_rules_boot, rollback_ssh_ports_boot_unrecorded,
};
/// A prepared rule is persisted by the SSH recovery journal before it is applied.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct OwnedPortRule {
    backend: Backend,
    transaction: String,
    pub(super) port: u16,
    family: String,
    zone: String,
    pub(super) scope: String,
    pub(super) value: String,
}
fn validate_owned(rule: &OwnedPortRule) -> Result<(), ManagementError> {
    if rule.port == 0
        || rule.transaction.is_empty()
        || rule.transaction.len() > 64
        || !rule
            .transaction
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        || !["ipv4", "ipv6"].contains(&rule.family.as_str())
    {
        return Err(invalid(i18n::tr!("platform-firewall-ssh-rule-invalid")));
    }
    if rule.backend == Backend::Ufw {
        if rule.scope != "both"
            || !rule.zone.is_empty()
            || rule.value != format!("tundra-ssh-{}", rule.transaction)
        {
            return Err(invalid(i18n::tr!("platform-firewall-ssh-ufw-invalid")));
        }
    } else if rule.backend == Backend::Firewalld {
        validate_zone(&rule.zone)?;
        if !["runtime", "permanent"].contains(&rule.scope.as_str())
            || rule.value != ssh_rich(&rule.transaction, rule.port, &rule.family)
        {
            return Err(invalid(i18n::tr!(
                "platform-firewall-ssh-firewalld-invalid"
            )));
        }
    } else {
        return Err(invalid(i18n::tr!("platform-firewall-ssh-backend-invalid")));
    }
    Ok(())
}
fn ssh_rich(transaction: &str, port: u16, family: &str) -> String {
    let digest = Sha256::digest(transaction.as_bytes());
    let priority = -(i32::from(u16::from_be_bytes([digest[0], digest[1]]) % 32767) + 1);
    format!(
        "rule priority=\"{priority}\" family=\"{family}\" port port=\"{port}\" protocol=\"tcp\" accept"
    )
}
fn ufw_covers(value: &str, port: u16, family: &str) -> bool {
    let value = value.split('#').next().unwrap_or(value).trim();
    let Some((destination, source)) = value.split_once("ALLOW IN") else {
        return false;
    };
    let destination = destination.trim();
    let ipv6 = destination.ends_with(" (v6)");
    if ipv6 != (family == "ipv6") {
        return false;
    }
    let destination = destination.strip_suffix(" (v6)").unwrap_or(destination);
    if !["Anywhere", "Anywhere (v6)"].contains(&source.trim()) {
        return false;
    }
    if destination == "Anywhere" {
        return true;
    }
    let port_text = if let Some((port, protocol)) = destination.split_once('/') {
        if protocol != "tcp" {
            return false;
        }
        port
    } else {
        destination
    };
    let Ok(range) = port_range(port_text) else {
        return false;
    };
    let (first, last) = range.split_once('-').unwrap_or((&range, &range));
    first.parse::<u16>().is_ok_and(|first| first <= port)
        && last.parse::<u16>().is_ok_and(|last| last >= port)
}
fn firewalld_covers(state: &State, zone: &str, scope: &str, port: u16, family: &str) -> bool {
    state.rules.iter().any(|rule| {
        if rule.zone != zone || rule.scope != scope {
            return false;
        }
        if rule.kind == "port" {
            let Some((range, protocol)) = rule.value.split_once('/') else {
                return false;
            };
            if protocol != "tcp" {
                return false;
            }
            let Ok(range) = port_range(range) else {
                return false;
            };
            let (first, last) = range.split_once('-').unwrap_or((&range, &range));
            first.parse::<u16>().is_ok_and(|first| first <= port)
                && last.parse::<u16>().is_ok_and(|last| last >= port)
        } else {
            rule.kind == "rich"
                && (rule.value
                    == format!(
                        "rule family=\"{family}\" port port=\"{port}\" protocol=\"tcp\" accept"
                    )
                    || rule.value == format!("rule port port=\"{port}\" protocol=\"tcp\" accept"))
        }
    })
}
fn service_covers(
    native: &mut impl Native,
    state: &State,
    zone: &str,
    scope: &str,
    port: u16,
    flag: &AtomicBool,
) -> Result<bool, ManagementError> {
    for service in state
        .rules
        .iter()
        .filter(|rule| rule.zone == zone && rule.scope == scope && rule.kind == "service")
    {
        let mut command = Vec::new();
        if scope == "permanent" {
            command.push("--permanent".into());
        }
        command.push(format!("--info-service={}", service.value));
        let info = run(native, "firewall-cmd", &command, flag)?;
        if info.lines().any(|line| {
            line.trim()
                .strip_prefix("destination:")
                .is_some_and(|value| !value.trim().is_empty())
        }) {
            continue;
        }
        let ports = info
            .lines()
            .find_map(|line| line.trim().strip_prefix("ports:"))
            .unwrap_or("");
        for value in ports.split_whitespace() {
            let Some((range, protocol)) = value.split_once('/') else {
                continue;
            };
            if protocol != "tcp" {
                continue;
            }
            let range = port_range(range)?;
            let (first, last) = range.split_once('-').unwrap_or((&range, &range));
            if first.parse::<u16>().is_ok_and(|first| first <= port)
                && last.parse::<u16>().is_ok_and(|last| last >= port)
            {
                return Ok(true);
            }
        }
    }
    Ok(false)
}
pub(crate) fn prepare_ssh_ports(
    transaction: &str,
    ports: &[u16],
    flag: &AtomicBool,
) -> Result<Vec<OwnedPortRule>, ManagementError> {
    prepare_ports(&mut System, transaction, ports, flag)
}
pub(crate) fn present_ssh_rules(
    rules: &[OwnedPortRule],
    flag: &AtomicBool,
) -> Result<Vec<OwnedPortRule>, ManagementError> {
    if rules.is_empty() {
        return Ok(Vec::new());
    }
    for rule in rules {
        validate_owned(rule)?;
    }
    let state = load(&mut System, flag)?;
    if state.simultaneous
        || !state.active
        || state.issue.is_some()
        || rules.iter().any(|rule| rule.backend != state.backend)
    {
        return Err(conflict());
    }
    Ok(rules
        .iter()
        .filter(|rule| own_rule(&state, rule).is_some())
        .cloned()
        .collect())
}
pub(super) fn prepare_ports(
    native: &mut impl Native,
    transaction: &str,
    ports: &[u16],
    flag: &AtomicBool,
) -> Result<Vec<OwnedPortRule>, ManagementError> {
    let state = load(native, flag)?;
    if state.simultaneous {
        return Err(ManagementError::Conflict(
            i18n::tr!("platform-firewall-both-running").into(),
        ));
    }
    if !state.active || state.backend == Backend::Unsupported {
        return Err(ManagementError::Unavailable(
            i18n::tr!("platform-firewall-no-running-backend").into(),
        ));
    }
    if let Some(issue) = state.issue {
        return Err(ManagementError::Unavailable(issue));
    }
    let mut result = Vec::new();
    let ipv6 = native
        .read("/etc/default/ufw")
        .is_none_or(|text| !text.lines().any(|line| line.trim() == "IPV6=no"));
    for port in ports.iter().copied().collect::<BTreeSet<_>>() {
        for family in ["ipv4", "ipv6"] {
            if state.backend == Backend::Ufw {
                if family == "ipv6" && !ipv6 {
                    continue;
                }
                if state
                    .rules
                    .iter()
                    .any(|rule| ufw_covers(&rule.value, port, family))
                {
                    continue;
                }
                let rule = OwnedPortRule {
                    backend: state.backend,
                    transaction: transaction.into(),
                    port,
                    family: family.into(),
                    zone: String::new(),
                    scope: "both".into(),
                    value: format!("tundra-ssh-{transaction}"),
                };
                validate_owned(&rule)?;
                result.push(rule);
            } else {
                for scope in ["runtime", "permanent"] {
                    if firewalld_covers(&state, &state.default_zone, scope, port, family)
                        || service_covers(native, &state, &state.default_zone, scope, port, flag)?
                    {
                        continue;
                    }
                    let rule = OwnedPortRule {
                        backend: state.backend,
                        transaction: transaction.into(),
                        port,
                        family: family.into(),
                        zone: state.default_zone.clone(),
                        scope: scope.into(),
                        value: ssh_rich(transaction, port, family),
                    };
                    validate_owned(&rule)?;
                    if state.rules.iter().any(|existing| {
                        existing.zone == rule.zone
                            && existing.scope == rule.scope
                            && existing.kind == "rich"
                            && existing.value == rule.value
                    }) {
                        return Err(conflict());
                    }
                    result.push(rule);
                }
            }
        }
    }
    Ok(result)
}
pub(super) fn own_rule<'a>(state: &'a State, rule: &OwnedPortRule) -> Option<&'a Rule> {
    state.rules.iter().find(|existing| {
        if rule.backend == Backend::Ufw {
            existing
                .value
                .split_once('#')
                .is_some_and(|(value, comment)| {
                    comment.trim() == rule.value && ufw_covers(value, rule.port, &rule.family)
                })
        } else {
            existing.zone == rule.zone
                && existing.scope == rule.scope
                && existing.kind == "rich"
                && existing.value == rule.value
        }
    })
}
fn owned_ufw_input(rule: &OwnedPortRule) -> Input {
    Input {
        protocol: "tcp".into(),
        port: rule.port.to_string(),
        source: if rule.family == "ipv4" {
            "0.0.0.0/0"
        } else {
            "::/0"
        }
        .into(),
        zone: String::new(),
        duration: "both".into(),
    }
}
fn owned_ufw_delete(rule: &OwnedPortRule) -> Vec<String> {
    let mut command = args(&["--force", "delete"]);
    command.extend(ufw_add(&owned_ufw_input(rule), Some(&rule.value)));
    command
}
fn owned_change(rule: &OwnedPortRule) -> Change {
    Change {
        zone: rule.zone.clone(),
        scope: rule.scope.clone(),
        kind: "rich".into(),
        value: rule.value.clone(),
        before: false,
        after: true,
    }
}
fn already_allowed(
    native: &mut impl Native,
    state: &State,
    rule: &OwnedPortRule,
    flag: &AtomicBool,
) -> Result<bool, ManagementError> {
    if rule.backend == Backend::Ufw {
        Ok(state
            .rules
            .iter()
            .any(|existing| ufw_covers(&existing.value, rule.port, &rule.family)))
    } else {
        Ok(
            firewalld_covers(state, &rule.zone, &rule.scope, rule.port, &rule.family)
                || service_covers(native, state, &rule.zone, &rule.scope, rule.port, flag)?,
        )
    }
}
pub(crate) fn apply_ssh_ports_unrecorded(
    rules: &[OwnedPortRule],
    flag: &AtomicBool,
) -> Result<(), ManagementError> {
    if rules.is_empty() {
        return Ok(());
    }
    let _lock = lock_rules(flag)?;
    apply_ports(&mut System, rules, flag)
}
pub(crate) fn ssh_backup_paths(rules: &[OwnedPortRule]) -> Vec<PathBuf> {
    if rules.iter().any(|rule| rule.backend == Backend::Ufw) {
        ufw_paths()
    } else {
        Vec::new()
    }
}
pub(super) fn apply_ports(
    native: &mut impl Native,
    rules: &[OwnedPortRule],
    flag: &AtomicBool,
) -> Result<(), ManagementError> {
    if rules.is_empty() {
        return Ok(());
    }
    for rule in rules {
        validate_owned(rule)?;
    }
    let state = load(native, flag)?;
    if !state.active
        || state.simultaneous
        || state.issue.is_some()
        || rules
            .iter()
            .any(|rule| rule.backend != state.backend || own_rule(&state, rule).is_some())
    {
        return Err(conflict());
    }
    for rule in rules {
        if already_allowed(native, &state, rule, flag)? {
            return Err(conflict());
        }
    }
    for rule in rules {
        let current = load(native, flag)?;
        if current.backend != rule.backend
            || !current.active
            || current.simultaneous
            || current.issue.is_some()
            || own_rule(&current, rule).is_some()
            || already_allowed(native, &current, rule, flag)?
        {
            return Err(conflict());
        }
        if rule.backend == Backend::Ufw {
            run(
                native,
                "ufw",
                &ufw_add(&owned_ufw_input(rule), Some(&rule.value)),
                flag,
            )?;
        } else {
            run(
                native,
                "firewall-cmd",
                &native_change(&owned_change(rule), true)?,
                flag,
            )?;
        }
        if own_rule(&load(native, flag)?, rule).is_none() {
            return Err(ManagementError::Failed(
                i18n::tr!("platform-firewall-ssh-not-created").into(),
            ));
        }
    }
    let after = load(native, flag)?;
    if rules.iter().any(|rule| own_rule(&after, rule).is_none()) {
        return Err(ManagementError::Failed(
            i18n::tr!("platform-firewall-ssh-changed").into(),
        ));
    }
    Ok(())
}
pub(crate) fn rollback_ssh_ports_unrecorded(
    rules: &[OwnedPortRule],
    flag: &AtomicBool,
) -> Result<(), ManagementError> {
    if rules.is_empty() {
        return Ok(());
    }
    let _lock = lock_rules(flag)?;
    rollback_ports(&mut System, rules, flag)
}
pub(super) fn rollback_ports(
    native: &mut impl Native,
    rules: &[OwnedPortRule],
    flag: &AtomicBool,
) -> Result<(), ManagementError> {
    for rule in rules {
        validate_owned(rule)?;
    }
    for rule in rules.iter().rev() {
        let current = load(native, flag)?;
        if current.backend != rule.backend
            || !current.active
            || current.simultaneous
            || current.issue.is_some()
        {
            return Err(conflict());
        }
        let Some(_) = own_rule(&current, rule) else {
            continue;
        };
        if rule.backend == Backend::Ufw {
            run(native, "ufw", &owned_ufw_delete(rule), flag)?;
        } else {
            run(
                native,
                "firewall-cmd",
                &native_change(&owned_change(rule), false)?,
                flag,
            )?;
        }
    }
    let after = load(native, flag)?;
    if rules.iter().any(|rule| own_rule(&after, rule).is_some()) {
        return Err(ManagementError::Failed(
            i18n::tr!("platform-firewall-ssh-rollback-remains").into(),
        ));
    }
    Ok(())
}
