use super::super::*;
#[derive(Clone)]
pub(in crate::session) struct ShellDiagnosticsTaskRuntime {
    pub(in crate::session) shared: Arc<ShellDiagnosticsTaskShared>,
}

pub(in crate::session) struct ShellDiagnosticsTaskShared {
    pub(in crate::session) engine: Mutex<Option<app::diagnostics::DiagnosticsTaskRuntime>>,
    pub(in crate::session) terminal_graphics: Mutex<Option<ui::TerminalGraphicsProbeStatus>>,
    pub(in crate::session) storage: StorageManager,
    pub(in crate::session) process: Option<ProcessWatchdog>,
    pub(in crate::session) watchdog: Option<AppWatchdog>,
}

impl std::fmt::Debug for ShellDiagnosticsTaskRuntime {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ShellDiagnosticsTaskRuntime")
            .finish_non_exhaustive()
    }
}

impl PartialEq for ShellDiagnosticsTaskRuntime {
    fn eq(&self, _other: &Self) -> bool {
        true
    }
}

impl Eq for ShellDiagnosticsTaskRuntime {}

impl ShellDiagnosticsTaskRuntime {
    pub(in crate::session) fn new(storage: StorageManager) -> Self {
        let process = ProcessWatchdog::global();
        let watchdog = process.as_ref().and_then(|process| {
            process
                .register_app(app::diagnostics::diagnostics_watchdog_descriptor())
                .ok()
        });
        Self::with_services(storage, process, watchdog)
    }

    pub(in crate::session) fn new_managed(
        storage: StorageManager,
        process: ProcessWatchdog,
        watchdog: AppWatchdog,
    ) -> Self {
        Self::with_services(storage, Some(process), Some(watchdog))
    }

    pub(in crate::session) fn with_services(
        storage: StorageManager,
        process: Option<ProcessWatchdog>,
        watchdog: Option<AppWatchdog>,
    ) -> Self {
        Self {
            shared: Arc::new(ShellDiagnosticsTaskShared {
                engine: Mutex::new(None),
                terminal_graphics: Mutex::new(None),
                storage,
                process,
                watchdog,
            }),
        }
    }

    pub(in crate::session) fn ensure_engine(
        &self,
    ) -> Result<std::sync::MutexGuard<'_, Option<app::diagnostics::DiagnosticsTaskRuntime>>, String>
    {
        let mut engine = self
            .shared
            .engine
            .lock()
            .map_err(|_| "Diagnostics worker lock poisoned".to_string())?;
        if engine.is_none() {
            let process = self
                .shared
                .process
                .clone()
                .ok_or_else(|| "Watchdog is unavailable".to_string())?;
            let watchdog = self
                .shared
                .watchdog
                .clone()
                .ok_or_else(|| "Diagnostics watchdog is unavailable".to_string())?;
            let platform: Arc<dyn Platform> = Arc::from(platform::native_platform());
            *engine = Some(
                app::diagnostics::DiagnosticsTaskRuntime::new_managed(
                    platform,
                    self.shared.storage.clone(),
                    process,
                    watchdog,
                )
                .map_err(|error| error.to_string())?,
            );
        }
        Ok(engine)
    }

    pub(in crate::session) fn request_scan(&self) -> Result<(), DiagnosticsRequestError> {
        let engine = self.ensure_engine()?;
        engine
            .as_ref()
            .expect("Diagnostics engine initialized")
            .request_scan()
            .map_err(|error| DiagnosticsRequestError {
                diagnostic: error.to_string(),
                message: error.localized().message.into(),
            })
    }

    pub(in crate::session) fn request_repair(
        &self,
        actions: Vec<app::diagnostics::DiagnosticsRepairAction>,
    ) -> Result<(), DiagnosticsRequestError> {
        let engine = self.ensure_engine()?;
        engine
            .as_ref()
            .expect("Diagnostics engine initialized")
            .request_repair(actions)
            .map_err(|error| DiagnosticsRequestError {
                diagnostic: error.to_string(),
                message: error.localized().message.into(),
            })
    }

    pub(in crate::session) fn is_busy(&self) -> bool {
        self.shared
            .engine
            .lock()
            .ok()
            .and_then(|engine| engine.as_ref().map(|engine| engine.is_busy()))
            .unwrap_or(false)
    }

    pub(in crate::session) fn set_terminal_graphics_probe(
        &self,
        status: ui::TerminalGraphicsProbeStatus,
    ) {
        if let Ok(mut terminal_graphics) = self.shared.terminal_graphics.lock() {
            *terminal_graphics = Some(status);
        }
    }

    pub(in crate::session) fn drain_events(&self) -> Vec<app::diagnostics::DiagnosticsTaskEvent> {
        let Ok(engine) = self.shared.engine.lock() else {
            return Vec::new();
        };
        let mut events = engine
            .as_ref()
            .map(|engine| engine.drain_events())
            .unwrap_or_default();
        let terminal_graphics = self
            .shared
            .terminal_graphics
            .lock()
            .ok()
            .and_then(|check| check.clone());
        for event in &mut events {
            match event {
                app::diagnostics::DiagnosticsTaskEvent::ScanCompleted(Ok(snapshot)) => {
                    apply_terminal_graphics_check(snapshot, terminal_graphics.as_ref());
                }
                app::diagnostics::DiagnosticsTaskEvent::RepairCompleted {
                    snapshot: Some(snapshot),
                    ..
                } => apply_terminal_graphics_check(snapshot, terminal_graphics.as_ref()),
                _ => {}
            }
        }
        events
    }

    pub(in crate::session) fn restart_required(&self) -> bool {
        self.shared
            .engine
            .lock()
            .ok()
            .and_then(|engine| engine.as_ref().map(|engine| engine.restart_required()))
            .unwrap_or(false)
    }
}

pub(in crate::session) fn apply_terminal_graphics_check(
    snapshot: &mut app::diagnostics::DiagnosticsSnapshot,
    terminal_graphics: Option<&ui::TerminalGraphicsProbeStatus>,
) {
    let Some(terminal_graphics) = terminal_graphics else {
        return;
    };
    let Some(check) = snapshot
        .checks
        .iter_mut()
        .find(|check| check.id == "environment.terminal")
    else {
        return;
    };

    let (status, message, remediation) = match terminal_graphics {
        ui::TerminalGraphicsProbeStatus::Verified(protocol) => (
            app::diagnostics::DiagnosticStatus::Pass,
            format!(
                "{} graphics protocol verified; image icons are available",
                protocol.label()
            ),
            None,
        ),
        ui::TerminalGraphicsProbeStatus::Unsupported => (
            app::diagnostics::DiagnosticStatus::Unsupported,
            "Unsupported: the terminal responded but advertised no supported graphics protocol; ASCII icons are active"
                .to_string(),
            None,
        ),
        ui::TerminalGraphicsProbeStatus::NoResponse { reason } => (
            app::diagnostics::DiagnosticStatus::Warning,
            format!("Terminal graphics probe received no response: {reason}"),
            Some(
                "Check terminal or multiplexer query passthrough, then restart TundraUX"
                    .to_string(),
            ),
        ),
    };
    check.status = status;
    check.summary.clone_from(&message);
    check.detail = message;
    check.remediation = remediation;
}

impl ShellSession {
    pub(in crate::session) fn apply_terminal_graphics_startup_policy(
        &mut self,
        status: &ui::TerminalGraphicsProbeStatus,
    ) {
        match status {
            ui::TerminalGraphicsProbeStatus::Verified(_) => {}
            ui::TerminalGraphicsProbeStatus::Unsupported
                if self.ascii_assets.theme_id() == ui::DEFAULT_THEME_ID =>
            {
                if self.selected_login_icon_display_mode() != storage::IconDisplayMode::Image {
                    return;
                }
                self.pending_default_ascii_icon_fallback = true;
                self.notify_modal(
                    i18n::LocalizedText::from(i18n::msg!("shell-terminal-graphics-unsupported")),
                    i18n::LocalizedText::from(i18n::msg!("shell-this-terminal-responded-but-does-not-support-a-compatible-graphics-protocol-the-default-theme-will-use-ascii-i")),
                    ui::NotificationTone::Warning,
                    vec![ShellNotificationAction::new("continue", i18n::LocalizedText::from(i18n::msg!("shell-continue"))).cancel()],
                );
            }
            ui::TerminalGraphicsProbeStatus::Unsupported => {
                self.notify_modal(
                    i18n::LocalizedText::from(i18n::msg!("shell-terminal-graphics-unsupported")),
                    i18n::LocalizedText::from(i18n::msg!("shell-this-terminal-responded-but-does-not-support-a-compatible-graphics-protocol-the-custom-theme-was-left-unchange")),
                    ui::NotificationTone::Warning,
                    vec![ShellNotificationAction::new("continue", i18n::LocalizedText::from(i18n::msg!("shell-continue"))).cancel()],
                );
            }
            ui::TerminalGraphicsProbeStatus::NoResponse { reason } => {
                self.notify_modal(
                    i18n::LocalizedText::from(i18n::msg!("shell-no-terminal-graphics-response")),
                    i18n::LocalizedText::from(i18n::msg!("shell-tundraux-could-not-determine-whether-this-terminal-supports-a-graphics-protocol-reason-your-theme-was-not-chan", reason = reason.to_string())),
                    ui::NotificationTone::Warning,
                    vec![ShellNotificationAction::new("continue", i18n::LocalizedText::from(i18n::msg!("shell-continue"))).cancel()],
                );
            }
        }
    }

    fn selected_login_icon_display_mode(&self) -> storage::IconDisplayMode {
        self.storage_manager
            .as_ref()
            .and_then(|storage| {
                UserService::new(storage.clone())
                    .with_backend(self.identity_backend)
                    .login_records()
                    .ok()
            })
            .and_then(|users| {
                users
                    .into_iter()
                    .find(|user| user.username == self.login_username)
            })
            .map(|user| user.appearance.icon_display_mode)
            .unwrap_or_default()
    }

    pub(in crate::session) fn open_diagnostics(&mut self) {
        let authenticated = self
            .app
            .auth_session()
            .is_some_and(|session| session.role != UserRole::Guest);
        if !authenticated || self.active_screen() != ShellScreen::SystemStatus {
            self.notify_status(i18n::LocalizedText::from(i18n::msg!(
                "shell-open-diagnostics-from-system-status"
            )));
            return;
        }
        self.diagnostics_restart_required = self.diagnostics_restart_is_required();
        self.set_system_status_tab(ui::SystemStatusTab::Health);
        self.focused_component = ShellComponent::SystemStatus;
        self.clear_diagnostics_scrollbar_drag();
        self.diagnostics_feedback = None;
        if self.diagnostics_task_runtime.is_some() {
            self.request_diagnostics_scan();
        } else if self.app.diagnostics_snapshot().is_none() {
            self.diagnostics_feedback = Some(i18n::LocalizedText::from(i18n::msg!(
                "shell-diagnostics-runtime-is-unavailable"
            )));
        }
        self.refresh_hit_map();
    }

    pub(in crate::session) fn close_diagnostics(&mut self) {
        self.clear_diagnostics_scrollbar_drag();
        if self.active_screen() == ShellScreen::SystemStatus {
            self.cancel_diagnostics_repair_preview();
            self.set_system_status_tab(ui::SystemStatusTab::Overview);
            self.focused_component = ShellComponent::SystemStatus;
            self.refresh_hit_map();
            return;
        }
        if self.active_screen() == ShellScreen::Diagnostics {
            self.screen_stack.pop();
        }
        self.diagnostics_repair_preview.clear();
        self.diagnostics_repair_selected = 0;
        self.diagnostics_repair_scroll_offset = 0;
        self.diagnostics_repair_confirm_selected = true;
        if self.active_screen() == ShellScreen::SystemStatus {
            self.focused_component = ShellComponent::SystemStatus;
            self.refresh_hit_map();
        } else {
            let _ = self.settings_task_runtime.set_system_status_active(false);
            self.screen_stack = vec![ShellScreen::Home];
            self.focused_component = ShellComponent::Home;
            self.refresh_hit_map();
        }
    }

    pub(in crate::session) fn request_diagnostics_scan(&mut self) {
        if self.diagnostics_restart_is_required() {
            self.diagnostics_restart_required = true;
            self.notify_alert_with_tone(
                i18n::LocalizedText::from(i18n::msg!(
                    "shell-restart-tundraux-before-running-another-diagnostics-scan"
                )),
                ui::NotificationTone::Warning,
            );
            return;
        }
        if self.diagnostics_scanning
            || self
                .diagnostics_task_runtime
                .as_ref()
                .is_some_and(ShellDiagnosticsTaskRuntime::is_busy)
        {
            self.diagnostics_scanning = true;
            self.diagnostics_rescan_pending = true;
            self.diagnostics_feedback = Some(i18n::LocalizedText::from(i18n::msg!(
                "shell-diagnostics-task-in-progress"
            )));
            return;
        }
        let result = self
            .diagnostics_task_runtime
            .as_ref()
            .ok_or_else(|| DiagnosticsRequestError {
                diagnostic: "Diagnostics runtime is unavailable".to_string(),
                message: i18n::msg!("shell-diagnostics-runtime-is-unavailable").into(),
            })
            .and_then(ShellDiagnosticsTaskRuntime::request_scan);
        match result {
            Ok(()) => {
                self.diagnostics_scanning = true;
                self.diagnostics_feedback = Some(i18n::LocalizedText::from(i18n::msg!(
                    "shell-scanning-system-health"
                )));
            }
            Err(error) => {
                self.diagnostics_scanning = false;
                self.diagnostics_feedback = Some(error.message.clone());
                self.notify_alert_with_tone(error.message, ui::NotificationTone::Critical);
            }
        }
    }

    pub(in crate::session) fn drain_diagnostics_events(&mut self) {
        let events = self
            .diagnostics_task_runtime
            .as_ref()
            .map(ShellDiagnosticsTaskRuntime::drain_events)
            .unwrap_or_default();
        for event in events {
            match event {
                app::diagnostics::DiagnosticsTaskEvent::ScanCompleted(result) => {
                    self.diagnostics_scanning = false;
                    match result {
                        Ok(snapshot) => {
                            self.install_diagnostics_snapshot(snapshot);
                            self.diagnostics_feedback =
                                Some(i18n::LocalizedText::from(i18n::msg!("shell-scan-complete")));
                        }
                        Err(error) => {
                            let message = if self.diagnostics_can_view_details() {
                                i18n::LocalizedText::from(i18n::msg!(
                                    "shell-diagnostics-scan-failed-error",
                                    error = error.to_string()
                                ))
                            } else {
                                i18n::LocalizedText::from(i18n::msg!(
                                    "shell-diagnostics-scan-failed-ask-an-administrator-to-review-the-details"
                                ))
                            };
                            self.diagnostics_feedback = Some(message.clone());
                            self.notify_alert_with_tone(message, ui::NotificationTone::Critical);
                        }
                    }
                    if self.diagnostics_rescan_pending && !self.diagnostics_restart_is_required() {
                        self.diagnostics_rescan_pending = false;
                        self.request_diagnostics_scan();
                    }
                }
                app::diagnostics::DiagnosticsTaskEvent::RepairProgress {
                    completed,
                    total,
                    label,
                } => {
                    self.diagnostics_feedback = Some(i18n::LocalizedText::from(i18n::msg!(
                        "shell-repairing-arg1-arg2-label",
                        arg1 = completed.saturating_add(1),
                        arg2 = total,
                        label = label
                    )));
                }
                app::diagnostics::DiagnosticsTaskEvent::RepairCompleted {
                    results,
                    snapshot,
                    restart_required,
                } => {
                    if let Some(context) = self.diagnostics_repair_log_context.take() {
                        for result in &results {
                            let mut event = runtime_log::RuntimeLogEvent::new(
                                context.clone(),
                                if result.success {
                                    runtime_log::LogLevel::Info
                                } else {
                                    runtime_log::LogLevel::Error
                                },
                                if result.success {
                                    runtime_log::LogPhase::Recovered
                                } else {
                                    runtime_log::LogPhase::Failed
                                },
                                result.action.label(),
                            );
                            if matches!(result.action, app::diagnostics::DiagnosticsRepairAction::RestoreDefaultThemeFile { .. }) { event.context.module = "ux.assets".into(); }
                            if !result.success {
                                event.error_code = Some("UX_DIAGNOSTICS_REPAIR_FAILED".into());
                                event.error_chain.push(result.message.clone());
                            }
                            record_shell_runtime_event(event);
                        }
                    }
                    self.diagnostics_scanning = false;
                    self.diagnostics_repair_preview.clear();
                    self.diagnostics_repair_selected = 0;
                    self.diagnostics_repair_scroll_offset = 0;
                    self.diagnostics_repair_confirm_selected = true;
                    let succeeded = results.iter().filter(|result| result.success).count();
                    let failed = results.len().saturating_sub(succeeded);
                    let backups = results
                        .iter()
                        .filter(|result| result.backup_path.is_some())
                        .count();
                    self.diagnostics_feedback = Some(i18n::LocalizedText::from(i18n::msg!(
                        "shell-repair-complete-succeeded-succeeded-failed-failedarg1",
                        succeeded = succeeded,
                        failed = failed,
                        arg1 = if backups == 0 {
                            i18n::LocalizedText::from("")
                        } else {
                            i18n::LocalizedText::from(i18n::msg!(
                                "shell-backups-backup-s-created",
                                backups = backups
                            ))
                        }
                    )));
                    if let Some(snapshot) = snapshot {
                        self.install_diagnostics_snapshot(snapshot);
                    }
                    if restart_required && succeeded > 0 {
                        self.diagnostics_restart_required = true;
                        self.notify_modal(
                            i18n::LocalizedText::from(i18n::msg!("shell-restart-required")),
                            i18n::LocalizedText::from(i18n::msg!("shell-storage-was-repaired-and-the-current-in-memory-session-is-stale-restart-tundraux-before-continuing")),
                            ui::NotificationTone::Warning,
                            vec![
                                ShellNotificationAction::new("restart", i18n::LocalizedText::from(i18n::msg!("shell-restart-now")))
                                    .with_shortcut(InputKey::Char('r'))
                                    .with_follow_up(ShellCommand::Restart),
                                ShellNotificationAction::new("exit", i18n::LocalizedText::from(i18n::msg!("shell-exit-now")))
                                    .with_shortcut(InputKey::Char('e'))
                                    .with_follow_up(ShellCommand::ConfirmExit),
                                ShellNotificationAction::new("review", i18n::LocalizedText::from(i18n::msg!("shell-review-results"))).cancel(),
                            ],
                        );
                    } else if failed > 0 {
                        self.notify_alert_with_tone(
                            i18n::LocalizedText::from(i18n::msg!(
                                "shell-failed-diagnostics-repair-action-s-failed",
                                failed = failed
                            )),
                            ui::NotificationTone::Warning,
                        );
                    } else {
                        self.notify_toast(i18n::LocalizedText::from(i18n::msg!(
                            "shell-diagnostics-repair-completed"
                        )));
                    }
                    let rescan_pending = std::mem::take(&mut self.diagnostics_rescan_pending);
                    if rescan_pending && !self.diagnostics_restart_is_required() {
                        self.request_diagnostics_scan();
                    }
                }
            }
        }
    }

    pub(in crate::session) fn diagnostics_can_view_details(&self) -> bool {
        PermissionService::new(self.debug_policy)
            .authorize(
                self.app.auth_session(),
                PermissionAction::ViewDiagnosticsDetails,
                None,
            )
            .allowed
    }

    pub(in crate::session) fn diagnostics_can_repair(&self) -> bool {
        !self.diagnostics_restart_is_required()
            && !self.diagnostics_scanning
            && !self
                .diagnostics_task_runtime
                .as_ref()
                .is_some_and(ShellDiagnosticsTaskRuntime::is_busy)
            && PermissionService::new(self.debug_policy)
                .authorize(
                    self.app.auth_session(),
                    PermissionAction::RepairDiagnostics,
                    None,
                )
                .allowed
    }

    pub(in crate::session) fn diagnostics_restart_is_required(&self) -> bool {
        self.diagnostics_restart_required
            || self
                .diagnostics_task_runtime
                .as_ref()
                .is_some_and(ShellDiagnosticsTaskRuntime::restart_required)
    }

    pub(in crate::session) fn diagnostics_item_count(&self) -> usize {
        let Some(snapshot) = self.app.diagnostics_snapshot() else {
            return 0;
        };
        match self.diagnostics_tab {
            ui::DiagnosticsTab::Health => snapshot.checks.len(),
            ui::DiagnosticsTab::Logs => snapshot.logs.len(),
            ui::DiagnosticsTab::Incidents => snapshot.incidents.len(),
        }
    }

    pub(in crate::session) fn clamp_diagnostics_selection(&mut self) {
        let check_count = self
            .app
            .diagnostics_snapshot()
            .map(|snapshot| snapshot.checks.len())
            .unwrap_or(0);
        self.diagnostics_selected_check = if check_count == 0 {
            0
        } else {
            self.diagnostics_selected_check.min(check_count - 1)
        };
        let log_count = self
            .app
            .diagnostics_snapshot()
            .map(|snapshot| snapshot.logs.len())
            .unwrap_or(0);
        self.diagnostics_selected_log = if log_count == 0 {
            0
        } else {
            self.diagnostics_selected_log.min(log_count - 1)
        };
        let incident_count = self
            .app
            .diagnostics_snapshot()
            .map(|snapshot| snapshot.incidents.len())
            .unwrap_or(0);
        self.diagnostics_selected_incident = if incident_count == 0 {
            0
        } else {
            self.diagnostics_selected_incident.min(incident_count - 1)
        };
    }

    pub(in crate::session) fn install_diagnostics_snapshot(
        &mut self,
        snapshot: app::diagnostics::DiagnosticsSnapshot,
    ) {
        let selected_log_path = self
            .app
            .diagnostics_snapshot()
            .and_then(|current| current.logs.get(self.diagnostics_selected_log))
            .map(|log| log.relative_path.clone());
        self.app.dispatch_at(
            app::AppCommand::SetDiagnosticsSnapshot(Some(snapshot)),
            Instant::now(),
        );
        if let Some(relative_path) = selected_log_path
            && let Some(index) = self.app.diagnostics_snapshot().and_then(|current| {
                current
                    .logs
                    .iter()
                    .position(|log| log.relative_path == relative_path)
            })
        {
            self.diagnostics_selected_log = index;
        }
        self.clamp_diagnostics_selection();
    }

    pub(in crate::session) fn move_diagnostics_selection(&mut self, delta: isize) {
        let count = self.diagnostics_item_count();
        if count == 0 {
            return;
        }
        let selected = match self.diagnostics_tab {
            ui::DiagnosticsTab::Health => &mut self.diagnostics_selected_check,
            ui::DiagnosticsTab::Logs => &mut self.diagnostics_selected_log,
            ui::DiagnosticsTab::Incidents => &mut self.diagnostics_selected_incident,
        };
        *selected =
            ((*selected as isize) + delta).clamp(0, count.saturating_sub(1) as isize) as usize;
        self.diagnostics_list_window_is_explicit = false;
    }

    pub(in crate::session) fn set_diagnostics_tab(&mut self, tab: ui::DiagnosticsTab) {
        self.diagnostics_tab = tab;
        if self.active_screen() == ShellScreen::SystemStatus {
            self.set_system_status_tab(match tab {
                ui::DiagnosticsTab::Health => ui::SystemStatusTab::Health,
                ui::DiagnosticsTab::Logs => ui::SystemStatusTab::Logs,
                ui::DiagnosticsTab::Incidents => ui::SystemStatusTab::Incidents,
            });
            return;
        }
        self.diagnostics_list_window_start = 0;
        self.diagnostics_list_window_is_explicit = false;
        self.clear_diagnostics_scrollbar_drag();
        self.clamp_diagnostics_selection();
    }

    pub(in crate::session) fn select_diagnostics_index(&mut self, index: usize) {
        let count = self.diagnostics_item_count();
        if count == 0 {
            return;
        }
        let index = index.min(count - 1);
        match self.diagnostics_tab {
            ui::DiagnosticsTab::Health => self.diagnostics_selected_check = index,
            ui::DiagnosticsTab::Logs => self.diagnostics_selected_log = index,
            ui::DiagnosticsTab::Incidents => self.diagnostics_selected_incident = index,
        }
        self.diagnostics_list_window_is_explicit = false;
    }

    pub(in crate::session) fn begin_diagnostics_scrollbar_drag(
        &mut self,
        coordinates: CellPosition,
    ) {
        let Some(layout) = self.active_diagnostics_content_layout() else {
            return;
        };
        let Some(scrollbar) = layout.list_scrollbar else {
            return;
        };
        if !rect_contains(scrollbar.thumb, coordinates) {
            return;
        }
        self.scrollbar_drag = Some(ScrollbarDragState::Diagnostics {
            grab_offset: coordinates.1.saturating_sub(scrollbar.thumb.y),
        });
    }

    pub(in crate::session) fn drag_diagnostics_scrollbar(&mut self, coordinates: CellPosition) {
        let Some(ScrollbarDragState::Diagnostics { grab_offset }) = self.scrollbar_drag else {
            return;
        };
        let model = self.to_diagnostics_view_model();
        let Some(layout) = self.active_diagnostics_content_layout() else {
            return;
        };
        let Some(scrollbar) = layout.list_scrollbar else {
            self.clear_diagnostics_scrollbar_drag();
            return;
        };
        self.diagnostics_list_window_start = scrollbar_window_start(
            coordinates.1,
            grab_offset,
            scrollbar.track.y,
            scrollbar.track.height,
            scrollbar.thumb.height,
            model.item_count(),
            layout.visible_capacity,
        );
        self.diagnostics_list_window_is_explicit = true;
    }

    pub(in crate::session) fn clear_diagnostics_scrollbar_drag(&mut self) -> bool {
        if matches!(
            self.scrollbar_drag,
            Some(ScrollbarDragState::Diagnostics { .. })
        ) {
            self.scrollbar_drag = None;
            true
        } else {
            false
        }
    }

    fn active_diagnostics_content_layout(&self) -> Option<ui::DiagnosticsContentLayout> {
        let area = Rect::new(0, 0, self.terminal_size.0, self.terminal_size.1);
        let ui::ShellLayout::Full { main, .. } = self.shell_layout_for(area) else {
            return None;
        };
        if self.active_screen() == ShellScreen::SystemStatus {
            return self
                .to_system_status_view_model()
                .and_then(|model| ui::system_status_layout(main, &model).diagnostics_content);
        }
        Some(ui::diagnostics_layout(main, &self.to_diagnostics_view_model()).content_layout())
    }

    pub(in crate::session) fn preview_selected_diagnostics_repair(&mut self) {
        if !self.ensure_diagnostics_repair_authorized() {
            return;
        }
        let repair = self
            .app
            .diagnostics_snapshot()
            .and_then(|snapshot| snapshot.checks.get(self.diagnostics_selected_check))
            .and_then(|check| check.repair.clone());
        match repair {
            Some(repair) => {
                self.diagnostics_repair_preview = vec![repair];
                self.reset_diagnostics_repair_dialog_selection();
            }
            None => self.notify_status(i18n::LocalizedText::from(i18n::msg!(
                "shell-selected-check-has-no-automatic-repair"
            ))),
        }
        self.refresh_hit_map();
    }

    pub(in crate::session) fn preview_all_diagnostics_repairs(&mut self) {
        if !self.ensure_diagnostics_repair_authorized() {
            return;
        }
        let plan = self
            .app
            .diagnostics_snapshot()
            .map(app::diagnostics::DiagnosticsSnapshot::repair_plan)
            .unwrap_or_default();
        if plan.is_empty() {
            self.notify_status(i18n::LocalizedText::from(i18n::msg!(
                "shell-no-automatic-repairs-are-available"
            )));
        } else {
            self.diagnostics_repair_preview = plan;
            self.reset_diagnostics_repair_dialog_selection();
        }
        self.refresh_hit_map();
    }

    pub(in crate::session) fn cancel_diagnostics_repair_preview(&mut self) {
        self.diagnostics_repair_preview.clear();
        self.reset_diagnostics_repair_dialog_selection();
        self.refresh_hit_map();
    }

    pub(in crate::session) fn reset_diagnostics_repair_dialog_selection(&mut self) {
        self.diagnostics_repair_selected = 0;
        self.diagnostics_repair_scroll_offset = 0;
        self.diagnostics_repair_confirm_selected = true;
    }

    pub(in crate::session) fn move_diagnostics_repair_selection(&mut self, delta: isize) {
        if self.diagnostics_repair_preview.is_empty() {
            return;
        }
        self.diagnostics_repair_selected = ((self.diagnostics_repair_selected as isize) + delta)
            .clamp(
                0,
                self.diagnostics_repair_preview.len().saturating_sub(1) as isize,
            ) as usize;
    }

    pub(in crate::session) fn select_diagnostics_repair_item(&mut self, index: usize) {
        if self.diagnostics_repair_preview.is_empty() {
            return;
        }
        self.diagnostics_repair_selected =
            index.min(self.diagnostics_repair_preview.len().saturating_sub(1));
    }

    pub(in crate::session) fn confirm_diagnostics_repair(&mut self) {
        if !self.ensure_diagnostics_repair_authorized() {
            return;
        }
        let actions = std::mem::take(&mut self.diagnostics_repair_preview);
        self.reset_diagnostics_repair_dialog_selection();
        if actions.is_empty() {
            return;
        }
        let mut context = self.operation_log_context("ux.diagnostics", "repair");
        context.task_id = Some("event-loop".into());
        self.diagnostics_repair_log_context = Some(context.clone());
        record_shell_runtime_event(runtime_log::RuntimeLogEvent::new(
            context.clone(),
            runtime_log::LogLevel::Info,
            runtime_log::LogPhase::Started,
            "Starting diagnostic repair",
        ));
        let result = self
            .diagnostics_task_runtime
            .as_ref()
            .ok_or_else(|| DiagnosticsRequestError {
                diagnostic: "Diagnostics runtime is unavailable".to_string(),
                message: i18n::msg!("shell-diagnostics-runtime-is-unavailable").into(),
            })
            .and_then(|runtime| runtime.request_repair(actions.clone()));
        match result {
            Ok(()) => {
                self.diagnostics_scanning = true;
                self.diagnostics_feedback = Some(i18n::LocalizedText::from(i18n::msg!(
                    "shell-starting-arg1-repair-action-s",
                    arg1 = actions.len()
                )));
            }
            Err(error) => {
                self.diagnostics_repair_log_context = None;
                let mut event = runtime_log::RuntimeLogEvent::new(
                    context,
                    runtime_log::LogLevel::Error,
                    runtime_log::LogPhase::Failed,
                    "Diagnostic repair could not start",
                );
                event.error_code = Some("UX_DIAGNOSTICS_SUBMIT_FAILED".into());
                event.error_chain.push(error.diagnostic);
                record_shell_runtime_event(event);
                self.diagnostics_repair_preview = actions;
                self.notify_alert_with_tone(error.message, ui::NotificationTone::Critical);
            }
        }
        self.refresh_hit_map();
    }

    pub(in crate::session) fn ensure_diagnostics_repair_authorized(&mut self) -> bool {
        let permission = PermissionService::new(self.debug_policy).authorize(
            self.app.auth_session(),
            PermissionAction::RepairDiagnostics,
            None,
        );
        if permission.allowed
            && !self.diagnostics_restart_is_required()
            && !self.diagnostics_scanning
            && !self
                .diagnostics_task_runtime
                .as_ref()
                .is_some_and(ShellDiagnosticsTaskRuntime::is_busy)
        {
            return true;
        }
        let reason = permission.reason.unwrap_or_else(|| {
            if self.diagnostics_restart_is_required() {
                "restart_required".to_string()
            } else {
                "diagnostics_task_in_progress".to_string()
            }
        });
        self.notify_alert_with_tone(
            i18n::LocalizedText::from(i18n::msg!(
                "shell-diagnostics-repair-denied-reason",
                reason = reason.to_string()
            )),
            ui::NotificationTone::Warning,
        );
        false
    }

    pub(in crate::session) fn copy_diagnostics_summary(&mut self, platform: &dyn Platform) {
        let Some(snapshot) = self.app.diagnostics_snapshot() else {
            self.notify_status(i18n::LocalizedText::from(i18n::msg!(
                "shell-no-diagnostics-snapshot-is-available"
            )));
            return;
        };
        let full = self.diagnostics_can_view_details();
        let (pass, unsupported, warning, fail) = snapshot.status_counts();
        let mut lines = vec![
            "TundraUX3 Diagnostics".to_string(),
            format!("Status: {:?}", snapshot.overall_status()),
            format!(
                "Checks: {pass} pass, {unsupported} unsupported, {warning} warning, {fail} fail"
            ),
            format!("Log files: {}", snapshot.logs.len()),
            format!("Incidents retained: {}", snapshot.incidents.len()),
        ];
        for check in &snapshot.checks {
            lines.push(format!(
                "[{}] {}: {}",
                check.status.label(),
                check.label,
                if full {
                    check.detail.clone()
                } else {
                    diagnostics_public_check_summary(check)
                }
            ));
        }
        if full {
            for incident in &snapshot.incidents {
                lines.push(format!(
                    "[{:?}] {} {}: {}",
                    incident.severity, incident.occurred_at, incident.incident_id, incident.summary
                ));
            }
        }
        let text = lines.join("\n");
        match platform.write_clipboard_text(&text) {
            Ok(()) => {
                self.notify_toast(i18n::LocalizedText::from(i18n::msg!(
                    "shell-copied-diagnostics-summary"
                )));
            }
            Err(error) => {
                self.notify_alert_with_tone(
                    i18n::LocalizedText::from(i18n::msg!(
                        "shell-could-not-copy-diagnostics-summary-error",
                        error = error.to_string()
                    )),
                    ui::NotificationTone::Critical,
                );
            }
        }
    }

    pub(in crate::session) fn open_diagnostics_logs_in_explorer(
        &mut self,
        platform: &dyn Platform,
    ) {
        if !self.diagnostics_can_explore_logs() {
            self.notify_alert_with_tone(
                i18n::LocalizedText::from(i18n::msg!(
                    "shell-only-administrators-can-explore-the-diagnostic-log-folder"
                )),
                ui::NotificationTone::Warning,
            );
            return;
        }

        let Some(storage) = self.storage_manager.clone() else {
            self.notify_alert_with_tone(
                i18n::LocalizedText::from(i18n::msg!(
                    "shell-diagnostics-log-directory-is-unavailable"
                )),
                ui::NotificationTone::Critical,
            );
            return;
        };
        let logs_path = storage.layout().logs_path.clone();
        if !logs_path.is_dir() {
            self.notify_alert_with_tone(
                i18n::LocalizedText::from(i18n::msg!(
                    "shell-diagnostics-log-directory-is-unavailable-arg1",
                    arg1 = logs_path.display().to_string()
                )),
                ui::NotificationTone::Critical,
            );
            return;
        }

        self.open_explorer_at(
            platform,
            &storage,
            logs_path,
            ExplorerPurpose::DiagnosticsLogs,
        );
        self.notify_toast(i18n::LocalizedText::from(i18n::msg!(
            "shell-opened-diagnostic-log-folder-in-explorer"
        )));
    }

    fn diagnostics_can_explore_logs(&self) -> bool {
        self.app.auth_session().is_some_and(|session| {
            session.source == identity::IdentitySource::LinuxCurrentProcess
                || session.role == UserRole::Admin
        })
    }

    pub(in crate::session) fn open_selected_diagnostics_report(
        &mut self,
        _platform: &dyn Platform,
    ) {
        if self.diagnostics_tab == ui::DiagnosticsTab::Health {
            self.notify_status(i18n::LocalizedText::from(i18n::msg!(
                "shell-open-logs-from-system-status"
            )));
            return;
        }
        if !self.diagnostics_can_view_details() {
            self.notify_alert_with_tone(
                i18n::LocalizedText::from(i18n::msg!(
                    "shell-only-administrators-can-open-diagnostic-logs-and-reports"
                )),
                ui::NotificationTone::Warning,
            );
            return;
        }

        let (reload, missing_message, opened_message) = match self.diagnostics_tab {
            ui::DiagnosticsTab::Logs => {
                let path = self
                    .app
                    .diagnostics_snapshot()
                    .and_then(|snapshot| snapshot.logs.get(self.diagnostics_selected_log))
                    .map(|log| log.path.clone());
                let Some(path) = path else {
                    self.notify_status(i18n::LocalizedText::from(i18n::msg!(
                        "shell-no-diagnostic-log-is-selected"
                    )));
                    return;
                };
                (
                    EditorReloadPolicy::Log { path },
                    i18n::LocalizedText::from(i18n::msg!("shell-could-not-open-diagnostic-log")),
                    i18n::LocalizedText::from(i18n::msg!("shell-opened-diagnostic-log-read-only")),
                )
            }
            ui::DiagnosticsTab::Incidents => {
                let path = self
                    .app
                    .diagnostics_snapshot()
                    .and_then(|snapshot| snapshot.incidents.get(self.diagnostics_selected_incident))
                    .map(|incident| {
                        incident
                            .text_report_path
                            .clone()
                            .unwrap_or_else(|| incident.json_report_path.clone())
                    });
                let Some(path) = path else {
                    self.notify_status(i18n::LocalizedText::from(i18n::msg!(
                        "shell-no-incident-report-is-selected"
                    )));
                    return;
                };
                (
                    EditorReloadPolicy::DiagnosticsReport { path },
                    i18n::LocalizedText::from(i18n::msg!(
                        "shell-could-not-open-diagnostics-report"
                    )),
                    i18n::LocalizedText::from(i18n::msg!(
                        "shell-opened-diagnostics-report-read-only"
                    )),
                )
            }
            ui::DiagnosticsTab::Health => unreachable!(),
        };

        match self.open_diagnostics_editor(reload) {
            Ok(()) => {
                self.notify_toast(opened_message);
            }
            Err(error) => {
                self.notify_alert_with_tone(
                    i18n::LocalizedText::from(i18n::msg!(
                        "shell-missing-message-error",
                        missing_message = missing_message,
                        error = error
                    )),
                    ui::NotificationTone::Critical,
                );
            }
        }
    }
}

#[cfg(test)]
#[path = "../../../tests/unit/session/controller/diagnostics/diagnostics_shell_tests.rs"]
mod diagnostics_shell_tests;

/// Keeps submission diagnostics stable while retaining the UI message for language changes.
#[derive(Debug)]
pub(in crate::session) struct DiagnosticsRequestError {
    diagnostic: String,
    message: i18n::LocalizedText,
}

impl From<String> for DiagnosticsRequestError {
    fn from(diagnostic: String) -> Self {
        Self {
            message: diagnostic.clone().into(),
            diagnostic,
        }
    }
}
