use rand_core::{OsRng, RngCore};
use storage::{AppearanceConfig, UserRecord, UsersDocument};

use crate::authorization::UserRole;
use crate::error::CoreError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthSession {
    pub session_id: String,
    pub user_id: String,
    pub username: String,
    pub role: UserRole,
    pub started_at_epoch_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserAccount {
    pub id: String,
    pub username: String,
    pub display_name: String,
    pub role: UserRole,
    pub enabled: bool,
    pub failed_login_attempts: u32,
    pub locked_until_epoch_ms: Option<u64>,
    pub password_hint: Option<String>,
    pub appearance: AppearanceConfig,
    pub created_at_epoch_ms: u64,
    pub updated_at_epoch_ms: u64,
    pub last_login_at_epoch_ms: Option<u64>,
}

impl UserAccount {
    pub(crate) fn from_record(record: &UserRecord) -> Self {
        Self {
            id: record.id.clone(),
            username: record.username.clone(),
            display_name: record.display_name.clone(),
            role: UserRole::from_storage(&record.role),
            enabled: record.enabled,
            failed_login_attempts: record.failed_login_attempts,
            locked_until_epoch_ms: record.locked_until_epoch_ms,
            password_hint: record.password_hint.clone(),
            appearance: record.appearance.clone(),
            created_at_epoch_ms: record.created_at_epoch_ms,
            updated_at_epoch_ms: record.updated_at_epoch_ms,
            last_login_at_epoch_ms: record.last_login_at_epoch_ms,
        }
    }
}

pub(crate) fn validate_username(username: &str) -> Result<(), CoreError> {
    let trimmed = username.trim();
    if trimmed.is_empty()
        || trimmed.len() > 64
        || !trimmed.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.')
        })
    {
        return Err(CoreError::InvalidUsername);
    }
    Ok(())
}

pub(crate) fn normalize_display_name(
    display_name: &str,
    username: &str,
) -> Result<String, CoreError> {
    let trimmed = display_name.trim();
    if trimmed.len() > 128 {
        return Err(CoreError::InvalidUserInfo(
            "display_name_too_long".to_string(),
        ));
    }
    if trimmed.chars().any(char::is_control) {
        return Err(CoreError::InvalidUserInfo(
            "display_name_has_control_chars".to_string(),
        ));
    }
    if trimmed.is_empty() {
        Ok(username.to_string())
    } else {
        Ok(trimmed.to_string())
    }
}

pub(crate) fn normalize_username(username: &str) -> String {
    username.trim().to_ascii_lowercase()
}

pub(crate) fn actor_can_manage_users(document: &UsersDocument, actor: &AuthSession) -> bool {
    actor.role == UserRole::Admin
        && find_authenticated_user_index(document, actor)
            .map(|index| is_enabled_admin(&document.users[index]))
            .unwrap_or(false)
}

pub(crate) fn is_same_user(actor: &AuthSession, record: &UserRecord) -> bool {
    actor.user_id == record.id
}

pub(crate) fn ensure_unique_username(
    document: &UsersDocument,
    username: &str,
) -> Result<(), CoreError> {
    let normalized = normalize_username(username);
    if document
        .users
        .iter()
        .any(|record| normalize_username(&record.username) == normalized)
    {
        return Err(CoreError::DuplicateUsername);
    }
    Ok(())
}

pub(crate) fn find_user_index(document: &UsersDocument, username: &str) -> Option<usize> {
    let normalized = normalize_username(username);
    document
        .users
        .iter()
        .position(|record| normalize_username(&record.username) == normalized)
}

pub(crate) fn find_authenticated_user_index(
    document: &UsersDocument,
    actor: &AuthSession,
) -> Option<usize> {
    document
        .users
        .iter()
        .position(|record| is_same_user(actor, record))
}

pub(crate) fn ensure_can_remove_enabled_admin(
    document: &UsersDocument,
    target_index: usize,
) -> Result<(), CoreError> {
    if !is_enabled_admin(&document.users[target_index]) {
        return Ok(());
    }
    let has_other_enabled_admin = document
        .users
        .iter()
        .enumerate()
        .any(|(index, record)| index != target_index && is_enabled_admin(record));
    if has_other_enabled_admin {
        Ok(())
    } else {
        Err(CoreError::LastPrivilegedUserRequired)
    }
}

fn is_enabled_admin(record: &UserRecord) -> bool {
    record.enabled && UserRole::from_storage(&record.role) == UserRole::Admin
}

pub(crate) fn next_user_id(document: &UsersDocument) -> String {
    let mut random = OsRng;
    loop {
        let mut bytes = [0_u8; 16];
        random.fill_bytes(&mut bytes);
        let candidate = format!("user-{}", hex::encode(bytes));
        if !document.users.iter().any(|record| record.id == candidate) {
            return candidate;
        }
    }
}
