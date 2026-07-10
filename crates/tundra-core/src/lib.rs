use std::fmt;
use std::time::{SystemTime, UNIX_EPOCH};

use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::{Argon2, password_hash};
use rand_core::{OsRng, RngCore};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tundra_storage::{StorageError, StorageManager, UserRecord, UsersDocument};

pub const PASSWORD_MIN_LEN: usize = 10;
pub const PASSWORD_MAX_LEN: usize = 256;
pub const PASSWORD_HINT_MAX_LEN: usize = 128;
pub const FAILED_LOGIN_LOCK_THRESHOLD: u32 = 5;
pub const LOCKOUT_DURATION_MS: u64 = 5 * 60 * 1000;
pub const AUDIT_SCHEMA_VERSION: u32 = 1;
pub const AUDIT_GENESIS_HASH: &str = "GENESIS";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum UserRole {
    Guest,
    User,
    Admin,
}

impl UserRole {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Guest => "Guest",
            Self::User => "User",
            Self::Admin => "Admin",
        }
    }

    fn from_storage(value: &str) -> Self {
        match value {
            "Admin" => Self::Admin,
            "Guest" => Self::Guest,
            _ => Self::User,
        }
    }
}

impl fmt::Display for UserRole {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PermissionAction {
    Login,
    Logout,
    ReadFile,
    WriteFile,
    DeleteFile,
    MoveFile,
    OpenExternal,
    ManageOwnUser,
    ManageUsers,
    ViewAuditLog,
    ChangeSettings,
    EnterDebugMode,
}

impl PermissionAction {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Login => "Login",
            Self::Logout => "Logout",
            Self::ReadFile => "ReadFile",
            Self::WriteFile => "WriteFile",
            Self::DeleteFile => "DeleteFile",
            Self::MoveFile => "MoveFile",
            Self::OpenExternal => "OpenExternal",
            Self::ManageOwnUser => "ManageOwnUser",
            Self::ManageUsers => "ManageUsers",
            Self::ViewAuditLog => "ViewAuditLog",
            Self::ChangeSettings => "ChangeSettings",
            Self::EnterDebugMode => "EnterDebugMode",
        }
    }
}

impl fmt::Display for PermissionAction {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AuditOutcome {
    Success,
    Failure,
    Denied,
}

impl AuditOutcome {
    fn as_str(self) -> &'static str {
        match self {
            Self::Success => "Success",
            Self::Failure => "Failure",
            Self::Denied => "Denied",
        }
    }
}

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
    pub created_at_epoch_ms: u64,
    pub updated_at_epoch_ms: u64,
    pub last_login_at_epoch_ms: Option<u64>,
}

impl UserAccount {
    fn from_record(record: &UserRecord) -> Self {
        Self {
            id: record.id.clone(),
            username: record.username.clone(),
            display_name: record.display_name.clone(),
            role: UserRole::from_storage(&record.role),
            enabled: record.enabled,
            failed_login_attempts: record.failed_login_attempts,
            locked_until_epoch_ms: record.locked_until_epoch_ms,
            password_hint: record.password_hint.clone(),
            created_at_epoch_ms: record.created_at_epoch_ms,
            updated_at_epoch_ms: record.updated_at_epoch_ms,
            last_login_at_epoch_ms: record.last_login_at_epoch_ms,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Authorization {
    pub allowed: bool,
    pub reason: Option<String>,
}

impl Authorization {
    pub fn allow() -> Self {
        Self {
            allowed: true,
            reason: None,
        }
    }

    pub fn deny(reason: impl Into<String>) -> Self {
        Self {
            allowed: false,
            reason: Some(reason.into()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DebugPolicy {
    pub debug_build: bool,
    pub allow_release_debug: bool,
}

impl DebugPolicy {
    pub fn current_build(allow_release_debug: bool) -> Self {
        Self {
            debug_build: cfg!(debug_assertions),
            allow_release_debug,
        }
    }

    fn allows_debug(self) -> bool {
        self.debug_build || self.allow_release_debug
    }
}

impl Default for DebugPolicy {
    fn default() -> Self {
        Self::current_build(false)
    }
}

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

#[derive(Debug, Clone)]
pub struct PermissionService {
    debug_policy: DebugPolicy,
}

impl PermissionService {
    pub fn new(debug_policy: DebugPolicy) -> Self {
        Self { debug_policy }
    }

    pub fn authorize(
        &self,
        session: Option<&AuthSession>,
        action: PermissionAction,
        _resource: Option<&str>,
    ) -> Authorization {
        let role = session
            .map(|session| session.role)
            .unwrap_or(UserRole::Guest);
        match action {
            PermissionAction::Login | PermissionAction::Logout => Authorization::allow(),
            PermissionAction::ReadFile
            | PermissionAction::WriteFile
            | PermissionAction::DeleteFile
            | PermissionAction::MoveFile
            | PermissionAction::OpenExternal => match role {
                UserRole::User | UserRole::Admin => Authorization::allow(),
                UserRole::Guest => Authorization::deny("not_authenticated"),
            },
            PermissionAction::ManageOwnUser => match role {
                UserRole::User | UserRole::Admin => Authorization::allow(),
                UserRole::Guest => Authorization::deny("not_authenticated"),
            },
            PermissionAction::ManageUsers
            | PermissionAction::ViewAuditLog
            | PermissionAction::ChangeSettings => match role {
                UserRole::Admin => Authorization::allow(),
                UserRole::Guest => Authorization::deny("not_authenticated"),
                UserRole::User => Authorization::deny("insufficient_role"),
            },
            PermissionAction::EnterDebugMode => match role {
                UserRole::Admin if self.debug_policy.allows_debug() => Authorization::allow(),
                UserRole::Admin => Authorization::deny("debug_policy_denied"),
                UserRole::Guest => Authorization::deny("not_authenticated"),
                UserRole::User => Authorization::deny("insufficient_role"),
            },
        }
    }
}

impl Default for PermissionService {
    fn default() -> Self {
        Self::new(DebugPolicy::default())
    }
}

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
        AuditService::new(self.storage.clone()).record(
            None,
            PermissionAction::ManageUsers,
            Some(&record.username),
            AuditOutcome::Success,
            Some("bootstrap_admin"),
        )?;
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
        AuditService::new(self.storage.clone()).record(
            Some(actor),
            PermissionAction::ManageUsers,
            Some(&record.username),
            AuditOutcome::Success,
            Some("create_user"),
        )?;
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
        let action =
            self.authorize_user_data_operation(actor, &document.users[index], "update_user_info")?;
        let display_name = normalize_display_name(display_name, &document.users[index].username)?;
        let now = unix_millis();
        document.users[index].display_name = display_name;
        document.users[index].updated_at_epoch_ms = now;
        let account = UserAccount::from_record(&document.users[index]);
        self.storage.save_users(&document)?;
        AuditService::new(self.storage.clone()).record(
            Some(actor),
            action,
            Some(&account.username),
            AuditOutcome::Success,
            Some("update_user_info"),
        )?;
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
        let action =
            self.authorize_user_data_operation(actor, &document.users[index], "set_user_password")?;
        validate_password(&document.users[index].username, password)?;
        let resource = document.users[index].username.clone();
        let now = unix_millis();
        document.users[index].password_hash = hash_password(password)?;
        document.users[index].failed_login_attempts = 0;
        document.users[index].locked_until_epoch_ms = None;
        document.users[index].updated_at_epoch_ms = now;
        self.storage.save_users(&document)?;
        AuditService::new(self.storage.clone()).record(
            Some(actor),
            action,
            Some(&resource),
            AuditOutcome::Success,
            Some("set_user_password"),
        )?;
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
        let action =
            self.authorize_user_data_operation(actor, &document.users[index], "delete_user")?;
        ensure_can_remove_enabled_admin(&document, index)?;
        let removed = document.users.remove(index);
        self.storage.save_users(&document)?;
        AuditService::new(self.storage.clone()).record(
            Some(actor),
            action,
            Some(&removed.username),
            AuditOutcome::Success,
            Some("delete_user"),
        )?;
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
        let resource = document.users[index].username.clone();
        update(&mut document, index, unix_millis())?;
        self.storage.save_users(&document)?;
        AuditService::new(self.storage.clone()).record(
            Some(actor),
            PermissionAction::ManageUsers,
            Some(&resource),
            AuditOutcome::Success,
            Some(operation),
        )?;
        Ok(())
    }

    fn authorize_user_data_operation(
        &self,
        actor: &AuthSession,
        target: &UserRecord,
        operation: &'static str,
    ) -> Result<PermissionAction, CoreError> {
        if is_same_user(actor, target) {
            if !target.enabled {
                return Err(CoreError::AccountDisabled);
            }
            self.authorize_manage_own_user(actor, operation)?;
            return Ok(PermissionAction::ManageOwnUser);
        }

        self.authorize_manage_users(actor, operation)?;
        Ok(PermissionAction::ManageUsers)
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
        AuditService::new(self.storage.clone()).record(
            Some(actor),
            PermissionAction::ManageOwnUser,
            Some(operation),
            AuditOutcome::Denied,
            Some(&reason),
        )?;
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
        AuditService::new(self.storage.clone()).record(
            Some(actor),
            PermissionAction::ManageUsers,
            Some(operation),
            AuditOutcome::Denied,
            Some(&reason),
        )?;
        Err(CoreError::PermissionDenied {
            action: PermissionAction::ManageUsers,
            reason,
        })
    }
}

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
            AuditService::new(self.storage.clone()).record(
                None,
                PermissionAction::Login,
                Some(username),
                AuditOutcome::Failure,
                Some("invalid_credentials"),
            )?;
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
            let reason = match &error {
                CoreError::AccountDisabled => "account_disabled",
                CoreError::AccountLocked { .. } => "account_locked",
                _ => "login_denied",
            };
            AuditService::new(self.storage.clone()).record(
                None,
                PermissionAction::Login,
                Some(username),
                AuditOutcome::Denied,
                Some(reason),
            )?;
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
            AuditService::new(self.storage.clone()).record(
                None,
                PermissionAction::Login,
                Some(username),
                if locked {
                    AuditOutcome::Denied
                } else {
                    AuditOutcome::Failure
                },
                Some(if locked {
                    "account_locked"
                } else {
                    "invalid_credentials"
                }),
            )?;
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
        AuditService::new(self.storage.clone()).record(
            Some(&session),
            PermissionAction::Login,
            Some(&session.username),
            AuditOutcome::Success,
            Some("login_success"),
        )?;
        self.current_session = Some(session.clone());
        Ok(session)
    }

    pub fn logout(&mut self) -> Result<(), CoreError> {
        if let Some(session) = self.current_session.take() {
            AuditService::new(self.storage.clone()).record(
                Some(&session),
                PermissionAction::Logout,
                Some(&session.username),
                AuditOutcome::Success,
                Some("logout"),
            )?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AuditRecord {
    pub schema_version: u32,
    pub sequence: u64,
    pub timestamp_epoch_ms: u64,
    pub actor_user_id: Option<String>,
    pub actor_username: Option<String>,
    pub session_id: Option<String>,
    pub action: String,
    pub resource: Option<String>,
    pub outcome: String,
    pub reason: Option<String>,
    pub previous_hash: String,
    pub hash: String,
}

#[derive(Debug, Clone, Serialize)]
struct AuditHashPayload {
    schema_version: u32,
    sequence: u64,
    timestamp_epoch_ms: u64,
    actor_user_id: Option<String>,
    actor_username: Option<String>,
    session_id: Option<String>,
    action: String,
    resource: Option<String>,
    outcome: String,
    reason: Option<String>,
}

#[derive(Debug, Clone)]
pub struct AuditService {
    storage: StorageManager,
    permission_service: PermissionService,
}

impl AuditService {
    pub fn new(storage: StorageManager) -> Self {
        Self {
            storage,
            permission_service: PermissionService::default(),
        }
    }

    pub fn with_permission_service(
        storage: StorageManager,
        permission_service: PermissionService,
    ) -> Self {
        Self {
            storage,
            permission_service,
        }
    }

    pub fn record(
        &self,
        actor: Option<&AuthSession>,
        action: PermissionAction,
        resource: Option<&str>,
        outcome: AuditOutcome,
        reason: Option<&str>,
    ) -> Result<AuditRecord, CoreError> {
        let existing = self.parse_raw_records()?;
        let previous_hash = existing
            .last()
            .map(|record| record.hash.clone())
            .unwrap_or_else(|| AUDIT_GENESIS_HASH.to_string());
        let sequence = existing.len() as u64 + 1;
        let timestamp_epoch_ms = unix_millis();
        let payload = AuditHashPayload {
            schema_version: AUDIT_SCHEMA_VERSION,
            sequence,
            timestamp_epoch_ms,
            actor_user_id: actor.map(|session| session.user_id.clone()),
            actor_username: actor.map(|session| session.username.clone()),
            session_id: actor.map(|session| session.session_id.clone()),
            action: action.as_str().to_string(),
            resource: resource.map(ToOwned::to_owned),
            outcome: outcome.as_str().to_string(),
            reason: reason.map(ToOwned::to_owned),
        };
        let hash = hash_audit_payload(&payload, &previous_hash)?;
        let record = AuditRecord {
            schema_version: payload.schema_version,
            sequence: payload.sequence,
            timestamp_epoch_ms: payload.timestamp_epoch_ms,
            actor_user_id: payload.actor_user_id,
            actor_username: payload.actor_username,
            session_id: payload.session_id,
            action: payload.action,
            resource: payload.resource,
            outcome: payload.outcome,
            reason: payload.reason,
            previous_hash,
            hash,
        };
        let line = serde_json::to_string(&record)?;
        self.storage.append_audit_line(&line)?;
        Ok(record)
    }

    pub fn read_records(&self, actor: &AuthSession) -> Result<Vec<AuditRecord>, CoreError> {
        let permission =
            self.permission_service
                .authorize(Some(actor), PermissionAction::ViewAuditLog, None);
        if !permission.allowed {
            let reason = permission
                .reason
                .unwrap_or_else(|| "permission_denied".to_string());
            self.record(
                Some(actor),
                PermissionAction::ViewAuditLog,
                None,
                AuditOutcome::Denied,
                Some(&reason),
            )?;
            return Err(CoreError::PermissionDenied {
                action: PermissionAction::ViewAuditLog,
                reason,
            });
        }

        self.parse_raw_records()
    }

    pub fn verify_chain(&self) -> Result<(), CoreError> {
        let records = self.parse_raw_records()?;
        let mut previous_hash = AUDIT_GENESIS_HASH.to_string();
        for (index, record) in records.iter().enumerate() {
            let expected_sequence = index as u64 + 1;
            if record.sequence != expected_sequence {
                return Err(CoreError::AuditIntegrity(format!(
                    "expected sequence {expected_sequence}, found {}",
                    record.sequence
                )));
            }
            if record.previous_hash != previous_hash {
                return Err(CoreError::AuditIntegrity(format!(
                    "sequence {} has mismatched previous_hash",
                    record.sequence
                )));
            }
            let payload = AuditHashPayload {
                schema_version: record.schema_version,
                sequence: record.sequence,
                timestamp_epoch_ms: record.timestamp_epoch_ms,
                actor_user_id: record.actor_user_id.clone(),
                actor_username: record.actor_username.clone(),
                session_id: record.session_id.clone(),
                action: record.action.clone(),
                resource: record.resource.clone(),
                outcome: record.outcome.clone(),
                reason: record.reason.clone(),
            };
            let expected_hash = hash_audit_payload(&payload, &record.previous_hash)?;
            if record.hash != expected_hash {
                return Err(CoreError::AuditIntegrity(format!(
                    "sequence {} has invalid hash",
                    record.sequence
                )));
            }
            previous_hash = record.hash.clone();
        }
        Ok(())
    }

    fn parse_raw_records(&self) -> Result<Vec<AuditRecord>, CoreError> {
        self.storage
            .read_audit_lines()?
            .into_iter()
            .enumerate()
            .map(|(index, line)| {
                serde_json::from_str::<AuditRecord>(&line).map_err(|error| {
                    CoreError::AuditIntegrity(format!("line {} is invalid: {error}", index + 1))
                })
            })
            .collect()
    }
}

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

fn normalize_password_hint(
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

fn validate_username(username: &str) -> Result<(), CoreError> {
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

fn normalize_display_name(display_name: &str, username: &str) -> Result<String, CoreError> {
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

fn normalize_username(username: &str) -> String {
    username.trim().to_ascii_lowercase()
}

fn actor_can_manage_users(document: &UsersDocument, actor: &AuthSession) -> bool {
    actor.role == UserRole::Admin
        && find_authenticated_user_index(document, actor)
            .map(|index| is_enabled_admin(&document.users[index]))
            .unwrap_or(false)
}

fn is_same_user(actor: &AuthSession, record: &UserRecord) -> bool {
    actor.user_id == record.id
}

fn ensure_unique_username(document: &UsersDocument, username: &str) -> Result<(), CoreError> {
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

fn find_user_index(document: &UsersDocument, username: &str) -> Option<usize> {
    let normalized = normalize_username(username);
    document
        .users
        .iter()
        .position(|record| normalize_username(&record.username) == normalized)
}

fn find_authenticated_user_index(document: &UsersDocument, actor: &AuthSession) -> Option<usize> {
    document
        .users
        .iter()
        .position(|record| is_same_user(actor, record))
}

fn ensure_can_remove_enabled_admin(
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

fn next_user_id(document: &UsersDocument) -> String {
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

fn hash_audit_payload(
    payload: &AuditHashPayload,
    previous_hash: &str,
) -> Result<String, CoreError> {
    let canonical = serde_json::to_string(payload)?;
    let mut hasher = Sha256::new();
    hasher.update(canonical.as_bytes());
    hasher.update(previous_hash.as_bytes());
    Ok(hex::encode(hasher.finalize()))
}

fn unix_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .ok()
        .and_then(|millis| u64::try_from(millis).ok())
        .unwrap_or(0)
}

fn unix_nanos() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0)
}
