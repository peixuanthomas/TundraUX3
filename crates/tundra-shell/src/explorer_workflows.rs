impl ShellState {
    fn open_explorer(&mut self, platform: &dyn Platform) {
        if self.is_strict_guest() {
            self.error_message = None;
            self.notify_status("Guest access is read-only");
            return;
        }
        if self.auth_session.is_none() {
            self.error_message = Some("Login required".to_string());
            return;
        }
        let Some(storage) = self.storage_manager.clone() else {
            self.error_message = Some("Storage unavailable".to_string());
            return;
        };

        let show_hidden = storage
            .load_config()
            .map(|config| config.explorer.show_hidden)
            .unwrap_or(false);
        let start_path = platform
            .user_dirs()
            .map(|dirs| dirs.documents().to_path_buf())
            .unwrap_or_else(|_| storage.layout().data_path.clone());
        let start_path = if start_path.exists() {
            start_path
        } else {
            storage.layout().data_path.clone()
        };

        self.explorer_state = Some(ExplorerState::new(start_path, show_hidden));
        self.explorer_input_mode = ExplorerInputMode::Browse;
        self.explorer_input.clear();
        self.screen_stack.push(ShellScreen::Explorer);
        self.focused_component = ShellComponent::Explorer;
        self.notify_status("Explorer");
        self.apply_explorer_command(ExplorerCommand::Refresh, platform);
        self.refresh_hit_map();
    }

    fn close_explorer(&mut self) {
        self.explorer_input_mode = ExplorerInputMode::Browse;
        self.explorer_input.clear();
        self.resolve_explorer_alert();
        self.pop_to_home();
        self.notify_status("Ready");
    }

    fn resolve_explorer_alert(&mut self) {
        let resolved_message = self
            .notifications
            .alert_message_for_key(EXPLORER_ALERT_KEY)
            .map(str::to_string);
        if self.error_message.as_ref() == resolved_message.as_ref() {
            self.error_message = None;
        }
        self.resolve_notification_alert(EXPLORER_ALERT_KEY);
    }

    fn apply_explorer_command(&mut self, command: ExplorerCommand, platform: &dyn Platform) {
        let command_kind = command.clone();
        let Some(storage) = self.storage_manager.clone() else {
            let message = "Storage unavailable".to_string();
            self.error_message = Some(message.clone());
            self.notify_alert_with_key(
                EXPLORER_ALERT_KEY,
                message,
                tundra_ui::NotificationTone::Error,
            );
            return;
        };
        let session = self.auth_session.clone();
        let Some(state) = self.explorer_state.as_mut() else {
            let message = "Explorer unavailable".to_string();
            self.error_message = Some(message.clone());
            self.notify_alert_with_key(
                EXPLORER_ALERT_KEY,
                message,
                tundra_ui::NotificationTone::Error,
            );
            return;
        };

        ExplorerController::default().apply(state, command, session.as_ref(), platform, &storage);
        let pending_dialog = state.pending_dialog.clone();
        let explorer_error = state.error.clone();
        let explorer_message = state.message.clone();
        if let Some(error) = explorer_error {
            self.error_message = Some(error.clone());
            self.notify_alert_with_key(
                EXPLORER_ALERT_KEY,
                error,
                tundra_ui::NotificationTone::Error,
            );
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
            ExplorerCommand::DeleteToTrash | ExplorerCommand::ConfirmDelete
        ) && let Some(dialog) = pending_dialog
        {
            self.notify_modal_with_options(
                ShellNotification::modal(
                    dialog.title,
                    dialog.message,
                    tundra_ui::NotificationTone::Warning,
                    vec![
                        ShellNotificationAction::new("confirm", "Move")
                            .with_shortcut(InputKey::Character('y'))
                            .with_follow_up(ShellCommand::ExplorerConfirmDelete),
                        ShellNotificationAction::new("cancel", "Cancel")
                            .with_shortcut(InputKey::Character('n'))
                            .cancel()
                            .with_follow_up(ShellCommand::CancelExplorerInput),
                    ],
                )
                .with_key(EXPLORER_DELETE_NOTIFICATION_KEY),
            );
        }
    }

    fn begin_explorer_input(&mut self, mode: ExplorerInputMode) {
        self.explorer_input_mode = mode;
        self.explorer_input = if mode == ExplorerInputMode::Rename {
            self.explorer_state
                .as_ref()
                .and_then(|state| state.selected_entry())
                .map(|entry| entry.name.clone())
                .unwrap_or_default()
        } else {
            String::new()
        };
        self.notify_status(explorer_input_prompt(mode));
    }

    fn append_explorer_char(&mut self, character: char) {
        self.explorer_input.push(character);
    }

    fn explorer_backspace(&mut self) {
        self.explorer_input.pop();
    }

    fn submit_explorer_input(&mut self, platform: &dyn Platform) {
        let value = self.explorer_input.trim().to_string();
        let command = match self.explorer_input_mode {
            ExplorerInputMode::Browse => return,
            ExplorerInputMode::Search => ExplorerCommand::Search(value),
            ExplorerInputMode::NewFolder => ExplorerCommand::NewFolder(value),
            ExplorerInputMode::NewTextFile => ExplorerCommand::NewTextFile(value),
            ExplorerInputMode::Rename => ExplorerCommand::Rename(value),
        };

        self.explorer_input_mode = ExplorerInputMode::Browse;
        self.explorer_input.clear();
        self.apply_explorer_command(command, platform);
    }

    fn cancel_explorer_input(&mut self) {
        if let Some(state) = self.explorer_state.as_mut()
            && state.pending_dialog.is_some()
        {
            state.pending_dialog = None;
            state.message = Some("Cancelled".to_string());
            self.notifications
                .dismiss_modal_by_key(EXPLORER_DELETE_NOTIFICATION_KEY);
            self.notify_status("Cancelled");
            return;
        }
        self.explorer_input_mode = ExplorerInputMode::Browse;
        self.explorer_input.clear();
        self.notify_status("Explorer");
    }

    fn select_explorer_at(
        &mut self,
        coordinates: CellPosition,
        click: ClickKind,
        platform: &dyn Platform,
    ) {
        let Some(index) = self.explorer_index_at(coordinates) else {
            return;
        };
        self.apply_explorer_command(ExplorerCommand::SelectIndex(index), platform);
        if click == ClickKind::Double {
            self.apply_explorer_command(ExplorerCommand::OpenSelected, platform);
        }
    }

    fn explorer_index_at(&self, coordinates: CellPosition) -> Option<usize> {
        let area = Rect::new(0, 0, self.terminal_size.0, self.terminal_size.1);
        let tundra_ui::ShellLayout::Full { main, .. } = tundra_ui::compute_shell_layout(area)
        else {
            return None;
        };
        if !rect_contains(main, coordinates) {
            return None;
        }
        let content_line = coordinates.1.checked_sub(main.y.saturating_add(1))? as usize;
        let explorer = self.to_explorer_view_model();
        let content_width = main.width.saturating_sub(2);
        let first_entry_line =
            tundra_ui::explorer_first_entry_content_line(&explorer, content_width);
        let index = content_line.checked_sub(first_entry_line)?;
        self.explorer_state
            .as_ref()
            .filter(|state| index < state.entries.len())
            .map(|_| index)
    }
}
