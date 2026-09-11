//! NSS supplies identity, PAM supplies authentication and account policy.
//! UX records are preferences only: their credentials and roles are never trusted.

use crate::time::unix_millis;
use crate::{AuthSession, CoreError, UserRole};
use std::process::Command;
use storage::{StorageManager, UserRecord};

fn system_error(message: impl Into<String>) -> CoreError {
    CoreError::SystemIdentity(message.into())
}

fn uid_range(definitions: &str) -> (u32, u32) {
    let mut minimum = 1000;
    let mut maximum = 60000;
    for line in definitions.lines() {
        let mut fields = line
            .split('#')
            .next()
            .unwrap_or_default()
            .split_whitespace();
        let (Some(key), Some(value)) = (fields.next(), fields.next()) else {
            continue;
        };
        let Ok(value) = value.parse() else { continue };
        match key {
            "UID_MIN" => minimum = value,
            "UID_MAX" => maximum = value,
            _ => {}
        }
    }
    if minimum > maximum {
        (1000, 60000)
    } else {
        (minimum, maximum)
    }
}

fn parse_users(passwd: &str, range: (u32, u32)) -> Vec<UserRecord> {
    let mut users = Vec::new();
    for line in passwd.lines() {
        let fields: Vec<_> = line.split(':').collect();
        if fields.len() != 7 {
            continue;
        }
        let Ok(uid) = fields[2].parse::<u32>() else {
            continue;
        };
        if uid == 0 || !(range.0..=range.1).contains(&uid) {
            continue;
        }
        let username = fields[0];
        let shell = fields[6];
        if username.is_empty()
            || username.starts_with(['-', '+'])
            || username
                .chars()
                .any(|c| c.is_control() || c.is_whitespace())
            || (!shell.is_empty() && !shell.starts_with('/'))
            || matches!(
                shell.rsplit('/').next(),
                Some("nologin" | "false" | "sync" | "shutdown" | "halt")
            )
        {
            continue;
        }
        if users.iter().any(|user: &UserRecord| {
            user.id == format!("linux-uid-{uid}") || user.username == username
        }) {
            continue;
        }
        let display_name: String = fields[4]
            .split(',')
            .next()
            .unwrap_or(username)
            .chars()
            .filter(|c| !c.is_control())
            .collect();
        users.push(UserRecord {
            id: format!("linux-uid-{uid}"),
            username: username.into(),
            display_name: if display_name.is_empty() {
                username.into()
            } else {
                display_name
            },
            // Application presentation only; privileged service rechecks authorization.
            role: UserRole::User.as_str().into(),
            password_hash: String::new(),
            password_hint: None,
            appearance: Default::default(),
            personalization_pending: true,
            system_status_dashboard: storage::SystemStatusDashboardConfig::for_role(
                UserRole::User.as_str(),
            ),
            enabled: true,
            failed_login_attempts: 0,
            locked_until_epoch_ms: None,
            created_at_epoch_ms: 0,
            updated_at_epoch_ms: 0,
            last_login_at_epoch_ms: None,
        });
    }
    users.sort_by(|left, right| left.username.cmp(&right.username));
    users
}

fn attach_preferences(users: &mut [UserRecord], saved: &[UserRecord]) {
    for user in users {
        if let Some(profile) = saved
            .iter()
            .find(|profile| profile.id == user.id && profile.username == user.username)
        {
            user.appearance = profile.appearance.clone();
            user.personalization_pending = profile.personalization_pending;
            user.system_status_dashboard = profile.system_status_dashboard.clone();
            user.created_at_epoch_ms = profile.created_at_epoch_ms;
            user.updated_at_epoch_ms = profile.updated_at_epoch_ms;
            user.last_login_at_epoch_ms = profile.last_login_at_epoch_ms;
        }
    }
}

pub(super) fn users(storage: &StorageManager) -> Result<Vec<UserRecord>, CoreError> {
    // getent uses NSS (including configured directory services), without sharing
    // libc's global passwd iterator with other runtime threads.
    let output = Command::new("/usr/bin/getent")
        .arg("passwd")
        .output()
        .map_err(|error| system_error(format!("Cannot read Linux users with getent: {error}")))?;
    if !output.status.success() {
        return Err(system_error("Cannot enumerate Linux users through NSS"));
    }
    let passwd = std::str::from_utf8(&output.stdout)
        .map_err(|_| system_error("Linux account names are not valid UTF-8"))?;
    let definitions = std::fs::read_to_string("/etc/login.defs").unwrap_or_default();
    let mut users = parse_users(passwd, uid_range(&definitions));
    attach_preferences(&mut users, &storage.load_users()?.users);
    Ok(users)
}

pub(super) fn attach(storage: &StorageManager) -> Result<AuthSession, CoreError> {
    let user = session_protocol::linux::current_user().map_err(|e| system_error(e.to_string()))?;
    let records = users(storage)?;
    let mut record = records
        .into_iter()
        .find(|r| r.id == format!("linux-uid-{}", user.uid) && r.username == user.username)
        .ok_or(CoreError::UserNotFound)?;
    // No fabricated logind identity when running as an ordinary standalone application.
    let system_session = session_protocol::linux::current_session().ok();
    let session_id = system_session
        .map(|s| s.identity.logind_session_id)
        .unwrap_or_default();
    record.role = if session_protocol::linux::account_in_group(&user, "tundra-admin")
        .map_err(|e| system_error(e.to_string()))?
    {
        "Admin"
    } else {
        "User"
    }
    .into();
    let now = unix_millis();
    if record.created_at_epoch_ms == 0 {
        record.created_at_epoch_ms = now;
    }
    record.updated_at_epoch_ms = now;
    record.last_login_at_epoch_ms = Some(now);
    let session = AuthSession {
        system_user: Some(user),
        session_id,
        user_id: record.id.clone(),
        username: record.username.clone(),
        role: UserRole::from_storage(&record.role),
        started_at_epoch_ms: now,
    };
    let mut document = storage.load_users()?;
    document.users.retain(|r| r.id != record.id);
    document.users.push(record);
    storage.save_users(&document)?;
    Ok(session)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn system_accounts_exclude_root_and_never_infer_admin() {
        let records = parse_users(
            "root:x:0:0:root:/root:/bin/bash\nalice:x:1000:1000:Alice:/home/alice:/bin/bash\nservice:x:1001:1001::/:/usr/sbin/nologin",
            (1000, 60000),
        );
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].id, "linux-uid-1000");
        assert_eq!(records[0].role, "User");
        assert!(records[0].password_hash.is_empty());
    }
    #[test]
    fn stored_preferences_cannot_change_system_authority() {
        let mut users = parse_users(
            "alice:x:1000:1000:Alice:/home/alice:/bin/bash",
            (1000, 60000),
        );
        let mut forged = users[0].clone();
        forged.role = "Admin".into();
        forged.password_hash = "secret".into();
        forged.enabled = false;
        forged.display_name = "forged".into();
        forged.personalization_pending = false;
        attach_preferences(&mut users, &[forged]);
        assert_eq!(users[0].role, "User");
        assert!(users[0].password_hash.is_empty());
        assert!(users[0].enabled);
        assert_eq!(users[0].display_name, "Alice");
        assert!(!users[0].personalization_pending);
    }
}
