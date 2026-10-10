use crate::session::*;

impl ShellSession {
    pub(in crate::session) fn open_diagnostics_editor(
        &mut self,
        reload: EditorReloadPolicy,
    ) -> Result<(), i18n::LocalizedText> {
        if self.app.editor_state().is_some_and(EditorState::is_dirty) {
            return Err(i18n::LocalizedText::from(i18n::msg!(
                "shell-the-current-editor-document-has-unsaved-changes-close-it-before-opening-diagnostics"
            )));
        }
        let path = reload.path().to_path_buf();
        self.begin_editor_open_task(path, EditorTaskAccess::ReadOnly, Some(reload), false)
    }

    pub(in crate::session) fn begin_editor_open_task(
        &mut self,
        path: std::path::PathBuf,
        access: EditorTaskAccess,
        reload: Option<EditorReloadPolicy>,
        replacing_dirty: bool,
    ) -> Result<(), i18n::LocalizedText> {
        if self.editor_load_state.is_some() {
            return Err(i18n::LocalizedText::from(i18n::msg!(
                "shell-another-editor-document-is-already-loading"
            )));
        }
        if self.editor_save_state.is_some() {
            return Err(i18n::LocalizedText::from(i18n::msg!(
                "shell-the-current-editor-document-is-still-saving"
            )));
        }
        let navigation = match self.active_screen() {
            ShellScreen::Explorer
                if matches!(self.explorer_purpose, ExplorerPurpose::EditorOpen) =>
            {
                EditorLoadNavigation::EditorPicker
            }
            ShellScreen::Explorer => EditorLoadNavigation::Explorer,
            ShellScreen::Diagnostics => EditorLoadNavigation::Diagnostics,
            ShellScreen::SystemStatus
                if matches!(
                    self.system_status_route,
                    ui::SystemStatusRoute::Detail(
                        ui::SystemStatusDetail::Diagnostics
                            | ui::SystemStatusDetail::Logs
                            | ui::SystemStatusDetail::Incidents
                    )
                ) =>
            {
                EditorLoadNavigation::Diagnostics
            }
            _ => EditorLoadNavigation::Editor,
        };
        let id = next_editor_task_id();
        self.editor_task_runtime.submit_load_with_owner(
            id,
            path.clone(),
            access,
            self.app.auth_session().map(|s| s.user_id.clone()),
        )?;

        let rollback =
            self.begin_editor_navigation(navigation == EditorLoadNavigation::EditorPicker);
        self.editor_load_state = Some(EditorLoadState {
            id,
            path: path.clone(),
            stage: EditorTaskStage::Inspecting,
            completed_bytes: 0,
            total_bytes: None,
            operation: EditorLoadOperation::Open {
                navigation,
                rollback,
                reload,
                replacing_dirty,
            },
        });
        self.editor_focus = ui::EditorFocus::Canvas;
        self.editor_open_menu = None;
        self.editor_selected_toolbar_action = None;
        self.editor_quick_menu_anchor = None;
        self.editor_drag_anchor = None;
        self.active_popup = None;
        self.focused_component = ShellComponent::Editor;
        self.editor_message = Some(i18n::LocalizedText::from(i18n::msg!(
            "shell-loading-arg1",
            arg1 = path.display().to_string()
        )));
        self.notify_status(i18n::LocalizedText::from(i18n::msg!(
            "shell-loading-arg1",
            arg1 = path.display().to_string()
        )));
        self.refresh_hit_map();
        Ok(())
    }

    pub(in crate::session) fn reload_log_editor(&mut self) {
        if self.refresh_logs_editor_snapshot() {
            return;
        }
        self.reload_log_editor_file();
    }

    pub(in crate::session) fn reload_log_editor_file(&mut self) {
        let Some(session) = self.editor_read_session.clone() else {
            return;
        };
        if !matches!(session.reload, EditorReloadPolicy::Log { .. }) {
            return;
        }
        let path = session.reload.path().to_path_buf();
        let model = self.to_editor_view_model();
        let area = Rect::new(0, 0, self.terminal_size.0, self.terminal_size.1);
        let editor_area = match self.shell_layout_for(area) {
            ui::ShellLayout::Compact(compact) => compact,
            ui::ShellLayout::Full { main, .. } => main,
        };
        let layout = ui::editor_layout(editor_area, &model);
        let visible_capacity = layout.visible_capacity.max(1);
        let old_maximum = layout.document_line_count.saturating_sub(visible_capacity);
        let was_at_bottom = layout.visible_start >= old_maximum;
        let old_top_line = layout.visible_start;
        let (old_left_column, old_cursor) = self
            .app
            .editor_state()
            .map(|state| (state.viewport.left_column, state.cursor.byte_offset))
            .unwrap_or_default();
        let id = next_editor_task_id();
        if let Err(error) = self.editor_task_runtime.submit_load_with_owner(
            id,
            path.clone(),
            EditorTaskAccess::ReadOnly,
            self.app.auth_session().map(|s| s.user_id.clone()),
        ) {
            self.report_editor_error(i18n::LocalizedText::from(i18n::msg!(
                "shell-could-not-reload-arg1-error",
                arg1 = path.display().to_string(),
                error = error.to_string()
            )));
            return;
        }
        self.editor_load_state = Some(EditorLoadState {
            id,
            path: path.clone(),
            stage: EditorTaskStage::Inspecting,
            completed_bytes: 0,
            total_bytes: None,
            operation: EditorLoadOperation::Reload {
                session,
                was_at_bottom,
                visible_capacity,
                old_top_line,
                old_left_column,
                old_cursor,
            },
        });
        self.editor_message = Some(i18n::LocalizedText::from(i18n::msg!(
            "shell-reloading-arg1",
            arg1 = path.display().to_string()
        )));
        self.refresh_hit_map();
    }

    pub(in crate::session) fn cancel_editor_load(&mut self) {
        let Some(load) = self.editor_load_state.take() else {
            return;
        };
        self.editor_task_runtime.cancel(load.id);
        self.restore_editor_load_navigation(&load.operation);
        self.editor_message = Some(i18n::LocalizedText::from(i18n::msg!(
            "shell-loading-cancelled"
        )));
        self.notify_status(i18n::LocalizedText::from(i18n::msg!(
            "shell-loading-cancelled"
        )));
        self.refresh_hit_map();
    }

    pub(in crate::session) fn poll_editor_background_tasks(&mut self, platform: &dyn Platform) {
        let events = self.editor_task_runtime.drain_events();
        if events.is_empty() {
            return;
        }
        for event in events {
            match event {
                EditorTaskEvent::Progress {
                    id,
                    stage,
                    completed_bytes,
                    total_bytes,
                } => {
                    if let Some(load) = self.editor_load_state.as_mut().filter(|load| load.id == id)
                    {
                        load.stage = stage;
                        load.completed_bytes = completed_bytes;
                        load.total_bytes = total_bytes;
                    }
                    if let Some(save) = self.editor_save_state.as_mut().filter(|save| save.id == id)
                    {
                        save.stage = stage;
                    }
                }
                EditorTaskEvent::LoadFinished { id, result } => {
                    let Some(load) = self.editor_load_state.take_if(|load| load.id == id) else {
                        continue;
                    };
                    match *result {
                        Ok(document) => self.finish_editor_load(load, document),
                        Err(error) => {
                            let action =
                                if matches!(load.operation, EditorLoadOperation::Reload { .. }) {
                                    i18n::LocalizedText::from(i18n::msg!("shell-reload"))
                                } else {
                                    i18n::LocalizedText::from(i18n::msg!("shell-open"))
                                };
                            self.restore_editor_load_navigation(&load.operation);
                            if error != "Editor load cancelled" {
                                self.report_editor_error(i18n::LocalizedText::from(i18n::msg!(
                                    "shell-could-not-action-arg1-error",
                                    action = action,
                                    arg1 = load.path.display().to_string(),
                                    error = error.to_string()
                                )));
                            }
                        }
                    }
                }
                EditorTaskEvent::SaveFinished { id, result } => {
                    let Some(save) = self.editor_save_state.take_if(|save| save.id == id) else {
                        continue;
                    };
                    self.finish_editor_save(save, result, platform);
                }
            }
        }
        self.refresh_hit_map();
    }

    pub(in crate::session) fn finish_editor_save(
        &mut self,
        save: EditorSaveState,
        result: Result<DocumentFingerprint, EditorSaveTaskError>,
        platform: &dyn Platform,
    ) {
        if save.document_generation != self.editor_document_generation {
            self.editor_close_after_save = false;
            self.editor_open_after_save = false;
            return;
        }
        let path = save.path;
        match result {
            Ok(fingerprint) => {
                self.app.dispatch_at(
                    app::AppCommand::Editor(app::editor::EditorCommand::MarkSaved {
                        path: Some(path.clone()),
                        revision: save.revision,
                    }),
                    Instant::now(),
                );
                let _ = self.app.take_editor_effects();
                self.editor_fingerprint = Some(fingerprint);
                self.clear_editor_recovery();
                if self.app.editor_state().is_some_and(EditorState::is_dirty) {
                    self.editor_recovery_dirty_since = Some(Instant::now());
                }
                self.error_message = None;
                self.resolve_notification_alert(EDITOR_ALERT_KEY);
                self.editor_message = Some(i18n::LocalizedText::from(i18n::msg!(
                    "shell-saved-arg1",
                    arg1 = path.display().to_string()
                )));
                self.notify_toast(i18n::LocalizedText::from(i18n::msg!(
                    "shell-saved-arg1",
                    arg1 = path.display().to_string()
                )));
                let close_after_save = std::mem::take(&mut self.editor_close_after_save);
                let open_after_save = std::mem::take(&mut self.editor_open_after_save);
                let clean = self
                    .app
                    .editor_state()
                    .is_none_or(|state| !state.is_dirty());
                if close_after_save && clean {
                    self.finish_editor_close(false);
                } else if open_after_save && clean {
                    self.continue_editor_open_after_save(platform);
                } else if !clean && (close_after_save || open_after_save) {
                    self.editor_message = Some(i18n::LocalizedText::from(i18n::msg!(
                        "shell-saved-an-earlier-revision-newer-edits-are-still-unsaved"
                    )));
                }
            }
            Err(EditorSaveTaskError::ExternalModification) => {
                self.editor_close_after_save = false;
                self.editor_open_after_save = false;
                self.report_editor_error(
                    i18n::LocalizedText::from(i18n::msg!("shell-the-file-changed-outside-the-editor-use-save-as-or-reload-it-before-saving")),
                );
            }
            Err(EditorSaveTaskError::Write(error)) => {
                self.editor_close_after_save = false;
                self.editor_open_after_save = false;
                self.report_editor_error(i18n::LocalizedText::from(i18n::msg!(
                    "shell-could-not-save-arg1-error",
                    arg1 = path.display().to_string(),
                    error = error.to_string()
                )));
            }
        }
    }

    pub(in crate::session) fn finish_editor_load(
        &mut self,
        load: EditorLoadState,
        document: EditorLoadedTaskDocument,
    ) {
        self.advance_editor_document_generation();
        let path = load.path;
        let EditorLoadedTaskDocument {
            mut state,
            fingerprint,
            total_bytes,
            rich_blocks,
        } = document;
        match load.operation {
            EditorLoadOperation::Open {
                navigation,
                reload,
                replacing_dirty,
                ..
            } => {
                let open_at_bottom = reload
                    .as_ref()
                    .is_some_and(|reload| matches!(reload, EditorReloadPolicy::Log { .. }));
                if open_at_bottom {
                    let _ = state.apply(app::editor::EditorCommand::MoveCursor {
                        movement: app::editor::CursorMove::DocumentEnd,
                        extend_selection: false,
                    });
                    state.viewport.top_line = state
                        .source_line_count()
                        .unwrap_or_else(|| state.document.line_count())
                        .saturating_sub(1);
                }
                if replacing_dirty {
                    self.clear_editor_recovery();
                }
                self.editor_read_session = reload.map(|reload| EditorReadSession {
                    reload,
                    total_bytes,
                });
                self.editor_rich_render_cache = rich_blocks.map(|blocks| EditorRichRenderCache {
                    revision: state.revision(),
                    blocks,
                });
                self.app
                    .dispatch_at(app::AppCommand::SetEditorState(Some(state)), Instant::now());
                self.editor_cursor_acceleration = None;
                self.editor_settings_dialog = None;
                self.editor_fingerprint = Some(fingerprint);
                self.editor_focus = ui::EditorFocus::Canvas;
                self.editor_open_menu = None;
                self.editor_selected_toolbar_action = None;
                self.editor_quick_menu_anchor = None;
                self.editor_drag_anchor = None;
                self.editor_table_column_widths.clear();
                self.editor_table_resize = None;
                self.editor_close_after_save = false;
                self.editor_open_after_save = false;
                self.editor_discard_for_open = false;
                self.editor_recovery_dirty_since = None;
                self.editor_last_recovery_write = None;
                if navigation == EditorLoadNavigation::EditorPicker {
                    self.explorer_purpose = ExplorerPurpose::Browse;
                    self.replace_explorer_state(None);
                }
                let read_only = self
                    .app
                    .editor_state()
                    .is_some_and(EditorState::is_read_only);
                self.editor_message = Some(if read_only {
                    i18n::LocalizedText::from(i18n::msg!(
                        "shell-read-only-arg1",
                        arg1 = path.display().to_string()
                    ))
                } else {
                    i18n::LocalizedText::from(i18n::msg!(
                        "shell-opened-arg1",
                        arg1 = path.display().to_string()
                    ))
                });
            }
            EditorLoadOperation::Reload {
                session,
                was_at_bottom,
                visible_capacity,
                old_top_line,
                old_left_column,
                old_cursor,
            } => {
                let new_line_count = state
                    .source_line_count()
                    .unwrap_or_else(|| state.document.line_count())
                    .max(1);
                let new_maximum = new_line_count.saturating_sub(visible_capacity);
                state.viewport.left_column = old_left_column;
                state.selection = None;
                if was_at_bottom {
                    let _ = state.apply(app::editor::EditorCommand::MoveCursor {
                        movement: app::editor::CursorMove::DocumentEnd,
                        extend_selection: false,
                    });
                    state.viewport.top_line = new_maximum;
                } else {
                    let _ = state.apply(app::editor::EditorCommand::MoveTo {
                        position: app::editor::EditorPosition::Source(old_cursor),
                        extend_selection: false,
                    });
                    state.viewport.top_line = old_top_line.min(new_maximum);
                }
                self.app
                    .dispatch_at(app::AppCommand::SetEditorState(Some(state)), Instant::now());
                self.editor_rich_render_cache = rich_blocks.map(|blocks| EditorRichRenderCache {
                    revision: self.app.editor_state().map_or(0, EditorState::revision),
                    blocks,
                });
                self.editor_fingerprint = Some(fingerprint);
                self.editor_read_session = Some(EditorReadSession {
                    reload: session.reload,
                    total_bytes,
                });
                self.editor_quick_menu_anchor = None;
                self.editor_drag_anchor = None;
                self.editor_message = Some(i18n::LocalizedText::from(i18n::msg!(
                    "shell-reloaded-arg1",
                    arg1 = path.display().to_string()
                )));
            }
        }
        if self.active_screen() == ShellScreen::Editor {
            self.active_popup = None;
            self.focused_component = ShellComponent::Editor;
        }
        self.notify_status(i18n::LocalizedText::from(i18n::msg!(
            "shell-editor-arg1",
            arg1 = path.display().to_string()
        )));
        self.refresh_hit_map();
    }

    pub(in crate::session) fn open_editor_path(&mut self, path: std::path::PathBuf) -> bool {
        if self.config_editor_active() {
            self.notify_toast(i18n::msg!("config-editor-finish-document"));
            return false;
        }
        if Self::is_system_config_path(&path) {
            self.begin_config_editor(path, None, None, None, "system".into());
            return self.config_editor_active();
        }
        let replacing_dirty = self.app.editor_state().is_some_and(EditorState::is_dirty);
        if replacing_dirty && !self.editor_discard_for_open {
            self.report_editor_error(
                i18n::LocalizedText::from(i18n::msg!("shell-the-current-document-has-unsaved-changes-use-open-in-the-editor-and-choose-save-or-discard-first")),
            );
            return false;
        }
        if !self.authorize_editor_file(PermissionAction::ReadFile, &path) {
            return false;
        }
        let is_log = is_log_document_path(&path);
        let access = if is_log {
            EditorTaskAccess::ReadOnly
        } else {
            EditorTaskAccess::Editable
        };
        let reload = is_log.then(|| EditorReloadPolicy::Log { path: path.clone() });
        match self.begin_editor_open_task(path.clone(), access, reload, replacing_dirty) {
            Ok(()) => true,
            Err(error) => {
                self.report_editor_error(i18n::LocalizedText::from(i18n::msg!(
                    "shell-could-not-open-arg1-error",
                    arg1 = path.display().to_string(),
                    error = error
                )));
                false
            }
        }
    }

    pub(in crate::session) fn confirm_editor_open(&mut self) {
        self.notify_modal_with_options(
            ShellNotification::modal(
                i18n::LocalizedText::from(i18n::msg!("shell-unsaved-document")),
                i18n::LocalizedText::from(i18n::msg!(
                    "shell-save-your-changes-before-opening-another-document"
                )),
                ui::NotificationTone::Warning,
                vec![
                    ShellNotificationAction::new(
                        "save",
                        i18n::LocalizedText::from(i18n::msg!("shell-save")),
                    )
                    .with_follow_up(ShellCommand::EditorSaveAndOpen),
                    ShellNotificationAction::new(
                        "discard",
                        i18n::LocalizedText::from(i18n::msg!("shell-discard")),
                    )
                    .with_follow_up(ShellCommand::EditorDiscardAndOpen),
                    ShellNotificationAction::new(
                        "cancel",
                        i18n::LocalizedText::from(i18n::msg!("shell-cancel")),
                    )
                    .cancel()
                    .with_follow_up(ShellCommand::EditorCancelOpen),
                ],
            )
            .with_key(EDITOR_OPEN_NOTIFICATION_KEY)
            .with_component(ShellComponent::NotificationDialog),
        );
    }

    pub(in crate::session) fn open_editor_picker(&mut self, platform: &dyn Platform) {
        self.open_explorer(platform);
        if self.active_screen() == ShellScreen::Explorer {
            self.explorer_purpose = ExplorerPurpose::EditorOpen;
            self.notify_status(i18n::LocalizedText::from(i18n::msg!(
                "shell-choose-a-markdown-or-text-document"
            )));
        } else {
            self.editor_open_after_save = false;
            self.editor_discard_for_open = false;
            self.report_editor_error(i18n::LocalizedText::from(i18n::msg!(
                "shell-could-not-open-the-file-picker"
            )));
        }
    }

    pub(in crate::session) fn open_editor_save_picker(
        &mut self,
        platform: &dyn Platform,
        suggested_name: String,
        snapshot: app::editor::SaveSnapshot,
    ) {
        self.open_explorer(platform);
        if self.active_screen() != ShellScreen::Explorer {
            self.editor_close_after_save = false;
            self.editor_open_after_save = false;
            self.report_editor_error(i18n::LocalizedText::from(i18n::msg!(
                "shell-could-not-open-the-save-as-picker"
            )));
            return;
        }
        self.explorer_purpose = ExplorerPurpose::EditorSaveAs { snapshot };
        self.begin_explorer_input(ExplorerInputMode::NewTextFile);
        self.explorer_input = suggested_name;
        self.explorer_input_replace_all = true;
        self.notify_status(i18n::LocalizedText::from(i18n::msg!(
            "shell-save-as-enter-a-file-name-in-the-current-directory"
        )));
    }

    pub(in crate::session) fn submit_editor_save_as_from_explorer(
        &mut self,
        platform: &dyn Platform,
    ) -> bool {
        let ExplorerPurpose::EditorSaveAs { snapshot, .. } = self.explorer_purpose.clone() else {
            return false;
        };
        if self.explorer_input_mode != ExplorerInputMode::NewTextFile {
            return false;
        }
        let name = self.explorer_input.trim();
        let name_path = std::path::Path::new(name);
        let valid_name = !name.is_empty()
            && !name_path.is_absolute()
            && name_path.components().count() == 1
            && matches!(
                name_path.components().next(),
                Some(std::path::Component::Normal(_))
            );
        if !valid_name {
            let _ = self.update_explorer_state(|state| {
                state.error = Some(i18n::LocalizedText::from(i18n::msg!(
                    "shell-enter-a-single-file-name-without-path-separators"
                )));
                state.message = None;
            });
            return true;
        }
        let Some(directory) = self
            .app
            .explorer_state()
            .map(|state| state.current_path.clone())
        else {
            self.report_editor_error(i18n::LocalizedText::from(i18n::msg!(
                "shell-save-as-destination-is-unavailable"
            )));
            return true;
        };
        let path = directory.join(name);
        if self.save_editor_document(path, snapshot, platform)
            && self.active_screen() == ShellScreen::Explorer
            && !matches!(self.explorer_purpose, ExplorerPurpose::EditorOpen)
        {
            self.return_from_editor_picker();
        }
        true
    }

    pub(in crate::session) fn return_from_editor_picker(&mut self) {
        self.return_from_screen(ShellScreen::Explorer);
        self.explorer_purpose = ExplorerPurpose::Browse;
        self.replace_explorer_state(None);
        self.explorer_input_mode = ExplorerInputMode::Browse;
        self.explorer_input.clear();
        self.explorer_input_replace_all = false;
        self.editor_discard_for_open = false;
        self.refresh_hit_map();
    }

    pub(in crate::session) fn save_editor_document(
        &mut self,
        path: std::path::PathBuf,
        snapshot: app::editor::SaveSnapshot,
        _platform: &dyn Platform,
    ) -> bool {
        if self.config_editor_active() {
            self.config_editor_action(ui::EditorConfigAction::Preview);
            return false;
        }
        if Self::is_system_config_path(&path) {
            self.begin_config_save(path);
            return false;
        }
        if self.editor_save_state.is_some() || self.editor_load_state.is_some() {
            return false;
        }
        if !self.authorize_editor_file(PermissionAction::WriteFile, &path) {
            self.editor_close_after_save = false;
            self.editor_open_after_save = false;
            return false;
        }
        let is_current_path = self
            .app
            .editor_state()
            .and_then(|state| state.document.path.as_ref())
            == Some(&path);
        let expected = is_current_path.then_some(self.editor_fingerprint).flatten();
        let id = next_editor_task_id();
        let revision = snapshot.revision;
        if let Err(error) = self.editor_task_runtime.submit_save_with_owner(
            id,
            path.clone(),
            snapshot,
            expected,
            self.app.auth_session().map(|s| s.user_id.clone()),
        ) {
            self.editor_close_after_save = false;
            self.editor_open_after_save = false;
            self.report_editor_error(i18n::LocalizedText::from(i18n::msg!(
                "shell-could-not-save-arg1-error",
                arg1 = path.display().to_string(),
                error = error.to_string()
            )));
            return false;
        }
        if self.active_screen() == ShellScreen::Explorer
            && matches!(self.explorer_purpose, ExplorerPurpose::EditorSaveAs { .. })
        {
            self.return_from_editor_picker();
        }
        self.editor_save_state = Some(EditorSaveState {
            id,
            path: path.clone(),
            document_generation: self.editor_document_generation,
            revision,
            stage: EditorTaskStage::Writing,
        });
        self.editor_message = Some(i18n::LocalizedText::from(i18n::msg!(
            "shell-saving-arg1",
            arg1 = path.display().to_string()
        )));
        self.notify_status(i18n::LocalizedText::from(i18n::msg!(
            "shell-saving-arg1",
            arg1 = path.display().to_string()
        )));
        self.refresh_hit_map();
        true
    }

    pub(in crate::session) fn continue_editor_open_after_save(&mut self, platform: &dyn Platform) {
        if self.active_screen() == ShellScreen::Explorer {
            self.explorer_purpose = ExplorerPurpose::EditorOpen;
            self.explorer_input_mode = ExplorerInputMode::Browse;
            self.explorer_input.clear();
            self.explorer_input_replace_all = false;
            self.explorer_overlay_mode = None;
            self.focused_component = ShellComponent::Explorer;
            self.notify_status(i18n::LocalizedText::from(i18n::msg!(
                "shell-choose-a-markdown-or-text-document"
            )));
            self.apply_explorer_command(ExplorerCommand::Refresh, platform);
            self.refresh_hit_map();
        } else {
            self.open_editor_picker(platform);
        }
    }

    pub(in crate::session) fn authorize_editor_file(
        &mut self,
        action: PermissionAction,
        path: &std::path::Path,
    ) -> bool {
        if self.storage_manager.is_none() {
            return true;
        }
        let authorization = PermissionService::new(self.debug_policy).authorize(
            self.app.auth_session(),
            action,
            Some(path.display().to_string().as_str()),
        );
        if authorization.allowed {
            return true;
        }
        let reason = authorization
            .reason
            .unwrap_or_else(|| "permission_denied".to_string());
        self.report_editor_error(i18n::LocalizedText::from(i18n::msg!(
            "shell-permission-denied-reason",
            reason = reason.to_string()
        )));
        false
    }
}
