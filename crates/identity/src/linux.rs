//! NSS supplies identity, PAM supplies authentication and account policy.
//! UX records are preferences only: their credentials and roles are never trusted.

mod pam;

use crate::time::{unix_millis, unix_nanos};
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
        if uid != 0 && !(range.0..=range.1).contains(&uid) {
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
            // The Linux shell deliberately runs as root for every authenticated user.
            role: UserRole::Admin.as_str().into(),
            password_hash: String::new(),
            password_hint: None,
            appearance: Default::default(),
            personalization_pending: true,
            system_status_dashboard: storage::SystemStatusDashboardConfig::for_role(
                UserRole::Admin.as_str(),
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

pub(super) fn login(
    storage: &StorageManager,
    username: &str,
    password: &str,
) -> Result<AuthSession, CoreError> {
    // Check real process authority; a UX Admin label is not OS elevation.
    if unsafe { libc::geteuid() } != 0 {
        return Err(system_error(
            "Linux mode requires root. Start tundra-shell with sudo.",
        ));
    }
    login_with(
        storage,
        username,
        password,
        || users(storage),
        pam::authenticate,
    )
}

fn login_with(
    storage: &StorageManager,
    username: &str,
    password: &str,
    mut enumerate: impl FnMut() -> Result<Vec<UserRecord>, CoreError>,
    authenticate: impl FnOnce(&str, &str) -> Result<(), CoreError>,
) -> Result<AuthSession, CoreError> {
    let record = enumerate()?
        .into_iter()
        .find(|user| user.username == username)
        .ok_or(CoreError::InvalidCredentials)?;
    authenticate(username, password)?;
    // Re-read after PAM to reject accounts removed/renamed during authentication.
    let mut current = enumerate()?
        .into_iter()
        .find(|user| user.id == record.id && user.username == username)
        .ok_or(CoreError::InvalidCredentials)?;
    let now = unix_millis();
    if current.created_at_epoch_ms == 0 {
        current.created_at_epoch_ms = now;
    }
    current.updated_at_epoch_ms = now;
    current.last_login_at_epoch_ms = Some(now);
    let session = AuthSession {
        session_id: format!("session-{}-{}", current.id, unix_nanos()),
        user_id: current.id.clone(),
        username: current.username.clone(),
        role: UserRole::Admin,
        started_at_epoch_ms: now,
    };
    let mut document = storage.load_users()?;
    document.users.retain(|user| user.id != current.id);
    document.users.push(current);
    storage.save_users(&document)?;
    Ok(session)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> (std::path::PathBuf, StorageManager) {
        let path = std::env::temp_dir().join(format!(
            "tundra-linux-auth-{}-{}",
            std::process::id(),
            unix_nanos()
        ));
        let paths = platform::AppPaths::from_parts(
            path.join("config.toml"),
            path.join("data"),
            path.join("cache"),
            path.join("logs"),
            path.join("tmp"),
        )
        .unwrap();
        let storage = StorageManager::open(paths).unwrap().manager;
        (path, storage)
    }

    #[test]
    fn authentication_controls_session_creation_and_never_stores_credentials() {
        let (path, storage) = fixture();
        let accounts = parse_users("alice:x:1000:1000::/home/alice:/bin/bash", (1000, 60000));
        let denied = login_with(
            &storage,
            "alice",
            "wrong",
            || Ok(accounts.clone()),
            |_, _| Err(CoreError::InvalidCredentials),
        );
        assert!(matches!(denied, Err(CoreError::InvalidCredentials)));
        assert!(storage.load_users().unwrap().users.is_empty());
        let accepted = login_with(
            &storage,
            "alice",
            "system-secret",
            || Ok(accounts.clone()),
            |name, password| {
                assert_eq!(name, "alice");
                assert_eq!(password, "system-secret");
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(accepted.user_id, "linux-uid-1000");
        assert_eq!(accepted.role, UserRole::Admin);
        let record = storage.load_users().unwrap().users.remove(0);
        assert!(record.personalization_pending);
        assert!(record.password_hash.is_empty());
        assert!(record.password_hint.is_none());
        assert!(record.last_login_at_epoch_ms.is_some());
        std::fs::remove_dir_all(path).unwrap();
    }

    #[test]
    fn linux_login_preserves_personalization_until_explicit_completion() {
        let (path, storage) = fixture();
        let enumerate = || {
            let mut accounts =
                parse_users("alice:x:1000:1000::/home/alice:/bin/bash", (1000, 60000));
            attach_preferences(&mut accounts, &storage.load_users()?.users);
            Ok(accounts)
        };
        let login = || login_with(&storage, "alice", "secret", enumerate, |_, _| Ok(()));
        let session = login().unwrap();
        assert!(storage.load_users().unwrap().users[0].personalization_pending);
        login().unwrap();
        assert!(storage.load_users().unwrap().users[0].personalization_pending);
        crate::UserService::new(storage.clone())
            .with_backend(crate::IdentityBackend::Linux)
            .complete_personalization(&session, storage::AppearanceConfig::default())
            .unwrap();
        login().unwrap();
        assert!(!storage.load_users().unwrap().users[0].personalization_pending);
        std::fs::remove_dir_all(path).unwrap();
    }

    #[test]
    fn renamed_or_removed_account_during_pam_cannot_create_session() {
        let (path, storage) = fixture();
        let mut calls = 0;
        let result = login_with(
            &storage,
            "alice",
            "password",
            || {
                calls += 1;
                Ok(parse_users(
                    if calls == 1 {
                        "alice:x:1000:1000::/:/bin/bash"
                    } else {
                        "bob:x:1000:1000::/:/bin/bash"
                    },
                    (1000, 60000),
                ))
            },
            |_, _| Ok(()),
        );
        assert!(matches!(result, Err(CoreError::InvalidCredentials)));
        assert!(storage.load_users().unwrap().users.is_empty());
        std::fs::remove_dir_all(path).unwrap();
    }

    #[test]
    fn enumerates_login_accounts_and_honors_uid_range() {
        let passwd = "root:x:0:0:root:/root:/bin/bash\ndaemon:x:1:1:daemon:/:/bin/bash\nAlice:x:1500:1500:Alice Smith,Room:/home/alice:/bin/bash\nservice:x:1501:1501::/:/usr/sbin/nologin\nlocked:x:1502:1502::/:/bin/false\nold:x:999:999::/:/bin/bash\nhigh:x:60001:60001::/:/bin/bash\nempty:x:1503:1503::/home/empty:\ninvalid\n";
        let users = parse_users(passwd, (1000, 60000));
        assert_eq!(
            users
                .iter()
                .map(|u| u.username.as_str())
                .collect::<Vec<_>>(),
            ["Alice", "empty", "root"]
        );
        assert_eq!(users[0].id, "linux-uid-1500");
        assert_eq!(users[0].display_name, "Alice Smith");
        assert!(
            users.iter().all(|u| u.role == "Admin"
                && u.password_hash.is_empty()
                && u.password_hint.is_none())
        );
        assert_eq!(
            uid_range(" UID_MIN 500 # comment\nUID_MAX 10000\nSYS_UID_MIN 100"),
            (500, 10000)
        );
        assert_eq!(uid_range("UID_MIN 90000"), (1000, 60000));
    }

    #[test]
    fn preferences_cannot_override_system_identity_or_credentials() {
        let mut users = parse_users(
            "alice:x:1000:1000:Alice:/home/alice:/bin/bash",
            (1000, 60000),
        );
        let mut forged = users[0].clone();
        forged.role = "Guest".into();
        forged.password_hash = "fake hash".into();
        forged.password_hint = Some("legacy secret".into());
        forged.enabled = false;
        forged.display_name = "Forged name".into();
        forged.failed_login_attempts = 10;
        forged.locked_until_epoch_ms = Some(u64::MAX);
        forged.last_login_at_epoch_ms = Some(42);
        attach_preferences(&mut users, &[forged]);
        assert_eq!(users[0].role, "Admin");
        assert!(users[0].enabled);
        assert!(users[0].password_hash.is_empty());
        assert_eq!(users[0].password_hint, None);
        assert_eq!(users[0].display_name, "Alice");
        assert_eq!(users[0].locked_until_epoch_ms, None);
        assert_eq!(users[0].last_login_at_epoch_ms, Some(42));
    }

    #[test]
    fn username_case_and_uid_are_identity_boundaries() {
        let mut users = parse_users(
            "Alice:x:1000:1000::/:/bin/sh\nalice:x:1001:1001::/:/bin/sh",
            (1000, 60000),
        );
        let mut stale = users[0].clone();
        stale.username = "previous-owner".into();
        stale.last_login_at_epoch_ms = Some(42);
        attach_preferences(&mut users, &[stale]);
        assert_eq!(users.len(), 2);
        assert!(
            users
                .iter()
                .all(|user| user.last_login_at_epoch_ms.is_none())
        );
    }
}
