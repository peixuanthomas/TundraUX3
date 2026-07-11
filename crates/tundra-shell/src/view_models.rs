impl ShellState {
    pub fn to_home_view_model(&self) -> tundra_ui::HomeViewModel {
        let model = match self.home_mode {
            ShellHomeMode::Debug => {
                tundra_ui::HomeViewModel::debug(tundra_ui::DebugDiagnosticsViewModel {
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
            ShellHomeMode::User => {
                let user = self.current_home_username().unwrap_or("Unauthenticated");
                tundra_ui::HomeViewModel::user_with_selection_and_icon_assets(
                    user,
                    self.current_time_label(),
                    self.user_home_entries(),
                    self.selected_home_entry_index(),
                    self.ascii_assets.clone(),
                )
            }
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

        let entries = state
            .entries
            .iter()
            .enumerate()
            .map(|(index, entry)| tundra_ui::ExplorerEntryViewModel {
                name: entry.name.clone(),
                kind: entry.kind.label().to_string(),
                size: (entry.kind == tundra_apps::explorer::ExplorerEntryKind::File)
                    .then(|| entry.size.to_string()),
                modified: entry.modified.map(system_time_label),
                attributes: explorer_attribute_labels(&entry.attributes),
                selected: index == state.selected_index,
            })
            .collect::<Vec<_>>();
        let selected_index = (!entries.is_empty()).then_some(state.selected_index);
        let mut model = tundra_ui::ExplorerViewModel::new(
            state.current_path.display().to_string(),
            entries,
            selected_index,
        );
        model.show_hidden = state.show_hidden;
        model.message = state.message.clone();
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
            tundra_ui::ExplorerDialogViewModel::new(
                dialog.title.clone(),
                dialog.message.clone(),
                "Y / Enter: move",
                "N / Esc: cancel",
            )
        });

        if self.explorer_input_mode != ExplorerInputMode::Browse
            && self.explorer_input_mode != ExplorerInputMode::Search
        {
            model.message = Some(format!(
                "{}: {}",
                explorer_input_prompt(self.explorer_input_mode),
                self.explorer_input
            ));
        }

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
        let status = if self.home_mode == ShellHomeMode::Debug
            && self.active_screen() == ShellScreen::Home
        {
            format!(
                "{} | Key: {} | Mouse: {} | Resize: {}",
                self.notifications.status(),
                self.last_key_event.as_deref().unwrap_or("none"),
                self.last_mouse_event.as_deref().unwrap_or("none"),
                self.last_resize_event.as_deref().unwrap_or("none")
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
