use argon2::Argon2;
use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use rand_core::OsRng;

use crate::error::CoreError;
use crate::identity::normalize_username;

pub const PASSWORD_MIN_LEN: usize = 10;
pub const PASSWORD_MAX_LEN: usize = 256;
pub const PASSWORD_HINT_MAX_LEN: usize = 128;

pub fn hash_password(password: &str) -> Result<String, CoreError> {
    let salt = SaltString::generate(&mut OsRng);
    Ok(Argon2::default()
        .hash_password(password.as_bytes(), &salt)?
        .to_string())
}

pub fn verify_password(password: &str, password_hash: &str) -> bool {
    let Ok(parsed) = PasswordHash::new(password_hash) else {
        return false;
    };
    Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .is_ok()
}

pub fn validate_password(username: &str, password: &str) -> Result<(), CoreError> {
    if password.len() < PASSWORD_MIN_LEN {
        return Err(CoreError::InvalidPassword("too_short".to_string()));
    }
    if password.len() > PASSWORD_MAX_LEN {
        return Err(CoreError::InvalidPassword("too_long".to_string()));
    }
    if password.trim().is_empty() {
        return Err(CoreError::InvalidPassword("blank".to_string()));
    }
    if normalize_username(username) == normalize_username(password) {
        return Err(CoreError::InvalidPassword("matches_username".to_string()));
    }
    Ok(())
}

pub(crate) fn normalize_password_hint(
    password_hint: Option<&str>,
    password: &str,
) -> Result<Option<String>, CoreError> {
    let Some(password_hint) = password_hint else {
        return Ok(None);
    };
    let trimmed = password_hint.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    if trimmed.len() > PASSWORD_HINT_MAX_LEN {
        return Err(CoreError::InvalidUserInfo(
            "password_hint_too_long".to_string(),
        ));
    }
    if trimmed.chars().any(char::is_control) {
        return Err(CoreError::InvalidUserInfo(
            "password_hint_has_control_chars".to_string(),
        ));
    }
    if trimmed == password || trimmed == password.trim() {
        return Err(CoreError::InvalidUserInfo(
            "password_hint_matches_password".to_string(),
        ));
    }
    Ok(Some(trimmed.to_string()))
}
