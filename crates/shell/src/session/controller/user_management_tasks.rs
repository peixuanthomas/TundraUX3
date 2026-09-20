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
#[path = "../../../tests/unit/session/controller/user_management_tasks/tests.rs"]
mod tests;
