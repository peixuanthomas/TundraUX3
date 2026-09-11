use super::super::*;
impl ShellSession {
    pub(in crate::session) fn launcher_controller(&self) -> LauncherController {
        LauncherController::new(PermissionService::new(self.debug_policy))
    }

    pub(in crate::session) fn can_manage_launcher(&self) -> bool {
        matches!(
            self.app.auth_session().map(|session| session.role),
            Some(UserRole::Admin)
        )
    }

    pub(in crate::session) fn can_execute_command_line(&self) -> bool {
        PermissionService::new(self.debug_policy)
            .authorize(
                self.app.auth_session(),
                PermissionAction::ExecuteCommandLine,
                Some(app::COMMAND_LINE_APPLICATION.id),
            )
            .allowed
    }

    pub(in crate::session) fn built_in_launcher_count(&self) -> usize {
        self.built_in_launcher_applications().len()
    }

    pub(in crate::session) fn built_in_launcher_applications(
        &self,
    ) -> Vec<app::BuiltInApplicationDescriptor> {
        app::BUILT_IN_LAUNCHER_APPLICATIONS
            .iter()
            .copied()
            .filter(|descriptor| !descriptor.admin_only || self.can_execute_command_line())
            .collect()
    }

    pub(in crate::session) fn launcher_item_count(&self) -> usize {
        self.built_in_launcher_count()
            + self
                .app
                .launcher_state()
                .map(|state| state.items.len())
                .unwrap_or(0)
    }

    pub(in crate::session) fn selected_external_launcher_index(&self) -> Option<usize> {
        self.launcher_selected_index
            .checked_sub(self.built_in_launcher_count())
            .filter(|index| {
                self.app
                    .launcher_state()
                    .is_some_and(|state| *index < state.items.len())
            })
    }

    pub(in crate::session) fn selected_built_in_launcher_application(
        &self,
    ) -> Option<app::BuiltInApplicationDescriptor> {
        self.built_in_launcher_applications()
            .get(self.launcher_selected_index)
            .copied()
    }

    pub(in crate::session) fn open_launcher(&mut self, platform: &dyn Platform) {
        if self.is_strict_guest() || self.app.auth_session().is_none() {
            self.error_message = Some(i18n::LocalizedText::from(i18n::msg!(
                "shell-login-required-to-use-launcher"
            )));
            return;
        }
        let Some(storage) = self.storage_manager.clone() else {
            self.error_message = Some(i18n::LocalizedText::from(i18n::msg!(
                "shell-storage-unavailable"
            )));
            return;
        };
        match self.launcher_controller().load(&storage) {
            Ok(state) => {
                self.app.dispatch_at(
                    app::AppCommand::SetLauncherState(Some(state)),
                    Instant::now(),
                );
            }
            Err(error) => {
                self.error_message = Some(error.localized().message.into());
                return;
            }
        }
        self.load_launcher_view_preference();
        self.refresh_launcher(platform);
        self.launcher_selected_index = self
            .launcher_selected_index
            .min(self.launcher_item_count().saturating_sub(1));
        if self.active_screen() != ShellScreen::Launcher {
            self.screen_stack.push(ShellScreen::Launcher);
        }
        self.focused_component = ShellComponent::Launcher;
        self.launcher_pending_confirmation = None;
        self.launcher_drag = None;
        self.notify_status(i18n::LocalizedText::from(i18n::msg!("shell-launcher")));
        self.refresh_hit_map();
    }

    pub(in crate::session) fn close_launcher(&mut self) {
        self.launcher_pending_confirmation = None;
        self.launcher_drag = None;
        if self.active_screen() == ShellScreen::Launcher {
            self.screen_stack.pop();
        }
        match self.active_screen() {
            ShellScreen::Explorer => {
                self.focused_component = ShellComponent::Explorer;
                self.notify_status(i18n::LocalizedText::from(i18n::msg!("shell-explorer")));
            }
            _ => {
                self.pop_to_home();
                self.notify_status(i18n::LocalizedText::from(i18n::msg!("shell-ready")));
            }
        }
        self.refresh_hit_map();
    }

    pub(in crate::session) fn launcher_preference_key(&self) -> Option<String> {
        self.app
            .auth_session()
            .map(|session| format!("launcher.view.{}", session.user_id))
    }

    pub(in crate::session) fn load_launcher_view_preference(&mut self) {
        let Some(key) = self.launcher_preference_key() else {
            return;
        };
        let Some(storage) = self.storage_manager.as_ref() else {
            return;
        };
        if let Ok(state) = storage.load_state() {
            self.launcher_view_mode = match state.values.get(&key).map(String::as_str) {
                Some("details") => app::launcher::LauncherViewMode::Details,
                _ => app::launcher::LauncherViewMode::LargeIcons,
            };
        }
    }

    pub(in crate::session) fn toggle_launcher_view(&mut self) {
        self.launcher_drag = None;
        self.launcher_view_mode = match self.launcher_view_mode {
            app::launcher::LauncherViewMode::LargeIcons => app::launcher::LauncherViewMode::Details,
            app::launcher::LauncherViewMode::Details => app::launcher::LauncherViewMode::LargeIcons,
        };
        let Some(key) = self.launcher_preference_key() else {
            return;
        };
        let Some(storage) = self.storage_manager.as_ref() else {
            return;
        };
        match storage.load_state() {
            Ok(mut state) => {
                state.values.insert(
                    key,
                    match self.launcher_view_mode {
                        app::launcher::LauncherViewMode::LargeIcons => "large_icons",
                        app::launcher::LauncherViewMode::Details => "details",
                    }
                    .to_string(),
                );
                if let Err(error) = storage.save_state(&state) {
                    self.notify_status(i18n::LocalizedText::from(i18n::msg!(
                        "shell-could-not-save-launcher-view-error",
                        error = error.to_string()
                    )));
                }
            }
            Err(error) => self.notify_status(i18n::LocalizedText::from(i18n::msg!(
                "shell-could-not-load-launcher-view-error",
                error = error.to_string()
            ))),
        }
    }

    pub(in crate::session) fn selected_launcher_id(&self) -> Option<String> {
        let external_index = self.selected_external_launcher_index()?;
        self.app
            .launcher_state()?
            .items
            .get(external_index)
            .map(|item| item.record.id.clone())
    }

    pub(in crate::session) fn update_launcher_state(
        &mut self,
        update: impl FnOnce(&mut LauncherState),
    ) {
        let Some(mut state) = self.app.launcher_state().cloned() else {
            return;
        };
        update(&mut state);
        self.app.dispatch_at(
            app::AppCommand::SetLauncherState(Some(state)),
            Instant::now(),
        );
    }

    pub(in crate::session) fn select_launcher_index(&mut self, index: usize) {
        let len = self.launcher_item_count();
        self.launcher_selected_index = index.min(len.saturating_sub(1));
    }

    pub(in crate::session) fn select_launcher_delta(&mut self, delta: isize) {
        let len = self.launcher_item_count();
        if len == 0 {
            return;
        }
        self.launcher_selected_index = self
            .launcher_selected_index
            .saturating_add_signed(delta)
            .min(len - 1);
    }

    pub(in crate::session) fn select_launcher_last(&mut self) {
        let last = self.launcher_item_count().saturating_sub(1);
        self.select_launcher_index(last);
    }

    pub(in crate::session) fn apply_launcher_command(
        &mut self,
        command: LauncherCommand,
        platform: &dyn Platform,
    ) {
        let Some(storage) = self.storage_manager.clone() else {
            self.error_message = Some(i18n::LocalizedText::from(i18n::msg!(
                "shell-storage-unavailable"
            )));
            return;
        };
        if self.app.launcher_state().is_none() {
            match self.launcher_controller().load(&storage) {
                Ok(state) => {
                    self.app.dispatch_at(
                        app::AppCommand::SetLauncherState(Some(state)),
                        Instant::now(),
                    );
                }
                Err(error) => {
                    self.error_message = Some(error.localized().message.into());
                    return;
                }
            }
        }
        let (_, effect) =
            self.app
                .dispatch_launcher_at(command, platform, &storage, Instant::now());
        self.handle_launcher_effect(effect, platform);
    }

    pub(in crate::session) fn handle_launcher_effect(
        &mut self,
        effect: LauncherEffect,
        platform: &dyn Platform,
    ) {
        match effect {
            LauncherEffect::None => {}
            LauncherEffect::OpenRequested { path, kind } => {
                let mut started = runtime_log::RuntimeLogEvent::new(
                    self.operation_log_context("ux.launcher", "launch_application"),
                    runtime_log::LogLevel::Info,
                    runtime_log::LogPhase::Started,
                    "Launching application",
                );
                started.source_path = Some(path.clone());
                let context = started.context.clone();
                record_shell_runtime_event(started);
                let result = platform.launch_approved(&path, kind);
                let mut completed = runtime_log::RuntimeLogEvent::new(
                    context,
                    if result.is_ok() {
                        runtime_log::LogLevel::Info
                    } else {
                        runtime_log::LogLevel::Error
                    },
                    if result.is_ok() {
                        runtime_log::LogPhase::Succeeded
                    } else {
                        runtime_log::LogPhase::Failed
                    },
                    "Application launch result",
                );
                completed.source_path = Some(path.clone());
                if let Err(error) = &result {
                    completed.error_code = Some("UX_LAUNCH_FAILED".into());
                    completed.os_error_code = error.raw_os_error().map(i64::from);
                    watchdog::capture_error(&mut completed, error);
                }
                record_shell_runtime_event(completed);
                self.update_launcher_state(|state| match result {
                    Ok(()) => {
                        state.message = Some(i18n::LocalizedText::from(i18n::msg!(
                            "shell-opened-arg1",
                            arg1 = path.display().to_string()
                        )));
                        state.error = None;
                    }
                    Err(error) => {
                        state.error = Some(error.to_string().into());
                        state.message = None;
                    }
                })
            }
            LauncherEffect::ConfirmationRequired { id, path, kind } => {
                self.launcher_confirm_selected = true;
                self.launcher_pending_confirmation =
                    Some(LauncherPendingConfirmation::Launch { id, path, kind });
            }
            LauncherEffect::Added(results) => {
                let added_ids = results
                    .iter()
                    .filter_map(|result| match &result.outcome {
                        LauncherAddOutcome::Added { id } => Some(id.clone()),
                        LauncherAddOutcome::Duplicate | LauncherAddOutcome::Rejected { .. } => None,
                    })
                    .collect::<Vec<_>>();
                if let Some(id) = added_ids.last()
                    && let Some(index) = self
                        .app
                        .launcher_state()
                        .and_then(|state| state.items.iter().position(|item| &item.record.id == id))
                {
                    self.launcher_selected_index =
                        index.saturating_add(self.built_in_launcher_count());
                }
                self.update_launcher_state(|state| {
                    let rejected = results.len().saturating_sub(added_ids.len());
                    state.message = Some(i18n::LocalizedText::from(i18n::msg!(
                        "shell-added-arg1-item-s-arg2",
                        arg1 = added_ids.len(),
                        arg2 = if rejected > 0 {
                            i18n::LocalizedText::from(i18n::msg!(
                                "shell-rejected-skipped",
                                rejected = rejected
                            ))
                        } else {
                            i18n::LocalizedText::from("")
                        }
                    )));
                });
            }
        }
    }

    pub(in crate::session) fn refresh_launcher(&mut self, platform: &dyn Platform) {
        if self.launcher_refresh_request.is_some() {
            self.update_launcher_state(|state| {
                state.message = Some(i18n::LocalizedText::from(i18n::msg!(
                    "shell-launcher-refresh-already-in-progress"
                )))
            });
            return;
        }
        if let Some(runtime) = self.launcher_task_runtime.as_ref() {
            let entries = self
                .app
                .launcher_state()
                .map(|state| {
                    state
                        .items
                        .iter()
                        .map(|item| item.record.clone())
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            match runtime.submit(entries) {
                Ok(request_id) => {
                    self.launcher_refresh_request = Some(request_id);
                    self.update_launcher_state(|state| {
                        state.error = None;
                        state.message = Some(i18n::LocalizedText::from(i18n::msg!(
                            "shell-checking-launcher-items"
                        )));
                        for item in &mut state.items {
                            item.status = if item.record.executable_kind.is_some() {
                                LauncherItemStatus::Checking
                            } else {
                                LauncherItemStatus::NeedsApproval
                            };
                        }
                    });
                }
                Err(error) => self.update_launcher_state(|state| state.error = Some(error)),
            }
            return;
        }
        self.apply_launcher_command(LauncherCommand::Refresh, platform);
    }

    pub(in crate::session) fn request_launcher_launch(&mut self, platform: &dyn Platform) {
        if let Some(application) = self.selected_built_in_launcher_application() {
            match application.id {
                id if id == app::COMMAND_LINE_APPLICATION.id => self.open_command_line(),
                id if id == app::EDITOR_APPLICATION.id => self.open_editor(),
                _ => self.notify_status(i18n::LocalizedText::from(i18n::msg!(
                    "shell-arg1-is-not-available-in-this-build",
                    arg1 = application.localized_name()
                ))),
            }
            return;
        }
        if let Some(id) = self.selected_launcher_id() {
            self.apply_launcher_command(LauncherCommand::RequestLaunch(id), platform);
        }
    }

    pub(in crate::session) fn request_launcher_remove(&mut self) {
        if !self.can_manage_launcher() {
            self.update_launcher_state(|state| {
                state.error = Some(i18n::LocalizedText::from(i18n::msg!(
                    "shell-only-administrators-can-manage-launcher-items"
                )))
            });
            return;
        }
        let Some(external_index) = self.selected_external_launcher_index() else {
            return;
        };
        let Some(item) = self
            .app
            .launcher_state()
            .and_then(|state| state.items.get(external_index))
        else {
            return;
        };
        self.launcher_pending_confirmation = Some(LauncherPendingConfirmation::Remove {
            ids: vec![item.record.id.clone()],
            label: item.record.path.clone(),
        });
        self.launcher_confirm_selected = true;
    }

    pub(in crate::session) fn confirm_launcher_action(&mut self, platform: &dyn Platform) {
        let Some(pending) = self.launcher_pending_confirmation.take() else {
            return;
        };
        match pending {
            LauncherPendingConfirmation::Launch { id, .. } => {
                self.apply_launcher_command(LauncherCommand::ConfirmLaunch(id), platform)
            }
            LauncherPendingConfirmation::Remove { ids, .. } => {
                self.apply_launcher_command(LauncherCommand::Remove(ids), platform)
            }
        }
    }

    pub(in crate::session) fn add_selected_explorer_to_launcher(
        &mut self,
        platform: &dyn Platform,
    ) {
        let paths = self
            .app
            .explorer_state()
            .map(ExplorerState::effective_selected_paths)
            .unwrap_or_default();
        if paths.is_empty() {
            return;
        }
        self.close_explorer_popup();
        self.apply_launcher_command(LauncherCommand::AddPaths(paths), platform);
    }

    pub(in crate::session) fn open_launcher_for_path(
        &mut self,
        path: std::path::PathBuf,
        platform: &dyn Platform,
    ) {
        self.open_launcher(platform);
        if let Some(index) = self.app.launcher_state().and_then(|state| {
            state.items.iter().position(|item| {
                let approved = std::path::Path::new(&item.record.path);
                if cfg!(windows) {
                    approved
                        .to_string_lossy()
                        .eq_ignore_ascii_case(&path.to_string_lossy())
                } else {
                    approved == path
                }
            })
        }) {
            self.launcher_selected_index = index.saturating_add(self.built_in_launcher_count());
        } else {
            self.update_launcher_state(|state| {
                state.error = Some(i18n::LocalizedText::from(i18n::msg!(
                    "shell-this-file-has-not-been-approved-in-launcher"
                )))
            });
        }
    }

    pub(in crate::session) fn activate_launcher_at(
        &mut self,
        coordinates: CellPosition,
        click: ClickKind,
        platform: &dyn Platform,
    ) {
        self.launcher_drag = None;
        let area = Rect::new(0, 0, self.terminal_size.0, self.terminal_size.1);
        let ui::ShellLayout::Full { main, .. } = ui::compute_shell_layout(area) else {
            return;
        };
        let model = self.to_launcher_view_model();
        match ui::launcher_layout(main, &model).hit_test(coordinates.0, coordinates.1) {
            Some(ui::LauncherHitTarget::Item(index)) => {
                self.select_launcher_index(index);
                if click == ClickKind::Double {
                    self.request_launcher_launch(platform);
                } else if self.launcher_view_mode == app::launcher::LauncherViewMode::LargeIcons
                    && self.can_manage_launcher()
                    && let Some(item_id) = self.selected_launcher_id()
                {
                    self.launcher_drag = Some(LauncherDragState {
                        item_id,
                        target: None,
                    });
                }
            }
            Some(ui::LauncherHitTarget::Toolbar(action)) => match action {
                ui::LauncherToolbarAction::Remove => self.request_launcher_remove(),
                ui::LauncherToolbarAction::Refresh => self.refresh_launcher(platform),
                ui::LauncherToolbarAction::ToggleView => self.toggle_launcher_view(),
            },
            Some(ui::LauncherHitTarget::Confirm) => self.confirm_launcher_action(platform),
            Some(ui::LauncherHitTarget::Cancel) => self.launcher_pending_confirmation = None,
            _ => {}
        }
    }

    pub(in crate::session) fn update_launcher_drag(&mut self, coordinates: CellPosition) {
        if self.launcher_view_mode != app::launcher::LauncherViewMode::LargeIcons {
            self.launcher_drag = None;
            return;
        }
        let area = Rect::new(0, 0, self.terminal_size.0, self.terminal_size.1);
        let ui::ShellLayout::Full { main, .. } = ui::compute_shell_layout(area) else {
            self.launcher_drag = None;
            return;
        };
        let model = self.to_launcher_view_model();
        let target =
            ui::launcher_layout(main, &model).large_icon_drop_target(coordinates.0, coordinates.1);
        if let Some(drag) = self.launcher_drag.as_mut() {
            drag.target = target;
        }
    }

    pub(in crate::session) fn drop_launcher_drag(
        &mut self,
        coordinates: CellPosition,
        platform: &dyn Platform,
    ) {
        let Some(drag) = self.launcher_drag.take() else {
            return;
        };
        if self.launcher_view_mode != app::launcher::LauncherViewMode::LargeIcons {
            return;
        }
        let area = Rect::new(0, 0, self.terminal_size.0, self.terminal_size.1);
        let ui::ShellLayout::Full { main, .. } = ui::compute_shell_layout(area) else {
            return;
        };
        let model = self.to_launcher_view_model();
        let Some(target) =
            ui::launcher_layout(main, &model).large_icon_drop_target(coordinates.0, coordinates.1)
        else {
            return;
        };
        let insertion_index = target
            .insertion_index()
            .saturating_sub(self.built_in_launcher_count());
        self.apply_launcher_command(
            LauncherCommand::Reorder {
                id: drag.item_id.clone(),
                insertion_index,
            },
            platform,
        );
        if let Some(index) = self.app.launcher_state().and_then(|state| {
            state
                .items
                .iter()
                .position(|item| item.record.id == drag.item_id)
        }) {
            self.launcher_selected_index = index.saturating_add(self.built_in_launcher_count());
        }
    }

    pub fn to_launcher_view_model(&self) -> ui::LauncherViewModel {
        let _language = i18n::enter_snapshot(self.language.clone());
        let built_in_applications = self.built_in_launcher_applications();
        let built_in_count = built_in_applications.len();
        let mut items = built_in_applications
            .into_iter()
            .enumerate()
            .map(|(index, descriptor)| {
                let mut item = ui::LauncherItemViewModel::built_in(descriptor);
                item.name = descriptor.localized_name().render_current();
                item.path = descriptor.localized_description().render_current();
                item.type_label = descriptor.localized_type_label().render_current();
                item.selected = self.launcher_selected_index == index;
                item
            })
            .collect::<Vec<_>>();
        if let Some(state) = self.app.launcher_state() {
            items.extend(
                state
                    .items
                    .iter()
                    .enumerate()
                    .map(|(external_index, item)| {
                        let path = std::path::Path::new(&item.record.path);
                        let name = path
                            .file_name()
                            .and_then(|name| name.to_str())
                            .unwrap_or(&item.record.path);
                        let type_label = match item.record.executable_kind {
                            Some(LauncherExecutableKind::NativeBinary) => {
                                i18n::tr!("shell-application")
                            }
                            Some(LauncherExecutableKind::Installer) => i18n::tr!("shell-installer"),
                            Some(LauncherExecutableKind::Script) => i18n::tr!("shell-script"),
                            Some(LauncherExecutableKind::Shortcut) => i18n::tr!("shell-shortcut"),
                            Some(LauncherExecutableKind::ApplicationBundle) => {
                                i18n::tr!("shell-application-bundle")
                            }
                            None => i18n::tr!("shell-unknown"),
                        };
                        let mut model = ui::LauncherItemViewModel::new(
                            item.record.id.clone(),
                            name,
                            item.record.path.clone(),
                            type_label,
                            item.status,
                        );
                        model.selected = external_index.saturating_add(built_in_count)
                            == self.launcher_selected_index;
                        model
                    })
                    .collect::<Vec<_>>(),
            );
        }
        let selected = items
            .len()
            .checked_sub(1)
            .map(|last_index| self.launcher_selected_index.min(last_index));
        let mut model = ui::LauncherViewModel::with_ascii_assets(
            items,
            selected,
            self.launcher_view_mode,
            self.can_manage_launcher(),
            self.ascii_assets.clone(),
        );
        model.viewport_offset = self.launcher_viewport_offset;
        model.drop_target = self.launcher_drag.as_ref().and_then(|drag| drag.target);
        if let Some(state) = self.app.launcher_state() {
            model.message = state
                .message
                .as_ref()
                .map(i18n::LocalizedText::render_current);
            model.error = state
                .error
                .as_ref()
                .map(i18n::LocalizedText::render_current);
        }
        model.confirmation =
            self.launcher_pending_confirmation
                .as_ref()
                .map(|pending| match pending {
                    LauncherPendingConfirmation::Launch { path, kind, .. } => {
                        ui::LauncherConfirmationViewModel {
                            kind: ui::LauncherConfirmationKind::Launch,
                            title: i18n::tr!("shell-confirm-launch"),
                            message: i18n::tr!(
                                "shell-open-arg1-kind-with-the-system-default-handler",
                                arg1 = path.display().to_string(),
                                kind = format!("{:?}", kind)
                            ),
                            confirm_label: i18n::tr!("shell-launch"),
                            cancel_label: i18n::tr!("shell-cancel"),
                            confirm_selected: self.launcher_confirm_selected,
                        }
                    }
                    LauncherPendingConfirmation::Remove { label, .. } => {
                        ui::LauncherConfirmationViewModel {
                            kind: ui::LauncherConfirmationKind::Remove,
                            title: i18n::tr!("shell-remove-from-launcher"),
                            message: i18n::tr!(
                                "shell-remove-label-from-launcher-the-file-will-not-be-deleted",
                                label = label
                            ),
                            confirm_label: i18n::tr!("shell-remove"),
                            cancel_label: i18n::tr!("shell-cancel"),
                            confirm_selected: self.launcher_confirm_selected,
                        }
                    }
                });
        model
    }
}
