//! Public Linux operations. The parser selects fixed backend actions, never command strings.
use crate::CliError;
use platform::management::ManagementKind;
use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ManagementCli {
    Help(String),
    Run(ManagementRequest),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManagementRequest {
    pub group: String,
    pub verb: String,
    pub target: Option<String>,
    pub values: BTreeMap<String, String>,
    pub json: bool,
    pub yes: bool,
    pub non_interactive: bool,
    pub wait_seconds: u64,
    pub password_fd: Option<i32>,
    pub authorization_fd: Option<i32>,
    pub input: Option<std::path::PathBuf>,
    pub apply: bool,
}

pub(crate) fn kind(group: &str) -> Option<ManagementKind> {
    Some(match group {
        "services" => ManagementKind::Services,
        "processes" => ManagementKind::Processes,
        "packages" => ManagementKind::Packages,
        "network" => ManagementKind::Network,
        "disks" => ManagementKind::Disks,
        "users" => ManagementKind::Users,
        "system-config" => ManagementKind::SystemConfig,
        "firewall" => ManagementKind::Firewall,
        "ssh" => ManagementKind::Ssh,
        "backups" => ManagementKind::Backups,
        "scheduled-tasks" => ManagementKind::ScheduledTasks,
        _ => return None,
    })
}

pub(crate) fn verbs(group: &str) -> &'static [&'static str] {
    match group {
        "services" => &[
            "list",
            "show",
            "dependencies",
            "start",
            "stop",
            "restart",
            "reload",
            "daemon-reload",
            "enable",
            "disable",
            "config",
            "create",
            "create-instance",
        ],
        "processes" => &[
            "list", "show", "files", "ports", "io", "service", "term", "kill", "stop", "cont",
            "nice",
        ],
        "packages" => &[
            "list",
            "show",
            "search",
            "updates",
            "install",
            "remove",
            "upgrade",
            "upgrade-all",
            "refresh",
            "status",
            "conflicts",
            "check",
            "repair-configure",
            "repair-dependencies",
            "sources",
            "source-add",
            "source-enable",
            "source-disable",
            "source-remove",
        ],
        "network" => &[
            "list",
            "show",
            "wifi-list",
            "wifi-scan",
            "wifi-connect",
            "wifi-disconnect",
            "wifi-forget",
            "forget-saved-wifi",
            "configure",
            "disconnect",
            "check",
            "confirm",
            "transaction-status",
        ],
        "disks" => &[
            "list",
            "show",
            "health",
            "inodes",
            "mount",
            "unmount",
            "scan",
            "automatic-mount",
        ],
        "users" => &[
            "list",
            "show",
            "groups",
            "shells",
            "create",
            "lookup",
            "info",
            "password",
            "lock",
            "unlock",
            "delete",
            "set-groups",
            "primary-group",
            "shell",
            "expiry",
            "ssh-keys",
            "ssh-add",
            "ssh-remove",
            "group-create",
            "group-members",
            "group-rename",
            "group-delete",
        ],
        "system-config" => &[
            "read",
            "diff",
            "check",
            "apply",
            "permissions",
            "history",
            "preview-restore",
            "restore",
            "reload",
        ],
        "operations" => &["list", "status", "attach"],
        "firewall" => &["list", "show", "listeners", "allow", "remove"],
        "ssh" => &[
            "show",
            "keys",
            "configure",
            "add-key",
            "remove-key",
            "confirm",
            "cancel",
            "status",
        ],
        "backups" => &["list", "show", "preview-restore", "restore"],
        "scheduled-tasks" => &[
            "list", "inspect", "preview", "create", "pause", "resume", "run-now",
        ],
        _ => &[],
    }
}

fn invalid(message: impl Into<String>) -> CliError {
    CliError::InvalidManagementArgument(message.into())
}

pub(crate) fn parse_management(group: &str, args: &[String]) -> Result<ManagementCli, CliError> {
    if args.is_empty()
        || args
            .first()
            .is_some_and(|arg| matches!(arg.as_str(), "help" | "--help" | "-h"))
    {
        return Ok(ManagementCli::Help(group.into()));
    }
    let mut verb = args[0].clone();
    let mut offset = 1;
    // Also accept the natural nested spelling: packages sources add/enable/disable/remove.
    if group == "packages"
        && verb == "sources"
        && args
            .get(1)
            .is_some_and(|arg| matches!(arg.as_str(), "add" | "enable" | "disable" | "remove"))
    {
        verb = format!("source-{}", args[1]);
        offset = 2;
    }
    if !verbs(group).contains(&verb.as_str()) {
        return Err(invalid(i18n::tr!(
            "cli-administration-unknown-operation",
            group = group.to_string(),
            verb = verb.to_string()
        )));
    }
    let mut result = ManagementRequest {
        group: group.into(),
        verb,
        target: None,
        values: BTreeMap::new(),
        json: false,
        yes: false,
        non_interactive: false,
        wait_seconds: 900,
        password_fd: None,
        authorization_fd: None,
        input: None,
        apply: false,
    };
    if group == "operations" && result.verb == "status" {
        result.wait_seconds = 1;
    }
    let allowed = allowed_values(group, &result.verb);
    let extended = matches!(group, "firewall" | "ssh" | "backups" | "scheduled-tasks");
    let mut seen_options = BTreeSet::new();
    let mut index = offset;
    while index < args.len() {
        let arg = &args[index];
        if extended && arg.starts_with("--") && !seen_options.insert(arg.clone()) {
            return Err(invalid(i18n::tr!(
                "cli-administration-duplicate-option",
                arg = arg.to_string()
            )));
        }
        match arg.as_str() {
            "--json" => result.json = true,
            "--yes" => result.yes = true,
            "--non-interactive" => result.non_interactive = true,
            "--authorize"
                if matches!(group, "firewall" | "ssh" | "backups" | "scheduled-tasks")
                    && allowed.contains(&"authorize") =>
            {
                result.values.insert("authorize".into(), "true".into());
            }
            "--apply" if is_draft(group, &result.verb) => result.apply = true,
            "--reload-service" if group == "system-config" && result.verb == "reload" => {
                result.values.insert("reload_service".into(), "true".into());
            }
            "--allow-unvalidated"
                if group == "system-config"
                    || is_draft(group, &result.verb)
                    || (group == "backups" && result.verb == "restore") =>
            {
                result
                    .values
                    .insert("allow_unvalidated".into(), "true".into());
            }
            "--wait" | "--wait-seconds" => {
                index += 1;
                let value = args
                    .get(index)
                    .ok_or_else(|| invalid(i18n::tr!("cli-administration-wait-needed")))?;
                result.wait_seconds = value
                    .parse::<u64>()
                    .ok()
                    .filter(|seconds| *seconds <= 86400)
                    .ok_or_else(|| invalid(i18n::tr!("cli-administration-wait-range")))?;
            }
            "--password-fd" | "--secret-fd" | "--authorization-fd" => {
                index += 1;
                let value = args
                    .get(index)
                    .ok_or_else(|| invalid(i18n::tr!("cli-administration-secret-fd-needed")))?;
                let fd = value
                    .parse::<i32>()
                    .ok()
                    .filter(|fd| *fd >= 0)
                    .ok_or_else(|| invalid(i18n::tr!("cli-administration-secret-fd-invalid")))?;
                if arg == "--authorization-fd" {
                    result.authorization_fd = Some(fd);
                } else {
                    result.password_fd = Some(fd);
                }
            }
            "--input" if group == "system-config" => {
                index += 1;
                result.input = Some(
                    args.get(index)
                        .ok_or_else(|| invalid("--input needs a text file"))?
                        .into(),
                );
            }
            _ if arg.starts_with("--") => {
                let key = arg.trim_start_matches("--").replace('-', "_");
                if matches!(
                    key.as_str(),
                    "password" | "sudo_password" | "secret" | "content"
                ) {
                    return Err(invalid(i18n::tr!("cli-administration-no-secret-values")));
                }
                if !allowed.contains(&key.as_str()) {
                    return Err(invalid(i18n::tr!(
                        "cli-administration-unsupported-group",
                        group = group,
                        operation = result.verb.clone(),
                        option = arg.to_string()
                    )));
                }
                index += 1;
                let value = args.get(index).ok_or_else(|| {
                    invalid(i18n::tr!(
                        "cli-administration-value-needed",
                        arg = arg.to_string()
                    ))
                })?;
                if extended && value.starts_with("--") {
                    return Err(invalid(i18n::tr!(
                        "cli-administration-value-needed",
                        arg = arg.to_string()
                    )));
                }
                if result.values.insert(key, value.clone()).is_some() {
                    return Err(invalid(i18n::tr!(
                        "cli-administration-duplicate-option",
                        arg = arg.to_string()
                    )));
                }
            }
            _ if arg.starts_with('-') => {
                return Err(invalid(i18n::tr!(
                    "cli-administration-unsupported-option",
                    arg = arg.to_string()
                )));
            }
            _ if result.target.is_none() => result.target = Some(arg.clone()),
            _ if group == "services"
                && result.verb == "create-instance"
                && !result.values.contains_key("instance") =>
            {
                result.values.insert("instance".into(), arg.clone());
            }
            _ => {
                return Err(invalid(i18n::tr!(
                    "cli-administration-unexpected-argument",
                    arg = arg.to_string()
                )));
            }
        }
        index += 1;
    }
    let requires_target = match group {
        "services" => !matches!(result.verb.as_str(), "list" | "daemon-reload"),
        "processes" => result.verb != "list",
        "packages" => matches!(
            result.verb.as_str(),
            "show"
                | "search"
                | "install"
                | "remove"
                | "upgrade"
                | "source-enable"
                | "source-disable"
                | "source-remove"
        ),
        "network" => result.verb != "list" && result.verb != "wifi-list",
        "disks" => result.verb != "list",
        "users" => !matches!(result.verb.as_str(), "list" | "groups" | "shells"),
        "system-config" => true,
        "operations" => result.verb != "list",
        "firewall" => matches!(result.verb.as_str(), "show" | "remove"),
        "ssh" => matches!(
            result.verb.as_str(),
            "confirm" | "cancel" | "status" | "add-key" | "remove-key"
        ),
        "backups" => result.verb != "list",
        "scheduled-tasks" => !matches!(result.verb.as_str(), "list" | "preview" | "create"),
        _ => false,
    };
    if group == "network"
        && result.verb == "confirm"
        && result
            .values
            .get("decision")
            .is_some_and(|decision| !matches!(decision.as_str(), "keep" | "revert"))
    {
        return Err(invalid("--decision must be keep or revert"));
    }
    if requires_target && result.target.is_none() {
        return Err(invalid(i18n::tr!(
            "cli-administration-target-needed",
            group = group,
            operation = result.verb.clone()
        )));
    }
    if !requires_target
        && result.target.is_some()
        && !(group == "ssh" && matches!(result.verb.as_str(), "show" | "keys" | "configure"))
    {
        return Err(invalid(i18n::tr!("cli-administration-no-target")));
    }
    if group == "system-config"
        && matches!(result.verb.as_str(), "diff" | "apply")
        && result.input.is_none()
    {
        return Err(invalid("Use --input to supply the candidate text"));
    }
    if group == "system-config"
        && matches!(
            result.verb.as_str(),
            "apply" | "permissions" | "restore" | "preview-restore"
        )
        && !result.values.contains_key("expected_version")
    {
        return Err(invalid("Read the file first, then pass --expected-version"));
    }
    if result.apply && !result.values.contains_key("expected_version") {
        return Err(invalid(
            "Saving a draft needs --expected-version from system-config read",
        ));
    }
    for (key, choices) in match group {
        "firewall" => vec![
            ("protocol", vec!["tcp", "udp"]),
            ("duration", vec!["runtime", "permanent", "both"]),
        ],
        "ssh" => vec![
            ("password_authentication", vec!["yes", "no"]),
            ("pubkey_authentication", vec!["yes", "no"]),
            (
                "permit_root_login",
                vec!["yes", "prohibit-password", "forced-commands-only", "no"],
            ),
            ("open_firewall", vec!["true", "false"]),
        ],
        _ => Vec::new(),
    } {
        if result
            .values
            .get(key)
            .is_some_and(|value| !choices.contains(&value.as_str()))
        {
            return Err(invalid(i18n::tr!(
                "cli-administration-choice-needed",
                option = key.replace('_', "-"),
                choices = choices.join("|")
            )));
        }
    }
    let required_values = match (group, result.verb.as_str()) {
        ("firewall", "allow") => &["port"][..],
        ("ssh", "add-key") => &["public_key"][..],
        ("ssh", "remove-key") => &["fingerprint"][..],
        ("scheduled-tasks", "preview" | "create") => &["name", "command", "schedule"][..],
        _ => &[],
    };
    for key in required_values {
        if result
            .values
            .get(*key)
            .is_none_or(|value| value.trim().is_empty())
        {
            return Err(invalid(i18n::tr!(
                "cli-administration-required-option",
                option = key.replace('_', "-"),
                field = match *key {
                    "port" => i18n::tr!("cli-administration-field-port"),
                    "public_key" => i18n::tr!("cli-administration-field-public-key"),
                    "fingerprint" => i18n::tr!("cli-administration-field-fingerprint"),
                    "name" => i18n::tr!("cli-administration-field-name"),
                    "command" => i18n::tr!("cli-administration-field-command"),
                    "schedule" => i18n::tr!("cli-administration-field-schedule"),
                    _ => key.to_string(),
                }
            )));
        }
    }
    if matches!(group, "firewall" | "ssh") {
        if let Some(port) = result.values.get("port") {
            let pieces = port.split(['-', ':']).collect::<Vec<_>>();
            let ports = pieces
                .iter()
                .map(|value| value.parse::<u16>().ok().filter(|port| *port > 0))
                .collect::<Option<Vec<_>>>();
            if ports.as_ref().is_none_or(|ports| {
                ports.is_empty()
                    || ports.len() > if group == "ssh" { 1 } else { 2 }
                    || ports.len() == 2 && ports[0] > ports[1]
            }) {
                return Err(invalid(i18n::tr!("cli-administration-invalid-port")));
            }
        }
    }
    Ok(ManagementCli::Run(result))
}

pub(crate) fn allowed_values(group: &str, verb: &str) -> Vec<&'static str> {
    match group {
        "firewall" => {
            return match verb {
                "list" | "listeners" => vec!["filter", "authorize"],
                "show" => vec!["authorize"],
                "allow" => vec!["protocol", "port", "source", "zone", "duration"],
                _ => Vec::new(),
            };
        }
        "ssh" => {
            return match verb {
                "show" | "keys" => vec!["authorize"],
                "configure" => vec![
                    "port",
                    "password_authentication",
                    "pubkey_authentication",
                    "permit_root_login",
                    "open_firewall",
                ],
                "add-key" => vec!["public_key"],
                "remove-key" => vec!["fingerprint"],
                _ => Vec::new(),
            };
        }
        "backups" => {
            return match verb {
                "list" => vec!["filter", "authorize"],
                "show" | "preview-restore" => vec!["authorize"],
                _ => Vec::new(),
            };
        }
        "scheduled-tasks" => {
            return match verb {
                "list" => vec!["filter", "authorize"],
                "inspect" => vec!["authorize"],
                "preview" | "create" => {
                    vec!["name", "command", "working_directory", "schedule", "user"]
                }
                _ => Vec::new(),
            };
        }
        _ => {}
    }
    let mut common = vec!["filter"];
    match group {
        "services" => {
            common.push("scope");
            if verb == "create" {
                common.extend(["program", "arguments", "user", "working_directory"]);
            }
            if verb == "create-instance" {
                common.push("instance");
            }
        }
        "processes" => common.extend(["sort", "descending", "tree", "nice"]),
        "packages" => {
            common.extend(["scope", "config_policy"]);
            if verb == "source-add" {
                common.extend(["name", "url", "suite", "components", "signed_by", "gpgkey"]);
            }
        }
        "network" => common.extend([
            "interface",
            "ssid",
            "ssid_hex",
            "security",
            "access_point",
            "ipv4",
            "address4",
            "gateway4",
            "ipv6",
            "address6",
            "gateway6",
            "dns",
            "host",
            "port",
            "decision",
        ]),
        "disks" => common.extend([
            "mode",
            "directory",
            "sort",
            "mountpoint",
            "options",
            "enabled",
        ]),
        "users" => common.extend([
            "scope",
            "display_name",
            "shell",
            "expires",
            "groups",
            "primary_group",
            "members",
            "name",
            "public_key",
            "fingerprint",
        ]),
        "system-config" => common.extend([
            "expected_version",
            "validator",
            "uid",
            "gid",
            "mode",
            "backup_id",
            "service",
            "scope",
        ]),
        _ => {}
    }
    if is_draft(group, verb) {
        common.extend(["expected_version", "validator"]);
    }
    common
}

pub(crate) fn is_draft(group: &str, verb: &str) -> bool {
    matches!(
        (group, verb),
        ("services", "create" | "create-instance" | "config")
            | (
                "packages",
                "source-add" | "source-enable" | "source-disable" | "source-remove"
            )
            | ("disks", "automatic-mount")
    )
}

pub(crate) fn help(output: &mut impl Write, group: &str) -> std::io::Result<()> {
    writeln!(
        output,
        "{}",
        i18n::tr!("cli-administration-usage", group = group.to_string())
    )?;
    writeln!(
        output,
        "{}",
        i18n::tr!(
            "cli-administration-operations",
            operations = verbs(group).join(", ")
        )
    )?;
    writeln!(output, "{}", i18n::tr!("cli-administration-options"))?;
    writeln!(output, "{}", i18n::tr!("cli-administration-secret-help"))?;
    match group {
        "services" => writeln!(
            output,
            "--scope system|user; create NAME --program /absolute/program [--arguments 'literal arguments'] [--user USER] [--working-directory PATH]; create-instance TEMPLATE INSTANCE. config/create produce a reviewed draft; --apply --expected-version VERSION saves it. No implicit start or startup enable."
        )?,
        "processes" => writeln!(
            output,
            "show/files/ports/io/service PID; nice PID --nice -20..19; list [--sort pid|cpu|memory|uid|name|nice] [--descending true|false] [--tree true|false]. Changes recheck PID and start time."
        )?,
        "packages" => writeln!(
            output,
            "list [--scope installed|search|updates]; search TEXT; source-add --name NAME --url URL [APT: --suite SUITE --components main --signed-by /key; DNF: --gpgkey URL]. sources add/enable/disable/remove are aliases. Source changes output drafts; --apply --expected-version saves. Script changes retain current config; --config-policy replace is APT-only. Arch install/upgrade always upgrades the full system."
        )?,
        "network" => writeln!(
            output,
            "wifi-list [--interface IFACE]; wifi-connect ACCESS_POINT_OR_IFACE [--ssid NAME --security open|wpa2|wpa3] [--password-fd FD]; configure IFACE --ipv4 dhcp|static|disabled --address4 CIDR --gateway4 IP --ipv6 auto|dhcp|static|disabled --address6 CIDR --gateway6 IP --dns IPs. check IFACE [--host HOST --port PORT]; confirm TRANSACTION [--decision keep|revert]. Configuration remains pending for 120 seconds until confirm; Wi-Fi confirms the selected network automatically."
        )?,
        "disks" => writeln!(
            output,
            "show/health/inodes /dev/DEVICE; mount /dev/DEVICE --mode read_only|read_write; scan /absolute/directory [--sort allocated|logical|files]; automatic-mount /dev/DEVICE --mountpoint /mnt/PATH [--options defaults --enabled true] outputs an fstab draft; --apply --expected-version saves without mounting."
        )?,
        "users" => writeln!(
            output,
            "create NAME --shell /bin/bash [--display-name NAME --password-fd FD]; lookup NAME includes directory accounts. shell NAME --shell PATH; expiry NAME --expires YYYY-MM-DD|never; set-groups NAME --groups GROUPS edits supplementary groups; primary-group NAME --primary-group GROUP edits the primary group; ssh-add NAME --public-key 'ssh-ed25519 ...'; ssh-remove NAME --fingerprint SHA256:...; group-create NAME; group-members GROUP --members NAMES; group-rename GROUP --name NEW. User deletion retains the home directory."
        )?,
        "system-config" => writeln!(
            output,
            "read FILE returns text/version/attributes; diff FILE --input CANDIDATE; check FILE [--input CANDIDATE] checks current or candidate content; apply FILE --input CANDIDATE --expected-version VERSION; permissions FILE --uid UID --gid GID --mode OCTAL --expected-version VERSION; history FILE; preview-restore/restore FILE --backup-id ID --expected-version VERSION; reload FILE --service UNIT [--scope system|user] [--reload-service]. SSH reload requires --backup-id ID returned by apply, so failed reload can recover the previous running configuration. For systemd, reload reads definitions by default; --reload-service also reloads the running service. --validator auto|sshd|systemd|fstab|sources; --allow-unvalidated is explicit. Applying checks the configuration; failed checks block save. Restore preserves a recovery entry."
        )?,
        "operations" => writeln!(
            output,
            "list displays recoverable IDs; status ID waits one second by default; attach ID reconnects without starting or repeating an operation. Timeout/disconnect returns status 7 with the task ID. Active package writes are never killed."
        )?,
        "firewall" => writeln!(output, "{}", i18n::tr!("cli-administration-help-firewall"))?,
        "ssh" => writeln!(output, "{}", i18n::tr!("cli-administration-help-ssh"))?,
        "backups" => writeln!(output, "{}", i18n::tr!("cli-administration-help-backups"))?,
        "scheduled-tasks" => writeln!(
            output,
            "{}",
            i18n::tr!("cli-administration-help-scheduled-tasks")
        )?,
        _ => {}
    }
    writeln!(output, "{}", i18n::tr!("cli-administration-exit-codes"))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn parse(group: &str, args: &[&str]) -> Result<ManagementCli, CliError> {
        parse_management(
            group,
            &args
                .iter()
                .map(|value| value.to_string())
                .collect::<Vec<_>>(),
        )
    }

    #[test]
    fn chinese_administration_help_keeps_native_option_values() {
        let language = std::sync::Arc::new(i18n::LanguageSnapshotData {
            code: "zh-CN".into(),
            generation: 1,
            current: vec![include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../ascii-assets/assets/locales/zh-CN/modules/management-cli-administration.ftl")).into()],
            english: Vec::new(),
        }.restore().unwrap());
        i18n::with_snapshot(&language, || {
            let mut output = Vec::new();
            help(&mut output, "firewall").unwrap();
            let output = String::from_utf8(output).unwrap();
            assert!(output.contains("用法："));
            assert!(output.contains("--duration runtime|permanent|both"));
            assert!(
                parse("firewall", &["allow", "--port", "0"])
                    .unwrap_err()
                    .to_string()
                    .contains("端口")
            );
        });
    }
    #[test]
    fn formal_commands_keep_secret_text_off_arguments_and_reject_arbitrary_flags() {
        assert!(
            parse(
                "network",
                &["wifi-connect", "wlan0", "--password", "secret"]
            )
            .is_err()
        );
        assert!(
            parse(
                "packages",
                &["install", "bash", "--flags", "--allow-unauthenticated"]
            )
            .is_err()
        );
        let ManagementCli::Run(command) = parse(
            "packages",
            &["install", "bash", "--yes", "--non-interactive", "--json"],
        )
        .unwrap() else {
            panic!()
        };
        assert!(command.yes && command.non_interactive && command.json);
        assert_eq!(command.wait_seconds, 900);
    }
    #[test]
    fn configuration_writes_require_a_prior_version_and_native_commands_are_explicit() {
        assert!(
            parse(
                "system-config",
                &["apply", "/etc/ssh/sshd_config", "--input", "candidate"]
            )
            .is_err()
        );
        assert!(
            parse(
                "services",
                &["create", "demo", "--program", "/bin/sleep", "--apply"]
            )
            .is_err()
        );
        assert!(parse("services", &["create-instance", "demo@.service", "worker"]).is_ok());
        assert!(
            parse(
                "packages",
                &[
                    "sources",
                    "add",
                    "--name",
                    "demo",
                    "--url",
                    "https://example.org"
                ]
            )
            .is_ok()
        );
        assert!(parse("operations", &["attach", "--wait", "4"]).is_err());
        assert!(parse("system-config", &["check", "/etc/ssh/sshd_config"]).is_ok());
        assert!(
            parse(
                "system-config",
                &["check", "/etc/ssh/sshd_config", "--input", "candidate"]
            )
            .is_ok()
        );
        assert!(
            parse(
                "users",
                &["primary-group", "demo", "--primary-group", "staff"]
            )
            .is_ok()
        );
        assert!(parse("network", &["confirm", "1-2", "--decision", "typo"]).is_err());
        assert!(parse("network", &["confirm", "1-2", "--decision", "revert"]).is_ok());
    }
    #[test]
    fn new_management_commands_reject_unsupported_values_before_authorization() {
        for (group, arguments) in [
            ("firewall", vec!["allow", "--port", "65536"]),
            ("firewall", vec!["allow", "--port", "90-80"]),
            (
                "firewall",
                vec!["allow", "--port", "22", "--protocol", "icmp"],
            ),
            ("firewall", vec!["allow", "--port", "--yes"]),
            ("firewall", vec!["allow", "--port", "22", "--yes", "--yes"]),
            ("ssh", vec!["configure", "--open-firewall", "yes"]),
            ("ssh", vec!["configure", "--permit-root-login", "never"]),
            ("ssh", vec!["configure", "--port", "22-23"]),
            ("ssh", vec!["confirm"]),
            ("backups", vec!["restore", "1-2", "--command", "true"]),
            ("scheduled-tasks", vec!["create", "--name", "demo"]),
        ] {
            assert!(parse(group, &arguments).is_err(), "{group} {arguments:?}");
        }
        for (group, arguments) in [
            (
                "firewall",
                vec!["allow", "--port", "8000-8010", "--source", "192.0.2.0/24"],
            ),
            ("ssh", vec!["configure", "--port", "2222"]),
            ("ssh", vec!["confirm", "1-2", "--yes", "--non-interactive"]),
            ("backups", vec!["restore", "1-2", "--allow-unvalidated"]),
            (
                "scheduled-tasks",
                vec![
                    "preview",
                    "--name",
                    "demo",
                    "--command",
                    "echo demo",
                    "--schedule",
                    "daily",
                ],
            ),
        ] {
            assert!(parse(group, &arguments).is_ok(), "{group} {arguments:?}");
        }
    }
}
