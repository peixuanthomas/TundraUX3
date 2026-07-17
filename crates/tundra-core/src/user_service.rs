use tundra_storage::{StorageManager, UserRecord, UsersDocument};

use crate::authorization::{DebugPolicy, PermissionAction, PermissionService, UserRole};
use crate::credentials::{hash_password, normalize_password_hint, validate_password};
use crate::error::CoreError;
use crate::identity::{
    AuthSession, UserAccount, actor_can_manage_users, ensure_can_remove_enabled_admin,
    ensure_unique_username, find_authenticated_user_index, find_user_index, is_same_user,
    next_user_id, normalize_display_name, validate_username,
};
use crate::time::unix_millis;

#[derive(Debug, Clone)]
pub struct UserService {
    storage: StorageManager,
    debug_policy: DebugPolicy,
}

impl UserService {
    pub fn new(storage: StorageManager) -> Self {
        Self {
            storage,
            debug_policy: DebugPolicy::default(),
        }
    }

    pub fn with_debug_policy(storage: StorageManager, debug_policy: DebugPolicy) -> Self {
        Self {
            storage,
            debug_policy,
        }
    }

    pub fn bootstrap_admin(
        &self,
        username: &str,
        password: &str,
    ) -> Result<UserAccount, CoreError> {
        self.bootstrap_admin_with_hint(username, password, None)
    }

    pub fn bootstrap_admin_with_hint(
        &self,
        username: &str,
        password: &str,
        password_hint: Option<&str>,
    ) -> Result<UserAccount, CoreError> {
        let mut document = self.storage.load_users()?;
        if !document.users.is_empty() {
            return Err(CoreError::BootstrapAlreadyExists);
        }

        validate_username(username)?;
        validate_password(username, password)?;
        let password_hint = normalize_password_hint(password_hint, password)?;
        let now = unix_millis();
        let record = UserRecord {
            id: next_user_id(&document),
            username: username.trim().to_string(),
            display_name: username.trim().to_string(),
            role: UserRole::Admin.as_str().to_string(),
            password_hash: hash_password(password)?,
            password_hint,
            enabled: true,
            failed_login_attempts: 0,
            locked_until_epoch_ms: None,
            created_at_epoch_ms: now,
            updated_at_epoch_ms: now,
            last_login_at_epoch_ms: None,
        };
        document.users.push(record.clone());
        self.storage.save_users(&document)?;
        Ok(UserAccount::from_record(&record))
    }

    pub fn list_users(&self, actor: &AuthSession) -> Result<Vec<UserAccount>, CoreError> {
        self.authorize_manage_users(actor, "list_users")?;
        Ok(self
            .storage
            .load_users()?
            .users
            .iter()
            .map(UserAccount::from_record)
            .collect())
    }

    pub fn list_accessible_users(
        &self,
        actor: &AuthSession,
    ) -> Result<Vec<UserAccount>, CoreError> {
        let document = self.storage.load_users()?;
        if actor_can_manage_users(&document, actor) {
            return Ok(document
                .users
                .iter()
                .map(UserAccount::from_record)
                .collect());
        }

        let Some(index) = find_authenticated_user_index(&document, actor) else {
            return Err(CoreError::UserNotFound);
        };
        if !document.users[index].enabled {
            return Err(CoreError::AccountDisabled);
        }
        Ok(vec![UserAccount::from_record(&document.users[index])])
    }

    pub fn list_all_users_unchecked(&self) -> Result<Vec<UserAccount>, CoreError> {
        Ok(self
            .storage
            .load_users()?
            .users
            .iter()
            .map(UserAccount::from_record)
            .collect())
    }

    pub fn create_user(
        &self,
        actor: &AuthSession,
        username: &str,
        display_name: &str,
        role: UserRole,
        password: &str,
    ) -> Result<UserAccount, CoreError> {
        self.authorize_manage_users(actor, "create_user")?;
        validate_username(username)?;
        validate_password(username, password)?;
        let mut document = self.storage.load_users()?;
        ensure_unique_username(&document, username)?;
        let username = username.trim().to_string();
        let display_name = normalize_display_name(display_name, &username)?;

        let now = unix_millis();
        let record = UserRecord {
            id: next_user_id(&document),
            username,
            display_name,
            role: role.as_str().to_string(),
            password_hash: hash_password(password)?,
            password_hint: None,
            enabled: true,
            failed_login_attempts: 0,
            locked_until_epoch_ms: None,
            created_at_epoch_ms: now,
            updated_at_epoch_ms: now,
            last_login_at_epoch_ms: None,
        };
        document.users.push(record.clone());
        self.storage.save_users(&document)?;
        Ok(UserAccount::from_record(&record))
    }

    pub fn update_user_info(
        &self,
        actor: &AuthSession,
        username: &str,
        display_name: &str,
    ) -> Result<UserAccount, CoreError> {
        let mut document = self.storage.load_users()?;
        let Some(index) = find_user_index(&document, username) else {
            return Err(CoreError::UserNotFound);
        };
        self.authorize_user_data_operation(actor, &document.users[index], "update_user_info")?;
        let display_name = normalize_display_name(display_name, &document.users[index].username)?;
        let now = unix_millis();
        document.users[index].display_name = display_name;
        document.users[index].updated_at_epoch_ms = now;
        let account = UserAccount::from_record(&document.users[index]);
        self.storage.save_users(&document)?;
        Ok(account)
    }

    pub fn set_user_password(
        &self,
        actor: &AuthSession,
        username: &str,
        password: &str,
    ) -> Result<(), CoreError> {
        let mut document = self.storage.load_users()?;
        let Some(index) = find_user_index(&document, username) else {
            return Err(CoreError::UserNotFound);
        };
        self.authorize_user_data_operation(actor, &document.users[index], "set_user_password")?;
        validate_password(&document.users[index].username, password)?;
        let now = unix_millis();
        document.users[index].password_hash = hash_password(password)?;
        document.users[index].failed_login_attempts = 0;
        document.users[index].locked_until_epoch_ms = None;
        document.users[index].updated_at_epoch_ms = now;
        self.storage.save_users(&document)?;
        Ok(())
    }

    pub fn disable_user(&self, actor: &AuthSession, username: &str) -> Result<(), CoreError> {
        self.update_user(actor, username, "disable_user", |document, index, now| {
            ensure_can_remove_enabled_admin(document, index)?;
            let record = &mut document.users[index];
            record.enabled = false;
            record.updated_at_epoch_ms = now;
            Ok(())
        })
    }

    pub fn enable_user(&self, actor: &AuthSession, username: &str) -> Result<(), CoreError> {
        self.update_user(actor, username, "enable_user", |document, index, now| {
            let record = &mut document.users[index];
            record.enabled = true;
            record.failed_login_attempts = 0;
            record.locked_until_epoch_ms = None;
            record.updated_at_epoch_ms = now;
            Ok(())
        })
    }

    pub fn unlock_user(&self, actor: &AuthSession, username: &str) -> Result<(), CoreError> {
        self.update_user(actor, username, "unlock_user", |document, index, now| {
            let record = &mut document.users[index];
            record.failed_login_attempts = 0;
            record.locked_until_epoch_ms = None;
            record.updated_at_epoch_ms = now;
            Ok(())
        })
    }

    pub fn reset_password(
        &self,
        actor: &AuthSession,
        username: &str,
        password: &str,
    ) -> Result<(), CoreError> {
        validate_password(username, password)?;
        self.update_user(actor, username, "reset_password", |document, index, now| {
            let record = &mut document.users[index];
            record.password_hash = hash_password(password)?;
            record.failed_login_attempts = 0;
            record.locked_until_epoch_ms = None;
            record.updated_at_epoch_ms = now;
            Ok(())
        })
    }

    pub fn change_role(
        &self,
        actor: &AuthSession,
        username: &str,
        role: UserRole,
    ) -> Result<(), CoreError> {
        self.update_user(actor, username, "change_role", |document, index, now| {
            if role != UserRole::Admin {
                ensure_can_remove_enabled_admin(document, index)?;
            }
            let record = &mut document.users[index];
            record.role = role.as_str().to_string();
            record.updated_at_epoch_ms = now;
            Ok(())
        })
    }

    pub fn delete_user(&self, actor: &AuthSession, username: &str) -> Result<(), CoreError> {
        let mut document = self.storage.load_users()?;
        let Some(index) = find_user_index(&document, username) else {
            return Err(CoreError::UserNotFound);
        };
        self.authorize_user_data_operation(actor, &document.users[index], "delete_user")?;
        ensure_can_remove_enabled_admin(&document, index)?;
        document.users.remove(index);
        self.storage.save_users(&document)?;
        Ok(())
    }

    fn update_user(
        &self,
        actor: &AuthSession,
        username: &str,
        operation: &'static str,
        update: impl FnOnce(&mut UsersDocument, usize, u64) -> Result<(), CoreError>,
    ) -> Result<(), CoreError> {
        self.authorize_manage_users(actor, operation)?;
        let mut document = self.storage.load_users()?;
        let Some(index) = find_user_index(&document, username) else {
            return Err(CoreError::UserNotFound);
        };
        update(&mut document, index, unix_millis())?;
        self.storage.save_users(&document)?;
        Ok(())
    }

    fn authorize_user_data_operation(
        &self,
        actor: &AuthSession,
        target: &UserRecord,
        operation: &'static str,
    ) -> Result<(), CoreError> {
        if is_same_user(actor, target) {
            if !target.enabled {
                return Err(CoreError::AccountDisabled);
            }
            self.authorize_manage_own_user(actor, operation)?;
            return Ok(());
        }

        self.authorize_manage_users(actor, operation)?;
        Ok(())
    }

    fn authorize_manage_own_user(
        &self,
        actor: &AuthSession,
        operation: &'static str,
    ) -> Result<(), CoreError> {
        let permission = PermissionService::new(self.debug_policy).authorize(
            Some(actor),
            PermissionAction::ManageOwnUser,
            Some(operation),
        );
        if permission.allowed {
            return Ok(());
        }

        let reason = permission
            .reason
            .unwrap_or_else(|| "permission_denied".to_string());
        Err(CoreError::PermissionDenied {
            action: PermissionAction::ManageOwnUser,
            reason,
        })
    }

    fn authorize_manage_users(
        &self,
        actor: &AuthSession,
        operation: &'static str,
    ) -> Result<(), CoreError> {
        let permission = PermissionService::new(self.debug_policy).authorize(
            Some(actor),
            PermissionAction::ManageUsers,
            Some(operation),
        );
        let document = self.storage.load_users()?;
        if permission.allowed && actor_can_manage_users(&document, actor) {
            return Ok(());
        }

        let reason = if permission.allowed {
            match find_authenticated_user_index(&document, actor) {
                Some(index) if !document.users[index].enabled => "account_disabled".to_string(),
                Some(_) => "insufficient_role".to_string(),
                None => "stale_session".to_string(),
            }
        } else {
            permission
                .reason
                .unwrap_or_else(|| "permission_denied".to_string())
        };
        Err(CoreError::PermissionDenied {
            action: PermissionAction::ManageUsers,
            reason,
        })
    }
}
