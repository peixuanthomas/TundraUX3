use storage::{
    AppearanceConfig, StorageManager, SystemStatusDashboardConfig, UserRecord, UsersDocument,
};

use crate::authorization::{DebugPolicy, PermissionAction, PermissionService, UserRole};
use crate::credentials::{hash_password, normalize_password_hint, validate_password};
use crate::error::CoreError;
use crate::identity::{
    AuthSession, UserAccount, actor_can_manage_users, ensure_can_remove_enabled_admin,
    ensure_unique_username, find_authenticated_user_index, find_user_index, is_same_user,
    next_user_id, normalize_display_name, validate_username,
};
use crate::time::unix_millis;

#[derive(Clone)]
pub struct UserService {
    storage: StorageManager,
    debug_policy: DebugPolicy,
    backend: crate::IdentityBackend,
    #[cfg(target_os = "linux")]
    interaction: Option<std::sync::Arc<dyn platform::linux::authorization::Interaction>>,
}

impl std::fmt::Debug for UserService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UserService")
            .field("backend", &self.backend)
            .field("debug_policy", &self.debug_policy)
            .finish_non_exhaustive()
    }
}

impl UserService {
    #[cfg(target_os = "linux")]
    pub fn with_authorization_interaction(
        mut self,
        interaction: std::sync::Arc<dyn platform::linux::authorization::Interaction>,
    ) -> Self {
        self.interaction = Some(interaction);
        self
    }

    #[cfg(target_os = "linux")]
    fn linux_accounts(
        &self,
        actor: &AuthSession,
    ) -> Result<platform::linux::accounts::Accounts, CoreError> {
        if actor.source != crate::IdentitySource::LinuxCurrentProcess {
            return Err(CoreError::SystemAccountManaged);
        }
        let mut accounts =
            platform::linux::accounts::Accounts::current().map_err(crate::linux::error)?;
        let current = accounts.actor().map_err(crate::linux::error)?;
        if actor.user_id != format!("linux-uid-{}", current.uid)
            || actor.username != current.username
        {
            return Err(CoreError::PermissionDenied {
                action: PermissionAction::ManageOwnUser,
                reason: "stale_session".into(),
            });
        }
        if let Some(interaction) = &self.interaction {
            accounts.set_interaction(interaction.clone());
        }
        Ok(accounts)
    }

    #[cfg(target_os = "linux")]
    fn linux_visible(
        &self,
        accounts: &platform::linux::accounts::Accounts,
    ) -> Result<Vec<UserAccount>, CoreError> {
        accounts
            .visible()
            .map_err(crate::linux::error)?
            .into_iter()
            .map(|account| {
                crate::linux::record(account, &self.storage)
                    .map(|record| UserAccount::from_record(&record))
            })
            .collect()
    }

    pub fn new(storage: StorageManager) -> Self {
        Self {
            storage,
            debug_policy: DebugPolicy::default(),
            backend: crate::IdentityBackend::Local,
            #[cfg(target_os = "linux")]
            interaction: None,
        }
    }

    pub fn with_debug_policy(storage: StorageManager, debug_policy: DebugPolicy) -> Self {
        Self {
            storage,
            debug_policy,
            backend: crate::IdentityBackend::Local,
            #[cfg(target_os = "linux")]
            interaction: None,
        }
    }

    pub fn with_backend(mut self, backend: crate::IdentityBackend) -> Self {
        self.backend = backend;
        self
    }

    /// Account records from the selected authority, with UX preferences attached.
    pub fn login_records(&self) -> Result<Vec<UserRecord>, CoreError> {
        self.backend.users(&self.storage)
    }

    /// Read only the caller's saved appearance, without listing system accounts.
    pub fn current_user_appearance(
        &self,
        actor: &AuthSession,
    ) -> Result<AppearanceConfig, CoreError> {
        Ok(self
            .own_preferences_record(actor, &actor.username)?
            .appearance)
    }

    fn own_preferences_record(
        &self,
        actor: &AuthSession,
        username: &str,
    ) -> Result<UserRecord, CoreError> {
        if !self.backend.usernames_match(username, &actor.username) {
            return Err(CoreError::UserNotFound);
        }
        #[cfg(target_os = "linux")]
        if self.backend == crate::IdentityBackend::Linux {
            let record = crate::linux::current_profile(&self.storage)?;
            if actor.source != crate::IdentitySource::LinuxCurrentProcess
                || actor.user_id != record.id
                || actor.username != record.username
                || username != record.username
            {
                return Err(CoreError::UserNotFound);
            }
            return Ok(record);
        }
        self.backend.require_local()?;
        let document = self.storage.load_users()?;
        let index =
            find_authenticated_user_index(&document, actor).ok_or(CoreError::UserNotFound)?;
        let record = &document.users[index];
        if !record.enabled {
            return Err(CoreError::AccountDisabled);
        }
        Ok(record.clone())
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
        self.bootstrap_admin_with_hint_and_appearance(
            username,
            password,
            password_hint,
            AppearanceConfig::default(),
        )
    }

    pub fn validate_bootstrap_admin(
        &self,
        username: &str,
        password: &str,
        password_hint: Option<&str>,
    ) -> Result<(), CoreError> {
        self.backend.require_local()?;
        let document = self.storage.load_users()?;
        validate_bootstrap_admin_input(&document, username, password, password_hint)
    }

    pub fn bootstrap_admin_with_hint_and_appearance(
        &self,
        username: &str,
        password: &str,
        password_hint: Option<&str>,
        appearance: AppearanceConfig,
    ) -> Result<UserAccount, CoreError> {
        self.backend.require_local()?;
        let mut document = self.storage.load_users()?;
        validate_bootstrap_admin_input(&document, username, password, password_hint)?;
        let password_hint = normalize_password_hint(password_hint, password)?;
        let now = unix_millis();
        let record = UserRecord {
            id: next_user_id(&document),
            username: username.trim().to_string(),
            display_name: username.trim().to_string(),
            role: UserRole::Admin.as_str().to_string(),
            password_hash: hash_password(password)?,
            password_hint,
            appearance,
            personalization_pending: false,
            system_status_dashboard: SystemStatusDashboardConfig::for_role(
                UserRole::Admin.as_str(),
            ),
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
        #[cfg(target_os = "linux")]
        if self.backend == crate::IdentityBackend::Linux {
            let accounts = self.linux_accounts(actor)?;
            if !accounts.actor().map_err(crate::linux::error)?.admin {
                return Err(CoreError::PermissionDenied {
                    action: PermissionAction::ManageUsers,
                    reason: "insufficient_role".into(),
                });
            }
            return self.linux_visible(&accounts);
        }
        self.backend.require_local()?;
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
        #[cfg(target_os = "linux")]
        if self.backend == crate::IdentityBackend::Linux {
            return self.linux_visible(&self.linux_accounts(actor)?);
        }
        self.backend.require_local()?;
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
            .login_records()?
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
        #[cfg(target_os = "linux")]
        if self.backend == crate::IdentityBackend::Linux {
            let accounts = self.linux_accounts(actor)?;
            validate_username(username)?;
            validate_password(username, password)?;
            let name = normalize_display_name(display_name, username)?;
            if role == UserRole::Guest {
                return Err(CoreError::InvalidUserInfo(
                    "Linux supports User and Admin accounts".into(),
                ));
            }
            let account = accounts
                .create(username.trim(), &name, role == UserRole::Admin, password)
                .map_err(crate::linux::error)?;
            return Ok(UserAccount::from_record(&crate::linux::record(
                account,
                &self.storage,
            )?));
        }

        self.backend.require_local()?;
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
            appearance: AppearanceConfig::default(),
            personalization_pending: false,
            system_status_dashboard: SystemStatusDashboardConfig::for_role(role.as_str()),
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
        #[cfg(target_os = "linux")]
        if self.backend == crate::IdentityBackend::Linux {
            let accounts = self.linux_accounts(actor)?;
            let name = normalize_display_name(display_name, username)?;
            accounts
                .rename(username, &name)
                .map_err(crate::linux::error)?;
            return self
                .linux_visible(&accounts)?
                .into_iter()
                .find(|user| user.username == username)
                .ok_or(CoreError::UserNotFound);
        }

        self.backend.require_local()?;
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

    /// Save first-login preferences and their completion marker in one write.
    /// The record was created by authentication; this does not create OS accounts.
    pub fn complete_personalization(
        &self,
        actor: &AuthSession,
        appearance: AppearanceConfig,
    ) -> Result<UserAccount, CoreError> {
        self.authorize_manage_own_user(actor, "complete_personalization")?;
        let mut document = self.storage.load_users()?;
        let record = document
            .users
            .iter_mut()
            .find(|user| {
                user.id == actor.user_id
                    && self
                        .backend
                        .usernames_match(&user.username, &actor.username)
            })
            .ok_or(CoreError::UserNotFound)?;
        if !record.enabled {
            return Err(CoreError::AccountDisabled);
        }
        record.appearance = appearance;
        record.personalization_pending = false;
        record.updated_at_epoch_ms = unix_millis();
        let account = UserAccount::from_record(record);
        self.storage.save_users(&document)?;
        Ok(account)
    }

    pub fn update_user_appearance(
        &self,
        actor: &AuthSession,
        username: &str,
        appearance: AppearanceConfig,
    ) -> Result<UserAccount, CoreError> {
        if self.backend == crate::IdentityBackend::Linux {
            let mut record = self.own_preferences_record(actor, username)?;
            record.appearance = appearance;
            record.updated_at_epoch_ms = unix_millis();
            let mut document = self.storage.load_users()?;
            document.users.retain(|user| user.id != record.id);
            document.users.push(record.clone());
            self.storage.save_users(&document)?;
            return Ok(UserAccount::from_record(&record));
        }

        let mut document = self.storage.load_users()?;
        let Some(index) = find_user_index(&document, username) else {
            return Err(CoreError::UserNotFound);
        };
        let target = &document.users[index];
        if !is_same_user(actor, target) {
            return Err(CoreError::PermissionDenied {
                action: PermissionAction::ManageOwnUser,
                reason: "appearance_is_self_managed".to_string(),
            });
        }
        if !target.enabled {
            return Err(CoreError::AccountDisabled);
        }
        self.authorize_manage_own_user(actor, "update_user_appearance")?;
        document.users[index].appearance = appearance;
        document.users[index].updated_at_epoch_ms = unix_millis();
        let account = UserAccount::from_record(&document.users[index]);
        self.storage.save_users(&document)?;
        Ok(account)
    }

    pub fn update_user_system_status_dashboard(
        &self,
        actor: &AuthSession,
        username: &str,
        mut dashboard: SystemStatusDashboardConfig,
    ) -> Result<UserAccount, CoreError> {
        if self.backend == crate::IdentityBackend::Linux {
            let mut record = self.own_preferences_record(actor, username)?;
            dashboard.normalize();
            record.system_status_dashboard = dashboard;
            record.updated_at_epoch_ms = unix_millis();
            let mut document = self.storage.load_users()?;
            document.users.retain(|user| user.id != record.id);
            document.users.push(record.clone());
            self.storage.save_users(&document)?;
            return Ok(UserAccount::from_record(&record));
        }

        let mut document = self.storage.load_users()?;
        let Some(index) = find_user_index(&document, username) else {
            return Err(CoreError::UserNotFound);
        };
        let target = &document.users[index];
        if !is_same_user(actor, target) {
            return Err(CoreError::PermissionDenied {
                action: PermissionAction::ManageOwnUser,
                reason: "system_status_dashboard_is_self_managed".to_string(),
            });
        }
        if !target.enabled {
            return Err(CoreError::AccountDisabled);
        }
        self.authorize_manage_own_user(actor, "update_user_system_status_dashboard")?;
        dashboard.normalize();
        document.users[index].system_status_dashboard = dashboard;
        document.users[index].updated_at_epoch_ms = unix_millis();
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
        #[cfg(target_os = "linux")]
        if self.backend == crate::IdentityBackend::Linux {
            let accounts = self.linux_accounts(actor)?;
            if username != actor.username {
                validate_password(username, password)?;
            }
            return accounts
                .password(username, password)
                .map_err(crate::linux::error);
        }

        self.backend.require_local()?;
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
        #[cfg(target_os = "linux")]
        if self.backend == crate::IdentityBackend::Linux {
            return self
                .linux_accounts(actor)?
                .set_locked(username, true)
                .map_err(crate::linux::error);
        }

        self.update_user(actor, username, "disable_user", |document, index, now| {
            ensure_can_remove_enabled_admin(document, index)?;
            let record = &mut document.users[index];
            record.enabled = false;
            record.updated_at_epoch_ms = now;
            Ok(())
        })
    }

    pub fn enable_user(&self, actor: &AuthSession, username: &str) -> Result<(), CoreError> {
        #[cfg(target_os = "linux")]
        if self.backend == crate::IdentityBackend::Linux {
            return self
                .linux_accounts(actor)?
                .set_locked(username, false)
                .map_err(crate::linux::error);
        }

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
        #[cfg(target_os = "linux")]
        if self.backend == crate::IdentityBackend::Linux {
            return self.enable_user(actor, username);
        }

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
        #[cfg(target_os = "linux")]
        if self.backend == crate::IdentityBackend::Linux {
            return self.set_user_password(actor, username, password);
        }

        self.backend.require_local()?;
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
        #[cfg(target_os = "linux")]
        if self.backend == crate::IdentityBackend::Linux {
            if role == UserRole::Guest {
                return Err(CoreError::InvalidUserInfo(
                    "Linux supports User and Admin accounts".into(),
                ));
            }
            return self
                .linux_accounts(actor)?
                .set_admin(username, role == UserRole::Admin)
                .map_err(crate::linux::error);
        }

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
        #[cfg(target_os = "linux")]
        if self.backend == crate::IdentityBackend::Linux {
            return self
                .linux_accounts(actor)?
                .delete(username)
                .map_err(crate::linux::error);
        }

        self.backend.require_local()?;
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
        self.backend.require_local()?;
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

fn validate_bootstrap_admin_input(
    document: &UsersDocument,
    username: &str,
    password: &str,
    password_hint: Option<&str>,
) -> Result<(), CoreError> {
    if !document.users.is_empty() {
        return Err(CoreError::BootstrapAlreadyExists);
    }
    validate_username(username)?;
    validate_password(username, password)?;
    normalize_password_hint(password_hint, password)?;
    Ok(())
}
