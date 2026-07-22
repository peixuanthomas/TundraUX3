use super::super::*;
#[derive(Clone)]
pub(in crate::session) struct ShellDiagnosticsTaskRuntime {
    pub(in crate::session) shared: Arc<ShellDiagnosticsTaskShared>,
}

pub(in crate::session) struct ShellDiagnosticsTaskShared {
    pub(in crate::session) engine: Mutex<Option<app::diagnostics::DiagnosticsTaskRuntime>>,
    pub(in crate::session) terminal_check: Mutex<Option<platform::EnvironmentCheck>>,
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
                terminal_check: Mutex::new(None),
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

    pub(in crate::session) fn request_scan(&self) -> Result<(), String> {
        let engine = self.ensure_engine()?;
        engine
            .as_ref()
            .expect("Diagnostics engine initialized")
            .request_scan()
            .map_err(|error| error.to_string())
    }

    pub(in crate::session) fn request_repair(
        &self,
        actions: Vec<app::diagnostics::DiagnosticsRepairAction>,
    ) -> Result<(), String> {
        let engine = self.ensure_engine()?;
        engine
            .as_ref()
            .expect("Diagnostics engine initialized")
            .request_repair(actions)
            .map_err(|error| error.to_string())
    }

    pub(in crate::session) fn is_busy(&self) -> bool {
        self.shared
            .engine
            .lock()
            .ok()
            .and_then(|engine| engine.as_ref().map(|engine| engine.is_busy()))
            .unwrap_or(false)
    }

    pub(in crate::session) fn set_terminal_graphics_protocol(
        &self,
        kind: platform::PlatformKind,
        protocol: Option<ui::EditorGraphicsProtocol>,
    ) {
        let wt_session = std::env::var("WT_SESSION").ok();
        let check = platform::terminal_environment_check_with_graphics_protocol(
            kind,
            wt_session.as_deref(),
            protocol.map(ui::EditorGraphicsProtocol::label),
        );
        if let Ok(mut terminal_check) = self.shared.terminal_check.lock() {
            *terminal_check = Some(check);
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
        let terminal_check = self
            .shared
            .terminal_check
            .lock()
            .ok()
            .and_then(|check| check.clone());
        for event in &mut events {
            match event {
                app::diagnostics::DiagnosticsTaskEvent::ScanCompleted(Ok(snapshot)) => {
                    apply_terminal_environment_check(snapshot, terminal_check.as_ref());
                }
                app::diagnostics::DiagnosticsTaskEvent::RepairCompleted {
                    snapshot: Some(snapshot),
                    ..
                } => apply_terminal_environment_check(snapshot, terminal_check.as_ref()),
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

pub(in crate::session) fn apply_terminal_environment_check(
    snapshot: &mut app::diagnostics::DiagnosticsSnapshot,
    terminal_check: Option<&platform::EnvironmentCheck>,
) {
    let Some(terminal_check) = terminal_check else {
        return;
    };
    let Some(check) = snapshot.checks.iter_mut().find(|check| {
        check.category == app::diagnostics::DiagnosticCategory::Environment
            && check.label == "Terminal"
    }) else {
        return;
    };

    let (status, remediation) = match terminal_check.status {
        platform::CheckStatus::Pass => (app::diagnostics::DiagnosticStatus::Pass, None),
        platform::CheckStatus::Warning => (
            app::diagnostics::DiagnosticStatus::Warning,
            Some(
                "Use a terminal with Kitty, Sixel, or iTerm2 graphics protocol support".to_string(),
            ),
        ),
        platform::CheckStatus::Fail => (
            app::diagnostics::DiagnosticStatus::Fail,
            Some("Use a supported platform and terminal configuration".to_string()),
        ),
    };
    check.status = status;
    check.summary.clone_from(&terminal_check.message);
    check.detail.clone_from(&terminal_check.message);
    check.remediation = remediation;
}

impl ShellSession {
    pub(in crate::session) fn open_diagnostics(&mut self) {
        if self.is_strict_guest() {
            self.notify_alert_with_tone(
                "Diagnostics requires an authenticated account",
                ui::NotificationTone::Warning,
            );
            return;
        }
        self.diagnostics_restart_required = self.diagnostics_restart_is_required();
        self.screen_stack.push(ShellScreen::Diagnostics);
        self.focused_component = ShellComponent::Diagnostics;
        self.diagnostics_tab = ui::DiagnosticsTab::Health;
        self.diagnostics_list_window_start = 0;
        self.diagnostics_list_window_is_explicit = false;
        self.clear_diagnostics_scrollbar_drag();
        self.diagnostics_feedback = None;
        if self.diagnostics_task_runtime.is_some() {
            self.request_diagnostics_scan();
        } else if self.app.diagnostics_snapshot().is_none() {
            self.diagnostics_feedback = Some("Diagnostics runtime is unavailable".to_string());
        }
        self.refresh_hit_map();
    }

    pub(in crate::session) fn close_diagnostics(&mut self) {
        self.clear_diagnostics_scrollbar_drag();
        if self.active_screen() == ShellScreen::Diagnostics {
            self.screen_stack.pop();
        }
        if self.screen_stack.is_empty() {
            self.screen_stack.push(ShellScreen::Home);
        }
        self.diagnostics_repair_preview.clear();
        self.diagnostics_repair_selected = 0;
        self.diagnostics_repair_scroll_offset = 0;
        self.diagnostics_repair_confirm_selected = true;
        self.focused_component = ShellComponent::Home;
        self.refresh_hit_map();
    }

    pub(in crate::session) fn request_diagnostics_scan(&mut self) {
        if self.diagnostics_restart_is_required() {
            self.diagnostics_restart_required = true;
            self.notify_alert_with_tone(
                "Restart TundraUX before running another diagnostics scan",
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
            self.diagnostics_feedback = Some("Diagnostics task in progress…".to_string());
            return;
        }
        let result = self
            .diagnostics_task_runtime
            .as_ref()
            .ok_or_else(|| "Diagnostics runtime is unavailable".to_string())
            .and_then(ShellDiagnosticsTaskRuntime::request_scan);
        match result {
            Ok(()) => {
                self.diagnostics_scanning = true;
                self.diagnostics_feedback = Some("Scanning system health…".to_string());
            }
            Err(error) => {
                self.diagnostics_scanning = false;
                self.diagnostics_feedback = Some(error.clone());
                self.notify_alert_with_tone(error, ui::NotificationTone::Critical);
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
                            self.diagnostics_feedback = Some("Scan complete".to_string());
                        }
                        Err(error) => {
                            let message = if self.diagnostics_can_view_details() {
                                format!("Diagnostics scan failed: {error}")
                            } else {
                                "Diagnostics scan failed; ask an administrator to review the details"
                                    .to_string()
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
                    self.diagnostics_feedback = Some(format!(
                        "Repairing {}/{}: {label}",
                        completed.saturating_add(1),
                        total
                    ));
                }
                app::diagnostics::DiagnosticsTaskEvent::RepairCompleted {
                    results,
                    snapshot,
                    restart_required,
                } => {
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
                    self.diagnostics_feedback = Some(format!(
                        "Repair complete: {succeeded} succeeded, {failed} failed{}",
                        if backups == 0 {
                            String::new()
                        } else {
                            format!(", {backups} backup(s) created")
                        }
                    ));
                    if let Some(snapshot) = snapshot {
                        self.install_diagnostics_snapshot(snapshot);
                    }
                    if restart_required && succeeded > 0 {
                        self.diagnostics_restart_required = true;
                        self.notify_modal(
                            "Restart required",
                            "Storage was repaired and the current in-memory session is stale. Exit TundraUX before continuing.",
                            ui::NotificationTone::Warning,
                            vec![
                                ShellNotificationAction::new("exit", "Exit now")
                                    .with_follow_up(ShellCommand::ConfirmExit),
                                ShellNotificationAction::new("review", "Review results").cancel(),
                            ],
                        );
                    } else if failed > 0 {
                        self.notify_alert_with_tone(
                            format!("{failed} diagnostics repair action(s) failed"),
                            ui::NotificationTone::Warning,
                        );
                    } else {
                        self.notify_toast("Diagnostics repair completed");
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
        let area = Rect::new(0, 0, self.terminal_size.0, self.terminal_size.1);
        let ui::ShellLayout::Full { main, .. } = ui::compute_shell_layout(area) else {
            return;
        };
        let layout = ui::diagnostics_layout(main, &self.to_diagnostics_view_model());
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
        let area = Rect::new(0, 0, self.terminal_size.0, self.terminal_size.1);
        let ui::ShellLayout::Full { main, .. } = ui::compute_shell_layout(area) else {
            return;
        };
        let model = self.to_diagnostics_view_model();
        let layout = ui::diagnostics_layout(main, &model);
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
            None => self.notify_status("Selected check has no automatic repair"),
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
            self.notify_status("No automatic repairs are available");
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
        let result = self
            .diagnostics_task_runtime
            .as_ref()
            .ok_or_else(|| "Diagnostics runtime is unavailable".to_string())
            .and_then(|runtime| runtime.request_repair(actions.clone()));
        match result {
            Ok(()) => {
                self.diagnostics_scanning = true;
                self.diagnostics_feedback =
                    Some(format!("Starting {} repair action(s)…", actions.len()));
            }
            Err(error) => {
                self.diagnostics_repair_preview = actions;
                self.notify_alert_with_tone(error, ui::NotificationTone::Critical);
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
            format!("Diagnostics repair denied: {reason}"),
            ui::NotificationTone::Warning,
        );
        false
    }

    pub(in crate::session) fn copy_diagnostics_summary(&mut self, platform: &dyn Platform) {
        let Some(snapshot) = self.app.diagnostics_snapshot() else {
            self.notify_status("No diagnostics snapshot is available");
            return;
        };
        let full = self.diagnostics_can_view_details();
        let (pass, warning, fail) = snapshot.status_counts();
        let mut lines = vec![
            "TundraUX3 Diagnostics".to_string(),
            format!("Status: {:?}", snapshot.overall_status()),
            format!("Checks: {pass} pass, {warning} warning, {fail} fail"),
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
                self.notify_toast("Copied diagnostics summary");
            }
            Err(error) => {
                self.notify_alert_with_tone(
                    format!("Could not copy diagnostics summary: {error}"),
                    ui::NotificationTone::Critical,
                );
            }
        }
    }

    pub(in crate::session) fn open_diagnostics_logs_in_explorer(
        &mut self,
        platform: &dyn Platform,
    ) {
        if !self.diagnostics_can_view_details() {
            self.notify_alert_with_tone(
                "Only administrators can explore the diagnostic log folder",
                ui::NotificationTone::Warning,
            );
            return;
        }

        let Some(storage) = self.storage_manager.clone() else {
            self.notify_alert_with_tone(
                "Diagnostics log directory is unavailable",
                ui::NotificationTone::Critical,
            );
            return;
        };
        let logs_path = storage.layout().logs_path.clone();
        if !logs_path.is_dir() {
            self.notify_alert_with_tone(
                format!(
                    "Diagnostics log directory is unavailable: {}",
                    logs_path.display()
                ),
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
        self.notify_toast("Opened diagnostic log folder in Explorer");
    }

    pub(in crate::session) fn open_selected_diagnostics_report(
        &mut self,
        _platform: &dyn Platform,
    ) {
        if self.diagnostics_tab == ui::DiagnosticsTab::Health {
            self.set_diagnostics_tab(ui::DiagnosticsTab::Logs);
            self.notify_status("Diagnostics logs");
            return;
        }
        if !self.diagnostics_can_view_details() {
            self.notify_alert_with_tone(
                "Only administrators can open diagnostic logs and reports",
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
                    self.notify_status("No diagnostic log is selected");
                    return;
                };
                (
                    EditorReloadPolicy::Log { path },
                    "Could not open diagnostic log",
                    "Opened diagnostic log read-only",
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
                    self.notify_status("No incident report is selected");
                    return;
                };
                (
                    EditorReloadPolicy::DiagnosticsReport { path },
                    "Could not open diagnostics report",
                    "Opened diagnostics report read-only",
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
                    format!("{missing_message}: {error}"),
                    ui::NotificationTone::Critical,
                );
            }
        }
    }
}

#[cfg(test)]
mod diagnostics_shell_tests {
    use super::*;

    struct TestEditorTaskDriver;

    impl TestEditorTaskDriver {
        fn install(state: &mut ShellSession) -> Self {
            state.editor_task_runtime = ShellEditorTaskRuntime::new();
            Self
        }

        fn complete_next_load(&self, state: &mut ShellSession) {
            for _ in 0..400 {
                state.poll_editor_background_tasks(&platform::mock::UnsupportedPlatform);
                if state.editor_load_state.is_none() {
                    return;
                }
                std::thread::yield_now();
                std::thread::sleep(Duration::from_millis(5));
            }
            panic!("Editor load task did not finish in time");
        }
    }

    fn temporary_document(name: &str, contents: &[u8]) -> (std::path::PathBuf, std::path::PathBuf) {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let directory = std::env::temp_dir().join(format!(
            "tundra-shell-diagnostics-{}-{nonce}",
            std::process::id()
        ));
        std::fs::create_dir_all(&directory).unwrap();
        let directory = std::fs::canonicalize(directory).unwrap();
        let path = directory.join(name);
        std::fs::write(&path, contents).unwrap();
        (directory, path)
    }

    fn log_file(
        path: std::path::PathBuf,
        relative_path: &str,
    ) -> app::diagnostics::DiagnosticLogFile {
        let size_bytes = std::fs::metadata(&path)
            .map(|metadata| metadata.len())
            .unwrap_or(0);
        app::diagnostics::DiagnosticLogFile {
            path,
            relative_path: std::path::PathBuf::from(relative_path),
            modified_at: Utc::now(),
            size_bytes,
        }
    }

    fn session(role: UserRole) -> AuthSession {
        AuthSession {
            session_id: format!("{}-session", role.as_str()),
            user_id: format!("{}-id", role.as_str()),
            username: role.as_str().to_ascii_lowercase(),
            role,
            started_at_epoch_ms: 1,
        }
    }

    fn snapshot() -> app::diagnostics::DiagnosticsSnapshot {
        app::diagnostics::DiagnosticsSnapshot {
            scanned_at: Utc::now(),
            checks: vec![app::diagnostics::DiagnosticCheck {
                id: "path.data".to_string(),
                category: app::diagnostics::DiagnosticCategory::Paths,
                label: "Data path".to_string(),
                status: app::diagnostics::DiagnosticStatus::Warning,
                summary: "Directory is missing".to_string(),
                detail: "/private/example/data is missing".to_string(),
                remediation: Some("Create the missing directory".to_string()),
                repair: Some(app::diagnostics::DiagnosticsRepairAction::CreateDirectory {
                    label: "Data path".to_string(),
                    path: std::path::PathBuf::from("/private/example/data"),
                }),
            }],
            incidents: Vec::new(),
            logs: Vec::new(),
            warnings: Vec::new(),
        }
    }

    fn terminal_snapshot(
        status: app::diagnostics::DiagnosticStatus,
    ) -> app::diagnostics::DiagnosticsSnapshot {
        let mut snapshot = snapshot();
        snapshot.checks = vec![app::diagnostics::DiagnosticCheck {
            id: "environment.terminal".to_string(),
            category: app::diagnostics::DiagnosticCategory::Environment,
            label: "Terminal".to_string(),
            status,
            summary: "legacy terminal result".to_string(),
            detail: "legacy terminal result".to_string(),
            remediation: None,
            repair: None,
        }];
        snapshot
    }

    fn state(role: UserRole) -> ShellSession {
        let mut state = ShellSession::new(ShellLaunchConfig::default(), (120, 30));
        state.app.dispatch_at(
            app::AppCommand::SetAuthSession(Some(session(role))),
            Instant::now(),
        );
        state.app.dispatch_at(
            app::AppCommand::SetDiagnosticsSnapshot(Some(snapshot())),
            Instant::now(),
        );
        state.screen_stack = vec![ShellScreen::Home];
        state
    }

    fn update_diagnostics_snapshot(
        state: &mut ShellSession,
        update: impl FnOnce(&mut app::diagnostics::DiagnosticsSnapshot),
    ) {
        let mut snapshot = state.app.diagnostics_snapshot().cloned().unwrap();
        update(&mut snapshot);
        state.app.dispatch_at(
            app::AppCommand::SetDiagnosticsSnapshot(Some(snapshot)),
            Instant::now(),
        );
    }

    #[test]
    fn probed_graphics_protocol_controls_terminal_diagnostic_status() {
        let mut snapshot = terminal_snapshot(app::diagnostics::DiagnosticStatus::Pass);
        let text_only = platform::terminal_environment_check_with_graphics_protocol(
            platform::PlatformKind::Macos,
            None,
            None,
        );

        apply_terminal_environment_check(&mut snapshot, Some(&text_only));
        let check = &snapshot.checks[0];
        assert_eq!(check.status, app::diagnostics::DiagnosticStatus::Warning);
        assert!(check.summary.contains("text-only"));
        assert!(check.remediation.is_some());

        let graphics = platform::terminal_environment_check_with_graphics_protocol(
            platform::PlatformKind::Macos,
            None,
            Some("Sixel"),
        );
        apply_terminal_environment_check(&mut snapshot, Some(&graphics));
        let check = &snapshot.checks[0];
        assert_eq!(check.status, app::diagnostics::DiagnosticStatus::Pass);
        assert!(check.summary.contains("Sixel graphics protocol"));
        assert!(check.remediation.is_none());
    }

    fn install_temporary_storage(state: &mut ShellSession) -> std::path::PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "tundra-shell-diagnostics-storage-{}-{nonce}",
            std::process::id()
        ));
        let app_paths = platform::AppPaths::from_parts(
            root.join("config.toml"),
            root.join("state"),
            root.join("cache"),
            root.join("logs"),
            root.join("temp"),
        )
        .unwrap();
        state.storage_manager = Some(StorageManager::open(app_paths).unwrap().manager);
        root
    }

    #[test]
    fn diagnostics_redacts_user_details_and_enables_admin_repairs() {
        let mut user_state = state(UserRole::User);
        let (private_log_directory, private_log_path) =
            temporary_document("private.log", b"private");
        update_diagnostics_snapshot(&mut user_state, |snapshot| {
            snapshot
                .logs
                .push(log_file(private_log_path, "private.log"));
            snapshot.checks[0].summary = "/private/example/data cannot be opened".to_string();
        });
        let user = user_state.to_diagnostics_view_model();
        assert!(!user.can_view_details);
        assert!(!user.can_repair);
        assert_eq!(user.checks[0].summary, "Application path needs attention");
        assert!(!user.checks[0].summary.contains("/private"));
        assert!(user.checks[0].detail.is_empty());
        assert!(user.logs.is_empty());

        let mut admin_state = state(UserRole::Admin);
        let user_logs = user_state.app.diagnostics_snapshot().unwrap().logs.clone();
        update_diagnostics_snapshot(&mut admin_state, |snapshot| {
            snapshot.logs = user_logs;
        });
        let admin = admin_state.to_diagnostics_view_model();
        assert!(admin.can_view_details);
        assert!(admin.can_repair);
        assert!(admin.checks[0].detail.contains("/private/example/data"));
        assert_eq!(admin.logs[0].relative_path, "private.log");
        std::fs::remove_dir_all(private_log_directory).unwrap();
    }

    #[test]
    fn diagnostics_log_opens_read_only_in_editor_and_returns_to_logs() {
        let (directory, path) = temporary_document("application.log", b"first\nsecond\n");
        let mut state = state(UserRole::Admin);
        update_diagnostics_snapshot(&mut state, |snapshot| {
            snapshot
                .logs
                .push(log_file(path.clone(), "application.log"));
        });
        state.open_diagnostics();
        state.set_diagnostics_tab(ui::DiagnosticsTab::Logs);
        let editor_tasks = TestEditorTaskDriver::install(&mut state);

        let platform = platform::mock::UnsupportedPlatform;
        state.open_selected_diagnostics_report(&platform);
        editor_tasks.complete_next_load(&mut state);

        assert_eq!(state.active_screen(), ShellScreen::Editor);
        let editor = state.app.editor_state().unwrap();
        assert!(editor.is_read_only());
        assert_eq!(editor.source_buffer().as_deref(), Some("first\nsecond\n"));
        assert!(state.editor_read_session.is_some());

        state.handle_editor_key(
            KeyInput::with_phase(
                InputKey::Char('n'),
                InputModifiers {
                    control: true,
                    ..InputModifiers::none()
                },
                InputPhase::Press,
            ),
            &platform,
        );
        state.handle_editor_paste("mutating paste".to_string());
        state.activate_editor_toolbar(ui::EditorToolbarAction::Open, &platform);
        assert_eq!(state.active_screen(), ShellScreen::Editor);
        assert_eq!(
            state.app.editor_state().unwrap().source_buffer().as_deref(),
            Some("first\nsecond\n")
        );
        assert!(!state.app.editor_state().unwrap().is_dirty());

        state.request_editor_close(&platform);
        assert_eq!(state.active_screen(), ShellScreen::Diagnostics);
        assert_eq!(state.diagnostics_tab, ui::DiagnosticsTab::Logs);
        assert!(state.app.editor_state().is_none());
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn diagnostics_log_reload_replaces_only_after_a_successful_read() {
        let (directory, path) = temporary_document("reload.log", b"before\n");
        let mut state = state(UserRole::Admin);
        update_diagnostics_snapshot(&mut state, |snapshot| {
            snapshot.logs.push(log_file(path.clone(), "reload.log"));
        });
        state.open_diagnostics();
        state.set_diagnostics_tab(ui::DiagnosticsTab::Logs);
        let editor_tasks = TestEditorTaskDriver::install(&mut state);
        state.open_selected_diagnostics_report(&platform::mock::UnsupportedPlatform);
        editor_tasks.complete_next_load(&mut state);

        std::fs::write(&path, b"before\nafter\n").unwrap();
        state.handle_editor_key(
            KeyInput::with_phase(
                InputKey::Char('r'),
                InputModifiers::none(),
                InputPhase::Press,
            ),
            &platform::mock::UnsupportedPlatform,
        );
        editor_tasks.complete_next_load(&mut state);
        assert_eq!(
            state.app.editor_state().unwrap().source_buffer().as_deref(),
            Some("before\nafter\n")
        );

        std::fs::remove_file(&path).unwrap();
        state.handle_editor_key(
            KeyInput::with_phase(
                InputKey::Char('r'),
                InputModifiers::none(),
                InputPhase::Press,
            ),
            &platform::mock::UnsupportedPlatform,
        );
        editor_tasks.complete_next_load(&mut state);
        assert_eq!(
            state.app.editor_state().unwrap().source_buffer().as_deref(),
            Some("before\nafter\n")
        );
        assert!(
            state
                .editor_message
                .as_deref()
                .unwrap()
                .contains("Could not reload")
        );
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn diagnostics_log_reload_preserves_bottom_or_clamps_the_previous_scroll() {
        let before = (0..100)
            .map(|index| format!("before-{index}\n"))
            .collect::<String>();
        let (directory, path) = temporary_document("scroll.log", before.as_bytes());
        let mut state = state(UserRole::Admin);
        update_diagnostics_snapshot(&mut state, |snapshot| {
            snapshot.logs.push(log_file(path.clone(), "scroll.log"));
        });
        state.open_diagnostics();
        state.set_diagnostics_tab(ui::DiagnosticsTab::Logs);
        let platform = platform::mock::UnsupportedPlatform;
        let editor_tasks = TestEditorTaskDriver::install(&mut state);
        state.open_selected_diagnostics_report(&platform);
        editor_tasks.complete_next_load(&mut state);

        let editor_layout = |state: &ShellSession| {
            let area = Rect::new(0, 0, state.terminal_size.0, state.terminal_size.1);
            let editor_area = match ui::compute_shell_layout(area) {
                ui::ShellLayout::Compact(compact) => compact,
                ui::ShellLayout::Full { main, .. } => main,
            };
            ui::editor_layout(editor_area, &state.to_editor_view_model())
        };
        let layout = editor_layout(&state);
        assert_eq!(
            layout.visible_start,
            layout
                .document_line_count
                .saturating_sub(layout.visible_capacity)
        );

        state.app.dispatch_at(
            app::AppCommand::Editor(app::editor::EditorCommand::SelectAll),
            Instant::now(),
        );
        let _ = state.app.take_editor_effects();
        let appended = format!("{before}after-100\nafter-101\n");
        std::fs::write(&path, appended).unwrap();
        state.handle_editor_key(
            KeyInput::with_phase(
                InputKey::Char('r'),
                InputModifiers::none(),
                InputPhase::Press,
            ),
            &platform,
        );
        editor_tasks.complete_next_load(&mut state);
        let layout = editor_layout(&state);
        assert_eq!(
            layout.visible_start,
            layout
                .document_line_count
                .saturating_sub(layout.visible_capacity)
        );
        assert!(state.app.editor_state().unwrap().selection.is_none());

        let mut viewport = state.app.editor_state().unwrap().viewport;
        viewport.top_line = 2;
        state
            .app
            .dispatch_at(app::AppCommand::SetEditorViewport(viewport), Instant::now());
        std::fs::write(&path, before).unwrap();
        state.handle_editor_key(
            KeyInput::with_phase(
                InputKey::Char('r'),
                InputModifiers::none(),
                InputPhase::Press,
            ),
            &platform,
        );
        editor_tasks.complete_next_load(&mut state);
        assert_eq!(state.app.editor_state().unwrap().viewport.top_line, 2);
        state.request_editor_close(&platform);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn diagnostics_log_editor_loads_a_single_long_line_in_full() {
        let contents = vec![b'x'; 5 * 1024 * 1024 + 137];
        let (directory, path) = temporary_document("large.log", &contents);
        let mut state = state(UserRole::Admin);
        update_diagnostics_snapshot(&mut state, |snapshot| {
            snapshot.logs.push(log_file(path, "large.log"));
        });
        state.open_diagnostics();
        state.set_diagnostics_tab(ui::DiagnosticsTab::Logs);
        let editor_tasks = TestEditorTaskDriver::install(&mut state);

        state.open_selected_diagnostics_report(&platform::mock::UnsupportedPlatform);
        editor_tasks.complete_next_load(&mut state);

        let source = state.app.editor_state().unwrap().source_buffer().unwrap();
        assert_eq!(source.len(), contents.len());
        assert_eq!(source.as_bytes(), contents);
        let session = state.editor_read_session.as_ref().unwrap();
        assert_eq!(session.total_bytes, contents.len() as u64);
        state.request_editor_close(&platform::mock::UnsupportedPlatform);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn diagnostics_viewer_never_replaces_an_existing_unsaved_editor_document() {
        let (directory, path) = temporary_document("blocked.log", b"diagnostic\n");
        let mut state = state(UserRole::Admin);
        update_diagnostics_snapshot(&mut state, |snapshot| {
            snapshot.logs.push(log_file(path, "blocked.log"));
        });
        let mut editor = EditorState::new();
        editor.apply(app::editor::EditorCommand::InsertText(
            "unsaved".to_string(),
        ));
        state.app.dispatch_at(
            app::AppCommand::SetEditorState(Some(editor)),
            Instant::now(),
        );
        state.open_diagnostics();
        state.set_diagnostics_tab(ui::DiagnosticsTab::Logs);

        state.open_selected_diagnostics_report(&platform::mock::UnsupportedPlatform);

        assert_eq!(state.active_screen(), ShellScreen::Diagnostics);
        let editor = state.app.editor_state().unwrap();
        assert!(editor.is_dirty());
        assert_eq!(editor.export_text(), "unsaved");
        assert!(state.editor_read_session.is_none());
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn incident_prefers_text_report_and_falls_back_to_json_in_read_only_editor() {
        let (directory, text_path) = temporary_document("incident.txt", b"text report\n");
        let json_path = directory.join("incident.json");
        std::fs::write(&json_path, b"{\"source\":\"json\"}\n").unwrap();
        let mut state = state(UserRole::Admin);
        update_diagnostics_snapshot(&mut state, |snapshot| {
            snapshot.incidents.push(watchdog::IncidentReportSummary {
                incident_id: "incident-1".to_string(),
                occurred_at: Utc::now(),
                kind: watchdog::IncidentKind::Error,
                severity: watchdog::IncidentSeverity::Error,
                app: None,
                component: Some("test".to_string()),
                boundary: "test".to_string(),
                summary: "test incident".to_string(),
                recovery: watchdog::RecoveryOutcome::Pending,
                json_report_path: json_path.clone(),
                text_report_path: Some(text_path.clone()),
            })
        });
        state.open_diagnostics();
        state.set_diagnostics_tab(ui::DiagnosticsTab::Incidents);
        let platform = platform::mock::UnsupportedPlatform;
        let editor_tasks = TestEditorTaskDriver::install(&mut state);

        state.open_selected_diagnostics_report(&platform);
        editor_tasks.complete_next_load(&mut state);
        assert_eq!(
            state.app.editor_state().unwrap().source_buffer().as_deref(),
            Some("text report\n")
        );
        assert!(state.app.editor_state().unwrap().is_read_only());
        assert_eq!(state.app.editor_state().unwrap().viewport.top_line, 0);
        std::fs::write(&text_path, b"updated text report\n").unwrap();
        state.handle_editor_key(
            KeyInput::with_phase(
                InputKey::Char('r'),
                InputModifiers::none(),
                InputPhase::Press,
            ),
            &platform,
        );
        assert_eq!(
            state.app.editor_state().unwrap().source_buffer().as_deref(),
            Some("text report\n")
        );
        state.request_editor_close(&platform);

        update_diagnostics_snapshot(&mut state, |snapshot| {
            snapshot.incidents[0].text_report_path = None;
        });
        state.open_selected_diagnostics_report(&platform);
        editor_tasks.complete_next_load(&mut state);
        assert!(
            state.app.editor_state().is_some(),
            "JSON incident should load: {:?}",
            state.editor_message
        );
        assert_eq!(
            state.app.editor_state().unwrap().source_buffer().as_deref(),
            Some("{\"source\":\"json\"}\n")
        );
        state.request_editor_close(&platform);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn diagnostics_rescan_preserves_log_selection_by_relative_path() {
        let (directory, first_path) = temporary_document("first.log", b"first\n");
        let second_path = directory.join("second.log");
        std::fs::write(&second_path, b"second\n").unwrap();
        let first = log_file(first_path, "first.log");
        let second = log_file(second_path, "nested/second.log");
        let mut state = state(UserRole::Admin);
        update_diagnostics_snapshot(&mut state, |snapshot| {
            snapshot.logs = vec![first.clone(), second.clone()];
        });
        state.diagnostics_selected_log = 1;

        let mut replacement = snapshot();
        replacement.logs = vec![second, first];
        state.install_diagnostics_snapshot(replacement);

        assert_eq!(state.diagnostics_selected_log, 0);
        assert_eq!(
            state.app.diagnostics_snapshot().unwrap().logs[0].relative_path,
            std::path::PathBuf::from("nested/second.log")
        );
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn diagnostics_three_tab_routing_and_health_open_logs_are_consistent() {
        let mut state = state(UserRole::User);
        state.open_diagnostics();
        let target = RoutedTarget::Component(ShellComponent::Diagnostics);

        assert_eq!(
            state.route_diagnostics_key(&KeyInput::from_label("Tab")),
            (target.clone(), ShellCommand::DiagnosticsLogsTab)
        );
        state.set_diagnostics_tab(ui::DiagnosticsTab::Logs);
        assert_eq!(
            state.route_diagnostics_key(&KeyInput::from_label("Tab")),
            (target.clone(), ShellCommand::DiagnosticsIncidentsTab)
        );
        assert_eq!(
            state.route_diagnostics_key(&KeyInput::from_label("f")).1,
            ShellCommand::RecordInput
        );
        assert_eq!(
            state
                .route_diagnostics_key(&KeyInput::from_label("Enter"))
                .1,
            ShellCommand::DiagnosticsOpenReport
        );
        assert_eq!(
            state.route_diagnostics_key(&KeyInput::from_label("e")).1,
            ShellCommand::DiagnosticsOpenLogsInExplorer
        );

        state.set_diagnostics_tab(ui::DiagnosticsTab::Health);
        state.open_selected_diagnostics_report(&platform::mock::UnsupportedPlatform);
        assert_eq!(state.diagnostics_tab, ui::DiagnosticsTab::Logs);
        assert!(!state.notification_has_active_modal());
    }

    #[test]
    fn diagnostics_opens_log_directory_in_explorer_and_returns_on_close() {
        let mut state = state(UserRole::Admin);
        let root = install_temporary_storage(&mut state);
        let logs_path = state
            .storage_manager
            .as_ref()
            .unwrap()
            .layout()
            .logs_path
            .clone();
        std::fs::write(logs_path.join("application.log"), b"application\n").unwrap();
        state.open_diagnostics();

        state.open_diagnostics_logs_in_explorer(&platform::mock::UnsupportedPlatform);

        assert_eq!(state.active_screen(), ShellScreen::Explorer);
        assert_eq!(state.explorer_purpose, ExplorerPurpose::DiagnosticsLogs);
        assert_eq!(state.app.explorer_state().unwrap().current_path, logs_path);
        assert!(
            state
                .app
                .explorer_state()
                .unwrap()
                .entries
                .iter()
                .any(|entry| entry.name == "application.log")
        );

        state.close_explorer();

        assert_eq!(state.active_screen(), ShellScreen::Diagnostics);
        assert_eq!(state.focused_component, ShellComponent::Diagnostics);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn diagnostics_log_directory_explorer_requires_detail_permission() {
        let mut state = state(UserRole::User);
        state.open_diagnostics();

        state.open_diagnostics_logs_in_explorer(&platform::mock::UnsupportedPlatform);

        assert_eq!(state.active_screen(), ShellScreen::Diagnostics);
        assert!(
            state
                .app
                .notification_center()
                .alert()
                .as_deref()
                .is_some_and(|message| message.contains("Only administrators"))
        );
    }

    #[test]
    fn diagnostics_navigation_and_repair_preview_are_modal() {
        let mut state = state(UserRole::Admin);
        state.open_diagnostics();
        assert_eq!(state.active_screen(), ShellScreen::Diagnostics);

        state.preview_selected_diagnostics_repair();
        assert_eq!(state.diagnostics_repair_preview.len(), 1);
        assert_eq!(
            state.focus_order(),
            vec![ShellComponent::DiagnosticsRepairDialog]
        );

        state.cancel_diagnostics_repair_preview();
        state.set_diagnostics_tab(ui::DiagnosticsTab::Incidents);
        assert_eq!(state.diagnostics_tab, ui::DiagnosticsTab::Incidents);
        state.close_diagnostics();
        assert_eq!(state.active_screen(), ShellScreen::Home);
    }

    #[test]
    fn diagnostics_scrollbar_thumb_drags_to_the_end_without_moving_selection() {
        let mut state = state(UserRole::Admin);
        let template = state.app.diagnostics_snapshot().unwrap().checks[0].clone();
        update_diagnostics_snapshot(&mut state, |snapshot| {
            snapshot.checks = (0..40)
                .map(|index| {
                    let mut check = template.clone();
                    check.id = format!("check-{index}");
                    check.label = format!("Check {index}");
                    check
                })
                .collect();
        });
        state.open_diagnostics();

        let area = Rect::new(0, 0, state.terminal_size.0, state.terminal_size.1);
        let ui::ShellLayout::Full { main, .. } = ui::compute_shell_layout(area) else {
            panic!("Diagnostics scrollbar test requires a full layout");
        };
        let layout = ui::diagnostics_layout(main, &state.to_diagnostics_view_model());
        let scrollbar = layout
            .list_scrollbar
            .expect("overflowing Diagnostics scrollbar");
        let grab = (
            scrollbar.thumb.x,
            scrollbar.thumb.y.saturating_add(scrollbar.thumb.height / 2),
        );
        let bottom = (
            scrollbar.track.x,
            scrollbar.track.bottom().saturating_sub(1),
        );
        let platform = platform::mock::UnsupportedPlatform;

        state.apply_input_with_platform(
            InputEvent::mouse_down(PointerButton::Left, grab),
            &platform,
        );
        state.apply_input_with_platform(
            InputEvent::mouse_drag(PointerButton::Left, bottom),
            &platform,
        );
        state.apply_input_with_platform(
            InputEvent::mouse_up(PointerButton::Left, bottom),
            &platform,
        );

        let model = state.to_diagnostics_view_model();
        let final_layout = ui::diagnostics_layout(main, &model);
        assert_eq!(model.selected_check, 0);
        assert!(model.list_window_is_explicit);
        assert_eq!(
            final_layout.visible_start,
            model
                .item_count()
                .saturating_sub(final_layout.visible_capacity)
        );

        let scrollbar = final_layout
            .list_scrollbar
            .expect("Diagnostics scrollbar after dragging down");
        let grab = (
            scrollbar.thumb.x,
            scrollbar.thumb.y.saturating_add(scrollbar.thumb.height / 2),
        );
        let top = (scrollbar.track.x, scrollbar.track.y);
        state.apply_input_with_platform(
            InputEvent::mouse_down(PointerButton::Left, grab),
            &platform,
        );
        state
            .apply_input_with_platform(InputEvent::mouse_drag(PointerButton::Left, top), &platform);
        state.apply_input_with_platform(InputEvent::mouse_up(PointerButton::Left, top), &platform);

        let model = state.to_diagnostics_view_model();
        assert_eq!(model.selected_check, 0);
        assert!(model.list_window_is_explicit);
        assert_eq!(ui::diagnostics_layout(main, &model).visible_start, 0);
    }

    #[test]
    fn restart_requirement_disables_follow_up_repairs() {
        let mut state = state(UserRole::Admin);
        state.diagnostics_restart_required = true;

        let model = state.to_diagnostics_view_model();
        assert!(model.restart_required);
        assert!(!model.can_repair);

        assert!(!state.logout_to_lockscreen_at(Instant::now()));
        assert!(state.diagnostics_restart_required);
        assert!(state.auth_session().is_some());
        assert!(!state.return_to_lockscreen_requested);
    }
}
