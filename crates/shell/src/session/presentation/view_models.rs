use super::super::*;
impl ShellSession {
    pub fn to_home_view_model(&self) -> ui::HomeViewModel {
        let user = self.current_home_username().unwrap_or("Unauthenticated");
        let model = ui::HomeViewModel::user_with_selection_and_icon_assets(
            user,
            self.current_time_label(),
            self.user_home_entries(),
            self.selected_home_entry_index(),
            self.ascii_assets.clone(),
        );
        let model = match self.home_mode {
            ShellHomeMode::Debug => model.with_debug_diagnostics(ui::DebugDiagnosticsViewModel {
                tick_count: self.tick_count,
                last_key_event: self.last_key_event.clone(),
                last_mouse_event: self.last_mouse_event.clone(),
                last_resize_event: self.last_resize_event.clone(),
                mouse_coordinates: self.mouse_coordinates,
                scroll_direction: self.mouse_scroll_direction.clone(),
                drag_direction: self.mouse_drag_direction.clone(),
                terminal_flags: terminal_flag_labels(self.terminal_flags),
                platform_capability_summary: self.platform_capability_summary.clone(),
            }),
            ShellHomeMode::User => model,
        };
        if let Some(username) = self.current_home_username() {
            model.with_account_logout(
                username,
                self.focused_component == ShellComponent::HomeLogout,
            )
        } else {
            model
        }
    }

    pub fn to_diagnostics_view_model(&self) -> ui::DiagnosticsViewModel {
        let can_view_details = self.diagnostics_can_view_details();
        let can_repair = self.diagnostics_can_repair();
        let (checks, logs, incidents, scanned_at) = self
            .app
            .diagnostics_snapshot()
            .map(|snapshot| {
                let checks = snapshot
                    .checks
                    .iter()
                    .map(|check| ui::DiagnosticsCheckViewModel {
                        id: check.id.clone(),
                        label: check.label.clone(),
                        category: check.category.label().to_string(),
                        status: diagnostics_status_to_ui(check.status),
                        summary: if can_view_details {
                            check.summary.clone()
                        } else {
                            diagnostics_public_check_summary(check)
                        },
                        detail: if can_view_details {
                            check.detail.clone()
                        } else {
                            String::new()
                        },
                        remediation: check.remediation.clone().unwrap_or_default(),
                        repairable: check.repair.is_some(),
                    })
                    .collect();
                let incidents = snapshot
                    .incidents
                    .iter()
                    .map(|incident| {
                        let app = incident
                            .app
                            .as_ref()
                            .map(|app| app.display_name.clone())
                            .unwrap_or_else(|| "TundraUX process".to_string());
                        let recovery = if can_view_details {
                            format!("{:?}", incident.recovery)
                        } else {
                            diagnostics_recovery_label(&incident.recovery)
                        };
                        let detail = if can_view_details {
                            format!(
                                "Boundary: {}; Component: {}",
                                incident.boundary,
                                incident.component.as_deref().unwrap_or("none")
                            )
                        } else {
                            String::new()
                        };
                        ui::DiagnosticsIncidentViewModel {
                            id: if can_view_details {
                                incident.incident_id.clone()
                            } else {
                                String::new()
                            },
                            occurred_at: incident
                                .occurred_at
                                .format("%Y-%m-%d %H:%M:%S UTC")
                                .to_string(),
                            app,
                            severity: diagnostics_incident_severity_to_ui(incident.severity),
                            recovery,
                            summary: if can_view_details {
                                incident.summary.clone()
                            } else {
                                String::new()
                            },
                            detail,
                            report_path: if can_view_details {
                                incident
                                    .text_report_path
                                    .as_ref()
                                    .unwrap_or(&incident.json_report_path)
                                    .display()
                                    .to_string()
                            } else {
                                String::new()
                            },
                            restricted: !can_view_details,
                        }
                    })
                    .collect();
                let logs = if can_view_details {
                    snapshot
                        .logs
                        .iter()
                        .map(|log| ui::DiagnosticsLogViewModel {
                            relative_path: log.relative_path.display().to_string(),
                            path: log.path.display().to_string(),
                            modified_at: log
                                .modified_at
                                .format("%Y-%m-%d %H:%M:%S UTC")
                                .to_string(),
                            size_bytes: log.size_bytes,
                        })
                        .collect()
                } else {
                    Vec::new()
                };
                (
                    checks,
                    logs,
                    incidents,
                    Some(
                        snapshot
                            .scanned_at
                            .format("%Y-%m-%d %H:%M:%S UTC")
                            .to_string(),
                    ),
                )
            })
            .unwrap_or_else(|| (Vec::new(), Vec::new(), Vec::new(), None));

        let repair_dialog = (!self.diagnostics_repair_preview.is_empty()).then(|| {
            ui::DiagnosticsRepairDialogViewModel {
                items: self
                    .diagnostics_repair_preview
                    .iter()
                    .enumerate()
                    .map(|(index, action)| ui::DiagnosticsRepairItemViewModel {
                        id: index.to_string(),
                        label: action.label(),
                    })
                    .collect(),
                selected: self.diagnostics_repair_selected,
                confirm_selected: self.diagnostics_repair_confirm_selected,
                scroll_offset: self.diagnostics_repair_scroll_offset,
            }
        });

        ui::DiagnosticsViewModel {
            tab: self.diagnostics_tab,
            checks,
            logs,
            incidents,
            selected_check: self.diagnostics_selected_check,
            selected_log: self.diagnostics_selected_log,
            selected_incident: self.diagnostics_selected_incident,
            list_window_start: self.diagnostics_list_window_start,
            list_window_is_explicit: self.diagnostics_list_window_is_explicit,
            scanning: self.diagnostics_scanning
                || self
                    .diagnostics_task_runtime
                    .as_ref()
                    .is_some_and(ShellDiagnosticsTaskRuntime::is_busy),
            can_view_details,
            can_repair,
            restart_required: self.diagnostics_restart_is_required(),
            repair_dialog,
            feedback: self.diagnostics_feedback.clone(),
            scanned_at,
        }
    }

    pub(in crate::session) fn current_home_username(&self) -> Option<&str> {
        self.app
            .auth_session()
            .map(|session| session.username.as_str())
    }

    pub fn to_clock_view_model(&self) -> ui::ClockViewModel {
        let snapshot = self.app.snapshot().clock;
        self.to_clock_view_model_at(&snapshot, Instant::now())
    }

    pub(in crate::session) fn to_clock_view_model_at(
        &self,
        snapshot: &time::ClockSnapshot,
        now: Instant,
    ) -> ui::ClockViewModel {
        let mut alarms = Vec::new();
        let mut countdowns = Vec::new();
        if let Some(scheduler) = &self.clock_scheduler {
            for entry in scheduler.entries(now) {
                let label = match entry.kind {
                    ScheduledClockEntryKind::DailyAlarm => {
                        if entry.snoozed {
                            format!("{} Daily (snoozed)", entry.display_time)
                        } else {
                            format!("{} Daily", entry.display_time)
                        }
                    }
                    ScheduledClockEntryKind::Countdown => {
                        format!("{} left", entry.display_time)
                    }
                };
                let view = ui::ClockEntryViewModel::new(entry.id, label, entry.strong);
                match entry.kind {
                    ScheduledClockEntryKind::DailyAlarm => alarms.push(view),
                    ScheduledClockEntryKind::Countdown => countdowns.push(view),
                }
            }
        }

        let mut model = ui::ClockViewModel::at(
            snapshot.date.to_string(),
            snapshot.time.format("%H:%M:%S").to_string(),
            snapshot.time.hour() as u8,
            snapshot.time.minute() as u8,
            snapshot.time.second() as u8,
        )
        .with_ascii_assets(self.ascii_assets.clone())
        .with_read_only(self.is_strict_guest());
        model.alarms = alarms;
        model.countdowns = countdowns;
        model.selected_entry_id = (self.focused_component == ShellComponent::ClockEntryList)
            .then_some(self.clock_selected_entry_id)
            .flatten();
        model.entry_window_start = self.clock_entry_window_start;
        model.create_dialog =
            self.clock_create_state
                .as_ref()
                .map(|state| ui::ClockCreateDialogViewModel {
                    input: state.input.clone(),
                    error: state.error.clone(),
                    focus: state.focus,
                });
        model
    }

    pub fn to_time_sync_dialog_view_model(&self) -> Option<ui::TimeSyncDialogViewModel> {
        self.time_sync_dialog_visible
            .then(ui::TimeSyncDialogViewModel::new)
    }

    pub fn to_login_view_model(&self) -> ui::LoginViewModel {
        self.to_login_view_model_at(Instant::now())
    }

    pub fn to_login_view_model_at(&self, now: Instant) -> ui::LoginViewModel {
        let model = ui::LoginViewModel::new(
            self.login_users
                .iter()
                .map(|user| ui::LoginUserOptionViewModel {
                    username: user.username.clone(),
                    display_name: user.display_name.clone(),
                    role: user.role.clone(),
                    enabled: user.enabled,
                    locked: user
                        .locked_until_epoch_ms
                        .map(|locked_until| locked_until > unix_millis())
                        .unwrap_or(false),
                })
                .collect(),
            self.login_selected_user,
            self.login_user_window_start,
            self.login_password.chars().count(),
            match self.focused_component {
                ShellComponent::LoginPassword => ui::LoginField::Password,
                ShellComponent::LoginPasswordVisibility => ui::LoginField::PasswordVisibility,
                _ => ui::LoginField::UserList,
            },
            self.error_message.clone(),
        );
        if self.login_password_is_visible_at(now) {
            model.with_visible_password(self.login_password.clone())
        } else {
            model
        }
    }

    pub fn to_bootstrap_admin_view_model(&self) -> ui::BootstrapAdminViewModel {
        ui::BootstrapAdminViewModel::new(
            self.bootstrap_username.clone(),
            self.bootstrap_password.chars().count(),
            match self.focused_component {
                ShellComponent::BootstrapPassword => ui::AuthField::Password,
                _ => ui::AuthField::Username,
            },
            self.error_message.clone(),
        )
    }

    pub fn to_setup_view_model(&self) -> ui::SetupViewModel {
        let password_requirements = setup_password_requirements(
            &self.setup_admin_username,
            &self.setup_admin_password,
            &self.setup_admin_password_confirm,
        );
        let can_submit = !self.setup_admin_username.trim().is_empty()
            && password_requirements
                .iter()
                .all(|requirement| requirement.met);
        let custom_color = self
            .setup_custom_color_input
            .parse::<storage::BorderColor>()
            .ok();
        let custom_color_conflicts_with_theme = self.setup_custom_color_target
            == Some(ui::SetupCustomColorTarget::Accent)
            && custom_color == Some(self.setup_theme_color);

        ui::SetupViewModel {
            step: self.setup_step,
            languages: app::setup_language_options(),
            timezones: app::setup_timezone_options(),
            selected_language_index: self.setup_selected_language_index,
            selected_timezone_index: self.setup_selected_timezone_index,
            timezone_window_start: self.setup_timezone_window_start,
            admin_username: self.setup_admin_username.clone(),
            admin_password_len: self.setup_admin_password.chars().count(),
            admin_password_confirm_len: self.setup_admin_password_confirm.chars().count(),
            password_requirements,
            password_hint: self.setup_admin_password_hint.clone(),
            focused_field: self.setup_focused_field,
            can_submit,
            border_shape: match self.setup_border_shape {
                storage::BorderShape::Rounded => ui::BorderShape::Rounded,
                storage::BorderShape::Square => ui::BorderShape::Square,
            },
            theme_color: ui_theme_color(self.setup_theme_color),
            theme_color_value: self.setup_theme_color.to_string(),
            accent_color: ui_theme_color(self.setup_accent_color),
            accent_color_value: self.setup_accent_color.to_string(),
            custom_color_target: self.setup_custom_color_target,
            custom_color_input: self.setup_custom_color_input.clone(),
            custom_color_valid: !self.setup_custom_color_input.trim().is_empty()
                && custom_color.is_some()
                && !custom_color_conflicts_with_theme,
            custom_color_conflicts_with_theme,
            custom_color_error: self.setup_custom_color_error.clone(),
            error: self.error_message.clone(),
        }
    }

    pub fn to_user_management_view_model(&self) -> ui::UserManagementViewModel {
        let current_user = self
            .app
            .auth_session()
            .map(|session| session.username.clone())
            .unwrap_or_else(|| "Unauthenticated".to_string());
        let mut model = ui::UserManagementViewModel::new(
            current_user.clone(),
            self.app
                .managed_users()
                .iter()
                .map(|user| ui::UserManagementUserViewModel {
                    username: user.username.clone(),
                    display_name: user.display_name.clone(),
                    role: user.role.as_str().to_string(),
                    enabled: user.enabled,
                    locked: user
                        .locked_until_epoch_ms
                        .map(|locked_until| locked_until > unix_millis())
                        .unwrap_or(false),
                    is_current: user.username.eq_ignore_ascii_case(&current_user),
                })
                .collect(),
            self.user_management_selected,
            self.user_management_message.clone(),
            self.can_manage_all_users(),
            self.user_management_form_view_model(),
        );
        model.user_window_start = self.user_management_window_start;
        model.focus = match self.user_management_focus {
            UserManagementPageFocus::UserList => ui::UserManagementFocus::UserList,
            UserManagementPageFocus::Action(action) => ui::UserManagementFocus::Action(action),
        };
        model.actions = self.user_management_action_view_models();
        model.feedback_tone = match self.user_management_feedback_tone {
            UserManagementFeedbackTone::Info => ui::UserManagementFeedbackTone::Info,
            UserManagementFeedbackTone::Success => ui::UserManagementFeedbackTone::Success,
            UserManagementFeedbackTone::Error => ui::UserManagementFeedbackTone::Error,
        };
        model
    }

    pub fn to_explorer_view_model(&self) -> ui::ExplorerViewModel {
        let app_snapshot = self.app.snapshot();
        let Some(state) = self.app.explorer_state() else {
            return ui::ExplorerViewModel::new("Explorer unavailable", Vec::new(), None);
        };
        let is_trash = state.current_location.is_trash();
        let display_path = if is_trash {
            "Trash".to_string()
        } else {
            state.current_path.display().to_string()
        };

        let entries = state
            .entries
            .iter()
            .map(|entry| ui::ExplorerEntryViewModel {
                name: explorer_display_name(entry, state.show_extensions),
                kind: entry.type_label.clone(),
                size: (entry.kind == app::explorer::ExplorerEntryKind::File)
                    .then(|| explorer_size_label(entry.size, state.size_format)),
                modified: entry.modified.map(|modified| {
                    explorer_system_time_label(
                        modified,
                        state.date_zone,
                        app_snapshot.clock_timezone_id,
                    )
                }),
                attributes: explorer_attribute_labels(&entry.attributes),
                selected: state.is_selected(&entry.path),
            })
            .collect::<Vec<_>>();
        let selected_index = (!entries.is_empty()).then_some(state.selected_index);
        let mut model = ui::ExplorerViewModel::with_ascii_assets(
            display_path.clone(),
            entries,
            selected_index,
            self.ascii_assets.clone(),
        );
        model.is_trash = is_trash;
        model.address_editing = self.explorer_input_mode == ExplorerInputMode::Address;
        model.address_value = if model.address_editing {
            self.explorer_input.clone()
        } else {
            display_path
        };
        model.entry_presentations = state
            .entries
            .iter()
            .enumerate()
            .map(|(index, entry)| {
                let mut presentation = ui::ExplorerEntryPresentationViewModel::new(
                    entry.path.display().to_string(),
                    entry.path.display().to_string(),
                    entry.icon_key.clone(),
                    entry.kind == app::explorer::ExplorerEntryKind::Directory,
                );
                presentation.selected = state.is_selected(&entry.path);
                presentation.focused = index == state.selected_index;
                presentation.cut = state.clipboard.as_ref().is_some_and(|clipboard| {
                    clipboard.mode == app::explorer::ExplorerClipboardMode::Cut
                        && clipboard.paths.contains(&entry.path)
                });
                presentation.drop_target = state
                    .drag
                    .as_ref()
                    .and_then(|drag| drag.target.as_ref())
                    .is_some_and(|target| target == &entry.path);
                presentation.metadata_warning = entry.metadata_warning.clone();
                presentation.original_path = entry
                    .original_path
                    .as_ref()
                    .map(|path| path.display().to_string());
                presentation
            })
            .collect();
        model.quick_locations = state
            .quick_locations
            .iter()
            .map(|location| {
                let mut model = ui::ExplorerQuickLocationViewModel::new(
                    location.id.clone(),
                    location.label.clone(),
                    location.path.display().to_string(),
                    location.icon_key.clone(),
                );
                model.kind = location.kind;
                model.current = if location.is_trash() {
                    is_trash
                } else {
                    !is_trash && location.path == state.current_path
                };
                model.enabled = location.enabled && (location.is_trash() || location.path.is_dir());
                model.drop_target = !location.is_trash()
                    && state
                        .drag
                        .as_ref()
                        .and_then(|drag| drag.target.as_ref())
                        .is_some_and(|target| target == &location.path);
                model
            })
            .collect();
        model.breadcrumbs = if is_trash {
            Vec::new()
        } else {
            explorer_breadcrumb_view_models(&state.current_path, state)
        };
        model.sort_column = match state.sort_field {
            app::explorer::ExplorerSortField::Name => ui::ExplorerSortColumn::Name,
            app::explorer::ExplorerSortField::Type => ui::ExplorerSortColumn::Type,
            app::explorer::ExplorerSortField::Size => ui::ExplorerSortColumn::Size,
            app::explorer::ExplorerSortField::Modified => ui::ExplorerSortColumn::Modified,
        };
        model.sort_direction = state.sort_direction;
        model.viewport_offset = state.viewport_offset;
        model.viewport_follows_focus = state.viewport_follows_focus;
        model.show_sidebar = state.show_sidebar;
        model.selected_count = state.effective_selected_paths().len();
        model.listing_warning_count = state.listing_warning_count;
        model.set_history_availability(
            !state.back_history.is_empty(),
            !state.forward_history.is_empty(),
        );
        let busy = state.operation.is_some();
        if is_trash {
            model.toolbar = ui::ExplorerToolbarViewModel::trash(
                !state.back_history.is_empty(),
                !state.forward_history.is_empty(),
                model.selected_count == 1 && !busy,
                !state.entries.is_empty() && !busy,
            );
        }
        for button in &mut model.toolbar.buttons {
            button.enabled = match button.action {
                ui::ExplorerToolbarAction::Back => !state.back_history.is_empty(),
                ui::ExplorerToolbarAction::Forward => !state.forward_history.is_empty(),
                ui::ExplorerToolbarAction::Up => !is_trash && state.current_path.parent().is_some(),
                ui::ExplorerToolbarAction::New => !is_trash && !busy,
                ui::ExplorerToolbarAction::Cut
                | ui::ExplorerToolbarAction::Copy
                | ui::ExplorerToolbarAction::Delete => model.selected_count > 0 && !busy,
                ui::ExplorerToolbarAction::Paste => state.clipboard.is_some() && !busy,
                ui::ExplorerToolbarAction::Rename => {
                    !is_trash && model.selected_count == 1 && !busy
                }
                ui::ExplorerToolbarAction::Restore => {
                    is_trash && model.selected_count == 1 && !busy
                }
                ui::ExplorerToolbarAction::DumpTrash => {
                    is_trash && !state.entries.is_empty() && !busy
                }
                _ => true,
            };
        }
        model.operation = state.operation.as_ref().map(|operation| {
            let phase = match operation.phase {
                app::explorer::ExplorerOperationPhase::Scanning => {
                    ui::ExplorerProgressStage::Scanning
                }
                app::explorer::ExplorerOperationPhase::WaitingForConflict => {
                    ui::ExplorerProgressStage::CheckingConflicts
                }
                app::explorer::ExplorerOperationPhase::Executing => {
                    if operation.label.to_ascii_lowercase().contains("mov") {
                        ui::ExplorerProgressStage::Moving
                    } else if operation.label.to_ascii_lowercase().contains("trash") {
                        ui::ExplorerProgressStage::Deleting
                    } else {
                        ui::ExplorerProgressStage::Copying
                    }
                }
                app::explorer::ExplorerOperationPhase::Completed
                | app::explorer::ExplorerOperationPhase::Cancelled
                | app::explorer::ExplorerOperationPhase::Failed => {
                    ui::ExplorerProgressStage::Finishing
                }
            };
            ui::ExplorerOperationProgressViewModel {
                phase,
                label: operation.label.clone(),
                completed_items: operation.completed_items as u64,
                total_items: operation.total_items.map(|value| value as u64),
                completed_bytes: operation.completed_bytes,
                total_bytes: operation.total_bytes,
                cancellable: operation.cancellable,
                cancel_label: "Cancel".to_string(),
            }
        });
        model.show_hidden = state.show_hidden;
        model.message = if is_trash && model.selected_count > 0 {
            state.selected_entry().map(|entry| {
                entry.original_path.as_ref().map_or_else(
                    || "Original location unavailable".to_string(),
                    |path| format!("Original location: {}", path.display()),
                )
            })
        } else {
            state.message.clone()
        };
        model.error = state.error.clone();
        model.search = if self.explorer_input_mode == ExplorerInputMode::Search {
            Some(ui::ExplorerSearchViewModel::new(
                self.explorer_input.clone(),
                true,
                Some(state.entries.len()),
            ))
        } else if !state.query.is_empty() {
            Some(ui::ExplorerSearchViewModel::new(
                state.query.clone(),
                false,
                Some(state.entries.len()),
            ))
        } else {
            None
        };
        model.pending_dialog = state.pending_dialog.as_ref().map(|dialog| {
            let (confirm, cancel) = match dialog.kind {
                app::explorer::ExplorerDialogKind::DeleteToTrash => {
                    ("Y / Enter: move", "N / Esc: cancel")
                }
                app::explorer::ExplorerDialogKind::DumpTrash => {
                    ("Y / Enter: empty permanently", "N / Esc: cancel")
                }
            };
            ui::ExplorerDialogViewModel::new(
                dialog.title.clone(),
                dialog.message.clone(),
                confirm,
                cancel,
            )
        });

        model.overlay = if let Some(conflict) = state.pending_restore.as_ref() {
            Some(ui::ExplorerOverlayViewModel::Conflict(
                ui::ExplorerConflictViewModel {
                    title: "Restore conflict".to_string(),
                    source: format!("Trash: {}", conflict.display_name),
                    destination: conflict.target.display().to_string(),
                    choices: vec![
                        ui::ExplorerConflictChoice::KeepBoth,
                        ui::ExplorerConflictChoice::Replace,
                        ui::ExplorerConflictChoice::Cancel,
                    ],
                    selected_choice: ui::ExplorerConflictChoice::KeepBoth,
                    apply_to_remaining: false,
                    allow_apply_to_remaining: false,
                },
            ))
        } else if let Some(conflict) = state.pending_conflict.as_ref() {
            Some(ui::ExplorerOverlayViewModel::Conflict(
                ui::ExplorerConflictViewModel {
                    title: "Name conflict".to_string(),
                    source: conflict.source.display().to_string(),
                    destination: conflict.target.display().to_string(),
                    choices: vec![
                        ui::ExplorerConflictChoice::KeepBoth,
                        ui::ExplorerConflictChoice::Replace,
                        ui::ExplorerConflictChoice::Skip,
                        ui::ExplorerConflictChoice::Cancel,
                    ],
                    selected_choice: ui::ExplorerConflictChoice::KeepBoth,
                    apply_to_remaining: self.explorer_conflict_apply_to_remaining,
                    allow_apply_to_remaining: true,
                },
            ))
        } else if matches!(
            self.explorer_input_mode,
            ExplorerInputMode::NewFolder
                | ExplorerInputMode::NewTextFile
                | ExplorerInputMode::Rename
                | ExplorerInputMode::RestoreDestination
        ) {
            let (kind, title, prompt, confirm_label) = match self.explorer_input_mode {
                ExplorerInputMode::NewFolder => (
                    ui::ExplorerNameDialogKind::NewFolder,
                    "New folder",
                    "Folder name",
                    "Create",
                ),
                ExplorerInputMode::NewTextFile => (
                    ui::ExplorerNameDialogKind::NewTextFile,
                    "New text file",
                    "File name",
                    "Create",
                ),
                ExplorerInputMode::Rename => (
                    ui::ExplorerNameDialogKind::Rename,
                    "Rename",
                    "New name",
                    "Rename",
                ),
                ExplorerInputMode::RestoreDestination => (
                    ui::ExplorerNameDialogKind::RestoreDestination,
                    "Restore item",
                    "Absolute destination directory",
                    "Restore",
                ),
                ExplorerInputMode::Browse
                | ExplorerInputMode::Address
                | ExplorerInputMode::Search => unreachable!(),
            };
            Some(ui::ExplorerOverlayViewModel::Name(
                ui::ExplorerNameDialogViewModel {
                    kind,
                    title: title.to_string(),
                    prompt: prompt.to_string(),
                    value: self.explorer_input.clone(),
                    error: state.error.clone(),
                    confirm_label: confirm_label.to_string(),
                    cancel_label: "Cancel".to_string(),
                },
            ))
        } else if let Some(overlay_mode) = self.explorer_overlay_mode {
            Some(match overlay_mode {
                ExplorerOverlayMode::ContextMenu { anchor } => explorer_context_menu_view_model(
                    anchor,
                    model.selected_count,
                    state.clipboard.is_some(),
                    is_trash,
                    !state.entries.is_empty(),
                    self.explorer_overlay_selection,
                    self.can_manage_launcher(),
                    state
                        .effective_selected_paths()
                        .iter()
                        .filter(|path| {
                            state.entries.iter().any(|entry| {
                                entry.path == **path && entry.open_policy.requires_launcher()
                            })
                        })
                        .count(),
                ),
                ExplorerOverlayMode::Sort { anchor } => explorer_sort_menu_view_model(
                    anchor,
                    model.sort_column,
                    self.explorer_overlay_selection,
                ),
                ExplorerOverlayMode::Options => explorer_options_view_model(
                    state,
                    self.explorer_overlay_selection,
                    self.can_change_explorer_settings(),
                ),
                ExplorerOverlayMode::Properties => {
                    explorer_properties_view_model(state, app_snapshot.clock_timezone_id)
                }
            })
        } else {
            None
        };

        model
    }

    pub(in crate::session) fn can_manage_all_users(&self) -> bool {
        matches!(
            self.app.auth_session().map(|session| session.role),
            Some(UserRole::Admin)
        )
    }

    pub(in crate::session) fn user_management_action_view_models(
        &self,
    ) -> Vec<ui::UserManagementActionViewModel> {
        use ui::UserManagementAction;

        let selected = self.app.managed_users().get(self.user_management_selected);
        let last_enabled_admin = self.selected_is_last_enabled_admin();
        let no_selection_reason = selected.is_none().then(|| "No user selected".to_string());
        let protected_reason =
            last_enabled_admin.then(|| "At least one enabled admin is required".to_string());
        let mut actions = Vec::new();

        if self.can_manage_all_users() {
            actions.push(user_management_action_model(
                UserManagementAction::NewUser,
                "New user",
                Some('N'),
                true,
                None,
                false,
            ));
        }

        actions.push(user_management_action_model(
            UserManagementAction::EditInfo,
            if self.can_manage_all_users() {
                "Edit"
            } else {
                "Edit profile"
            },
            Some('E'),
            selected.is_some(),
            no_selection_reason.clone(),
            false,
        ));
        actions.push(user_management_action_model(
            UserManagementAction::SetPassword,
            if self.can_manage_all_users() {
                "Password"
            } else {
                "Change password"
            },
            Some('R'),
            selected.is_some(),
            no_selection_reason.clone(),
            false,
        ));

        if self.can_manage_all_users() {
            let locked = selected.is_some_and(user_is_locked);
            let enabled = selected.is_some_and(|user| user.enabled);
            let (toggle_label, toggle_shortcut, disabling) = if !enabled {
                ("Enable", Some('U'), false)
            } else if locked {
                ("Unlock", Some('U'), false)
            } else {
                ("Disable", Some('D'), true)
            };
            actions.push(user_management_action_model(
                UserManagementAction::ToggleEnabled,
                toggle_label,
                toggle_shortcut,
                selected.is_some() && !(disabling && last_enabled_admin),
                no_selection_reason.clone().or_else(|| {
                    (disabling && last_enabled_admin)
                        .then(|| protected_reason.clone())
                        .flatten()
                }),
                disabling,
            ));

            let demoting = selected.is_some_and(|user| user.role == UserRole::Admin);
            actions.push(user_management_action_model(
                UserManagementAction::ToggleRole,
                if demoting { "Make user" } else { "Make admin" },
                Some('C'),
                selected.is_some() && !(demoting && last_enabled_admin),
                no_selection_reason.clone().or_else(|| {
                    (demoting && last_enabled_admin)
                        .then(|| protected_reason.clone())
                        .flatten()
                }),
                demoting,
            ));
        }

        actions.push(user_management_action_model(
            UserManagementAction::Delete,
            if self.can_manage_all_users() {
                "Delete"
            } else {
                "Delete account"
            },
            Some('X'),
            selected.is_some() && !last_enabled_admin,
            no_selection_reason.or(protected_reason),
            true,
        ));
        actions.push(user_management_action_model(
            UserManagementAction::Back,
            "Back",
            None,
            true,
            None,
            false,
        ));
        actions
    }

    fn user_management_form_view_model(&self) -> Option<ui::UserManagementFormViewModel> {
        match &self.user_management_mode {
            UserManagementMode::Browse => None,
            UserManagementMode::Create(form) => Some(ui::UserManagementFormViewModel {
                kind: ui::UserManagementFormKind::Create,
                title: "Create user".to_string(),
                username: form.username.clone(),
                display_name: form.display_name.clone(),
                role: form.role.as_str().to_string(),
                password_len: form.password.chars().count(),
                focused_field: to_ui_user_management_field(form.focused_field),
                error: self.user_management_form_error(),
            }),
            UserManagementMode::EditInfo(form) => Some(ui::UserManagementFormViewModel {
                kind: ui::UserManagementFormKind::EditInfo,
                title: "Edit user info".to_string(),
                username: form.username.clone(),
                display_name: form.display_name.clone(),
                role: String::new(),
                password_len: 0,
                focused_field: to_ui_user_management_field(form.focused_field),
                error: self.user_management_form_error(),
            }),
            UserManagementMode::Password(form) => Some(ui::UserManagementFormViewModel {
                kind: ui::UserManagementFormKind::Password,
                title: "Set password".to_string(),
                username: form.username.clone(),
                display_name: String::new(),
                role: String::new(),
                password_len: form.password.chars().count(),
                focused_field: to_ui_user_management_field(form.focused_field),
                error: self.user_management_form_error(),
            }),
        }
    }

    fn user_management_form_error(&self) -> Option<String> {
        (self.user_management_feedback_tone == UserManagementFeedbackTone::Error)
            .then(|| self.user_management_message.clone())
            .flatten()
    }

    pub fn to_shell_chrome_view_model(&self) -> ui::ShellChromeViewModel {
        let status = if self.home_mode == ShellHomeMode::Debug {
            let mouse_position = self
                .mouse_coordinates
                .map(|(x, y)| format!("{x},{y}"))
                .unwrap_or_else(|| "none".to_string());
            format!(
                "{} | Last Key: {} | Mouse position: {} | Size: {}x{} | Scroll: {} | Drag: {}",
                self.status(),
                self.last_key_event.as_deref().unwrap_or("none"),
                mouse_position,
                self.terminal_size.0,
                self.terminal_size.1,
                self.mouse_scroll_direction.as_deref().unwrap_or("none"),
                self.mouse_drag_direction.as_deref().unwrap_or("none")
            )
        } else {
            self.status().to_string()
        };
        ui::ShellChromeViewModel {
            app_name: "TundraUX 3".to_string(),
            build_mode: build_mode_label().to_string(),
            display_mode: self.home_display_mode(),
            terminal_size: self.terminal_size,
            screen_stack: self
                .screen_stack
                .iter()
                .map(|screen| format!("{screen:?}"))
                .collect(),
            status: ui::StatusViewModel {
                status,
                toast: self.app.notification_center().toast().map(str::to_owned),
                error: self.app.notification_center().alert().map(str::to_owned),
                alert_tone: self
                    .app
                    .notification_center()
                    .alert_tone()
                    .unwrap_or(ui::NotificationTone::Info),
                time_button_label: self.status_time_button_label(),
                time_button_selected: self.time_button_selected(),
            },
        }
    }
}
