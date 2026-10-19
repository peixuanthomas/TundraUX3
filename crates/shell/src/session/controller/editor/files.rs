use crate::session::*;

impl ShellSession {
    fn editor_open_reuses_document(&self) -> bool {
        self.active_screen() == ShellScreen::Editor
            || (self.active_screen() == ShellScreen::Explorer
                && matches!(
                    self.pages.explorer.explorer_purpose,
                    ExplorerPurpose::EditorOpen
                ))
    }

    pub(in crate::session) fn open_diagnostics_editor(
        &mut self,
        reload: EditorReloadPolicy,
    ) -> Result<(), i18n::LocalizedText> {
        if self.editor_open_reuses_document()
            && self.app.editor_state().is_some_and(EditorState::is_dirty)
        {
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
        if self.editor_open_reuses_document() && self.pages.editor.editor_load_state.is_some() {
            return Err(i18n::LocalizedText::from(i18n::msg!(
                "shell-another-editor-document-is-already-loading"
            )));
        }
        if self.editor_open_reuses_document() && self.pages.editor.editor_save_state.is_some() {
            return Err(i18n::LocalizedText::from(i18n::msg!(
                "shell-the-current-editor-document-is-still-saving"
            )));
        }
        let navigation = match self.active_screen() {
            ShellScreen::Explorer
                if matches!(
                    self.pages.explorer.explorer_purpose,
                    ExplorerPurpose::EditorOpen
                ) =>
            {
                EditorLoadNavigation::EditorPicker
            }
            ShellScreen::Explorer => EditorLoadNavigation::Explorer,
            ShellScreen::Diagnostics => EditorLoadNavigation::Diagnostics,
            ShellScreen::SystemStatus
                if matches!(
                    self.pages.system_status.system_status_route,
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
        let rollback =
            self.begin_editor_navigation(navigation == EditorLoadNavigation::EditorPicker);
        let operation = EditorLoadOperation::Open {
            navigation,
            rollback,
            reload,
            replacing_dirty,
        };
        let id = next_editor_task_id();
        if let Err(error) = self.editor_task_runtime.submit_load_with_owner(
            id,
            path.clone(),
            access,
            self.app.auth_session().map(|s| s.user_id.clone()),
        ) {
            self.restore_editor_load_navigation(&operation);
            return Err(error.into());
        }
        self.register_page_task(
            TaskDomain::Editor,
            id,
            TaskClass::Query,
            self.editor_task_runtime.task_control(id, true),
        );
        self.pages.editor.editor_load_state = Some(EditorLoadState {
            id,
            path: path.clone(),
            stage: EditorTaskStage::Inspecting,
            completed_bytes: 0,
            total_bytes: None,
            operation,
        });
        self.pages.editor.editor_focus = ui::EditorFocus::Canvas;
        self.pages.editor.editor_open_menu = None;
        self.pages.editor.editor_selected_toolbar_action = None;
        self.pages.editor.editor_quick_menu_anchor = None;
        self.pages.editor.editor_drag_anchor = None;
        self.pages.common.active_popup = None;
        self.pages.common.focused_component = ShellComponent::Editor;
        self.pages.editor.editor_message = Some(i18n::LocalizedText::from(i18n::msg!(
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
        let Some(session) = self.pages.editor.editor_read_session.clone() else {
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
        self.register_page_task(
            TaskDomain::Editor,
            id,
            TaskClass::Query,
            self.editor_task_runtime.task_control(id, true),
        );
        self.pages.editor.editor_load_state = Some(EditorLoadState {
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
        self.pages.editor.editor_message = Some(i18n::LocalizedText::from(i18n::msg!(
            "shell-reloading-arg1",
            arg1 = path.display().to_string()
        )));
        self.refresh_hit_map();
    }

    pub(in crate::session) fn cancel_editor_load(&mut self) {
        let Some(load) = self.pages.editor.editor_load_state.take() else {
            return;
        };
        self.editor_task_runtime.cancel(load.id);
        self.tasks.cancel(TaskDomain::Editor, load.id);
        let owner = self.pages.editor.selected_id();
        self.restore_editor_load_navigation(&load.operation);
        if self.pages.editor.selected_id() == owner {
            self.pages.editor.editor_message = Some(i18n::msg!("shell-loading-cancelled").into());
        }
        self.notify_status(i18n::LocalizedText::from(i18n::msg!(
            "shell-loading-cancelled"
        )));
        self.refresh_hit_map();
    }

    pub(in crate::session) fn poll_editor_background_tasks(&mut self, platform: &dyn Platform) {
        self.poll_editor_preview();
        let cursor = self.app.editor_state().map(|s| s.cursor.byte_offset);
        let computation = self.app.poll_editor_computations();
        if computation.changed {
            if cursor != self.app.editor_state().map(|s| s.cursor.byte_offset)
                || (computation.wrapped && self.pages.editor.editor_soft_wrap)
            {
                self.reveal_source_caret();
            }
            self.refresh_hit_map();
        }
        self.editor_task_runtime
            .retry_recovery_cleanup(Instant::now());
        let events = self.editor_task_runtime.drain_events();
        if events.is_empty() {
            self.resume_pending_editor_open(platform);
            return;
        }
        for event in events {
            let (id, done) = match &event {
                EditorTaskEvent::Progress { id, .. } => (*id, false),
                EditorTaskEvent::LoadFinished { id, .. }
                | EditorTaskEvent::SaveFinished { id, .. } => (*id, true),
            };
            let owner = self.tasks.owner(TaskDomain::Editor, id);
            if done {
                self.tasks.complete(TaskDomain::Editor, id);
            }
            match owner {
                Some(TaskOwner::Page(owner)) => {
                    self.with_page_context(owner, |session| {
                        session.apply_editor_task_event(event, platform)
                    });
                }
                Some(TaskOwner::Session) => {
                    if let EditorTaskEvent::SaveFinished { id, result } = event {
                        self.finish_detached_editor_save(id, result, platform);
                    }
                }
                None => {}
            }
            if done {
                self.editor_task_runtime.take_detached_save(id);
            }
        }
        self.resume_pending_editor_open(platform);
        self.refresh_hit_map();
    }

    fn resume_pending_editor_open(&mut self, platform: &dyn Platform) {
        if self.active_screen() == ShellScreen::Editor
            && self.pages.editor.editor_save_state.is_none()
            && self.pages.editor.editor_load_state.is_none()
            && self.pages.editor.editor_open_after_save
            && self
                .app
                .editor_state()
                .is_some_and(|state| !state.is_dirty())
        {
            self.pages.editor.editor_open_after_save = false;
            self.continue_editor_open_after_save(platform);
        }
    }

    fn apply_editor_task_event(&mut self, event: EditorTaskEvent, platform: &dyn Platform) {
        match event {
            EditorTaskEvent::Progress {
                id,
                stage,
                completed_bytes,
                total_bytes,
            } => {
                if let Some(load) = self
                    .pages
                    .editor
                    .editor_load_state
                    .as_mut()
                    .filter(|load| load.id == id)
                {
                    load.stage = stage;
                    load.completed_bytes = completed_bytes;
                    load.total_bytes = total_bytes;
                }
                if let Some(save) = self
                    .pages
                    .editor
                    .editor_save_state
                    .as_mut()
                    .filter(|save| save.id == id)
                {
                    save.stage = stage;
                }
            }
            EditorTaskEvent::LoadFinished { id, result } => {
                let Some(load) = self
                    .pages
                    .editor
                    .editor_load_state
                    .take_if(|load| load.id == id)
                else {
                    return;
                };
                match *result {
                    Ok(document) => self.finish_editor_load(load, document),
                    Err(error) => {
                        let action = if matches!(load.operation, EditorLoadOperation::Reload { .. })
                        {
                            i18n::LocalizedText::from(i18n::msg!("shell-reload"))
                        } else {
                            i18n::LocalizedText::from(i18n::msg!("shell-open"))
                        };
                        let owner = self.pages.editor.selected_id();
                        let origin = match &load.operation {
                            EditorLoadOperation::Open { rollback, .. } => {
                                Some(rollback.origin_page_id())
                            }
                            EditorLoadOperation::Reload { .. } => owner,
                        };
                        self.restore_editor_load_navigation(&load.operation);
                        if error != "Editor load cancelled" {
                            let message = i18n::LocalizedText::from(i18n::msg!(
                                "shell-could-not-action-arg1-error",
                                action = action,
                                arg1 = load.path.display().to_string(),
                                error = error.to_string()
                            ));
                            // A picker replacement can leave the original Editor alive.
                            // End its loading message even while the picker owns the alert.
                            if let Some(editor) = owner.and_then(|id| self.pages.editor.get_mut(id))
                            {
                                editor.editor_message = Some(message.clone());
                            }
                            let target = origin
                                .filter(|id| self.page_screen(*id).is_some())
                                .or_else(|| owner.filter(|id| self.page_screen(*id).is_some()));
                            if let Some(target) = target {
                                self.with_page_context(target, |session| {
                                    if session.page_screen(target) == Some(ShellScreen::Editor) {
                                        session.report_editor_error(message);
                                    } else {
                                        session.pages.common.error_message = Some(message.clone());
                                        session.notify_page_alert_with_key(
                                            EDITOR_ALERT_KEY,
                                            message,
                                            ui::NotificationTone::Error,
                                        );
                                    }
                                });
                            }
                        }
                    }
                }
            }
            EditorTaskEvent::SaveFinished { id, result } => {
                let Some(save) = self
                    .pages
                    .editor
                    .editor_save_state
                    .take_if(|save| save.id == id)
                else {
                    return;
                };
                self.finish_editor_save(save, result, platform);
            }
        }
    }

    pub(in crate::session) fn prepare_editor_background_close(&mut self) -> bool {
        let Some(save) = self.pages.editor.editor_save_state.clone() else {
            return self.persist_editor_recovery_now(Instant::now());
        };
        if !self.persist_editor_recovery_now(Instant::now()) {
            return false;
        }
        let completion = super::tasks::EditorDetachedSave {
            path: save.path,
            recovery: self.pages.editor.editor_recovery_lease.clone(),
            clear_recovery_on_success: self
                .app
                .editor_state()
                .is_some_and(|state| state.revision() == save.revision),
        };
        if let Err(error) = self
            .editor_task_runtime
            .retain_detached_save(save.id, completion)
        {
            self.report_editor_error(error);
            return false;
        }
        true
    }

    fn finish_detached_editor_save(
        &mut self,
        id: u64,
        result: Result<DocumentFingerprint, EditorSaveTaskError>,
        platform: &dyn Platform,
    ) {
        let Some(completion) = self.editor_task_runtime.take_detached_save(id) else {
            return;
        };
        match result {
            Ok(_) => {
                self.refresh_explorers_containing(&completion.path, platform);
                if completion.clear_recovery_on_success
                    && let Some(lease) = completion.recovery
                    && let Err(error) = app::editor_recovery::clear_editor_recovery(&lease)
                {
                    self.editor_task_runtime.defer_recovery_cleanup(lease);
                    self.notify_alert_with_tone(
                        i18n::msg!(
                            "shell-could-not-clear-recovery-error",
                            error = error.to_string()
                        ),
                        ui::NotificationTone::Error,
                    );
                }
                let message = i18n::msg!(
                    "shell-saved-arg1",
                    arg1 = completion.path.display().to_string()
                );
                self.notify_status(message.clone());
                self.notify_toast(message);
            }
            Err(error) => {
                let error = match error {
                    EditorSaveTaskError::ExternalModification => i18n::tr!(
                        "shell-the-file-changed-outside-the-editor-use-save-as-or-reload-it-before-saving"
                    ),
                    EditorSaveTaskError::Write(error) => error,
                };
                let message = i18n::msg!(
                    "shell-could-not-save-arg1-error",
                    arg1 = completion.path.display().to_string(),
                    error = error
                );
                self.notify_status(message.clone());
                self.notify_alert_with_tone(message, ui::NotificationTone::Error);
            }
        }
    }

    pub(in crate::session) fn finish_editor_save(
        &mut self,
        save: EditorSaveState,
        result: Result<DocumentFingerprint, EditorSaveTaskError>,
        platform: &dyn Platform,
    ) {
        if save.document_generation != self.pages.editor.editor_document_generation {
            self.pages.editor.editor_close_after_save = false;
            self.pages.editor.editor_open_after_save = false;
            return;
        }
        let path = save.path;
        match result {
            Ok(fingerprint) => {
                self.refresh_explorers_containing(&path, platform);
                self.app.dispatch_at(
                    app::AppCommand::Editor(app::editor::EditorCommand::MarkSaved {
                        path: Some(path.clone()),
                        revision: save.revision,
                    }),
                    Instant::now(),
                );
                let _ = self.app.take_editor_effects();
                self.pages.editor.editor_fingerprint = Some(fingerprint);
                self.pages.common.error_message = None;
                self.resolve_page_alert(EDITOR_ALERT_KEY);
                let message = i18n::LocalizedText::from(i18n::msg!(
                    "shell-saved-arg1",
                    arg1 = path.display().to_string()
                ));
                self.pages.editor.editor_message = Some(message.clone());
                self.notify_status(message.clone());
                self.notify_toast(message);
                let close_after_save =
                    std::mem::take(&mut self.pages.editor.editor_close_after_save);
                let open_after_save = std::mem::take(&mut self.pages.editor.editor_open_after_save);
                let clean = self
                    .app
                    .editor_state()
                    .is_none_or(|state| !state.is_dirty());
                if clean {
                    self.clear_editor_recovery();
                } else {
                    let now = Instant::now();
                    self.pages.editor.editor_recovery_dirty_since = Some(now);
                    let _ = self.persist_editor_recovery_now(now);
                }
                if close_after_save && clean {
                    self.finish_editor_close(false);
                } else if open_after_save && clean {
                    if self.pages.editor.selected_id() == Some(self.active_page_id()) {
                        self.continue_editor_open_after_save(platform);
                    } else {
                        self.pages.editor.editor_open_after_save = true;
                    }
                } else if !clean && (close_after_save || open_after_save) {
                    self.pages.editor.editor_message = Some(i18n::LocalizedText::from(i18n::msg!(
                        "shell-saved-an-earlier-revision-newer-edits-are-still-unsaved"
                    )));
                }
            }
            Err(EditorSaveTaskError::ExternalModification) => {
                self.pages.editor.editor_close_after_save = false;
                self.pages.editor.editor_open_after_save = false;
                let message = i18n::msg!(
                    "shell-the-file-changed-outside-the-editor-use-save-as-or-reload-it-before-saving"
                );
                self.notify_status(message.clone());
                self.report_editor_error(message);
            }
            Err(EditorSaveTaskError::Write(error)) => {
                self.pages.editor.editor_close_after_save = false;
                self.pages.editor.editor_open_after_save = false;
                let message = i18n::msg!(
                    "shell-could-not-save-arg1-error",
                    arg1 = path.display().to_string(),
                    error = error.to_string()
                );
                self.notify_status(message.clone());
                self.report_editor_error(message);
            }
        }
    }

    pub(in crate::session) fn finish_editor_load(
        &mut self,
        load: EditorLoadState,
        document: EditorLoadedTaskDocument,
    ) {
        if matches!(load.operation, EditorLoadOperation::Open { .. })
            && !self.reset_editor_recovery_identity()
        {
            self.restore_editor_load_navigation(&load.operation);
            return;
        }
        self.advance_editor_document_generation();
        let navigation_operation = load.operation.clone();
        let path = load.path;
        let EditorLoadedTaskDocument {
            mut state,
            fingerprint,
            total_bytes,
            rich_blocks,
        } = document;
        match load.operation {
            EditorLoadOperation::Open { reload, .. } => {
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
                self.pages.editor.editor_read_session = reload.map(|reload| EditorReadSession {
                    reload,
                    total_bytes,
                });
                self.pages.editor.editor_rich_render_cache =
                    rich_blocks.map(|blocks| EditorRichRenderCache {
                        revision: state.revision(),
                        blocks,
                    });
                self.app
                    .dispatch_at(app::AppCommand::SetEditorState(Some(state)), Instant::now());
                self.pages.editor.editor_cursor_acceleration = None;
                self.pages.editor.editor_settings_dialog = None;
                self.pages.editor.editor_fingerprint = Some(fingerprint);
                self.pages.editor.editor_focus = ui::EditorFocus::Canvas;
                self.pages.editor.editor_open_menu = None;
                self.pages.editor.editor_selected_toolbar_action = None;
                self.pages.editor.editor_quick_menu_anchor = None;
                self.pages.editor.editor_drag_anchor = None;
                self.pages.editor.editor_table_column_widths.clear();
                self.pages.editor.editor_table_resize = None;
                self.pages.editor.editor_close_after_save = false;
                self.pages.editor.editor_open_after_save = false;
                self.pages.editor.editor_discard_for_open = false;
                self.pages.editor.editor_recovery_dirty_since = None;
                self.pages.editor.editor_last_recovery_write = None;
                let read_only = self
                    .app
                    .editor_state()
                    .is_some_and(EditorState::is_read_only);
                self.pages.editor.editor_message = Some(if read_only {
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
                self.pages.editor.editor_rich_render_cache =
                    rich_blocks.map(|blocks| EditorRichRenderCache {
                        revision: self.app.editor_state().map_or(0, EditorState::revision),
                        blocks,
                    });
                self.pages.editor.editor_fingerprint = Some(fingerprint);
                self.pages.editor.editor_read_session = Some(EditorReadSession {
                    reload: session.reload,
                    total_bytes,
                });
                self.pages.editor.editor_quick_menu_anchor = None;
                self.pages.editor.editor_drag_anchor = None;
                self.pages.editor.editor_message = Some(i18n::LocalizedText::from(i18n::msg!(
                    "shell-reloaded-arg1",
                    arg1 = path.display().to_string()
                )));
            }
        }
        if self.active_screen() == ShellScreen::Editor {
            self.pages.common.active_popup = None;
            self.pages.common.focused_component = ShellComponent::Editor;
        }
        self.notify_status(i18n::LocalizedText::from(i18n::msg!(
            "shell-editor-arg1",
            arg1 = path.display().to_string()
        )));
        self.finish_editor_load_navigation(&navigation_operation);
        self.refresh_hit_map();
    }

    pub(in crate::session) fn open_editor_path(&mut self, path: std::path::PathBuf) -> bool {
        if self.editor_open_reuses_document() && self.config_editor_active() {
            self.notify_toast(i18n::msg!("config-editor-finish-document"));
            return false;
        }
        if Self::is_system_config_path(&path) {
            self.begin_config_editor(path, None, None, None, "system".into());
            return self.config_editor_active();
        }
        let replacing_dirty = false;
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
                self.report_editor_open_error(i18n::LocalizedText::from(i18n::msg!(
                    "shell-could-not-open-arg1-error",
                    arg1 = path.display().to_string(),
                    error = error
                )));
                false
            }
        }
    }

    fn report_editor_open_error(&mut self, message: i18n::LocalizedText) {
        if self.editor_open_reuses_document() {
            self.report_editor_error(message);
        } else {
            self.pages.common.error_message = Some(message.clone());
            self.notify_page_alert_with_key(EDITOR_ALERT_KEY, message, ui::NotificationTone::Error);
        }
    }

    pub(in crate::session) fn open_editor_picker(&mut self, platform: &dyn Platform) {
        let owner = self.pages.editor.selected_id();
        self.open_explorer(platform);
        if self.active_screen() == ShellScreen::Explorer {
            self.pages.explorer.explorer_purpose = ExplorerPurpose::EditorOpen;
            self.pages.explorer.editor_picker_owner = owner;
            self.pages.explorer.editor_picker_save_submitted = false;
            self.notify_status(i18n::LocalizedText::from(i18n::msg!(
                "shell-choose-a-markdown-or-text-document"
            )));
        } else {
            self.pages.editor.editor_open_after_save = false;
            self.pages.editor.editor_discard_for_open = false;
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
        let owner = self.pages.editor.selected_id();
        self.open_explorer(platform);
        if self.active_screen() != ShellScreen::Explorer {
            self.pages.editor.editor_close_after_save = false;
            self.pages.editor.editor_open_after_save = false;
            self.report_editor_error(i18n::LocalizedText::from(i18n::msg!(
                "shell-could-not-open-the-save-as-picker"
            )));
            return;
        }
        self.pages.explorer.explorer_purpose = ExplorerPurpose::EditorSaveAs { snapshot };
        self.pages.explorer.editor_picker_owner = owner;
        self.pages.explorer.editor_picker_save_submitted = false;
        self.begin_explorer_input(ExplorerInputMode::NewTextFile);
        self.pages.explorer.explorer_input = suggested_name;
        self.pages.explorer.explorer_input_replace_all = true;
        self.notify_status(i18n::LocalizedText::from(i18n::msg!(
            "shell-save-as-enter-a-file-name-in-the-current-directory"
        )));
    }

    pub(in crate::session) fn submit_editor_save_as_from_explorer(
        &mut self,
        platform: &dyn Platform,
    ) -> bool {
        let ExplorerPurpose::EditorSaveAs { snapshot, .. } =
            self.pages.explorer.explorer_purpose.clone()
        else {
            return false;
        };
        if self.pages.explorer.explorer_input_mode != ExplorerInputMode::NewTextFile {
            return false;
        }
        let name = self.pages.explorer.explorer_input.trim();
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
        self.save_editor_document(path, snapshot, platform);
        true
    }

    fn return_from_editor_picker(&mut self, picker: PageInstanceId) {
        if let Some(page) = self.pages.explorer.get_mut(picker) {
            page.editor_picker_save_submitted = true;
        }
        self.request_page_close(picker);
    }

    pub(in crate::session) fn save_editor_document(
        &mut self,
        path: std::path::PathBuf,
        snapshot: app::editor::SaveSnapshot,
        platform: &dyn Platform,
    ) -> bool {
        let picker = self.pages.common.selected_id().filter(|id| {
            self.page_screen(*id) == Some(ShellScreen::Explorer)
                && self.pages.explorer.get(*id).is_some_and(|page| {
                    matches!(page.explorer_purpose, ExplorerPurpose::EditorSaveAs { .. })
                })
        });
        let owner = picker
            .and_then(|id| self.pages.explorer.get(id)?.editor_picker_owner)
            .or_else(|| self.pages.editor.selected_id());
        let Some(owner) = owner else {
            return false;
        };
        self.with_page_context(owner, |session| {
            session.save_editor_document_for_owner(path, snapshot, platform, picker)
        })
        .unwrap_or(false)
    }

    fn save_editor_document_for_owner(
        &mut self,
        path: std::path::PathBuf,
        snapshot: app::editor::SaveSnapshot,
        _platform: &dyn Platform,
        picker: Option<PageInstanceId>,
    ) -> bool {
        if self.config_editor_active() {
            self.config_editor_action(ui::EditorConfigAction::Preview);
            return false;
        }
        if Self::is_system_config_path(&path) {
            self.begin_config_save(path);
            return false;
        }
        if self.pages.editor.editor_save_state.is_some()
            || self.pages.editor.editor_load_state.is_some()
        {
            return false;
        }
        if !self.authorize_editor_file(PermissionAction::WriteFile, &path) {
            self.pages.editor.editor_close_after_save = false;
            self.pages.editor.editor_open_after_save = false;
            return false;
        }
        let is_current_path = self
            .app
            .editor_state()
            .and_then(|state| state.document.path.as_ref())
            == Some(&path);
        let expected = is_current_path
            .then_some(self.pages.editor.editor_fingerprint)
            .flatten();
        let id = next_editor_task_id();
        let revision = snapshot.revision;
        if let Err(error) = self.editor_task_runtime.submit_save_with_owner(
            id,
            path.clone(),
            snapshot,
            expected,
            self.app.auth_session().map(|s| s.user_id.clone()),
        ) {
            self.pages.editor.editor_close_after_save = false;
            self.pages.editor.editor_open_after_save = false;
            self.report_editor_error(i18n::LocalizedText::from(i18n::msg!(
                "shell-could-not-save-arg1-error",
                arg1 = path.display().to_string(),
                error = error.to_string()
            )));
            return false;
        }
        self.register_page_task(
            TaskDomain::Editor,
            id,
            TaskClass::Operation,
            self.editor_task_runtime.task_control(id, false),
        );
        self.pages.editor.editor_save_state = Some(EditorSaveState {
            id,
            path: path.clone(),
            document_generation: self.pages.editor.editor_document_generation,
            revision,
            stage: EditorTaskStage::Writing,
        });
        self.pages.editor.editor_message = Some(i18n::LocalizedText::from(i18n::msg!(
            "shell-saving-arg1",
            arg1 = path.display().to_string()
        )));
        self.notify_status(i18n::LocalizedText::from(i18n::msg!(
            "shell-saving-arg1",
            arg1 = path.display().to_string()
        )));
        self.refresh_hit_map();
        if let Some(picker) = picker {
            self.return_from_editor_picker(picker);
        }
        true
    }

    pub(in crate::session) fn continue_editor_open_after_save(&mut self, platform: &dyn Platform) {
        if self.active_screen() == ShellScreen::Explorer {
            self.pages.explorer.explorer_purpose = ExplorerPurpose::EditorOpen;
            self.pages.explorer.explorer_input_mode = ExplorerInputMode::Browse;
            self.pages.explorer.explorer_input.clear();
            self.pages.explorer.explorer_input_replace_all = false;
            self.pages.explorer.explorer_overlay_mode = None;
            self.pages.common.focused_component = ShellComponent::Explorer;
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
        let message = i18n::LocalizedText::from(i18n::msg!(
            "shell-permission-denied-reason",
            reason = reason.to_string()
        ));
        if action == PermissionAction::ReadFile {
            self.report_editor_open_error(message);
        } else {
            self.report_editor_error(message);
        }
        false
    }
}

#[cfg(test)]
#[path = "../../../../tests/unit/session/controller/editor/instance_tests.rs"]
mod instance_tests;
