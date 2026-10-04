use super::super::*;

use platform::management::{OperationEvent, OperationInput};
use zeroize::Zeroizing;

pub(in crate::session) enum UserManagementOperation {
    Create(UserManagementCreateForm),
    EditInfo(UserManagementInfoForm),
    Password { username: String },
    Disable { username: String },
    Enable { username: String },
    Role { username: String, role: UserRole },
    Delete { username: String },
}

#[derive(Default)]
enum Completion {
    #[default]
    None,
    DisableCurrent,
    DeleteLocal {
        user_id: Option<String>,
        current: bool,
    },
}

impl UserManagementOperation {
    fn username(&self) -> &str {
        match self {
            Self::Create(form) => &form.username,
            Self::EditInfo(form) => &form.username,
            Self::Password { username }
            | Self::Disable { username }
            | Self::Enable { username }
            | Self::Role { username, .. }
            | Self::Delete { username } => username,
        }
    }
    fn description(&self, backend: identity::IdentityBackend) -> String {
        let linux = backend == identity::IdentityBackend::Linux;
        let user = self.username().to_string();
        match self {
            Self::Create(form) => i18n::tr!(
                if linux {
                    "aa-user-create"
                } else {
                    "aa-local-user-create"
                },
                user = user,
                role = format!("{:?}", form.role)
            ),
            Self::EditInfo(form) => i18n::tr!(
                if linux {
                    "aa-user-rename"
                } else {
                    "aa-local-user-rename"
                },
                user = user,
                name = form.display_name.clone()
            ),
            Self::Password { .. } => {
                i18n::tr!(
                    if linux {
                        "aa-user-password"
                    } else {
                        "aa-local-user-password"
                    },
                    user = user
                )
            }
            Self::Disable { .. } => i18n::tr!(
                if linux {
                    "aa-user-disable"
                } else {
                    "aa-local-user-disable"
                },
                user = user
            ),
            Self::Enable { .. } => i18n::tr!(
                if linux {
                    "aa-user-enable"
                } else {
                    "aa-local-user-enable"
                },
                user = user
            ),
            Self::Role { role, .. } => i18n::tr!(
                if linux {
                    "aa-user-role"
                } else {
                    "aa-local-user-role"
                },
                user = user,
                role = format!("{role:?}")
            ),
            Self::Delete { .. } => i18n::tr!(
                if linux {
                    "aa-user-delete"
                } else {
                    "aa-local-user-delete"
                },
                user = user
            ),
        }
    }
    fn message(&self) -> i18n::LocalizedText {
        let user = self.username().to_string();
        match self {
            Self::Create(_) => i18n::msg!("shell-created-arg1", arg1 = user).into(),
            Self::EditInfo(_) => i18n::msg!("shell-updated-arg1", arg1 = user).into(),
            Self::Password { .. } => {
                i18n::msg!("shell-updated-password-for-arg1", arg1 = user).into()
            }
            Self::Delete { .. } => i18n::msg!("shell-deleted-username", username = user).into(),
            Self::Disable { .. } | Self::Enable { .. } | Self::Role { .. } => i18n::msg!(
                "shell-success-prefix-username",
                success_prefix = i18n::msg!(match self {
                    Self::Disable { .. } => "shell-disabled",
                    Self::Enable { .. } => "shell-enabled-unlocked",
                    _ => "shell-changed-role-for",
                }),
                username = user
            )
            .into(),
        }
    }
    fn execute(
        self,
        service: &UserService,
        actor: &AuthSession,
        backend: identity::IdentityBackend,
        job: &AutoAdminJob,
        inputs: &mpsc::Receiver<OperationInput>,
    ) -> Result<(), CoreError> {
        match self {
            Self::Create(form) => {
                let password = Zeroizing::new(form.password);
                service
                    .create_user(
                        actor,
                        &form.username,
                        &form.display_name,
                        form.role,
                        &password,
                    )
                    .map(|_| ())
            }
            Self::EditInfo(form) => service
                .update_user_info(actor, &form.username, &form.display_name)
                .map(|_| ()),
            Self::Password { username } => {
                if backend == identity::IdentityBackend::Linux
                    && backend.usernames_match(&actor.username, &username)
                {
                    service.set_user_password(actor, &username, "")
                } else {
                    let password = read_confirmed_password(job, inputs)?;
                    service.set_user_password(actor, &username, &password)
                }
            }
            Self::Disable { username } => service.disable_user(actor, &username),
            Self::Enable { username } => service.enable_user(actor, &username),
            Self::Role { username, role } => service.change_role(actor, &username, role),
            Self::Delete { username } => service.delete_user(actor, &username),
        }
    }
}

fn read_confirmed_password(
    job: &AutoAdminJob,
    inputs: &mpsc::Receiver<OperationInput>,
) -> Result<Zeroizing<String>, CoreError> {
    loop {
        let password = job
            .read_secret(inputs, "new-password", i18n::tr!("aa-new-password"))
            .map_err(|error| CoreError::SystemIdentity(error.to_string()))?;
        let confirmation = job
            .read_secret(inputs, "confirm-password", i18n::tr!("aa-confirm-password"))
            .map_err(|error| CoreError::SystemIdentity(error.to_string()))?;
        if *password == *confirmation {
            return Ok(password);
        }
        job.emit(&OperationEvent::Output {
            text: i18n::tr!("account-passwords-do-not-match"),
        });
    }
}

#[derive(Clone)]
pub(in crate::session) struct UserManagementJob(Arc<Job>);
struct Job {
    result: Mutex<Option<Outcome>>,
    worker: Mutex<Option<ManagedThreadHandle<()>>>,
    session_id: String,
    completion: Completion,
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
    pub(in crate::session) fn start_user_management_task(
        &mut self,
        operation: Option<UserManagementOperation>,
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
        let backend = self.identity_backend;
        let service =
            UserService::with_debug_policy(storage, self.debug_policy).with_backend(backend);
        let (responses, inputs) = mpsc::channel();
        let aa = if let Some(operation) = &operation {
            let mut description = operation.description(backend);
            if matches!(operation, UserManagementOperation::Password { .. })
                && backend == identity::IdentityBackend::Linux
                && !self.is_current_username(operation.username())
            {
                description.push_str(&format!(
                    "\n{}",
                    i18n::tr!("shell-linux-password-enables-account")
                ));
            }
            if matches!(operation, UserManagementOperation::Delete { .. })
                && backend == identity::IdentityBackend::Local
                && self.is_current_username(operation.username())
            {
                description = i18n::tr!(
                    "shell-delete-username-you-will-be-signed-out-immediately",
                    username = operation.username().to_string()
                );
            }
            let Some(job) = self.begin_auto_admin(description, true, responses) else {
                return false;
            };
            Some(job)
        } else {
            None
        };
        #[cfg(target_os = "linux")]
        let service = if backend == identity::IdentityBackend::Linux {
            if let Some(job) = &aa {
                service.with_authorization_interaction(Arc::new(
                    super::auto_admin::AutoAdminAuthorization::new(job.clone()),
                ))
            } else {
                service
            }
        } else {
            service
        };
        let completion = if backend == identity::IdentityBackend::Local {
            match &operation {
                Some(UserManagementOperation::Disable { username })
                    if self.is_current_username(username) =>
                {
                    Completion::DisableCurrent
                }
                Some(UserManagementOperation::Delete { username }) => Completion::DeleteLocal {
                    current: self.is_current_username(username),
                    user_id: self
                        .app
                        .managed_users()
                        .iter()
                        .find(|user| backend.usernames_match(&user.username, username))
                        .map(|user| user.id.clone()),
                },
                _ => Completion::None,
            }
        } else {
            Completion::None
        };
        let message = operation.as_ref().map(UserManagementOperation::message);
        let select = operation
            .as_ref()
            .map(|operation| operation.username().trim().to_string());
        let worker_aa = aa.clone();
        let language = self.language.clone();
        let shared = Arc::new(Job {
            result: Mutex::new(None),
            worker: Mutex::new(None),
            session_id: actor.session_id.clone(),
            completion,
        });
        let output = Arc::downgrade(&shared);
        let mut pending = Some(operation);
        match group.spawn_thread(
            TaskSpec::one_shot(TaskId::from_static("user-management")),
            move || {
                let _language = i18n::enter_snapshot(language.clone());
                let Some(operation) = pending.take() else {
                    return;
                };
                let result = match (operation, worker_aa.as_ref()) {
                    (Some(operation), Some(job)) => job
                        .wait_for_approval()
                        .map_err(|error| CoreError::SystemIdentity(error.to_string()))
                        .and_then(|()| operation.execute(&service, &actor, backend, job, &inputs)),
                    (None, _) => Ok(()),
                    _ => Err(CoreError::SystemIdentity(
                        "Missing AutoAdmin approval".into(),
                    )),
                };
                if let Some(job) = &worker_aa {
                    job.finish(
                        result
                            .as_ref()
                            .map(|()| {
                                message
                                    .as_ref()
                                    .map(i18n::LocalizedText::render_current)
                                    .unwrap_or_else(|| i18n::tr!("aa-completed"))
                            })
                            .map_err(ToString::to_string),
                    );
                }
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
                *shared
                    .worker
                    .lock()
                    .unwrap_or_else(|error| error.into_inner()) = Some(worker);
                self.user_management_job = Some(UserManagementJob(shared));
                self.user_management_message =
                    Some(i18n::msg!("shell-linux-accounts-working").into());
                self.user_management_feedback_tone = UserManagementFeedbackTone::Info;
                true
            }
            Err(error) => {
                if let Some(job) = &aa {
                    job.finish(Err(error.to_string()));
                }
                self.report_user_management_refresh_error(error.to_string());
                false
            }
        }
    }

    pub(in crate::session) fn poll_user_management_task(&mut self) {
        let Some(job) = self.user_management_job.clone() else {
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
        if outcome.result.is_ok() {
            let logout: Option<i18n::LocalizedText> = match &job.0.completion {
                Completion::None => None,
                Completion::DisableCurrent => Some(i18n::msg!("shell-account-disabled").into()),
                Completion::DeleteLocal { user_id, current } => {
                    if let Some(user_id) = user_id
                        && let Some(storage) = &self.storage_manager
                    {
                        let result = storage.load_clock().and_then(|mut document| {
                            document.profiles.remove(user_id);
                            storage.save_clock(&document)
                        });
                        if let Err(error) = result {
                            self.report_clock_storage_error(error.to_string());
                        }
                    }
                    current.then(|| i18n::msg!("shell-account-deleted").into())
                }
            };
            if let Some(message) = logout {
                self.return_to_login(message);
                return;
            }
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
                // Linux authority may have revoked access. Local read failures
                // retain the last list, matching ordinary local refresh behavior.
                if self.identity_backend == identity::IdentityBackend::Linux {
                    self.app
                        .dispatch_at(app::AppCommand::SetManagedUsers(Vec::new()), Instant::now());
                }
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
