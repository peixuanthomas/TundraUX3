impl ShellState {
    pub fn to_home_view_model(&self) -> tundra_ui::HomeViewModel {
        let user = self.current_home_username().unwrap_or("Unauthenticated");
        let model = tundra_ui::HomeViewModel::user_with_selection_and_icon_assets(
            user,
            self.current_time_label(),
            self.user_home_entries(),
            self.selected_home_entry_index(),
            self.ascii_assets.clone(),
        );
        let model = match self.home_mode {
            ShellHomeMode::Debug => {
                model.with_debug_diagnostics(tundra_ui::DebugDiagnosticsViewModel {
                    tick_count: self.tick_count,
                    last_key_event: self.last_key_event.clone(),
                    last_mouse_event: self.last_mouse_event.clone(),
                    last_resize_event: self.last_resize_event.clone(),
                    mouse_coordinates: self.mouse_coordinates,
                    scroll_direction: self.mouse_scroll_direction.clone(),
                    drag_direction: self.mouse_drag_direction.clone(),
                    terminal_flags: terminal_flag_labels(self.terminal_flags),
                    platform_capability_summary: self.platform_capability_summary.clone(),
                })
            }
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

    pub fn to_diagnostics_view_model(&self) -> tundra_ui::DiagnosticsViewModel {
        let can_view_details = self.diagnostics_can_view_details();
        let can_repair = self.diagnostics_can_repair();
        let (checks, logs, incidents, scanned_at) = self
            .diagnostics_snapshot
            .as_ref()
            .map(|snapshot| {
                let checks = snapshot
                    .checks
                    .iter()
                    .map(|check| tundra_ui::DiagnosticsCheckViewModel {
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
                        tundra_ui::DiagnosticsIncidentViewModel {
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
                        .map(|log| tundra_ui::DiagnosticsLogViewModel {
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
            tundra_ui::DiagnosticsRepairDialogViewModel {
                items: self
                    .diagnostics_repair_preview
                    .iter()
                    .enumerate()
                    .map(
                        |(index, action)| tundra_ui::DiagnosticsRepairItemViewModel {
                            id: index.to_string(),
                            label: action.label(),
                        },
                    )
                    .collect(),
                selected: self.diagnostics_repair_selected,
                confirm_selected: self.diagnostics_repair_confirm_selected,
                scroll_offset: self.diagnostics_repair_scroll_offset,
            }
        });

        tundra_ui::DiagnosticsViewModel {
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

    fn current_home_username(&self) -> Option<&str> {
        self.auth_session
            .as_ref()
            .map(|session| session.username.as_str())
    }

    pub fn to_clock_view_model(&self) -> tundra_ui::ClockViewModel {
        let snapshot = self.network_clock.snapshot();
        self.to_clock_view_model_at(&snapshot, Instant::now())
    }

    fn to_clock_view_model_at(
        &self,
        snapshot: &tundra_weathr::network_clock::ClockSnapshot,
        now: Instant,
    ) -> tundra_ui::ClockViewModel {
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
                let view = tundra_ui::ClockEntryViewModel::new(entry.id, label, entry.strong);
                match entry.kind {
                    ScheduledClockEntryKind::DailyAlarm => alarms.push(view),
                    ScheduledClockEntryKind::Countdown => countdowns.push(view),
                }
            }
        }

        let mut model = tundra_ui::ClockViewModel::at(
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
                .map(|state| tundra_ui::ClockCreateDialogViewModel {
                    input: state.input.clone(),
                    error: state.error.clone(),
                    focus: state.focus,
                });
        model
    }

    pub fn to_time_sync_dialog_view_model(&self) -> Option<tundra_ui::TimeSyncDialogViewModel> {
        self.time_sync_dialog_visible
            .then(tundra_ui::TimeSyncDialogViewModel::new)
    }

    pub fn to_login_view_model(&self) -> tundra_ui::LoginViewModel {
        self.to_login_view_model_at(Instant::now())
    }

    pub fn to_login_view_model_at(&self, now: Instant) -> tundra_ui::LoginViewModel {
        let model = tundra_ui::LoginViewModel::new(
            self.login_users
                .iter()
                .map(|user| tundra_ui::LoginUserOptionViewModel {
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
                ShellComponent::LoginPassword => tundra_ui::LoginField::Password,
                ShellComponent::LoginPasswordVisibility => {
                    tundra_ui::LoginField::PasswordVisibility
                }
                _ => tundra_ui::LoginField::UserList,
            },
            self.error_message.clone(),
        );
        if self.login_password_is_visible_at(now) {
            model.with_visible_password(self.login_password.clone())
        } else {
            model
        }
    }

    pub fn to_bootstrap_admin_view_model(&self) -> tundra_ui::BootstrapAdminViewModel {
        tundra_ui::BootstrapAdminViewModel::new(
            self.bootstrap_username.clone(),
            self.bootstrap_password.chars().count(),
            match self.focused_component {
                ShellComponent::BootstrapPassword => tundra_ui::AuthField::Password,
                _ => tundra_ui::AuthField::Username,
            },
            self.error_message.clone(),
        )
    }

    pub fn to_setup_view_model(&self) -> tundra_ui::SetupViewModel {
        let password_requirements = setup_password_requirements(
            &self.setup_admin_username,
            &self.setup_admin_password,
            &self.setup_admin_password_confirm,
        );
        let can_submit = !self.setup_admin_username.trim().is_empty()
            && password_requirements
                .iter()
                .all(|requirement| requirement.met);

        tundra_ui::SetupViewModel {
            step: self.setup_step,
            languages: tundra_ui::setup_language_options(),
            timezones: tundra_ui::setup_timezone_options(),
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
            error: self.error_message.clone(),
        }
    }

    pub fn to_user_management_view_model(&self) -> tundra_ui::UserManagementViewModel {
        let current_user = self
            .auth_session
            .as_ref()
            .map(|session| session.username.clone())
            .unwrap_or_else(|| "Unauthenticated".to_string());
        let mut model = tundra_ui::UserManagementViewModel::new(
            current_user.clone(),
            self.user_management_users
                .iter()
                .map(|user| tundra_ui::UserManagementUserViewModel {
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
            UserManagementPageFocus::UserList => tundra_ui::UserManagementFocus::UserList,
            UserManagementPageFocus::Action(action) => {
                tundra_ui::UserManagementFocus::Action(action)
            }
        };
        model.actions = self.user_management_action_view_models();
        model.feedback_tone = match self.user_management_feedback_tone {
            UserManagementFeedbackTone::Info => tundra_ui::UserManagementFeedbackTone::Info,
            UserManagementFeedbackTone::Success => tundra_ui::UserManagementFeedbackTone::Success,
            UserManagementFeedbackTone::Error => tundra_ui::UserManagementFeedbackTone::Error,
        };
        model
    }

    pub fn to_explorer_view_model(&self) -> tundra_ui::ExplorerViewModel {
        let Some(state) = self.explorer_state.as_ref() else {
            return tundra_ui::ExplorerViewModel::new("Explorer unavailable", Vec::new(), None);
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
            .map(|entry| tundra_ui::ExplorerEntryViewModel {
                name: explorer_display_name(entry, state.show_extensions),
                kind: entry.type_label.clone(),
                size: (entry.kind == tundra_apps::explorer::ExplorerEntryKind::File)
                    .then(|| explorer_size_label(entry.size, state.size_format)),
                modified: entry.modified.map(|modified| {
                    explorer_system_time_label(
                        modified,
                        state.date_zone,
                        self.clock_timezone_id.as_deref(),
                    )
                }),
                attributes: explorer_attribute_labels(&entry.attributes),
                selected: state.is_selected(&entry.path),
            })
            .collect::<Vec<_>>();
        let selected_index = (!entries.is_empty()).then_some(state.selected_index);
        let mut model = tundra_ui::ExplorerViewModel::with_ascii_assets(
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
                let mut presentation = tundra_ui::ExplorerEntryPresentationViewModel::new(
                    entry.path.display().to_string(),
                    entry.path.display().to_string(),
                    entry.icon_key.clone(),
                    entry.kind == tundra_apps::explorer::ExplorerEntryKind::Directory,
                );
                presentation.selected = state.is_selected(&entry.path);
                presentation.focused = index == state.selected_index;
                presentation.cut = state.clipboard.as_ref().is_some_and(|clipboard| {
                    clipboard.mode == tundra_apps::explorer::ExplorerClipboardMode::Cut
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
                let mut model = tundra_ui::ExplorerQuickLocationViewModel::new(
                    location.id.clone(),
                    location.label.clone(),
                    location.path.display().to_string(),
                    location.icon_key.clone(),
                );
                model.kind = match location.kind {
                    tundra_apps::explorer::ExplorerQuickLocationKind::Directory => {
                        tundra_ui::ExplorerQuickLocationKind::Directory
                    }
                    tundra_apps::explorer::ExplorerQuickLocationKind::Volume => {
                        tundra_ui::ExplorerQuickLocationKind::Volume
                    }
                    tundra_apps::explorer::ExplorerQuickLocationKind::Trash => {
                        tundra_ui::ExplorerQuickLocationKind::Trash
                    }
                };
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
            tundra_apps::explorer::ExplorerSortField::Name => tundra_ui::ExplorerSortColumn::Name,
            tundra_apps::explorer::ExplorerSortField::Type => tundra_ui::ExplorerSortColumn::Type,
            tundra_apps::explorer::ExplorerSortField::Size => tundra_ui::ExplorerSortColumn::Size,
            tundra_apps::explorer::ExplorerSortField::Modified => {
                tundra_ui::ExplorerSortColumn::Modified
            }
        };
        model.sort_direction = match state.sort_direction {
            tundra_apps::explorer::ExplorerSortDirection::Ascending => {
                tundra_ui::ExplorerSortDirection::Ascending
            }
            tundra_apps::explorer::ExplorerSortDirection::Descending => {
                tundra_ui::ExplorerSortDirection::Descending
            }
        };
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
            model.toolbar = tundra_ui::ExplorerToolbarViewModel::trash(
                !state.back_history.is_empty(),
                !state.forward_history.is_empty(),
                model.selected_count == 1 && !busy,
                !state.entries.is_empty() && !busy,
            );
        }
        for button in &mut model.toolbar.buttons {
            button.enabled = match button.action {
                tundra_ui::ExplorerToolbarAction::Back => !state.back_history.is_empty(),
                tundra_ui::ExplorerToolbarAction::Forward => !state.forward_history.is_empty(),
                tundra_ui::ExplorerToolbarAction::Up => {
                    !is_trash && state.current_path.parent().is_some()
                }
                tundra_ui::ExplorerToolbarAction::New => !is_trash && !busy,
                tundra_ui::ExplorerToolbarAction::Cut
                | tundra_ui::ExplorerToolbarAction::Copy
                | tundra_ui::ExplorerToolbarAction::Delete => model.selected_count > 0 && !busy,
                tundra_ui::ExplorerToolbarAction::Paste => state.clipboard.is_some() && !busy,
                tundra_ui::ExplorerToolbarAction::Rename => {
                    !is_trash && model.selected_count == 1 && !busy
                }
                tundra_ui::ExplorerToolbarAction::Restore => {
                    is_trash && model.selected_count == 1 && !busy
                }
                tundra_ui::ExplorerToolbarAction::DumpTrash => {
                    is_trash && !state.entries.is_empty() && !busy
                }
                _ => true,
            };
        }
        model.operation = state.operation.as_ref().map(|operation| {
            let phase = match operation.phase {
                tundra_apps::explorer::ExplorerOperationPhase::Scanning => {
                    tundra_ui::ExplorerOperationPhase::Scanning
                }
                tundra_apps::explorer::ExplorerOperationPhase::WaitingForConflict => {
                    tundra_ui::ExplorerOperationPhase::CheckingConflicts
                }
                tundra_apps::explorer::ExplorerOperationPhase::Executing => {
                    if operation.label.to_ascii_lowercase().contains("mov") {
                        tundra_ui::ExplorerOperationPhase::Moving
                    } else if operation.label.to_ascii_lowercase().contains("trash") {
                        tundra_ui::ExplorerOperationPhase::Deleting
                    } else {
                        tundra_ui::ExplorerOperationPhase::Copying
                    }
                }
                tundra_apps::explorer::ExplorerOperationPhase::Completed
                | tundra_apps::explorer::ExplorerOperationPhase::Cancelled
                | tundra_apps::explorer::ExplorerOperationPhase::Failed => {
                    tundra_ui::ExplorerOperationPhase::Finishing
                }
            };
            tundra_ui::ExplorerOperationProgressViewModel {
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
            Some(tundra_ui::ExplorerSearchViewModel::new(
                self.explorer_input.clone(),
                true,
                Some(state.entries.len()),
            ))
        } else if !state.query.is_empty() {
            Some(tundra_ui::ExplorerSearchViewModel::new(
                state.query.clone(),
                false,
                Some(state.entries.len()),
            ))
        } else {
            None
        };
        model.pending_dialog = state.pending_dialog.as_ref().map(|dialog| {
            let (confirm, cancel) = match dialog.kind {
                tundra_apps::explorer::ExplorerDialogKind::DeleteToTrash => {
                    ("Y / Enter: move", "N / Esc: cancel")
                }
                tundra_apps::explorer::ExplorerDialogKind::DumpTrash => {
                    ("Y / Enter: empty permanently", "N / Esc: cancel")
                }
            };
            tundra_ui::ExplorerDialogViewModel::new(
                dialog.title.clone(),
                dialog.message.clone(),
                confirm,
                cancel,
            )
        });

        model.overlay = if let Some(conflict) = state.pending_restore.as_ref() {
            Some(tundra_ui::ExplorerOverlayViewModel::Conflict(
                tundra_ui::ExplorerConflictViewModel {
                    title: "Restore conflict".to_string(),
                    source: format!("Trash: {}", conflict.display_name),
                    destination: conflict.target.display().to_string(),
                    choices: vec![
                        tundra_ui::ExplorerConflictChoice::KeepBoth,
                        tundra_ui::ExplorerConflictChoice::Replace,
                        tundra_ui::ExplorerConflictChoice::Cancel,
                    ],
                    selected_choice: tundra_ui::ExplorerConflictChoice::KeepBoth,
                    apply_to_remaining: false,
                    allow_apply_to_remaining: false,
                },
            ))
        } else if let Some(conflict) = state.pending_conflict.as_ref() {
            Some(tundra_ui::ExplorerOverlayViewModel::Conflict(
                tundra_ui::ExplorerConflictViewModel {
                    title: "Name conflict".to_string(),
                    source: conflict.source.display().to_string(),
                    destination: conflict.target.display().to_string(),
                    choices: vec![
                        tundra_ui::ExplorerConflictChoice::KeepBoth,
                        tundra_ui::ExplorerConflictChoice::Replace,
                        tundra_ui::ExplorerConflictChoice::Skip,
                        tundra_ui::ExplorerConflictChoice::Cancel,
                    ],
                    selected_choice: tundra_ui::ExplorerConflictChoice::KeepBoth,
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
                    tundra_ui::ExplorerNameDialogKind::NewFolder,
                    "New folder",
                    "Folder name",
                    "Create",
                ),
                ExplorerInputMode::NewTextFile => (
                    tundra_ui::ExplorerNameDialogKind::NewTextFile,
                    "New text file",
                    "File name",
                    "Create",
                ),
                ExplorerInputMode::Rename => (
                    tundra_ui::ExplorerNameDialogKind::Rename,
                    "Rename",
                    "New name",
                    "Rename",
                ),
                ExplorerInputMode::RestoreDestination => (
                    tundra_ui::ExplorerNameDialogKind::RestoreDestination,
                    "Restore item",
                    "Absolute destination directory",
                    "Restore",
                ),
                ExplorerInputMode::Browse
                | ExplorerInputMode::Address
                | ExplorerInputMode::Search => unreachable!(),
            };
            Some(tundra_ui::ExplorerOverlayViewModel::Name(
                tundra_ui::ExplorerNameDialogViewModel {
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
                ExplorerOverlayMode::Options => {
                    explorer_options_view_model(state, self.explorer_overlay_selection)
                }
                ExplorerOverlayMode::Properties => {
                    explorer_properties_view_model(state, self.clock_timezone_id.as_deref())
                }
            })
        } else {
            None
        };

        model
    }

    fn can_manage_all_users(&self) -> bool {
        matches!(
            self.auth_session.as_ref().map(|session| session.role),
            Some(UserRole::Admin)
        )
    }

    fn user_management_action_view_models(&self) -> Vec<tundra_ui::UserManagementActionViewModel> {
        use tundra_ui::UserManagementAction;

        let selected = self
            .user_management_users
            .get(self.user_management_selected);
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

    fn user_management_form_view_model(&self) -> Option<tundra_ui::UserManagementFormViewModel> {
        match &self.user_management_mode {
            UserManagementMode::Browse => None,
            UserManagementMode::Create(form) => Some(tundra_ui::UserManagementFormViewModel {
                kind: tundra_ui::UserManagementFormKind::Create,
                title: "Create user".to_string(),
                username: form.username.clone(),
                display_name: form.display_name.clone(),
                role: form.role.as_str().to_string(),
                password_len: form.password.chars().count(),
                focused_field: to_ui_user_management_field(form.focused_field),
                error: self.user_management_form_error(),
            }),
            UserManagementMode::EditInfo(form) => Some(tundra_ui::UserManagementFormViewModel {
                kind: tundra_ui::UserManagementFormKind::EditInfo,
                title: "Edit user info".to_string(),
                username: form.username.clone(),
                display_name: form.display_name.clone(),
                role: String::new(),
                password_len: 0,
                focused_field: to_ui_user_management_field(form.focused_field),
                error: self.user_management_form_error(),
            }),
            UserManagementMode::Password(form) => Some(tundra_ui::UserManagementFormViewModel {
                kind: tundra_ui::UserManagementFormKind::Password,
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

    pub fn to_shell_chrome_view_model(&self) -> tundra_ui::ShellChromeViewModel {
        let status = if self.home_mode == ShellHomeMode::Debug {
            let mouse_position = self
                .mouse_coordinates
                .map(|(x, y)| format!("{x},{y}"))
                .unwrap_or_else(|| "none".to_string());
            format!(
                "{} | Last Key: {} | Mouse position: {} | Size: {}x{} | Scroll: {} | Drag: {}",
                self.notifications.status(),
                self.last_key_event.as_deref().unwrap_or("none"),
                mouse_position,
                self.terminal_size.0,
                self.terminal_size.1,
                self.mouse_scroll_direction.as_deref().unwrap_or("none"),
                self.mouse_drag_direction.as_deref().unwrap_or("none")
            )
        } else {
            self.notifications.status().to_string()
        };
        tundra_ui::ShellChromeViewModel {
            app_name: "TundraUX 3".to_string(),
            build_mode: build_mode_label().to_string(),
            display_mode: self.home_display_mode(),
            terminal_size: self.terminal_size,
            screen_stack: self
                .screen_stack
                .iter()
                .map(|screen| format!("{screen:?}"))
                .collect(),
            status: tundra_ui::StatusViewModel {
                status,
                toast: self.notifications.toast(),
                error: self.notifications.alert(),
                alert_tone: self
                    .notifications
                    .alert_tone()
                    .unwrap_or(tundra_ui::NotificationTone::Info),
                time_button_label: self.status_time_button_label(),
                time_button_selected: self.time_button_selected(),
            },
        }
    }
}
