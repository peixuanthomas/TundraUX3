use super::*;

struct AccountFixture(PathBuf);
impl Drop for AccountFixture {
    fn drop(&mut self) {
        let _ = platform::cleanup_temp_path(&self.0);
    }
}
fn local_state(policy: storage::AutoAdminPolicy) -> (ShellSession, StorageManager, AccountFixture) {
    let root = std::env::temp_dir().join(format!(
        "tundra-aa-accounts-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let paths = platform::build_windows_app_paths(
        root.join("roaming"),
        root.join("local"),
        root.join("temp"),
    )
    .unwrap();
    let storage = StorageManager::open(paths).unwrap().manager;
    let mut config = storage.load_config().unwrap();
    config.auto_admin = policy;
    storage.save_config(&config).unwrap();
    let service = UserService::new(storage.clone());
    service
        .bootstrap_admin("admin", "InitialPassword123!")
        .unwrap();
    let actor = identity::SessionService::new(storage.clone())
        .login("admin", "InitialPassword123!")
        .unwrap();
    service
        .create_user(&actor, "other", "Other", UserRole::User, "InitialOther123!")
        .unwrap();
    let mut state = ShellSession::new_for_home_mode(
        ShellLaunchConfig::default(),
        (120, 40),
        ShellHomeMode::User,
    );
    state.identity_backend = identity::IdentityBackend::Local;
    state.storage_manager = Some(storage.clone());
    state.app.dispatch_at(
        app::AppCommand::SetAuthSession(Some(actor.clone())),
        Instant::now(),
    );
    state.app.dispatch_at(
        app::AppCommand::SetManagedUsers(service.list_accessible_users(&actor).unwrap()),
        Instant::now(),
    );
    state.select_managed_username("other");
    (state, storage, AccountFixture(root))
}
fn wait_for(state: &mut ShellSession, ready: impl Fn(&ShellSession) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        state.poll_user_management_task();
        if ready(state) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "account operation did not reach the expected state"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}
fn secret_prompt(state: &mut ShellSession) {
    wait_for(state, |state| {
        state
            .auto_admin_view()
            .is_some_and(|view| view.input.is_some())
    });
}
fn answer_secret(state: &mut ShellSession, value: &str) {
    state.apply_input(InputEvent::Paste(value.into()));
    let view = state.auto_admin_view().unwrap();
    assert_eq!(
        view.input.as_deref(),
        Some(format!("> {}", "•".repeat(value.chars().count())).as_str())
    );
    assert!(!view.description.contains(value));
    assert!(
        !view
            .terminal
            .cells
            .iter()
            .map(|cell| cell.symbol.as_str())
            .collect::<String>()
            .contains(value)
    );
    state.apply_input(InputEvent::key(InputKey::Enter));
    state.apply_input(InputEvent::Key(KeyInput::with_phase(
        InputKey::Enter,
        InputModifiers::NONE,
        InputPhase::Release,
    )));
}

#[test]
fn password_change_waits_for_approval_and_two_matching_hidden_answers() {
    let (mut state, storage, _fixture) = local_state(storage::AutoAdminPolicy::Manual);
    let old = storage
        .load_users()
        .unwrap()
        .users
        .iter()
        .find(|user| user.username == "other")
        .unwrap()
        .password_hash
        .clone();
    state.begin_set_selected_password();
    assert!(matches!(
        state.user_management_mode,
        UserManagementMode::Browse
    ));
    assert!(state.auto_admin_view().unwrap().confirming);
    assert!(state.auto_admin_view().unwrap().input.is_none());
    assert_eq!(
        storage
            .load_users()
            .unwrap()
            .users
            .iter()
            .find(|user| user.username == "other")
            .unwrap()
            .password_hash,
        old
    );
    state.apply_input(InputEvent::key(InputKey::Tab));
    state.apply_input(InputEvent::key(InputKey::Enter));
    secret_prompt(&mut state);
    answer_secret(&mut state, "NewPassword123!");
    secret_prompt(&mut state);
    answer_secret(&mut state, "Mismatch123!");
    secret_prompt(&mut state);
    assert_eq!(
        storage
            .load_users()
            .unwrap()
            .users
            .iter()
            .find(|user| user.username == "other")
            .unwrap()
            .password_hash,
        old
    );
    answer_secret(&mut state, "NewPassword123!");
    secret_prompt(&mut state);
    answer_secret(&mut state, "NewPassword123!");
    wait_for(&mut state, |state| state.user_management_job.is_none());
    assert_eq!(
        state.user_management_feedback_tone,
        UserManagementFeedbackTone::Success
    );
    assert!(
        identity::SessionService::new(storage.clone())
            .login("other", "NewPassword123!")
            .is_ok()
    );
    let view = state.auto_admin_view().unwrap();
    assert!(view.finished);
    assert!(!format!("{view:?}").contains("NewPassword123!"));
    assert!(!format!("{view:?}").contains("Mismatch123!"));
}

#[test]
fn rejected_and_forbidden_account_operations_do_not_write_user_data() {
    for policy in [
        storage::AutoAdminPolicy::Manual,
        storage::AutoAdminPolicy::Deny,
    ] {
        let (mut state, storage, _fixture) = local_state(policy);
        for action in 0..7 {
            let before = std::fs::read(&storage.layout().users_path).unwrap();
            let operation = match action {
                0 => UserManagementOperation::Password {
                    username: "other".into(),
                },
                1 => UserManagementOperation::Disable {
                    username: "other".into(),
                },
                2 => UserManagementOperation::Enable {
                    username: "other".into(),
                },
                3 => UserManagementOperation::Role {
                    username: "other".into(),
                    role: UserRole::Admin,
                },
                4 => UserManagementOperation::Delete {
                    username: "other".into(),
                },
                5 => UserManagementOperation::EditInfo(UserManagementInfoForm {
                    username: "other".into(),
                    display_name: "Changed".into(),
                    focused_field: UserManagementFormField::DisplayName,
                }),
                _ => UserManagementOperation::Create(UserManagementCreateForm {
                    username: "created".into(),
                    display_name: "Created".into(),
                    password: "CreatedPassword123!".into(),
                    role: UserRole::User,
                    focused_field: UserManagementFormField::Username,
                }),
            };
            assert!(state.start_user_management_task(Some(operation)));
            if policy == storage::AutoAdminPolicy::Manual {
                state.apply_input(InputEvent::key(InputKey::Escape));
            }
            wait_for(&mut state, |state| state.user_management_job.is_none());
            assert_eq!(
                std::fs::read(&storage.layout().users_path).unwrap(),
                before,
                "action {action} under {policy:?}"
            );
            assert!(state.auto_admin_view().unwrap().input.is_none());
        }
    }
}

#[test]
fn automatic_approval_still_requires_password_and_cancel_leaves_it_unchanged() {
    let (mut state, storage, _fixture) = local_state(storage::AutoAdminPolicy::Automatic);
    let before = std::fs::read(&storage.layout().users_path).unwrap();
    state.begin_set_selected_password();
    assert!(!state.auto_admin_view().unwrap().confirming);
    secret_prompt(&mut state);
    assert_eq!(std::fs::read(&storage.layout().users_path).unwrap(), before);
    state.apply_input(InputEvent::Key(KeyInput::with_modifiers(
        InputKey::Char('c'),
        InputModifiers::CTRL,
    )));
    wait_for(&mut state, |state| state.user_management_job.is_none());
    assert_eq!(std::fs::read(&storage.layout().users_path).unwrap(), before);
    assert_eq!(
        state.user_management_feedback_tone,
        UserManagementFeedbackTone::Error
    );
}

#[test]
fn local_admin_role_checks_still_apply_after_auto_admin_approval() {
    let (mut state, storage, _fixture) = local_state(storage::AutoAdminPolicy::Automatic);
    let actor = identity::SessionService::new(storage.clone())
        .login("other", "InitialOther123!")
        .unwrap();
    state
        .app
        .dispatch_at(app::AppCommand::SetAuthSession(Some(actor)), Instant::now());
    let before = std::fs::read(&storage.layout().users_path).unwrap();
    state.start_user_management_task(Some(UserManagementOperation::Delete {
        username: "admin".into(),
    }));
    wait_for(&mut state, |state| state.user_management_job.is_none());
    assert_eq!(std::fs::read(&storage.layout().users_path).unwrap(), before);
    assert_eq!(
        state.user_management_feedback_tone,
        UserManagementFeedbackTone::Error
    );
}

fn user(name: &str, role: UserRole) -> UserAccount {
    UserAccount {
        id: format!("linux-{name}"),
        username: name.into(),
        display_name: name.into(),
        role,
        enabled: true,
        failed_login_attempts: 0,
        locked_until_epoch_ms: None,
        password_hint: None,
        appearance: Default::default(),
        system_status_dashboard: Default::default(),
        created_at_epoch_ms: 0,
        updated_at_epoch_ms: 0,
        last_login_at_epoch_ms: None,
    }
}
fn state() -> ShellSession {
    let mut state = ShellSession::new_for_home_mode(
        ShellLaunchConfig::default(),
        (120, 40),
        ShellHomeMode::User,
    );
    state.identity_backend = identity::IdentityBackend::Linux;
    state.app.dispatch_at(
        app::AppCommand::SetAuthSession(Some(AuthSession {
            source: identity::IdentitySource::LinuxCurrentProcess,
            session_id: "session".into(),
            user_id: "linux-current".into(),
            username: "current".into(),
            role: UserRole::Admin,
            started_at_epoch_ms: 0,
        })),
        Instant::now(),
    );
    state.app.dispatch_at(
        app::AppCommand::SetManagedUsers(vec![
            user("current", UserRole::Admin),
            user("other", UserRole::User),
        ]),
        Instant::now(),
    );
    state
}
fn deliver(state: &mut ShellSession, outcome: Outcome, session_id: &str) {
    state.user_management_job = Some(UserManagementJob(Arc::new(Job {
        result: Mutex::new(Some(outcome)),
        worker: Mutex::new(None),
        session_id: session_id.into(),
        completion: Completion::None,
    })));
    state.poll_user_management_task();
}
#[test]
fn linux_refresh_removes_other_rows_and_admin_controls_after_demotion() {
    let mut state = state();
    deliver(
        &mut state,
        Outcome {
            result: Ok(()),
            users: Ok(vec![user("current", UserRole::User)]),
            message: None,
            select: None,
        },
        "session",
    );
    assert_eq!(state.app.managed_users().len(), 1);
    assert!(!state.can_manage_all_users());
    assert!(state.user_management_job.is_none());
}
#[test]
fn linux_partial_write_error_survives_refresh_and_keeps_created_user_visible() {
    let mut state = state();
    deliver(
        &mut state,
        Outcome {
            result: Err(CoreError::SystemIdentity("password setup failed".into())),
            users: Ok(vec![
                user("current", UserRole::Admin),
                user("created", UserRole::User),
            ]),
            message: Some("success".into()),
            select: Some("created".into()),
        },
        "session",
    );
    assert_eq!(
        state.selected_managed_username().as_deref(),
        Some("created")
    );
    assert_eq!(
        state.user_management_feedback_tone,
        UserManagementFeedbackTone::Error
    );
    assert!(
        state
            .user_management_message
            .as_ref()
            .unwrap()
            .render_current()
            .contains("password setup failed")
    );
}
#[test]
fn linux_stale_results_are_ignored_and_failed_refresh_clears_other_users() {
    let mut state = state();
    deliver(
        &mut state,
        Outcome {
            result: Ok(()),
            users: Ok(vec![]),
            message: None,
            select: None,
        },
        "old-session",
    );
    assert_eq!(state.app.managed_users().len(), 2);
    deliver(
        &mut state,
        Outcome {
            result: Ok(()),
            users: Err(CoreError::UserNotFound),
            message: Some("Saved".into()),
            select: None,
        },
        "session",
    );
    assert!(state.app.managed_users().is_empty());
    assert!(
        state
            .user_management_message
            .as_ref()
            .unwrap()
            .render_current()
            .contains("Saved")
    );
}
#[test]
fn linux_current_account_cannot_be_deleted_disabled_or_demoted_in_ui() {
    let state = state();
    for action in [
        ui::UserManagementAction::Delete,
        ui::UserManagementAction::ToggleEnabled,
        ui::UserManagementAction::ToggleRole,
    ] {
        assert!(!state.user_management_action_enabled(action));
    }
    assert!(state.user_management_action_enabled(ui::UserManagementAction::EditInfo));
    assert!(state.user_management_action_enabled(ui::UserManagementAction::SetPassword));
}
