use tundra_storage::StorageManager;

use crate::authorization::UserRole;
use crate::credentials::verify_password;
use crate::error::CoreError;
use crate::identity::{AuthSession, find_user_index};
use crate::time::{unix_millis, unix_nanos};

pub const FAILED_LOGIN_LOCK_THRESHOLD: u32 = 5;
pub const LOCKOUT_DURATION_MS: u64 = 5 * 60 * 1000;

#[derive(Debug, Clone)]
pub struct SessionService {
    storage: StorageManager,
    current_session: Option<AuthSession>,
}

impl SessionService {
    pub fn new(storage: StorageManager) -> Self {
        Self {
            storage,
            current_session: None,
        }
    }

    pub fn current_session(&self) -> Option<&AuthSession> {
        self.current_session.as_ref()
    }

    pub fn bootstrap_required(&self) -> Result<bool, CoreError> {
        Ok(self.storage.load_users()?.users.is_empty())
    }

    pub fn login(&mut self, username: &str, password: &str) -> Result<AuthSession, CoreError> {
        let mut document = self.storage.load_users()?;
        if document.users.is_empty() {
            return Err(CoreError::BootstrapRequired);
        }

        let Some(index) = find_user_index(&document, username) else {
            return Err(CoreError::InvalidCredentials);
        };

        let now = unix_millis();
        let mut result = Ok(());
        {
            let record = &mut document.users[index];
            if !record.enabled {
                result = Err(CoreError::AccountDisabled);
            } else if let Some(locked_until) = record.locked_until_epoch_ms
                && locked_until > now
            {
                result = Err(CoreError::AccountLocked {
                    locked_until_epoch_ms: locked_until,
                });
            }
        }

        if let Err(error) = result {
            return Err(error);
        }

        let password_ok = verify_password(password, &document.users[index].password_hash);
        if !password_ok {
            let record = &mut document.users[index];
            record.failed_login_attempts = record.failed_login_attempts.saturating_add(1);
            let locked = record.failed_login_attempts >= FAILED_LOGIN_LOCK_THRESHOLD;
            if locked {
                record.locked_until_epoch_ms = Some(now.saturating_add(LOCKOUT_DURATION_MS));
            }
            record.updated_at_epoch_ms = now;
            self.storage.save_users(&document)?;
            return if locked {
                Err(CoreError::AccountLocked {
                    locked_until_epoch_ms: now.saturating_add(LOCKOUT_DURATION_MS),
                })
            } else {
                Err(CoreError::InvalidCredentials)
            };
        }

        let record = &mut document.users[index];
        record.failed_login_attempts = 0;
        record.locked_until_epoch_ms = None;
        record.last_login_at_epoch_ms = Some(now);
        record.updated_at_epoch_ms = now;
        let session = AuthSession {
            session_id: format!("session-{}-{}", record.id, unix_nanos()),
            user_id: record.id.clone(),
            username: record.username.clone(),
            role: UserRole::from_storage(&record.role),
            started_at_epoch_ms: now,
        };
        self.storage.save_users(&document)?;
        self.current_session = Some(session.clone());
        Ok(session)
    }

    pub fn logout(&mut self) -> Result<(), CoreError> {
        self.current_session = None;
        Ok(())
    }
}
