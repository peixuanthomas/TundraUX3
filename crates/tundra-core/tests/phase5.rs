use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use tundra_core::{
    AuditOutcome, AuditService, CoreError, DebugPolicy, FAILED_LOGIN_LOCK_THRESHOLD,
    PASSWORD_HINT_MAX_LEN, PermissionAction, PermissionService, SessionService, UserRole,
    UserService, verify_password,
};
use tundra_platform::{AppPaths, cleanup_temp_path};
use tundra_storage::{StorageManager, UserRecord};

#[test]
fn permission_matrix_matches_phase5_roles() {
    let service = PermissionService::new(DebugPolicy {
        debug_build: true,
        allow_release_debug: false,
    });
    let user = session("user", UserRole::User);
    let admin = session("admin", UserRole::Admin);
    let debug = session("debug", UserRole::Debug);

    assert!(
        service
            .authorize(None, PermissionAction::Login, None)
            .allowed
    );
    assert!(
        !service
            .authorize(None, PermissionAction::ReadFile, None)
            .allowed
    );
    assert!(
        service
            .authorize(Some(&user), PermissionAction::DeleteFile, None)
            .allowed
    );
    assert!(
        !service
            .authorize(Some(&user), PermissionAction::ManageUsers, None)
            .allowed
    );
    assert!(
        service
            .authorize(Some(&admin), PermissionAction::ViewAuditLog, None)
            .allowed
    );
    assert!(
        !service
            .authorize(Some(&admin), PermissionAction::EnterDebugMode, None)
            .allowed
    );
    assert!(
        service
            .authorize(Some(&debug), PermissionAction::EnterDebugMode, None)
            .allowed
    );
}

#[test]
fn release_debug_policy_denies_debug_even_for_debug_role() {
    let service = PermissionService::new(DebugPolicy {
        debug_build: false,
        allow_release_debug: false,
    });
    let debug = session("debug", UserRole::Debug);

    let result = service.authorize(Some(&debug), PermissionAction::EnterDebugMode, None);

    assert!(!result.allowed);
    assert_eq!(result.reason.as_deref(), Some("debug_policy_denied"));
}

#[test]
fn bootstrap_login_and_password_hashing_work_without_plaintext_storage() {
    let fixture = FixtureRoot::new("auth");
    let manager = storage(fixture.path());
    let users = UserService::new(manager.clone());

    let admin = users
        .bootstrap_admin("AdminUser", "StrongPass123")
        .expect("bootstrap should create admin");
    assert_eq!(admin.role, UserRole::Admin);
    assert_eq!(admin.password_hint, None);

    let stored = manager.load_users().expect("users should load");
    assert_eq!(stored.users.len(), 1);
    assert_eq!(stored.users[0].password_hint, None);
    assert_ne!(stored.users[0].password_hash, "StrongPass123");
    assert!(stored.users[0].password_hash.starts_with("$argon2"));
    assert!(verify_password(
        "StrongPass123",
        &stored.users[0].password_hash
    ));

    assert!(matches!(
        users.bootstrap_admin("second", "StrongPass123"),
        Err(CoreError::BootstrapAlreadyExists)
    ));

    let mut sessions = SessionService::new(manager);
    let session = sessions
        .login("adminuser", "StrongPass123")
        .expect("case-insensitive login should work");
    assert_eq!(session.username, "AdminUser");
    assert_eq!(
        sessions.current_session().map(|s| s.username.as_str()),
        Some("AdminUser")
    );

    sessions.logout().expect("logout should audit");
    assert!(sessions.current_session().is_none());
}

#[test]
fn bootstrap_admin_with_trimmed_hint_persists_and_login_still_works() {
    let fixture = FixtureRoot::new("auth-hint");
    let manager = storage(fixture.path());
    let users = UserService::new(manager.clone());

    let admin = users
        .bootstrap_admin_with_hint(
            "AdminUser",
            "StrongPass123",
            Some("  used for this device  "),
        )
        .expect("bootstrap should create admin with hint");

    assert_eq!(admin.password_hint.as_deref(), Some("used for this device"));
    let stored = manager.load_users().expect("users should load");
    assert_eq!(
        stored.users[0].password_hint.as_deref(),
        Some("used for this device")
    );

    let mut sessions = SessionService::new(manager);
    let session = sessions
        .login("AdminUser", "StrongPass123")
        .expect("login should work with hinted account");
    assert_eq!(session.username, "AdminUser");
}

#[test]
fn bootstrap_admin_with_blank_hint_stores_none() {
    let fixture = FixtureRoot::new("blank-hint");
    let manager = storage(fixture.path());
    let users = UserService::new(manager.clone());

    let admin = users
        .bootstrap_admin_with_hint("AdminUser", "StrongPass123", Some(" \t\n "))
        .expect("blank hint should normalize to none");

    assert_eq!(admin.password_hint, None);
    let stored = manager.load_users().expect("users should load");
    assert_eq!(stored.users[0].password_hint, None);
}

#[test]
fn invalid_password_hint_rejects_without_creating_user() {
    let fixture = FixtureRoot::new("invalid-hint");
    let manager = storage(fixture.path());
    let users = UserService::new(manager.clone());
    let too_long = "x".repeat(PASSWORD_HINT_MAX_LEN + 1);
    let invalid_hints = [
        "StrongPass123".to_string(),
        "remember\u{0007}this".to_string(),
        too_long,
    ];

    for hint in invalid_hints {
        let result = users.bootstrap_admin_with_hint("AdminUser", "StrongPass123", Some(&hint));

        assert!(matches!(result, Err(CoreError::InvalidUserInfo(_))));
        assert!(
            manager
                .load_users()
                .expect("users should load")
                .users
                .is_empty()
        );
    }
}

#[test]
fn repeated_bad_passwords_lock_account_until_admin_unlocks() {
    let fixture = FixtureRoot::new("lockout");
    let manager = storage(fixture.path());
    UserService::new(manager.clone())
        .bootstrap_admin("AdminUser", "StrongPass123")
        .expect("bootstrap should create admin");

    let mut sessions = SessionService::new(manager.clone());
    for _ in 0..FAILED_LOGIN_LOCK_THRESHOLD - 1 {
        assert!(matches!(
            sessions.login("AdminUser", "WrongPass123"),
            Err(CoreError::InvalidCredentials)
        ));
    }
    assert!(matches!(
        sessions.login("AdminUser", "WrongPass123"),
        Err(CoreError::AccountLocked { .. })
    ));
    assert!(matches!(
        sessions.login("AdminUser", "StrongPass123"),
        Err(CoreError::AccountLocked { .. })
    ));

    unlock_first_user_for_test(&manager);
    assert!(sessions.login("AdminUser", "StrongPass123").is_ok());
}

#[test]
fn user_service_management_requires_admin_or_debug_session() {
    let fixture = FixtureRoot::new("manage-users");
    let manager = storage(fixture.path());
    let users = UserService::new(manager.clone());
    users
        .bootstrap_admin("AdminUser", "StrongPass123")
        .expect("bootstrap");
    let mut sessions = SessionService::new(manager.clone());
    let admin = sessions.login("AdminUser", "StrongPass123").expect("login");

    let created = users
        .create_user(
            &admin,
            "NormalUser",
            "Normal User",
            UserRole::User,
            "NormalPass123",
        )
        .expect("admin can create users");
    assert_eq!(created.role, UserRole::User);

    let mut normal_login = SessionService::new(manager.clone());
    let user_session = normal_login
        .login("NormalUser", "NormalPass123")
        .expect("normal login");
    let self_list = users
        .list_accessible_users(&user_session)
        .expect("user can list self");
    assert_eq!(self_list.len(), 1);
    assert_eq!(self_list[0].username, "NormalUser");

    users
        .update_user_info(&user_session, "NormalUser", "Updated Normal")
        .expect("user can update own profile");
    users
        .set_user_password(&user_session, "NormalUser", "SelfPass123")
        .expect("user can update own password");
    assert!(normal_login.login("NormalUser", "SelfPass123").is_ok());

    assert!(matches!(
        users.create_user(
            &user_session,
            "DeniedUser",
            "Denied",
            UserRole::User,
            "DeniedPass123"
        ),
        Err(CoreError::PermissionDenied { .. })
    ));
    assert!(matches!(
        users.update_user_info(&user_session, "AdminUser", "Denied"),
        Err(CoreError::PermissionDenied { .. })
    ));
    assert!(matches!(
        users.delete_user(&user_session, "AdminUser"),
        Err(CoreError::PermissionDenied { .. })
    ));

    users
        .change_role(&admin, "NormalUser", UserRole::Debug)
        .expect("role changes");
    let debug_session = SessionService::new(manager.clone())
        .login("NormalUser", "SelfPass123")
        .expect("debug login");
    users
        .create_user(
            &debug_session,
            "DebugCreated",
            "Debug Created",
            UserRole::User,
            "DebugPass123",
        )
        .expect("debug can create users");
    users
        .reset_password(&debug_session, "DebugCreated", "ChangedPass123")
        .expect("debug can reset password");
    users
        .disable_user(&debug_session, "DebugCreated")
        .expect("debug can disable");

    let all = users.list_users(&admin).expect("list users");
    let normal = all
        .iter()
        .find(|user| user.username == "NormalUser")
        .expect("normal user");
    assert_eq!(normal.role, UserRole::Debug);
    assert!(normal.enabled);
    assert!(all.iter().any(|user| user.username == "DebugCreated"));
}

#[test]
fn delete_user_removes_accounts_but_preserves_last_privileged_user() {
    let fixture = FixtureRoot::new("delete-users");
    let manager = storage(fixture.path());
    let users = UserService::new(manager.clone());
    users
        .bootstrap_admin("AdminUser", "StrongPass123")
        .expect("bootstrap");
    let mut sessions = SessionService::new(manager.clone());
    let admin = sessions.login("AdminUser", "StrongPass123").expect("login");

    users
        .create_user(
            &admin,
            "NormalUser",
            "Normal User",
            UserRole::User,
            "NormalPass123",
        )
        .expect("admin can create normal");
    users
        .create_user(
            &admin,
            "SecondAdmin",
            "Second Admin",
            UserRole::Admin,
            "SecondPass123",
        )
        .expect("admin can create second admin");
    users
        .create_user(
            &admin,
            "DebugManager",
            "Debug Manager",
            UserRole::Debug,
            "DebugPass123",
        )
        .expect("admin can create debug manager");

    users
        .delete_user(&admin, "NormalUser")
        .expect("admin can delete normal user");
    assert!(
        !users
            .list_users(&admin)
            .expect("users")
            .iter()
            .any(|user| user.username == "NormalUser")
    );

    users
        .delete_user(&admin, "SecondAdmin")
        .expect("another enabled admin remains");

    let debug_session = SessionService::new(manager.clone())
        .login("DebugManager", "DebugPass123")
        .expect("debug login");
    users
        .delete_user(&debug_session, "AdminUser")
        .expect("debug can delete the last admin when debug remains privileged");
    assert!(matches!(
        users.create_user(
            &admin,
            "StaleAdminUser",
            "Stale Admin User",
            UserRole::User,
            "StalePass123"
        ),
        Err(CoreError::PermissionDenied { .. })
    ));
    assert!(matches!(
        users.delete_user(&debug_session, "DebugManager"),
        Err(CoreError::LastPrivilegedUserRequired)
    ));
    assert!(matches!(
        users.disable_user(&debug_session, "DebugManager"),
        Err(CoreError::LastPrivilegedUserRequired)
    ));
    assert!(matches!(
        users.change_role(&debug_session, "DebugManager", UserRole::User),
        Err(CoreError::LastPrivilegedUserRequired)
    ));
}

#[test]
fn audit_chain_verifies_and_detects_tampering() {
    let fixture = FixtureRoot::new("audit");
    let manager = storage(fixture.path());
    let audit = AuditService::new(manager.clone());
    let admin = session("admin", UserRole::Admin);

    audit
        .record(
            Some(&admin),
            PermissionAction::Login,
            Some("admin"),
            AuditOutcome::Success,
            Some("login_success"),
        )
        .expect("first audit");
    audit
        .record(
            Some(&admin),
            PermissionAction::ManageUsers,
            Some("user"),
            AuditOutcome::Success,
            Some("create_user"),
        )
        .expect("second audit");
    audit.verify_chain().expect("chain should verify");
    assert_eq!(
        audit
            .read_records(&admin)
            .expect("admin can read audit")
            .len(),
        2
    );

    let user = session("user", UserRole::User);
    assert!(matches!(
        audit.read_records(&user),
        Err(CoreError::PermissionDenied { .. })
    ));

    let mut contents = fs::read_to_string(manager.layout().audit_path()).expect("audit file");
    contents = contents.replacen("login_success", "tampered", 1);
    fs::write(manager.layout().audit_path(), contents).expect("tamper audit");

    assert!(matches!(
        audit.verify_chain(),
        Err(CoreError::AuditIntegrity(_))
    ));
}

fn session(username: &str, role: UserRole) -> tundra_core::AuthSession {
    tundra_core::AuthSession {
        session_id: format!("session-{username}"),
        user_id: format!("id-{username}"),
        username: username.to_string(),
        role,
        started_at_epoch_ms: 1,
    }
}

fn storage(base: &Path) -> StorageManager {
    StorageManager::open(
        AppPaths::from_parts(
            base.join("config").join("config.toml"),
            base.join("state"),
            base.join("cache"),
            base.join("logs"),
            base.join("temp"),
        )
        .expect("absolute fixture paths"),
    )
    .expect("storage open")
    .manager
}

fn unlock_first_user_for_test(manager: &StorageManager) {
    let mut users = manager.load_users().expect("users load");
    let record: &mut UserRecord = users.users.first_mut().expect("first user");
    record.failed_login_attempts = 0;
    record.locked_until_epoch_ms = None;
    manager.save_users(&users).expect("users save");
}

struct FixtureRoot {
    path: PathBuf,
}

impl FixtureRoot {
    fn new(case: &str) -> Self {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "tundra-core-test-{}-{nanos}-{case}",
            std::process::id()
        ));
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for FixtureRoot {
    fn drop(&mut self) {
        let _ = cleanup_temp_path(&self.path);
    }
}
