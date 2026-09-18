use super::super::*;

#[derive(Clone)]
pub(in crate::session) struct UserManagementJob(Arc<Job>);
struct Job {
    result: Mutex<Option<Outcome>>,
    worker: Mutex<Option<ManagedThreadHandle<()>>>,
    session_id: String,
}
struct Outcome {
    result: Result<(), CoreError>,
    users: Result<Vec<UserAccount>, CoreError>,
    message: Option<i18n::LocalizedText>,
    select: Option<String>,
}
impl std::fmt::Debug for UserManagementJob {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("UserManagementJob")
    }
}
impl PartialEq for UserManagementJob {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}
impl Eq for UserManagementJob {}

impl ShellSession {
    pub(in crate::session) fn start_linux_user_task(
        &mut self,
        message: Option<i18n::LocalizedText>,
        select: Option<String>,
        operation: impl FnOnce(&UserService, &AuthSession) -> Result<(), CoreError> + Send + 'static,
    ) -> bool {
        if self.user_management_job.is_some() {
            return false;
        }
        let Some(storage) = self.storage_manager.clone() else {
            return false;
        };
        let Some(actor) = self.app.auth_session().cloned() else {
            return false;
        };
        let Some(group) = self.settings_task_runtime.shared.task_group.clone() else {
            self.report_user_management_refresh_error(
                platform::service::ServiceError::ServiceUnavailable.to_string(),
            );
            return false;
        };
        let mut service = UserService::with_debug_policy(storage, self.debug_policy)
            .with_backend(self.identity_backend);
        if let Ok(interaction) = self.settings_task_runtime.shared.authorization.lock()
            && let Some(interaction) = interaction.as_ref()
        {
            service = service.with_authorization_interaction(interaction.clone());
        }
        let shared = Arc::new(Job {
            result: Mutex::new(None),
            worker: Mutex::new(None),
            session_id: actor.session_id.clone(),
        });
        let output = Arc::downgrade(&shared);
        let mut operation = Some(operation);
        match group.spawn_thread(
            TaskSpec::one_shot(TaskId::from_static("linux-user-management")),
            move || {
                let Some(operation) = operation.take() else {
                    return;
                };
                let result = operation(&service, &actor);
                let users = service.list_accessible_users(&actor);
                if let Some(output) = output.upgrade()
                    && let Ok(mut slot) = output.result.lock()
                {
                    *slot = Some(Outcome {
                        result,
                        users,
                        message: message.clone(),
                        select: select.clone(),
                    });
                }
            },
        ) {
            Ok(worker) => {
                *shared.worker.lock().unwrap_or_else(|e| e.into_inner()) = Some(worker);
                self.user_management_job = Some(UserManagementJob(shared));
                self.user_management_message =
                    Some(i18n::msg!("shell-linux-accounts-working").into());
                self.user_management_feedback_tone = UserManagementFeedbackTone::Info;
                true
            }
            Err(error) => {
                self.report_user_management_refresh_error(error.to_string());
                false
            }
        }
    }

    pub(in crate::session) fn poll_user_management_task(&mut self) {
        let Some(job) = self.user_management_job.as_ref() else {
            return;
        };
        let result = job
            .0
            .result
            .lock()
            .ok()
            .and_then(|mut result| result.take());
        let Some(outcome) = result else {
            if job
                .0
                .worker
                .lock()
                .ok()
                .is_some_and(|worker| worker.as_ref().is_some_and(|worker| worker.is_finished()))
            {
                self.user_management_job = None;
                self.report_user_management_refresh_error(
                    platform::service::ServiceError::Unknown.to_string(),
                );
            }
            return;
        };
        let session_id = job.0.session_id.clone();
        self.user_management_job = None;
        if self
            .app
            .auth_session()
            .is_none_or(|actor| actor.session_id != session_id)
        {
            return;
        }
        match outcome.result {
            Ok(()) => {
                if outcome.message.is_some() {
                    self.user_management_mode = UserManagementMode::Browse;
                }
                self.user_management_message = outcome.message;
                self.user_management_feedback_tone = UserManagementFeedbackTone::Success;
            }
            Err(error) => {
                self.user_management_message = Some(format_core_error(&error));
                self.user_management_feedback_tone = UserManagementFeedbackTone::Error;
            }
        }
        match outcome.users {
            Ok(users) => {
                let selected = outcome.select.or_else(|| self.selected_managed_username());
                self.app
                    .dispatch_at(app::AppCommand::SetManagedUsers(users), Instant::now());
                self.sync_current_session_role();
                if let Some(selected) = selected {
                    self.select_managed_username(&selected);
                }
                self.ensure_user_management_selection_visible();
                self.normalize_user_management_focus();
                self.resolve_user_management_refresh_alert();
            }
            Err(error) => {
                // Clear stale rows so revoked access never leaves other users visible.
                self.app
                    .dispatch_at(app::AppCommand::SetManagedUsers(Vec::new()), Instant::now());
                let error = format_core_error(&error);
                let feedback = if let Some(operation) = self.user_management_message.clone() {
                    i18n::msg!(
                        "shell-linux-account-refresh-error",
                        operation = operation,
                        error = error
                    )
                    .into()
                } else {
                    error
                };
                self.report_user_management_refresh_error(feedback);
            }
        }
    }
}

#[cfg(test)]
mod tests {
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
}
