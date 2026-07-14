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
        self.diagnostics_feedback = None;
        if self.diagnostics_task_runtime.is_some() {
            self.request_diagnostics_scan();
        } else if self.diagnostics_snapshot.is_none() {
            self.diagnostics_feedback = Some("Diagnostics runtime is unavailable".to_string());
        }
        self.refresh_hit_map();
    }

    fn close_diagnostics(&mut self) {
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
                            self.diagnostics_snapshot = Some(snapshot);
                            self.clamp_diagnostics_selection();
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
                        self.diagnostics_snapshot = Some(snapshot);
                        self.clamp_diagnostics_selection();
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

    fn move_diagnostics_selection(&mut self, delta: isize) {
        let count = self.diagnostics_item_count();
        if count == 0 {
            return;
        }
        let selected = match self.diagnostics_tab {
            tundra_ui::DiagnosticsTab::Health => &mut self.diagnostics_selected_check,
            tundra_ui::DiagnosticsTab::Incidents => &mut self.diagnostics_selected_incident,
        };
        *selected =
            ((*selected as isize) + delta).clamp(0, count.saturating_sub(1) as isize) as usize;
    }

    fn set_diagnostics_tab(&mut self, tab: tundra_ui::DiagnosticsTab) {
        self.diagnostics_tab = tab;
        self.diagnostics_list_window_start = 0;
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
            tundra_ui::DiagnosticsTab::Incidents => self.diagnostics_selected_incident = index,
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

    fn open_selected_diagnostics_report(&mut self, platform: &dyn Platform) {
        if !self.diagnostics_can_view_details() {
            self.record_diagnostics_audit(
                PermissionAction::ViewDiagnosticsDetails,
                "diagnostics.report.open",
                AuditOutcome::Denied,
                Some("insufficient_role"),
            );
            self.notify_alert_with_tone(
                "Only administrators can open diagnostics reports",
                tundra_ui::NotificationTone::Warning,
            );
            return;
        }
        if self.diagnostics_tab == tundra_ui::DiagnosticsTab::Health {
            let path = self
                .storage_manager
                .as_ref()
                .map(|storage| storage.layout().logs_path.clone());
            let Some(path) = path else {
                self.notify_status("Diagnostics log directory is unavailable");
                return;
            };
            match platform.open_path(&path) {
                Ok(()) => {
                    self.record_diagnostics_audit(
                        PermissionAction::ViewDiagnosticsDetails,
                        "diagnostics.logs.open",
                        AuditOutcome::Success,
                        None,
                    );
                    self.notify_toast("Opened diagnostics logs");
                }
                Err(error) => {
                    self.record_diagnostics_audit(
                        PermissionAction::ViewDiagnosticsDetails,
                        "diagnostics.logs.open",
                        AuditOutcome::Failure,
                        Some(&error.to_string()),
                    );
                    self.notify_alert_with_tone(
                        format!("Could not open diagnostics logs: {error}"),
                        tundra_ui::NotificationTone::Critical,
                    );
                }
            }
            return;
        }
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
        match platform.open_path(&path) {
            Ok(()) => {
                self.record_diagnostics_audit(
                    PermissionAction::ViewDiagnosticsDetails,
                    "diagnostics.report.open",
                    AuditOutcome::Success,
                    None,
                );
                self.notify_toast("Opened diagnostics report");
            }
            Err(error) => {
                self.record_diagnostics_audit(
                    PermissionAction::ViewDiagnosticsDetails,
                    "diagnostics.report.open",
                    AuditOutcome::Failure,
                    Some(&error.to_string()),
                );
                self.notify_alert_with_tone(
                    format!("Could not open diagnostics report: {error}"),
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

    #[test]
    fn diagnostics_redacts_user_details_and_enables_admin_repairs() {
        let mut user_state = state(UserRole::User);
        user_state.diagnostics_snapshot.as_mut().unwrap().checks[0].summary =
            "/private/example/data cannot be opened".to_string();
        let user = user_state.to_diagnostics_view_model();
        assert!(!user.can_view_details);
        assert!(!user.can_repair);
        assert_eq!(user.checks[0].summary, "Application path needs attention");
        assert!(!user.checks[0].summary.contains("/private"));
        assert!(user.checks[0].detail.is_empty());

        let admin = state(UserRole::Admin).to_diagnostics_view_model();
        assert!(admin.can_view_details);
        assert!(admin.can_repair);
        assert!(admin.checks[0].detail.contains("/private/example/data"));
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
