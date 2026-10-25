use super::*;
impl ShellSession {
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
    ) -> Result<Self, ui::AssetError> {
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
            ui::RuntimeAsciiAssets::load_default().expect("default ASCII assets must load");
        Self::new_with_startup_and_assets(launch_config, terminal_size, startup, ascii_assets)
    }

    pub fn try_new_with_startup(
        launch_config: ShellLaunchConfig,
        terminal_size: (u16, u16),
        startup: ShellStartupState,
    ) -> Result<Self, ui::AssetError> {
        let ascii_assets = ui::RuntimeAsciiAssets::load_default()?;
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
        ascii_assets: ui::RuntimeAsciiAssets,
    ) -> Self {
        let explorer_task_runtime = startup
            .storage_manager
            .as_ref()
            .map(|storage| ShellExplorerTaskRuntime::new(storage.clone()));
        let diagnostics_task_runtime = startup
            .storage_manager
            .as_ref()
            .map(|storage| ShellDiagnosticsTaskRuntime::new(storage.clone()));
        let task_watchdog = default_editor_watchdog();
        let editor_task_runtime = task_watchdog.clone().map_or_else(
            ShellEditorTaskRuntime::unavailable,
            ShellEditorTaskRuntime::new_managed,
        );
        let settings_task_runtime = task_watchdog.map_or_else(
            ShellSettingsTaskRuntime::unavailable,
            ShellSettingsTaskRuntime::new_managed,
        );
        Self::new_with_runtime_services(
            launch_config,
            terminal_size,
            startup,
            ascii_assets,
            ShellRuntimeServices {
                language: None,
                explorer: explorer_task_runtime,
                diagnostics: diagnostics_task_runtime,
                editor: editor_task_runtime,
                settings: settings_task_runtime,
            },
        )
    }

    pub(in crate::session) fn new_with_runtime_services(
        launch_config: ShellLaunchConfig,
        terminal_size: (u16, u16),
        startup: ShellStartupState,
        ascii_assets: ui::RuntimeAsciiAssets,
        runtime_services: ShellRuntimeServices,
    ) -> Self {
        let diagnostics_restart_required = runtime_services
            .diagnostics
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
            ShellScreen::Home
        };
        let initial_focus = match initial_screen {
            ShellScreen::FirstRunSetup => ShellComponent::SetupLanguage,
            ShellScreen::BootstrapAdmin => ShellComponent::BootstrapUsername,
            ShellScreen::Login => ShellComponent::LoginUserList,
            ShellScreen::Launcher => ShellComponent::Launcher,
            ShellScreen::CommandLine => ShellComponent::CommandLine,
            ShellScreen::Editor => ShellComponent::Editor,
            ShellScreen::SystemStatus => ShellComponent::SystemStatus,
            _ => ShellComponent::Home,
        };
        let login_users = startup.login_users.clone();
        let login_selected_user = default_login_user_index(&login_users);
        let login_username = login_users
            .get(login_selected_user)
            .map(|user| user.username.clone())
            .unwrap_or_default();
        let mut storage_config = startup
            .storage_manager
            .as_ref()
            .and_then(|storage| storage.load_config().ok())
            .unwrap_or_default();
        if startup.platform_kind == PlatformKind::Linux {
            storage_config.time_sync.source = storage::TimeSyncSource::OperatingSystem;
            storage_config.time_sync.server_url = None;
        }
        let prepared_language = runtime_services.language.unwrap_or_else(|| {
            PreparedLanguage::load(ascii_assets.store().root(), &storage_config.language)
        });
        let language_catalog = prepared_language.catalog;
        let language = prepared_language.snapshot;
        let _language = i18n::enter_snapshot(language.clone());
        let mut app = app::AppState::with_storage_config(storage_config);
        if startup.platform_kind == PlatformKind::Linux {
            app.dispatch_at(
                app::AppCommand::SetClockTimezone {
                    timezone_id: None,
                    synchronized_utc: None,
                },
                Instant::now(),
            );
        }
        if startup.storage_report.has_recovery_warnings() {
            app.dispatch_at(
                app::AppCommand::Notification(app::NotificationCommand::ShowToast(
                    i18n::msg!("startup-storage-recovered").into(),
                )),
                Instant::now(),
            );
        }

        let ui = UiSessionState {
            pages: ShellPages::default(),
            command_line_close_request: None,
            script_requests: VecDeque::new(),
            editor_vim_terminal: None,
            editor_vim_install: None,
            editor_vim_toggle_requested: false,
            editor_vim_script_export: None,
            editor_vim_close_requested: false,
            language,
            language_catalog,
            repaired_resource_paths: Vec::new(),
            fallback_resource_paths: Vec::new(),
            home_mode,
            ascii_assets,
            navigation: ShellNavigation::new(initial_screen),
            storage_manager: startup.storage_manager.clone(),
            last_time_sync_utc: None,
            clock_scheduler: None,

            clock_persist_pending: false,
            clock_pending_due_summary: None,
            clock_profile_pending_sync: None,
            time_sync_attempted: false,
            time_sync_dialog_visible: false,
            help_open: false,
            help_scroll: 0,
            help_scrollbar_drag: None,
            time_sync_failure_message: None,
            debug_home_after_login: launch_config.home_mode_override == HomeModeOverride::Debug,
            debug_policy: startup.debug_policy,
            identity_backend: startup.identity_backend,
            login_users,

            pending_personalization_session: None,

            system_settings_backend: app::system_settings::UnavailableSystemSettingsBackend {
                reason: if startup.platform_kind == PlatformKind::Linux {
                    app::system_settings::UnavailableReason::NotIntegrated
                } else {
                    app::system_settings::UnavailableReason::UnsupportedPlatform
                },
            },
            tasks: ShellTasks::default(),
            pending_page_closes: HashSet::new(),
            pending_login_reset: None,
            pending_lockscreen_after_logout: false,
            pending_session_exit: None,
            approved_editor_closes: BTreeMap::new(),
            settings_update_request: None,
            settings_task_runtime: runtime_services.settings,
            settings_update_state: SettingsUpdateState::default(),
            update_apply_manifest: None,
            update_restart_requested: false,

            system_status_history: SystemStatusMetricHistory::default(),

            system_status_storage_alerts: BTreeMap::new(),
            system_status_network_baseline: None,
            system_status_disconnected_notified: false,

            launcher_task_runtime: None,

            explorer_task_runtime: runtime_services.explorer,
            editor_task_runtime: runtime_services.editor,

            diagnostics_task_runtime: runtime_services.diagnostics,

            auto_admin: AutoAdminState::default(),
            screen_keyboard: Default::default(),
            details_motion: None,
            #[cfg(target_os = "linux")]
            privilege_session: Default::default(),
            detached_management_operations: Vec::new(),
            detached_user_management_jobs: Vec::new(),

            diagnostics_restart_required,
            terminal_size,
            terminal_flags: ShellTerminalFlags::enabled(),
            terminal_image_support: false,
            terminal_text_sizing_support: false,
            pending_default_ascii_icon_fallback: false,

            interaction: Default::default(),
            screen_saver: Default::default(),
            button_regions: Vec::new(),
            notification_scrollbar_drag: None,

            hovered_component: None,

            frame_layout: None,
            displayed_status: None,
            hit_map: ShellHitMap::empty(terminal_size),
            hit_map_generation: 0,
            overlay_interaction_ready: true,
            tick_count: 0,
            notification_bindings: NotificationBindings::default(),
            notification_pointer_capture: None,
            pending_notification_commands: VecDeque::new(),

            latest_watchdog_report: None,
            latest_watchdog_summary: None,
            shutdown_requested: false,
            restart_requested: false,
            return_to_lockscreen_requested: false,
            last_command: None,
            last_routed_target: None,
            last_key_event: None,
            last_mouse_event: None,
            last_resize_event: None,
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
        let mut state = Self { app, ui };
        state.initialize_root_page();
        state.pages.common.focused_component = initial_focus;
        state.pages.login.login_selected_user = login_selected_user;
        state.pages.login.login_username = login_username;
        state.pages.setup.setup_selected_language_index = state
            .language_catalog
            .options()
            .iter()
            .position(|option| option.code == state.language.code())
            .unwrap_or(0);
        state.report_language_diagnostics(&prepared_language.diagnostics);
        if let Some(target) = std::env::var_os(app::update::UPDATE_TARGET_SHA_ENV) {
            let target = target.to_string_lossy();
            state.notify_toast(i18n::msg!(
                "startup-updated",
                revision = target.chars().take(7).collect::<String>()
            ));
        }
        if let Some(reason) = std::env::var_os(app::update::UPDATE_ROLLBACK_ENV) {
            state.notify_alert_with_tone(
                i18n::msg!(
                    "startup-update-restored",
                    reason = reason.to_string_lossy().to_string()
                ),
                ui::NotificationTone::Error,
            );
        }
        #[cfg(target_os = "linux")]
        if state.identity_backend == identity::IdentityBackend::Linux {
            if let Some(storage) = state.storage_manager.clone() {
                match SessionService::new(storage)
                    .with_backend(identity::IdentityBackend::Linux)
                    .attach_current_linux_user()
                {
                    Ok(session) => state.complete_login(session),
                    Err(error) => {
                        state.pages.common.error_message = Some(format_core_error(&error));
                        state.shutdown_requested = true;
                    }
                }
            }
        }
        state.refresh_hit_map();
        if !auth_gate_enabled && let Some(restored_session) = startup.restored_session.as_ref() {
            state.apply_restored_session(restored_session);
        }
        state
    }

    pub fn sanitized_session_state(&self) -> ShellRestoredSession {
        let (focus_manager, focus_order) =
            self.focus_manager(Some(self.pages.common.focused_component));
        let focused_component = self
            .focused_component_from_manager(&focus_manager, &focus_order)
            .unwrap_or(ShellComponent::Home);

        ShellRestoredSession {
            active_screen: ShellScreen::Home,
            focused_component,
            display_mode: self.home_mode,
            active_popup: None,
        }
    }

    pub(in crate::session) fn legacy_default_home_mode(
        launch_config: ShellLaunchConfig,
    ) -> ShellHomeMode {
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
