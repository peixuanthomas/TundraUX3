impl ShellState {
    pub fn apply_input(&mut self, input: InputEvent) -> ShellAction {
        let platform = tundra_platform::native_platform();
        self.apply_input_with_platform(input, platform.as_ref())
    }

    pub fn apply_input_with_platform(
        &mut self,
        input: InputEvent,
        platform: &dyn Platform,
    ) -> ShellAction {
        self.apply_input_with_platform_at(input, platform, Instant::now())
    }

    #[doc(hidden)]
    pub fn apply_input_at_for_test(
        &mut self,
        input: InputEvent,
        received_at: Instant,
    ) -> ShellAction {
        let platform = tundra_platform::native_platform();
        self.apply_input_with_platform_at(input, platform.as_ref(), received_at)
    }

    fn apply_input_with_platform_at(
        &mut self,
        input: InputEvent,
        platform: &dyn Platform,
        received_at: Instant,
    ) -> ShellAction {
        self.notifications.expire(received_at);
        self.expire_login_password_visibility_at(received_at);
        let requests_shutdown = match &input {
            InputEvent::Shutdown => true,
            InputEvent::Key(key) => {
                key.is_ctrl_c() && self.active_screen() != ShellScreen::Editor
            }
            _ => false,
        };
        if self.login_idle_tracking_active()
            && received_at >= self.login_idle_deadline
            && !requests_shutdown
        {
            self.prepare_return_to_lockscreen();
            return ShellAction::Exit;
        }
        if self.login_idle_tracking_active() && resets_login_idle_timeout(&input) {
            self.login_idle_deadline = received_at + LOGIN_IDLE_TIMEOUT;
        }
        let routed = self.route_input_at(input, received_at);
        self.apply_routed_event(routed, platform, received_at)
    }

    pub fn route_input_at(&mut self, input: InputEvent, received_at: Instant) -> RoutedEvent {
        let (target, command) = match &input {
            InputEvent::Shutdown => (RoutedTarget::Global, ShellCommand::Shutdown),
            InputEvent::Tick => (RoutedTarget::Global, ShellCommand::Tick),
            InputEvent::Resize { width, height } => (
                RoutedTarget::Global,
                ShellCommand::RefreshHitMap {
                    width: *width,
                    height: *height,
                },
            ),
            InputEvent::Key(key) => {
                let (target, command) = self.route_key_input(key);
                (target, command)
            }
            InputEvent::Mouse(mouse) => {
                let (target, command) = self.route_mouse_input(*mouse, received_at);
                (target, command)
            }
            InputEvent::Paste(value) if self.active_screen() == ShellScreen::Editor => (
                RoutedTarget::Component(ShellComponent::Editor),
                ShellCommand::EditorPaste(value.clone()),
            ),
            InputEvent::FocusGained | InputEvent::FocusLost | InputEvent::Paste(_) => {
                (RoutedTarget::Global, ShellCommand::RecordInput)
            }
        };

        RoutedEvent {
            input,
            target,
            command,
        }
    }

    fn apply_routed_event(
        &mut self,
        routed: RoutedEvent,
        platform: &dyn Platform,
        received_at: Instant,
    ) -> ShellAction {
        self.pending_notification_commands.clear();
        let follow_up_input = routed.input.clone();
        let follow_up_target = routed.target;
        let mut action = self.apply_routed_event_once(routed, platform, received_at);
        let mut steps = 0_usize;

        while action != ShellAction::Exit {
            let Some(command) = self.pending_notification_commands.pop_front() else {
                break;
            };
            if steps >= MAX_NOTIFICATION_FOLLOW_UP_STEPS {
                self.pending_notification_commands.clear();
                self.notify_alert_with_key(
                    NOTIFICATION_FOLLOW_UP_ALERT_KEY,
                    "Notification follow-up limit reached",
                    tundra_ui::NotificationTone::Critical,
                );
                break;
            }
            steps = steps.saturating_add(1);
            action = self.apply_routed_event_once(
                RoutedEvent {
                    input: follow_up_input.clone(),
                    target: follow_up_target,
                    command,
                },
                platform,
                received_at,
            );
        }

        if action == ShellAction::Exit {
            self.pending_notification_commands.clear();
        }
        self.finish_modal_focus_transition();
        action
    }

    fn apply_routed_event_once(
        &mut self,
        routed: RoutedEvent,
        platform: &dyn Platform,
        received_at: Instant,
    ) -> ShellAction {
        self.record_input_diagnostics(&routed);
        if !matches!(routed.input, InputEvent::Mouse(_)) {
            self.notification_pointer_capture = None;
        }
        self.last_routed_target = Some(routed.target);
        self.last_command = Some(routed.command.clone());

        match routed.command {
            ShellCommand::Shutdown => {
                let _ = self.persist_editor_recovery_now(received_at);
                self.shutdown_requested = true;
                ShellAction::Exit
            }
            ShellCommand::Tick => {
                self.tick_count = self.tick_count.saturating_add(1);
                self.notifications.tick();
                self.advance_clock_background();
                self.poll_explorer_background_tasks(platform);
                self.drain_diagnostics_events();
                self.persist_editor_recovery_if_due(received_at);
                ShellAction::Redraw
            }
            ShellCommand::RefreshHitMap { width, height } => {
                self.terminal_size = (width, height);
                self.notification_pointer_capture = None;
                self.last_resize_event = Some(format!("{width}x{height}"));
                if self.active_screen() == ShellScreen::FirstRunSetup {
                    self.sync_setup_timezone_window();
                }
                if self.active_screen() == ShellScreen::Login {
                    self.sync_login_user_window();
                }
                if self.active_screen() == ShellScreen::UserManagement {
                    self.ensure_user_management_selection_visible();
                }
                self.refresh_hit_map();
                ShellAction::Redraw
            }
            ShellCommand::RequestExit => {
                self.capture_modal_focus_context();
                if self.active_screen() != ShellScreen::ExitConfirm {
                    self.screen_stack.push(ShellScreen::ExitConfirm);
                }
                self.active_popup = None;
                self.notify_status("Confirm exit");
                self.notify_modal_with_options(
                    ShellNotification::modal(
                        "Exit TundraUX 3",
                        "Leave the shell and restore the terminal?",
                        tundra_ui::NotificationTone::Warning,
                        vec![
                            ShellNotificationAction::new("confirm", "Exit")
                                .with_shortcut(InputKey::Character('y'))
                                .with_follow_up(ShellCommand::ConfirmExit),
                            ShellNotificationAction::new("cancel", "Cancel")
                                .with_shortcut(InputKey::Character('n'))
                                .cancel()
                                .with_follow_up(ShellCommand::CancelExit),
                        ],
                    )
                    .with_key(EXIT_CONFIRM_NOTIFICATION_KEY)
                    .with_component(ShellComponent::ExitDialog),
                );
                self.refresh_hit_map();
                ShellAction::Redraw
            }
            ShellCommand::ConfirmExit => {
                if !self.persist_editor_recovery_now(received_at) {
                    self.shutdown_requested = false;
                    return ShellAction::Redraw;
                }
                self.shutdown_requested = true;
                ShellAction::Exit
            }
            ShellCommand::CancelExit => {
                self.notifications
                    .dismiss_modal_by_key(EXIT_CONFIRM_NOTIFICATION_KEY);
                self.cancel_exit_confirmation();
                self.active_popup = None;
                self.notify_status("Ready");
                self.refresh_hit_map();
                ShellAction::Redraw
            }
            ShellCommand::OpenLatestCrashReport => {
                if !self.diagnostics_can_view_details() {
                    self.record_diagnostics_audit(
                        PermissionAction::ViewDiagnosticsDetails,
                        "watchdog.report.open",
                        AuditOutcome::Denied,
                        Some("insufficient_role"),
                    );
                    self.notify_alert_with_tone(
                        "Only administrators can open watchdog reports",
                        tundra_ui::NotificationTone::Warning,
                    );
                } else {
                    match self.latest_watchdog_report.clone() {
                        Some(path) => match platform.open_path(&path) {
                            Ok(()) => {
                                self.record_diagnostics_audit(
                                    PermissionAction::ViewDiagnosticsDetails,
                                    "watchdog.report.open",
                                    AuditOutcome::Success,
                                    None,
                                );
                                self.notify_toast("Opened watchdog crash report");
                            }
                            Err(error) => {
                                self.record_diagnostics_audit(
                                    PermissionAction::ViewDiagnosticsDetails,
                                    "watchdog.report.open",
                                    AuditOutcome::Failure,
                                    Some(&error.to_string()),
                                );
                                self.notify_alert_with_tone(
                                    format!("Could not open crash report: {error}"),
                                    tundra_ui::NotificationTone::Critical,
                                );
                            }
                        },
                        None => self.notify_alert_with_tone(
                            "No watchdog crash report path is available",
                            tundra_ui::NotificationTone::Critical,
                        ),
                    }
                }
                ShellAction::Redraw
            }
            ShellCommand::CopyLatestCrashSummary => {
                if !self.diagnostics_can_view_details() {
                    self.record_diagnostics_audit(
                        PermissionAction::ViewDiagnosticsDetails,
                        "watchdog.summary.copy",
                        AuditOutcome::Denied,
                        Some("insufficient_role"),
                    );
                    self.notify_alert_with_tone(
                        "Only administrators can copy full watchdog summaries",
                        tundra_ui::NotificationTone::Warning,
                    );
                } else {
                    match self.latest_watchdog_summary.clone() {
                        Some(summary) => match platform.write_clipboard_text(&summary) {
                            Ok(()) => {
                                self.record_diagnostics_audit(
                                    PermissionAction::ViewDiagnosticsDetails,
                                    "watchdog.summary.copy",
                                    AuditOutcome::Success,
                                    None,
                                );
                                self.notify_toast("Copied watchdog incident summary");
                            }
                            Err(error) => {
                                self.record_diagnostics_audit(
                                    PermissionAction::ViewDiagnosticsDetails,
                                    "watchdog.summary.copy",
                                    AuditOutcome::Failure,
                                    Some(&error.to_string()),
                                );
                                self.notify_alert_with_tone(
                                    format!("Could not copy crash summary: {error}"),
                                    tundra_ui::NotificationTone::Critical,
                                );
                            }
                        },
                        None => self.notify_alert_with_tone(
                            "No watchdog incident summary is available",
                            tundra_ui::NotificationTone::Critical,
                        ),
                    }
                }
                ShellAction::Redraw
            }
            ShellCommand::FocusNext => {
                self.move_focus(1);
                self.notify_status(format!("Focus: {}", self.focused_component.label()));
                ShellAction::Redraw
            }
            ShellCommand::FocusPrevious => {
                self.move_focus(-1);
                self.notify_status(format!("Focus: {}", self.focused_component.label()));
                ShellAction::Redraw
            }
            ShellCommand::AppendAuthChar(character) => {
                self.append_auth_char(character);
                ShellAction::Redraw
            }
            ShellCommand::AuthBackspace => {
                self.auth_backspace();
                ShellAction::Redraw
            }
            ShellCommand::LoginPreviousUser => {
                self.select_login_user_delta(-1);
                ShellAction::Redraw
            }
            ShellCommand::LoginNextUser => {
                self.select_login_user_delta(1);
                ShellAction::Redraw
            }
            ShellCommand::LoginPageUserUp => {
                self.select_login_user_delta(-(self.login_user_visible_row_count() as isize));
                ShellAction::Redraw
            }
            ShellCommand::LoginPageUserDown => {
                self.select_login_user_delta(self.login_user_visible_row_count() as isize);
                ShellAction::Redraw
            }
            ShellCommand::LoginFirstUser => {
                self.select_first_login_user();
                ShellAction::Redraw
            }
            ShellCommand::LoginLastUser => {
                self.select_last_login_user();
                ShellAction::Redraw
            }
            ShellCommand::LoginFocusUserList => {
                self.focused_component = ShellComponent::LoginUserList;
                self.error_message = None;
                ShellAction::Redraw
            }
            ShellCommand::LoginFocusPassword => {
                self.focused_component = ShellComponent::LoginPassword;
                self.error_message = None;
                ShellAction::Redraw
            }
            ShellCommand::LoginFocusPasswordVisibility => {
                self.focused_component = ShellComponent::LoginPasswordVisibility;
                self.error_message = None;
                ShellAction::Redraw
            }
            ShellCommand::ToggleLoginPasswordVisibility => {
                self.focused_component = ShellComponent::LoginPasswordVisibility;
                self.toggle_login_password_visibility_at(received_at);
                ShellAction::Redraw
            }
            ShellCommand::SubmitLogin => {
                self.login_password_visible_until = None;
                self.submit_login();
                ShellAction::Redraw
            }
            ShellCommand::SubmitBootstrapAdmin => {
                self.submit_bootstrap_admin();
                ShellAction::Redraw
            }
            ShellCommand::SetupPreviousLanguage => {
                self.setup_previous_language();
                ShellAction::Redraw
            }
            ShellCommand::SetupNextLanguage => {
                self.setup_next_language();
                ShellAction::Redraw
            }
            ShellCommand::SetupContinue => {
                self.setup_continue();
                ShellAction::Redraw
            }
            ShellCommand::SetupPreviousTimezone => {
                self.setup_select_timezone_delta(-1);
                ShellAction::Redraw
            }
            ShellCommand::SetupNextTimezone => {
                self.setup_select_timezone_delta(1);
                ShellAction::Redraw
            }
            ShellCommand::SetupPageTimezoneUp => {
                self.setup_select_timezone_delta(
                    -(self.setup_timezone_visible_row_count() as isize),
                );
                ShellAction::Redraw
            }
            ShellCommand::SetupPageTimezoneDown => {
                self.setup_select_timezone_delta(self.setup_timezone_visible_row_count() as isize);
                ShellAction::Redraw
            }
            ShellCommand::SetupFirstTimezone => {
                self.setup_select_first_timezone();
                ShellAction::Redraw
            }
            ShellCommand::SetupLastTimezone => {
                self.setup_select_last_timezone();
                ShellAction::Redraw
            }
            ShellCommand::SetupFocusNext => {
                self.move_setup_admin_focus(1);
                ShellAction::Redraw
            }
            ShellCommand::SetupFocusPrevious => {
                self.move_setup_admin_focus(-1);
                ShellAction::Redraw
            }
            ShellCommand::AppendSetupAdminChar(character) => {
                self.append_setup_admin_char(character);
                ShellAction::Redraw
            }
            ShellCommand::SetupAdminBackspace => {
                self.setup_admin_backspace();
                ShellAction::Redraw
            }
            ShellCommand::SubmitSetup => {
                self.submit_first_run_setup();
                ShellAction::Redraw
            }
            ShellCommand::ActivateSetup {
                target,
                coordinates,
            } => {
                self.activate_setup(target, coordinates);
                ShellAction::Redraw
            }
            ShellCommand::ActivateLogin {
                target,
                coordinates,
            } => {
                self.activate_login(target, coordinates);
                ShellAction::Redraw
            }
            ShellCommand::HomeEntryLeft => {
                self.select_home_entry_delta(-1);
                ShellAction::Redraw
            }
            ShellCommand::HomeEntryRight => {
                self.select_home_entry_delta(1);
                ShellAction::Redraw
            }
            ShellCommand::HomeEntryUp => {
                self.select_home_entry_row_delta(-1);
                ShellAction::Redraw
            }
            ShellCommand::HomeEntryDown => {
                self.select_home_entry_row_delta(1);
                ShellAction::Redraw
            }
            ShellCommand::HomeFirstEntry => {
                self.select_home_entry(0);
                ShellAction::Redraw
            }
            ShellCommand::HomeLastEntry => {
                self.select_home_entry(self.user_home_entries().len().saturating_sub(1));
                ShellAction::Redraw
            }
            ShellCommand::ActivateSelectedHomeEntry => {
                self.activate_selected_home_entry(platform);
                ShellAction::Redraw
            }
            ShellCommand::Logout => {
                self.logout_at(received_at);
                ShellAction::Redraw
            }
            ShellCommand::SelectHomeEntryAt(coordinates) => {
                if let Some(index) = self.home_entry_index_at(coordinates) {
                    self.select_home_entry(index);
                }
                ShellAction::Redraw
            }
            ShellCommand::ActivateHomeEntryAt(coordinates, click) => {
                if let Some(index) = self.home_entry_index_at(coordinates) {
                    self.select_home_entry(index);
                    if click == ClickKind::Double {
                        self.activate_home_entry(index, platform);
                    }
                }
                ShellAction::Redraw
            }
            ShellCommand::OpenExplorer => {
                self.open_explorer(platform);
                ShellAction::Redraw
            }
            ShellCommand::CloseExplorer => {
                self.close_explorer();
                ShellAction::Redraw
            }
            ShellCommand::OpenEditor => {
                self.open_editor();
                ShellAction::Redraw
            }
            ShellCommand::CloseEditor => {
                self.request_editor_close(platform);
                ShellAction::Redraw
            }
            ShellCommand::EditorKey(key) => {
                self.handle_editor_key(key, platform);
                ShellAction::Redraw
            }
            ShellCommand::EditorPaste(value) => {
                self.handle_editor_paste(value);
                ShellAction::Redraw
            }
            ShellCommand::EditorPointer(mouse) => {
                self.handle_editor_pointer(mouse, platform);
                ShellAction::Redraw
            }
            ShellCommand::EditorSaveAndClose => {
                self.editor_close_after_save = true;
                self.apply_editor_command(
                    tundra_apps::editor::EditorCommand::RequestSave,
                    platform,
                );
                ShellAction::Redraw
            }
            ShellCommand::EditorDiscardAndClose => {
                self.editor_close_after_save = false;
                self.finish_editor_close(true);
                ShellAction::Redraw
            }
            ShellCommand::EditorCancelClose => {
                self.editor_close_after_save = false;
                self.notifications
                    .dismiss_modal_by_key(EDITOR_CLOSE_NOTIFICATION_KEY);
                self.notify_status("Close cancelled");
                ShellAction::Redraw
            }
            ShellCommand::EditorSaveAndOpen => {
                self.editor_open_after_save = true;
                self.editor_discard_for_open = false;
                self.notifications
                    .dismiss_modal_by_key(EDITOR_OPEN_NOTIFICATION_KEY);
                self.apply_editor_command(
                    tundra_apps::editor::EditorCommand::RequestSave,
                    platform,
                );
                ShellAction::Redraw
            }
            ShellCommand::EditorDiscardAndOpen => {
                self.editor_open_after_save = false;
                self.editor_discard_for_open = true;
                self.notifications
                    .dismiss_modal_by_key(EDITOR_OPEN_NOTIFICATION_KEY);
                self.open_editor_picker(platform);
                ShellAction::Redraw
            }
            ShellCommand::EditorCancelOpen => {
                self.editor_open_after_save = false;
                self.editor_discard_for_open = false;
                self.notifications
                    .dismiss_modal_by_key(EDITOR_OPEN_NOTIFICATION_KEY);
                self.notify_status("Open cancelled");
                ShellAction::Redraw
            }
            ShellCommand::ExplorerNext => {
                self.apply_explorer_command(ExplorerCommand::SelectNext, platform);
                ShellAction::Redraw
            }
            ShellCommand::ExplorerPrevious => {
                self.apply_explorer_command(ExplorerCommand::SelectPrevious, platform);
                ShellAction::Redraw
            }
            ShellCommand::ExplorerNextExtend => {
                let next = self
                    .explorer_state
                    .as_ref()
                    .filter(|state| !state.entries.is_empty())
                    .map(|state| (state.selected_index + 1).min(state.entries.len() - 1));
                if let Some(index) = next {
                    self.apply_explorer_command(
                        ExplorerCommand::SelectIndexWithMode(
                            index,
                            tundra_apps::explorer::ExplorerSelectionMode::Range,
                        ),
                        platform,
                    );
                }
                ShellAction::Redraw
            }
            ShellCommand::ExplorerPreviousExtend => {
                let previous = self
                    .explorer_state
                    .as_ref()
                    .map(|state| state.selected_index.saturating_sub(1));
                if let Some(index) = previous {
                    self.apply_explorer_command(
                        ExplorerCommand::SelectIndexWithMode(
                            index,
                            tundra_apps::explorer::ExplorerSelectionMode::Range,
                        ),
                        platform,
                    );
                }
                ShellAction::Redraw
            }
            ShellCommand::ExplorerSelectAll => {
                self.apply_explorer_command(ExplorerCommand::SelectAll, platform);
                ShellAction::Redraw
            }
            ShellCommand::ExplorerToggleFocused => {
                self.apply_explorer_command(ExplorerCommand::ToggleFocused, platform);
                ShellAction::Redraw
            }
            ShellCommand::ExplorerOpenSelected => {
                self.apply_explorer_command(ExplorerCommand::OpenSelected, platform);
                ShellAction::Redraw
            }
            ShellCommand::ExplorerOpenParent => {
                self.apply_explorer_command(ExplorerCommand::OpenParent, platform);
                ShellAction::Redraw
            }
            ShellCommand::ExplorerOpenBack => {
                self.apply_explorer_command(ExplorerCommand::OpenBack, platform);
                ShellAction::Redraw
            }
            ShellCommand::ExplorerOpenForward => {
                self.apply_explorer_command(ExplorerCommand::OpenForward, platform);
                ShellAction::Redraw
            }
            ShellCommand::ExplorerToggleHidden => {
                self.apply_explorer_command(ExplorerCommand::ToggleHidden, platform);
                ShellAction::Redraw
            }
            ShellCommand::ExplorerToggleSystem => {
                self.apply_explorer_command(ExplorerCommand::ToggleSystem, platform);
                ShellAction::Redraw
            }
            ShellCommand::ExplorerToggleExtensions => {
                self.apply_explorer_command(ExplorerCommand::ToggleExtensions, platform);
                ShellAction::Redraw
            }
            ShellCommand::ExplorerToggleFoldersFirst => {
                self.apply_explorer_command(ExplorerCommand::ToggleFoldersFirst, platform);
                ShellAction::Redraw
            }
            ShellCommand::ExplorerToggleCaseSensitiveSort => {
                self.apply_explorer_command(ExplorerCommand::ToggleCaseSensitiveSort, platform);
                ShellAction::Redraw
            }
            ShellCommand::ExplorerToggleSidebar => {
                self.apply_explorer_command(ExplorerCommand::ToggleSidebar, platform);
                ShellAction::Redraw
            }
            ShellCommand::ExplorerToggleSizeFormat => {
                self.apply_explorer_command(ExplorerCommand::ToggleSizeFormat, platform);
                ShellAction::Redraw
            }
            ShellCommand::ExplorerToggleDateZone => {
                self.apply_explorer_command(ExplorerCommand::ToggleDateZone, platform);
                ShellAction::Redraw
            }
            ShellCommand::ExplorerToggleDeleteConfirmation => {
                self.apply_explorer_command(ExplorerCommand::ToggleDeleteConfirmation, platform);
                ShellAction::Redraw
            }
            ShellCommand::ExplorerToggleConflictConfirmation => {
                self.apply_explorer_command(ExplorerCommand::ToggleConflictConfirmation, platform);
                ShellAction::Redraw
            }
            ShellCommand::ExplorerSortName => {
                self.apply_explorer_command(
                    ExplorerCommand::SetSort(tundra_apps::explorer::ExplorerSortField::Name),
                    platform,
                );
                ShellAction::Redraw
            }
            ShellCommand::ExplorerSortType => {
                self.apply_explorer_command(
                    ExplorerCommand::SetSort(tundra_apps::explorer::ExplorerSortField::Type),
                    platform,
                );
                ShellAction::Redraw
            }
            ShellCommand::ExplorerSortSize => {
                self.apply_explorer_command(
                    ExplorerCommand::SetSort(tundra_apps::explorer::ExplorerSortField::Size),
                    platform,
                );
                ShellAction::Redraw
            }
            ShellCommand::ExplorerSortModified => {
                self.apply_explorer_command(
                    ExplorerCommand::SetSort(tundra_apps::explorer::ExplorerSortField::Modified),
                    platform,
                );
                ShellAction::Redraw
            }
            ShellCommand::ExplorerCopy => {
                self.apply_explorer_command(ExplorerCommand::Copy, platform);
                ShellAction::Redraw
            }
            ShellCommand::ExplorerCut => {
                self.apply_explorer_command(ExplorerCommand::Cut, platform);
                ShellAction::Redraw
            }
            ShellCommand::ExplorerPaste => {
                self.apply_explorer_command(ExplorerCommand::Paste, platform);
                ShellAction::Redraw
            }
            ShellCommand::ExplorerDelete => {
                self.apply_explorer_command(ExplorerCommand::DeleteToTrash, platform);
                ShellAction::Redraw
            }
            ShellCommand::ExplorerConfirmDelete => {
                self.apply_explorer_command(ExplorerCommand::ConfirmDelete, platform);
                ShellAction::Redraw
            }
            ShellCommand::ExplorerRestore => {
                self.restore_selected_explorer_item(platform);
                ShellAction::Redraw
            }
            ShellCommand::ExplorerDumpTrash => {
                self.apply_explorer_command(ExplorerCommand::DumpTrash, platform);
                ShellAction::Redraw
            }
            ShellCommand::ExplorerConfirmDumpTrash => {
                self.apply_explorer_command(ExplorerCommand::ConfirmDumpTrash, platform);
                ShellAction::Redraw
            }
            ShellCommand::ExplorerRestoreKeepBoth => {
                self.apply_explorer_command(
                    ExplorerCommand::ResolveRestoreConflict(ExplorerConflictAction::KeepBoth),
                    platform,
                );
                ShellAction::Redraw
            }
            ShellCommand::ExplorerRestoreReplace => {
                self.apply_explorer_command(
                    ExplorerCommand::ResolveRestoreConflict(ExplorerConflictAction::Replace),
                    platform,
                );
                ShellAction::Redraw
            }
            ShellCommand::ExplorerRestoreCancel => {
                self.apply_explorer_command(
                    ExplorerCommand::ResolveRestoreConflict(ExplorerConflictAction::Cancel),
                    platform,
                );
                ShellAction::Redraw
            }
            ShellCommand::ExplorerConflictKeepBoth => {
                let apply_to_all = self.explorer_conflict_apply_to_remaining;
                self.apply_explorer_command(
                    ExplorerCommand::ResolveConflict {
                        action: ExplorerConflictAction::KeepBoth,
                        apply_to_all,
                    },
                    platform,
                );
                self.explorer_conflict_apply_to_remaining = false;
                ShellAction::Redraw
            }
            ShellCommand::ExplorerConflictReplace => {
                let apply_to_all = self.explorer_conflict_apply_to_remaining;
                self.apply_explorer_command(
                    ExplorerCommand::ResolveConflict {
                        action: ExplorerConflictAction::Replace,
                        apply_to_all,
                    },
                    platform,
                );
                self.explorer_conflict_apply_to_remaining = false;
                ShellAction::Redraw
            }
            ShellCommand::ExplorerConflictSkip => {
                let apply_to_all = self.explorer_conflict_apply_to_remaining;
                self.apply_explorer_command(
                    ExplorerCommand::ResolveConflict {
                        action: ExplorerConflictAction::Skip,
                        apply_to_all,
                    },
                    platform,
                );
                self.explorer_conflict_apply_to_remaining = false;
                ShellAction::Redraw
            }
            ShellCommand::ExplorerConflictCancel => {
                self.apply_explorer_command(
                    ExplorerCommand::ResolveConflict {
                        action: ExplorerConflictAction::Cancel,
                        apply_to_all: false,
                    },
                    platform,
                );
                self.explorer_conflict_apply_to_remaining = false;
                ShellAction::Redraw
            }
            ShellCommand::ExplorerConflictToggleApplyToRemaining => {
                self.explorer_conflict_apply_to_remaining =
                    !self.explorer_conflict_apply_to_remaining;
                ShellAction::Redraw
            }
            ShellCommand::ExplorerCancelOperation => {
                self.apply_explorer_command(ExplorerCommand::CancelOperation, platform);
                ShellAction::Redraw
            }
            ShellCommand::ExplorerOverlayPrevious => {
                self.move_explorer_overlay_selection(-1);
                ShellAction::Redraw
            }
            ShellCommand::ExplorerOverlayNext => {
                self.move_explorer_overlay_selection(1);
                ShellAction::Redraw
            }
            ShellCommand::ExplorerOverlayActivate => {
                self.activate_selected_explorer_overlay(platform);
                ShellAction::Redraw
            }
            ShellCommand::ExplorerSelectAt(coordinates, click) => {
                self.select_explorer_at(coordinates, click, platform);
                ShellAction::Redraw
            }
            ShellCommand::ExplorerPointerDown(coordinates, click, modifiers) => {
                self.pointer_down_explorer_at(coordinates, click, modifiers, platform);
                ShellAction::Redraw
            }
            ShellCommand::ExplorerDragUpdate(coordinates, modifiers) => {
                self.update_explorer_drag(coordinates, modifiers, platform);
                ShellAction::Redraw
            }
            ShellCommand::ExplorerDrop(coordinates, modifiers) => {
                self.drop_explorer_drag(coordinates, modifiers, platform);
                ShellAction::Redraw
            }
            ShellCommand::ExplorerCancelDrag => {
                self.apply_explorer_command(ExplorerCommand::CancelDrag, platform);
                ShellAction::Redraw
            }
            ShellCommand::ExplorerScroll(delta) => {
                if let Some(state) = self.explorer_state.as_mut() {
                    state.viewport_follows_focus = false;
                    if delta < 0 {
                        state.viewport_offset = state
                            .viewport_offset
                            .saturating_sub(delta.unsigned_abs() as usize);
                    } else {
                        state.viewport_offset = state
                            .viewport_offset
                            .saturating_add(delta as usize)
                            .min(state.entries.len().saturating_sub(1));
                    }
                }
                ShellAction::Redraw
            }
            ShellCommand::BeginExplorerSearch => {
                self.begin_explorer_input(ExplorerInputMode::Search);
                ShellAction::Redraw
            }
            ShellCommand::BeginExplorerAddress => {
                self.begin_explorer_input(ExplorerInputMode::Address);
                ShellAction::Redraw
            }
            ShellCommand::BeginExplorerNewFolder => {
                self.begin_explorer_input(ExplorerInputMode::NewFolder);
                ShellAction::Redraw
            }
            ShellCommand::BeginExplorerNewTextFile => {
                self.begin_explorer_input(ExplorerInputMode::NewTextFile);
                ShellAction::Redraw
            }
            ShellCommand::BeginExplorerRename => {
                self.begin_explorer_input(ExplorerInputMode::Rename);
                ShellAction::Redraw
            }
            ShellCommand::AppendExplorerChar(character) => {
                self.append_explorer_char(character, platform);
                ShellAction::Redraw
            }
            ShellCommand::ExplorerBackspace => {
                self.explorer_backspace(platform);
                ShellAction::Redraw
            }
            ShellCommand::SubmitExplorerInput => {
                self.submit_explorer_input(platform);
                ShellAction::Redraw
            }
            ShellCommand::CancelExplorerInput => {
                self.cancel_explorer_input();
                ShellAction::Redraw
            }
            ShellCommand::OpenUserManagement => {
                self.open_user_management();
                ShellAction::Redraw
            }
            ShellCommand::CloseUserManagement => {
                self.close_user_management();
                ShellAction::Redraw
            }
            ShellCommand::OpenClock => {
                self.open_clock();
                ShellAction::Redraw
            }
            ShellCommand::CloseClock => {
                self.close_clock();
                ShellAction::Redraw
            }
            ShellCommand::OpenDiagnostics => {
                self.open_diagnostics();
                ShellAction::Redraw
            }
            ShellCommand::CloseDiagnostics => {
                self.close_diagnostics();
                ShellAction::Redraw
            }
            ShellCommand::DiagnosticsHealthTab => {
                self.set_diagnostics_tab(tundra_ui::DiagnosticsTab::Health);
                ShellAction::Redraw
            }
            ShellCommand::DiagnosticsIncidentsTab => {
                self.set_diagnostics_tab(tundra_ui::DiagnosticsTab::Incidents);
                ShellAction::Redraw
            }
            ShellCommand::DiagnosticsPrevious => {
                self.move_diagnostics_selection(-1);
                ShellAction::Redraw
            }
            ShellCommand::DiagnosticsNext => {
                self.move_diagnostics_selection(1);
                ShellAction::Redraw
            }
            ShellCommand::DiagnosticsPageUp => {
                self.move_diagnostics_selection(-8);
                ShellAction::Redraw
            }
            ShellCommand::DiagnosticsPageDown => {
                self.move_diagnostics_selection(8);
                ShellAction::Redraw
            }
            ShellCommand::DiagnosticsFirst => {
                self.move_diagnostics_selection(-(self.diagnostics_item_count() as isize));
                ShellAction::Redraw
            }
            ShellCommand::DiagnosticsLast => {
                self.move_diagnostics_selection(self.diagnostics_item_count() as isize);
                ShellAction::Redraw
            }
            ShellCommand::DiagnosticsSelectIndex(index) => {
                self.select_diagnostics_index(index);
                ShellAction::Redraw
            }
            ShellCommand::DiagnosticsRescan => {
                self.request_diagnostics_scan();
                ShellAction::Redraw
            }
            ShellCommand::DiagnosticsPreviewSelectedRepair => {
                self.preview_selected_diagnostics_repair();
                ShellAction::Redraw
            }
            ShellCommand::DiagnosticsPreviewAllRepairs => {
                self.preview_all_diagnostics_repairs();
                ShellAction::Redraw
            }
            ShellCommand::DiagnosticsCancelRepair => {
                self.cancel_diagnostics_repair_preview();
                ShellAction::Redraw
            }
            ShellCommand::DiagnosticsConfirmRepair => {
                self.confirm_diagnostics_repair();
                ShellAction::Redraw
            }
            ShellCommand::DiagnosticsRepairPrevious => {
                self.move_diagnostics_repair_selection(-1);
                ShellAction::Redraw
            }
            ShellCommand::DiagnosticsRepairNext => {
                self.move_diagnostics_repair_selection(1);
                ShellAction::Redraw
            }
            ShellCommand::DiagnosticsRepairToggleAction => {
                self.diagnostics_repair_confirm_selected =
                    !self.diagnostics_repair_confirm_selected;
                ShellAction::Redraw
            }
            ShellCommand::DiagnosticsSelectRepairItem(index) => {
                self.select_diagnostics_repair_item(index);
                ShellAction::Redraw
            }
            ShellCommand::DiagnosticsCopySummary => {
                self.copy_diagnostics_summary(platform);
                ShellAction::Redraw
            }
            ShellCommand::DiagnosticsOpenReport => {
                self.open_selected_diagnostics_report(platform);
                ShellAction::Redraw
            }
            ShellCommand::ClockOpenCreate => {
                self.open_clock_create_dialog();
                ShellAction::Redraw
            }
            ShellCommand::ClockCloseCreate => {
                self.close_clock_create_dialog();
                ShellAction::Redraw
            }
            ShellCommand::ClockCreateFocusNext => {
                self.move_clock_create_focus(1);
                ShellAction::Redraw
            }
            ShellCommand::ClockCreateFocusPrevious => {
                self.move_clock_create_focus(-1);
                ShellAction::Redraw
            }
            ShellCommand::ClockCreateSetFocus(focus) => {
                self.set_clock_create_focus(focus);
                ShellAction::Redraw
            }
            ShellCommand::ClockCreateAppend(character) => {
                self.append_clock_create_char(character);
                ShellAction::Redraw
            }
            ShellCommand::ClockCreateBackspace => {
                self.clock_create_backspace();
                ShellAction::Redraw
            }
            ShellCommand::ClockCreateAlarm => {
                self.create_clock_entry(ScheduledClockEntryKind::DailyAlarm);
                ShellAction::Redraw
            }
            ShellCommand::ClockCreateCountdown => {
                self.create_clock_entry(ScheduledClockEntryKind::Countdown);
                ShellAction::Redraw
            }
            ShellCommand::ClockSelectPrevious => {
                self.select_clock_entry_delta(-1);
                ShellAction::Redraw
            }
            ShellCommand::ClockSelectNext => {
                self.select_clock_entry_delta(1);
                ShellAction::Redraw
            }
            ShellCommand::ClockSelectPageUp => {
                let page = self.clock_entry_capacity_at(Instant::now()) as isize;
                self.select_clock_entry_delta(-page.max(1));
                ShellAction::Redraw
            }
            ShellCommand::ClockSelectPageDown => {
                let page = self.clock_entry_capacity_at(Instant::now()) as isize;
                self.select_clock_entry_delta(page.max(1));
                ShellAction::Redraw
            }
            ShellCommand::ClockSelectFirst => {
                self.select_clock_entry_edge(false);
                ShellAction::Redraw
            }
            ShellCommand::ClockSelectLast => {
                self.select_clock_entry_edge(true);
                ShellAction::Redraw
            }
            ShellCommand::ClockSelectEntry(id) => {
                self.select_clock_entry(id);
                ShellAction::Redraw
            }
            ShellCommand::ClockActivateSelected => {
                if let Some(id) = self.clock_selected_entry_id {
                    self.show_clock_manage_dialog(id);
                }
                ShellAction::Redraw
            }
            ShellCommand::ClockManageEntry(id) => {
                self.select_clock_entry(id);
                self.show_clock_manage_dialog(id);
                ShellAction::Redraw
            }
            ShellCommand::ClockDeleteEntry(id) => {
                self.delete_clock_entry(id);
                ShellAction::Redraw
            }
            ShellCommand::ClockToggleStrong(id) => {
                self.toggle_clock_entry_strong(id);
                ShellAction::Redraw
            }
            ShellCommand::ClockSnoozeFiveMinutes(id) => {
                self.snooze_clock_alarm(id);
                ShellAction::Redraw
            }
            ShellCommand::UserManagementNext => {
                self.select_user_management_row(1);
                ShellAction::Redraw
            }
            ShellCommand::UserManagementPrevious => {
                self.select_user_management_row(-1);
                ShellAction::Redraw
            }
            ShellCommand::UserManagementPageUp => {
                self.select_user_management_page(-1);
                ShellAction::Redraw
            }
            ShellCommand::UserManagementPageDown => {
                self.select_user_management_page(1);
                ShellAction::Redraw
            }
            ShellCommand::UserManagementFirst => {
                self.select_user_management_edge(false);
                ShellAction::Redraw
            }
            ShellCommand::UserManagementLast => {
                self.select_user_management_edge(true);
                ShellAction::Redraw
            }
            ShellCommand::UserManagementSelectRow(index) => {
                if index < self.user_management_users.len() {
                    self.user_management_selected = index;
                    self.user_management_focus = UserManagementPageFocus::UserList;
                    self.ensure_user_management_selection_visible();
                }
                ShellAction::Redraw
            }
            ShellCommand::UserManagementFocusAction(action) => {
                self.focus_user_management_action(action);
                ShellAction::Redraw
            }
            ShellCommand::UserManagementActivateFocused => {
                self.activate_focused_user_management_control();
                self.refresh_hit_map();
                ShellAction::Redraw
            }
            ShellCommand::UserManagementActivateAction(action) => {
                self.focus_user_management_action(action);
                self.activate_user_management_action(action);
                self.refresh_hit_map();
                ShellAction::Redraw
            }
            ShellCommand::UserManagementSetFormFocus(field) => {
                self.set_user_management_form_focus(field);
                ShellAction::Redraw
            }
            ShellCommand::UserManagementActivateFormControl(field) => {
                self.set_user_management_form_focus(field);
                match field {
                    tundra_ui::UserManagementField::Role => {
                        self.toggle_user_management_form_role();
                    }
                    tundra_ui::UserManagementField::Submit => {
                        self.submit_user_management_form();
                    }
                    tundra_ui::UserManagementField::Cancel => {
                        self.cancel_user_management_form();
                    }
                    tundra_ui::UserManagementField::Username
                    | tundra_ui::UserManagementField::DisplayName
                    | tundra_ui::UserManagementField::Password => {}
                }
                self.refresh_hit_map();
                ShellAction::Redraw
            }
            ShellCommand::UserManagementToggleFormRole => {
                self.toggle_user_management_form_role();
                ShellAction::Redraw
            }
            ShellCommand::CreateManagedUser => {
                self.begin_create_managed_user();
                self.refresh_hit_map();
                ShellAction::Redraw
            }
            ShellCommand::EditManagedUserInfo => {
                self.begin_edit_selected_user_info();
                self.refresh_hit_map();
                ShellAction::Redraw
            }
            ShellCommand::DisableManagedUser => {
                self.disable_selected_user();
                ShellAction::Redraw
            }
            ShellCommand::UnlockManagedUser => {
                self.unlock_selected_user();
                ShellAction::Redraw
            }
            ShellCommand::ResetManagedPassword => {
                self.reset_selected_password();
                self.refresh_hit_map();
                ShellAction::Redraw
            }
            ShellCommand::CycleManagedRole => {
                self.cycle_selected_role();
                self.normalize_user_management_focus();
                ShellAction::Redraw
            }
            ShellCommand::RequestDeleteManagedUser => {
                self.request_delete_selected_user();
                ShellAction::Redraw
            }
            ShellCommand::DeleteManagedUser => {
                self.delete_selected_user();
                self.normalize_user_management_focus();
                ShellAction::Redraw
            }
            ShellCommand::AppendUserManagementChar(character) => {
                self.append_user_management_char(character);
                ShellAction::Redraw
            }
            ShellCommand::UserManagementBackspace => {
                self.user_management_backspace();
                ShellAction::Redraw
            }
            ShellCommand::UserManagementFocusNext => {
                if self.user_management_mode == UserManagementMode::Browse {
                    self.move_user_management_page_focus(1);
                } else {
                    self.move_user_management_form_focus(1);
                }
                ShellAction::Redraw
            }
            ShellCommand::UserManagementFocusPrevious => {
                if self.user_management_mode == UserManagementMode::Browse {
                    self.move_user_management_page_focus(-1);
                } else {
                    self.move_user_management_form_focus(-1);
                }
                ShellAction::Redraw
            }
            ShellCommand::SubmitUserManagementForm => {
                self.submit_user_management_form();
                self.refresh_hit_map();
                ShellAction::Redraw
            }
            ShellCommand::CancelUserManagementForm => {
                self.cancel_user_management_form();
                self.refresh_hit_map();
                ShellAction::Redraw
            }
            ShellCommand::Hover(target) => {
                self.hovered_component = target;
                ShellAction::Redraw
            }
            ShellCommand::Activate {
                target,
                coordinates,
                click,
            } => {
                if target == ShellComponent::ContextMenu && self.explorer_overlay_mode.is_some() {
                    self.activate_explorer_overlay_at(coordinates, platform);
                    return ShellAction::Redraw;
                }
                if target == ShellComponent::Explorer {
                    self.focus_component(target);
                    self.select_explorer_at(coordinates, click, platform);
                    return ShellAction::Redraw;
                }
                self.focus_component(target);
                let click_label = match click {
                    ClickKind::Single => "single click",
                    ClickKind::Double => "double click",
                };
                self.notify_status(format!("{} activated by {click_label}", target.label()));
                ShellAction::Redraw
            }
            ShellCommand::OpenContextMenu {
                target,
                coordinates,
            } => {
                if target == Some(ShellComponent::Explorer)
                    && let Some(index) = self.explorer_index_at(coordinates)
                {
                    let already_selected = self
                        .explorer_state
                        .as_ref()
                        .and_then(|state| {
                            state
                                .entries
                                .get(index)
                                .map(|entry| state.is_selected(&entry.path))
                        })
                        .unwrap_or(false);
                    if !already_selected {
                        self.apply_explorer_command(ExplorerCommand::SelectIndex(index), platform);
                    }
                } else if target == Some(ShellComponent::Explorer)
                    && let Some(state) = self.explorer_state.as_mut()
                {
                    state.clear_selection();
                }
                self.explorer_overlay_mode = (target == Some(ShellComponent::Explorer)).then_some(
                    ExplorerOverlayMode::ContextMenu {
                        anchor: coordinates,
                    },
                );
                self.explorer_overlay_selection = 0;
                self.active_popup = Some(ShellPopup {
                    owner: target,
                    anchor: coordinates,
                });
                self.focused_component = ShellComponent::ContextMenu;
                let status = match target {
                    Some(target) => format!("Context menu: {}", target.label()),
                    None => "Context menu".to_string(),
                };
                self.notify_status(status);
                self.refresh_hit_map();
                ShellAction::Redraw
            }
            ShellCommand::ClosePopup => {
                self.active_popup = None;
                self.explorer_overlay_mode = None;
                self.explorer_overlay_selection = 0;
                self.notify_status("Ready");
                self.refresh_hit_map();
                ShellAction::Redraw
            }
            ShellCommand::CloseTimeSyncDialog => {
                self.close_time_sync_dialog();
                ShellAction::Redraw
            }
            ShellCommand::NotificationNextAction => {
                self.notifications.select_next_action();
                ShellAction::Redraw
            }
            ShellCommand::NotificationPreviousAction => {
                self.notifications.select_previous_action();
                ShellAction::Redraw
            }
            ShellCommand::NotificationActivateSelected => self.activate_notification_selected(),
            ShellCommand::NotificationActivateAction(index) => {
                self.activate_notification_action(index)
            }
            ShellCommand::NotificationCancel => {
                if let Some(index) = self.notifications.explicit_cancel_action_index() {
                    self.activate_notification_action(index)
                } else if !self.notification_can_render()
                    && self.notifications.dismiss_active_modal_without_response()
                {
                    self.apply_notification_follow_up(None)
                } else if let Some(index) = self.notifications.cancel_action_index() {
                    self.activate_notification_action(index)
                } else {
                    ShellAction::Redraw
                }
            }
            ShellCommand::CaptureOverlayInput => ShellAction::Redraw,
            ShellCommand::RecordInput | ShellCommand::Noop => ShellAction::Redraw,
        }
    }
}
