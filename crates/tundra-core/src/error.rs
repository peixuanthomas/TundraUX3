use std::fmt;

use argon2::password_hash;
use tundra_storage::StorageError;

use crate::authorization::PermissionAction;

#[derive(Debug)]
pub enum CoreError {
    Storage(StorageError),
    Json(String),
    PasswordHash(String),
    InvalidUsername,
    InvalidUserInfo(String),
    InvalidPassword(String),
    BootstrapAlreadyExists,
    BootstrapRequired,
    DuplicateUsername,
    UserNotFound,
    InvalidCredentials,
    AccountDisabled,
    AccountLocked {
        locked_until_epoch_ms: u64,
    },
    PermissionDenied {
        action: PermissionAction,
        reason: String,
    },
    LastPrivilegedUserRequired,
    AuditIntegrity(String),
}

impl fmt::Display for CoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Storage(error) => write!(formatter, "{error}"),
            Self::Json(message) => formatter.write_str(message),
            Self::PasswordHash(message) => formatter.write_str(message),
            Self::InvalidUsername => formatter.write_str("invalid username"),
            Self::InvalidUserInfo(message) => write!(formatter, "invalid user info: {message}"),
            Self::InvalidPassword(message) => write!(formatter, "invalid password: {message}"),
            Self::BootstrapAlreadyExists => formatter.write_str("bootstrap admin already exists"),
            Self::BootstrapRequired => formatter.write_str("bootstrap admin is required"),
            Self::DuplicateUsername => formatter.write_str("username already exists"),
            Self::UserNotFound => formatter.write_str("user not found"),
            Self::InvalidCredentials => formatter.write_str("invalid credentials"),
            Self::AccountDisabled => formatter.write_str("account disabled"),
            Self::AccountLocked {
                locked_until_epoch_ms,
            } => write!(formatter, "account locked until {locked_until_epoch_ms}"),
            Self::PermissionDenied { action, reason } => {
                write!(formatter, "{action} denied: {reason}")
            }
            Self::LastPrivilegedUserRequired => {
                formatter.write_str("at least one enabled admin is required")
            }
            Self::AuditIntegrity(message) => write!(formatter, "audit integrity error: {message}"),
        }
    }
}

impl std::error::Error for CoreError {}

impl From<StorageError> for CoreError {
    fn from(value: StorageError) -> Self {
        Self::Storage(value)
    }
}

impl From<serde_json::Error> for CoreError {
    fn from(value: serde_json::Error) -> Self {
        Self::Json(value.to_string())
    }
}

impl From<password_hash::Error> for CoreError {
    fn from(value: password_hash::Error) -> Self {
        Self::PasswordHash(value.to_string())
    }
}
