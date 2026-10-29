use super::*;
use platform::management::{
    ManagementAction, ManagementCommand, ManagementError, ManagementKind, ManagementQuery,
    ManagementRow, ManagementSnapshot, OperationEvent,
};
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(in crate::session) struct SystemTimeSettings {
    pub snapshot: Option<ManagementSnapshot>,
    pub error: Option<i18n::LocalizedText>,
    pub server_field: Option<String>,
    job: Option<TimeJob>,
}

#[cfg(test)]
#[path = "../../../../tests/unit/session/controller/settings/system_time_tests.rs"]
mod tests;

impl SystemTimeSettings {
    pub fn action(&self, id: &str) -> Option<(&ManagementRow, &ManagementAction)> {
        self.snapshot.as_ref()?.rows.iter().find_map(|row| {
            row.actions
                .iter()
                .find(|action| action.id == id)
                .map(|action| (row, action))
        })
    }

    pub fn busy(&self) -> bool {
        self.job.is_some()
    }

    pub fn timezone(&self) -> Option<&str> {
        self.snapshot
            .as_ref()?
            .rows
            .iter()
            .find(|row| row.id == "time")?
            .identity
            .get("timezone")
            .map(String::as_str)
    }

    pub fn configure_item(
        &self,
        item: ui::SettingsItemViewModel,
        action: &str,
    ) -> ui::SettingsItemViewModel {
        if self.busy() {
            return item.unavailable(i18n::tr!("management-working"));
        }
        match self.action(action) {
            Some((_, action)) => match &action.disabled_reason {
                Some(reason) => item.unavailable(reason.clone()),
                None => item,
            },
            None => item.unavailable(
                self.error
                    .as_ref()
                    .map(i18n::LocalizedText::render_current)
                    .unwrap_or_else(|| {
                        i18n::tr!(if self.snapshot.is_some() {
                            "settings-device-unavailable"
                        } else {
                            "management-loading"
                        })
                    }),
            ),
        }
    }

    fn command(&self, id: &str) -> Option<ManagementCommand> {
        let (row, action) = self.action(id)?;
        if self.busy() || action.disabled_reason.is_some() {
            return None;
        }
        let mut values = action.values.clone();
        values.extend(
            action
                .fields
                .iter()
                .map(|field| (field.id.clone(), field.value.clone())),
        );
        Some(ManagementCommand {
            kind: ManagementKind::SystemSettings,
            action: id.into(),
            target: Some(row.id.clone()),
            identity: row.identity.clone(),
            values,
        })
    }
}

#[derive(Clone)]
struct TimeJob(Arc<TimeJobShared>);

struct TimeJobShared {
    id: u64,
    operation: bool,
    result: Mutex<Option<Result<ManagementSnapshot, ManagementError>>>,
    cancelled: Arc<AtomicBool>,
    worker: Mutex<Option<ManagedThreadHandle<()>>>,
    auto_admin: Option<AutoAdminJob>,
}

impl fmt::Debug for TimeJob {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("TimeJob")
    }
}
impl PartialEq for TimeJob {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}
impl Eq for TimeJob {}

impl Drop for TimeJobShared {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Relaxed);
        if let Ok(Some(worker)) = self.worker.get_mut() {
            worker.cancel();
        }
    }
}

impl TimeJob {
    fn new(auto_admin: Option<AutoAdminJob>) -> Self {
        Self(Arc::new(TimeJobShared {
            id: PageInstanceId::new().get(),
            operation: auto_admin.is_some(),
            result: Mutex::new(None),
            cancelled: Arc::new(AtomicBool::new(false)),
            worker: Mutex::new(None),
            auto_admin,
        }))
    }

    fn finished(&self) -> bool {
        self.0
            .worker
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
            .is_none_or(ManagedThreadHandle::is_finished)
    }

    fn control(&self) -> TaskControl {
        let cancel = self.clone();
        let finished = self.clone();
        TaskControl::new(
            move || {
                if let Some(aa) = &cancel.0.auto_admin {
                    if aa.phase() == ::auto_admin::JobPhase::Running {
                        aa.request_stop(false);
                        return;
                    }
                    aa.cancel();
                }
                cancel.0.cancelled.store(true, Ordering::Relaxed);
            },
            move || finished.finished(),
        )
    }
}

impl ShellSession {
    pub(super) fn refresh_system_time_settings(&mut self) {
        if self.identity_backend != identity::IdentityBackend::Linux {
            return;
        }
        if self
            .pages
            .settings
            .settings_state
            .as_ref()
            .is_none_or(|state| state.system_time.busy())
        {
            return;
        }
        let Some(group) = self.settings_task_runtime.shared.task_group.clone() else {
            if let Some(state) = self.pages.settings.settings_state.as_mut() {
                state.system_time.error = Some(i18n::msg!("management-worker-unavailable").into());
            }
            self.set_settings_error(i18n::msg!("management-worker-unavailable"));
            return;
        };
        let job = TimeJob::new(None);
        let output = Arc::downgrade(&job.0);
        let cancelled = job.0.cancelled.clone();
        let language = self.language.clone();
        let task = TaskId::new(format!("settings-system-time-query-{}", job.0.id)).unwrap();
        let worker = group.spawn_thread(TaskSpec::one_shot(task), move || {
            let _language = i18n::enter_snapshot(language.clone());
            let mut query = ManagementQuery::new(ManagementKind::SystemSettings);
            query.scope = "time".into();
            let result = platform::management::query(&query, &cancelled);
            if let Some(output) = output.upgrade() {
                *output.result.lock().unwrap_or_else(|e| e.into_inner()) = Some(result);
            }
        });
        self.install_system_time_job(job, worker);
    }

    fn install_system_time_job(
        &mut self,
        job: TimeJob,
        worker: Result<ManagedThreadHandle<()>, watchdog::WatchdogError>,
    ) {
        match worker {
            Ok(worker) => {
                *job.0.worker.lock().unwrap_or_else(|e| e.into_inner()) = Some(worker);
                self.register_page_task(
                    TaskDomain::SettingsSystemTime,
                    job.0.id,
                    if job.0.operation {
                        TaskClass::Operation
                    } else {
                        TaskClass::Query
                    },
                    job.control(),
                );
                if let Some(state) = self.pages.settings.settings_state.as_mut() {
                    if job.0.operation
                        && let Some(editor) = state.time_sync_server_editor.as_mut()
                    {
                        editor.validating = true;
                    }
                    state.system_time.job = Some(job);
                    state.system_time.error = None;
                    state.status = i18n::msg!("management-working").into();
                }
            }
            Err(error) => {
                let message = error.to_string();
                if let Some(state) = self.pages.settings.settings_state.as_mut() {
                    state.system_time.error = Some(message.clone().into());
                }
                self.set_settings_error(message);
            }
        }
    }

    fn submit_system_time_command(&mut self, command: ManagementCommand) {
        if !self.can_change_global_settings() {
            self.set_settings_error(i18n::msg!("settings-admin-required"));
            return;
        }
        if self
            .pages
            .settings
            .settings_state
            .as_ref()
            .is_none_or(|state| state.system_time.busy())
        {
            return;
        }
        let Some(group) = self.settings_task_runtime.shared.task_group.clone() else {
            self.set_settings_error(i18n::msg!("management-worker-unavailable"));
            return;
        };
        let (tx, rx) = mpsc::channel();
        let Some(aa) = self.begin_auto_admin(i18n::tr!("settings-time-sync"), true, tx) else {
            return;
        };
        #[cfg(target_os = "linux")]
        aa.enable_helper_control();
        let job = TimeJob::new(Some(aa.clone()));
        let output = Arc::downgrade(&job.0);
        let cancelled = job.0.cancelled.clone();
        #[cfg(target_os = "linux")]
        let authority = self.privilege_session.clone();
        let task = TaskId::new(format!("settings-system-time-write-{}", job.0.id)).unwrap();
        let worker_aa = aa.clone();
        let worker = crate::session::controller::auto_admin::spawn_task(
            &group,
            task,
            self.language.clone(),
            Some(&aa),
            move || {
                let emit = |event| {
                    worker_aa.emit(&event);
                    let result = match event {
                        OperationEvent::Snapshot { snapshot } => Some(Ok(snapshot)),
                        OperationEvent::Failed { message }
                        | OperationEvent::Disconnected { message } => {
                            Some(Err(ManagementError::Failed(message)))
                        }
                        _ => None,
                    };
                    if let Some(result) = result
                        && let Some(output) = output.upgrade()
                    {
                        *output.result.lock().unwrap_or_else(|e| e.into_inner()) = Some(result);
                    }
                };
                let result = worker_aa.run_approved(std::convert::identity, || {
                    platform::management::client::run(
                        #[cfg(target_os = "linux")]
                        &authority,
                        Some(command),
                        true,
                        None,
                        rx,
                        &cancelled,
                        &emit,
                    )
                });
                if let Err(error) = result
                    && let Some(output) = output.upgrade()
                {
                    *output.result.lock().unwrap_or_else(|e| e.into_inner()) = Some(Err(error));
                }
            },
        );
        self.install_system_time_job(job, worker);
    }

    pub(super) fn poll_system_time_settings(&mut self) {
        for page in self.pages.settings.ids() {
            self.with_page_context(page, |session| session.poll_system_time_page());
        }
    }

    pub(in crate::session) fn detach_system_time_page(&mut self, page: PageInstanceId) {
        if self.page_screen(page) != Some(ShellScreen::Settings) {
            return;
        }
        self.with_page_context(page, |session| {
            if let Some(id) = session
                .pages
                .settings
                .settings_state
                .as_ref()
                .and_then(|state| state.system_time.job.as_ref().map(|job| job.0.id))
            {
                session.tasks.complete(TaskDomain::SettingsSystemTime, id);
            }
        });
    }

    fn poll_system_time_page(&mut self) {
        let Some(job) = self
            .pages
            .settings
            .settings_state
            .as_ref()
            .and_then(|state| state.system_time.job.clone())
        else {
            return;
        };
        if !job.finished() {
            return;
        }
        let owner = self.tasks.owner(TaskDomain::SettingsSystemTime, job.0.id);
        self.tasks
            .complete(TaskDomain::SettingsSystemTime, job.0.id);
        if owner != self.pages.common.selected_id().map(TaskOwner::Page) {
            return;
        }
        let result = job
            .0
            .result
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take()
            .unwrap_or_else(|| {
                Err(ManagementError::Failed(i18n::tr!(
                    "system-settings-verify-failed"
                )))
            });
        let Some(state) = self.pages.settings.settings_state.as_mut() else {
            return;
        };
        state.system_time.job = None;
        match result {
            Ok(snapshot) => {
                state.system_time.snapshot = Some(snapshot);
                state.system_time.error = None;
                state.status = i18n::msg!("settings-ready").into();
                if job.0.operation {
                    state.picker = None;
                    state.time_sync_server_editor = None;
                    state.system_time.server_field = None;
                    state.status = i18n::msg!("system-settings-completed").into();
                }
            }
            Err(error) => {
                let message = i18n::LocalizedText::Raw(error.to_string());
                state.system_time.error = Some(message.clone());
                state.status = message.clone();
                if let Some(editor) = state.time_sync_server_editor.as_mut() {
                    editor.validating = false;
                    editor.error = Some(message);
                }
            }
        }
        self.refresh_hit_map();
    }

    pub(super) fn set_system_timezone(&mut self, timezone: String) {
        let Some(mut command) = self
            .pages
            .settings
            .settings_state
            .as_ref()
            .and_then(|state| state.system_time.command("set_system_timezone"))
        else {
            return;
        };
        command.values.insert("timezone".into(), timezone);
        self.submit_system_time_command(command);
    }

    pub(super) fn toggle_system_time_sync(&mut self) {
        let Some(mut command) = self
            .pages
            .settings
            .settings_state
            .as_ref()
            .and_then(|state| state.system_time.command("set_system_ntp"))
        else {
            return;
        };
        let enabled = command
            .values
            .get("enabled")
            .is_some_and(|value| value == "true");
        command
            .values
            .insert("enabled".into(), (!enabled).to_string());
        self.submit_system_time_command(command);
    }

    pub(super) fn open_system_time_servers(&mut self) {
        let Some(state) = self.pages.settings.settings_state.as_ref() else {
            return;
        };
        let Some((_, action)) = state.system_time.action("set_system_ntp_servers") else {
            if let Some(command) = state.system_time.command("inspect_system_settings") {
                self.submit_system_time_command(command);
            }
            return;
        };
        let options = action
            .fields
            .iter()
            .map(|field| (field.label.clone(), field.value.clone()))
            .collect();
        if let Some(state) = self.pages.settings.settings_state.as_mut() {
            state.system_time.server_field = None;
            state.picker = Some(SettingsPickerState {
                editing: Default::default(),
                kind: ui::SettingsPickerKind::SystemTimeServers,
                query: String::new(),
                selected_index: 0,
                window_start: 0,
                image_icons_supported: false,
                options,
            });
        }
    }

    pub(super) fn edit_system_time_server(&mut self, index: usize) {
        let Some(state) = self.pages.settings.settings_state.as_ref() else {
            return;
        };
        let Some(field) = state
            .system_time
            .action("set_system_ntp_servers")
            .and_then(|(_, action)| action.fields.get(index))
            .cloned()
        else {
            return;
        };
        if let Some(state) = self.pages.settings.settings_state.as_mut() {
            state.system_time.server_field = Some(field.id);
            state.picker = None;
            state.form_selected = 0;
            state.form_interaction = Default::default();
            state.time_sync_server_editor = Some(SettingsTimeSyncServerEditorState {
                editing: Default::default(),
                value: field.value,
                error: None,
                validating: false,
            });
        }
    }

    pub(super) fn save_system_time_server(&mut self, value: String) {
        let Some(state) = self.pages.settings.settings_state.as_ref() else {
            return;
        };
        let Some(field) = state.system_time.server_field.clone() else {
            return;
        };
        let Some(mut command) = state.system_time.command("set_system_ntp_servers") else {
            return;
        };
        command.values.insert(field, value);
        self.submit_system_time_command(command);
    }
}
