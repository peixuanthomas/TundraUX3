use super::super::*;
impl ShellSession {
    pub(in crate::session) fn replace_explorer_state(
        &mut self,
        explorer_state: Option<ExplorerState>,
    ) {
        let _ = self.app.dispatch_at(
            app::AppCommand::SetExplorerState(explorer_state),
            Instant::now(),
        );
    }

    pub(in crate::session) fn update_explorer_state<T>(
        &mut self,
        update: impl FnOnce(&mut ExplorerState) -> T,
    ) -> Option<T> {
        let mut explorer_state = self.app.explorer_state()?.clone();
        let result = update(&mut explorer_state);
        self.replace_explorer_state(Some(explorer_state));
        Some(result)
    }

    pub(in crate::session) fn can_change_explorer_settings(&self) -> bool {
        PermissionService::new(self.debug_policy)
            .authorize(
                self.app.auth_session(),
                PermissionAction::ChangeSettings,
                None,
            )
            .allowed
    }

    pub(in crate::session) fn open_explorer(&mut self, platform: &dyn Platform) {
        self.explorer_purpose = ExplorerPurpose::Browse;
        if self.is_strict_guest() {
            self.error_message = None;
            self.notify_status("Guest access is read-only");
            return;
        }
        if self.app.auth_session().is_none() {
            self.error_message = Some("Login required".to_string());
            return;
        }
        let Some(storage) = self.storage_manager.clone() else {
            self.error_message = Some("Storage unavailable".to_string());
            return;
        };

        let user_dirs = platform.user_dirs().ok();
        let start_path = user_dirs
            .as_ref()
            .map(|dirs| dirs.documents().to_path_buf())
            .unwrap_or_else(|| storage.layout().data_path.clone());
        let start_path = if start_path.exists() {
            start_path
        } else {
            storage.layout().data_path.clone()
        };

        self.open_explorer_at(platform, &storage, start_path, ExplorerPurpose::Browse);
    }

    pub(in crate::session) fn open_explorer_at(
        &mut self,
        platform: &dyn Platform,
        _storage: &StorageManager,
        start_path: std::path::PathBuf,
        purpose: ExplorerPurpose,
    ) {
        let explorer_config = self.app.storage_config().explorer.clone();
        self.replace_explorer_state(Some(ExplorerState::with_config(
            start_path,
            &explorer_config,
        )));
        self.explorer_purpose = purpose;
        self.refresh_explorer_quick_locations(platform);
        self.explorer_input_mode = ExplorerInputMode::Browse;
        self.explorer_input.clear();
        self.explorer_input_replace_all = false;
        self.explorer_overlay_mode = None;
        self.screen_stack.push(ShellScreen::Explorer);
        self.focused_component = ShellComponent::Explorer;
        self.notify_status("Explorer");
        self.apply_explorer_command(ExplorerCommand::Refresh, platform);
        self.refresh_hit_map();
    }

    pub(in crate::session) fn close_explorer(&mut self) {
        self.explorer_input_mode = ExplorerInputMode::Browse;
        self.explorer_input.clear();
        self.explorer_input_replace_all = false;
        self.explorer_overlay_mode = None;
        self.resolve_explorer_alert();
        if matches!(self.explorer_purpose, ExplorerPurpose::DiagnosticsLogs) {
            self.explorer_purpose = ExplorerPurpose::Browse;
            self.replace_explorer_state(None);
            if self.active_screen() == ShellScreen::Explorer {
                self.screen_stack.pop();
            }
            if self.active_screen() == ShellScreen::Diagnostics {
                self.focused_component = ShellComponent::Diagnostics;
                self.notify_status("Diagnostics");
            } else {
                self.pop_to_home();
                self.notify_status("Ready");
            }
            self.refresh_hit_map();
            return;
        }
        if !matches!(self.explorer_purpose, ExplorerPurpose::Browse)
            && self.app.editor_state().is_some()
        {
            if matches!(self.explorer_purpose, ExplorerPurpose::EditorSaveAs { .. }) {
                self.editor_close_after_save = false;
                self.editor_open_after_save = false;
            }
            self.return_from_editor_picker();
            return;
        }
        self.explorer_purpose = ExplorerPurpose::Browse;
        self.pop_to_home();
        self.notify_status("Ready");
    }

    pub(in crate::session) fn refresh_explorer_quick_locations(&mut self, platform: &dyn Platform) {
        let retained_volumes = self
            .app
            .explorer_state()
            .map(|state| {
                state
                    .quick_locations
                    .iter()
                    .filter(|location| {
                        location.kind == app::explorer::ExplorerQuickLocationKind::Volume
                    })
                    .cloned()
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let mut locations = Vec::new();
        if let Ok(dirs) = platform.user_dirs() {
            locations.extend([
                app::explorer::ExplorerQuickLocation::new(
                    "desktop",
                    "Desktop",
                    dirs.desktop(),
                    "desktop",
                ),
                app::explorer::ExplorerQuickLocation::new(
                    "documents",
                    "Documents",
                    dirs.documents(),
                    "documents",
                ),
                app::explorer::ExplorerQuickLocation::new(
                    "downloads",
                    "Downloads",
                    dirs.downloads(),
                    "downloads",
                ),
                app::explorer::ExplorerQuickLocation::new(
                    "pictures",
                    "Pictures",
                    dirs.pictures(),
                    "pictures",
                ),
                app::explorer::ExplorerQuickLocation::new("music", "Music", dirs.music(), "music"),
                app::explorer::ExplorerQuickLocation::new(
                    "videos",
                    "Videos",
                    dirs.videos(),
                    "videos",
                ),
            ]);
        }
        let volumes = platform
            .local_volumes()
            .map_or(retained_volumes, |volumes| {
                volumes
                    .into_iter()
                    .enumerate()
                    .map(|(index, volume)| {
                        let root_label = volume.root.display().to_string();
                        let label = volume
                            .label
                            .filter(|label| !label.trim().is_empty())
                            .map(|label| format!("{label} ({root_label})"))
                            .unwrap_or_else(|| root_label.clone());
                        app::explorer::ExplorerQuickLocation::volume(
                            format!("volume-{index}-{root_label}"),
                            label,
                            volume.root,
                        )
                    })
                    .collect()
            });
        locations.extend(volumes);
        locations.push(app::explorer::ExplorerQuickLocation::trash());
        let mut unique = Vec::with_capacity(locations.len());
        for location in locations {
            let duplicate = unique
                .iter()
                .any(|existing: &app::explorer::ExplorerQuickLocation| {
                    (existing.is_trash() && location.is_trash())
                        || (!existing.is_trash()
                            && !location.is_trash()
                            && existing.path == location.path)
                });
            if !duplicate {
                unique.push(location);
            }
        }
        let _ = self.update_explorer_state(|state| {
            state.quick_locations = unique;
        });
    }

    pub(in crate::session) fn resolve_explorer_alert(&mut self) {
        let resolved_message = self
            .notification_alert_message_for_key(EXPLORER_ALERT_KEY)
            .map(str::to_string);
        if self.error_message.as_ref() == resolved_message.as_ref() {
            self.error_message = None;
        }
        self.resolve_notification_alert(EXPLORER_ALERT_KEY);
    }

    pub(in crate::session) fn apply_explorer_command(
        &mut self,
        command: ExplorerCommand,
        platform: &dyn Platform,
    ) {
        if self.try_handle_explorer_background_command(&command, platform) {
            return;
        }
        let command_kind = command.clone();
        let can_change_settings = self.can_change_explorer_settings();
        let Some(storage) = self.storage_manager.clone() else {
            let message = "Storage unavailable".to_string();
            self.error_message = Some(message.clone());
            self.notify_alert_with_key(EXPLORER_ALERT_KEY, message, ui::NotificationTone::Error);
            return;
        };
        if self.app.explorer_state().is_none() {
            let message = "Explorer unavailable".to_string();
            self.error_message = Some(message.clone());
            self.notify_alert_with_key(EXPLORER_ALERT_KEY, message, ui::NotificationTone::Error);
            return;
        }

        // Explorer's advanced options are global defaults. A regular user may still
        // rearrange the current view, but must not be able to alter those defaults.
        if explorer_command_requires_global_settings_permission(&command_kind)
            && !can_change_settings
        {
            let _ = self.update_explorer_state(|state| {
                state.error = None;
                state.message = Some(
                    "Explorer options are read-only. Administrator permission is required."
                        .to_string(),
                );
            });
            self.notify_status("Explorer options are read-only");
            self.refresh_hit_map();
            return;
        }

        let (_, effect) =
            self.app
                .dispatch_explorer_at(command, platform, &storage, Instant::now());
        self.handle_explorer_effect(effect, platform, &storage, can_change_settings);
        let (pending_dialog, pending_conflict, explorer_error, explorer_message) = self
            .app
            .explorer_state()
            .map(|state| {
                (
                    state.pending_dialog.clone(),
                    state.pending_conflict.clone(),
                    state.error.clone(),
                    state.message.clone(),
                )
            })
            .unwrap_or((None, None, None, None));
        if let Some(error) = explorer_error {
            self.error_message = Some(error.clone());
            self.notify_alert_with_key(EXPLORER_ALERT_KEY, error, ui::NotificationTone::Error);
            self.notify_status("Explorer error");
        } else {
            self.error_message = None;
            self.resolve_explorer_alert();
            if let Some(message) = explorer_message {
                self.notify_status(message);
            }
        }

        if matches!(
            command_kind,
            ExplorerCommand::DeleteToTrash
                | ExplorerCommand::ConfirmDelete
                | ExplorerCommand::DumpTrash
                | ExplorerCommand::ConfirmDumpTrash
        ) && let Some(dialog) = pending_dialog
        {
            let (confirm_label, follow_up) = match dialog.kind {
                app::explorer::ExplorerDialogKind::DeleteToTrash => {
                    ("Move", ShellCommand::ExplorerConfirmDelete)
                }
                app::explorer::ExplorerDialogKind::DumpTrash => {
                    ("Empty", ShellCommand::ExplorerConfirmDumpTrash)
                }
            };
            self.notify_modal_with_options(
                ShellNotification::modal(
                    dialog.title,
                    dialog.message,
                    ui::NotificationTone::Warning,
                    vec![
                        ShellNotificationAction::new("confirm", confirm_label)
                            .with_shortcut(InputKey::Char('y'))
                            .with_follow_up(follow_up),
                        ShellNotificationAction::new("cancel", "Cancel")
                            .with_shortcut(InputKey::Char('n'))
                            .cancel()
                            .with_follow_up(ShellCommand::CancelExplorerInput),
                    ],
                )
                .with_key(EXPLORER_DELETE_NOTIFICATION_KEY),
            );
        }

        if matches!(
            command_kind,
            ExplorerCommand::Paste | ExplorerCommand::ResolveConflict { .. }
        ) {
            // Name conflicts are rendered and hit-tested by the Explorer itself. Keep the old
            // notification key clean so it cannot cover the clickable Ratatui conflict dialog.
            let _ = pending_conflict;
            self.notification_dismiss_modal_by_key(EXPLORER_CONFLICT_NOTIFICATION_KEY);
        }
    }
    pub(in crate::session) fn handle_explorer_effect(
        &mut self,
        effect: ExplorerEffect,
        platform: &dyn Platform,
        storage: &StorageManager,
        can_change_settings: bool,
    ) {
        match effect {
            ExplorerEffect::None => {}
            ExplorerEffect::PersistConfig(_explorer) if !can_change_settings => {
                // Sorting is deliberately session-local for non-admin users. The controller has
                // already updated the in-memory projection; skip persistence of shared defaults.
                let _ = self.update_explorer_state(|state| {
                    state.error = None;
                    state.message = Some(
                        "Sort applied for this session; Explorer defaults are read-only."
                            .to_string(),
                    );
                });
            }
            ExplorerEffect::PersistConfig(explorer) => match storage.load_config() {
                Ok(mut config) => {
                    config.explorer = explorer;
                    if let Err(error) = storage.save_config(&config) {
                        let _ = self.update_explorer_state(|state| {
                            state.error = Some(format!("Could not save Explorer options: {error}"));
                            state.message = None;
                        });
                    } else {
                        self.replace_storage_config(config);
                    }
                }
                Err(error) => {
                    let _ = self.update_explorer_state(|state| {
                        state.error = Some(format!("Could not load Explorer options: {error}"));
                        state.message = None;
                    });
                }
            },
            ExplorerEffect::OpenRequested(request) => match request.target {
                ExplorerOpenTarget::SystemDefault => {
                    let result = platform.open_path(&request.path);
                    let _ = self.update_explorer_state(|state| match result {
                        Ok(()) => {
                            state.message = Some(format!("Opened {}", request.path.display()));
                            state.error = None;
                        }
                        Err(error) => {
                            state.error = Some(error.to_string());
                            state.message = None;
                        }
                    });
                }
                ExplorerOpenTarget::Editor => {
                    let _ = self.open_editor_path(request.path);
                }
                ExplorerOpenTarget::Launcher => self.open_launcher_for_path(request.path, platform),
            },
        }
    }
    pub(in crate::session) fn begin_explorer_input(&mut self, mode: ExplorerInputMode) {
        self.explorer_input_mode = mode;
        self.explorer_input = match mode {
            ExplorerInputMode::Address => self
                .app
                .explorer_state()
                .map(|state| state.current_path.display().to_string())
                .unwrap_or_default(),
            ExplorerInputMode::Rename => self
                .app
                .explorer_state()
                .and_then(|state| state.selected_entry())
                .map(|entry| entry.name.clone())
                .unwrap_or_default(),
            ExplorerInputMode::Browse
            | ExplorerInputMode::Search
            | ExplorerInputMode::NewFolder
            | ExplorerInputMode::NewTextFile
            | ExplorerInputMode::RestoreDestination => String::new(),
        };
        self.explorer_input_replace_all = mode == ExplorerInputMode::Address;
        let _ = self.update_explorer_state(|state| {
            state.error = None;
        });
        self.notify_status(explorer_input_prompt(mode));
    }

    pub(in crate::session) fn append_explorer_char(
        &mut self,
        character: char,
        platform: &dyn Platform,
    ) {
        if self.explorer_input_replace_all {
            self.explorer_input.clear();
            self.explorer_input_replace_all = false;
        }
        self.explorer_input.push(character);
        self.apply_live_explorer_search(platform);
    }

    pub(in crate::session) fn explorer_backspace(&mut self, platform: &dyn Platform) {
        if self.explorer_input_replace_all {
            self.explorer_input.clear();
            self.explorer_input_replace_all = false;
        } else {
            self.explorer_input.pop();
        }
        self.apply_live_explorer_search(platform);
    }

    pub(in crate::session) fn apply_live_explorer_search(&mut self, platform: &dyn Platform) {
        if self.explorer_input_mode == ExplorerInputMode::Search {
            self.apply_explorer_command(
                ExplorerCommand::Search(self.explorer_input.clone()),
                platform,
            );
        }
    }

    pub(in crate::session) fn submit_explorer_input(&mut self, platform: &dyn Platform) {
        if self.submit_editor_save_as_from_explorer(platform) {
            return;
        }
        let raw_value = self.explorer_input.clone();
        let trimmed_value = raw_value.trim().to_string();
        let command = match self.explorer_input_mode {
            ExplorerInputMode::Browse => return,
            ExplorerInputMode::Address => {
                ExplorerCommand::Navigate(std::path::PathBuf::from(raw_value))
            }
            ExplorerInputMode::Search => ExplorerCommand::Search(trimmed_value),
            ExplorerInputMode::NewFolder => ExplorerCommand::NewFolder(trimmed_value),
            ExplorerInputMode::NewTextFile => ExplorerCommand::NewTextFile(trimmed_value),
            ExplorerInputMode::Rename => ExplorerCommand::Rename(trimmed_value),
            ExplorerInputMode::RestoreDestination => {
                ExplorerCommand::RestoreSelectedToDirectory(std::path::PathBuf::from(raw_value))
            }
        };

        self.apply_explorer_command(command, platform);
        if self
            .app
            .explorer_state()
            .is_some_and(|state| state.error.is_some())
        {
            return;
        }
        self.explorer_input_mode = ExplorerInputMode::Browse;
        self.explorer_input.clear();
        self.explorer_input_replace_all = false;
    }

    pub(in crate::session) fn restore_selected_explorer_item(&mut self, platform: &dyn Platform) {
        let selected = self.app.explorer_state().and_then(|state| {
            (state.current_location.is_trash() && state.effective_selected_paths().len() == 1)
                .then(|| state.selected_entry())
                .flatten()
                .map(|entry| entry.original_path.is_some())
        });
        match selected {
            Some(true) => self.apply_explorer_command(ExplorerCommand::RestoreSelected, platform),
            Some(false) => self.begin_explorer_input(ExplorerInputMode::RestoreDestination),
            None => {}
        }
    }

    pub(in crate::session) fn cancel_explorer_input(&mut self) {
        if self
            .app
            .explorer_state()
            .is_some_and(|state| state.pending_dialog.is_some())
        {
            let _ = self.update_explorer_state(|state| {
                state.pending_dialog = None;
                state.message = Some("Cancelled".to_string());
            });
            self.notification_dismiss_modal_by_key(EXPLORER_DELETE_NOTIFICATION_KEY);
            self.notify_status("Cancelled");
            return;
        }
        self.explorer_input_mode = ExplorerInputMode::Browse;
        self.explorer_input.clear();
        self.explorer_input_replace_all = false;
        self.notify_status("Explorer");
    }

    pub(in crate::session) fn select_explorer_at(
        &mut self,
        coordinates: CellPosition,
        click: ClickKind,
        platform: &dyn Platform,
    ) {
        self.pointer_down_explorer_at(coordinates, click, InputModifiers::none(), platform);
        let _ = self.update_explorer_state(|state| {
            state.drag = None;
        });
    }

    pub(in crate::session) fn pointer_down_explorer_at(
        &mut self,
        coordinates: CellPosition,
        click: ClickKind,
        modifiers: InputModifiers,
        platform: &dyn Platform,
    ) {
        let Some(hit_target) = self.explorer_hit_target_at(coordinates) else {
            self.clear_explorer_scrollbar_drag();
            return;
        };
        if !matches!(&hit_target, ui::ExplorerHitTarget::Scrollbar) {
            self.clear_explorer_scrollbar_drag();
        }
        let index = match hit_target {
            ui::ExplorerHitTarget::Entry(index) => index,
            ui::ExplorerHitTarget::Toolbar(action) => {
                self.activate_explorer_toolbar(action, coordinates, platform);
                return;
            }
            ui::ExplorerHitTarget::Address => {
                self.begin_explorer_input(ExplorerInputMode::Address);
                return;
            }
            ui::ExplorerHitTarget::Breadcrumb(index) => {
                let destination = self
                    .to_explorer_view_model()
                    .breadcrumbs
                    .get(index)
                    .filter(|breadcrumb| breadcrumb.enabled)
                    .map(|breadcrumb| std::path::PathBuf::from(&breadcrumb.path));
                if let Some(destination) = destination {
                    self.apply_explorer_command(ExplorerCommand::Navigate(destination), platform);
                }
                return;
            }
            ui::ExplorerHitTarget::QuickLocation(index) => {
                let location = self
                    .to_explorer_view_model()
                    .quick_locations
                    .get(index)
                    .filter(|location| location.enabled)
                    .cloned();
                if let Some(location) = location {
                    match location.kind {
                        app::explorer::ExplorerQuickLocationKind::Trash => {
                            self.apply_explorer_command(ExplorerCommand::NavigateTrash, platform)
                        }
                        app::explorer::ExplorerQuickLocationKind::Directory
                        | app::explorer::ExplorerQuickLocationKind::Volume => self
                            .apply_explorer_command(
                                ExplorerCommand::Navigate(std::path::PathBuf::from(location.path)),
                                platform,
                            ),
                    }
                }
                return;
            }
            ui::ExplorerHitTarget::Column(column) => {
                self.apply_explorer_command(
                    ExplorerCommand::SetSort(explorer_sort_field(column)),
                    platform,
                );
                return;
            }
            ui::ExplorerHitTarget::Search => {
                self.begin_explorer_input(ExplorerInputMode::Search);
                return;
            }
            ui::ExplorerHitTarget::CancelOperation => {
                self.apply_explorer_command(ExplorerCommand::CancelOperation, platform);
                return;
            }
            ui::ExplorerHitTarget::Overlay(control) => {
                self.activate_explorer_overlay_control(control, platform);
                return;
            }
            ui::ExplorerHitTarget::EmptyTable => {
                let _ = self.update_explorer_state(|state| {
                    state.clear_selection();
                });
                return;
            }
            ui::ExplorerHitTarget::Scrollbar => {
                self.begin_explorer_scrollbar_drag(coordinates);
                return;
            }
            ui::ExplorerHitTarget::OverlaySurface => return,
        };
        let selection_mode = if modifiers.shift {
            app::explorer::ExplorerSelectionMode::Range
        } else if explorer_toggle_modifier(platform.kind(), modifiers) {
            app::explorer::ExplorerSelectionMode::Toggle
        } else {
            app::explorer::ExplorerSelectionMode::Replace
        };
        self.apply_explorer_command(
            ExplorerCommand::SelectIndexWithMode(index, selection_mode),
            platform,
        );
        if click == ClickKind::Double
            && selection_mode == app::explorer::ExplorerSelectionMode::Replace
        {
            self.apply_explorer_command(ExplorerCommand::OpenSelected, platform);
            return;
        }
        if self.app.explorer_state().is_none_or(|state| {
            state.current_location.is_trash() || state.effective_selected_paths().is_empty()
        }) {
            return;
        }
        self.apply_explorer_command(ExplorerCommand::BeginDrag, platform);
    }

    pub(in crate::session) fn activate_explorer_toolbar(
        &mut self,
        action: ui::ExplorerToolbarAction,
        anchor: CellPosition,
        platform: &dyn Platform,
    ) {
        use ui::ExplorerToolbarAction;

        match action {
            ExplorerToolbarAction::Back => {
                self.apply_explorer_command(ExplorerCommand::OpenBack, platform)
            }
            ExplorerToolbarAction::Forward => {
                self.apply_explorer_command(ExplorerCommand::OpenForward, platform)
            }
            ExplorerToolbarAction::Up => {
                self.apply_explorer_command(ExplorerCommand::OpenParent, platform)
            }
            ExplorerToolbarAction::Refresh => {
                self.refresh_explorer_quick_locations(platform);
                self.apply_explorer_command(ExplorerCommand::Refresh, platform)
            }
            ExplorerToolbarAction::New => self.begin_explorer_input(ExplorerInputMode::NewFolder),
            ExplorerToolbarAction::Cut => {
                self.apply_explorer_command(ExplorerCommand::Cut, platform)
            }
            ExplorerToolbarAction::Copy => {
                self.apply_explorer_command(ExplorerCommand::Copy, platform)
            }
            ExplorerToolbarAction::Paste => {
                self.apply_explorer_command(ExplorerCommand::Paste, platform)
            }
            ExplorerToolbarAction::Rename => self.begin_explorer_input(ExplorerInputMode::Rename),
            ExplorerToolbarAction::Delete => {
                self.apply_explorer_command(ExplorerCommand::DeleteToTrash, platform)
            }
            ExplorerToolbarAction::Restore => self.restore_selected_explorer_item(platform),
            ExplorerToolbarAction::DumpTrash => {
                self.apply_explorer_command(ExplorerCommand::DumpTrash, platform)
            }
            ExplorerToolbarAction::Sort => {
                self.open_explorer_popup(ExplorerOverlayMode::Sort { anchor }, anchor)
            }
            ExplorerToolbarAction::Options => {
                self.open_explorer_popup(ExplorerOverlayMode::Options, anchor)
            }
        }
        self.refresh_hit_map();
    }

    pub(in crate::session) fn activate_explorer_overlay_at(
        &mut self,
        coordinates: CellPosition,
        platform: &dyn Platform,
    ) {
        if let Some(ui::ExplorerHitTarget::Overlay(control)) =
            self.explorer_hit_target_at(coordinates)
        {
            self.activate_explorer_overlay_control(control, platform);
        }
    }

    pub(in crate::session) fn activate_explorer_overlay_control(
        &mut self,
        control: ui::ExplorerOverlayControl,
        platform: &dyn Platform,
    ) {
        use ui::{ExplorerConflictChoice, ExplorerOverlayControl, ExplorerOverlayViewModel};

        match control {
            ExplorerOverlayControl::ContextItem(index) => {
                let item = match self.to_explorer_view_model().overlay {
                    Some(ExplorerOverlayViewModel::ContextMenu(menu)) => {
                        menu.items.get(index).filter(|item| item.enabled).cloned()
                    }
                    _ => None,
                };
                let Some(item) = item else {
                    return;
                };
                let anchor = match self.explorer_overlay_mode {
                    Some(ExplorerOverlayMode::ContextMenu { anchor })
                    | Some(ExplorerOverlayMode::Sort { anchor }) => anchor,
                    Some(ExplorerOverlayMode::Options)
                    | Some(ExplorerOverlayMode::Properties)
                    | None => (0, 0),
                };
                self.close_explorer_popup();
                match item.id.as_str() {
                    "open" => self.apply_explorer_command(ExplorerCommand::OpenSelected, platform),
                    "add-to-launcher" => self.add_selected_explorer_to_launcher(platform),
                    "cut" => self.apply_explorer_command(ExplorerCommand::Cut, platform),
                    "copy" => self.apply_explorer_command(ExplorerCommand::Copy, platform),
                    "rename" => self.begin_explorer_input(ExplorerInputMode::Rename),
                    "delete" => {
                        self.apply_explorer_command(ExplorerCommand::DeleteToTrash, platform)
                    }
                    "restore" => self.restore_selected_explorer_item(platform),
                    "dump-trash" => {
                        self.apply_explorer_command(ExplorerCommand::DumpTrash, platform)
                    }
                    "properties" => {
                        self.open_explorer_popup(ExplorerOverlayMode::Properties, anchor)
                    }
                    "new-folder" => self.begin_explorer_input(ExplorerInputMode::NewFolder),
                    "new-text" => self.begin_explorer_input(ExplorerInputMode::NewTextFile),
                    "paste" => self.apply_explorer_command(ExplorerCommand::Paste, platform),
                    "select-all" => {
                        self.apply_explorer_command(ExplorerCommand::SelectAll, platform)
                    }
                    "refresh" => {
                        self.refresh_explorer_quick_locations(platform);
                        self.apply_explorer_command(ExplorerCommand::Refresh, platform);
                    }
                    "sort" => {
                        self.open_explorer_popup(ExplorerOverlayMode::Sort { anchor }, anchor)
                    }
                    "options" => self.open_explorer_popup(ExplorerOverlayMode::Options, anchor),
                    "sort-name" => self.apply_explorer_command(
                        ExplorerCommand::SetSort(app::explorer::ExplorerSortField::Name),
                        platform,
                    ),
                    "sort-type" => self.apply_explorer_command(
                        ExplorerCommand::SetSort(app::explorer::ExplorerSortField::Type),
                        platform,
                    ),
                    "sort-size" => self.apply_explorer_command(
                        ExplorerCommand::SetSort(app::explorer::ExplorerSortField::Size),
                        platform,
                    ),
                    "sort-modified" => self.apply_explorer_command(
                        ExplorerCommand::SetSort(app::explorer::ExplorerSortField::Modified),
                        platform,
                    ),
                    _ => {}
                }
            }
            ExplorerOverlayControl::NameInput => {}
            ExplorerOverlayControl::Confirm => self.submit_explorer_input(platform),
            ExplorerOverlayControl::Cancel => {
                self.close_explorer_popup();
                self.cancel_explorer_input();
            }
            ExplorerOverlayControl::Option(index) => {
                let option = match self.to_explorer_view_model().overlay {
                    Some(ExplorerOverlayViewModel::Options(options)) => options
                        .options
                        .get(index)
                        .filter(|option| option.enabled)
                        .cloned(),
                    _ => None,
                };
                let Some(option) = option else {
                    return;
                };
                let command = match option.id.as_str() {
                    "hidden" => ExplorerCommand::ToggleHidden,
                    "system" => ExplorerCommand::ToggleSystem,
                    "extensions" => ExplorerCommand::ToggleExtensions,
                    "folders-first" => ExplorerCommand::ToggleFoldersFirst,
                    "case-sensitive" => ExplorerCommand::ToggleCaseSensitiveSort,
                    "size-format" => ExplorerCommand::ToggleSizeFormat,
                    "date-zone" => ExplorerCommand::ToggleDateZone,
                    "confirm-delete" => ExplorerCommand::ToggleDeleteConfirmation,
                    "confirm-conflicts" => ExplorerCommand::ToggleConflictConfirmation,
                    "sidebar" => ExplorerCommand::ToggleSidebar,
                    _ => return,
                };
                self.apply_explorer_command(command, platform);
                self.refresh_hit_map();
            }
            ExplorerOverlayControl::OptionsClose | ExplorerOverlayControl::PropertiesClose => {
                self.close_explorer_popup()
            }
            ExplorerOverlayControl::ConflictChoice(choice) => {
                let action = match choice {
                    ExplorerConflictChoice::KeepBoth => ExplorerConflictAction::KeepBoth,
                    ExplorerConflictChoice::Replace => ExplorerConflictAction::Replace,
                    ExplorerConflictChoice::Skip => ExplorerConflictAction::Skip,
                    ExplorerConflictChoice::Cancel => ExplorerConflictAction::Cancel,
                };
                let restore_conflict = self
                    .app
                    .explorer_state()
                    .is_some_and(|state| state.pending_restore.is_some());
                if restore_conflict {
                    self.apply_explorer_command(
                        ExplorerCommand::ResolveRestoreConflict(action),
                        platform,
                    );
                } else {
                    self.apply_explorer_command(
                        ExplorerCommand::ResolveConflict {
                            action,
                            apply_to_all: self.explorer_conflict_apply_to_remaining,
                        },
                        platform,
                    );
                }
                self.explorer_conflict_apply_to_remaining = false;
            }
            ExplorerOverlayControl::ApplyToRemaining => {
                if self
                    .app
                    .explorer_state()
                    .is_none_or(|state| state.pending_restore.is_none())
                {
                    self.explorer_conflict_apply_to_remaining =
                        !self.explorer_conflict_apply_to_remaining;
                }
            }
        }
        self.refresh_hit_map();
    }

    pub(in crate::session) fn open_explorer_popup(
        &mut self,
        mode: ExplorerOverlayMode,
        anchor: CellPosition,
    ) {
        self.explorer_overlay_selection = match mode {
            ExplorerOverlayMode::Sort { .. } => self
                .app
                .explorer_state()
                .map(|state| match state.sort_field {
                    app::explorer::ExplorerSortField::Name => 0,
                    app::explorer::ExplorerSortField::Type => 1,
                    app::explorer::ExplorerSortField::Size => 2,
                    app::explorer::ExplorerSortField::Modified => 3,
                })
                .unwrap_or(0),
            ExplorerOverlayMode::ContextMenu { .. }
            | ExplorerOverlayMode::Options
            | ExplorerOverlayMode::Properties => 0,
        };
        self.explorer_overlay_mode = Some(mode);
        self.active_popup = Some(ShellPopup {
            owner: Some(ShellComponent::Explorer),
            anchor,
        });
        self.focused_component = ShellComponent::ContextMenu;
        self.refresh_hit_map();
    }

    pub(in crate::session) fn close_explorer_popup(&mut self) {
        self.active_popup = None;
        self.explorer_overlay_mode = None;
        self.explorer_overlay_selection = 0;
        self.focused_component = ShellComponent::Explorer;
        self.refresh_hit_map();
    }

    pub(in crate::session) fn move_explorer_overlay_selection(&mut self, delta: isize) {
        let count = match self.to_explorer_view_model().overlay {
            Some(ui::ExplorerOverlayViewModel::ContextMenu(menu)) => menu.items.len(),
            Some(ui::ExplorerOverlayViewModel::Options(options)) => options.options.len(),
            Some(ui::ExplorerOverlayViewModel::Properties(_)) => 1,
            _ => 0,
        };
        if count == 0 {
            return;
        }
        self.explorer_overlay_selection = if delta < 0 {
            self.explorer_overlay_selection
                .checked_sub(delta.unsigned_abs())
                .unwrap_or(count - 1)
        } else {
            (self.explorer_overlay_selection + delta as usize) % count
        };
        self.refresh_hit_map();
    }

    pub(in crate::session) fn activate_selected_explorer_overlay(
        &mut self,
        platform: &dyn Platform,
    ) {
        let control = match self.to_explorer_view_model().overlay {
            Some(ui::ExplorerOverlayViewModel::ContextMenu(menu)) => menu
                .items
                .get(self.explorer_overlay_selection)
                .filter(|item| item.enabled)
                .map(|_| ui::ExplorerOverlayControl::ContextItem(self.explorer_overlay_selection)),
            Some(ui::ExplorerOverlayViewModel::Options(options)) => options
                .options
                .get(self.explorer_overlay_selection)
                .filter(|option| option.enabled)
                .map(|_| ui::ExplorerOverlayControl::Option(self.explorer_overlay_selection)),
            Some(ui::ExplorerOverlayViewModel::Properties(_)) => {
                Some(ui::ExplorerOverlayControl::PropertiesClose)
            }
            _ => None,
        };
        if let Some(control) = control {
            self.activate_explorer_overlay_control(control, platform);
        }
    }

    pub(in crate::session) fn update_explorer_drag(
        &mut self,
        coordinates: CellPosition,
        modifiers: InputModifiers,
        platform: &dyn Platform,
    ) {
        if matches!(
            self.scrollbar_drag,
            Some(ScrollbarDragState::Explorer { .. })
        ) {
            self.drag_explorer_scrollbar(coordinates);
            return;
        }
        let left_start_cell = self
            .drag_tracker
            .filter(|tracker| tracker.button == PointerButton::Left)
            .is_some_and(|tracker| tracker.origin_coordinates != coordinates);
        if !left_start_cell {
            return;
        }
        if self
            .app
            .explorer_state()
            .and_then(|state| state.drag.as_ref())
            .is_none()
        {
            return;
        }
        let target = self.explorer_drop_destination_at(coordinates);
        let mode = if explorer_copy_modifier(platform.kind(), modifiers) {
            app::explorer::ExplorerTransferMode::Copy
        } else {
            app::explorer::ExplorerTransferMode::Move
        };
        self.apply_explorer_command(ExplorerCommand::UpdateDrag { target, mode }, platform);
    }

    pub(in crate::session) fn drop_explorer_drag(
        &mut self,
        coordinates: CellPosition,
        modifiers: InputModifiers,
        platform: &dyn Platform,
    ) {
        if self.clear_explorer_scrollbar_drag() {
            let _ = self.update_explorer_state(|state| {
                state.drag = None;
            });
            return;
        }
        // A normal click also creates a potential drag so keyboard/mouse selection stays unified.
        // Only a preceding terminal Drag event may activate it; mouse-up by itself must never turn
        // a click into a move (especially a self-drop on a directory row).
        let drag_active = self
            .app
            .explorer_state()
            .and_then(|state| state.drag.as_ref())
            .is_some_and(|drag| drag.active);
        if drag_active {
            self.update_explorer_drag(coordinates, modifiers, platform);
        }
        let has_target = self
            .app
            .explorer_state()
            .and_then(|state| state.drag.as_ref())
            .and_then(|drag| drag.target.as_ref())
            .is_some();
        self.apply_explorer_command(
            if has_target {
                ExplorerCommand::DropDrag
            } else {
                ExplorerCommand::CancelDrag
            },
            platform,
        );
    }

    pub(in crate::session) fn begin_explorer_scrollbar_drag(&mut self, coordinates: CellPosition) {
        let area = Rect::new(0, 0, self.terminal_size.0, self.terminal_size.1);
        let ui::ShellLayout::Full { main, .. } = ui::compute_shell_layout(area) else {
            return;
        };
        let layout = ui::explorer_layout(main, &self.to_explorer_view_model());
        let Some(scrollbar) = layout.scrollbar else {
            return;
        };
        if !rect_contains(scrollbar.thumb, coordinates) {
            return;
        }
        let _ = self.update_explorer_state(|state| {
            state.drag = None;
        });
        self.scrollbar_drag = Some(ScrollbarDragState::Explorer {
            grab_offset: coordinates.1.saturating_sub(scrollbar.thumb.y),
        });
    }

    pub(in crate::session) fn drag_explorer_scrollbar(&mut self, coordinates: CellPosition) {
        let Some(ScrollbarDragState::Explorer { grab_offset }) = self.scrollbar_drag else {
            return;
        };
        let area = Rect::new(0, 0, self.terminal_size.0, self.terminal_size.1);
        let ui::ShellLayout::Full { main, .. } = ui::compute_shell_layout(area) else {
            return;
        };
        let model = self.to_explorer_view_model();
        let layout = ui::explorer_layout(main, &model);
        let Some(scrollbar) = layout.scrollbar else {
            self.clear_explorer_scrollbar_drag();
            return;
        };
        let window_start = scrollbar_window_start(
            coordinates.1,
            grab_offset,
            scrollbar.track.y,
            scrollbar.track.height,
            scrollbar.thumb.height,
            model.entries.len(),
            layout.visible_capacity,
        );
        let _ = self.update_explorer_state(|state| {
            state.viewport_offset = window_start;
            state.viewport_follows_focus = false;
        });
    }

    pub(in crate::session) fn clear_explorer_scrollbar_drag(&mut self) -> bool {
        if matches!(
            self.scrollbar_drag,
            Some(ScrollbarDragState::Explorer { .. })
        ) {
            self.scrollbar_drag = None;
            true
        } else {
            false
        }
    }

    pub(in crate::session) fn explorer_index_at(&self, coordinates: CellPosition) -> Option<usize> {
        match self.explorer_hit_target_at(coordinates) {
            Some(ui::ExplorerHitTarget::Entry(index)) => Some(index),
            _ => None,
        }
    }

    pub(in crate::session) fn explorer_hit_target_at(
        &self,
        coordinates: CellPosition,
    ) -> Option<ui::ExplorerHitTarget> {
        let area = Rect::new(0, 0, self.terminal_size.0, self.terminal_size.1);
        let ui::ShellLayout::Full { main, .. } = ui::compute_shell_layout(area) else {
            return None;
        };
        if !rect_contains(main, coordinates) {
            return None;
        }
        let explorer = self.to_explorer_view_model();
        ui::explorer_layout(main, &explorer).hit_test(coordinates.0, coordinates.1)
    }

    pub(in crate::session) fn explorer_drop_destination_at(
        &self,
        coordinates: CellPosition,
    ) -> Option<std::path::PathBuf> {
        let target = self.explorer_hit_target_at(coordinates)?;
        let model = self.to_explorer_view_model();
        match target {
            ui::ExplorerHitTarget::Entry(index) => self
                .app
                .explorer_state()
                .and_then(|state| state.entries.get(index))
                .filter(|entry| entry.kind == app::explorer::ExplorerEntryKind::Directory)
                .map(|entry| entry.path.clone()),
            ui::ExplorerHitTarget::QuickLocation(index) => model
                .quick_locations
                .get(index)
                .filter(|location| {
                    location.enabled
                        && location.kind != app::explorer::ExplorerQuickLocationKind::Trash
                })
                .map(|location| std::path::PathBuf::from(&location.path)),
            ui::ExplorerHitTarget::Breadcrumb(index) => model
                .breadcrumbs
                .get(index)
                .filter(|breadcrumb| breadcrumb.enabled)
                .map(|breadcrumb| std::path::PathBuf::from(&breadcrumb.path)),
            ui::ExplorerHitTarget::EmptyTable => self
                .app
                .explorer_state()
                .filter(|state| !state.current_location.is_trash())
                .map(|state| state.current_path.clone()),
            _ => None,
        }
    }
}

pub(in crate::session) fn explorer_toggle_modifier(
    kind: PlatformKind,
    modifiers: InputModifiers,
) -> bool {
    match kind {
        PlatformKind::Macos => modifiers.super_key || modifiers.control,
        PlatformKind::Windows | PlatformKind::Unsupported => modifiers.control,
    }
}

pub(in crate::session) fn explorer_copy_modifier(
    kind: PlatformKind,
    modifiers: InputModifiers,
) -> bool {
    match kind {
        PlatformKind::Macos => modifiers.alt,
        PlatformKind::Windows | PlatformKind::Unsupported => modifiers.control,
    }
}

pub(in crate::session) fn explorer_command_requires_global_settings_permission(
    command: &ExplorerCommand,
) -> bool {
    matches!(
        command,
        ExplorerCommand::ToggleHidden
            | ExplorerCommand::ToggleSystem
            | ExplorerCommand::ToggleExtensions
            | ExplorerCommand::ToggleFoldersFirst
            | ExplorerCommand::ToggleCaseSensitiveSort
            | ExplorerCommand::ToggleSidebar
            | ExplorerCommand::ToggleSizeFormat
            | ExplorerCommand::ToggleDateZone
            | ExplorerCommand::ToggleDeleteConfirmation
            | ExplorerCommand::ToggleConflictConfirmation
    )
}

pub(in crate::session) fn explorer_sort_field(
    column: ui::ExplorerSortColumn,
) -> app::explorer::ExplorerSortField {
    match column {
        ui::ExplorerSortColumn::Name => app::explorer::ExplorerSortField::Name,
        ui::ExplorerSortColumn::Type => app::explorer::ExplorerSortField::Type,
        ui::ExplorerSortColumn::Size => app::explorer::ExplorerSortField::Size,
        ui::ExplorerSortColumn::Modified => app::explorer::ExplorerSortField::Modified,
    }
}
