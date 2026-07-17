impl ShellState {
    pub fn new(launch_config: ShellLaunchConfig, terminal_size: (u16, u16)) -> Self {
        Self::new_with_startup(
            launch_config,
            terminal_size,
            ShellStartupState::current_process_defaults(),
        )
    }

    pub fn try_new(
        launch_config: ShellLaunchConfig,
        terminal_size: (u16, u16),
    ) -> Result<Self, tundra_ui::AssetError> {
        Self::try_new_with_startup(
            launch_config,
            terminal_size,
            ShellStartupState::current_process_defaults(),
        )
    }

    pub fn new_with_startup(
        launch_config: ShellLaunchConfig,
        terminal_size: (u16, u16),
        startup: ShellStartupState,
    ) -> Self {
        let ascii_assets =
            tundra_ui::RuntimeAsciiAssets::load_default().expect("default ASCII assets must load");
        Self::new_with_startup_and_assets(launch_config, terminal_size, startup, ascii_assets)
    }

    pub fn try_new_with_startup(
        launch_config: ShellLaunchConfig,
        terminal_size: (u16, u16),
        startup: ShellStartupState,
    ) -> Result<Self, tundra_ui::AssetError> {
        let ascii_assets = tundra_ui::RuntimeAsciiAssets::load_default()?;
        Ok(Self::new_with_startup_and_assets(
            launch_config,
            terminal_size,
            startup,
            ascii_assets,
        ))
    }

    pub fn new_with_startup_and_assets(
        launch_config: ShellLaunchConfig,
        terminal_size: (u16, u16),
        startup: ShellStartupState,
        ascii_assets: tundra_ui::RuntimeAsciiAssets,
    ) -> Self {
        let explorer_task_runtime = startup
            .storage_manager
            .as_ref()
            .map(|storage| ShellExplorerTaskRuntime::new(storage.clone()));
        let diagnostics_task_runtime = startup
            .storage_manager
            .as_ref()
            .map(|storage| ShellDiagnosticsTaskRuntime::new(storage.clone()));
        let editor_task_runtime = ShellEditorTaskRuntime::new();
        Self::new_with_runtime_services(
            launch_config,
            terminal_size,
            startup,
            ascii_assets,
            explorer_task_runtime,
            diagnostics_task_runtime,
            editor_task_runtime,
        )
    }

    pub(crate) fn new_with_runtime_services(
        launch_config: ShellLaunchConfig,
        terminal_size: (u16, u16),
        startup: ShellStartupState,
        ascii_assets: tundra_ui::RuntimeAsciiAssets,
        explorer_task_runtime: Option<ShellExplorerTaskRuntime>,
        diagnostics_task_runtime: Option<ShellDiagnosticsTaskRuntime>,
        editor_task_runtime: ShellEditorTaskRuntime,
    ) -> Self {
        let diagnostics_restart_required = diagnostics_task_runtime
            .as_ref()
            .is_some_and(ShellDiagnosticsTaskRuntime::restart_required);
        let home_mode = resolved_home_mode(launch_config, &startup);
        let auth_gate_enabled = startup.storage_manager.is_some();
        let initial_screen = if auth_gate_enabled {
            if startup.auth_bootstrap_required {
                ShellScreen::FirstRunSetup
            } else {
                ShellScreen::Login
            }
        } else {
            match launch_config.launch_target {
                ShellLaunchTarget::Home => ShellScreen::Home,
                ShellLaunchTarget::Editor => ShellScreen::Editor,
            }
        };
        let initial_focus = match initial_screen {
            ShellScreen::FirstRunSetup => ShellComponent::SetupLanguage,
            ShellScreen::BootstrapAdmin => ShellComponent::BootstrapUsername,
            ShellScreen::Login => ShellComponent::LoginUserList,
            ShellScreen::Editor => ShellComponent::Editor,
            _ => ShellComponent::Home,
        };
        let login_users = startup.login_users.clone();
        let login_selected_user = default_login_user_index(&login_users);
        let login_username = login_users
            .get(login_selected_user)
            .map(|user| user.username.clone())
            .unwrap_or_default();
        let clock_timezone_id = startup_clock_timezone_id(&startup);
        let network_clock = ShellNetworkClock::new(clock_timezone_id.clone());
        let mut notifications = NotificationCenter::new("Ready");
        if startup.storage_report.has_recovery_warnings() {
            notifications.notify_toast("Storage recovered defaults");
        }

        let created_at = Instant::now();
        let mut state = Self {
            home_mode,
            launch_target: launch_config.launch_target,
            ascii_assets,
            screen_stack: vec![initial_screen],
            storage_manager: startup.storage_manager.clone(),
            network_clock,
            clock_timezone_id,
            last_time_sync_utc: None,
            clock_scheduler: None,
            clock_selected_entry_id: None,
            clock_entry_window_start: 0,
            clock_create_state: None,
            clock_persist_pending: false,
            clock_pending_due_summary: None,
            clock_profile_pending_sync: None,
            time_sync_attempted: false,
            time_sync_dialog_visible: false,
            time_sync_failure_message: None,
            auth_session: None,
            requested_debug_mode: launch_config.home_mode_override == HomeModeOverride::Debug,
            debug_policy: startup.debug_policy,
            login_users,
            login_selected_user,
            login_user_window_start: 0,
            login_username,
            login_password: String::new(),
            login_idle_deadline: created_at + LOGIN_IDLE_TIMEOUT,
            login_password_visible_until: None,
            setup_step: tundra_ui::SetupStep::Language,
            setup_selected_language_index: 0,
            setup_selected_timezone_index: 0,
            setup_admin_username: String::new(),
            setup_admin_password: String::new(),
            setup_admin_password_confirm: String::new(),
            setup_admin_password_hint: String::new(),
            setup_focused_field: tundra_ui::SetupField::LanguageList,
            setup_timezone_window_start: 0,
            bootstrap_username: String::new(),
            bootstrap_password: String::new(),
            user_management_users: Vec::new(),
            user_management_selected: 0,
            user_management_window_start: 0,
            user_management_focus: UserManagementPageFocus::UserList,
            user_management_message: None,
            user_management_feedback_tone: UserManagementFeedbackTone::Info,
            user_management_mode: UserManagementMode::Browse,
            selected_home_entry_index: 0,
            explorer_state: None,
            explorer_input_mode: ExplorerInputMode::Browse,
            explorer_input: String::new(),
            explorer_input_replace_all: false,
            explorer_overlay_mode: None,
            explorer_overlay_selection: 0,
            explorer_conflict_apply_to_remaining: false,
            explorer_purpose: ExplorerPurpose::Browse,
            explorer_task_runtime,
            editor_task_runtime,
            editor_load_state: None,
            editor_save_state: None,
            editor_document_generation: 0,
            editor_state: None,
            editor_rich_render_cache: None,
            editor_focus: tundra_ui::EditorFocus::Canvas,
            editor_open_menu: None,
            editor_selected_toolbar_action: None,
            editor_quick_menu_anchor: None,
            editor_drag_anchor: None,
            editor_table_column_widths: std::collections::BTreeMap::new(),
            editor_table_resize: None,
            editor_fingerprint: None,
            editor_close_after_save: false,
            editor_open_after_save: false,
            editor_discard_for_open: false,
            editor_message: None,
            editor_recovery_dirty_since: None,
            editor_last_recovery_write: None,
            editor_read_session: None,
            diagnostics_task_runtime,
            diagnostics_snapshot: None,
            diagnostics_tab: tundra_ui::DiagnosticsTab::Health,
            diagnostics_selected_check: 0,
            diagnostics_selected_log: 0,
            diagnostics_selected_incident: 0,
            diagnostics_list_window_start: 0,
            diagnostics_list_window_is_explicit: false,
            diagnostics_scanning: false,
            diagnostics_rescan_pending: false,
            diagnostics_repair_preview: Vec::new(),
            diagnostics_repair_selected: 0,
            diagnostics_repair_scroll_offset: 0,
            diagnostics_repair_confirm_selected: true,
            diagnostics_feedback: None,
            diagnostics_restart_required,
            terminal_size,
            terminal_flags: ShellTerminalFlags::enabled(),
            focused_component: initial_focus,
            hovered_component: None,
            active_popup: None,
            hit_map: ShellHitMap::empty(terminal_size),
            hit_map_generation: 0,
            tick_count: 0,
            notifications,
            modal_focus_context: None,
            modal_focus_prepared_for_follow_up: false,
            notification_pointer_capture: None,
            pending_notification_commands: VecDeque::new(),
            error_message: None,
            latest_watchdog_report: None,
            latest_watchdog_summary: None,
            shutdown_requested: false,
            return_to_lockscreen_requested: false,
            last_command: None,
            last_routed_target: None,
            last_key_event: None,
            last_mouse_event: None,
            last_resize_event: None,
            mouse_coordinates: None,
            mouse_scroll_direction: None,
            mouse_drag_direction: None,
            platform_capability_summary: platform_capability_summary(
                startup.platform_kind,
                &startup.platform_capabilities,
            ),
            last_click: None,
            drag_tracker: None,
            scrollbar_drag: None,
        };
        state.refresh_hit_map();
        if !auth_gate_enabled {
            match launch_config.launch_target {
                ShellLaunchTarget::Home => {
                    if let Some(restored_session) = startup.restored_session.as_ref() {
                        state.apply_restored_session(restored_session);
                    }
                }
                ShellLaunchTarget::Editor => state.open_editor(),
            }
        }
        state
    }

    pub fn sanitized_session_state(&self) -> ShellRestoredSession {
        let focused_component = if self.focus_order().contains(&self.focused_component) {
            self.focused_component
        } else {
            self.focus_order()
                .first()
                .copied()
                .unwrap_or(ShellComponent::Home)
        };

        ShellRestoredSession {
            active_screen: ShellScreen::Home,
            focused_component,
            display_mode: self.home_mode,
            active_popup: None,
        }
    }

    fn legacy_default_home_mode(launch_config: ShellLaunchConfig) -> ShellHomeMode {
        match launch_config.home_mode_override {
            HomeModeOverride::Debug => ShellHomeMode::Debug,
            HomeModeOverride::BuildDefault => {
                if cfg!(debug_assertions) {
                    ShellHomeMode::Debug
                } else {
                    ShellHomeMode::User
                }
            }
        }
    }

    pub fn new_for_home_mode(
        launch_config: ShellLaunchConfig,
        terminal_size: (u16, u16),
        home_mode: ShellHomeMode,
    ) -> Self {
        let mut state = Self::new(launch_config, terminal_size);
        state.home_mode = home_mode;
        state
    }
}
