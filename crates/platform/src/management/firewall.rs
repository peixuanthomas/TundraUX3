//! Native UFW/firewalld rules. Never installs, enables, or switches a firewall.
use super::*;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::net::IpAddr;
use std::os::fd::AsRawFd;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::Path;

mod ssh_ports;
#[cfg(test)]
#[path = "../../tests/unit/management/firewall_tests.rs"]
mod tests;
pub(crate) use ssh_ports::{
    OwnedPortRule, apply_ssh_ports_boot_unrecorded, apply_ssh_ports_unrecorded, prepare_ssh_ports,
    present_ssh_rules, present_ssh_rules_boot, rollback_ssh_ports_boot_unrecorded,
    rollback_ssh_ports_unrecorded, ssh_backup_paths,
};

trait Native {
    fn installed(&self, name: &str) -> bool;
    fn firewalld_owner(&self, flag: &AtomicBool) -> Result<bool, ManagementError>;
    fn read(&self, path: &str) -> Option<String>;
    fn command(
        &mut self,
        name: &str,
        args: &[String],
        flag: &AtomicBool,
    ) -> Result<(String, String, i32), ManagementError>;
}

struct System;
impl Native for System {
    fn installed(&self, name: &str) -> bool {
        super::network::program(name).is_ok()
    }
    fn firewalld_owner(&self, flag: &AtomicBool) -> Result<bool, ManagementError> {
        super::network::check_cancelled(flag)?;
        let failed = |error: crate::service::ServiceError| {
            ManagementError::Unavailable(i18n::tr!(
                "platform-firewall-owner-error",
                error = error.to_string()
            ))
        };
        let bus = crate::linux::dbus::system().map_err(failed)?;
        let owner =
            crate::linux::dbus::has_owner(&bus, "org.fedoraproject.FirewallD1").map_err(failed)?;
        super::network::check_cancelled(flag)?;
        Ok(owner)
    }
    fn read(&self, path: &str) -> Option<String> {
        std::fs::read_to_string(path).ok()
    }
    fn command(
        &mut self,
        name: &str,
        args: &[String],
        flag: &AtomicBool,
    ) -> Result<(String, String, i32), ManagementError> {
        super::network::run_status(
            name,
            &args.iter().map(String::as_str).collect::<Vec<_>>(),
            flag,
        )
    }
}

fn args(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).into()).collect()
}
fn run(
    native: &mut impl Native,
    tool: &str,
    arguments: &[String],
    flag: &AtomicBool,
) -> Result<String, ManagementError> {
    super::network::check_cancelled(flag)?;
    let (output, error, code) = native.command(tool, arguments, flag)?;
    if code == 0 {
        Ok(output)
    } else {
        Err(ManagementError::Failed(i18n::tr!(
            "platform-firewall-native-failed",
            tool = tool.to_string(),
            code = code.to_string(),
            error = error.to_string(),
            output = output.to_string()
        )))
    }
}
fn hash(text: &str) -> String {
    format!("{:x}", Sha256::digest(text.as_bytes()))
}
fn invalid(message: impl Into<String>) -> ManagementError {
    ManagementError::InvalidInput(message.into())
}
fn conflict() -> ManagementError {
    ManagementError::Conflict(i18n::tr!("platform-firewall-changed").into())
}
pub(crate) fn lock_rules(flag: &AtomicBool) -> Result<std::fs::File, ManagementError> {
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open("/run/tundraux3-firewall.lock")
        .map_err(super::network::io_error)?;
    let metadata = file.metadata().map_err(super::network::io_error)?;
    if !metadata.is_file()
        || metadata.uid() != unsafe { libc::geteuid() }
        || metadata.nlink() != 1
        || metadata.mode() & 0o077 != 0
    {
        return Err(ManagementError::PermissionDenied(
            i18n::tr!("platform-firewall-unsafe-lock").into(),
        ));
    }
    loop {
        super::network::check_cancelled(flag)?;
        if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 {
            return Ok(file);
        }
        let error = std::io::Error::last_os_error();
        if error.kind() != std::io::ErrorKind::WouldBlock {
            return Err(super::network::io_error(error));
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}
fn field(
    id: &str,
    label: impl Into<String>,
    value: &str,
    required: bool,
    choices: &[&str],
) -> ManagementField {
    ManagementField {
        id: id.into(),
        label: label.into(),
        value: value.into(),
        required,
        choices: choices.iter().map(|v| (*v).into()).collect(),
        ..Default::default()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Backend {
    Ufw,
    Firewalld,
    Unsupported,
}
impl Backend {
    fn id(self) -> &'static str {
        match self {
            Self::Ufw => "ufw",
            Self::Firewalld => "firewalld",
            Self::Unsupported => "unsupported",
        }
    }
}
#[derive(Debug, Clone)]
struct Rule {
    id: String,
    zone: String,
    scope: String,
    kind: String,
    value: String,
}
#[derive(Debug, Clone)]
struct State {
    backend: Backend,
    active: bool,
    simultaneous: bool,
    issue: Option<String>,
    fingerprint: String,
    raw: String,
    rules: Vec<Rule>,
    zones: Vec<String>,
    default_zone: String,
    detail: Vec<(String, String)>,
}

fn ufw_enabled(native: &impl Native) -> bool {
    native
        .read("/etc/ufw/ufw.conf")
        .is_some_and(|text| text.lines().any(|line| line.trim() == "ENABLED=yes"))
}
fn load(native: &mut impl Native, flag: &AtomicBool) -> Result<State, ManagementError> {
    let ufw = native.installed("ufw");
    let firewalld = native.installed("firewall-cmd");
    let mut ufw_active = ufw && ufw_enabled(native);
    let (fw_active, fw_failed) = if firewalld && native.firewalld_owner(flag)? {
        let (output, error, code) = native.command("firewall-cmd", &args(&["--state"]), flag)?;
        match (code, output.trim()) {
            (0, "running") => (true, false),
            (251, _) => (true, true),
            (252, _) => (false, false),
            _ => {
                return Err(ManagementError::Failed(i18n::tr!(
                    "platform-firewall-state-error",
                    code = code.to_string(),
                    error = error.to_string(),
                    output = output.to_string()
                )));
            }
        }
    } else {
        (false, false)
    };
    let ufw_status = if ufw {
        let (output, error, code) = native.command("ufw", &args(&["status", "numbered"]), flag)?;
        if code == 0 {
            ufw_active = output.lines().any(|line| line.trim() == "Status: active");
            Ok(output)
        } else {
            Err(i18n::tr!(
                "platform-firewall-ufw-read-error",
                error = error.to_string()
            ))
        }
    } else {
        Ok(String::new())
    };
    let simultaneous = ufw_active && fw_active;
    let backend = if fw_active {
        Backend::Firewalld
    } else if ufw {
        Backend::Ufw
    } else {
        Backend::Unsupported
    };
    let mut state = State {
        backend,
        active: ufw_active || fw_active,
        simultaneous,
        issue: simultaneous
            .then(|| i18n::tr!("platform-firewall-both-running").into())
            .or_else(|| fw_failed.then(|| i18n::tr!("platform-firewall-firewalld-failed").into())),
        fingerprint: String::new(),
        raw: String::new(),
        rules: Vec::new(),
        zones: Vec::new(),
        default_zone: String::new(),
        detail: Vec::new(),
    };
    match backend {
        Backend::Ufw => match ufw_status {
            Ok(status) => {
                state.raw = status;
                match run(native, "ufw", &args(&["show", "added"]), flag) {
                    Ok(added) => state
                        .detail
                        .push((i18n::tr!("platform-firewall-saved-ufw").into(), added)),
                    Err(error) => state.issue = Some(error.to_string()),
                }
                state.rules = parse_ufw(&state.raw)?;
                if !state.active {
                    if let Some((_, added)) = state.detail.first() {
                        state.rules = parse_saved_ufw(added)?;
                    }
                }
                state.detail.push((
                    i18n::tr!("platform-firewall-effective").into(),
                    if state.active {
                        i18n::tr!("platform-firewall-yes")
                    } else {
                        i18n::tr!("platform-firewall-ufw-inactive")
                    }
                    .into(),
                ));
                state.detail.push((
                    i18n::tr!("platform-firewall-saved").into(),
                    i18n::tr!("platform-firewall-ufw-saved").into(),
                ));
                state.detail.push((
                    i18n::tr!("platform-firewall-after-restart").into(),
                    if state.active {
                        i18n::tr!("platform-firewall-saved-ufw")
                    } else {
                        i18n::tr!("platform-firewall-inactive-until-enabled")
                    }
                    .into(),
                ));
            }
            Err(error) => state.issue = Some(error),
        },
        Backend::Firewalld => {
            state.default_zone = run(native, "firewall-cmd", &args(&["--get-default-zone"]), flag)?;
            validate_zone(&state.default_zone)?;
            let runtime_zones = run(native, "firewall-cmd", &args(&["--list-all-zones"]), flag)?;
            let permanent_zones = run(
                native,
                "firewall-cmd",
                &args(&["--permanent", "--list-all-zones"]),
                flag,
            )?;
            let active_zones = run(native, "firewall-cmd", &args(&["--get-active-zones"]), flag)?;
            state.detail.push((
                i18n::tr!("platform-firewall-active-zones").into(),
                active_zones,
            ));
            state.detail.push((
                i18n::tr!("platform-firewall-default-zone").into(),
                state.default_zone.clone(),
            ));
            for (scope, text) in [("runtime", runtime_zones), ("permanent", permanent_zones)] {
                for (zone, text) in zone_blocks(&text)? {
                    state.rules.extend(parse_firewalld(&zone, scope, &text)?);
                    state.raw.push_str(&format!("{zone}/{scope}\n{text}\n"));
                    state.detail.push((format!("{zone} / {scope}"), text));
                    state.zones.push(zone);
                }
            }
            state.zones.sort();
            state.zones.dedup();
        }
        Backend::Unsupported => state.detail.push((
            i18n::tr!("management-requirements"),
            i18n::tr!(if firewalld {
                "management-firewalld-inactive"
            } else {
                "management-firewall-tools-missing"
            }),
        )),
    }
    state.fingerprint = hash(&format!(
        "{:?}:{}:{}:{}:{:?}",
        state.backend, state.active, state.simultaneous, state.raw, state.detail
    ));
    Ok(state)
}

fn zone_blocks(text: &str) -> Result<Vec<(String, String)>, ManagementError> {
    let mut blocks: Vec<(String, String)> = Vec::new();
    for line in text.lines() {
        if line.trim().is_empty() {
            continue;
        }
        if !line.starts_with(char::is_whitespace) {
            let zone = line
                .split_whitespace()
                .next()
                .ok_or_else(|| invalid(i18n::tr!("platform-firewall-zones-error")))?;
            validate_zone(zone)?;
            blocks.push((zone.into(), format!("{line}\n")));
        } else if let Some((_, block)) = blocks.last_mut() {
            block.push_str(line);
            block.push('\n');
        } else {
            return Err(invalid(i18n::tr!("platform-firewall-zone-header-error")));
        }
    }
    Ok(blocks)
}
fn parse_ufw(text: &str) -> Result<Vec<Rule>, ManagementError> {
    let mut rules = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        let Some(rest) = line.strip_prefix('[') else {
            continue;
        };
        let (number, value) = rest
            .split_once(']')
            .ok_or_else(|| invalid(i18n::tr!("platform-firewall-number-error")))?;
        let number: u32 = number
            .trim()
            .parse()
            .map_err(|_| invalid(i18n::tr!("platform-firewall-number-error")))?;
        if number == 0 || value.trim().is_empty() {
            return Err(invalid(i18n::tr!("platform-firewall-number-error")));
        }
        rules.push(Rule {
            id: number.to_string(),
            zone: String::new(),
            scope: "both".into(),
            kind: "number".into(),
            value: value.trim().into(),
        });
    }
    Ok(rules)
}
fn parse_saved_ufw(text: &str) -> Result<Vec<Rule>, ManagementError> {
    let mut rules = Vec::new();
    for line in text
        .lines()
        .map(str::trim)
        .filter(|line| line.starts_with("ufw "))
    {
        let words = shlex::split(line)
            .ok_or_else(|| invalid(i18n::tr!("platform-firewall-saved-rules-error")))?;
        if words.len() < 3 {
            return Err(invalid(i18n::tr!("platform-firewall-saved-rules-error")));
        }
        rules.push(Rule {
            id: hash(line),
            zone: String::new(),
            scope: "both".into(),
            kind: "saved".into(),
            value: line.into(),
        });
    }
    Ok(rules)
}
fn is_allow(rule: &Rule, backend: Backend) -> bool {
    if backend == Backend::Ufw {
        if rule.kind == "saved" {
            shlex::split(&rule.value)
                .is_some_and(|words| words.get(1).map(String::as_str) == Some("allow"))
        } else {
            rule.value
                .split('#')
                .next()
                .unwrap_or("")
                .contains(" ALLOW ")
        }
    } else {
        rule.kind != "rich"
            || rule
                .value
                .rsplit_once(" accept")
                .is_some_and(|(_, rest)| rest.is_empty() || rest.starts_with(" limit "))
    }
}
fn parse_firewalld(zone: &str, scope: &str, text: &str) -> Result<Vec<Rule>, ManagementError> {
    let mut result = Vec::new();
    let mut rich = false;
    for line in text.lines() {
        let trimmed = line.trim();
        if rich && trimmed.starts_with("rule ") {
            result.push(Rule {
                id: hash(&format!("{zone}/{scope}/rich/{trimmed}")),
                zone: zone.into(),
                scope: scope.into(),
                kind: "rich".into(),
                value: trimmed.into(),
            });
        } else if let Some((key, values)) = trimmed.split_once(':') {
            rich = key == "rich rules";
            let kind = match key {
                "ports" => "port",
                "services" => "service",
                "source-ports" => "source_port",
                _ => continue,
            };
            for value in values.split_whitespace() {
                result.push(Rule {
                    id: hash(&format!("{zone}/{scope}/{kind}/{value}")),
                    zone: zone.into(),
                    scope: scope.into(),
                    kind: kind.into(),
                    value: value.into(),
                });
            }
        } else if rich && !trimmed.is_empty() {
            return Err(invalid(i18n::tr!("platform-firewall-rich-error")));
        }
    }
    Ok(result)
}

fn switch_action(id: &str, label: impl Into<String>) -> ManagementAction {
    ManagementAction {
        id: id.into(),
        label: label.into(),
        ..Default::default()
    }
}
fn inspect_action(scope: &str, filter: &str) -> ManagementAction {
    ManagementAction {
        id: "inspect_firewall".into(),
        label: i18n::tr!("platform-firewall-authorize").into(),
        privileged: true,
        values: BTreeMap::from([
            ("scope".into(), scope.into()),
            ("filter".into(), filter.into()),
        ]),
        ..Default::default()
    }
}

pub fn query(
    query: &ManagementQuery,
    flag: &AtomicBool,
) -> Result<ManagementSnapshot, ManagementError> {
    if query.scope == "listeners" {
        let (rows, notices) =
            super::listening_ports::query(Path::new("/proc"), &query.filter, flag)?;
        let mut actions = vec![switch_action(
            "show_firewall_rules",
            i18n::tr!("platform-firewall-rules"),
        )];
        if unsafe { libc::geteuid() } != 0 {
            actions.push(inspect_action("listeners", &query.filter));
        }
        return Ok(ManagementSnapshot {
            columns: vec![
                i18n::tr!("platform-firewall-protocol").into(),
                i18n::tr!("platform-firewall-address-family").into(),
                i18n::tr!("platform-firewall-address-port").into(),
                i18n::tr!("platform-firewall-pid-process").into(),
            ],
            rows,
            notices,
            actions,
            backend: i18n::tr!("platform-firewall-listeners-backend").into(),
            details_complete: true,
            ..Default::default()
        });
    }
    match load(&mut System, flag) {
        Ok(state) => snapshot(state, &query.filter),
        Err(error)
            if unsafe { libc::geteuid() } != 0
                && matches!(error, ManagementError::PermissionDenied(_)) =>
        {
            Ok(ManagementSnapshot {
                needs_authorization: true,
                backend: i18n::tr!("platform-firewall-authorize-backend").into(),
                notices: vec![i18n::tr!(
                    "platform-firewall-authorize-error",
                    error = error.to_string()
                )],
                actions: vec![
                    switch_action(
                        "show_listening_ports",
                        i18n::tr!("platform-firewall-listeners"),
                    ),
                    inspect_action(&query.scope, &query.filter),
                ],
                ..Default::default()
            })
        }
        Err(error) => Err(error),
    }
}
fn snapshot(state: State, filter: &str) -> Result<ManagementSnapshot, ManagementError> {
    let mut snapshot = ManagementSnapshot {
        details_complete: true,
        needs_authorization: unsafe { libc::geteuid() } != 0
            && state.backend == Backend::Ufw
            && state.issue.is_some()
            && !state.simultaneous,
        columns: vec![
            i18n::tr!("platform-firewall-rule-backend").into(),
            i18n::tr!("platform-firewall-zone-source").into(),
            i18n::tr!("platform-firewall-effect").into(),
            i18n::tr!("platform-firewall-rule").into(),
        ],
        backend: if state.backend == Backend::Unsupported {
            i18n::tr!("platform-firewall-unsupported").into()
        } else {
            state.backend.id().into()
        },
        actions: vec![switch_action(
            "show_listening_ports",
            i18n::tr!("platform-firewall-listeners"),
        )],
        ..Default::default()
    };
    if state.backend == Backend::Unsupported {
        snapshot
            .notices
            .push(i18n::tr!("management-firewall-unavailable"));
        snapshot.details = state.detail;
        return Ok(snapshot);
    }
    let identity = BTreeMap::from([
        ("backend".into(), state.backend.id().into()),
        ("firewall_state".into(), state.fingerprint.clone()),
    ]);
    snapshot.rows.push(ManagementRow {
        id: "firewall-status".into(),
        cells: vec![
            state.backend.id().into(),
            state.default_zone.clone(),
            if state.active {
                i18n::tr!("platform-firewall-active")
            } else {
                i18n::tr!("platform-firewall-inactive")
            }
            .into(),
            state.issue.clone().unwrap_or_default(),
        ],
        detail: state.detail.clone(),
        identity: identity.clone(),
        ..Default::default()
    });
    for rule in &state.rules {
        let mut row_identity = identity.clone();
        row_identity.extend([
            ("rule_kind".into(), rule.kind.clone()),
            ("rule_value".into(), rule.value.clone()),
            ("rule_zone".into(), rule.zone.clone()),
            ("rule_scope".into(), rule.scope.clone()),
        ]);
        let effect = effect_label(state.backend, &rule.scope, state.active);
        let allow = is_allow(rule, state.backend);
        let row = ManagementRow {
            id: rule.id.clone(),
            cells: vec![
                match rule.kind.as_str() {
                    "number" => i18n::tr!("platform-firewall-kind-number"),
                    "saved" => i18n::tr!("platform-firewall-kind-saved"),
                    "rich" => i18n::tr!("platform-firewall-kind-rich"),
                    "service" => i18n::tr!("platform-firewall-kind-service"),
                    "source_port" => i18n::tr!("platform-firewall-kind-source-port"),
                    "port" => i18n::tr!("platform-firewall-kind-port"),
                    _ => rule.kind.clone(),
                },
                rule.zone.clone(),
                effect.into(),
                rule.value.clone(),
            ],
            detail: vec![
                (
                    i18n::tr!("platform-firewall-effective").into(),
                    if rule.scope == "permanent" || !state.active {
                        i18n::tr!("platform-firewall-no")
                    } else {
                        i18n::tr!("platform-firewall-yes")
                    }
                    .into(),
                ),
                (
                    i18n::tr!("platform-firewall-saved").into(),
                    if rule.scope == "runtime" {
                        i18n::tr!("platform-firewall-no")
                    } else {
                        i18n::tr!("platform-firewall-yes")
                    }
                    .into(),
                ),
                (
                    i18n::tr!("platform-firewall-after-restart").into(),
                    if rule.scope == "runtime" {
                        i18n::tr!("platform-firewall-removed")
                    } else if state.active {
                        i18n::tr!("platform-firewall-saved-rule")
                    } else {
                        i18n::tr!("platform-firewall-inactive-until-enabled")
                    }
                    .into(),
                ),
            ],
            identity: row_identity,
            actions: if allow {
                vec![ManagementAction {
                    id: "remove_rule".into(),
                    label: i18n::tr!("platform-firewall-remove-rule").into(),
                    confirm: true,
                    privileged: true,
                    disabled_reason: state.issue.clone(),
                    ..Default::default()
                }]
            } else {
                Vec::new()
            },
        };
        if filter.is_empty()
            || row
                .cells
                .iter()
                .any(|value| value.to_lowercase().contains(&filter.to_lowercase()))
        {
            snapshot.rows.push(row);
        }
    }
    let mut fields = vec![
        field(
            "protocol",
            i18n::tr!("platform-firewall-protocol"),
            "tcp",
            true,
            &["tcp", "udp"],
        ),
        field(
            "port",
            i18n::tr!("platform-firewall-port-range"),
            "",
            true,
            &[],
        ),
        field(
            "source",
            i18n::tr!("platform-firewall-source-label"),
            "",
            false,
            &[],
        ),
    ];
    if state.backend == Backend::Firewalld {
        fields.push(ManagementField {
            choices: state.zones.clone(),
            ..field(
                "zone",
                i18n::tr!("platform-firewall-zone"),
                &state.default_zone,
                true,
                &[],
            )
        });
        fields.push(field(
            "duration",
            i18n::tr!("platform-firewall-duration"),
            "both",
            true,
            &["runtime", "permanent", "both"],
        ));
    }
    snapshot.actions.push(ManagementAction {
        id: "add_rule".into(),
        label: i18n::tr!("platform-firewall-add-rule").into(),
        fields,
        confirm: true,
        privileged: true,
        disabled_reason: state.issue,
        values: identity,
        ..Default::default()
    });
    if unsafe { libc::geteuid() } != 0 {
        snapshot.actions.push(inspect_action("", filter));
    }
    Ok(snapshot)
}
fn effect_label(backend: Backend, scope: &str, active: bool) -> String {
    match (backend, scope, active) {
        (Backend::Ufw, _, true) => i18n::tr!("platform-firewall-immediate-permanent"),
        (Backend::Ufw, _, false) => i18n::tr!("platform-firewall-saved-inactive"),
        (_, "runtime", _) => i18n::tr!("platform-firewall-runtime-effect"),
        (_, "permanent", _) => i18n::tr!("platform-firewall-permanent-effect"),
        _ => i18n::tr!("platform-firewall-immediate-permanent"),
    }
}

#[derive(Debug, Clone)]
struct Input {
    protocol: String,
    port: String,
    source: String,
    zone: String,
    duration: String,
}
fn validate_zone(value: &str) -> Result<(), ManagementError> {
    if value.is_empty()
        || value.len() > 64
        || !value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
    {
        Err(invalid(i18n::tr!("platform-firewall-valid-zone")))
    } else {
        Ok(())
    }
}
fn port_range(value: &str) -> Result<String, ManagementError> {
    let parts = value.split(['-', ':']).collect::<Vec<_>>();
    if parts.is_empty() || parts.len() > 2 {
        return Err(invalid(i18n::tr!("platform-firewall-valid-port")));
    }
    let parsed = parts
        .iter()
        .map(|v| v.parse::<u16>().ok().filter(|n| *n != 0))
        .collect::<Option<Vec<_>>>()
        .ok_or_else(|| invalid(i18n::tr!("platform-firewall-valid-port")))?;
    if parsed.len() == 2 && parsed[0] > parsed[1] {
        return Err(invalid(i18n::tr!("platform-firewall-reversed-range")));
    }
    Ok(if parsed.len() == 2 && parsed[0] != parsed[1] {
        format!("{}-{}", parsed[0], parsed[1])
    } else {
        parsed[0].to_string()
    })
}
fn source_address(value: &str) -> Result<String, ManagementError> {
    if value.is_empty() || value == "any" {
        return Ok(String::new());
    }
    let (address, prefix) = value
        .split_once('/')
        .map_or((value, None), |(a, p)| (a, Some(p)));
    let address: IpAddr = address
        .parse()
        .map_err(|_| invalid(i18n::tr!("platform-firewall-valid-source")))?;
    if let Some(prefix) = prefix {
        let prefix: u8 = prefix
            .parse()
            .map_err(|_| invalid(i18n::tr!("platform-firewall-valid-prefix")))?;
        if prefix > if address.is_ipv4() { 32 } else { 128 } {
            return Err(invalid(i18n::tr!("platform-firewall-valid-prefix")));
        }
        Ok(format!("{address}/{prefix}"))
    } else {
        Ok(address.to_string())
    }
}
fn input(command: &ManagementCommand, state: &State) -> Result<Input, ManagementError> {
    let get = |name: &str, default: &str| {
        command
            .values
            .get(name)
            .cloned()
            .unwrap_or_else(|| default.into())
    };
    if command
        .values
        .get("decision")
        .is_some_and(|value| value != "allow")
    {
        return Err(invalid(i18n::tr!("platform-firewall-allow-only-add")));
    }
    let protocol = get("protocol", "tcp");
    if !["tcp", "udp"].contains(&protocol.as_str()) {
        return Err(invalid(i18n::tr!("platform-firewall-tcp-udp")));
    }
    let port = port_range(&get("port", ""))?;
    let source = source_address(&get("source", ""))?;
    let zone = get("zone", &state.default_zone);
    let duration = get("duration", "both");
    if !["runtime", "permanent", "both"].contains(&duration.as_str()) {
        return Err(invalid(i18n::tr!("platform-firewall-valid-duration")));
    }
    if state.backend == Backend::Firewalld {
        validate_zone(&zone)?;
        if !state.zones.contains(&zone) {
            return Err(conflict());
        }
    }
    if state.backend == Backend::Ufw && duration != "both" {
        return Err(invalid(i18n::tr!("platform-firewall-ufw-duration")));
    }
    Ok(Input {
        protocol,
        port,
        source,
        zone,
        duration,
    })
}
fn rich_rule(input: &Input) -> String {
    let mut rule = "rule".to_string();
    if !input.source.is_empty() {
        rule.push_str(&format!(
            " family=\"{}\" source address=\"{}\"",
            if input.source.contains(':') {
                "ipv6"
            } else {
                "ipv4"
            },
            input.source
        ));
    }
    rule.push_str(&format!(
        " port port=\"{}\" protocol=\"{}\" accept",
        input.port, input.protocol,
    ));
    rule
}
fn ufw_add(input: &Input, comment: Option<&str>) -> Vec<String> {
    let mut result = vec![
        "allow".into(),
        "in".into(),
        "proto".into(),
        input.protocol.clone(),
        "from".into(),
        if input.source.is_empty() {
            "any".into()
        } else {
            input.source.clone()
        },
        "to".into(),
        "any".into(),
        "port".into(),
        input.port.replace('-', ":"),
    ];
    if let Some(comment) = comment {
        result.extend(["comment".into(), comment.into()]);
    }
    result
}
fn ufw_delete(rule: &Rule) -> Result<Vec<String>, ManagementError> {
    if rule.kind == "saved" {
        let saved = shlex::split(&rule.value)
            .ok_or_else(|| invalid(i18n::tr!("platform-firewall-saved-rule-error")))?;
        let mut result = args(&["--force", "delete"]);
        result.extend(saved.into_iter().skip(1));
        return Ok(result);
    }
    let (value, comment) = rule
        .value
        .split_once('#')
        .map_or((rule.value.as_str(), None), |(value, comment)| {
            (value, Some(comment.trim()))
        });
    let (destination, source) = value
        .trim()
        .split_once(" ALLOW IN ")
        .ok_or_else(|| invalid(i18n::tr!("platform-firewall-safe-rule-error")))?;
    let ipv6 = value.contains(" (v6)");
    let endpoint =
        |value: &str| -> Result<(String, Option<String>, Option<String>), ManagementError> {
            let value = value.trim().strip_suffix(" (v6)").unwrap_or(value.trim());
            let words = value.split_whitespace().collect::<Vec<_>>();
            let address = |value: &str| -> Result<String, ManagementError> {
                if value == "Anywhere" {
                    return Ok(if ipv6 { "::/0" } else { "0.0.0.0/0" }.into());
                }
                let value = source_address(value)?;
                let ip = value
                    .split('/')
                    .next()
                    .unwrap()
                    .parse::<IpAddr>()
                    .map_err(|_| invalid(i18n::tr!("platform-firewall-safe-address-error")))?;
                if ip.is_ipv6() != ipv6 {
                    return Err(invalid(i18n::tr!("platform-firewall-family-changed")));
                }
                Ok(value)
            };
            let port = |value: &str| -> Result<(String, Option<String>), ManagementError> {
                let (number, protocol) = value
                    .split_once('/')
                    .map_or((value, None), |(number, protocol)| (number, Some(protocol)));
                if protocol.is_some_and(|protocol| !["tcp", "udp"].contains(&protocol)) {
                    return Err(invalid(i18n::tr!("platform-firewall-safe-protocol-error")));
                }
                Ok((
                    port_range(&number.replace(':', "-"))?.replace('-', ":"),
                    protocol.map(str::to_string),
                ))
            };
            match words.as_slice() {
                [one] => {
                    if let Ok(address) = address(one) {
                        return Ok((address, None, None));
                    }
                    let (number, protocol) = port(one)?;
                    Ok((address("Anywhere")?, Some(number), protocol))
                }
                [host, number] => {
                    let (number, protocol) = port(number)?;
                    Ok((address(host)?, Some(number), protocol))
                }
                _ => Err(invalid(i18n::tr!("platform-firewall-safe-rule-error"))),
            }
        };
    let (destination, destination_port, destination_protocol) = endpoint(destination)?;
    let (source, source_port, source_protocol) = endpoint(source)?;
    if destination_protocol.is_some()
        && source_protocol.is_some()
        && destination_protocol != source_protocol
    {
        return Err(invalid(i18n::tr!("platform-firewall-protocol-mismatch")));
    }
    let mut result = args(&["--force", "delete", "allow", "in"]);
    if let Some(protocol) = destination_protocol.or(source_protocol) {
        result.extend(["proto".into(), protocol]);
    }
    result.extend(["from".into(), source]);
    if let Some(port) = source_port {
        result.extend(["port".into(), port]);
    }
    result.extend(["to".into(), destination]);
    if let Some(port) = destination_port {
        result.extend(["port".into(), port]);
    }
    if let Some(comment) = comment {
        result.extend(["comment".into(), comment.into()]);
    }
    Ok(result)
}
fn ufw_paths() -> Vec<PathBuf> {
    [
        "/etc/ufw/user.rules",
        "/etc/ufw/user6.rules",
        "/etc/ufw/ufw.conf",
        "/etc/default/ufw",
    ]
    .into_iter()
    .map(PathBuf::from)
    .collect()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Change {
    zone: String,
    scope: String,
    kind: String,
    value: String,
    before: bool,
    after: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
struct BackupPayload {
    version: u8,
    backend: Backend,
    active: bool,
    ufw_before: String,
    ufw_after: String,
    changes: Vec<Change>,
    #[serde(default)]
    missing_zones: Vec<String>,
}
fn firewall_paths(backend: Backend, changes: &[Change]) -> Vec<PathBuf> {
    if backend == Backend::Ufw {
        return ufw_paths();
    }
    changes
        .iter()
        .filter(|change| change.scope == "permanent")
        .flat_map(|change| {
            [
                PathBuf::from(format!("/etc/firewalld/zones/{}.xml", change.zone)),
                PathBuf::from(format!("/etc/firewalld/zones/{}.xml.old", change.zone)),
            ]
        })
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}
fn missing_zones(changes: &[Change]) -> Result<Vec<String>, ManagementError> {
    let mut missing = BTreeSet::new();
    for change in changes.iter().filter(|change| change.scope == "permanent") {
        if !Path::new(&format!("/etc/firewalld/zones/{}.xml", change.zone))
            .try_exists()
            .map_err(super::network::io_error)?
        {
            missing.insert(change.zone.clone());
        }
    }
    Ok(missing.into_iter().collect())
}
fn start_payload(
    backup: &super::config_backups::BackupSession,
    payload: &BackupPayload,
) -> Result<(), ManagementError> {
    let result = serde_json::to_value(payload)
        .map_err(|error| ManagementError::Failed(error.to_string()))
        .and_then(|value| backup.set_payload(value));
    if let Err(error) = &result {
        backup.finish(&i18n::tr!(
            "platform-firewall-backup-preparation",
            error = error.to_string()
        ))?;
    }
    result
}
fn native_change(change: &Change, present: bool) -> Result<Vec<String>, ManagementError> {
    validate_zone(&change.zone)?;
    if !["runtime", "permanent"].contains(&change.scope.as_str()) {
        return Err(invalid(i18n::tr!("platform-firewall-saved-duration-error")));
    }
    let option = match change.kind.as_str() {
        "rich"
            if change.value.starts_with("rule ") && !change.value.contains(['\n', '\r', '\0']) =>
        {
            "rich-rule"
        }
        "port" | "source_port" => {
            let (port, protocol) = change
                .value
                .split_once('/')
                .ok_or_else(|| invalid(i18n::tr!("platform-firewall-saved-port-error")))?;
            port_range(port)?;
            if !["tcp", "udp", "sctp", "dccp"].contains(&protocol) {
                return Err(invalid(i18n::tr!("platform-firewall-saved-protocol-error")));
            }
            if change.kind == "port" {
                "port"
            } else {
                "source-port"
            }
        }
        "service"
            if !change.value.is_empty()
                && change
                    .value
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-') =>
        {
            "service"
        }
        _ => return Err(invalid(i18n::tr!("platform-firewall-saved-change-error"))),
    };
    let mut result = vec![format!("--zone={}", change.zone)];
    if change.scope == "permanent" {
        result.push("--permanent".into());
    }
    result.push(format!(
        "--{}-{option}={}",
        if present { "add" } else { "remove" },
        change.value
    ));
    Ok(result)
}
fn rule_present(state: &State, change: &Change) -> bool {
    state.rules.iter().any(|rule| {
        rule.zone == change.zone
            && rule.scope == change.scope
            && rule.kind == change.kind
            && rule.value == change.value
    })
}
fn validate_request(command: &ManagementCommand, state: &State) -> Result<(), ManagementError> {
    if state.simultaneous {
        return Err(ManagementError::Conflict(
            i18n::tr!("platform-firewall-both-running").into(),
        ));
    }
    if let Some(issue) = &state.issue {
        return Err(ManagementError::Unavailable(issue.clone()));
    }
    if state.backend == Backend::Unsupported {
        return Err(ManagementError::Unavailable(
            i18n::tr!("platform-firewall-unsupported").into(),
        ));
    }
    let expected = |name| {
        command
            .identity
            .get(name)
            .or_else(|| command.values.get(name))
    };
    if expected("backend").map(String::as_str) != Some(state.backend.id())
        || expected("firewall_state") != Some(&state.fingerprint)
    {
        return Err(conflict());
    }
    Ok(())
}

pub fn execute(
    command: &ManagementCommand,
    _context: &ExecutionContext,
    interaction: &mut dyn OperationInteraction,
    flag: &AtomicBool,
) -> Result<String, ManagementError> {
    if command.action == "inspect_firewall" {
        let query = ManagementQuery::from_inspection(command);
        interaction.emit(OperationEvent::Snapshot {
            snapshot: self::query(&query, flag)?.with_authorized_refresh(command),
        });
        return Ok(i18n::tr!("platform-firewall-inspected").into());
    }
    if !["add_rule", "remove_rule"].contains(&command.action.as_str()) {
        return Err(invalid(i18n::tr!("platform-firewall-unknown-operation")));
    }
    let _lock = lock_rules(flag)?;
    execute_native(command, interaction, flag, &mut System)
}
fn execute_native(
    command: &ManagementCommand,
    interaction: &mut dyn OperationInteraction,
    flag: &AtomicBool,
    native: &mut impl Native,
) -> Result<String, ManagementError> {
    let before = load(native, flag)?;
    validate_request(command, &before)?;
    let mut payload = BackupPayload {
        version: 1,
        backend: before.backend,
        active: before.active,
        ufw_before: before.raw.clone(),
        ufw_after: before.raw.clone(),
        changes: Vec::new(),
        missing_zones: Vec::new(),
    };
    let mut ufw_command = Vec::new();
    if command.action == "add_rule" {
        let input = input(command, &before)?;
        if before.backend == Backend::Ufw {
            ufw_command = ufw_add(&input, None);
        } else {
            for scope in ["runtime", "permanent"] {
                if input.duration != "both" && input.duration != scope {
                    continue;
                }
                let change = Change {
                    zone: input.zone.clone(),
                    scope: scope.into(),
                    kind: "rich".into(),
                    value: rich_rule(&input),
                    before: false,
                    after: true,
                };
                if rule_present(&before, &change) {
                    continue;
                }
                payload.changes.push(change);
            }
            if payload.changes.is_empty() {
                return Ok(i18n::tr!("platform-firewall-already-exists").into());
            }
        }
    } else {
        let target = command
            .target
            .as_ref()
            .ok_or_else(|| invalid(i18n::tr!("platform-firewall-select-rule")))?;
        let rule = before
            .rules
            .iter()
            .find(|rule| &rule.id == target)
            .ok_or_else(conflict)?;
        if !is_allow(rule, before.backend) {
            return Err(invalid(i18n::tr!("platform-firewall-allow-only-remove")));
        }
        for (key, value) in [
            ("rule_kind", &rule.kind),
            ("rule_value", &rule.value),
            ("rule_zone", &rule.zone),
            ("rule_scope", &rule.scope),
        ] {
            if command.identity.get(key) != Some(value) {
                return Err(conflict());
            }
        }
        if before.backend == Backend::Ufw {
            ufw_command = ufw_delete(rule)?;
        } else {
            payload.changes.push(Change {
                zone: rule.zone.clone(),
                scope: rule.scope.clone(),
                kind: rule.kind.clone(),
                value: rule.value.clone(),
                before: true,
                after: false,
            });
        }
    }
    let paths = firewall_paths(before.backend, &payload.changes);
    payload.missing_zones = missing_zones(&payload.changes)?;
    let backup = super::config_backups::begin("firewall", &paths)?;
    start_payload(&backup, &payload)?;
    interaction.emit(OperationEvent::Output {
        text: i18n::tr!("platform-firewall-backup-id", id = backup.id()),
    });
    // Recheck after the backup, so an old UFW number cannot silently select a new rule.
    let mut result = (|| {
        let current = load(native, flag)?;
        validate_request(command, &current)?;
        if before.backend == Backend::Ufw {
            run(native, "ufw", &ufw_command, flag)?;
        } else {
            for change in &payload.changes {
                run(
                    native,
                    "firewall-cmd",
                    &native_change(change, change.after)?,
                    flag,
                )?;
            }
        }
        Ok(i18n::tr!("platform-firewall-updated").to_string())
    })();
    let after = load(native, &AtomicBool::new(false));
    match after {
        Ok(after) => {
            if result.is_ok()
                && (after.backend != before.backend
                    || after.simultaneous
                    || after.issue.is_some()
                    || payload
                        .changes
                        .iter()
                        .any(|change| rule_present(&after, change) != change.after))
            {
                result = Err(ManagementError::Failed(
                    i18n::tr!("platform-firewall-result-mismatch").into(),
                ));
            }
            if result.is_ok()
                && before.backend == Backend::Ufw
                && command.action == "remove_rule"
                && after.rules.len() >= before.rules.len()
            {
                result = Err(ManagementError::Failed(
                    i18n::tr!("platform-firewall-remove-mismatch").into(),
                ));
            }
            payload.ufw_after = after.raw.clone();
            for change in &mut payload.changes {
                change.after = rule_present(&after, change);
            }
            let saved = backup.set_payload(
                serde_json::to_value(&payload)
                    .map_err(|error| ManagementError::Failed(error.to_string()))?,
            );
            let finished = backup.finish(&result.as_ref().map_or_else(
                |error| i18n::tr!("platform-firewall-failed", error = error.to_string()),
                Clone::clone,
            ));
            saved?;
            finished?;
        }
        Err(error) => {
            backup.finish(&i18n::tr!(
                "platform-firewall-unknown-result",
                error = error.to_string()
            ))?;
            return Err(ManagementError::Failed(i18n::tr!(
                "platform-firewall-unverified-result",
                error = error.to_string()
            )));
        }
    }
    result
}

fn payload(value: &Value) -> Result<BackupPayload, ManagementError> {
    let result: BackupPayload = serde_json::from_value(value.clone())
        .map_err(|_| invalid(i18n::tr!("platform-firewall-invalid-backup")))?;
    if result.version != 1 || result.backend == Backend::Unsupported {
        return Err(invalid(i18n::tr!("platform-firewall-unsupported-backup")));
    }
    for change in &result.changes {
        native_change(change, true)?;
    }
    for zone in &result.missing_zones {
        validate_zone(zone)?;
        if !result
            .changes
            .iter()
            .any(|change| change.zone == *zone && change.scope == "permanent")
        {
            return Err(invalid(i18n::tr!("platform-firewall-missing-zone-error")));
        }
    }
    Ok(result)
}
pub(crate) fn restore_preflight(value: &Value, flag: &AtomicBool) -> Result<(), ManagementError> {
    restore_check(&payload(value)?, &load(&mut System, flag)?)
}
fn restore_check(saved: &BackupPayload, current: &State) -> Result<(), ManagementError> {
    if current.backend != saved.backend
        || current.simultaneous
        || current.active != saved.active
        || current.issue.is_some()
    {
        return Err(conflict());
    }
    if current.backend == Backend::Ufw && current.raw != saved.ufw_after {
        return Err(conflict());
    }
    for change in &saved.changes {
        if rule_present(current, change) != change.after {
            return Err(conflict());
        }
    }
    Ok(())
}
pub(crate) fn restore_backup(
    backup_id: &str,
    value: &Value,
    _context: &ExecutionContext,
    interaction: &mut dyn OperationInteraction,
    flag: &AtomicBool,
) -> Result<String, ManagementError> {
    let saved = payload(value)?;
    let mut native = System;
    let before = load(&mut native, flag)?;
    restore_check(&saved, &before)?;
    super::config_backups::validate_current(backup_id)?;
    let paths = firewall_paths(saved.backend, &saved.changes);
    let backup = super::config_backups::begin("firewall", &paths)?;
    let mut inverse = saved.clone();
    inverse.ufw_before = before.raw.clone();
    inverse.ufw_after = saved.ufw_before.clone();
    inverse.missing_zones = missing_zones(&saved.changes)?;
    for change in &mut inverse.changes {
        std::mem::swap(&mut change.before, &mut change.after);
    }
    start_payload(&backup, &inverse)?;
    interaction.emit(OperationEvent::Output {
        text: i18n::tr!("platform-firewall-backup-id", id = backup.id()),
    });
    let result = (|| {
        if saved.backend == Backend::Ufw {
            super::config_backups::restore_files_unrecorded(backup_id)?;
            if saved.active {
                run(&mut native, "ufw", &args(&["reload"]), flag)?;
            }
        } else {
            super::config_backups::validate_current(backup_id)?;
            restore_check(&saved, &load(&mut native, flag)?)?;
            for change in &saved.changes {
                if change.scope == "permanent" && saved.missing_zones.contains(&change.zone) {
                    continue;
                }
                if change.before != change.after
                    && rule_present(&load(&mut native, flag)?, change) != change.before
                {
                    run(
                        &mut native,
                        "firewall-cmd",
                        &native_change(change, change.before)?,
                        flag,
                    )?;
                }
            }
            for zone in &saved.missing_zones {
                if Path::new(&format!("/etc/firewalld/zones/{zone}.xml"))
                    .try_exists()
                    .map_err(super::network::io_error)?
                {
                    run(
                        &mut native,
                        "firewall-cmd",
                        &vec!["--permanent".into(), format!("--load-zone-defaults={zone}")],
                        flag,
                    )?;
                }
            }
            let current = load(&mut native, flag)?;
            for change in &saved.changes {
                if rule_present(&current, change) != change.before {
                    return Err(ManagementError::Failed(
                        i18n::tr!("platform-firewall-restore-firewalld-mismatch").into(),
                    ));
                }
            }
            let candidates = paths
                .iter()
                .map(|path| {
                    let current = super::system_config::read(path)?;
                    Ok((path.clone(), (current.content, current.existed)))
                })
                .collect::<Result<BTreeMap<_, _>, ManagementError>>()?;
            super::config_backups::restore_pending_files_unrecorded(backup_id, &candidates)?;
        }
        let after = load(&mut native, flag)?;
        if saved.backend == Backend::Ufw && after.raw != saved.ufw_before {
            return Err(ManagementError::Failed(
                i18n::tr!("platform-firewall-restore-ufw-mismatch").into(),
            ));
        }
        for change in &saved.changes {
            if rule_present(&after, change) != change.before {
                return Err(ManagementError::Failed(
                    i18n::tr!("platform-firewall-restore-firewalld-mismatch").into(),
                ));
            }
        }
        Ok(i18n::tr!("platform-firewall-restored").into())
    })();
    let after = load(&mut native, &AtomicBool::new(false));
    if let Ok(after) = &after {
        inverse.ufw_after = after.raw.clone();
        for change in &mut inverse.changes {
            change.after = rule_present(after, change);
        }
    }
    let saved = backup.set_payload(
        serde_json::to_value(&inverse)
            .map_err(|error| ManagementError::Failed(error.to_string()))?,
    );
    let finished = backup.finish(&result.as_ref().map_or_else(
        |error| {
            i18n::tr!(
                "platform-firewall-restore-failed",
                error = error.to_string()
            )
        },
        Clone::clone,
    ));
    saved?;
    finished?;
    after?;
    result
}
