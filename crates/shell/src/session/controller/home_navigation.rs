use super::super::*;
impl ShellSession {
    pub(in crate::session) fn logout_at(&mut self, now: Instant) -> bool {
        if self.diagnostics_restart_is_required() {
            self.notify_alert_with_tone(
                "Restart TundraUX before signing out",
                ui::NotificationTone::Warning,
            );
            return false;
        }
        if self.diagnostics_scanning
            || self
                .diagnostics_task_runtime
                .as_ref()
                .is_some_and(ShellDiagnosticsTaskRuntime::is_busy)
        {
            self.notify_alert_with_tone(
                "Wait for the diagnostics task to finish before signing out",
                ui::NotificationTone::Warning,
            );
            return false;
        }
        if !self.persist_editor_recovery_now(now) {
            self.notify_alert_with_tone(
                "Could not save the Editor recovery; sign out was cancelled",
                ui::NotificationTone::Error,
            );
            return false;
        }
        self.return_to_login_at("Signed out", now);
        true
    }

    pub(in crate::session) fn logout_to_lockscreen_at(&mut self, now: Instant) -> bool {
        if !self.logout_at(now) {
            return false;
        }
        self.prepare_return_to_lockscreen();
        true
    }

    pub(in crate::session) fn return_to_login(&mut self, status: &str) {
        self.return_to_login_at(status, Instant::now());
    }

    pub(in crate::session) fn return_to_login_at(&mut self, status: &str, now: Instant) {
        // Account disable/delete may force a return to login without passing
        // through the ordinary Logout command. Preserve any dirty editor text
        // before the authenticated recovery context is cleared.
        let _ = self.persist_editor_recovery_now(now);
        self.resolve_user_management_refresh_alert();
        self.notification_bindings = NotificationBindings::default();
        self.app.dispatch_at(
            app::AppCommand::Notification(app::NotificationCommand::Reset(status.to_string())),
            now,
        );
        self.modal_focus_context = None;
        self.modal_focus_prepared_for_follow_up = false;
        self.notification_pointer_capture = None;
        self.pending_notification_commands.clear();
        self.app
            .dispatch_at(app::AppCommand::SetAuthSession(None), now);
        self.app
            .dispatch_at(app::AppCommand::SetActiveAppearance(None), now);
        self.time_sync_dialog_visible = false;
        self.time_sync_failure_message = None;
        self.clock_scheduler = None;
        self.clock_selected_entry_id = None;
        self.clock_entry_window_start = 0;
        self.clock_create_state = None;
        self.clock_persist_pending = false;
        self.clock_pending_due_summary = None;
        self.clock_profile_pending_sync = None;
        self.app
            .dispatch_at(app::AppCommand::SetManagedUsers(Vec::new()), now);
        self.user_management_selected = 0;
        self.user_management_window_start = 0;
        self.user_management_focus = UserManagementPageFocus::UserList;
        self.user_management_feedback_tone = UserManagementFeedbackTone::Info;
        self.user_management_mode = UserManagementMode::Browse;
        self.user_management_message = None;
        self.selected_home_entry_index = 0;
        self.settings_state = None;
        self.launcher_drag = None;
        self.replace_explorer_state(None);
        self.explorer_input_mode = ExplorerInputMode::Browse;
        self.explorer_input.clear();
        self.explorer_input_replace_all = false;
        self.explorer_overlay_mode = None;
        self.explorer_purpose = ExplorerPurpose::Browse;
        if let Some(load) = self.editor_load_state.take() {
            self.editor_task_runtime.cancel(load.id);
        }
        self.editor_save_state = None;
        self.editor_read_session = None;
        self.advance_editor_document_generation();
        self.app
            .dispatch_at(app::AppCommand::SetEditorState(None), Instant::now());
        self.editor_rich_render_cache = None;
        self.editor_cursor_acceleration = None;
        self.editor_settings_dialog = None;
        self.editor_focus = ui::EditorFocus::Canvas;
        self.editor_open_menu = None;
        self.editor_selected_toolbar_action = None;
        self.editor_quick_menu_anchor = None;
        self.editor_drag_anchor = None;
        self.editor_table_column_widths.clear();
        self.editor_table_resize = None;
        self.editor_fingerprint = None;
        self.editor_close_after_save = false;
        self.editor_open_after_save = false;
        self.editor_discard_for_open = false;
        self.editor_message = None;
        self.editor_recovery_dirty_since = None;
        self.editor_last_recovery_write = None;
        self.app.dispatch_at(
            app::AppCommand::SetDiagnosticsSnapshot(None),
            Instant::now(),
        );
        self.diagnostics_tab = ui::DiagnosticsTab::Health;
        self.diagnostics_selected_check = 0;
        self.diagnostics_selected_log = 0;
        self.diagnostics_selected_incident = 0;
        self.diagnostics_list_window_start = 0;
        self.diagnostics_scanning = false;
        self.diagnostics_rescan_pending = false;
        self.diagnostics_repair_preview.clear();
        self.diagnostics_repair_selected = 0;
        self.diagnostics_repair_scroll_offset = 0;
        self.diagnostics_repair_confirm_selected = true;
        self.diagnostics_feedback = None;
        self.diagnostics_restart_required = self.diagnostics_restart_is_required();
        self.active_popup = None;
        self.hovered_component = None;
        self.last_click = None;
        self.drag_tracker = None;
        self.login_password.clear();
        self.login_password_visible_until = None;
        self.error_message = None;
        self.return_to_lockscreen_requested = false;
        self.reset_login_idle_deadline_at(now);
        let _ = self.refresh_login_users_from_storage();
        self.screen_stack = vec![ShellScreen::Login];
        self.focused_component = ShellComponent::LoginUserList;
        self.refresh_hit_map();
    }

    pub(in crate::session) fn user_home_entries(&self) -> Vec<ui::ShellEntry> {
        if self.is_strict_guest() {
            return Vec::new();
        }
        let mut entries = user_home_entries();
        if self.can_manage_all_users() {
            entries.push(ui::ShellEntry::new(
                "User Management",
                "Manage local TundraUX users",
            ));
        } else if self
            .app
            .auth_session()
            .is_some_and(|session| session.role == UserRole::User)
        {
            entries.push(ui::ShellEntry::new(
                "User Profile",
                "Manage your local TundraUX account",
            ));
        }
        entries
    }

    pub(in crate::session) fn is_strict_guest(&self) -> bool {
        self.app
            .auth_session()
            .is_some_and(|session| session.role == UserRole::Guest)
    }

    pub(in crate::session) fn sync_home_entry_selection(&mut self) {
        let count = self.user_home_entries().len();
        self.selected_home_entry_index = if count == 0 {
            0
        } else {
            self.selected_home_entry_index.min(count - 1)
        };
    }

    pub(in crate::session) fn select_home_entry(&mut self, index: usize) {
        let entries = self.user_home_entries();
        if entries.is_empty() {
            self.selected_home_entry_index = 0;
            return;
        }

        self.selected_home_entry_index = index.min(entries.len() - 1);
        self.notify_status(format!(
            "Home: {}",
            entries[self.selected_home_entry_index].label
        ));
    }

    pub(in crate::session) fn select_home_entry_delta(&mut self, delta: isize) {
        let count = self.user_home_entries().len();
        if count == 0 {
            self.selected_home_entry_index = 0;
            return;
        }

        let current = self.selected_home_entry_index().min(count - 1) as isize;
        let next = (current + delta).clamp(0, count.saturating_sub(1) as isize);
        self.select_home_entry(next as usize);
    }

    pub(in crate::session) fn select_home_entry_row_delta(&mut self, direction: isize) {
        let columns = self.visible_home_entry_columns().max(1) as isize;
        self.select_home_entry_delta(direction.saturating_mul(columns));
    }

    pub(in crate::session) fn activate_selected_home_entry(&mut self, platform: &dyn Platform) {
        self.activate_home_entry(self.selected_home_entry_index(), platform);
    }

    pub(in crate::session) fn activate_home_entry(
        &mut self,
        index: usize,
        platform: &dyn Platform,
    ) {
        let entries = self.user_home_entries();
        let Some(entry) = entries.get(index) else {
            return;
        };

        self.selected_home_entry_index = index;
        match entry.label.as_str() {
            "Explorer" => self.open_explorer(platform),
            "Launcher" => self.open_launcher(platform),
            "Editor" => self.open_editor(),
            "Settings" => self.open_settings(),
            "Diagnostics" => self.open_diagnostics(),
            "User Management" | "User Profile" => self.open_user_management(),
            label => {
                self.error_message = None;
                self.notify_status(format!("{label} is not implemented yet"));
            }
        }
    }

    pub(in crate::session) fn visible_home_entry_columns(&self) -> usize {
        let area = Rect::new(0, 0, self.terminal_size.0, self.terminal_size.1);
        let ui::ShellLayout::Full { main, .. } = ui::compute_shell_layout(area) else {
            return 1;
        };
        let areas = ui::home_entry_tile_areas(main, self.user_home_entries().len());
        let Some(first) = areas.first() else {
            return 1;
        };

        areas.iter().take_while(|area| area.y == first.y).count()
    }

    pub(in crate::session) fn home_entry_index_at(
        &self,
        coordinates: CellPosition,
    ) -> Option<usize> {
        let area = Rect::new(0, 0, self.terminal_size.0, self.terminal_size.1);
        let ui::ShellLayout::Full { main, .. } = ui::compute_shell_layout(area) else {
            return None;
        };

        ui::home_entry_index_at(main, self.user_home_entries().len(), coordinates)
    }

    pub(in crate::session) fn notification_action_index_at(
        &self,
        coordinates: CellPosition,
    ) -> Option<usize> {
        let model = self.notification_active_modal_view_model()?;
        let area = Rect::new(0, 0, self.terminal_size.0, self.terminal_size.1);
        let ui::NotificationLayout::Dialog(layout) = ui::notification_layout(area, &model) else {
            return None;
        };

        layout
            .actions
            .iter()
            .find(|action| rect_contains(action.area, coordinates))
            .map(|action| action.index)
    }

    pub(in crate::session) fn notification_can_render(&self) -> bool {
        let Some(model) = self.notification_active_modal_view_model() else {
            return false;
        };
        let area = Rect::new(0, 0, self.terminal_size.0, self.terminal_size.1);
        matches!(
            ui::notification_layout(area, &model),
            ui::NotificationLayout::Dialog(_)
        )
    }
}
