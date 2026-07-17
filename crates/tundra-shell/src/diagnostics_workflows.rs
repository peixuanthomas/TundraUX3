#[derive(Clone)]
struct ShellDiagnosticsTaskRuntime {
    shared: Arc<ShellDiagnosticsTaskShared>,
}

struct ShellDiagnosticsTaskShared {
    engine: Mutex<Option<tundra_apps::diagnostics::DiagnosticsTaskRuntime>>,
    repair_initiator: Mutex<Option<AuthSession>>,
    storage: StorageManager,
    process: Option<ProcessWatchdog>,
    watchdog: Option<AppWatchdog>,
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
    fn new(storage: StorageManager) -> Self {
        let process = ProcessWatchdog::global();
        let watchdog = process.as_ref().and_then(|process| {
            process
                .register_app(tundra_apps::diagnostics::diagnostics_watchdog_descriptor())
                .ok()
        });
        Self::with_services(storage, process, watchdog)
    }

    fn new_managed(
        storage: StorageManager,
        process: ProcessWatchdog,
        watchdog: AppWatchdog,
    ) -> Self {
        Self::with_services(storage, Some(process), Some(watchdog))
    }

    fn with_services(
        storage: StorageManager,
        process: Option<ProcessWatchdog>,
        watchdog: Option<AppWatchdog>,
    ) -> Self {
        Self {
            shared: Arc::new(ShellDiagnosticsTaskShared {
                engine: Mutex::new(None),
                repair_initiator: Mutex::new(None),
                storage,
                process,
                watchdog,
            }),
        }
    }

    fn ensure_engine(
        &self,
    ) -> Result<
        std::sync::MutexGuard<'_, Option<tundra_apps::diagnostics::DiagnosticsTaskRuntime>>,
        String,
    > {
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
            let platform: Arc<dyn Platform> = Arc::from(tundra_platform::native_platform());
            *engine = Some(
                tundra_apps::diagnostics::DiagnosticsTaskRuntime::new_managed(
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

    fn request_scan(&self) -> Result<(), String> {
        let engine = self.ensure_engine()?;
        engine
            .as_ref()
            .expect("Diagnostics engine initialized")
            .request_scan()
            .map_err(|error| error.to_string())
    }

    fn request_repair(
        &self,
        actions: Vec<tundra_apps::diagnostics::DiagnosticsRepairAction>,
        initiator: AuthSession,
    ) -> Result<(), String> {
        let mut pending_initiator = self
            .shared
            .repair_initiator
            .lock()
            .map_err(|_| "Diagnostics repair initiator lock poisoned".to_string())?;
        if pending_initiator.is_some() {
            return Err("a diagnostics repair is already pending completion".to_string());
        }
        let engine = self.ensure_engine()?;
        engine
            .as_ref()
            .expect("Diagnostics engine initialized")
            .request_repair(actions)
            .map_err(|error| error.to_string())?;
        *pending_initiator = Some(initiator);
        Ok(())
    }

    fn is_busy(&self) -> bool {
        self.shared
            .engine
            .lock()
            .ok()
            .and_then(|engine| engine.as_ref().map(|engine| engine.is_busy()))
            .unwrap_or(false)
    }

    fn drain_events(&self) -> Vec<tundra_apps::diagnostics::DiagnosticsTaskEvent> {
        let Ok(engine) = self.shared.engine.lock() else {
            return Vec::new();
        };
        engine
            .as_ref()
            .map(|engine| engine.drain_events())
            .unwrap_or_default()
    }

    fn restart_required(&self) -> bool {
        self.shared
            .engine
            .lock()
            .ok()
            .and_then(|engine| engine.as_ref().map(|engine| engine.restart_required()))
            .unwrap_or(false)
    }

    fn take_repair_initiator(&self) -> Option<AuthSession> {
        self.shared
            .repair_initiator
            .lock()
            .ok()
            .and_then(|mut initiator| initiator.take())
    }
}

impl ShellState {
    fn open_diagnostics(&mut self) {
        if self.is_strict_guest() {
            self.notify_alert_with_tone(
                "Diagnostics requires an authenticated account",
                tundra_ui::NotificationTone::Warning,
            );
            return;
        }
        self.diagnostics_restart_required = self.diagnostics_restart_is_required();
        self.screen_stack.push(ShellScreen::Diagnostics);
        self.focused_component = ShellComponent::Diagnostics;
        self.diagnostics_tab = tundra_ui::DiagnosticsTab::Health;
        self.diagnostics_list_window_start = 0;
        self.diagnostics_list_window_is_explicit = false;
        self.clear_diagnostics_scrollbar_drag();
        self.diagnostics_feedback = None;
        if self.diagnostics_task_runtime.is_some() {
            self.request_diagnostics_scan();
        } else if self.diagnostics_snapshot.is_none() {
            self.diagnostics_feedback = Some("Diagnostics runtime is unavailable".to_string());
        }
        self.refresh_hit_map();
    }

    fn close_diagnostics(&mut self) {
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

    fn request_diagnostics_scan(&mut self) {
        if self.diagnostics_restart_is_required() {
            self.diagnostics_restart_required = true;
            self.notify_alert_with_tone(
                "Restart TundraUX before running another diagnostics scan",
                tundra_ui::NotificationTone::Warning,
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
                self.notify_alert_with_tone(error, tundra_ui::NotificationTone::Critical);
            }
        }
    }

    fn drain_diagnostics_events(&mut self) {
        let events = self
            .diagnostics_task_runtime
            .as_ref()
            .map(ShellDiagnosticsTaskRuntime::drain_events)
            .unwrap_or_default();
        for event in events {
            match event {
                tundra_apps::diagnostics::DiagnosticsTaskEvent::ScanCompleted(result) => {
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
                            self.notify_alert_with_tone(
                                message,
                                tundra_ui::NotificationTone::Critical,
                            );
                        }
                    }
                    if self.diagnostics_rescan_pending && !self.diagnostics_restart_is_required() {
                        self.diagnostics_rescan_pending = false;
                        self.request_diagnostics_scan();
                    }
                }
                tundra_apps::diagnostics::DiagnosticsTaskEvent::RepairProgress {
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
                tundra_apps::diagnostics::DiagnosticsTaskEvent::RepairCompleted {
                    results,
                    snapshot,
                    restart_required,
                } => {
                    self.diagnostics_scanning = false;
                    let initiator = self
                        .diagnostics_task_runtime
                        .as_ref()
                        .and_then(ShellDiagnosticsTaskRuntime::take_repair_initiator);
                    self.diagnostics_repair_preview.clear();
                    self.diagnostics_repair_selected = 0;
                    self.diagnostics_repair_scroll_offset = 0;
                    self.diagnostics_repair_confirm_selected = true;
                    self.record_diagnostics_repair_audits(&results, initiator.as_ref());
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
                            format!(", {backups} backup(s) recorded in the audit log")
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
                            tundra_ui::NotificationTone::Warning,
                            vec![
                                ShellNotificationAction::new("exit", "Exit now")
                                    .with_follow_up(ShellCommand::ConfirmExit),
                                ShellNotificationAction::new("review", "Review results").cancel(),
                            ],
                        );
                    } else if failed > 0 {
                        self.notify_alert_with_tone(
                            format!("{failed} diagnostics repair action(s) failed"),
                            tundra_ui::NotificationTone::Warning,
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

    fn diagnostics_can_view_details(&self) -> bool {
        PermissionService::new(self.debug_policy)
            .authorize(
                self.auth_session.as_ref(),
                PermissionAction::ViewDiagnosticsDetails,
                None,
            )
            .allowed
    }

    fn diagnostics_can_repair(&self) -> bool {
        !self.diagnostics_restart_is_required()
            && !self.diagnostics_scanning
            && !self
                .diagnostics_task_runtime
                .as_ref()
                .is_some_and(ShellDiagnosticsTaskRuntime::is_busy)
            && PermissionService::new(self.debug_policy)
                .authorize(
                    self.auth_session.as_ref(),
                    PermissionAction::RepairDiagnostics,
                    None,
                )
                .allowed
    }

    fn diagnostics_restart_is_required(&self) -> bool {
        self.diagnostics_restart_required
            || self
                .diagnostics_task_runtime
                .as_ref()
                .is_some_and(ShellDiagnosticsTaskRuntime::restart_required)
    }

    fn diagnostics_item_count(&self) -> usize {
        let Some(snapshot) = &self.diagnostics_snapshot else {
            return 0;
        };
        match self.diagnostics_tab {
            tundra_ui::DiagnosticsTab::Health => snapshot.checks.len(),
            tundra_ui::DiagnosticsTab::Logs => snapshot.logs.len(),
            tundra_ui::DiagnosticsTab::Incidents => snapshot.incidents.len(),
        }
    }

    fn clamp_diagnostics_selection(&mut self) {
        let check_count = self
            .diagnostics_snapshot
            .as_ref()
            .map(|snapshot| snapshot.checks.len())
            .unwrap_or(0);
        self.diagnostics_selected_check = if check_count == 0 {
            0
        } else {
            self.diagnostics_selected_check.min(check_count - 1)
        };
        let log_count = self
            .diagnostics_snapshot
            .as_ref()
            .map(|snapshot| snapshot.logs.len())
            .unwrap_or(0);
        self.diagnostics_selected_log = if log_count == 0 {
            0
        } else {
            self.diagnostics_selected_log.min(log_count - 1)
        };
        let incident_count = self
            .diagnostics_snapshot
            .as_ref()
            .map(|snapshot| snapshot.incidents.len())
            .unwrap_or(0);
        self.diagnostics_selected_incident = if incident_count == 0 {
            0
        } else {
            self.diagnostics_selected_incident.min(incident_count - 1)
        };
    }

    fn install_diagnostics_snapshot(
        &mut self,
        snapshot: tundra_apps::diagnostics::DiagnosticsSnapshot,
    ) {
        let selected_log_path = self
            .diagnostics_snapshot
            .as_ref()
            .and_then(|current| current.logs.get(self.diagnostics_selected_log))
            .map(|log| log.relative_path.clone());
        self.diagnostics_snapshot = Some(snapshot);
        if let Some(relative_path) = selected_log_path
            && let Some(index) = self.diagnostics_snapshot.as_ref().and_then(|current| {
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

    fn move_diagnostics_selection(&mut self, delta: isize) {
        let count = self.diagnostics_item_count();
        if count == 0 {
            return;
        }
        let selected = match self.diagnostics_tab {
            tundra_ui::DiagnosticsTab::Health => &mut self.diagnostics_selected_check,
            tundra_ui::DiagnosticsTab::Logs => &mut self.diagnostics_selected_log,
            tundra_ui::DiagnosticsTab::Incidents => &mut self.diagnostics_selected_incident,
        };
        *selected =
            ((*selected as isize) + delta).clamp(0, count.saturating_sub(1) as isize) as usize;
        self.diagnostics_list_window_is_explicit = false;
    }

    fn set_diagnostics_tab(&mut self, tab: tundra_ui::DiagnosticsTab) {
        self.diagnostics_tab = tab;
        self.diagnostics_list_window_start = 0;
        self.diagnostics_list_window_is_explicit = false;
        self.clear_diagnostics_scrollbar_drag();
        self.clamp_diagnostics_selection();
    }

    fn select_diagnostics_index(&mut self, index: usize) {
        let count = self.diagnostics_item_count();
        if count == 0 {
            return;
        }
        let index = index.min(count - 1);
        match self.diagnostics_tab {
            tundra_ui::DiagnosticsTab::Health => self.diagnostics_selected_check = index,
            tundra_ui::DiagnosticsTab::Logs => self.diagnostics_selected_log = index,
            tundra_ui::DiagnosticsTab::Incidents => self.diagnostics_selected_incident = index,
        }
        self.diagnostics_list_window_is_explicit = false;
    }

    fn begin_diagnostics_scrollbar_drag(&mut self, coordinates: CellPosition) {
        let area = Rect::new(0, 0, self.terminal_size.0, self.terminal_size.1);
        let tundra_ui::ShellLayout::Full { main, .. } = tundra_ui::compute_shell_layout(area)
        else {
            return;
        };
        let layout = tundra_ui::diagnostics_layout(main, &self.to_diagnostics_view_model());
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

    fn drag_diagnostics_scrollbar(&mut self, coordinates: CellPosition) {
        let Some(ScrollbarDragState::Diagnostics { grab_offset }) = self.scrollbar_drag else {
            return;
        };
        let area = Rect::new(0, 0, self.terminal_size.0, self.terminal_size.1);
        let tundra_ui::ShellLayout::Full { main, .. } = tundra_ui::compute_shell_layout(area)
        else {
            return;
        };
        let model = self.to_diagnostics_view_model();
        let layout = tundra_ui::diagnostics_layout(main, &model);
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

    fn clear_diagnostics_scrollbar_drag(&mut self) -> bool {
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

    fn preview_selected_diagnostics_repair(&mut self) {
        if !self.ensure_diagnostics_repair_authorized() {
            return;
        }
        let repair = self
            .diagnostics_snapshot
            .as_ref()
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

    fn preview_all_diagnostics_repairs(&mut self) {
        if !self.ensure_diagnostics_repair_authorized() {
            return;
        }
        let plan = self
            .diagnostics_snapshot
            .as_ref()
            .map(tundra_apps::diagnostics::DiagnosticsSnapshot::repair_plan)
            .unwrap_or_default();
        if plan.is_empty() {
            self.notify_status("No automatic repairs are available");
        } else {
            self.diagnostics_repair_preview = plan;
            self.reset_diagnostics_repair_dialog_selection();
        }
        self.refresh_hit_map();
    }

    fn cancel_diagnostics_repair_preview(&mut self) {
        self.diagnostics_repair_preview.clear();
        self.reset_diagnostics_repair_dialog_selection();
        self.refresh_hit_map();
    }

    fn reset_diagnostics_repair_dialog_selection(&mut self) {
        self.diagnostics_repair_selected = 0;
        self.diagnostics_repair_scroll_offset = 0;
        self.diagnostics_repair_confirm_selected = true;
    }

    fn move_diagnostics_repair_selection(&mut self, delta: isize) {
        if self.diagnostics_repair_preview.is_empty() {
            return;
        }
        self.diagnostics_repair_selected = ((self.diagnostics_repair_selected as isize) + delta)
            .clamp(
                0,
                self.diagnostics_repair_preview.len().saturating_sub(1) as isize,
            ) as usize;
    }

    fn select_diagnostics_repair_item(&mut self, index: usize) {
        if self.diagnostics_repair_preview.is_empty() {
            return;
        }
        self.diagnostics_repair_selected =
            index.min(self.diagnostics_repair_preview.len().saturating_sub(1));
    }

    fn confirm_diagnostics_repair(&mut self) {
        if !self.ensure_diagnostics_repair_authorized() {
            return;
        }
        let actions = std::mem::take(&mut self.diagnostics_repair_preview);
        self.reset_diagnostics_repair_dialog_selection();
        if actions.is_empty() {
            return;
        }
        let Some(initiator) = self.auth_session.clone() else {
            self.diagnostics_repair_preview = actions;
            self.notify_alert_with_tone(
                "Diagnostics repair requires an authenticated administrator",
                tundra_ui::NotificationTone::Warning,
            );
            return;
        };
        let result = self
            .diagnostics_task_runtime
            .as_ref()
            .ok_or_else(|| "Diagnostics runtime is unavailable".to_string())
            .and_then(|runtime| runtime.request_repair(actions.clone(), initiator));
        match result {
            Ok(()) => {
                self.diagnostics_scanning = true;
                self.diagnostics_feedback =
                    Some(format!("Starting {} repair action(s)…", actions.len()));
            }
            Err(error) => {
                self.diagnostics_repair_preview = actions;
                self.notify_alert_with_tone(error, tundra_ui::NotificationTone::Critical);
            }
        }
        self.refresh_hit_map();
    }

    fn ensure_diagnostics_repair_authorized(&mut self) -> bool {
        let permission = PermissionService::new(self.debug_policy).authorize(
            self.auth_session.as_ref(),
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
        self.record_diagnostics_audit(
            PermissionAction::RepairDiagnostics,
            "diagnostics.repair",
            AuditOutcome::Denied,
            Some(&reason),
        );
        self.notify_alert_with_tone(
            format!("Diagnostics repair denied: {reason}"),
            tundra_ui::NotificationTone::Warning,
        );
        false
    }

    fn copy_diagnostics_summary(&mut self, platform: &dyn Platform) {
        let Some(snapshot) = &self.diagnostics_snapshot else {
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
                if full {
                    self.record_diagnostics_audit(
                        PermissionAction::ViewDiagnosticsDetails,
                        "diagnostics.summary.copy",
                        AuditOutcome::Success,
                        None,
                    );
                }
                self.notify_toast("Copied diagnostics summary");
            }
            Err(error) => {
                if full {
                    self.record_diagnostics_audit(
                        PermissionAction::ViewDiagnosticsDetails,
                        "diagnostics.summary.copy",
                        AuditOutcome::Failure,
                        Some(&error.to_string()),
                    );
                }
                self.notify_alert_with_tone(
                    format!("Could not copy diagnostics summary: {error}"),
                    tundra_ui::NotificationTone::Critical,
                );
            }
        }
    }

    fn open_diagnostics_logs_in_explorer(&mut self, platform: &dyn Platform) {
        const AUDIT_RESOURCE: &str = "diagnostics.logs.explore";

        if !self.diagnostics_can_view_details() {
            self.record_diagnostics_audit(
                PermissionAction::ViewDiagnosticsDetails,
                AUDIT_RESOURCE,
                AuditOutcome::Denied,
                Some("insufficient_role"),
            );
            self.notify_alert_with_tone(
                "Only administrators can explore the diagnostic log folder",
                tundra_ui::NotificationTone::Warning,
            );
            return;
        }

        let Some(storage) = self.storage_manager.clone() else {
            self.notify_alert_with_tone(
                "Diagnostics log directory is unavailable",
                tundra_ui::NotificationTone::Critical,
            );
            return;
        };
        let logs_path = storage.layout().logs_path.clone();
        if !logs_path.is_dir() {
            self.record_diagnostics_audit(
                PermissionAction::ViewDiagnosticsDetails,
                AUDIT_RESOURCE,
                AuditOutcome::Failure,
                Some("log_directory_unavailable"),
            );
            self.notify_alert_with_tone(
                format!("Diagnostics log directory is unavailable: {}", logs_path.display()),
                tundra_ui::NotificationTone::Critical,
            );
            return;
        }

        self.open_explorer_at(
            platform,
            &storage,
            logs_path,
            ExplorerPurpose::DiagnosticsLogs,
        );
        self.record_diagnostics_audit(
            PermissionAction::ViewDiagnosticsDetails,
            AUDIT_RESOURCE,
            AuditOutcome::Success,
            None,
        );
        self.notify_toast("Opened diagnostic log folder in Explorer");
    }

    fn open_selected_diagnostics_report(&mut self, _platform: &dyn Platform) {
        if self.diagnostics_tab == tundra_ui::DiagnosticsTab::Health {
            self.set_diagnostics_tab(tundra_ui::DiagnosticsTab::Logs);
            self.notify_status("Diagnostics logs");
            return;
        }
        if !self.diagnostics_can_view_details() {
            let audit_resource = match self.diagnostics_tab {
                tundra_ui::DiagnosticsTab::Logs => "diagnostics.log.open",
                tundra_ui::DiagnosticsTab::Incidents => "diagnostics.report.open",
                tundra_ui::DiagnosticsTab::Health => unreachable!(),
            };
            self.record_diagnostics_audit(
                PermissionAction::ViewDiagnosticsDetails,
                audit_resource,
                AuditOutcome::Denied,
                Some("insufficient_role"),
            );
            self.notify_alert_with_tone(
                "Only administrators can open diagnostic logs and reports",
                tundra_ui::NotificationTone::Warning,
            );
            return;
        }

        let (reload, audit_resource, missing_message, opened_message) = match self.diagnostics_tab {
            tundra_ui::DiagnosticsTab::Logs => {
                let path = self
                    .diagnostics_snapshot
                    .as_ref()
                    .and_then(|snapshot| snapshot.logs.get(self.diagnostics_selected_log))
                    .map(|log| log.path.clone());
                let Some(path) = path else {
                    self.notify_status("No diagnostic log is selected");
                    return;
                };
                (
                    EditorReloadPolicy::Log { path },
                    "diagnostics.log.open",
                    "Could not open diagnostic log",
                    "Opened diagnostic log read-only",
                )
            }
            tundra_ui::DiagnosticsTab::Incidents => {
                let path = self
                    .diagnostics_snapshot
                    .as_ref()
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
                    "diagnostics.report.open",
                    "Could not open diagnostics report",
                    "Opened diagnostics report read-only",
                )
            }
            tundra_ui::DiagnosticsTab::Health => unreachable!(),
        };

        match self.open_diagnostics_editor(reload) {
            Ok(()) => {
                self.record_diagnostics_audit(
                    PermissionAction::ViewDiagnosticsDetails,
                    audit_resource,
                    AuditOutcome::Success,
                    None,
                );
                self.notify_toast(opened_message);
            }
            Err(error) => {
                self.record_diagnostics_audit(
                    PermissionAction::ViewDiagnosticsDetails,
                    audit_resource,
                    AuditOutcome::Failure,
                    Some("open_failed"),
                );
                self.notify_alert_with_tone(
                    format!("{missing_message}: {error}"),
                    tundra_ui::NotificationTone::Critical,
                );
            }
        }
    }

    fn record_diagnostics_repair_audits(
        &mut self,
        results: &[tundra_apps::diagnostics::DiagnosticsRepairResult],
        initiator: Option<&AuthSession>,
    ) {
        for result in results {
            let resource = format!("diagnostics.repair.{}", result.action.label());
            let detail = if result.success {
                result
                    .backup_path
                    .as_ref()
                    .map(|path| format!("backup={}", path.display()))
            } else {
                Some(result.message.clone())
            };
            self.record_diagnostics_audit_for_session(
                initiator,
                PermissionAction::RepairDiagnostics,
                &resource,
                if result.success {
                    AuditOutcome::Success
                } else {
                    AuditOutcome::Failure
                },
                detail.as_deref(),
            );
        }
    }

    fn record_diagnostics_audit(
        &self,
        action: PermissionAction,
        resource: &str,
        outcome: AuditOutcome,
        reason: Option<&str>,
    ) {
        self.record_diagnostics_audit_for_session(
            self.auth_session.as_ref(),
            action,
            resource,
            outcome,
            reason,
        );
    }

    fn record_diagnostics_audit_for_session(
        &self,
        session: Option<&AuthSession>,
        action: PermissionAction,
        resource: &str,
        outcome: AuditOutcome,
        reason: Option<&str>,
    ) {
        let Some(storage) = self.storage_manager.clone() else {
            return;
        };
        let _ = AuditService::with_permission_service(
            storage,
            PermissionService::new(self.debug_policy),
        )
        .record(session, action, Some(resource), outcome, reason);
    }
}

#[cfg(test)]
mod diagnostics_shell_tests {
    use super::*;

    struct TestEditorTaskDriver;

    impl TestEditorTaskDriver {
        fn install(state: &mut ShellState) -> Self {
            state.editor_task_runtime = ShellEditorTaskRuntime::new();
            Self
        }

        fn complete_next_load(&self, state: &mut ShellState) {
            for _ in 0..400 {
                state.poll_editor_background_tasks(&tundra_platform::mock::UnsupportedPlatform);
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
    ) -> tundra_apps::diagnostics::DiagnosticLogFile {
        let size_bytes = std::fs::metadata(&path)
            .map(|metadata| metadata.len())
            .unwrap_or(0);
        tundra_apps::diagnostics::DiagnosticLogFile {
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

    fn snapshot() -> tundra_apps::diagnostics::DiagnosticsSnapshot {
        tundra_apps::diagnostics::DiagnosticsSnapshot {
            scanned_at: Utc::now(),
            checks: vec![tundra_apps::diagnostics::DiagnosticCheck {
                id: "path.data".to_string(),
                category: tundra_apps::diagnostics::DiagnosticCategory::Paths,
                label: "Data path".to_string(),
                status: tundra_apps::diagnostics::DiagnosticStatus::Warning,
                summary: "Directory is missing".to_string(),
                detail: "/private/example/data is missing".to_string(),
                remediation: Some("Create the missing directory".to_string()),
                repair: Some(
                    tundra_apps::diagnostics::DiagnosticsRepairAction::CreateDirectory {
                        label: "Data path".to_string(),
                        path: std::path::PathBuf::from("/private/example/data"),
                    },
                ),
            }],
            incidents: Vec::new(),
            logs: Vec::new(),
            warnings: Vec::new(),
        }
    }

    fn state(role: UserRole) -> ShellState {
        let mut state = ShellState::new(ShellLaunchConfig::default(), (120, 30));
        state.auth_session = Some(session(role));
        state.diagnostics_snapshot = Some(snapshot());
        state.screen_stack = vec![ShellScreen::Home];
        state
    }

    fn install_temporary_storage(state: &mut ShellState) -> std::path::PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "tundra-shell-diagnostics-storage-{}-{nonce}",
            std::process::id()
        ));
        let app_paths = tundra_platform::AppPaths::from_parts(
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
        user_state
            .diagnostics_snapshot
            .as_mut()
            .unwrap()
            .logs
            .push(log_file(private_log_path, "private.log"));
        user_state.diagnostics_snapshot.as_mut().unwrap().checks[0].summary =
            "/private/example/data cannot be opened".to_string();
        let user = user_state.to_diagnostics_view_model();
        assert!(!user.can_view_details);
        assert!(!user.can_repair);
        assert_eq!(user.checks[0].summary, "Application path needs attention");
        assert!(!user.checks[0].summary.contains("/private"));
        assert!(user.checks[0].detail.is_empty());
        assert!(user.logs.is_empty());

        let mut admin_state = state(UserRole::Admin);
        admin_state.diagnostics_snapshot.as_mut().unwrap().logs = user_state
            .diagnostics_snapshot
            .as_ref()
            .unwrap()
            .logs
            .clone();
        let admin = admin_state.to_diagnostics_view_model();
        assert!(admin.can_view_details);
        assert!(admin.can_repair);
        assert!(admin.checks[0].detail.contains("/private/example/data"));
        assert_eq!(admin.logs[0].relative_path, "private.log");
        std::fs::remove_dir_all(private_log_directory).unwrap();
    }

    #[test]
    fn diagnostics_log_opens_read_only_in_editor_and_returns_to_logs() {
        let (directory, path) = temporary_document("audit.v1.log", b"first\nsecond\n");
        let mut state = state(UserRole::Admin);
        state
            .diagnostics_snapshot
            .as_mut()
            .unwrap()
            .logs
            .push(log_file(path.clone(), "audit.v1.log"));
        state.open_diagnostics();
        state.set_diagnostics_tab(tundra_ui::DiagnosticsTab::Logs);
        let editor_tasks = TestEditorTaskDriver::install(&mut state);

        let platform = tundra_platform::mock::UnsupportedPlatform;
        state.open_selected_diagnostics_report(&platform);
        editor_tasks.complete_next_load(&mut state);

        assert_eq!(state.active_screen(), ShellScreen::Editor);
        let editor = state.editor_state.as_ref().unwrap();
        assert!(editor.is_read_only());
        assert_eq!(editor.source_buffer().as_deref(), Some("first\nsecond\n"));
        assert!(state.editor_read_session.is_some());

        state.handle_editor_key(
            KeyInput::new(
                InputKey::Character('n'),
                InputModifiers {
                    control: true,
                    ..InputModifiers::none()
                },
                InputPhase::Press,
            ),
            &platform,
        );
        state.handle_editor_paste("mutating paste".to_string());
        state.activate_editor_toolbar(tundra_ui::EditorToolbarAction::Open, &platform);
        assert_eq!(state.active_screen(), ShellScreen::Editor);
        assert_eq!(
            state
                .editor_state
                .as_ref()
                .unwrap()
                .source_buffer()
                .as_deref(),
            Some("first\nsecond\n")
        );
        assert!(!state.editor_state.as_ref().unwrap().is_dirty());

        state.request_editor_close(&platform);
        assert_eq!(state.active_screen(), ShellScreen::Diagnostics);
        assert_eq!(state.diagnostics_tab, tundra_ui::DiagnosticsTab::Logs);
        assert!(state.editor_state.is_none());
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn diagnostics_log_reload_replaces_only_after_a_successful_read() {
        let (directory, path) = temporary_document("reload.log", b"before\n");
        let mut state = state(UserRole::Admin);
        state
            .diagnostics_snapshot
            .as_mut()
            .unwrap()
            .logs
            .push(log_file(path.clone(), "reload.log"));
        state.open_diagnostics();
        state.set_diagnostics_tab(tundra_ui::DiagnosticsTab::Logs);
        let editor_tasks = TestEditorTaskDriver::install(&mut state);
        state.open_selected_diagnostics_report(&tundra_platform::mock::UnsupportedPlatform);
        editor_tasks.complete_next_load(&mut state);

        std::fs::write(&path, b"before\nafter\n").unwrap();
        state.handle_editor_key(
            KeyInput::new(
                InputKey::Character('r'),
                InputModifiers::none(),
                InputPhase::Press,
            ),
            &tundra_platform::mock::UnsupportedPlatform,
        );
        editor_tasks.complete_next_load(&mut state);
        assert_eq!(
            state
                .editor_state
                .as_ref()
                .unwrap()
                .source_buffer()
                .as_deref(),
            Some("before\nafter\n")
        );

        std::fs::remove_file(&path).unwrap();
        state.handle_editor_key(
            KeyInput::new(
                InputKey::Character('r'),
                InputModifiers::none(),
                InputPhase::Press,
            ),
            &tundra_platform::mock::UnsupportedPlatform,
        );
        editor_tasks.complete_next_load(&mut state);
        assert_eq!(
            state
                .editor_state
                .as_ref()
                .unwrap()
                .source_buffer()
                .as_deref(),
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
        state
            .diagnostics_snapshot
            .as_mut()
            .unwrap()
            .logs
            .push(log_file(path.clone(), "scroll.log"));
        state.open_diagnostics();
        state.set_diagnostics_tab(tundra_ui::DiagnosticsTab::Logs);
        let platform = tundra_platform::mock::UnsupportedPlatform;
        let editor_tasks = TestEditorTaskDriver::install(&mut state);
        state.open_selected_diagnostics_report(&platform);
        editor_tasks.complete_next_load(&mut state);

        let editor_layout = |state: &ShellState| {
            let area = Rect::new(0, 0, state.terminal_size.0, state.terminal_size.1);
            let editor_area = match tundra_ui::compute_shell_layout(area) {
                tundra_ui::ShellLayout::Compact(compact) => compact,
                tundra_ui::ShellLayout::Full { main, .. } => main,
            };
            tundra_ui::editor_layout(editor_area, &state.to_editor_view_model())
        };
        let layout = editor_layout(&state);
        assert_eq!(
            layout.visible_start,
            layout
                .document_line_count
                .saturating_sub(layout.visible_capacity)
        );

        state
            .editor_state
            .as_mut()
            .unwrap()
            .apply(tundra_apps::editor::EditorCommand::SelectAll);
        let appended = format!("{before}after-100\nafter-101\n");
        std::fs::write(&path, appended).unwrap();
        state.handle_editor_key(
            KeyInput::new(
                InputKey::Character('r'),
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
        assert!(state.editor_state.as_ref().unwrap().selection.is_none());

        state.editor_state.as_mut().unwrap().viewport.top_line = 2;
        std::fs::write(&path, before).unwrap();
        state.handle_editor_key(
            KeyInput::new(
                InputKey::Character('r'),
                InputModifiers::none(),
                InputPhase::Press,
            ),
            &platform,
        );
        editor_tasks.complete_next_load(&mut state);
        assert_eq!(state.editor_state.as_ref().unwrap().viewport.top_line, 2);
        state.request_editor_close(&platform);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn diagnostics_log_editor_loads_a_single_long_line_in_full() {
        let contents = vec![b'x'; 5 * 1024 * 1024 + 137];
        let (directory, path) = temporary_document("large.log", &contents);
        let mut state = state(UserRole::Admin);
        state
            .diagnostics_snapshot
            .as_mut()
            .unwrap()
            .logs
            .push(log_file(path, "large.log"));
        state.open_diagnostics();
        state.set_diagnostics_tab(tundra_ui::DiagnosticsTab::Logs);
        let editor_tasks = TestEditorTaskDriver::install(&mut state);

        state.open_selected_diagnostics_report(&tundra_platform::mock::UnsupportedPlatform);
        editor_tasks.complete_next_load(&mut state);

        let source = state
            .editor_state
            .as_ref()
            .unwrap()
            .source_buffer()
            .unwrap();
        assert_eq!(source.len(), contents.len());
        assert_eq!(source.as_bytes(), contents);
        let session = state.editor_read_session.as_ref().unwrap();
        assert_eq!(session.total_bytes, contents.len() as u64);
        state.request_editor_close(&tundra_platform::mock::UnsupportedPlatform);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn diagnostics_viewer_never_replaces_an_existing_unsaved_editor_document() {
        let (directory, path) = temporary_document("blocked.log", b"diagnostic\n");
        let mut state = state(UserRole::Admin);
        state
            .diagnostics_snapshot
            .as_mut()
            .unwrap()
            .logs
            .push(log_file(path, "blocked.log"));
        let mut editor = EditorState::new();
        editor.apply(tundra_apps::editor::EditorCommand::InsertText(
            "unsaved".to_string(),
        ));
        state.editor_state = Some(editor);
        state.open_diagnostics();
        state.set_diagnostics_tab(tundra_ui::DiagnosticsTab::Logs);

        state.open_selected_diagnostics_report(&tundra_platform::mock::UnsupportedPlatform);

        assert_eq!(state.active_screen(), ShellScreen::Diagnostics);
        let editor = state.editor_state.as_ref().unwrap();
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
        state.diagnostics_snapshot.as_mut().unwrap().incidents.push(
            tundra_watchdog::IncidentReportSummary {
                incident_id: "incident-1".to_string(),
                occurred_at: Utc::now(),
                kind: tundra_watchdog::IncidentKind::Error,
                severity: tundra_watchdog::IncidentSeverity::Error,
                app: None,
                component: Some("test".to_string()),
                boundary: "test".to_string(),
                summary: "test incident".to_string(),
                recovery: tundra_watchdog::RecoveryOutcome::Pending,
                json_report_path: json_path.clone(),
                text_report_path: Some(text_path.clone()),
            },
        );
        state.open_diagnostics();
        state.set_diagnostics_tab(tundra_ui::DiagnosticsTab::Incidents);
        let platform = tundra_platform::mock::UnsupportedPlatform;
        let editor_tasks = TestEditorTaskDriver::install(&mut state);

        state.open_selected_diagnostics_report(&platform);
        editor_tasks.complete_next_load(&mut state);
        assert_eq!(
            state
                .editor_state
                .as_ref()
                .unwrap()
                .source_buffer()
                .as_deref(),
            Some("text report\n")
        );
        assert!(state.editor_state.as_ref().unwrap().is_read_only());
        assert_eq!(state.editor_state.as_ref().unwrap().viewport.top_line, 0);
        std::fs::write(&text_path, b"updated text report\n").unwrap();
        state.handle_editor_key(
            KeyInput::new(
                InputKey::Character('r'),
                InputModifiers::none(),
                InputPhase::Press,
            ),
            &platform,
        );
        assert_eq!(
            state
                .editor_state
                .as_ref()
                .unwrap()
                .source_buffer()
                .as_deref(),
            Some("text report\n")
        );
        state.request_editor_close(&platform);

        state.diagnostics_snapshot.as_mut().unwrap().incidents[0].text_report_path = None;
        state.open_selected_diagnostics_report(&platform);
        editor_tasks.complete_next_load(&mut state);
        assert!(
            state.editor_state.is_some(),
            "JSON incident should load: {:?}",
            state.editor_message
        );
        assert_eq!(
            state
                .editor_state
                .as_ref()
                .unwrap()
                .source_buffer()
                .as_deref(),
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
        state.diagnostics_snapshot.as_mut().unwrap().logs = vec![first.clone(), second.clone()];
        state.diagnostics_selected_log = 1;

        let mut replacement = snapshot();
        replacement.logs = vec![second, first];
        state.install_diagnostics_snapshot(replacement);

        assert_eq!(state.diagnostics_selected_log, 0);
        assert_eq!(
            state.diagnostics_snapshot.as_ref().unwrap().logs[0].relative_path,
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
        state.set_diagnostics_tab(tundra_ui::DiagnosticsTab::Logs);
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

        state.set_diagnostics_tab(tundra_ui::DiagnosticsTab::Health);
        state.open_selected_diagnostics_report(&tundra_platform::mock::UnsupportedPlatform);
        assert_eq!(state.diagnostics_tab, tundra_ui::DiagnosticsTab::Logs);
        assert!(!state.notifications.has_active_modal());
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
        std::fs::write(logs_path.join("audit.v1.log"), b"audit\n").unwrap();
        state.open_diagnostics();

        state.open_diagnostics_logs_in_explorer(&tundra_platform::mock::UnsupportedPlatform);

        assert_eq!(state.active_screen(), ShellScreen::Explorer);
        assert_eq!(state.explorer_purpose, ExplorerPurpose::DiagnosticsLogs);
        assert_eq!(
            state.explorer_state.as_ref().unwrap().current_path,
            logs_path
        );
        assert!(
            state
                .explorer_state
                .as_ref()
                .unwrap()
                .entries
                .iter()
                .any(|entry| entry.name == "audit.v1.log")
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

        state.open_diagnostics_logs_in_explorer(&tundra_platform::mock::UnsupportedPlatform);

        assert_eq!(state.active_screen(), ShellScreen::Diagnostics);
        assert!(
            state
                .notifications
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
        state.set_diagnostics_tab(tundra_ui::DiagnosticsTab::Incidents);
        assert_eq!(state.diagnostics_tab, tundra_ui::DiagnosticsTab::Incidents);
        state.close_diagnostics();
        assert_eq!(state.active_screen(), ShellScreen::Home);
    }

    #[test]
    fn diagnostics_scrollbar_thumb_drags_to_the_end_without_moving_selection() {
        let mut state = state(UserRole::Admin);
        let template = state.diagnostics_snapshot.as_ref().unwrap().checks[0].clone();
        state.diagnostics_snapshot.as_mut().unwrap().checks = (0..40)
            .map(|index| {
                let mut check = template.clone();
                check.id = format!("check-{index}");
                check.label = format!("Check {index}");
                check
            })
            .collect();
        state.open_diagnostics();

        let area = Rect::new(0, 0, state.terminal_size.0, state.terminal_size.1);
        let tundra_ui::ShellLayout::Full { main, .. } = tundra_ui::compute_shell_layout(area)
        else {
            panic!("Diagnostics scrollbar test requires a full layout");
        };
        let layout = tundra_ui::diagnostics_layout(main, &state.to_diagnostics_view_model());
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
        let platform = tundra_platform::mock::UnsupportedPlatform;

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
        let final_layout = tundra_ui::diagnostics_layout(main, &model);
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
        state.apply_input_with_platform(
            InputEvent::mouse_drag(PointerButton::Left, top),
            &platform,
        );
        state.apply_input_with_platform(
            InputEvent::mouse_up(PointerButton::Left, top),
            &platform,
        );

        let model = state.to_diagnostics_view_model();
        assert_eq!(model.selected_check, 0);
        assert!(model.list_window_is_explicit);
        assert_eq!(
            tundra_ui::diagnostics_layout(main, &model).visible_start,
            0
        );
    }

    #[test]
    fn restart_requirement_disables_follow_up_repairs() {
        let mut state = state(UserRole::Admin);
        state.diagnostics_restart_required = true;

        let model = state.to_diagnostics_view_model();
        assert!(model.restart_required);
        assert!(!model.can_repair);

        state.logout_at(Instant::now());
        assert!(state.diagnostics_restart_required);
        assert!(state.auth_session.is_some());
    }
}
