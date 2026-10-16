//! Account recovery changes selected entries with the native account tools.
use super::*;
use serde_json::Value;

type Database = BTreeMap<String, Vec<String>>;
pub(super) type Snapshot = BTreeMap<String, Database>;

#[derive(Clone, Serialize, Deserialize)]
struct Change {
    database: String,
    name: String,
    before: Option<Vec<String>>,
    after: Option<Vec<String>>,
    fields: Vec<usize>,
    member: Option<String>,
}
#[derive(Clone, Serialize, Deserialize)]
struct Identity {
    before: Option<u32>,
    after: Option<u32>,
}
#[derive(Clone, Serialize, Deserialize)]
struct AccountBackup {
    action: String,
    target: String,
    changes: Vec<Change>,
    users: BTreeMap<String, Identity>,
    groups: BTreeMap<String, Identity>,
}

pub(crate) fn describe_backup(payload: &Value) -> String {
    let Ok(backup) = serde_json::from_value::<AccountBackup>(payload.clone()) else {
        return i18n::tr!("platform-account-recovery-unavailable").into();
    };
    let mut text = String::new();
    for change in &backup.changes {
        text.push_str(&format!("{} ({})\n", change.name, change.database));
        if change.before.is_none() {
            text.push_str(&format!(
                "{}\n",
                i18n::tr!("platform-account-recovery-created")
            ));
        } else if change.after.is_none() {
            text.push_str(&format!(
                "{}\n",
                i18n::tr!("platform-account-recovery-deleted")
            ));
        } else {
            let old = change.before.as_ref().unwrap();
            let new = change.after.as_ref().unwrap();
            let fields = if change.fields.is_empty() {
                (1..old.len().min(new.len())).collect()
            } else {
                change.fields.clone()
            };
            for index in fields {
                if old.get(index) == new.get(index) {
                    continue;
                }
                if matches!(change.database.as_str(), "shadow" | "gshadow") && index == 1 {
                    text.push_str(&format!(
                        "{}\n",
                        i18n::tr!("platform-account-recovery-password-changed")
                    ));
                    continue;
                }
                if let Some(member) = &change.member {
                    text.push_str(&i18n::tr!(
                        "platform-account-recovery-member-change",
                        member = member.as_str(),
                        before = self::member(old, index, member).to_string(),
                        after = self::member(new, index, member).to_string()
                    ));
                    text.push('\n');
                } else {
                    let field = match (change.database.as_str(), index) {
                        ("passwd", 2) => i18n::tr!("platform-account-recovery-uid"),
                        ("passwd", 3) => i18n::tr!("platform-account-recovery-primary-group"),
                        ("passwd", 4) => i18n::tr!("platform-account-recovery-display-name"),
                        ("passwd", 5) => i18n::tr!("platform-account-recovery-home"),
                        ("passwd", 6) => i18n::tr!("platform-account-recovery-shell"),
                        ("shadow", 2) => i18n::tr!("platform-account-recovery-password-date"),
                        ("shadow", 3) => i18n::tr!("platform-account-recovery-minimum-age"),
                        ("shadow", 4) => i18n::tr!("platform-account-recovery-maximum-age"),
                        ("shadow", 5) => i18n::tr!("platform-account-recovery-warning"),
                        ("shadow", 6) => i18n::tr!("platform-account-recovery-inactivity"),
                        ("shadow", 7) => i18n::tr!("platform-account-recovery-expiry"),
                        ("group", 2) => i18n::tr!("platform-account-recovery-gid"),
                        ("group" | "gshadow", 3) => i18n::tr!("platform-account-recovery-members"),
                        ("gshadow", 2) => i18n::tr!("platform-account-recovery-administrators"),
                        ("subuid", i) if i % 3 == 1 => {
                            i18n::tr!("platform-account-recovery-first-subuid")
                        }
                        ("subgid", i) if i % 3 == 1 => {
                            i18n::tr!("platform-account-recovery-first-subgid")
                        }
                        ("subuid" | "subgid", i) if i % 3 == 2 => {
                            i18n::tr!("platform-account-recovery-sub-count")
                        }
                        _ => i18n::tr!("platform-account-recovery-field"),
                    };
                    text.push_str(&format!(
                        "{field}: {} -> {}\n",
                        old.get(index).map(String::as_str).unwrap_or(""),
                        new.get(index).map(String::as_str).unwrap_or("")
                    ));
                }
            }
        }
    }
    text
}

pub(super) fn capture() -> Result<Snapshot, ManagementError> {
    let mut snapshot = Snapshot::new();
    for file in ["passwd", "shadow", "group", "gshadow", "subuid", "subgid"] {
        let path = Path::new("/etc").join(file);
        let text = match fs::read_to_string(&path) {
            Ok(v) => v,
            Err(e)
                if e.kind() == std::io::ErrorKind::NotFound
                    && matches!(file, "subuid" | "subgid" | "gshadow") =>
            {
                String::new()
            }
            Err(e) => return Err(io_error(e)),
        };
        let mut entries = Database::new();
        for line in text
            .lines()
            .filter(|line| !line.is_empty() && !line.starts_with('#'))
        {
            let fields = line.split(':').map(String::from).collect::<Vec<_>>();
            if matches!(file, "subuid" | "subgid") {
                entries.entry(fields[0].clone()).or_default().extend(fields);
            } else {
                entries.insert(fields[0].clone(), fields);
            }
        }
        snapshot.insert(file.into(), entries);
    }
    Ok(snapshot)
}

fn affected(
    command: &ManagementCommand,
    database: &str,
    name: &str,
    before: Option<&Vec<String>>,
    after: Option<&Vec<String>>,
) -> Option<(Vec<usize>, Option<String>)> {
    let target = command
        .target
        .as_deref()
        .unwrap_or("")
        .trim_start_matches("group:");
    let user = if command.action == "user_create" {
        value(command, "username")
    } else {
        target
    };
    let group = if command.action == "group_create" {
        value(command, "name")
    } else {
        target
    };
    if command.action.starts_with("group_") {
        if !matches!(database, "group" | "gshadow")
            || name != group
                && !(command.action == "group_rename" && name == value(command, "name"))
        {
            return None;
        }
        return Some((
            if command.action == "group_members" {
                vec![3]
            } else {
                vec![]
            },
            None,
        ));
    }
    if matches!(database, "group" | "gshadow") {
        if matches!(command.action.as_str(), "user_create" | "user_delete")
            && name == user
            && (before.is_none() || after.is_none())
        {
            return Some((vec![], None));
        }
        if matches!(command.action.as_str(), "user_groups" | "user_delete") {
            let contains = |row: Option<&Vec<String>>| {
                row.and_then(|r| r.get(3))
                    .is_some_and(|s| s.split(',').any(|n| n == user))
            };
            if contains(before) != contains(after) {
                return Some((vec![3], Some(user.into())));
            }
        }
        return None;
    }
    if name != user {
        return None;
    }
    if matches!(command.action.as_str(), "user_create" | "user_delete") {
        return Some((vec![], None));
    }
    let fields = match (command.action.as_str(), database) {
        ("user_info", "passwd") => vec![4],
        ("user_shell", "passwd") => vec![6],
        ("user_primary_group", "passwd") => vec![3],
        ("user_password", "shadow") => vec![1, 2],
        ("user_lock" | "user_unlock", "shadow") => vec![1],
        ("user_expiry", "shadow") => vec![7],
        _ => return None,
    };
    Some((fields, None))
}
fn account_backup(
    action: String,
    target: String,
    changes: Vec<Change>,
    before: &Snapshot,
    after: &Snapshot,
) -> Result<AccountBackup, ManagementError> {
    let mut user_names = BTreeSet::new();
    let mut group_names = BTreeSet::new();
    for change in &changes {
        if matches!(change.database.as_str(), "group" | "gshadow") {
            group_names.insert(change.name.clone());
            if let Some(member) = &change.member {
                user_names.insert(member.clone());
            } else {
                let fields: &[usize] = if change.database == "gshadow" && change.fields.is_empty() {
                    &[2, 3]
                } else if change.fields.is_empty() || change.fields.contains(&3) {
                    &[3]
                } else {
                    &[]
                };
                for row in change.before.iter().chain(&change.after) {
                    for index in fields {
                        if let Some(names) = row.get(*index) {
                            user_names.extend(
                                names.split(',').filter(|n| !n.is_empty()).map(String::from),
                            );
                        }
                    }
                }
            }
        } else {
            user_names.insert(change.name.clone());
        }
        if change.database == "passwd" && (change.fields.is_empty() || change.fields.contains(&3)) {
            for row in change.before.iter().chain(&change.after) {
                let gid = number(row, 3)?;
                for snapshot in [before, after] {
                    if let Some(groups) = snapshot.get("group") {
                        for (name, group) in groups {
                            if number(group, 2)? == gid {
                                group_names.insert(name.clone());
                            }
                        }
                    }
                }
            }
        }
    }
    let identities = |database: &str, names: BTreeSet<String>| {
        names
            .into_iter()
            .map(|name| {
                Ok((
                    name.clone(),
                    Identity {
                        before: identity_number(before, database, &name)?,
                        after: identity_number(after, database, &name)?,
                    },
                ))
            })
            .collect::<Result<BTreeMap<_, _>, ManagementError>>()
    };
    Ok(AccountBackup {
        action,
        target,
        changes,
        users: identities("passwd", user_names)?,
        groups: identities("group", group_names)?,
    })
}

fn identity_number(
    snapshot: &Snapshot,
    database: &str,
    name: &str,
) -> Result<Option<u32>, ManagementError> {
    snapshot
        .get(database)
        .and_then(|entries| entries.get(name))
        .map(|row| number(row, 2))
        .transpose()
}

pub(super) fn payload(
    command: &ManagementCommand,
    before: &Snapshot,
    after: &Snapshot,
) -> Result<Value, ManagementError> {
    let mut changes = Vec::new();
    for database in before.keys().chain(after.keys()).collect::<BTreeSet<_>>() {
        let old = before.get(database).cloned().unwrap_or_default();
        let new = after.get(database).cloned().unwrap_or_default();
        for name in old.keys().chain(new.keys()).collect::<BTreeSet<_>>() {
            let a = old.get(name);
            let b = new.get(name);
            if a == b {
                continue;
            }
            if let Some((fields, member)) = affected(command, database, name, a, b) {
                changes.push(Change {
                    database: database.clone(),
                    name: name.clone(),
                    before: a.cloned(),
                    after: b.cloned(),
                    fields,
                    member,
                });
            }
        }
    }
    serde_json::to_value(account_backup(
        command.action.clone(),
        command.target.clone().unwrap_or_else(|| {
            value(
                command,
                if command.action == "user_create" {
                    "username"
                } else {
                    "name"
                },
            )
            .into()
        }),
        changes,
        before,
        after,
    )?)
    .map_err(|e| ManagementError::Failed(e.to_string()))
}
pub(super) fn prepared_payload(
    command: &ManagementCommand,
    before: &Snapshot,
) -> Result<Value, ManagementError> {
    let target = if command.action == "user_create" {
        value(command, "username")
    } else if command.action == "group_create" {
        value(command, "name")
    } else {
        command
            .target
            .as_deref()
            .unwrap_or("")
            .trim_start_matches("group:")
    };
    let mut changes = Vec::new();
    for (database, entries) in before {
        for (name, row) in entries {
            let membership = matches!(database.as_str(), "group" | "gshadow")
                && matches!(command.action.as_str(), "user_groups" | "user_delete")
                && (member(row, 3, target)
                    || value(command, "groups").split(',').any(|g| g == name));
            if let Some((fields, member_name)) =
                affected(command, database, name, Some(row), Some(row))
                    .or_else(|| membership.then(|| (vec![3], Some(target.into()))))
            {
                changes.push(Change {
                    database: database.clone(),
                    name: name.clone(),
                    before: Some(row.clone()),
                    after: Some(row.clone()),
                    fields,
                    member: member_name,
                });
            }
        }
        if !entries.contains_key(target)
            && matches!(command.action.as_str(), "user_create" | "group_create")
        {
            changes.push(Change {
                database: database.clone(),
                name: target.into(),
                before: None,
                after: None,
                fields: vec![],
                member: None,
            });
        }
    }
    serde_json::to_value(account_backup(
        command.action.clone(),
        target.into(),
        changes,
        before,
        before,
    )?)
    .map_err(|e| ManagementError::Failed(e.to_string()))
}

fn member(row: &Vec<String>, index: usize, name: &str) -> bool {
    row.get(index)
        .is_some_and(|s| s.split(',').any(|n| n == name))
}
fn expected(change: &Change, actual: Option<&Vec<String>>, after: bool) -> bool {
    let wanted = if after {
        change.after.as_ref()
    } else {
        change.before.as_ref()
    };
    match (wanted, actual) {
        (None, None) => true,
        (Some(w), Some(a)) if !change.fields.is_empty() => {
            a.first() == w.first()
                && change.fields.iter().all(|i| {
                    if let Some(m) = &change.member {
                        member(w, *i, m) == member(a, *i, m)
                    } else {
                        w.get(*i) == a.get(*i)
                    }
                })
        }
        _ => wanted == actual,
    }
}
fn check_current(backup: &AccountBackup, current: &Snapshot) -> Result<(), ManagementError> {
    check_identities(backup, current, true)?;
    for change in &backup.changes {
        if !expected(
            change,
            current
                .get(&change.database)
                .and_then(|db| db.get(&change.name)),
            true,
        ) {
            return Err(ManagementError::Conflict(i18n::tr!(
                "platform-account-recovery-changed",
                name = change.name.as_str()
            )));
        }
    }
    Ok(())
}
fn check_identities(
    backup: &AccountBackup,
    current: &Snapshot,
    after: bool,
) -> Result<(), ManagementError> {
    for (database, identities) in [("passwd", &backup.users), ("group", &backup.groups)] {
        for (name, identity) in identities {
            let wanted = if after {
                identity.after
            } else {
                identity.before
            };
            if identity_number(current, database, name)? != wanted {
                return Err(ManagementError::Conflict(i18n::tr!(
                    "platform-account-recovery-identity-changed",
                    name = name.as_str()
                )));
            }
        }
    }
    Ok(())
}
fn number(row: &[String], index: usize) -> Result<u32, ManagementError> {
    row.get(index)
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| invalid(&i18n::tr!("platform-account-recovery-invalid")))
}
fn validate_collisions(
    backup: &AccountBackup,
    context: &ExecutionContext,
) -> Result<(), ManagementError> {
    let users = accounts(None)?;
    let all_groups = groups()?;
    let actor = users.iter().find(|u| u.uid == context.actor_uid);
    for (name, identity) in &backup.groups {
        if let Some(gid) = identity.before {
            let renamed = backup.action == "group_rename"
                && backup
                    .groups
                    .values()
                    .any(|other| other.before.is_none() && other.after == Some(gid));
            if !renamed && all_groups.iter().any(|g| g.gid == gid && g.name != *name) {
                return Err(ManagementError::Conflict(
                    i18n::tr!("platform-account-recovery-gid-occupied").into(),
                ));
            }
        }
    }
    for change in &backup.changes {
        if change.database == "shadow" && actor.is_some_and(|a| a.username == change.name) {
            if let Some(before) = &change.before {
                if (change.fields.is_empty() || change.fields.contains(&1))
                    && before
                        .get(1)
                        .is_some_and(|hash| hash.starts_with('!') || hash.starts_with('*'))
                {
                    return Err(ManagementError::PermissionDenied(
                        i18n::tr!("platform-account-recovery-keep-unlocked").into(),
                    ));
                }
                let today = chrono::Utc::now().timestamp().div_euclid(86400);
                if (change.fields.is_empty() || change.fields.contains(&7))
                    && before
                        .get(7)
                        .and_then(|d| d.parse::<i64>().ok())
                        .is_some_and(|d| d >= 0 && d <= today)
                {
                    return Err(ManagementError::PermissionDenied(
                        i18n::tr!("platform-account-recovery-keep-unexpired").into(),
                    ));
                }
            }
        }
        if change.database == "passwd" {
            if change.before.is_none()
                && change.after.as_ref().is_some_and(|r| {
                    r.get(2)
                        .is_some_and(|s| s == "0" || s == &context.actor_uid.to_string())
                })
            {
                return Err(ManagementError::PermissionDenied(
                    i18n::tr!("platform-account-recovery-keep-account").into(),
                ));
            }
            if let Some(before) = &change.before {
                let uid = number(before, 2)?;
                if users
                    .iter()
                    .any(|u| u.uid == uid && u.username != change.name)
                {
                    return Err(ManagementError::Conflict(
                        i18n::tr!("platform-account-recovery-uid-occupied").into(),
                    ));
                }
                if change.after.is_none() && users.iter().any(|u| u.username == change.name) {
                    return Err(ManagementError::Conflict(
                        i18n::tr!("platform-account-recovery-name-occupied").into(),
                    ));
                }
                if uid == context.actor_uid && change.fields.contains(&3) {
                    let current_admin = actor.is_some_and(|a| {
                        all_groups
                            .iter()
                            .any(|g| g.gid == a.gid && is_admin_group(&g.name))
                    });
                    if current_admin
                        && actor.is_some_and(|a| before.get(3) != Some(&a.gid.to_string()))
                    {
                        return Err(ManagementError::PermissionDenied(
                            i18n::tr!("platform-account-recovery-keep-primary").into(),
                        ));
                    }
                }
            }
        }
        if change.database == "group" && is_admin_group(&change.name) {
            if let Some(actor) = actor {
                let was = change
                    .before
                    .as_ref()
                    .is_some_and(|row| member(row, 3, &actor.username));
                let is = change
                    .after
                    .as_ref()
                    .is_some_and(|row| member(row, 3, &actor.username));
                if is && !was {
                    return Err(ManagementError::PermissionDenied(
                        i18n::tr!("platform-account-recovery-keep-administrator").into(),
                    ));
                }
            }
        }
    }
    Ok(())
}
fn shadow_restore(name: &str, row: &[String], fields: &[usize]) -> Result<(), ManagementError> {
    let preserved_lastday = if fields.contains(&1) && !fields.contains(&2) {
        Some(
            capture()?["shadow"]
                .get(name)
                .and_then(|row| row.get(2))
                .cloned()
                .ok_or_else(|| invalid(&i18n::tr!("platform-account-recovery-date-unavailable")))?,
        )
    } else {
        None
    };
    if fields.is_empty() || fields.contains(&1) {
        let hash = row
            .get(1)
            .ok_or_else(|| invalid(&i18n::tr!("platform-account-recovery-invalid-password")))?;
        let input = Zeroizing::new(format!("{name}:{hash}\n"));
        native("chpasswd", &["--encrypted"], Some(input.as_bytes()))?;
    }
    for (index, option) in [
        (2, "--lastday"),
        (3, "--mindays"),
        (4, "--maxdays"),
        (5, "--warndays"),
        (6, "--inactive"),
        (7, "--expiredate"),
    ] {
        if fields.is_empty() || fields.contains(&index) {
            let v = row
                .get(index)
                .filter(|s| !s.is_empty())
                .map(String::as_str)
                .unwrap_or("-1");
            native("chage", &[option, v, "--", name], None)?;
        }
    }
    if let Some(lastday) = preserved_lastday {
        native(
            "chage",
            &[
                "--lastday",
                if lastday.is_empty() { "-1" } else { &lastday },
                "--",
                name,
            ],
            None,
        )?;
    }
    Ok(())
}
fn restore_entries(
    backup: &AccountBackup,
    context: &ExecutionContext,
    flag: &AtomicBool,
) -> Result<(), ManagementError> {
    validate_collisions(backup, context)?;
    if backup.action == "group_rename" {
        let original = backup
            .changes
            .iter()
            .find(|c| c.database == "group" && c.before.is_some() && c.after.is_none());
        let renamed = backup
            .changes
            .iter()
            .find(|c| c.database == "group" && c.before.is_none() && c.after.is_some());
        if let (Some(original), Some(renamed)) = (original, renamed) {
            native(
                "groupmod",
                &["--new-name", &original.name, "--", &renamed.name],
                None,
            )?;
        }
    }
    for change in backup.changes.iter().filter(|c| {
        c.database == "group"
            && c.before.is_some()
            && c.after.is_none()
            && backup.action != "group_rename"
    }) {
        cancelled(flag)?;
        let row = change.before.as_ref().unwrap();
        native(
            "groupadd",
            &["--gid", &number(row, 2)?.to_string(), "--", &change.name],
            None,
        )?;
    }
    for change in backup.changes.iter().filter(|c| c.database == "passwd") {
        cancelled(flag)?;
        match (&change.before, &change.after) {
            (None, Some(_)) => {
                native("userdel", &["--", &change.name], None)?;
            }
            (Some(row), None) => {
                native(
                    "useradd",
                    &[
                        "--no-create-home",
                        "--no-user-group",
                        "-K",
                        "SUB_UID_COUNT=0",
                        "-K",
                        "SUB_GID_COUNT=0",
                        "--uid",
                        &number(row, 2)?.to_string(),
                        "--gid",
                        &number(row, 3)?.to_string(),
                        "--home-dir",
                        &row[5],
                        "--shell",
                        &row[6],
                        "--comment",
                        &row[4],
                        "--",
                        &change.name,
                    ],
                    None,
                )?;
            }
            (Some(row), Some(_)) => {
                for i in &change.fields {
                    let option = match i {
                        3 => "--gid",
                        4 => "--comment",
                        6 => "--shell",
                        _ => {
                            return Err(invalid(&i18n::tr!(
                                "platform-account-recovery-unsupported-field"
                            )));
                        }
                    };
                    native("usermod", &[option, &row[*i], "--", &change.name], None)?;
                }
            }
            _ => {}
        }
    }
    for change in backup
        .changes
        .iter()
        .filter(|c| c.database == "shadow" && c.before.is_some())
    {
        cancelled(flag)?;
        shadow_restore(
            &change.name,
            change.before.as_ref().unwrap(),
            &change.fields,
        )?;
    }
    for change in backup
        .changes
        .iter()
        .filter(|c| c.database == "group" && c.before.is_some())
    {
        cancelled(flag)?;
        let before = change.before.as_ref().unwrap();
        if let Some(member_name) = &change.member {
            let add = member(before, 3, member_name);
            let mut actor = accounts(Some(member_name))?
                .into_iter()
                .find(|a| a.username == *member_name)
                .ok_or_else(|| invalid(&i18n::tr!("platform-account-recovery-missing-member")))?;
            complete_account_groups(&mut actor);
            if !add && actor.uid == context.actor_uid && is_admin_group(&change.name) {
                return Err(ManagementError::PermissionDenied(
                    i18n::tr!("platform-account-recovery-keep-administrator").into(),
                ));
            }
            native(
                "gpasswd",
                &[
                    if add { "--add" } else { "--delete" },
                    member_name,
                    &change.name,
                ],
                None,
            )?;
        } else if change.fields.contains(&3) || change.after.is_none() {
            let current_actor = accounts(Some(&context.actor_uid.to_string()))?
                .into_iter()
                .next();
            if is_admin_group(&change.name)
                && current_actor.as_ref().is_some_and(|a| {
                    !before[3].split(',').any(|m| m == a.username)
                        && a.groups.contains(&change.name)
                })
            {
                return Err(ManagementError::PermissionDenied(
                    i18n::tr!("platform-account-recovery-keep-administrator").into(),
                ));
            }
            native("gpasswd", &["--members", &before[3], &change.name], None)?;
        }
    }
    for change in backup
        .changes
        .iter()
        .filter(|c| c.database == "gshadow" && c.before.is_some() && c.fields.is_empty())
    {
        let row = change.before.as_ref().unwrap();
        let input = Zeroizing::new(format!("{}:{}\n", change.name, row[1]));
        native("chgpasswd", &["--encrypted"], Some(input.as_bytes()))?;
        native(
            "gpasswd",
            &["--administrators", &row[2], &change.name],
            None,
        )?;
    }
    for change in backup
        .changes
        .iter()
        .filter(|c| c.database == "subuid" || c.database == "subgid")
    {
        cancelled(flag)?;
        if backup
            .changes
            .iter()
            .any(|c| c.database == "passwd" && c.name == change.name && c.before.is_none())
        {
            continue;
        }
        let (del, add) = if change.database == "subuid" {
            ("--del-subuids", "--add-subuids")
        } else {
            ("--del-subgids", "--add-subgids")
        };
        if let Some(rows) = &change.after {
            for row in rows.chunks_exact(3) {
                let start = number(row, 1)?;
                let count = number(row, 2)?;
                native(
                    "usermod",
                    &[
                        del,
                        &format!(
                            "{start}-{}",
                            start
                                .checked_add(count)
                                .and_then(|v| v.checked_sub(1))
                                .ok_or_else(|| invalid(&i18n::tr!(
                                    "platform-account-recovery-invalid-range"
                                )))?
                        ),
                        "--",
                        &change.name,
                    ],
                    None,
                )?;
            }
        }
        if let Some(rows) = &change.before {
            for row in rows.chunks_exact(3) {
                let start = number(row, 1)?;
                let count = number(row, 2)?;
                native(
                    "usermod",
                    &[
                        add,
                        &format!(
                            "{start}-{}",
                            start
                                .checked_add(count)
                                .and_then(|v| v.checked_sub(1))
                                .ok_or_else(|| invalid(&i18n::tr!(
                                    "platform-account-recovery-invalid-range"
                                )))?
                        ),
                        "--",
                        &change.name,
                    ],
                    None,
                )?;
            }
        }
    }
    for change in backup.changes.iter().filter(|c| {
        c.database == "group"
            && c.before.is_none()
            && c.after.is_some()
            && backup.action != "group_rename"
    }) {
        cancelled(flag)?;
        if groups()?.iter().any(|g| g.name == change.name) {
            native("groupdel", &["--", &change.name], None)?;
        }
    }
    Ok(())
}
pub(crate) fn restore_backup(
    payload: &Value,
    context: &ExecutionContext,
    io: &mut dyn OperationInteraction,
    flag: &AtomicBool,
) -> Result<String, ManagementError> {
    if unsafe { libc::geteuid() } != 0 {
        return Err(ManagementError::PermissionDenied(
            i18n::tr!("platform-account-recovery-authorization").into(),
        ));
    }
    let backup: AccountBackup = serde_json::from_value(payload.clone())
        .map_err(|_| invalid(&i18n::tr!("platform-account-recovery-invalid")))?;
    let before = capture()?;
    check_current(&backup, &before)?;
    validate_collisions(&backup, context)?;
    let session = super::super::config_backups::begin("users", &[])?;
    let mut inverse = backup.clone();
    for c in &mut inverse.changes {
        std::mem::swap(&mut c.before, &mut c.after);
    }
    for identity in inverse
        .users
        .values_mut()
        .chain(inverse.groups.values_mut())
    {
        std::mem::swap(&mut identity.before, &mut identity.after);
    }
    session.set_payload(
        serde_json::to_value(&inverse).map_err(|e| ManagementError::Failed(e.to_string()))?,
    )?;
    let outcome = restore_entries(&backup, context, flag);
    let actual = capture()?;
    for change in &mut inverse.changes {
        change.before = before
            .get(&change.database)
            .and_then(|db| db.get(&change.name))
            .cloned();
        change.after = actual
            .get(&change.database)
            .and_then(|db| db.get(&change.name))
            .cloned();
    }
    inverse = account_backup(
        inverse.action,
        inverse.target,
        inverse.changes,
        &before,
        &actual,
    )?;
    session.set_payload(
        serde_json::to_value(&inverse).map_err(|e| ManagementError::Failed(e.to_string()))?,
    )?;
    let verified = check_identities(&backup, &actual, false).is_ok()
        && backup.changes.iter().all(|c| {
            expected(
                c,
                actual.get(&c.database).and_then(|db| db.get(&c.name)),
                false,
            )
        });
    let checks = if outcome.is_ok() && verified {
        Some((
            native("pwck", &["--read-only"], None).is_ok(),
            native("grpck", &["--read-only"], None).is_ok(),
        ))
    } else {
        None
    };
    let result = if let Some((passwd, group)) = checks {
        i18n::tr!(
            "platform-account-recovery-check-results",
            passwd = i18n::tr!(if passwd {
                "platform-account-recovery-passed"
            } else {
                "platform-account-recovery-failed"
            }),
            group = i18n::tr!(if group {
                "platform-account-recovery-passed"
            } else {
                "platform-account-recovery-failed"
            })
        )
    } else {
        i18n::tr!("platform-account-recovery-partial").into()
    };
    session.finish(&result)?;
    outcome?;
    if !verified {
        return Err(ManagementError::Failed(
            i18n::tr!("platform-account-recovery-mismatch").into(),
        ));
    }
    if checks != Some((true, true)) {
        return Err(ManagementError::Failed(
            i18n::tr!("platform-account-recovery-check-failed").into(),
        ));
    }
    io.emit(OperationEvent::Output {
        text: i18n::tr!("platform-account-recovery-content-restored").into(),
    });
    Ok(i18n::tr!("platform-account-recovery-verified").into())
}

#[cfg(test)]
#[path = "../../../tests/unit/management/users_recovery_tests.rs"]
mod tests;
