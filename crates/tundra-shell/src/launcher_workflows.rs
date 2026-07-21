impl ShellState {
    fn launcher_controller(&self) -> LauncherController {
        LauncherController::new(PermissionService::new(self.debug_policy))
    }

    fn can_manage_launcher(&self) -> bool {
        matches!(
            self.auth_session.as_ref().map(|session| session.role),
            Some(UserRole::Admin)
        )
    }

    fn open_launcher(&mut self, platform: &dyn Platform) {
        if self.is_strict_guest() || self.auth_session.is_none() {
            self.error_message = Some("Login required to use Launcher".to_string());
            return;
        }
        let Some(storage) = self.storage_manager.clone() else {
            self.error_message = Some("Storage unavailable".to_string());
            return;
        };
        match self.launcher_controller().load(&storage) {
            Ok(state) => self.launcher_state = Some(state),
            Err(error) => {
                self.error_message = Some(error.to_string());
                return;
            }
        }
        self.load_launcher_view_preference();
        self.refresh_launcher(platform);
        self.launcher_selected_index = self.launcher_selected_index.min(
            self.launcher_state
                .as_ref()
                .map(|state| state.items.len().saturating_sub(1))
                .unwrap_or(0),
        );
        if self.active_screen() != ShellScreen::Launcher {
            self.screen_stack.push(ShellScreen::Launcher);
        }
        self.focused_component = ShellComponent::Launcher;
        self.launcher_pending_confirmation = None;
        self.launcher_drag = None;
        self.notify_status("Launcher");
        self.refresh_hit_map();
    }

    fn close_launcher(&mut self) {
        self.launcher_pending_confirmation = None;
        self.launcher_drag = None;
        if self.active_screen() == ShellScreen::Launcher {
            self.screen_stack.pop();
        }
        match self.active_screen() {
            ShellScreen::Explorer => {
                self.focused_component = ShellComponent::Explorer;
                self.notify_status("Explorer");
            }
            _ => {
                self.pop_to_home();
                self.notify_status("Ready");
            }
        }
        self.refresh_hit_map();
    }

    fn launcher_preference_key(&self) -> Option<String> {
        self.auth_session
            .as_ref()
            .map(|session| format!("launcher.view.{}", session.user_id))
    }

    fn load_launcher_view_preference(&mut self) {
        let Some(key) = self.launcher_preference_key() else { return };
        let Some(storage) = self.storage_manager.as_ref() else { return };
        if let Ok(state) = storage.load_state() {
            self.launcher_view_mode = match state.values.get(&key).map(String::as_str) {
                Some("details") => tundra_ui::LauncherViewMode::Details,
                _ => tundra_ui::LauncherViewMode::LargeIcons,
            };
        }
    }

    fn toggle_launcher_view(&mut self) {
        self.launcher_drag = None;
        self.launcher_view_mode = match self.launcher_view_mode {
            tundra_ui::LauncherViewMode::LargeIcons => tundra_ui::LauncherViewMode::Details,
            tundra_ui::LauncherViewMode::Details => tundra_ui::LauncherViewMode::LargeIcons,
        };
        let Some(key) = self.launcher_preference_key() else { return };
        let Some(storage) = self.storage_manager.as_ref() else { return };
        match storage.load_state() {
            Ok(mut state) => {
                state.values.insert(
                    key,
                    match self.launcher_view_mode {
                        tundra_ui::LauncherViewMode::LargeIcons => "large_icons",
                        tundra_ui::LauncherViewMode::Details => "details",
                    }
                    .to_string(),
                );
                if let Err(error) = storage.save_state(&state) {
                    self.notify_status(format!("Could not save Launcher view: {error}"));
                }
            }
            Err(error) => self.notify_status(format!("Could not load Launcher view: {error}")),
        }
    }

    fn selected_launcher_id(&self) -> Option<String> {
        self.launcher_state
            .as_ref()?
            .items
            .get(self.launcher_selected_index)
            .map(|item| item.record.id.clone())
    }

    fn select_launcher_index(&mut self, index: usize) {
        let len = self.launcher_state.as_ref().map(|state| state.items.len()).unwrap_or(0);
        self.launcher_selected_index = index.min(len.saturating_sub(1));
    }

    fn select_launcher_delta(&mut self, delta: isize) {
        let len = self.launcher_state.as_ref().map(|state| state.items.len()).unwrap_or(0);
        if len == 0 { return; }
        self.launcher_selected_index = self
            .launcher_selected_index
            .saturating_add_signed(delta)
            .min(len - 1);
    }

    fn select_launcher_last(&mut self) {
        let last = self.launcher_state.as_ref().map(|state| state.items.len().saturating_sub(1)).unwrap_or(0);
        self.select_launcher_index(last);
    }

    fn apply_launcher_command(&mut self, command: LauncherCommand, platform: &dyn Platform) {
        let Some(storage) = self.storage_manager.clone() else {
            self.error_message = Some("Storage unavailable".to_string());
            return;
        };
        if self.launcher_state.is_none() {
            match self.launcher_controller().load(&storage) {
                Ok(state) => self.launcher_state = Some(state),
                Err(error) => {
                    self.error_message = Some(error.to_string());
                    return;
                }
            }
        }
        let controller = self.launcher_controller();
        let effect = controller.apply(
            self.launcher_state.as_mut().expect("Launcher state loaded"),
            command,
            self.auth_session.as_ref(),
            platform,
            &storage,
        );
        self.handle_launcher_effect(effect, platform);
    }

    fn handle_launcher_effect(&mut self, effect: LauncherEffect, platform: &dyn Platform) {
        match effect {
            LauncherEffect::None => {}
            LauncherEffect::OpenRequested { path } => {
                if let Some(state) = self.launcher_state.as_mut() {
                    match platform.open_path(&path) {
                        Ok(()) => {
                            state.message = Some(format!("Opened {}", path.display()));
                            state.error = None;
                        }
                        Err(error) => {
                            state.error = Some(error.to_string());
                            state.message = None;
                        }
                    }
                }
            }
            LauncherEffect::ConfirmationRequired { id, path, kind } => {
                self.launcher_pending_confirmation = Some(
                    LauncherPendingConfirmation::Launch { id, path, kind },
                );
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
                    && let Some(index) = self.launcher_state.as_ref().and_then(|state| {
                        state.items.iter().position(|item| &item.record.id == id)
                    })
                {
                    self.launcher_selected_index = index;
                }
                if let Some(state) = self.launcher_state.as_mut() {
                    let rejected = results.len().saturating_sub(added_ids.len());
                    state.message = Some(format!(
                        "Added {} item(s){}",
                        added_ids.len(),
                        if rejected > 0 { format!(", {rejected} skipped") } else { String::new() },
                    ));
                }
            }
        }
    }

    fn refresh_launcher(&mut self, platform: &dyn Platform) {
        self.apply_launcher_command(LauncherCommand::Refresh, platform);
    }

    fn request_launcher_launch(&mut self, platform: &dyn Platform) {
        if let Some(id) = self.selected_launcher_id() {
            self.apply_launcher_command(LauncherCommand::RequestLaunch(id), platform);
        }
    }

    fn request_launcher_remove(&mut self) {
        if !self.can_manage_launcher() {
            if let Some(state) = self.launcher_state.as_mut() {
                state.error = Some("Only administrators can manage Launcher items".to_string());
            }
            return;
        }
        let Some(item) = self.launcher_state.as_ref().and_then(|state| state.items.get(self.launcher_selected_index)) else { return };
        self.launcher_pending_confirmation = Some(LauncherPendingConfirmation::Remove {
            ids: vec![item.record.id.clone()],
            label: item.record.path.clone(),
        });
    }

    fn reapprove_selected_launcher_item(&mut self, platform: &dyn Platform) {
        if let Some(id) = self.selected_launcher_id() {
            self.apply_launcher_command(LauncherCommand::Reapprove(vec![id]), platform);
        }
    }

    fn confirm_launcher_action(&mut self, platform: &dyn Platform) {
        let Some(pending) = self.launcher_pending_confirmation.take() else { return };
        match pending {
            LauncherPendingConfirmation::Launch { id, .. } => {
                self.apply_launcher_command(LauncherCommand::ConfirmLaunch(id), platform)
            }
            LauncherPendingConfirmation::Remove { ids, .. } => {
                self.apply_launcher_command(LauncherCommand::Remove(ids), platform)
            }
        }
    }

    fn add_selected_explorer_to_launcher(&mut self, platform: &dyn Platform) {
        let paths = self
            .explorer_state
            .as_ref()
            .map(ExplorerState::effective_selected_paths)
            .unwrap_or_default();
        if paths.is_empty() { return; }
        self.close_explorer_popup();
        self.apply_launcher_command(LauncherCommand::AddPaths(paths), platform);
    }

    fn open_launcher_for_path(&mut self, path: std::path::PathBuf, platform: &dyn Platform) {
        self.open_launcher(platform);
        if let Some(index) = self.launcher_state.as_ref().and_then(|state| {
            state.items.iter().position(|item| {
                let approved = std::path::Path::new(&item.record.path);
                if cfg!(windows) {
                    approved.to_string_lossy().eq_ignore_ascii_case(&path.to_string_lossy())
                } else {
                    approved == path
                }
            })
        }) {
            self.launcher_selected_index = index;
        } else if let Some(state) = self.launcher_state.as_mut() {
            state.error = Some("This file has not been approved in Launcher".to_string());
        }
    }

    fn activate_launcher_at(
        &mut self,
        coordinates: CellPosition,
        click: ClickKind,
        platform: &dyn Platform,
    ) {
        self.launcher_drag = None;
        let area = Rect::new(0, 0, self.terminal_size.0, self.terminal_size.1);
        let tundra_ui::ShellLayout::Full { main, .. } = tundra_ui::compute_shell_layout(area) else { return };
        let model = self.to_launcher_view_model();
        match tundra_ui::launcher_layout(main, &model).hit_test(coordinates.0, coordinates.1) {
            Some(tundra_ui::LauncherHitTarget::Item(index)) => {
                self.select_launcher_index(index);
                if click == ClickKind::Double {
                    self.request_launcher_launch(platform);
                } else if self.launcher_view_mode == tundra_ui::LauncherViewMode::LargeIcons
                    && self.can_manage_launcher()
                    && let Some(item_id) = self.selected_launcher_id()
                {
                    self.launcher_drag = Some(LauncherDragState {
                        item_id,
                        target: None,
                    });
                }
            }
            Some(tundra_ui::LauncherHitTarget::Toolbar(action)) => match action {
                tundra_ui::LauncherToolbarAction::Remove => self.request_launcher_remove(),
                tundra_ui::LauncherToolbarAction::Reapprove => self.reapprove_selected_launcher_item(platform),
                tundra_ui::LauncherToolbarAction::Refresh => self.refresh_launcher(platform),
                tundra_ui::LauncherToolbarAction::ToggleView => self.toggle_launcher_view(),
            },
            Some(tundra_ui::LauncherHitTarget::Confirm) => self.confirm_launcher_action(platform),
            Some(tundra_ui::LauncherHitTarget::Cancel) => self.launcher_pending_confirmation = None,
            _ => {}
        }
    }

    fn update_launcher_drag(&mut self, coordinates: CellPosition) {
        if self.launcher_view_mode != tundra_ui::LauncherViewMode::LargeIcons {
            self.launcher_drag = None;
            return;
        }
        let area = Rect::new(0, 0, self.terminal_size.0, self.terminal_size.1);
        let tundra_ui::ShellLayout::Full { main, .. } = tundra_ui::compute_shell_layout(area) else {
            self.launcher_drag = None;
            return;
        };
        let model = self.to_launcher_view_model();
        let target = tundra_ui::launcher_layout(main, &model)
            .large_icon_drop_target(coordinates.0, coordinates.1);
        if let Some(drag) = self.launcher_drag.as_mut() {
            drag.target = target;
        }
    }

    fn drop_launcher_drag(&mut self, coordinates: CellPosition, platform: &dyn Platform) {
        let Some(drag) = self.launcher_drag.take() else { return };
        if self.launcher_view_mode != tundra_ui::LauncherViewMode::LargeIcons {
            return;
        }
        let area = Rect::new(0, 0, self.terminal_size.0, self.terminal_size.1);
        let tundra_ui::ShellLayout::Full { main, .. } = tundra_ui::compute_shell_layout(area) else { return };
        let model = self.to_launcher_view_model();
        let Some(target) = tundra_ui::launcher_layout(main, &model)
            .large_icon_drop_target(coordinates.0, coordinates.1)
        else { return };
        self.apply_launcher_command(
            LauncherCommand::Reorder {
                id: drag.item_id.clone(),
                insertion_index: target.insertion_index(),
            },
            platform,
        );
        if let Some(index) = self.launcher_state.as_ref().and_then(|state| {
            state
                .items
                .iter()
                .position(|item| item.record.id == drag.item_id)
        }) {
            self.launcher_selected_index = index;
        }
    }

    pub fn to_launcher_view_model(&self) -> tundra_ui::LauncherViewModel {
        let items = self.launcher_state.as_ref().map(|state| {
            state.items.iter().enumerate().map(|(index, item)| {
                let path = std::path::Path::new(&item.record.path);
                let name = path.file_name().and_then(|name| name.to_str()).unwrap_or(&item.record.path);
                let type_label = match item.record.executable_kind {
                    Some(LauncherExecutableKind::NativeBinary) => "Application",
                    Some(LauncherExecutableKind::Installer) => "Installer",
                    Some(LauncherExecutableKind::Script) => "Script",
                    Some(LauncherExecutableKind::Shortcut) => "Shortcut",
                    Some(LauncherExecutableKind::ApplicationBundle) => "Application bundle",
                    None => "Unknown",
                };
                let status = match item.status {
                    DomainLauncherItemStatus::Ready => tundra_ui::LauncherItemStatus::Ready,
                    DomainLauncherItemStatus::Checking => tundra_ui::LauncherItemStatus::Checking,
                    DomainLauncherItemStatus::Changed => tundra_ui::LauncherItemStatus::Changed,
                    DomainLauncherItemStatus::Missing => tundra_ui::LauncherItemStatus::Missing,
                    DomainLauncherItemStatus::NeedsApproval => tundra_ui::LauncherItemStatus::NeedsApproval,
                    DomainLauncherItemStatus::Unsupported => tundra_ui::LauncherItemStatus::Unsupported,
                };
                let mut model = tundra_ui::LauncherItemViewModel::new(
                    item.record.id.clone(), name, item.record.path.clone(), type_label, status,
                );
                model.selected = index == self.launcher_selected_index;
                model
            }).collect::<Vec<_>>()
        }).unwrap_or_default();
        let selected = items
            .len()
            .checked_sub(1)
            .map(|last_index| self.launcher_selected_index.min(last_index));
        let mut model = tundra_ui::LauncherViewModel::new(items, selected, self.launcher_view_mode, self.can_manage_launcher());
        model.viewport_offset = self.launcher_viewport_offset;
        model.drop_target = self.launcher_drag.as_ref().and_then(|drag| drag.target);
        if let Some(state) = self.launcher_state.as_ref() {
            model.message = state.message.clone();
            model.error = state.error.clone();
        }
        model.confirmation = self.launcher_pending_confirmation.as_ref().map(|pending| match pending {
            LauncherPendingConfirmation::Launch { path, kind, .. } => tundra_ui::LauncherConfirmationViewModel {
                kind: tundra_ui::LauncherConfirmationKind::Launch,
                title: "Confirm launch".to_string(),
                message: format!("Open {} ({kind:?}) with the system default handler?", path.display()),
                confirm_label: "Launch".to_string(),
                cancel_label: "Cancel".to_string(),
                confirm_selected: true,
            },
            LauncherPendingConfirmation::Remove { label, .. } => tundra_ui::LauncherConfirmationViewModel {
                kind: tundra_ui::LauncherConfirmationKind::Remove,
                title: "Remove from Launcher".to_string(),
                message: format!("Remove {label} from Launcher? The file will not be deleted."),
                confirm_label: "Remove".to_string(),
                cancel_label: "Cancel".to_string(),
                confirm_selected: true,
            },
        });
        model
    }
}
