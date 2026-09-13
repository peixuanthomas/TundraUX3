//! Attach to the current process account. Saved records hold preferences only.
use crate::time::{unix_millis, unix_nanos};
use crate::{AuthSession, CoreError, IdentitySource, UserRole};
use storage::{StorageManager, UserRecord};

pub(super) fn users(storage: &StorageManager) -> Result<Vec<UserRecord>, CoreError> {
    let current = platform::linux::identity::LinuxUserContext::current()
        .map_err(|error| CoreError::SystemIdentity(error.to_string()))?;
    let mut record = UserRecord {
        id: format!("linux-uid-{}", current.process.uid),
        username: current.username.clone(),
        display_name: current.username,
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
    };
    attach_preferences(&mut record, &storage.load_users()?.users);
    Ok(vec![record])
}

fn attach_preferences(record: &mut UserRecord, saved: &[UserRecord]) {
    if let Some(profile) = saved.iter().find(|profile| profile.id == record.id) {
        record.appearance = profile.appearance.clone();
        record.personalization_pending = profile.personalization_pending;
        record.system_status_dashboard = profile.system_status_dashboard.clone();
        record.created_at_epoch_ms = profile.created_at_epoch_ms;
    }
}

pub(super) fn attach(storage: &StorageManager) -> Result<AuthSession, CoreError> {
    let mut current = users(storage)?.remove(0);
    let now = unix_millis();
    if current.created_at_epoch_ms == 0 {
        current.created_at_epoch_ms = now;
    }
    current.updated_at_epoch_ms = now;
    let session = AuthSession {
        source: IdentitySource::LinuxCurrentProcess,
        // Application identifier only; never a logind session identifier.
        session_id: format!("app-session-{}-{}", current.id, unix_nanos()),
        user_id: current.id.clone(),
        username: current.username.clone(),
        role: UserRole::User,
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
    #[test]
    fn historical_admin_credentials_and_lock_state_are_not_identity() {
        let mut record: UserRecord = serde_json_record();
        let mut saved = record.clone();
        saved.role = "Admin".into();
        saved.password_hash = "secret".into();
        saved.password_hint = Some("secret hint".into());
        saved.enabled = false;
        saved.failed_login_attempts = 12;
        saved.locked_until_epoch_ms = Some(u64::MAX);
        saved.personalization_pending = false;
        attach_preferences(&mut record, &[saved]);
        assert_eq!(record.role, "User");
        assert!(record.password_hash.is_empty());
        assert!(record.password_hint.is_none());
        assert!(record.enabled);
        assert_eq!(record.failed_login_attempts, 0);
        assert_eq!(record.locked_until_epoch_ms, None);
        assert!(!record.personalization_pending);
    }
    fn serde_json_record() -> UserRecord {
        UserRecord {
            id: "linux-uid-42".into(),
            username: "user".into(),
            display_name: "user".into(),
            role: "User".into(),
            password_hash: String::new(),
            password_hint: None,
            appearance: Default::default(),
            personalization_pending: true,
            system_status_dashboard: Default::default(),
            enabled: true,
            failed_login_attempts: 0,
            locked_until_epoch_ms: None,
            created_at_epoch_ms: 0,
            updated_at_epoch_ms: 0,
            last_login_at_epoch_ms: None,
        }
    }
}
