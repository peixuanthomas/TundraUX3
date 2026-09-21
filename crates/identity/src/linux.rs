//! Attach to the current process account. Saved records hold preferences only.
use crate::time::{unix_millis, unix_nanos};
use crate::{AuthSession, CoreError, IdentitySource, UserRole};
use storage::{StorageManager, UserRecord};

pub(super) fn error(error: platform::service::ServiceError) -> CoreError {
    CoreError::SystemIdentity(error.to_string())
}

pub(super) fn record(
    account: platform::linux::accounts::Account,
    storage: &StorageManager,
) -> Result<UserRecord, CoreError> {
    let role = if account.admin {
        UserRole::Admin
    } else {
        UserRole::User
    };
    let mut record = UserRecord {
        id: format!("linux-uid-{}", account.uid),
        username: account.username,
        display_name: account.display_name,
        role: role.as_str().into(),
        password_hash: String::new(),
        password_hint: None,
        appearance: Default::default(),
        personalization_pending: true,
        system_status_dashboard: storage::SystemStatusDashboardConfig::for_role(role.as_str()),
        // AccountsService Locked only locks password authentication. Existing
        // sessions and key-based login can remain valid; do not disable the UX.
        enabled: true,
        failed_login_attempts: 0,
        locked_until_epoch_ms: account.locked.then_some(u64::MAX),
        created_at_epoch_ms: 0,
        updated_at_epoch_ms: 0,
        last_login_at_epoch_ms: None,
    };
    if record.display_name.is_empty() {
        record.display_name = record.username.clone();
    }
    attach_preferences(&mut record, &storage.load_users()?.users);
    Ok(record)
}

/// Personal UX preferences need only the process identity and local storage.
/// Do not query AccountsService here: it may be absent or unavailable.
pub(super) fn current_profile(storage: &StorageManager) -> Result<UserRecord, CoreError> {
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
    Ok(record)
}

pub(super) fn users(storage: &StorageManager) -> Result<Vec<UserRecord>, CoreError> {
    let record = current_profile(storage)?;
    // The desktop can still start without AccountsService; management reports its
    // availability separately. Stored UX roles never grant Linux admin access.
    if let Ok(account) = platform::linux::accounts::Accounts::current_account() {
        return Ok(vec![self::record(account, storage)?]);
    }
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
        role: UserRole::from_storage(&current.role),
        started_at_epoch_ms: now,
    };
    let mut document = storage.load_users()?;
    document.users.retain(|user| user.id != current.id);
    document.users.push(current);
    storage.save_users(&document)?;
    Ok(session)
}

#[cfg(test)]
#[path = "../tests/unit/linux/tests.rs"]
mod tests;
