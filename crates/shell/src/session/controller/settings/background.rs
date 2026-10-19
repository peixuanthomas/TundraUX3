use super::*;
use crate::session::*;

impl ShellSession {
    pub(in crate::session) fn cancel_update(&mut self) {
        self.update_restart_requested = false;
        match self.settings_task_runtime.cancel_update() {
            Ok(true) => {
                self.settings_update_state.error = None;
                // Clear the restart request immediately. The worker remains
                // registered until its processes and private files are gone.
                self.update_apply_manifest = None;
                self.settings_update_state.confirmation_open = false;
                self.settings_update_state.busy = true;
                self.settings_update_state.phase = Some(app::update::UpdatePhase::Cancelling);
                self.settings_update_state.status = i18n::msg!("settings-update-cancelling").into();
            }
            Ok(false) => {}
            Err(error) => {
                self.update_apply_manifest = None;
                // A durable cancellation can fail to start cleanup. In that case
                // the runtime has already released its record and sends no event.
                if self.settings_task_runtime.active_update_id().is_none()
                    && let Some(request) = self.settings_update_request.take()
                {
                    self.tasks.complete(TaskDomain::Update, request);
                }
                self.set_update_error(error);
            }
        }
    }

    pub(in crate::session) fn switch_update_mode(&mut self) {
        if !cfg!(target_os = "linux")
            || !self.can_change_global_settings()
            || self.settings_update_state.busy
            || self.settings_task_runtime.update_busy()
        {
            return;
        }
        let Some(storage) = self.storage_manager.clone() else {
            self.set_update_error(i18n::msg!("settings-storage-unavailable"));
            return;
        };
        let result = (|| {
            let mut config = storage.load_config()?;
            config.linux_update_mode = match config.linux_update_mode {
                storage::LinuxUpdateMode::Release => storage::LinuxUpdateMode::Beta,
                storage::LinuxUpdateMode::Beta => storage::LinuxUpdateMode::Release,
            };
            self.save_settings_config_logged(&storage, &config)?;
            Ok::<_, storage::StorageError>(config)
        })();
        match result {
            Ok(config) => {
                self.settings_update_state = SettingsUpdateState {
                    mode: config.linux_update_mode,
                    ..Default::default()
                };
                self.replace_storage_config(config);
                self.begin_update_check();
            }
            Err(error) => self.set_update_error(error.to_string()),
        }
    }

    pub(in crate::session) fn begin_update_check(&mut self) {
        self.settings_update_state.checked_once = true;
        self.settings_update_state.confirmation_open = false;
        self.settings_update_state.error = None;
        if !self.settings_task_runtime.update_supported() {
            self.settings_update_state.status = i18n::msg!("settings-update-unsupported").into();
            self.settings_update_state.phase = None;
            return;
        }
        if self.settings_update_state.busy || self.settings_task_runtime.update_busy() {
            self.settings_update_state.status = i18n::msg!("settings-update-running").into();
            return;
        }
        self.settings_update_state.check_result = None;
        self.settings_update_state.checked_at = None;
        match self.settings_task_runtime.submit_update_check(
            app::update::current_build_identity(),
            self.settings_update_state.mode,
        ) {
            Ok(()) => {
                self.register_settings_update(TaskClass::Query);
                self.settings_update_state.busy = true;
                self.settings_update_state.phase = Some(app::update::UpdatePhase::Checking);
                self.settings_update_state.status = i18n::msg!("settings-checking-github").into();
                self.notify_status(i18n::msg!("settings-checking-updates"));
            }
            Err(error) => self.set_update_error(error),
        }
    }

    pub(in crate::session) fn open_update_confirmation(&mut self) {
        if !self.settings_task_runtime.update_supported() {
            self.set_update_error(i18n::msg!("settings-update-unsupported"));
            return;
        }
        if !self.can_change_global_settings() {
            self.set_update_error(i18n::msg!("settings-update-admin-required"));
            return;
        }
        if self.settings_update_state.busy {
            self.set_update_error(i18n::msg!("settings-update-wait"));
            return;
        }
        let Some(check) = self.settings_update_state.check_result.as_ref() else {
            self.set_update_error(i18n::msg!("settings-update-check-first"));
            return;
        };
        let identity = app::update::current_build_identity();
        if matches!(check.relation, app::update::UpdateRelation::Identical) && !identity.dirty {
            self.settings_update_state.status = i18n::msg!("settings-update-current-build").into();
            return;
        }
        self.settings_update_state.confirmation_open = true;
        self.settings_update_state.confirm_selected = true;
    }

    pub(in crate::session) fn cancel_update_confirmation(&mut self) {
        self.settings_update_state.confirmation_open = false;
        self.settings_update_state.confirm_selected = true;
        self.settings_update_state.status = i18n::msg!("settings-update-cancelled").into();
    }

    pub(in crate::session) fn begin_confirmed_update(&mut self) {
        if !self.settings_update_state.confirmation_open {
            return;
        }
        self.settings_update_state.confirmation_open = false;
        if !self.can_change_global_settings() {
            self.set_update_error(i18n::msg!("settings-update-admin-required"));
            return;
        }
        let Some(check) = self.settings_update_state.check_result.clone() else {
            self.set_update_error(i18n::msg!("settings-update-check-expired"));
            return;
        };
        let install_dir = match std::env::current_exe()
            .ok()
            .and_then(|path| path.parent().map(std::path::Path::to_path_buf))
        {
            Some(path) => path,
            None => {
                self.set_update_error(i18n::msg!("settings-installation-unavailable"));
                return;
            }
        };
        match self
            .settings_task_runtime
            .submit_update_prepare(check, install_dir)
        {
            Ok(()) => {
                self.register_settings_update(TaskClass::Operation);
                self.settings_update_state.busy = true;
                self.settings_update_state.error = None;
                self.settings_update_state.activity =
                    Some(ui::components::UpdateActivityViewModel::default());
                self.settings_update_state.phase = Some(app::update::UpdatePhase::Downloading);
                self.settings_update_state.status =
                    i18n::msg!("settings-update-downloading").into();
                self.notify_status(i18n::msg!("settings-update-started"));
            }
            Err(error) => self.set_update_error(error),
        }
    }

    pub(in crate::session) fn set_update_error(&mut self, message: impl Into<i18n::LocalizedText>) {
        self.update_restart_requested = false;
        let message = message.into();
        // Keep backend diagnostics intact while adding context to the UI.
        let diagnostic = match &message {
            i18n::LocalizedText::Raw(raw) => raw.clone(),
            i18n::LocalizedText::Message(message) => format!("{} {:?}", message.id, message.args),
        };
        let message = i18n::LocalizedText::from(i18n::msg!(
            "startup-update-error-explanation",
            details = message
        ));
        self.settings_update_state.busy = false;
        self.settings_update_state.phase = Some(app::update::UpdatePhase::Failed);
        self.settings_update_state.status =
            i18n::msg!("settings-update-failed", reason = message.clone()).into();
        self.settings_update_state.error = Some(message.clone());
        self.settings_update_state
            .append_output(&format!("ERROR: {diagnostic}"));
        self.notify_status(i18n::msg!("settings-update-failed", reason = message));
    }

    fn register_settings_update(&mut self, class: TaskClass) {
        if let Some(id) = self.settings_task_runtime.active_update_id() {
            let control = self.settings_task_runtime.update_task_control();
            self.register_page_task(TaskDomain::Update, id, class, control);
            self.settings_update_request = Some(id);
        }
    }

    pub(in crate::session) fn poll_settings_background_tasks(&mut self) {
        let events = self
            .settings_task_runtime
            .drain_time_sync_validation_events();
        for event in events {
            let request = event.request_id;
            let owner = self.tasks.owner(TaskDomain::SettingsTimeSync, request);
            self.tasks.complete(TaskDomain::SettingsTimeSync, request);
            if let Some(TaskOwner::Page(owner)) = owner {
                self.with_page_context(owner, |session| session.apply_time_validation_event(event));
            }
        }

        for event in self.settings_task_runtime.drain_update_events() {
            let request = self.settings_update_request;
            let owner = request.and_then(|id| self.tasks.owner(TaskDomain::Update, id));
            let prepared = matches!(&event, SettingsUpdateTaskEvent::PrepareCompleted(Ok(_)));
            if let Some(error) = self.settings_task_runtime.update_cancel_error() {
                if let Some(TaskOwner::Page(page)) = owner {
                    self.pending_page_closes.remove(&page);
                }
                self.pending_login_reset = None;
                self.pending_lockscreen_after_logout = false;
                self.pending_session_exit = None;
                self.set_update_error(error);
                continue;
            }
            let done = !matches!(&event, SettingsUpdateTaskEvent::Progress(_)) && !prepared;
            if done && let Some(id) = request {
                self.tasks.complete(TaskDomain::Update, id);
                self.settings_update_request = None;
            }
            if request.is_some() && owner.is_none() {
                if done {
                    self.settings_update_state.busy = false;
                }
                continue;
            }
            match event {
                SettingsUpdateTaskEvent::Progress(progress) => {
                    self.settings_update_state.apply_progress(progress);
                }
                SettingsUpdateTaskEvent::CheckCompleted(Ok(result)) => {
                    self.settings_update_state.busy = false;
                    self.settings_update_state.phase = None;
                    self.settings_update_state.error = None;
                    self.settings_update_state.checked_at = Some(Utc::now());
                    self.settings_update_state.status = checked_update_label(&result).into();
                    self.settings_update_state.check_result = Some(result);
                }
                SettingsUpdateTaskEvent::CheckCompleted(Err(error))
                | SettingsUpdateTaskEvent::PrepareCompleted(Err(error)) => {
                    self.set_update_error(error);
                }
                SettingsUpdateTaskEvent::PreparationFailed(error) => {
                    self.set_update_error(error.to_string());
                }
                SettingsUpdateTaskEvent::PrepareCompleted(Ok(manifest_path)) => {
                    self.settings_update_state.busy = false;
                    self.settings_update_state.phase =
                        Some(app::update::UpdatePhase::WaitingForRestart);
                    self.settings_update_state.status = i18n::msg!("progress-phase-restart").into();
                    self.update_apply_manifest = Some(manifest_path);
                }
                SettingsUpdateTaskEvent::Cancelled => {
                    self.update_apply_manifest = None;
                    self.settings_update_state.busy = false;
                    self.settings_update_state.phase = Some(app::update::UpdatePhase::Cancelled);
                    self.settings_update_state.error = None;
                    self.settings_update_state.status =
                        i18n::msg!("settings-update-cancelled").into();
                    self.notify_status(i18n::msg!("settings-update-cancelled"));
                }
            }
        }
        // Keep a prepared update until AA finishes or its confirmation is rejected.
        if self.update_apply_manifest.is_some() && !self.auto_admin_running() {
            self.settings_update_state.status = i18n::msg!("settings-update-restarting").into();
            self.update_restart_requested = true;
        }
    }

    fn apply_time_validation_event(&mut self, event: SettingsTimeSyncValidationEvent) {
        let active = self
            .pages
            .settings
            .settings_state
            .as_ref()
            .is_some_and(|state| state.time_sync_validation_request_id == Some(event.request_id));
        if !active {
            return;
        }
        if let Some(state) = self.pages.settings.settings_state.as_mut() {
            state.time_sync_validation_request_id = None;
            if let Some(editor) = state.time_sync_server_editor.as_mut() {
                editor.validating = false;
            }
        }
        match event.result {
            Ok(utc) => self.persist_validated_time_sync_config(event.config, utc),
            Err(error) => {
                let message = match event.config.server_url.as_deref() {
                    Some(server) => i18n::msg!(
                        "settings-server-sync-failed",
                        server = server,
                        reason = error.to_string()
                    ),
                    None => {
                        i18n::msg!("settings-default-sync-failed", reason = error.to_string())
                    }
                };
                if let Some(state) = self.pages.settings.settings_state.as_mut() {
                    state.status = i18n::msg!("settings-sync-test-failed").into();
                    if let Some(editor) = state.time_sync_server_editor.as_mut() {
                        editor.error = Some(i18n::msg!("settings-sync-review-error").into());
                    }
                }
                self.show_time_sync_failure_dialog(message);
            }
        }
    }

    pub(in crate::session) fn persist_validated_time_sync_config(
        &mut self,
        time_sync: storage::TimeSyncConfig,
        utc: DateTime<Utc>,
    ) {
        let Some(storage) = self.storage_manager.clone() else {
            self.set_settings_error(i18n::msg!("settings-storage-unavailable"));
            return;
        };
        let mut config = match storage.load_config() {
            Ok(config) => config,
            Err(error) => {
                self.set_settings_error(i18n::msg!(
                    "settings-load-failed",
                    reason = error.to_string()
                ));
                return;
            }
        };
        config.time_sync = time_sync;
        if let Err(error) = self.save_settings_config_logged(&storage, &config) {
            self.set_settings_error(i18n::msg!(
                "settings-save-failed",
                reason = error.to_string()
            ));
            return;
        }
        self.replace_storage_config(config);
        self.apply_time_sync_utc(utc);
        if let Some(state) = self.pages.settings.settings_state.as_mut() {
            state.time_sync_server_editor = None;
            state.time_sync_validation_request_id = None;
            state.status = i18n::msg!("settings-saved-time-sync").into();
        }
        self.notify_status(i18n::msg!("settings-saved-time-sync"));
    }

    pub(in crate::session) fn change_time_sync_source(&mut self, platform: &dyn Platform) {
        if !self.can_change_global_settings() {
            self.set_settings_error(i18n::msg!("settings-admin-required"));
            return;
        }
        let current = self.app.storage_config().time_sync.clone();
        match current.source {
            storage::TimeSyncSource::NetworkServer => match platform.system_time() {
                Ok(system_time) => {
                    let mut config = current;
                    config.source = storage::TimeSyncSource::OperatingSystem;
                    self.persist_validated_time_sync_config(
                        config,
                        DateTime::<Utc>::from(system_time),
                    );
                }
                Err(error) => self.show_time_sync_failure_dialog(i18n::msg!(
                    "settings-system-time-failed",
                    reason = error.to_string()
                )),
            },
            storage::TimeSyncSource::OperatingSystem => {
                let mut config = current;
                config.source = storage::TimeSyncSource::NetworkServer;
                self.begin_settings_time_sync_validation(config);
            }
        }
    }

    pub(in crate::session) fn begin_settings_time_sync_validation(
        &mut self,
        config: storage::TimeSyncConfig,
    ) {
        if self
            .pages
            .settings
            .settings_state
            .as_ref()
            .is_some_and(|state| state.time_sync_validation_request_id.is_some())
        {
            self.set_settings_error(i18n::msg!("settings-validation-running"));
            return;
        }
        match self
            .settings_task_runtime
            .submit_time_sync_validation(config)
        {
            Ok(request_id) => {
                let control = self
                    .settings_task_runtime
                    .validation_task_control(request_id);
                self.register_page_task(
                    TaskDomain::SettingsTimeSync,
                    request_id,
                    TaskClass::Query,
                    control,
                );
                if let Some(state) = self.pages.settings.settings_state.as_mut() {
                    state.time_sync_validation_request_id = Some(request_id);
                    state.status = i18n::msg!("settings-testing-time").into();
                    if let Some(editor) = state.time_sync_server_editor.as_mut() {
                        editor.validating = true;
                        editor.error = None;
                    }
                }
                self.notify_status(i18n::msg!("settings-testing-time"));
            }
            Err(error) => self.show_time_sync_failure_dialog(error),
        }
    }
}
