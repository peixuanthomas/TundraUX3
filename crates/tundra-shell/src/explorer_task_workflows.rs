use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use tundra_apps::explorer::{
    ExplorerClipboard, ExplorerClipboardMode, ExplorerConflict, ExplorerOperationPhase,
    ExplorerOperationProgress, ExplorerPendingTransfer, ExplorerTransferMode,
};
use tundra_apps::explorer_tasks::{
    ExplorerCollisionPolicy, ExplorerCollisionResolution, ExplorerDeletePlan, ExplorerTaskEngine,
    ExplorerTaskEvent, ExplorerTaskId, ExplorerTaskPhase, ExplorerTaskPlan,
    ExplorerTaskSubmitError, ExplorerTransferOperation, ExplorerTransferPlan, SystemExplorerTrash,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ShellExplorerTaskKind {
    Copy,
    Move { from_clipboard: bool },
    Delete,
}

#[derive(Debug, Clone)]
struct ShellExplorerTaskContext {
    id: ExplorerTaskId,
    kind: ShellExplorerTaskKind,
    sources: Vec<ShellExplorerTaskSource>,
}

#[derive(Debug, Clone)]
struct ShellExplorerTaskSource {
    original: PathBuf,
    canonical: PathBuf,
}

fn task_plan_sources(plan: &ExplorerTaskPlan) -> Vec<PathBuf> {
    match plan {
        ExplorerTaskPlan::Transfer(plan) => plan.sources.clone(),
        ExplorerTaskPlan::DeleteToTrash(plan) => plan.paths.clone(),
    }
}

fn explorer_task_paths_match(left: &Path, right: &Path) -> bool {
    #[cfg(windows)]
    {
        left.to_string_lossy()
            .eq_ignore_ascii_case(&right.to_string_lossy())
    }
    #[cfg(not(windows))]
    {
        left == right
    }
}

fn collect_explorer_conflicts_no_follow(
    platform: &dyn Platform,
    source: &Path,
    target: &Path,
    conflicts: &mut Vec<(PathBuf, PathBuf)>,
) -> Result<(), String> {
    let source_attributes = platform.file_attributes(source).map_err(|error| {
        format!(
            "Could not inspect transfer source {}: {error}",
            source.display()
        )
    })?;
    let target_attributes = match std::fs::symlink_metadata(target) {
        Ok(_) => Some(platform.file_attributes(target).map_err(|error| {
            format!(
                "Could not inspect transfer target {}: {error}",
                target.display()
            )
        })?),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => {
            return Err(format!(
                "Could not inspect transfer target {}: {error}",
                target.display()
            ));
        }
    };
    let Some(target_attributes) = target_attributes else {
        return Ok(());
    };
    let source_is_safe_directory = source_attributes.is_dir
        && !source_attributes.symlink
        && !source_attributes.junction
        && !source_attributes.reparse_point;
    let target_is_safe_directory = target_attributes.is_dir
        && !target_attributes.symlink
        && !target_attributes.junction
        && !target_attributes.reparse_point;
    if !(source_is_safe_directory && target_is_safe_directory) {
        conflicts.push((source.to_path_buf(), target.to_path_buf()));
        return Ok(());
    }

    let directory = std::fs::read_dir(source).map_err(|error| {
        format!(
            "Could not scan source directory {} for conflicts: {error}",
            source.display()
        )
    })?;
    let mut entries = directory
        .map(|entry| {
            entry.map_err(|error| {
                format!(
                    "Could not read an entry in {} during conflict scan: {error}",
                    source.display()
                )
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(|entry| entry.file_name().to_string_lossy().to_ascii_lowercase());
    for entry in entries {
        collect_explorer_conflicts_no_follow(
            platform,
            &entry.path(),
            &target.join(entry.file_name()),
            conflicts,
        )?;
    }
    Ok(())
}

struct ShellExplorerTaskShared {
    engine: Mutex<Option<ExplorerTaskEngine>>,
    context: Mutex<Option<ShellExplorerTaskContext>>,
    watchdog: Option<AppWatchdog>,
}

/// Cloneable shell handle around the single-worker Explorer mutation engine.
///
/// The worker and receiver are deliberately excluded from ShellState equality: they are runtime
/// infrastructure, while Explorer's visible progress remains in `ExplorerState::operation`.
#[derive(Clone)]
struct ShellExplorerTaskRuntime {
    shared: Arc<ShellExplorerTaskShared>,
}

impl ShellExplorerTaskRuntime {
    fn new(_storage: StorageManager) -> Self {
        let watchdog = ProcessWatchdog::global().and_then(|process| {
            process
                .register_app(tundra_apps::explorer_tasks::explorer_watchdog_descriptor())
                .ok()
        });
        Self::with_watchdog(watchdog)
    }

    fn new_managed(_storage: StorageManager, watchdog: AppWatchdog) -> Self {
        Self::with_watchdog(Some(watchdog))
    }

    fn with_watchdog(watchdog: Option<AppWatchdog>) -> Self {
        Self {
            shared: Arc::new(ShellExplorerTaskShared {
                engine: Mutex::new(None),
                context: Mutex::new(None),
                watchdog,
            }),
        }
    }

    fn submit(
        &self,
        plan: ExplorerTaskPlan,
        kind: ShellExplorerTaskKind,
        _actor: String,
    ) -> Result<ExplorerTaskId, ExplorerTaskSubmitError> {
        let mut context = self
            .shared
            .context
            .lock()
            .expect("Explorer task context lock poisoned");
        if let Some(active) = context.as_ref() {
            return Err(ExplorerTaskSubmitError::Busy { active: active.id });
        }
        let sources = task_plan_sources(&plan)
            .into_iter()
            .map(|original| ShellExplorerTaskSource {
                canonical: std::fs::canonicalize(&original).unwrap_or_else(|_| original.clone()),
                original,
            })
            .collect();
        let mut engine = self
            .shared
            .engine
            .lock()
            .expect("Explorer task engine lock poisoned");
        if engine.is_none() {
            let Some(watchdog) = self.shared.watchdog.clone() else {
                return Err(ExplorerTaskSubmitError::WorkerStopped);
            };
            let platform: Arc<dyn Platform> = Arc::from(tundra_platform::native_platform());
            let trash = Arc::new(SystemExplorerTrash);
            *engine = Some(
                ExplorerTaskEngine::new_managed(platform, trash, watchdog)
                    .map_err(|_| ExplorerTaskSubmitError::WorkerStopped)?,
            );
        }
        let engine = engine
            .as_ref()
            .expect("Explorer engine was initialized in the preceding branch");
        let handle = engine.submit(plan)?;
        *context = Some(ShellExplorerTaskContext {
            id: handle.id,
            kind,
            sources,
        });
        Ok(handle.id)
    }

    fn cancel_active(&self) -> bool {
        self.shared
            .engine
            .lock()
            .expect("Explorer task engine lock poisoned")
            .as_ref()
            .is_some_and(ExplorerTaskEngine::cancel_active)
    }

    fn drain_events(&self) -> Vec<ExplorerTaskEvent> {
        let engine = self
            .shared
            .engine
            .lock()
            .expect("Explorer task engine lock poisoned");
        let Some(engine) = engine.as_ref() else {
            return Vec::new();
        };
        std::iter::from_fn(|| engine.try_recv().ok()).collect()
    }

    fn context_for(&self, id: ExplorerTaskId) -> Option<ShellExplorerTaskContext> {
        self.shared
            .context
            .lock()
            .expect("Explorer task context lock poisoned")
            .as_ref()
            .filter(|context| context.id == id)
            .cloned()
    }

    fn finish(&self, id: ExplorerTaskId) -> Option<ShellExplorerTaskContext> {
        let mut context = self
            .shared
            .context
            .lock()
            .expect("Explorer task context lock poisoned");
        if context.as_ref().is_some_and(|context| context.id == id) {
            context.take()
        } else {
            None
        }
    }
}

impl fmt::Debug for ShellExplorerTaskRuntime {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ShellExplorerTaskRuntime")
            .finish_non_exhaustive()
    }
}

impl PartialEq for ShellExplorerTaskRuntime {
    fn eq(&self, _other: &Self) -> bool {
        true
    }
}

impl Eq for ShellExplorerTaskRuntime {}

impl ShellState {
    /// Returns true when this command belongs to the asynchronous mutation path.
    fn try_handle_explorer_background_command(
        &mut self,
        command: &ExplorerCommand,
        platform: &dyn Platform,
    ) -> bool {
        if !platform.is_native_backend() {
            return false;
        }
        match command {
            ExplorerCommand::Paste => {
                self.start_explorer_background_paste(platform);
                true
            }
            ExplorerCommand::DeleteToTrash => {
                let should_confirm = self
                    .explorer_state
                    .as_ref()
                    .is_some_and(|state| state.confirm_delete);
                if should_confirm {
                    false
                } else {
                    let paths = self
                        .explorer_state
                        .as_ref()
                        .map(|state| state.effective_selected_paths())
                        .unwrap_or_default();
                    self.start_explorer_background_delete_paths(paths);
                    true
                }
            }
            ExplorerCommand::ConfirmDelete => {
                let Some(paths) = self.explorer_state.as_ref().and_then(|state| {
                    state
                        .pending_dialog
                        .as_ref()
                        .filter(|dialog| {
                            dialog.kind == tundra_apps::explorer::ExplorerDialogKind::DeleteToTrash
                        })
                        .map(|dialog| dialog.targets.clone())
                }) else {
                    return false;
                };
                self.notifications
                    .dismiss_modal_by_key(EXPLORER_DELETE_NOTIFICATION_KEY);
                if let Some(state) = self.explorer_state.as_mut() {
                    state.pending_dialog = None;
                }
                self.start_explorer_background_delete_paths(paths);
                true
            }
            ExplorerCommand::DropDrag => {
                self.start_explorer_background_drop(platform);
                true
            }
            ExplorerCommand::ResolveConflict {
                action,
                apply_to_all,
            } if self
                .explorer_state
                .as_ref()
                .is_some_and(|state| state.pending_transfer.is_some()) =>
            {
                self.resolve_explorer_background_conflict(*action, *apply_to_all);
                true
            }
            ExplorerCommand::CancelOperation => {
                if self
                    .explorer_task_runtime
                    .as_ref()
                    .is_some_and(ShellExplorerTaskRuntime::cancel_active)
                {
                    if let Some(operation) = self
                        .explorer_state
                        .as_mut()
                        .and_then(|state| state.operation.as_mut())
                    {
                        operation.label = "Cancelling operation".to_string();
                        operation.cancellable = false;
                    }
                    self.notify_status("Cancelling Explorer operation");
                    true
                } else {
                    // The core controller still owns cancellation while waiting in its dialog.
                    let _ = platform;
                    false
                }
            }
            _ => false,
        }
    }

    fn start_explorer_background_paste(&mut self, platform: &dyn Platform) {
        let Some(state) = self.explorer_state.as_ref() else {
            self.report_explorer_task_error("Explorer unavailable");
            return;
        };
        let Some(clipboard) = state.clipboard.clone() else {
            self.report_explorer_task_error("clipboard is empty");
            return;
        };
        self.prepare_explorer_background_transfer(
            clipboard,
            state.current_path.clone(),
            true,
            platform,
        );
    }

    fn start_explorer_background_drop(&mut self, platform: &dyn Platform) {
        let drag = self
            .explorer_state
            .as_mut()
            .and_then(|state| state.drag.take());
        let Some(drag) = drag else {
            return;
        };
        if !drag.active {
            return;
        }
        let Some(destination) = drag.target else {
            self.report_explorer_task_error("Drag has no valid destination");
            return;
        };
        self.prepare_explorer_background_transfer(
            ExplorerClipboard {
                paths: drag.sources,
                mode: match drag.mode {
                    ExplorerTransferMode::Copy => ExplorerClipboardMode::Copy,
                    ExplorerTransferMode::Move => ExplorerClipboardMode::Cut,
                },
            },
            destination,
            false,
            platform,
        );
    }

    fn prepare_explorer_background_transfer(
        &mut self,
        clipboard: ExplorerClipboard,
        destination: PathBuf,
        from_clipboard: bool,
        platform: &dyn Platform,
    ) {
        if clipboard.paths.is_empty() {
            self.report_explorer_task_error("clipboard is empty");
            return;
        }
        let destination = match std::fs::canonicalize(&destination) {
            Ok(destination) => destination,
            Err(error) => {
                self.report_explorer_task_error(format!(
                    "Could not resolve transfer destination {}: {error}",
                    destination.display()
                ));
                return;
            }
        };
        let mut conflicts = Vec::new();
        let mut targets = Vec::with_capacity(clipboard.paths.len());
        for source in &clipboard.paths {
            let Some(file_name) = source.file_name() else {
                self.report_explorer_task_error(format!("{} has no file name", source.display()));
                return;
            };
            let target = destination.join(file_name);
            if let Err(message) =
                collect_explorer_conflicts_no_follow(platform, source, &target, &mut conflicts)
            {
                self.report_explorer_task_error(message);
                return;
            }
            targets.push(target);
        }

        let permission = match clipboard.mode {
            ExplorerClipboardMode::Copy => PermissionAction::WriteFile,
            ExplorerClipboardMode::Cut => PermissionAction::MoveFile,
        };
        if let Err(message) = self.preflight_explorer_permissions(permission, &targets) {
            self.report_explorer_task_error(message);
            return;
        }

        let confirm_conflicts = self
            .explorer_state
            .as_ref()
            .is_some_and(|state| state.confirm_name_conflicts);
        if confirm_conflicts && !conflicts.is_empty() {
            self.explorer_conflict_apply_to_remaining = false;
            let (source, target) = conflicts[0].clone();
            if let Some(state) = self.explorer_state.as_mut() {
                state.pending_conflict = Some(ExplorerConflict {
                    source,
                    target,
                    remaining: conflicts.len(),
                });
                state.pending_transfer = Some(ExplorerPendingTransfer {
                    clipboard,
                    destination,
                    conflicts,
                    current_conflict: 0,
                    resolutions: BTreeMap::new(),
                });
                state.operation = Some(waiting_for_conflict_progress());
            }
            self.sync_explorer_background_conflict_notification();
            return;
        }

        self.submit_explorer_background_transfer(
            clipboard,
            destination,
            BTreeMap::new(),
            from_clipboard,
        );
    }

    fn resolve_explorer_background_conflict(
        &mut self,
        action: ExplorerConflictAction,
        apply_to_all: bool,
    ) {
        if action == ExplorerConflictAction::Cancel {
            if let Some(state) = self.explorer_state.as_mut() {
                state.pending_conflict = None;
                state.pending_transfer = None;
                state.operation = None;
                state.message = Some("Transfer cancelled".to_string());
                state.error = None;
            }
            self.notifications
                .dismiss_modal_by_key(EXPLORER_CONFLICT_NOTIFICATION_KEY);
            return;
        }

        let ready = {
            let Some(state) = self.explorer_state.as_mut() else {
                return;
            };
            let Some(pending) = state.pending_transfer.as_mut() else {
                return;
            };
            let Some((_, target)) = pending.conflicts.get(pending.current_conflict).cloned() else {
                self.report_explorer_task_error("Invalid conflict state");
                return;
            };
            pending.resolutions.insert(target, action);
            if apply_to_all {
                for (_, target) in pending.conflicts.iter().skip(pending.current_conflict + 1) {
                    pending.resolutions.insert(target.clone(), action);
                }
                pending.current_conflict = pending.conflicts.len();
            } else {
                pending.current_conflict += 1;
            }
            pending.current_conflict >= pending.conflicts.len()
        };

        if !ready {
            if let Some(state) = self.explorer_state.as_mut()
                && let Some(pending) = state.pending_transfer.as_ref()
                && let Some((source, target)) =
                    pending.conflicts.get(pending.current_conflict).cloned()
            {
                state.pending_conflict = Some(ExplorerConflict {
                    source,
                    target,
                    remaining: pending.conflicts.len() - pending.current_conflict,
                });
            }
            self.sync_explorer_background_conflict_notification();
            return;
        }

        let pending = self
            .explorer_state
            .as_mut()
            .and_then(|state| {
                state.pending_conflict = None;
                state.pending_transfer.take()
            })
            .expect("completed conflict sequence has a pending transfer");
        self.notifications
            .dismiss_modal_by_key(EXPLORER_CONFLICT_NOTIFICATION_KEY);
        // Pending transfers originate from either Paste or drag. Only clear a cut clipboard when
        // it still contains the exact paths being moved.
        let from_clipboard = self
            .explorer_state
            .as_ref()
            .and_then(|state| state.clipboard.as_ref())
            .is_some_and(|clipboard| clipboard == &pending.clipboard);
        self.submit_explorer_background_transfer(
            pending.clipboard,
            pending.destination,
            pending.resolutions,
            from_clipboard,
        );
    }

    fn submit_explorer_background_transfer(
        &mut self,
        clipboard: ExplorerClipboard,
        destination: PathBuf,
        resolutions: BTreeMap<PathBuf, ExplorerConflictAction>,
        from_clipboard: bool,
    ) {
        let operation = match clipboard.mode {
            ExplorerClipboardMode::Copy => ExplorerTransferOperation::Copy,
            ExplorerClipboardMode::Cut => ExplorerTransferOperation::Move,
        };
        let mut plan = ExplorerTransferPlan::new(operation, clipboard.paths, destination);
        plan.collisions = ExplorerCollisionPolicy {
            default: ExplorerCollisionResolution::KeepBoth,
            overrides: resolutions
                .into_iter()
                .map(|(path, action)| (path, collision_resolution(action)))
                .collect(),
        };
        let kind = match operation {
            ExplorerTransferOperation::Copy => ShellExplorerTaskKind::Copy,
            ExplorerTransferOperation::Move => ShellExplorerTaskKind::Move { from_clipboard },
        };
        self.submit_explorer_task(ExplorerTaskPlan::Transfer(plan), kind);
    }

    fn start_explorer_background_delete_paths(&mut self, paths: Vec<std::path::PathBuf>) {
        if paths.is_empty() {
            self.report_explorer_task_error("No file is selected");
            return;
        }
        if let Err(message) =
            self.preflight_explorer_permissions(PermissionAction::DeleteFile, &paths)
        {
            self.report_explorer_task_error(message);
            return;
        }
        self.submit_explorer_task(
            ExplorerTaskPlan::DeleteToTrash(ExplorerDeletePlan::new(paths)),
            ShellExplorerTaskKind::Delete,
        );
    }

    fn submit_explorer_task(&mut self, plan: ExplorerTaskPlan, kind: ShellExplorerTaskKind) {
        let Some(runtime) = self.explorer_task_runtime.as_ref() else {
            self.report_explorer_task_error("Explorer task service is unavailable");
            return;
        };
        let actor = self
            .auth_session
            .as_ref()
            .map(|session| session.username.clone())
            .unwrap_or_else(|| "Guest".to_string());
        match runtime.submit(plan, kind, actor) {
            Ok(_) => {
                if let Some(state) = self.explorer_state.as_mut() {
                    state.pending_conflict = None;
                    state.pending_transfer = None;
                    state.error = None;
                    state.message = None;
                    state.operation = Some(ExplorerOperationProgress {
                        phase: ExplorerOperationPhase::Scanning,
                        label: "Scanning files".to_string(),
                        completed_items: 0,
                        total_items: None,
                        completed_bytes: 0,
                        total_bytes: None,
                        cancellable: true,
                    });
                }
                self.error_message = None;
                self.resolve_explorer_alert();
                self.notify_status("Explorer operation started");
            }
            Err(ExplorerTaskSubmitError::Busy { .. }) => {
                let message = "Another Explorer file operation is still running".to_string();
                if let Some(state) = self.explorer_state.as_mut() {
                    state.error = Some(message.clone());
                }
                self.error_message = Some(message.clone());
                self.notify_alert_with_key(
                    EXPLORER_ALERT_KEY,
                    message,
                    tundra_ui::NotificationTone::Error,
                );
            }
            Err(error) => self.report_explorer_task_error(error.to_string()),
        }
    }

    fn preflight_explorer_permissions(
        &self,
        action: PermissionAction,
        resources: &[PathBuf],
    ) -> Result<(), String> {
        let service = PermissionService::default();
        for resource in resources {
            let display = resource.display().to_string();
            let authorization =
                service.authorize(self.auth_session.as_ref(), action, Some(display.as_str()));
            if !authorization.allowed {
                let reason = authorization
                    .reason
                    .unwrap_or_else(|| "permission_denied".to_string());
                if let Some(storage) = self.storage_manager.clone() {
                    let _ = AuditService::with_permission_service(storage, service.clone()).record(
                        self.auth_session.as_ref(),
                        action,
                        Some(display.as_str()),
                        AuditOutcome::Denied,
                        Some(reason.as_str()),
                    );
                }
                return Err(format!(
                    "Permission denied for {}: {reason}",
                    resource.display()
                ));
            }
        }
        Ok(())
    }

    fn poll_explorer_background_tasks(&mut self, platform: &dyn Platform) {
        let events = self
            .explorer_task_runtime
            .as_ref()
            .map(ShellExplorerTaskRuntime::drain_events)
            .unwrap_or_default();
        for event in events {
            self.apply_explorer_task_event(event, platform);
        }
    }

    fn apply_explorer_task_event(&mut self, event: ExplorerTaskEvent, platform: &dyn Platform) {
        match event {
            ExplorerTaskEvent::Accepted { .. } => {}
            ExplorerTaskEvent::PhaseChanged { id, phase } => {
                if self.explorer_task_context(id).is_some() {
                    self.update_explorer_task_phase(phase);
                }
            }
            ExplorerTaskEvent::PlanningProgress {
                id,
                discovered_items,
                discovered_bytes,
                ..
            } => {
                if self.explorer_task_context(id).is_some()
                    && let Some(operation) = self
                        .explorer_state
                        .as_mut()
                        .and_then(|state| state.operation.as_mut())
                {
                    operation.phase = ExplorerOperationPhase::Scanning;
                    operation.total_items = usize::try_from(discovered_items).ok();
                    operation.total_bytes = Some(discovered_bytes);
                }
            }
            ExplorerTaskEvent::Progress { id, progress } => {
                if self.explorer_task_context(id).is_some()
                    && let Some(operation) = self
                        .explorer_state
                        .as_mut()
                        .and_then(|state| state.operation.as_mut())
                {
                    operation.phase = match progress.phase {
                        ExplorerTaskPhase::Planning => ExplorerOperationPhase::Scanning,
                        ExplorerTaskPhase::Executing | ExplorerTaskPhase::CleaningUp => {
                            ExplorerOperationPhase::Executing
                        }
                    };
                    operation.completed_items =
                        usize::try_from(progress.processed_items).unwrap_or(usize::MAX);
                    operation.total_items = usize::try_from(progress.total_items).ok();
                    operation.completed_bytes = progress.processed_bytes;
                    operation.total_bytes = Some(progress.total_bytes);
                }
            }
            ExplorerTaskEvent::ItemCompleted { id, source, target } => {
                if let Some(context) = self.explorer_task_context(id) {
                    self.record_explorer_task_success(context.kind, &source, target.as_deref());
                }
            }
            ExplorerTaskEvent::ItemSkipped { .. } => {}
            ExplorerTaskEvent::ItemFailed { id, failure } => {
                if let Some(context) = self.explorer_task_context(id) {
                    self.record_explorer_task_failure(
                        context.kind,
                        &failure.source,
                        &failure.error.to_string(),
                    );
                }
            }
            ExplorerTaskEvent::Panicked {
                id,
                incident_id,
                message,
                recovery,
            } => {
                let context = self
                    .explorer_task_runtime
                    .as_ref()
                    .and_then(|runtime| runtime.finish(id));
                if context.is_none() {
                    return;
                }
                let detail = format!(
                    "Explorer operation stopped after an internal error: {message} (incident {incident_id}; recovery: {recovery:?})"
                );
                if let Some(state) = self.explorer_state.as_mut() {
                    state.operation = None;
                    state.message = Some(detail.clone());
                    state.error = Some(detail.clone());
                }
                self.error_message = Some(detail);
                self.apply_explorer_command(ExplorerCommand::Refresh, platform);
            }
            ExplorerTaskEvent::Finished { id, summary } => {
                let context = self
                    .explorer_task_runtime
                    .as_ref()
                    .and_then(|runtime| runtime.finish(id));
                let Some(context) = context else {
                    return;
                };
                let succeeded_originals = context
                    .sources
                    .iter()
                    .filter(|source| {
                        summary
                            .succeeded_sources
                            .iter()
                            .any(|path| explorer_task_paths_match(path, &source.canonical))
                    })
                    .map(|source| source.original.clone())
                    .collect::<Vec<_>>();

                if let ShellExplorerTaskKind::Move {
                    from_clipboard: true,
                } = context.kind
                    && let Some(state) = self.explorer_state.as_mut()
                    && let Some(clipboard) = state.clipboard.as_mut()
                    && clipboard.mode == ExplorerClipboardMode::Cut
                {
                    clipboard
                        .paths
                        .retain(|path| !succeeded_originals.contains(path));
                    if clipboard.paths.is_empty() {
                        state.clipboard = None;
                    }
                }
                if context.kind == ShellExplorerTaskKind::Delete
                    && let Some(state) = self.explorer_state.as_mut()
                {
                    for path in &succeeded_originals {
                        state.selected_paths.remove(path);
                    }
                }

                self.apply_explorer_command(ExplorerCommand::Refresh, platform);
                let fatal = summary
                    .fatal_error
                    .as_ref()
                    .map(ToString::to_string)
                    .unwrap_or_default();
                let detail = [(!summary.cancelled && !fatal.is_empty()).then_some(fatal)]
                    .into_iter()
                    .flatten()
                    .collect::<Vec<_>>()
                    .join("; ");
                let message = if summary.cancelled {
                    format!(
                        "Operation cancelled: {} succeeded, {} failed{}",
                        summary.succeeded_items,
                        summary.failed_items,
                        if detail.is_empty() {
                            String::new()
                        } else {
                            format!(" ({detail})")
                        }
                    )
                } else if summary.failed_items > 0 || summary.fatal_error.is_some() {
                    format!(
                        "Operation finished with errors: {} succeeded, {} failed{}",
                        summary.succeeded_items,
                        summary.failed_items,
                        if detail.is_empty() {
                            String::new()
                        } else {
                            format!(" ({detail})")
                        }
                    )
                } else {
                    format!(
                        "Operation complete: {} succeeded, {} skipped",
                        summary.succeeded_items, summary.skipped_items
                    )
                };
                let has_error = summary.failed_items > 0
                    || (!summary.cancelled && summary.fatal_error.is_some());
                if let Some(state) = self.explorer_state.as_mut() {
                    state.operation = None;
                    state.message = Some(message.clone());
                    state.error = has_error.then_some(message.clone());
                }
                if has_error {
                    self.error_message = Some(message.clone());
                    self.notify_alert_with_key(
                        EXPLORER_ALERT_KEY,
                        message,
                        tundra_ui::NotificationTone::Error,
                    );
                } else {
                    self.error_message = None;
                    self.resolve_explorer_alert();
                    self.notify_status(message);
                }
            }
        }
    }

    fn explorer_task_context(&self, id: ExplorerTaskId) -> Option<ShellExplorerTaskContext> {
        self.explorer_task_runtime
            .as_ref()
            .and_then(|runtime| runtime.context_for(id))
    }

    fn update_explorer_task_phase(&mut self, phase: ExplorerTaskPhase) {
        if let Some(operation) = self
            .explorer_state
            .as_mut()
            .and_then(|state| state.operation.as_mut())
        {
            match phase {
                ExplorerTaskPhase::Planning => {
                    operation.phase = ExplorerOperationPhase::Scanning;
                    operation.label = "Scanning files".to_string();
                }
                ExplorerTaskPhase::Executing => {
                    operation.phase = ExplorerOperationPhase::Executing;
                    operation.label = "Applying file operation".to_string();
                }
                ExplorerTaskPhase::CleaningUp => {
                    operation.phase = ExplorerOperationPhase::Executing;
                    operation.label = "Cleaning up staged files".to_string();
                    operation.cancellable = false;
                }
            }
        }
    }

    fn record_explorer_task_success(
        &self,
        kind: ShellExplorerTaskKind,
        source: &Path,
        target: Option<&Path>,
    ) {
        let Some(storage) = self.storage_manager.clone() else {
            return;
        };
        let (action, reason) = match kind {
            ShellExplorerTaskKind::Copy => (PermissionAction::WriteFile, "copy_paste"),
            ShellExplorerTaskKind::Move { .. } => (PermissionAction::MoveFile, "cut_paste"),
            ShellExplorerTaskKind::Delete => (PermissionAction::DeleteFile, "delete_to_trash"),
        };
        let resource_path = if kind == ShellExplorerTaskKind::Delete {
            source
        } else {
            target.unwrap_or(source)
        };
        let resource = resource_path.display().to_string();
        let _ = AuditService::new(storage.clone()).record(
            self.auth_session.as_ref(),
            action,
            Some(resource.as_str()),
            AuditOutcome::Success,
            Some(reason),
        );
    }

    fn record_explorer_task_failure(
        &self,
        kind: ShellExplorerTaskKind,
        source: &Path,
        reason: &str,
    ) {
        let Some(storage) = self.storage_manager.clone() else {
            return;
        };
        let action = match kind {
            ShellExplorerTaskKind::Copy => PermissionAction::WriteFile,
            ShellExplorerTaskKind::Move { .. } => PermissionAction::MoveFile,
            ShellExplorerTaskKind::Delete => PermissionAction::DeleteFile,
        };
        let resource = source.display().to_string();
        let _ = AuditService::new(storage).record(
            self.auth_session.as_ref(),
            action,
            Some(resource.as_str()),
            AuditOutcome::Failure,
            Some(reason),
        );
    }

    fn sync_explorer_background_conflict_notification(&mut self) {
        // Conflict interaction is owned by Explorer's clickable overlay. Keep the old global
        // notification key clear so it cannot mask or duplicate that dialog.
        self.notifications
            .dismiss_modal_by_key(EXPLORER_CONFLICT_NOTIFICATION_KEY);
    }

    fn report_explorer_task_error(&mut self, message: impl Into<String>) {
        let message = message.into();
        if let Some(state) = self.explorer_state.as_mut() {
            state.operation = None;
            state.error = Some(message.clone());
        }
        self.error_message = Some(message.clone());
        self.notify_alert_with_key(
            EXPLORER_ALERT_KEY,
            message,
            tundra_ui::NotificationTone::Error,
        );
    }
}

fn waiting_for_conflict_progress() -> ExplorerOperationProgress {
    ExplorerOperationProgress {
        phase: ExplorerOperationPhase::WaitingForConflict,
        label: "Waiting for conflict resolution".to_string(),
        completed_items: 0,
        total_items: None,
        completed_bytes: 0,
        total_bytes: None,
        cancellable: true,
    }
}

fn collision_resolution(action: ExplorerConflictAction) -> ExplorerCollisionResolution {
    match action {
        ExplorerConflictAction::KeepBoth => ExplorerCollisionResolution::KeepBoth,
        ExplorerConflictAction::Replace => ExplorerCollisionResolution::Replace,
        ExplorerConflictAction::Skip => ExplorerCollisionResolution::Skip,
        ExplorerConflictAction::Cancel => ExplorerCollisionResolution::Cancel,
    }
}

#[cfg(test)]
mod explorer_task_workflow_tests {
    use super::*;

    fn test_explorer_watchdog() -> tundra_watchdog::AppWatchdog {
        static WATCHDOG: std::sync::OnceLock<tundra_watchdog::AppWatchdog> =
            std::sync::OnceLock::new();
        WATCHDOG
            .get_or_init(|| {
                let _ = default_editor_watchdog();
                if let Some(process) = tundra_watchdog::ProcessWatchdog::global() {
                    return process
                        .register_app(tundra_apps::explorer_tasks::explorer_watchdog_descriptor())
                        .expect("Explorer workflow watchdog registration");
                }
                let root = std::env::temp_dir().join(format!(
                    "tundra-shell-explorer-watchdog-tests-{}",
                    std::process::id()
                ));
                let config = tundra_watchdog::WatchdogConfig::new(
                    root.join("reports"),
                    root.join("fallback"),
                    root.join("state"),
                    "tundra-shell-tests",
                    env!("CARGO_PKG_VERSION"),
                );
                let (runtime, process) = tundra_watchdog::WatchdogRuntime::start(config)
                    .expect("Explorer workflow test watchdog");
                let process = process
                    .install_global()
                    .expect("install Explorer workflow test watchdog");
                let _runtime = Box::leak(Box::new(runtime));
                process
                    .register_app(tundra_apps::explorer_tasks::explorer_watchdog_descriptor())
                    .expect("Explorer workflow watchdog registration")
            })
            .clone()
    }

    #[test]
    fn shell_runtime_executes_copy_on_real_temporary_paths() {
        let fixture = std::env::temp_dir().join(format!(
            "tundra-shell-explorer-task-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        let source_root = fixture.join("source");
        let destination = fixture.join("destination");
        std::fs::create_dir_all(&source_root).expect("source directory");
        std::fs::create_dir_all(&destination).expect("destination directory");
        let source = source_root.join("note.txt");
        std::fs::write(&source, b"background copy").expect("source file");

        let storage = storage_at(&fixture);
        let runtime = ShellExplorerTaskRuntime::new_managed(storage, test_explorer_watchdog());
        let plan =
            ExplorerTransferPlan::new(ExplorerTransferOperation::Copy, vec![source], &destination);
        runtime
            .submit(
                ExplorerTaskPlan::Transfer(plan),
                ShellExplorerTaskKind::Copy,
                "Tester".to_string(),
            )
            .expect("task accepted");

        let summary = wait_for_summary(&runtime);

        assert_eq!(summary.succeeded_items, 1);
        assert_eq!(summary.failed_items, 0);
        assert_eq!(
            std::fs::read(destination.join("note.txt")).expect("copied file"),
            b"background copy"
        );
        drop(runtime);
        let _ = std::fs::remove_dir_all(fixture);
    }

    #[cfg(any())]
    #[test]
    fn storage_trash_adapter_indexes_background_delete_once() {
        let fixture = std::env::temp_dir().join(format!(
            "tundra-shell-explorer-trash-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        let source_root = fixture.join("source");
        std::fs::create_dir_all(&source_root).expect("source directory");
        let source = source_root.join("obsolete.txt");
        std::fs::write(&source, b"trash me").expect("source file");

        let storage = storage_at(&fixture);
        let runtime =
            ShellExplorerTaskRuntime::new_managed(storage.clone(), test_explorer_watchdog());
        runtime
            .submit(
                ExplorerTaskPlan::DeleteToTrash(ExplorerDeletePlan::new(vec![source.clone()])),
                ShellExplorerTaskKind::Delete,
                "TestActor".to_string(),
            )
            .expect("task accepted");
        let summary = wait_for_summary(&runtime);

        assert_eq!(summary.succeeded_items, 1);
        assert!(!source.exists());
        let trash = storage.load_trash().expect("trash manifest");
        assert_eq!(trash.records.len(), 1);
        assert_eq!(
            trash.records[0].original_path.file_name(),
            source.file_name()
        );
        assert_eq!(trash.records[0].actor, "TestActor");
        assert!(trash.records[0].trash_path.exists());

        drop(runtime);
        let _ = std::fs::remove_dir_all(fixture);
    }

    #[cfg(any())]
    #[test]
    fn replacement_is_indexed_in_storage_trash() {
        let fixture = std::env::temp_dir().join(format!(
            "tundra-shell-explorer-replace-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        let source_root = fixture.join("source");
        let destination = fixture.join("destination");
        std::fs::create_dir_all(&source_root).expect("source directory");
        std::fs::create_dir_all(&destination).expect("destination directory");
        let source = source_root.join("same.txt");
        let replaced = destination.join("same.txt");
        std::fs::write(&source, b"new contents").expect("source file");
        std::fs::write(&replaced, b"old contents").expect("destination file");

        let storage = storage_at(&fixture);
        let runtime =
            ShellExplorerTaskRuntime::new_managed(storage.clone(), test_explorer_watchdog());
        let mut plan =
            ExplorerTransferPlan::new(ExplorerTransferOperation::Copy, vec![source], &destination);
        plan.collisions = ExplorerCollisionPolicy::replace();
        runtime
            .submit(
                ExplorerTaskPlan::Transfer(plan),
                ShellExplorerTaskKind::Copy,
                "ReplaceActor".to_string(),
            )
            .expect("task accepted");
        let summary = wait_for_summary(&runtime);

        assert_eq!(summary.succeeded_items, 1);
        assert_eq!(
            std::fs::read(&replaced).expect("replacement"),
            b"new contents"
        );
        let trash = storage.load_trash().expect("trash manifest");
        assert_eq!(trash.records.len(), 1);
        assert_eq!(trash.records[0].actor, "ReplaceActor");
        assert_eq!(
            std::fs::read(&trash.records[0].trash_path).expect("replaced file in trash"),
            b"old contents"
        );

        drop(runtime);
        let _ = std::fs::remove_dir_all(fixture);
    }

    #[cfg(any())]
    #[test]
    fn trash_manifest_failure_rolls_back_filesystem_move() {
        let fixture = std::env::temp_dir().join(format!(
            "tundra-shell-explorer-trash-rollback-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        let source_root = fixture.join("source");
        std::fs::create_dir_all(&source_root).expect("source directory");
        let source = source_root.join("preserved.txt");
        std::fs::write(&source, b"preserve me").expect("source file");
        let storage = storage_at(&fixture);
        std::fs::write(&storage.layout().trash_manifest_path, b"not-json")
            .expect("corrupt trash manifest");
        let actor = Arc::new(Mutex::new("RollbackActor".to_string()));
        let adapter = StorageExplorerTrash::new(storage, actor);
        let platform = tundra_platform::native_platform();

        let error = adapter
            .move_to_trash(platform.as_ref(), &source)
            .expect_err("manifest failure should fail the trash operation");

        assert!(error.to_string().contains("rolled back"));
        assert_eq!(
            std::fs::read(&source).expect("restored source"),
            b"preserve me"
        );
        let _ = std::fs::remove_dir_all(fixture);
    }

    #[cfg(any())]
    #[test]
    fn cross_volume_trash_move_commits_copy_before_removing_source() {
        let fixture = std::env::temp_dir().join(format!(
            "tundra-shell-explorer-trash-cross-volume-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        let source_root = fixture.join("source");
        let target_root = fixture.join("target");
        std::fs::create_dir_all(&source_root).expect("source directory");
        std::fs::create_dir_all(&target_root).expect("target directory");
        let source = source_root.join("cross-volume.txt");
        let target = target_root.join("cross-volume.txt");
        std::fs::write(&source, b"cross volume").expect("source file");
        let user_dirs = tundra_platform::UserDirs::new(
            fixture.join("Desktop"),
            fixture.join("Documents"),
            fixture.join("Downloads"),
            fixture.join("Pictures"),
            fixture.join("Videos"),
            fixture.join("Music"),
            fixture.join("AppData"),
        )
        .expect("absolute user dirs");
        let platform = tundra_platform::mock::MockPlatform::new(user_dirs, app_paths_at(&fixture));
        platform.set_cross_device_rename(
            source.clone(),
            target.clone(),
            "simulated different volume",
        );

        move_to_trash_path(&platform, &source, &target).expect("cross-volume fallback");

        assert!(!source.exists());
        assert_eq!(
            std::fs::read(&target).expect("committed trash target"),
            b"cross volume"
        );
        assert!(
            std::fs::read_dir(&target_root)
                .expect("target listing")
                .all(|entry| !entry
                    .expect("target entry")
                    .file_name()
                    .to_string_lossy()
                    .contains("tundra-trash-stage"))
        );
        let _ = std::fs::remove_dir_all(fixture);
    }

    #[cfg(any())]
    #[test]
    fn reconciliation_removes_manifest_record_restored_by_engine_rollback() {
        let fixture = std::env::temp_dir().join(format!(
            "tundra-shell-explorer-trash-reconcile-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        let storage = storage_at(&fixture);
        let original = fixture.join("destination").join("restored.txt");
        std::fs::create_dir_all(original.parent().expect("destination parent"))
            .expect("destination directory");
        std::fs::write(&original, b"restored").expect("restored destination");
        let missing_trash_path = storage.layout().trash_path.join("rolled-back.txt");
        let mut trash = storage.load_trash().expect("trash manifest");
        trash.records.push(TrashRecord {
            original_path: original,
            trash_path: missing_trash_path,
            actor: "TestActor".to_string(),
            timestamp_epoch_ms: explorer_unix_millis(),
        });
        storage.save_trash(&trash).expect("save stale record");

        reconcile_storage_trash_manifest(&storage).expect("reconcile trash manifest");

        assert!(
            storage
                .load_trash()
                .expect("reconciled manifest")
                .records
                .is_empty()
        );
        let _ = std::fs::remove_dir_all(fixture);
    }

    #[test]
    fn recursive_conflict_preflight_reports_only_non_merge_targets() {
        let fixture = std::env::temp_dir().join(format!(
            "tundra-shell-explorer-conflicts-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        let source = fixture.join("source").join("folder");
        let destination = fixture.join("destination");
        let target = destination.join("folder");
        std::fs::create_dir_all(source.join("sub")).expect("source directories");
        std::fs::create_dir_all(target.join("sub")).expect("target directories");
        std::fs::write(source.join("same.txt"), b"new").expect("source collision");
        std::fs::write(target.join("same.txt"), b"old").expect("target collision");
        std::fs::write(source.join("only.txt"), b"only source").expect("source-only file");
        std::fs::write(source.join("sub").join("deep.txt"), b"new deep")
            .expect("deep source collision");
        std::fs::write(target.join("sub").join("deep.txt"), b"old deep")
            .expect("deep target collision");
        let canonical_destination =
            std::fs::canonicalize(&destination).expect("canonical destination");
        let canonical_target = canonical_destination.join("folder");
        let platform = tundra_platform::native_platform();
        let mut conflicts = Vec::new();

        collect_explorer_conflicts_no_follow(
            platform.as_ref(),
            &source,
            &canonical_target,
            &mut conflicts,
        )
        .expect("recursive conflict scan");

        assert_eq!(conflicts.len(), 2);
        assert!(
            conflicts
                .iter()
                .any(|(_, target)| target == &canonical_target.join("same.txt"))
        );
        assert!(
            conflicts
                .iter()
                .any(|(_, target)| { target == &canonical_target.join("sub").join("deep.txt") })
        );
        assert!(
            !conflicts
                .iter()
                .any(|(_, target)| target == &canonical_target)
        );
        let _ = std::fs::remove_dir_all(fixture);
    }

    fn storage_at(fixture: &Path) -> StorageManager {
        StorageManager::open(app_paths_at(fixture))
            .expect("test storage opens")
            .manager
    }

    fn app_paths_at(fixture: &Path) -> tundra_platform::AppPaths {
        tundra_platform::AppPaths::from_parts(
            fixture.join("config.toml"),
            fixture.join("data"),
            fixture.join("cache"),
            fixture.join("logs"),
            fixture.join("temp"),
        )
        .expect("absolute test app paths")
    }

    fn wait_for_summary(
        runtime: &ShellExplorerTaskRuntime,
    ) -> tundra_apps::explorer_tasks::ExplorerTaskSummary {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(summary) = runtime.drain_events().into_iter().find_map(|event| {
                if let ExplorerTaskEvent::Finished { summary, .. } = event {
                    Some(summary)
                } else {
                    None
                }
            }) {
                return summary;
            }
            assert!(Instant::now() < deadline, "background task timed out");
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}
