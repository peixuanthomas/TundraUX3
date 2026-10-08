//! NSS account queries and fixed, authorized Linux account operations.
//!
//! Saved desktop roles are never consulted here. Writes run only in the root
//! helper, after the caller has obtained real operating-system authorization.
use super::*;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::{Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::Ordering;
use zeroize::Zeroizing;

const MAX_DATA: usize = 4 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UserAccount {
    pub username: String,
    pub uid: u32,
    pub gid: u32,
    pub display_name: String,
    pub home: PathBuf,
    pub shell: String,
    pub local: bool,
    pub system: bool,
    pub groups: Vec<String>,
    #[serde(default)]
    pub groups_complete: bool,
    pub locked: Option<bool>,
    pub expires: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UserGroup {
    pub name: String,
    pub gid: u32,
    pub members: Vec<String>,
    pub local: bool,
}

fn cancelled(flag: &AtomicBool) -> Result<(), ManagementError> {
    if flag.load(Ordering::Relaxed) {
        Err(ManagementError::Cancelled)
    } else {
        Ok(())
    }
}
fn invalid(message: &str) -> ManagementError {
    ManagementError::InvalidInput(message.into())
}
fn io_error(error: std::io::Error) -> ManagementError {
    if error.kind() == std::io::ErrorKind::PermissionDenied {
        ManagementError::PermissionDenied(
            "Access denied. Authorize the operation and retry.".into(),
        )
    } else {
        ManagementError::Failed(error.to_string())
    }
}

fn native(tool: &str, args: &[&str], input: Option<&[u8]>) -> Result<String, ManagementError> {
    let path = ["/usr/sbin", "/usr/bin", "/sbin", "/bin"]
        .into_iter()
        .map(|directory| Path::new(directory).join(tool))
        .find(|path| path.is_file())
        .ok_or_else(|| {
            ManagementError::Unavailable(format!(
                "{tool} is missing. Install the system account tools."
            ))
        })?;
    let mut child = Command::new(path)
        .args(args)
        .env("LC_ALL", "C")
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(io_error)?;
    if let Some(input) = input {
        child
            .stdin
            .take()
            .ok_or_else(|| ManagementError::Failed("Account tool input is unavailable".into()))?
            .write_all(input)
            .map_err(io_error)?;
    }
    let output = child.wait_with_output().map_err(io_error)?;
    if output.stdout.len() > MAX_DATA || output.stderr.len() > MAX_DATA {
        return Err(ManagementError::Failed(
            "Account tool output is too large. Check the system logs.".into(),
        ));
    }
    if !output.status.success() {
        let detail = runtime_log::sanitize_text(String::from_utf8_lossy(&output.stderr).trim());
        return Err(ManagementError::Failed(format!(
            "{tool} failed ({}). Check the account and retry. {detail}",
            output.status.code().unwrap_or(-1)
        )));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn parse_passwd(text: &str) -> Vec<UserAccount> {
    text.lines()
        .filter_map(|line| {
            let fields: Vec<_> = line.split(':').collect();
            if fields.len() != 7 {
                return None;
            }
            Some(UserAccount {
                username: fields[0].into(),
                uid: fields[2].parse().ok()?,
                gid: fields[3].parse().ok()?,
                display_name: fields[4].split(',').next().unwrap_or("").into(),
                home: fields[5].into(),
                shell: fields[6].into(),
                local: false,
                system: false,
                groups: vec![],
                groups_complete: false,
                locked: None,
                expires: None,
            })
        })
        .collect()
}
fn parse_groups(text: &str) -> Vec<UserGroup> {
    text.lines()
        .filter_map(|line| {
            let fields: Vec<_> = line.split(':').collect();
            if fields.len() != 4 {
                return None;
            }
            Some(UserGroup {
                name: fields[0].into(),
                gid: fields[2].parse().ok()?,
                members: fields[3]
                    .split(',')
                    .filter(|name| !name.is_empty())
                    .map(String::from)
                    .collect(),
                local: false,
            })
        })
        .collect()
}

pub fn groups() -> Result<Vec<UserGroup>, ManagementError> {
    let local = parse_groups(&fs::read_to_string("/etc/group").map_err(io_error)?);
    let mut groups = parse_groups(&native("getent", &["group"], None)?);
    for group in &mut groups {
        group.local = local
            .iter()
            .any(|entry| entry.name == group.name && entry.gid == group.gid);
    }
    groups.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(groups)
}

/// Enumerates NSS-visible accounts; a name lookup also reaches directory users
/// whose provider does not support listing all accounts.
pub fn accounts(name: Option<&str>) -> Result<Vec<UserAccount>, ManagementError> {
    if let Some(name) = name {
        validate_lookup_name(name)?;
    }
    let local = parse_passwd(&fs::read_to_string("/etc/passwd").map_err(io_error)?);
    let text = match name {
        Some(name) => native("getent", &["passwd", name], None)?,
        None => native("getent", &["passwd"], None)?,
    };
    let mut accounts = parse_passwd(&text);
    let groups = groups()?;
    let uid_min = fs::read_to_string("/etc/login.defs")
        .ok()
        .and_then(|text| {
            text.lines().find_map(|line| {
                let mut fields = line.split_whitespace();
                (fields.next() == Some("UID_MIN"))
                    .then(|| fields.next().and_then(|value| value.parse::<u32>().ok()))
                    .flatten()
            })
        })
        .unwrap_or(1000);
    let shadow = fs::read_to_string("/etc/shadow").ok();
    for account in &mut accounts {
        account.local = local
            .iter()
            .any(|entry| entry.username == account.username && entry.uid == account.uid);
        account.system = account.uid < uid_min;
        account.groups = groups
            .iter()
            .filter(|group| group.gid == account.gid || group.members.contains(&account.username))
            .map(|group| group.name.clone())
            .collect();
        if name.is_some() {
            complete_account_groups(account);
        }
        if account.local
            && let Some(line) = shadow.as_deref().and_then(|text| {
                text.lines()
                    .find(|line| line.split(':').next() == Some(&account.username))
            })
        {
            let fields: Vec<_> = line.split(':').collect();
            if fields.len() >= 8 {
                account.locked = Some(fields[1].starts_with('!') || fields[1].starts_with('*'));
                account.expires = Some(if fields[7].is_empty() || fields[7] == "-1" {
                    "never".into()
                } else {
                    fields[7]
                        .parse::<i64>()
                        .ok()
                        .and_then(|days| {
                            chrono::DateTime::from_timestamp(days.checked_mul(86400)?, 0)
                        })
                        .map(|date| date.format("%Y-%m-%d").to_string())
                        .unwrap_or_else(|| "unknown".into())
                });
            }
        }
    }
    accounts.sort_by(|a, b| a.username.cmp(&b.username));
    Ok(accounts)
}

fn validate_lookup_name(name: &str) -> Result<(), ManagementError> {
    if name.is_empty()
        || name.len() > 255
        || name.starts_with('-')
        || name.chars().any(char::is_control)
    {
        Err(invalid(
            "Invalid account lookup. Enter its exact system name.",
        ))
    } else {
        Ok(())
    }
}

fn complete_account_groups_with(
    account: &mut UserAccount,
    mut run: impl FnMut(&str, &[&str]) -> Result<String, ManagementError>,
) {
    account.groups_complete = false;
    let result = (|| {
        validate_lookup_name(&account.username)?;
        let ids = run("id", &["--groups", "--", &account.username])?
            .split_whitespace()
            .map(|text| {
                text.parse::<u32>()
                    .map_err(|_| invalid("Group lookup returned an invalid identifier"))
            })
            .collect::<Result<BTreeSet<_>, _>>()?;
        if ids.is_empty() || ids.len() > 65_536 {
            return Err(invalid("Group lookup did not return complete membership"));
        }
        let keys: Vec<_> = ids.iter().map(u32::to_string).collect();
        let mut args = vec!["group"];
        args.extend(keys.iter().map(String::as_str));
        // Numeric IDs avoid splitting directory group names containing spaces.
        // One keyed NSS query also works with providers that cannot enumerate.
        let groups = parse_groups(&run("getent", &args)?);
        if ids
            .iter()
            .any(|id| !groups.iter().any(|group| group.gid == *id))
        {
            return Err(invalid("Group names could not be resolved completely"));
        }
        Ok(groups
            .into_iter()
            .filter(|group| ids.contains(&group.gid))
            .map(|group| group.name)
            .collect::<BTreeSet<_>>())
    })();
    if let Ok(groups) = result {
        account.groups = groups.into_iter().collect();
        account.groups_complete = true;
    }
}

fn complete_account_groups(account: &mut UserAccount) {
    complete_account_groups_with(account, |program, args| native(program, args, None));
}

pub fn allowed_shells() -> Result<Vec<String>, ManagementError> {
    let mut shells: Vec<_> = fs::read_to_string("/etc/shells")
        .map_err(io_error)?
        .lines()
        .map(str::trim)
        .filter(|line| line.starts_with('/') && !line.contains(char::is_whitespace))
        .map(String::from)
        .collect();
    shells.sort();
    shells.dedup();
    Ok(shells)
}
fn field(
    id: &str,
    label: &str,
    value: &str,
    required: bool,
    choices: Vec<String>,
) -> ManagementField {
    ManagementField {
        id: id.into(),
        label: label.into(),
        value: value.into(),
        required,
        choices,
        ..Default::default()
    }
}
fn action(id: &str, label: &str, fields: Vec<ManagementField>, group: &str) -> ManagementAction {
    ManagementAction {
        id: id.into(),
        label: label.into(),
        fields,
        group: group.into(),
        privileged: true,
        confirm: true,
        ..Default::default()
    }
}
fn account_row(
    account: &UserAccount,
    shells: &[String],
    all_groups: &[UserGroup],
) -> ManagementRow {
    let source = if !account.local {
        "remote"
    } else if account.system {
        "system"
    } else {
        "local"
    };
    let mut actions = vec![
        action(
            "user_info",
            "Edit account name",
            vec![field(
                "display_name",
                "Display name",
                &account.display_name,
                false,
                vec![],
            )],
            "account",
        ),
        action(
            "user_password",
            "Set password",
            vec![ManagementField {
                secret: true,
                ..field("password", "Password", "", true, vec![])
            }],
            "account",
        ),
        action("user_lock", "Lock password", vec![], "account"),
        action("user_unlock", "Unlock password", vec![], "account"),
        action(
            "user_groups",
            "Edit groups",
            vec![field(
                "groups",
                "Supplementary groups (comma separated)",
                &account
                    .groups
                    .iter()
                    .filter(|name| {
                        all_groups
                            .iter()
                            .find(|group| &group.name == *name)
                            .is_some_and(|group| group.gid != account.gid)
                    })
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(","),
                false,
                vec![],
            )],
            "account",
        ),
        action(
            "user_shell",
            "Change login Shell",
            vec![field(
                "shell",
                "Login Shell",
                &account.shell,
                true,
                shells.to_vec(),
            )],
            "account",
        ),
        action(
            "user_primary_group",
            "Change primary group",
            vec![field(
                "primary_group",
                "Primary group",
                &all_groups
                    .iter()
                    .find(|g| g.gid == account.gid)
                    .map(|g| g.name.clone())
                    .unwrap_or_default(),
                true,
                all_groups
                    .iter()
                    .filter(|g| g.local)
                    .map(|g| g.name.clone())
                    .collect(),
            )],
            "account",
        ),
        action(
            "user_expiry",
            "Set account expiry",
            vec![field(
                "expires",
                "Expiry (YYYY-MM-DD or never)",
                account.expires.as_deref().unwrap_or("never"),
                true,
                vec![],
            )],
            "account",
        ),
        action("user_ssh_keys", "View SSH public keys", vec![], "ssh"),
        action(
            "user_ssh_add",
            "Add SSH public key",
            vec![field("public_key", "SSH public key", "", true, vec![])],
            "ssh",
        ),
        action(
            "user_ssh_remove",
            "Remove SSH public key",
            vec![field(
                "fingerprint",
                "Key fingerprint (SHA256:…)",
                "",
                true,
                vec![],
            )],
            "ssh",
        ),
        action("user_delete", "Delete account", vec![], "danger"),
    ];
    for action in &mut actions {
        if !account.local {
            action.disabled_reason =
                Some("Directory accounts are read-only. Use the directory administrator.".into());
        }
        if account.uid == 0 && action.id == "user_delete" {
            action.disabled_reason = Some("Root cannot be deleted.".into());
        }
        if account.local && action.id == "user_groups" && !account.groups_complete {
            action.disabled_reason = Some("Group membership is unknown. Select the account and refresh before editing groups.".into());
        }
    }
    ManagementRow {
        id: account.username.clone(),
        cells: vec![
            account.username.clone(),
            account.uid.to_string(),
            source.into(),
            account.shell.clone(),
            account.display_name.clone(),
        ],
        detail: vec![
            ("Source".into(), source.into()),
            ("UID".into(), account.uid.to_string()),
            ("Primary group ID".into(), account.gid.to_string()),
            ("Home".into(), account.home.display().to_string()),
            ("Login Shell".into(), account.shell.clone()),
            (
                "Groups".into(),
                if account.groups_complete {
                    account.groups.join(", ")
                } else {
                    "unknown (select the account to query full membership)".into()
                },
            ),
            (
                "Password locked".into(),
                account
                    .locked
                    .map(|value| value.to_string())
                    .unwrap_or_else(|| "unknown (requires authorization)".into()),
            ),
            (
                "Account expiry".into(),
                account
                    .expires
                    .clone()
                    .unwrap_or_else(|| "unknown (requires authorization)".into()),
            ),
        ],
        actions,
        identity: BTreeMap::from([
            ("username".into(), account.username.clone()),
            ("uid".into(), account.uid.to_string()),
            ("gid".into(), account.gid.to_string()),
            ("home".into(), account.home.display().to_string()),
        ]),
    }
}

pub fn query(
    query: &ManagementQuery,
    flag: &AtomicBool,
) -> Result<ManagementSnapshot, ManagementError> {
    cancelled(flag)?;
    let mut snapshot = ManagementSnapshot {
        backend: "NSS / AccountsService / system account tools".into(),
        actions: vec![ManagementAction {
            id: "set_view".into(),
            label: "Choose accounts or groups".into(),
            fields: vec![field(
                "scope",
                "View",
                if query.scope == "groups" {
                    "groups"
                } else {
                    "users"
                },
                true,
                vec!["users".into(), "groups".into()],
            )],
            ..Default::default()
        }],
        ..Default::default()
    };
    if query.scope == "groups" {
        snapshot.columns = ["Group", "GID", "Source", "Members"]
            .map(String::from)
            .to_vec();
        snapshot.actions.push(action(
            "group_create",
            "Create group",
            vec![field("name", "Group name", "", true, vec![])],
            "create",
        ));
        for group in groups()? {
            cancelled(flag)?;
            if !query.filter.is_empty()
                && !format!("{} {}", group.name, group.members.join(" "))
                    .to_lowercase()
                    .contains(&query.filter.to_lowercase())
            {
                continue;
            }
            let mut actions = vec![
                action(
                    "group_members",
                    "Edit group members",
                    vec![field(
                        "members",
                        "Members (comma separated)",
                        &group.members.join(","),
                        false,
                        vec![],
                    )],
                    "account",
                ),
                action(
                    "group_rename",
                    "Rename group",
                    vec![field("name", "Group name", &group.name, true, vec![])],
                    "account",
                ),
                action("group_delete", "Delete group", vec![], "danger"),
            ];
            for action in &mut actions {
                if !group.local {
                    action.disabled_reason = Some("Directory groups are read-only.".into());
                }
            }
            snapshot.rows.push(ManagementRow {
                id: format!("group:{}", group.name),
                cells: vec![
                    group.name.clone(),
                    group.gid.to_string(),
                    if group.local {
                        "local".into()
                    } else {
                        "remote".into()
                    },
                    group.members.join(", "),
                ],
                actions,
                identity: BTreeMap::from([
                    ("group".into(), group.name),
                    ("gid".into(), group.gid.to_string()),
                ]),
                ..Default::default()
            });
        }
    } else {
        snapshot.columns = ["Account", "UID", "Source", "Login Shell", "Display name"]
            .map(String::from)
            .to_vec();
        let shells = allowed_shells().unwrap_or_default();
        let all_groups = groups()?;
        snapshot.actions.push(action(
            "user_create",
            "Create account",
            vec![
                field("username", "Account name", "", true, vec![]),
                field("display_name", "Display name", "", false, vec![]),
                field(
                    "shell",
                    "Login Shell",
                    if shells.iter().any(|shell| shell == "/bin/bash") {
                        "/bin/bash"
                    } else {
                        shells.first().map(String::as_str).unwrap_or("")
                    },
                    true,
                    shells.clone(),
                ),
                ManagementField {
                    secret: true,
                    ..field("password", "Password", "", true, vec![])
                },
            ],
            "create",
        ));
        snapshot.actions.push(ManagementAction {
            privileged: false,
            confirm: false,
            ..action(
                "user_lookup",
                "Find account by name",
                vec![field("username", "Account name", "", true, vec![])],
                "lookup",
            )
        });
        let lookup = query.options.get("username").map(String::as_str);
        for mut account in accounts(lookup)? {
            cancelled(flag)?;
            if lookup.is_none() && query.target.as_deref() == Some(account.username.as_str()) {
                complete_account_groups(&mut account);
            }
            if !query.filter.is_empty()
                && !format!(
                    "{} {} {} {}",
                    account.username, account.uid, account.display_name, account.shell
                )
                .to_lowercase()
                .contains(&query.filter.to_lowercase())
            {
                continue;
            }
            snapshot
                .rows
                .push(account_row(&account, &shells, &all_groups));
        }
    }
    Ok(snapshot)
}

fn validate_name(name: &str) -> Result<(), ManagementError> {
    if name.is_empty()
        || name.len() > 255
        || name.starts_with('-')
        || name
            .chars()
            .any(|c| c.is_control() || c.is_whitespace() || matches!(c, ':' | ',' | '/' | '\\'))
    {
        Err(invalid(
            "Invalid account or group name. Enter the exact system name.",
        ))
    } else {
        Ok(())
    }
}
fn value<'a>(command: &'a ManagementCommand, name: &str) -> &'a str {
    command.values.get(name).map(String::as_str).unwrap_or("")
}
fn names(text: &str) -> Result<Vec<String>, ManagementError> {
    let mut result = Vec::new();
    for name in text
        .split(',')
        .map(str::trim)
        .filter(|name| !name.is_empty())
    {
        validate_name(name)?;
        result.push(name.into());
    }
    result.sort();
    result.dedup();
    Ok(result)
}
fn validate_expiry(text: &str, current: bool) -> Result<(), ManagementError> {
    if text == "never" {
        return Ok(());
    }
    let date = chrono::NaiveDate::parse_from_str(text, "%Y-%m-%d")
        .map_err(|_| invalid("Invalid expiry date. Use YYYY-MM-DD or never."))?;
    if date.format("%Y-%m-%d").to_string() != text {
        return Err(invalid("Invalid expiry date. Use YYYY-MM-DD or never."));
    }
    if current && date <= chrono::Utc::now().date_naive() {
        return Err(ManagementError::PermissionDenied(
            "The current account cannot expire today. Choose a future date.".into(),
        ));
    }
    Ok(())
}
fn protect_user(
    account: &UserAccount,
    actor: u32,
    action: &str,
    groups_value: &str,
) -> Result<(), ManagementError> {
    if !account.local {
        return Err(ManagementError::PermissionDenied(
            "Directory accounts are read-only.".into(),
        ));
    }
    if action == "user_delete" && account.uid == 0 {
        return Err(ManagementError::PermissionDenied(
            "Root cannot be deleted.".into(),
        ));
    }
    if account.uid == actor {
        if matches!(action, "user_delete" | "user_lock") {
            return Err(ManagementError::PermissionDenied(
                "The current account cannot be deleted or locked.".into(),
            ));
        }
        if action == "user_groups" {
            if !account.groups_complete {
                return Err(ManagementError::Failed("Current account groups are unknown. Check the account lookup and refresh before editing groups.".into()));
            }
            let proposed = names(groups_value)?;
            let primary = groups()?
                .into_iter()
                .find(|group| group.gid == account.gid)
                .map(|group| group.name);
            if account.groups.iter().any(|name| {
                is_admin_group(name) && primary.as_ref() != Some(name) && !proposed.contains(name)
            }) {
                return Err(ManagementError::PermissionDenied(
                    "Keep the current account's administrator groups.".into(),
                ));
            }
        }
    }
    Ok(())
}

fn policy_groups(text: &str) -> BTreeSet<String> {
    let mut groups = BTreeSet::new();
    for line in text.lines() {
        let line = line.split('#').next().unwrap_or_default();
        // Keep custom sudoers groups as well as the usual distribution groups.
        for rest in line.split('%').skip(1) {
            let name: String = rest
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
                .collect();
            if !name.is_empty() {
                groups.insert(name);
            }
        }
        for rest in line.split("unix-group:").skip(1) {
            let name: String = rest
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
                .collect();
            if !name.is_empty() {
                groups.insert(name);
            }
        }
        for rest in line.split("isInGroup(").skip(1) {
            let rest = rest.trim_start();
            if let Some(quote) = rest.chars().next().filter(|c| matches!(c, '\'' | '"')) {
                if let Some((name, _)) = rest[1..].split_once(quote) {
                    if validate_name(name).is_ok() {
                        groups.insert(name.into());
                    }
                }
            }
        }
    }
    groups
}

fn is_admin_group(name: &str) -> bool {
    if matches!(name, "sudo" | "wheel" | "admin") {
        return true;
    }
    let mut files = vec![PathBuf::from("/etc/sudoers")];
    for directory in [
        "/etc/sudoers.d",
        "/etc/polkit-1/rules.d",
        "/usr/share/polkit-1/rules.d",
        "/etc/polkit-1/localauthority.conf.d",
    ] {
        if let Ok(entries) = fs::read_dir(directory) {
            files.extend(
                entries
                    .take(1000)
                    .flatten()
                    .map(|entry| entry.path())
                    .filter(|path| path.is_file()),
            );
        }
    }
    files.into_iter().any(|path| {
        File::open(path)
            .ok()
            .and_then(|file| {
                let mut text = String::new();
                file.take(MAX_DATA as u64)
                    .read_to_string(&mut text)
                    .ok()
                    .map(|_| text)
            })
            .is_some_and(|text| policy_groups(&text).contains(name))
    })
}

pub fn execute(
    command: &ManagementCommand,
    context: &ExecutionContext,
    interaction: &mut dyn OperationInteraction,
    flag: &AtomicBool,
) -> Result<String, ManagementError> {
    cancelled(flag)?;
    if command.action == "user_lookup" {
        let mut query = ManagementQuery::new(ManagementKind::Users);
        query
            .options
            .insert("username".into(), value(command, "username").into());
        interaction.emit(OperationEvent::Snapshot {
            snapshot: self::query(&query, flag)?,
        });
        return Ok("Account found".into());
    }
    if unsafe { libc::getuid() } != 0 || unsafe { libc::geteuid() } != 0 {
        return Err(ManagementError::PermissionDenied(
            "System authorization is required. Authorize the account operation.".into(),
        ));
    }
    if command.action.starts_with("group_") {
        return execute_group(command, context);
    }
    if command.action == "user_create" {
        return create_account(command, interaction);
    }
    let name = command
        .target
        .as_deref()
        .ok_or_else(|| invalid("Choose an account."))?;
    validate_name(name)?;
    let account = accounts(Some(name))?
        .into_iter()
        .find(|account| account.username == name)
        .ok_or_else(|| {
            ManagementError::Conflict("The account disappeared. Refresh and retry.".into())
        })?;
    if command.identity.get("uid") != Some(&account.uid.to_string())
        || command.identity.get("gid") != Some(&account.gid.to_string())
        || command.identity.get("home") != Some(&account.home.display().to_string())
    {
        return Err(ManagementError::Conflict(
            "The account changed. Refresh and retry.".into(),
        ));
    }
    protect_user(
        &account,
        context.actor_uid,
        &command.action,
        value(command, "groups"),
    )?;
    let result = match command.action.as_str() {
        "user_info" => {
            let display = value(command, "display_name");
            if display
                .chars()
                .any(|c| c.is_control() || matches!(c, ':' | ','))
            {
                return Err(invalid(
                    "Invalid display name. Remove control characters, commas and colons.",
                ));
            }
            if accounts_write(&account, "SetRealName", display)? {
                Ok(())
            } else {
                native("usermod", &["--comment", display, "--", name], None).map(|_| ())
            }
        }
        "user_password" => set_password(&account, value(command, "password"), interaction),
        "user_lock" | "user_unlock" => {
            let locked = command.action == "user_lock";
            if accounts_write(&account, "SetLocked", if locked { "true" } else { "false" })? {
                Ok(())
            } else {
                native(
                    "usermod",
                    &[if locked { "--lock" } else { "--unlock" }, "--", name],
                    None,
                )
                .map(|_| ())
            }
        }
        "user_delete" => {
            if accounts_write(&account, "DeleteUser", "")? {
                Ok(())
            } else {
                native("userdel", &["--", name], None).map(|_| ())
            }
        }
        "user_shell" => {
            let shell = value(command, "shell");
            if !allowed_shells()?.iter().any(|item| item == shell) {
                return Err(invalid(
                    "Shell is not in /etc/shells. Choose an allowed Shell.",
                ));
            }
            native("usermod", &["--shell", shell, "--", name], None).map(|_| ())
        }
        "user_expiry" => {
            let expires = value(command, "expires");
            validate_expiry(expires, account.uid == context.actor_uid)?;
            native(
                "chage",
                &[
                    "--expiredate",
                    if expires == "never" { "-1" } else { expires },
                    "--",
                    name,
                ],
                None,
            )
            .map(|_| ())
        }
        "user_groups" => {
            let requested = names(value(command, "groups"))?;
            let available = groups()?;
            for name in &requested {
                if !available
                    .iter()
                    .any(|group| group.local && &group.name == name)
                {
                    return Err(invalid("Choose existing local groups."));
                }
            }
            native(
                "usermod",
                &["--groups", &requested.join(","), "--", name],
                None,
            )
            .map(|_| ())
        }
        "user_primary_group" => {
            let requested = value(command, "primary_group");
            let available = groups()?;
            if !available
                .iter()
                .any(|group| group.local && group.name == requested)
            {
                return Err(invalid("Choose an existing local primary group."));
            }
            if account.uid == context.actor_uid
                && available
                    .iter()
                    .any(|group| group.gid == account.gid && is_admin_group(&group.name))
                && available
                    .iter()
                    .find(|group| group.name == requested)
                    .is_some_and(|group| group.gid != account.gid)
            {
                return Err(ManagementError::PermissionDenied(
                    "Keep the current account's administrator primary group.".into(),
                ));
            }
            native("usermod", &["--gid", requested, "--", name], None).map(|_| ())
        }
        "user_ssh_keys" | "user_ssh_add" | "user_ssh_remove" => {
            ssh_keys(command, &account, interaction)
        }
        _ => Err(invalid("Unknown account operation.")),
    };
    result?;
    Ok(if command.action == "user_delete" {
        "Account deleted. Home directory retained.".into()
    } else {
        "Account operation completed".into()
    })
}

fn fallback_error(error: &zbus::Error) -> bool {
    matches!(error, zbus::Error::MethodError(name,_,_) if matches!(name.as_str(),
        "org.freedesktop.DBus.Error.ServiceUnknown" | "org.freedesktop.DBus.Error.NameHasNoOwner" |
        "org.freedesktop.DBus.Error.UnknownMethod" | "org.freedesktop.DBus.Error.NotSupported" |
        "org.freedesktop.Accounts.Error.NotSupported"))
}
fn account_dbus_error(error: zbus::Error) -> ManagementError {
    if matches!(&error,zbus::Error::MethodError(name,_,_) if matches!(name.as_str(),"org.freedesktop.DBus.Error.AccessDenied" | "org.freedesktop.Accounts.Error.PermissionDenied" | "org.freedesktop.PolicyKit1.Error.NotAuthorized"))
    {
        ManagementError::PermissionDenied(
            "AccountsService denied this change. Check system permissions.".into(),
        )
    } else {
        ManagementError::Failed(format!(
            "AccountsService did not confirm the change. Refresh before retrying. {error}"
        ))
    }
}

/// Only explicit absence or an unsupported method permits another backend.
/// Denial, disconnect, timeout and unknown outcome must never retry a write.
fn accounts_write(
    account: &UserAccount,
    method: &str,
    text: &str,
) -> Result<bool, ManagementError> {
    let connection = match crate::linux::dbus::system() {
        Ok(connection) => connection,
        Err(crate::service::ServiceError::ServiceUnavailable) => return Ok(false),
        Err(error) => {
            return Err(ManagementError::Failed(format!(
                "Account service unavailable. Check the system bus. {error}"
            )));
        }
    };
    let manager = zbus::blocking::Proxy::new(
        &connection,
        "org.freedesktop.Accounts",
        "/org/freedesktop/Accounts",
        "org.freedesktop.Accounts",
    )
    .map_err(account_dbus_error)?;
    let result: Result<(), zbus::Error> = if method == "DeleteUser" {
        manager.call(method, &(i64::from(account.uid), false))
    } else {
        let path: zbus::zvariant::OwnedObjectPath =
            match manager.call("FindUserByName", &(account.username.as_str(),)) {
                Ok(path) => path,
                Err(error) if fallback_error(&error) => return Ok(false),
                // AccountsService intentionally does not expose some system users.
                Err(zbus::Error::MethodError(name, _, _))
                    if name.as_str() == "org.freedesktop.Accounts.Error.UserDoesNotExist" =>
                {
                    return Ok(false);
                }
                Err(error) => return Err(account_dbus_error(error)),
            };
        let proxy = zbus::blocking::Proxy::new(
            &connection,
            "org.freedesktop.Accounts",
            path,
            "org.freedesktop.Accounts.User",
        )
        .map_err(account_dbus_error)?;
        match method {
            "SetLocked" => proxy.call(method, &(text == "true",)),
            "SetPassword" => proxy.call(method, &(text, "")),
            "SetRealName" => proxy.call(method, &(text,)),
            _ => return Err(invalid("Unknown AccountsService operation.")),
        }
    };
    match result {
        Ok(()) => Ok(true),
        Err(error) if fallback_error(&error) => Ok(false),
        Err(error) => Err(account_dbus_error(error)),
    }
}
fn password_text(text: &str) -> Result<(), ManagementError> {
    if text.is_empty() || text.chars().any(|c| matches!(c, '\0' | '\r' | '\n')) {
        Err(invalid(
            "Invalid password. Enter a nonempty password without line breaks.",
        ))
    } else {
        Ok(())
    }
}
fn set_password(
    account: &UserAccount,
    text: &str,
    _interaction: &mut dyn OperationInteraction,
) -> Result<(), ManagementError> {
    password_text(text)?;
    let hash = Zeroizing::new(
        crate::linux::accounts::management_password_hash(text).map_err(|_| {
            ManagementError::Unavailable(
                "Password hashing is unavailable. Install libcrypt.".into(),
            )
        })?,
    );
    if accounts_write(account, "SetPassword", &hash)? {
        return Ok(());
    }
    let input = Zeroizing::new(format!("{}:{}\n", account.username, hash.as_str()));
    native("chpasswd", &["--encrypted"], Some(input.as_bytes())).map(|_| ())
}
fn create_account(
    command: &ManagementCommand,
    interaction: &mut dyn OperationInteraction,
) -> Result<String, ManagementError> {
    let name = value(command, "username");
    validate_name(name)?;
    if !name
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'$' | b'.'))
    {
        return Err(invalid(
            "Use letters, numbers, underscores, dots or hyphens for the account name.",
        ));
    }
    let shell = value(command, "shell");
    if !allowed_shells()?.iter().any(|item| item == shell) {
        return Err(invalid("Choose a Shell from /etc/shells."));
    }
    let display = value(command, "display_name");
    if display
        .chars()
        .any(|c| c.is_control() || matches!(c, ':' | ','))
    {
        return Err(invalid("Invalid display name."));
    }
    password_text(value(command, "password"))?;
    // A supported AccountsService owns creation. An unsupported/missing service
    // permits useradd; a denied or unconfirmed write is never retried.
    let created_by_accounts = accounts_create(name, display)?;
    if !created_by_accounts {
        native(
            "useradd",
            &[
                "--create-home",
                "--shell",
                shell,
                "--comment",
                display,
                "--",
                name,
            ],
            None,
        )?;
    }
    let account = accounts(Some(name))?.into_iter().next().ok_or_else(|| {
        ManagementError::Failed(
            "Account created, but lookup failed. Refresh the account list.".into(),
        )
    })?;
    if created_by_accounts && account.shell != shell {
        native("usermod", &["--shell", shell, "--", name], None).map_err(|_| {
            ManagementError::Failed(
                "Account created; Shell setup failed. Refresh and choose Change login Shell."
                    .into(),
            )
        })?;
    }
    set_password(&account, value(command, "password"), interaction).map_err(|_| {
        ManagementError::Failed(
            "Account created; password setup failed. Refresh and choose Set password.".into(),
        )
    })?;
    Ok("Account created".into())
}

fn accounts_create(name: &str, display: &str) -> Result<bool, ManagementError> {
    let connection = match crate::linux::dbus::system() {
        Ok(connection) => connection,
        Err(crate::service::ServiceError::ServiceUnavailable) => return Ok(false),
        Err(error) => {
            return Err(ManagementError::Failed(format!(
                "Account service unavailable. Check the system bus. {error}"
            )));
        }
    };
    let manager = zbus::blocking::Proxy::new(
        &connection,
        "org.freedesktop.Accounts",
        "/org/freedesktop/Accounts",
        "org.freedesktop.Accounts",
    )
    .map_err(account_dbus_error)?;
    let result: Result<zbus::zvariant::OwnedObjectPath, zbus::Error> =
        manager.call("CreateUser", &(name, display, 0_i32));
    match result {
        Ok(_) => Ok(true),
        Err(error) if fallback_error(&error) => Ok(false),
        Err(error) => Err(account_dbus_error(error)),
    }
}
fn execute_group(
    command: &ManagementCommand,
    context: &ExecutionContext,
) -> Result<String, ManagementError> {
    if command.action == "group_create" {
        let name = value(command, "name");
        validate_name(name)?;
        native("groupadd", &["--", name], None)?;
        return Ok("Group created".into());
    }
    let name = command
        .target
        .as_deref()
        .and_then(|name| name.strip_prefix("group:"))
        .ok_or_else(|| invalid("Choose a group."))?;
    let group = groups()?
        .into_iter()
        .find(|group| group.name == name)
        .ok_or_else(|| ManagementError::Conflict("Group disappeared. Refresh and retry.".into()))?;
    if !group.local {
        return Err(ManagementError::PermissionDenied(
            "Directory groups are read-only.".into(),
        ));
    }
    if command.identity.get("gid") != Some(&group.gid.to_string()) {
        return Err(ManagementError::Conflict(
            "Group changed. Refresh and retry.".into(),
        ));
    }
    let accounts = accounts(None)?;
    let actor_by_uid = parse_passwd(&native(
        "getent",
        &["passwd", &context.actor_uid.to_string()],
        None,
    )?)
    .into_iter()
    .next();
    let mut actor = accounts
        .iter()
        .find(|account| account.uid == context.actor_uid)
        .cloned()
        .or(actor_by_uid);
    if let Some(actor) = actor.as_mut() {
        complete_account_groups(actor);
    }
    let protect_admin = actor.as_ref().is_some_and(|actor| {
        actor.gid == group.gid
            || actor.groups.contains(&group.name)
            || group.members.contains(&actor.username)
    }) && is_admin_group(name);
    if is_admin_group(name) && actor.as_ref().is_none_or(|actor| !actor.groups_complete) {
        return Err(ManagementError::Failed(
            "Current account groups are unknown. Refresh before changing an administrator group."
                .into(),
        ));
    }
    match command.action.as_str() {
        "group_delete" => {
            if accounts.iter().any(|account| account.gid == group.gid) {
                return Err(ManagementError::Conflict(
                    "This group is a primary group. Change the accounts' primary group first."
                        .into(),
                ));
            }
            if protect_admin {
                return Err(ManagementError::PermissionDenied(
                    "Keep the current account's administrator group.".into(),
                ));
            }
            native("groupdel", &["--", name], None)?;
        }
        "group_rename" => {
            if protect_admin {
                return Err(ManagementError::PermissionDenied(
                    "Keep the current account's administrator group name.".into(),
                ));
            }
            let new_name = value(command, "name");
            validate_name(new_name)?;
            native("groupmod", &["--new-name", new_name, "--", name], None)?;
        }
        "group_members" => {
            let members = names(value(command, "members"))?;
            for member in &members {
                if !accounts
                    .iter()
                    .any(|account| account.local && &account.username == member)
                {
                    return Err(invalid("Choose existing local accounts."));
                }
            }
            if protect_admin
                && actor.as_ref().is_some_and(|actor| {
                    !members.contains(&actor.username) && actor.gid != group.gid
                })
            {
                return Err(ManagementError::PermissionDenied(
                    "Keep the current account in its administrator group.".into(),
                ));
            }
            native(
                "gpasswd",
                &["--members", &members.join(","), "--", name],
                None,
            )?;
        }
        _ => return Err(invalid("Unknown group operation.")),
    }
    Ok("Group operation completed".into())
}

fn ssh_key_paths(account: &UserAccount) -> Result<Vec<PathBuf>, ManagementError> {
    let output = native(
        "sshd",
        &[
            "-T",
            "-C",
            &format!("user={},host=localhost,addr=127.0.0.1", account.username),
        ],
        None,
    )?;
    let configured = output
        .lines()
        .find_map(|line| line.strip_prefix("authorizedkeysfile "))
        .ok_or_else(|| {
            ManagementError::Unavailable(
                "SSH key file configuration is unavailable. Check sshd configuration.".into(),
            )
        })?;
    let mut paths = Vec::new();
    for item in configured.split_whitespace() {
        if item == "none" {
            continue;
        }
        let expanded = item
            .replace("%%", "\u{1}")
            .replace("%h", &account.home.display().to_string())
            .replace("%u", &account.username)
            .replace("%U", &account.uid.to_string())
            .replace('\u{1}', "%");
        if expanded.contains('%') {
            return Err(invalid(
                "This SSH key path uses an unsupported token. Edit the SSH configuration first.",
            ));
        }
        let path = PathBuf::from(expanded);
        let path = if path.is_absolute() {
            path
        } else {
            account.home.join(path)
        };
        if path
            .components()
            .any(|component| matches!(component, Component::ParentDir | Component::CurDir))
        {
            return Err(invalid(
                "SSH key path is not a direct path. Check AuthorizedKeysFile.",
            ));
        }
        if !paths.contains(&path) {
            paths.push(path);
        }
    }
    if paths.is_empty() {
        return Err(ManagementError::Unavailable(
            "SSH public key files are disabled. Configure AuthorizedKeysFile.".into(),
        ));
    }
    Ok(paths)
}
fn fingerprint(line: &str) -> Result<String, ManagementError> {
    let mut file = tempfile::NamedTempFile::new().map_err(io_error)?;
    file.as_file()
        .set_permissions(fs::Permissions::from_mode(0o600))
        .map_err(io_error)?;
    file.write_all(line.as_bytes()).map_err(io_error)?;
    file.write_all(b"\n").map_err(io_error)?;
    let output = native(
        "ssh-keygen",
        &[
            "-l",
            "-E",
            "sha256",
            "-f",
            file.path()
                .to_str()
                .ok_or_else(|| invalid("Invalid temporary path"))?,
        ],
        None,
    )?;
    output
        .split_whitespace()
        .nth(1)
        .filter(|value| value.starts_with("SHA256:"))
        .map(String::from)
        .ok_or_else(|| invalid("Invalid SSH public key. Paste a complete public key."))
}

/// Walk from / using directory descriptors. A symlink replacement cannot send
/// an authorized key edit to another file, even while a user changes their home.
fn key_directory(
    path: &Path,
    account: &UserAccount,
    create: bool,
) -> Result<(File, String), ManagementError> {
    use std::ffi::CString;
    let filename = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| invalid("Invalid SSH key path."))?
        .to_string();
    let parent = path
        .parent()
        .ok_or_else(|| invalid("Invalid SSH key path."))?;
    let mut directory = File::open("/").map_err(io_error)?;
    let mut traversed = PathBuf::from("/");
    for component in parent.components() {
        let Component::Normal(name) = component else {
            continue;
        };
        let name_text = name
            .to_str()
            .ok_or_else(|| invalid("SSH key paths must be UTF-8."))?;
        let c_name = CString::new(name_text).map_err(|_| invalid("Invalid SSH key path."))?;
        traversed.push(name);
        let mut fd = unsafe {
            libc::openat(
                directory.as_raw_fd(),
                c_name.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        };
        if fd < 0
            && std::io::Error::last_os_error().kind() == std::io::ErrorKind::NotFound
            && create
            && traversed == account.home.join(".ssh")
        {
            if unsafe { libc::mkdirat(directory.as_raw_fd(), c_name.as_ptr(), 0o700) } != 0 {
                return Err(io_error(std::io::Error::last_os_error()));
            }
            fd = unsafe {
                libc::openat(
                    directory.as_raw_fd(),
                    c_name.as_ptr(),
                    libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                )
            };
            if fd >= 0 && unsafe { libc::fchown(fd, account.uid, account.gid) } != 0 {
                unsafe {
                    libc::close(fd);
                }
                return Err(io_error(std::io::Error::last_os_error()));
            }
        }
        if fd < 0 {
            return Err(io_error(std::io::Error::last_os_error()));
        }
        use std::os::fd::FromRawFd;
        directory = unsafe { File::from_raw_fd(fd) };
        let metadata = directory.metadata().map_err(io_error)?;
        if metadata.mode() & 0o002 != 0
            || !matches!(metadata.uid(), 0) && metadata.uid() != account.uid
        {
            return Err(ManagementError::PermissionDenied(
                "SSH key directory has unsafe ownership or permissions. Fix the directory first."
                    .into(),
            ));
        }
    }
    Ok((directory, filename))
}
fn read_key_file(
    directory: &File,
    name: &str,
) -> Result<(String, Option<(u64, u64, u32, u32, u32)>), ManagementError> {
    use std::ffi::CString;
    use std::os::fd::FromRawFd;
    let name = CString::new(name).map_err(|_| invalid("Invalid key filename."))?;
    let fd = unsafe {
        libc::openat(
            directory.as_raw_fd(),
            name.as_ptr(),
            libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK,
        )
    };
    if fd < 0 {
        let error = std::io::Error::last_os_error();
        if error.kind() == std::io::ErrorKind::NotFound {
            return Ok((String::new(), None));
        }
        return Err(io_error(error));
    }
    let mut file = unsafe { File::from_raw_fd(fd) };
    let metadata = file.metadata().map_err(io_error)?;
    if !metadata.is_file() || metadata.nlink() != 1 || metadata.len() > MAX_DATA as u64 {
        return Err(invalid(
            "SSH key file is not a regular single-link file or is too large.",
        ));
    }
    let mut content = String::new();
    file.read_to_string(&mut content).map_err(io_error)?;
    Ok((
        content,
        Some((
            metadata.dev(),
            metadata.ino(),
            metadata.uid(),
            metadata.gid(),
            metadata.mode(),
        )),
    ))
}
fn write_key_file(
    directory: &File,
    name: &str,
    expected: &str,
    identity: Option<(u64, u64, u32, u32, u32)>,
    content: &str,
    account: &UserAccount,
) -> Result<(), ManagementError> {
    use std::ffi::CString;
    use std::os::fd::FromRawFd;
    let current = read_key_file(directory, name)?;
    if current.0 != expected || current.1 != identity {
        return Err(ManagementError::Conflict(
            "SSH key file changed. View keys again before retrying.".into(),
        ));
    }
    // Stage beside the destination. Preserve all extended attributes (including
    // ACLs), ownership and mode before replacing the old file in one rename.
    let c_name = CString::new(name).map_err(|_| invalid("Invalid key filename."))?;
    let mut staged =
        tempfile::NamedTempFile::new_in(format!("/proc/self/fd/{}", directory.as_raw_fd()))
            .map_err(io_error)?;
    staged.write_all(content.as_bytes()).map_err(io_error)?;
    let (uid, gid, mode) = identity
        .map(|(_, _, uid, gid, mode)| (uid, gid, mode & 0o7777))
        .unwrap_or((account.uid, account.gid, 0o600));
    if uid != 0 && uid != account.uid {
        return Err(ManagementError::PermissionDenied(
            "SSH key file belongs to another account. Check the file owner.".into(),
        ));
    }
    if unsafe { libc::fchown(staged.as_raw_fd(), uid, gid) } != 0
        || unsafe { libc::fchmod(staged.as_raw_fd(), mode) } != 0
    {
        return Err(io_error(std::io::Error::last_os_error()));
    }
    if let Some((dev, ino, _, _, _)) = identity {
        let fd = unsafe {
            libc::openat(
                directory.as_raw_fd(),
                c_name.as_ptr(),
                libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        };
        if fd < 0 {
            return Err(io_error(std::io::Error::last_os_error()));
        }
        let original = unsafe { File::from_raw_fd(fd) };
        let metadata = original.metadata().map_err(io_error)?;
        if metadata.dev() != dev || metadata.ino() != ino {
            return Err(ManagementError::Conflict(
                "SSH key file was replaced. Refresh and retry.".into(),
            ));
        }
        copy_key_attributes(&original, staged.as_file())?;
    }
    staged.as_file().sync_all().map_err(io_error)?;
    let latest = read_key_file(directory, name)?;
    if latest.0 != expected || latest.1 != identity {
        return Err(ManagementError::Conflict(
            "SSH key file changed. View keys and retry.".into(),
        ));
    }
    let staged_name = CString::new(staged.path().file_name().unwrap().as_encoded_bytes())
        .map_err(|_| invalid("Invalid staging filename."))?;
    let rename_result = if identity.is_none() {
        // RENAME_NOREPLACE protects a new file from a concurrent creator.
        unsafe {
            libc::renameat2(
                directory.as_raw_fd(),
                staged_name.as_ptr(),
                directory.as_raw_fd(),
                c_name.as_ptr(),
                libc::RENAME_NOREPLACE,
            )
        }
    } else {
        unsafe {
            libc::renameat(
                directory.as_raw_fd(),
                staged_name.as_ptr(),
                directory.as_raw_fd(),
                c_name.as_ptr(),
            )
        }
    };
    if rename_result != 0 {
        return Err(io_error(std::io::Error::last_os_error()));
    }
    directory.sync_all().map_err(io_error)
}

fn copy_key_attributes(source: &File, destination: &File) -> Result<(), ManagementError> {
    let size = unsafe { libc::flistxattr(source.as_raw_fd(), std::ptr::null_mut(), 0) };
    if size < 0 {
        let error = std::io::Error::last_os_error();
        if error.raw_os_error() == Some(libc::ENOTSUP) {
            return Ok(());
        }
        return Err(io_error(error));
    }
    let mut names = vec![0_u8; size as usize];
    if size > 0
        && unsafe { libc::flistxattr(source.as_raw_fd(), names.as_mut_ptr().cast(), names.len()) }
            != size
    {
        return Err(ManagementError::Conflict(
            "SSH key file attributes changed. Refresh and retry.".into(),
        ));
    }
    for name in names
        .split(|byte| *byte == 0)
        .filter(|name| !name.is_empty())
    {
        let name = std::ffi::CString::new(name).map_err(|_| invalid("Invalid file attribute."))?;
        let size =
            unsafe { libc::fgetxattr(source.as_raw_fd(), name.as_ptr(), std::ptr::null_mut(), 0) };
        if size < 0 {
            return Err(io_error(std::io::Error::last_os_error()));
        }
        let mut value = vec![0_u8; size as usize];
        if unsafe {
            libc::fgetxattr(
                source.as_raw_fd(),
                name.as_ptr(),
                value.as_mut_ptr().cast(),
                value.len(),
            )
        } != size
        {
            return Err(ManagementError::Conflict(
                "SSH key file attributes changed. Refresh and retry.".into(),
            ));
        }
        if unsafe {
            libc::fsetxattr(
                destination.as_raw_fd(),
                name.as_ptr(),
                value.as_ptr().cast(),
                value.len(),
                0,
            )
        } != 0
        {
            return Err(ManagementError::Failed(
                "SSH key attributes could not be retained. Fix the file attributes and retry."
                    .into(),
            ));
        }
    }
    Ok(())
}
fn ssh_keys(
    command: &ManagementCommand,
    account: &UserAccount,
    interaction: &mut dyn OperationInteraction,
) -> Result<(), ManagementError> {
    let paths = ssh_key_paths(account)?;
    let add = if command.action == "user_ssh_add" {
        let text = value(command, "public_key").trim();
        if text.contains(['\r', '\n', '\0'])
            || !text.split_whitespace().next().is_some_and(|kind| {
                kind.starts_with("ssh-") || kind.starts_with("ecdsa-") || kind.starts_with("sk-")
            })
        {
            return Err(invalid(
                "Paste one SSH public key without authorization options.",
            ));
        }
        Some((text, fingerprint(text)?))
    } else {
        None
    };
    let remove = value(command, "fingerprint");
    if command.action == "user_ssh_remove" && !remove.starts_with("SHA256:") {
        return Err(invalid(
            "Choose a SHA256 key fingerprint from View SSH public keys.",
        ));
    }
    let mut removed = false;
    let mut existing = BTreeSet::new();
    let mut files = Vec::new();
    for path in &paths {
        let (directory, name) = match key_directory(path, account, command.action == "user_ssh_add")
        {
            Err(ManagementError::Failed(_))
                if !path.exists() && command.action != "user_ssh_add" =>
            {
                continue;
            }
            result => result?,
        };
        let (text, identity) = read_key_file(&directory, &name)?;
        let mut kept = Vec::new();
        let mut changed = false;
        for line in text.lines() {
            if !line.trim().is_empty()
                && !line.trim_start().starts_with('#')
                && let Ok(key) = fingerprint(line)
            {
                existing.insert(key.clone());
                if command.action == "user_ssh_keys" {
                    interaction.emit(OperationEvent::Output {
                        text: format!("{}  {}", key, path.display()),
                    });
                }
                if command.action == "user_ssh_remove" && key == remove {
                    removed = true;
                    changed = true;
                    continue;
                }
            }
            kept.push(line);
        }
        let content = if !changed {
            text.clone()
        } else if kept.is_empty() {
            String::new()
        } else {
            format!("{}\n", kept.join("\n"))
        };
        files.push((directory, name, text, identity, content));
    }
    if let Some((text, key)) = add {
        if existing.contains(&key) {
            return Err(ManagementError::Conflict(
                "This key already exists. View the existing keys.".into(),
            ));
        }
        let (directory, name, old, identity, _) = files
            .into_iter()
            .next()
            .ok_or_else(|| invalid("No writable SSH key file."))?;
        let content = format!(
            "{}{}{}\n",
            old,
            if old.is_empty() || old.ends_with('\n') {
                ""
            } else {
                "\n"
            },
            text
        );
        write_key_file(&directory, &name, &old, identity, &content, account)?;
        interaction.emit(OperationEvent::Output {
            text: format!("Added {key}"),
        });
    } else if command.action == "user_ssh_remove" {
        if !removed {
            return Err(ManagementError::Conflict(
                "The key was not found. View keys and retry.".into(),
            ));
        }
        for (directory, name, old, identity, content) in files {
            if old != content {
                write_key_file(&directory, &name, &old, identity, &content, account)?;
            }
        }
    } else if existing.is_empty() {
        interaction.emit(OperationEvent::Output {
            text: "No SSH public keys found".into(),
        });
    }
    Ok(())
}

#[cfg(test)]
#[path = "../../tests/unit/management/users_tests.rs"]
mod tests;
