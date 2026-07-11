impl ShellState {
    fn logout_at(&mut self, now: Instant) {
        let audit_error = self
            .auth_session
            .as_ref()
            .zip(self.storage_manager.clone())
            .and_then(|(session, storage)| {
                SessionService::new(storage)
                    .logout_session(session)
                    .err()
                    .map(|error| error.to_string())
            });

        self.return_to_login_at("Signed out", now);
        if let Some(error) = audit_error {
            self.notify_alert_with_key(
                "auth.logout-audit",
                format!("Signed out, but logout audit failed: {error}"),
                tundra_ui::NotificationTone::Warning,
            );
        }
    }

    fn return_to_login(&mut self, status: &str) {
        self.return_to_login_at(status, Instant::now());
    }

    fn return_to_login_at(&mut self, status: &str, now: Instant) {
        self.resolve_user_management_refresh_alert();
        self.notifications = NotificationCenter::new(status);
        self.modal_focus_context = None;
        self.modal_focus_prepared_for_follow_up = false;
        self.notification_pointer_capture = None;
        self.pending_notification_commands.clear();
        self.auth_session = None;
        self.guest_mode = false;
        self.time_sync_dialog_visible = false;
        self.time_sync_failure_message = None;
        self.clock_scheduler = None;
        self.clock_selected_entry_id = None;
        self.clock_entry_window_start = 0;
        self.clock_create_state = None;
        self.clock_persist_pending = false;
        self.clock_pending_due_summary = None;
        self.clock_profile_pending_sync = None;
        self.user_management_users.clear();
        self.user_management_selected = 0;
        self.user_management_window_start = 0;
        self.user_management_focus = UserManagementPageFocus::UserList;
        self.user_management_feedback_tone = UserManagementFeedbackTone::Info;
        self.user_management_mode = UserManagementMode::Browse;
        self.user_management_message = None;
        self.selected_home_entry_index = 0;
        self.explorer_state = None;
        self.explorer_input_mode = ExplorerInputMode::Browse;
        self.explorer_input.clear();
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

    fn user_home_entries(&self) -> Vec<tundra_ui::ShellEntry> {
        if self.is_strict_guest() {
            return Vec::new();
        }
        let mut entries = user_home_entries();
        if self.can_manage_all_users() {
            entries.push(tundra_ui::ShellEntry::new(
                "User Management",
                "Manage local TundraUX users",
            ));
        } else if self
            .auth_session
            .as_ref()
            .is_some_and(|session| session.role == UserRole::User)
        {
            entries.push(tundra_ui::ShellEntry::new(
                "User Profile",
                "Manage your local TundraUX account",
            ));
        }
        entries
    }

    fn is_strict_guest(&self) -> bool {
        self.guest_mode
            || self
                .auth_session
                .as_ref()
                .is_some_and(|session| session.role == UserRole::Guest)
    }

    fn sync_home_entry_selection(&mut self) {
        let count = self.user_home_entries().len();
        self.selected_home_entry_index = if count == 0 {
            0
        } else {
            self.selected_home_entry_index.min(count - 1)
        };
    }

    fn select_home_entry(&mut self, index: usize) {
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

    fn select_home_entry_delta(&mut self, delta: isize) {
        let count = self.user_home_entries().len();
        if count == 0 {
            self.selected_home_entry_index = 0;
            return;
        }

        let current = self.selected_home_entry_index().min(count - 1) as isize;
        let next = (current + delta).clamp(0, count.saturating_sub(1) as isize);
        self.select_home_entry(next as usize);
    }

    fn select_home_entry_row_delta(&mut self, direction: isize) {
        let columns = self.visible_home_entry_columns().max(1) as isize;
        self.select_home_entry_delta(direction.saturating_mul(columns));
    }

    fn activate_selected_home_entry(&mut self, platform: &dyn Platform) {
        self.activate_home_entry(self.selected_home_entry_index(), platform);
    }

    fn activate_home_entry(&mut self, index: usize, platform: &dyn Platform) {
        let entries = self.user_home_entries();
        let Some(entry) = entries.get(index) else {
            return;
        };

        self.selected_home_entry_index = index;
        match entry.label.as_str() {
            "Explorer" => self.open_explorer(platform),
            "User Management" | "User Profile" => self.open_user_management(),
            label => {
                self.error_message = None;
                self.notify_status(format!("{label} is not implemented yet"));
            }
        }
    }

    fn visible_home_entry_columns(&self) -> usize {
        let area = Rect::new(0, 0, self.terminal_size.0, self.terminal_size.1);
        let tundra_ui::ShellLayout::Full { main, .. } = tundra_ui::compute_shell_layout(area)
        else {
            return 1;
        };
        let areas = tundra_ui::home_entry_tile_areas(main, self.user_home_entries().len());
        let Some(first) = areas.first() else {
            return 1;
        };

        areas.iter().take_while(|area| area.y == first.y).count()
    }

    fn home_entry_index_at(&self, coordinates: CellPosition) -> Option<usize> {
        let area = Rect::new(0, 0, self.terminal_size.0, self.terminal_size.1);
        let tundra_ui::ShellLayout::Full { main, .. } = tundra_ui::compute_shell_layout(area)
        else {
            return None;
        };

        tundra_ui::home_entry_index_at(main, self.user_home_entries().len(), coordinates)
    }

    fn notification_action_index_at(&self, coordinates: CellPosition) -> Option<usize> {
        let model = self.notifications.active_modal_view_model()?;
        let area = Rect::new(0, 0, self.terminal_size.0, self.terminal_size.1);
        let tundra_ui::NotificationLayout::Dialog(layout) =
            tundra_ui::notification_layout(area, &model)
        else {
            return None;
        };

        layout
            .actions
            .iter()
            .find(|action| rect_contains(action.area, coordinates))
            .map(|action| action.index)
    }

    fn notification_can_render(&self) -> bool {
        let Some(model) = self.notifications.active_modal_view_model() else {
            return false;
        };
        let area = Rect::new(0, 0, self.terminal_size.0, self.terminal_size.1);
        matches!(
            tundra_ui::notification_layout(area, &model),
            tundra_ui::NotificationLayout::Dialog(_)
        )
    }
}
