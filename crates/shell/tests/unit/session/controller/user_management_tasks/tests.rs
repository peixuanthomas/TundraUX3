use super::*;
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
