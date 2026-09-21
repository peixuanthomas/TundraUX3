use super::super::*;
use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use app::explorer::{
    ExplorerClipboard, ExplorerClipboardMode, ExplorerConflict, ExplorerOperationPhase,
    ExplorerOperationProgress, ExplorerPendingTransfer, ExplorerTransferMode,
};
use app::explorer_tasks::{
    ExplorerCollisionPolicy, ExplorerCollisionResolution, ExplorerDeletePlan, ExplorerTaskEngine,
    ExplorerTaskEvent, ExplorerTaskId, ExplorerTaskPhase, ExplorerTaskPlan,
    ExplorerTaskSubmitError, ExplorerTransferOperation, ExplorerTransferPlan, SystemExplorerTrash,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::session) enum ShellExplorerTaskKind {
    Copy,
    Move { from_clipboard: bool },
    Delete,
}

#[derive(Debug, Clone)]
pub(in crate::session) struct ShellExplorerTaskContext {
    pub(in crate::session) id: ExplorerTaskId,
    pub(in crate::session) kind: ShellExplorerTaskKind,
    pub(in crate::session) sources: Vec<ShellExplorerTaskSource>,
}

#[derive(Debug, Clone)]
pub(in crate::session) struct ShellExplorerTaskSource {
    pub(in crate::session) original: PathBuf,
    pub(in crate::session) canonical: PathBuf,
}

pub(in crate::session) fn task_plan_sources(plan: &ExplorerTaskPlan) -> Vec<PathBuf> {
    match plan {
        ExplorerTaskPlan::Transfer(plan) => plan.sources.clone(),
        ExplorerTaskPlan::DeleteToTrash(plan) => plan.paths.clone(),
    }
}

pub(in crate::session) fn explorer_task_paths_match(left: &Path, right: &Path) -> bool {
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

pub(in crate::session) fn collect_explorer_conflicts_no_follow(
    platform: &dyn Platform,
    source: &Path,
    target: &Path,
    conflicts: &mut Vec<(PathBuf, PathBuf)>,
) -> Result<(), i18n::LocalizedText> {
    let source_attributes = platform.file_attributes(source).map_err(|error| {
        i18n::LocalizedText::from(i18n::msg!(
            "shell-could-not-inspect-transfer-source-arg1-error",
            arg1 = source.display().to_string(),
            error = error.to_string()
        ))
    })?;
    let target_attributes = match std::fs::symlink_metadata(target) {
        Ok(_) => Some(platform.file_attributes(target).map_err(|error| {
            i18n::LocalizedText::from(i18n::msg!(
                "shell-could-not-inspect-transfer-target-arg1-error",
                arg1 = target.display().to_string(),
                error = error.to_string()
            ))
        })?),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => {
            return Err(i18n::LocalizedText::from(i18n::msg!(
                "shell-could-not-inspect-transfer-target-arg1-error",
                arg1 = target.display().to_string(),
                error = error.to_string()
            )));
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
        i18n::LocalizedText::from(i18n::msg!(
            "shell-could-not-scan-source-directory-arg1-for-conflicts-error",
            arg1 = source.display().to_string(),
            error = error.to_string()
        ))
    })?;
    let mut entries = directory
        .map(|entry| {
            entry.map_err(|error| {
                i18n::LocalizedText::from(i18n::msg!(
                    "shell-could-not-read-an-entry-in-arg1-during-conflict-scan-error",
                    arg1 = source.display().to_string(),
                    error = error.to_string()
                ))
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

pub(in crate::session) struct ShellExplorerTaskShared {
    pub(in crate::session) engine: Mutex<Option<ExplorerTaskEngine>>,
    pub(in crate::session) context: Mutex<Option<ShellExplorerTaskContext>>,
    pub(in crate::session) watchdog: Option<AppWatchdog>,
}

/// Cloneable shell handle around the single-worker Explorer mutation engine.
///
/// The worker and receiver are deliberately excluded from ShellSession equality: they are runtime
/// infrastructure, while Explorer's visible progress remains in `ExplorerState::operation`.
#[derive(Clone)]
pub(in crate::session) struct ShellExplorerTaskRuntime {
    pub(in crate::session) shared: Arc<ShellExplorerTaskShared>,
}

impl ShellExplorerTaskRuntime {
    pub(in crate::session) fn new(_storage: StorageManager) -> Self {
        let watchdog = ProcessWatchdog::global().and_then(|process| {
            process
                .register_app(app::explorer_tasks::explorer_watchdog_descriptor())
                .ok()
        });
        Self::with_watchdog(watchdog)
    }

    pub(in crate::session) fn new_managed(_storage: StorageManager, watchdog: AppWatchdog) -> Self {
        Self::with_watchdog(Some(watchdog))
    }

    pub(in crate::session) fn with_watchdog(watchdog: Option<AppWatchdog>) -> Self {
        Self {
            shared: Arc::new(ShellExplorerTaskShared {
                engine: Mutex::new(None),
                context: Mutex::new(None),
                watchdog,
            }),
        }
    }

    pub(in crate::session) fn submit(
        &self,
        plan: ExplorerTaskPlan,
        kind: ShellExplorerTaskKind,
        actor: String,
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
            let platform: Arc<dyn Platform> = Arc::from(platform::native_platform());
            let trash = Arc::new(SystemExplorerTrash);
            *engine = Some(
                ExplorerTaskEngine::new_managed(platform, trash, watchdog)
                    .map_err(|_| ExplorerTaskSubmitError::WorkerStopped)?,
            );
        }
        let engine = engine
            .as_ref()
            .expect("Explorer engine was initialized in the preceding branch");
        let handle = engine.submit_with_owner(plan, Some(actor))?;
        *context = Some(ShellExplorerTaskContext {
            id: handle.id,
            kind,
            sources,
        });
        Ok(handle.id)
    }

    pub(in crate::session) fn cancel_active(&self) -> bool {
        self.shared
            .engine
            .lock()
            .expect("Explorer task engine lock poisoned")
            .as_ref()
            .is_some_and(ExplorerTaskEngine::cancel_active)
    }

    pub(in crate::session) fn drain_events(&self) -> Vec<ExplorerTaskEvent> {
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

    pub(in crate::session) fn context_for(
        &self,
        id: ExplorerTaskId,
    ) -> Option<ShellExplorerTaskContext> {
        self.shared
            .context
            .lock()
            .expect("Explorer task context lock poisoned")
            .as_ref()
            .filter(|context| context.id == id)
            .cloned()
    }

    pub(in crate::session) fn finish(
        &self,
        id: ExplorerTaskId,
    ) -> Option<ShellExplorerTaskContext> {
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

impl ShellSession {
    /// Returns true when this command belongs to the asynchronous mutation path.
    pub(in crate::session) fn try_handle_explorer_background_command(
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
                    .app
                    .explorer_state()
                    .is_some_and(|state| state.confirm_delete);
                if should_confirm {
                    false
                } else {
                    let paths = self
                        .app
                        .explorer_state()
                        .map(|state| state.effective_selected_paths())
                        .unwrap_or_default();
                    self.start_explorer_background_delete_paths(paths);
                    true
                }
            }
            ExplorerCommand::ConfirmDelete => {
                let Some(paths) = self.app.explorer_state().and_then(|state| {
                    state
                        .pending_dialog
                        .as_ref()
                        .filter(|dialog| {
                            dialog.kind == app::explorer::ExplorerDialogKind::DeleteToTrash
                        })
                        .map(|dialog| dialog.targets.clone())
                }) else {
                    return false;
                };
                self.notification_dismiss_modal_by_key(EXPLORER_DELETE_NOTIFICATION_KEY);
                let _ = self.update_explorer_state(|state| {
                    state.pending_dialog = None;
                });
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
                .app
                .explorer_state()
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
                    let _ = self.update_explorer_state(|state| {
                        if let Some(operation) = state.operation.as_mut() {
                            operation.label =
                                i18n::LocalizedText::from(i18n::msg!("shell-cancelling-operation"));
                            operation.cancellable = false;
                        }
                    });
                    self.notify_status(i18n::LocalizedText::from(i18n::msg!(
                        "shell-cancelling-explorer-operation"
                    )));
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
    pub(in crate::session) fn start_explorer_background_paste(&mut self, platform: &dyn Platform) {
        let Some(state) = self.app.explorer_state() else {
            self.report_explorer_task_error(i18n::LocalizedText::from(i18n::msg!(
                "shell-explorer-unavailable"
            )));
            return;
        };
        let Some(clipboard) = state.clipboard.clone() else {
            self.report_explorer_task_error(i18n::LocalizedText::from(i18n::msg!(
                "shell-clipboard-is-empty"
            )));
            return;
        };
        self.prepare_explorer_background_transfer(
            clipboard,
            state.current_path.clone(),
            true,
            platform,
        );
    }

    pub(in crate::session) fn start_explorer_background_drop(&mut self, platform: &dyn Platform) {
        let drag = self
            .update_explorer_state(|state| state.drag.take())
            .flatten();
        let Some(drag) = drag else {
            return;
        };
        if !drag.active {
            return;
        }
        let Some(destination) = drag.target else {
            self.report_explorer_task_error(i18n::LocalizedText::from(i18n::msg!(
                "shell-drag-has-no-valid-destination"
            )));
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
    pub(in crate::session) fn prepare_explorer_background_transfer(
        &mut self,
        clipboard: ExplorerClipboard,
        destination: PathBuf,
        from_clipboard: bool,
        platform: &dyn Platform,
    ) {
        if clipboard.paths.is_empty() {
            self.report_explorer_task_error(i18n::LocalizedText::from(i18n::msg!(
                "shell-clipboard-is-empty"
            )));
            return;
        }
        let destination = match std::fs::canonicalize(&destination) {
            Ok(destination) => destination,
            Err(error) => {
                self.report_explorer_task_error(i18n::LocalizedText::from(i18n::msg!(
                    "shell-could-not-resolve-transfer-destination-arg1-error",
                    arg1 = destination.display().to_string(),
                    error = error.to_string()
                )));
                return;
            }
        };
        let mut conflicts = Vec::new();
        let mut targets = Vec::with_capacity(clipboard.paths.len());
        for source in &clipboard.paths {
            let Some(file_name) = source.file_name() else {
                self.report_explorer_task_error(i18n::LocalizedText::from(i18n::msg!(
                    "shell-arg1-has-no-file-name",
                    arg1 = source.display().to_string()
                )));
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
            .app
            .explorer_state()
            .is_some_and(|state| state.confirm_name_conflicts);
        if confirm_conflicts && !conflicts.is_empty() {
            self.explorer_overlay_selection = 0;
            self.explorer_conflict_apply_to_remaining = false;
            let (source, target) = conflicts[0].clone();
            let _ = self.update_explorer_state(|state| {
                state.pending_conflict = Some(ExplorerConflict {
                    source,
                    target,
                    remaining: conflicts.len(),
                });
                let clipboard_mode = clipboard.mode;
                state.pending_transfer = Some(ExplorerPendingTransfer {
                    clipboard,
                    destination,
                    conflicts,
                    current_conflict: 0,
                    resolutions: BTreeMap::new(),
                });
                state.operation = Some(waiting_for_conflict_progress(clipboard_mode));
            });
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

    pub(in crate::session) fn resolve_explorer_background_conflict(
        &mut self,
        action: ExplorerConflictAction,
        apply_to_all: bool,
    ) {
        if action == ExplorerConflictAction::Cancel {
            let _ = self.update_explorer_state(|state| {
                state.pending_conflict = None;
                state.pending_transfer = None;
                state.operation = None;
                state.message = Some(i18n::LocalizedText::from(i18n::msg!(
                    "shell-transfer-cancelled"
                )));
                state.error = None;
            });
            self.notification_dismiss_modal_by_key(EXPLORER_CONFLICT_NOTIFICATION_KEY);
            return;
        }

        let ready = self
            .update_explorer_state(|state| {
                let pending = state.pending_transfer.as_mut()?;
                let (_, target) = pending.conflicts.get(pending.current_conflict).cloned()?;
                pending.resolutions.insert(target, action);
                if apply_to_all {
                    for (_, target) in pending.conflicts.iter().skip(pending.current_conflict + 1) {
                        pending.resolutions.insert(target.clone(), action);
                    }
                    pending.current_conflict = pending.conflicts.len();
                } else {
                    pending.current_conflict += 1;
                }
                Some(pending.current_conflict >= pending.conflicts.len())
            })
            .flatten();
        let Some(ready) = ready else {
            if self
                .app
                .explorer_state()
                .is_some_and(|state| state.pending_transfer.is_some())
            {
                self.report_explorer_task_error(i18n::LocalizedText::from(i18n::msg!(
                    "shell-invalid-conflict-state"
                )));
            }
            return;
        };

        if !ready {
            let _ = self.update_explorer_state(|state| {
                if let Some(pending) = state.pending_transfer.as_ref()
                    && let Some((source, target)) =
                        pending.conflicts.get(pending.current_conflict).cloned()
                {
                    state.pending_conflict = Some(ExplorerConflict {
                        source,
                        target,
                        remaining: pending.conflicts.len() - pending.current_conflict,
                    });
                }
            });
            self.sync_explorer_background_conflict_notification();
            return;
        }

        let pending = self
            .update_explorer_state(|state| {
                state.pending_conflict = None;
                state.pending_transfer.take()
            })
            .flatten()
            .expect("completed conflict sequence has a pending transfer");
        self.notification_dismiss_modal_by_key(EXPLORER_CONFLICT_NOTIFICATION_KEY);
        // Pending transfers originate from either Paste or drag. Only clear a cut clipboard when
        // it still contains the exact paths being moved.
        let from_clipboard = self
            .app
            .explorer_state()
            .and_then(|state| state.clipboard.as_ref())
            .is_some_and(|clipboard| clipboard == &pending.clipboard);
        self.submit_explorer_background_transfer(
            pending.clipboard,
            pending.destination,
            pending.resolutions,
            from_clipboard,
        );
    }
    pub(in crate::session) fn submit_explorer_background_transfer(
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

    pub(in crate::session) fn start_explorer_background_delete_paths(
        &mut self,
        paths: Vec<std::path::PathBuf>,
    ) {
        if paths.is_empty() {
            self.report_explorer_task_error(i18n::LocalizedText::from(i18n::msg!(
                "shell-no-file-is-selected"
            )));
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

    pub(in crate::session) fn submit_explorer_task(
        &mut self,
        plan: ExplorerTaskPlan,
        kind: ShellExplorerTaskKind,
    ) {
        let Some(runtime) = self.explorer_task_runtime.as_ref() else {
            self.report_explorer_task_error(i18n::LocalizedText::from(i18n::msg!(
                "shell-explorer-task-service-is-unavailable"
            )));
            return;
        };
        let actor = self
            .app
            .auth_session()
            .map(|session| session.user_id.clone())
            .unwrap_or_else(|| "Guest".to_string());
        let operation = plan.operation();
        match runtime.submit(plan, kind, actor) {
            Ok(_) => {
                let _ = self.update_explorer_state(|state| {
                    state.pending_conflict = None;
                    state.pending_transfer = None;
                    state.error = None;
                    state.message = None;
                    state.operation = Some(ExplorerOperationProgress {
                        operation,
                        phase: ExplorerOperationPhase::Scanning,
                        label: i18n::LocalizedText::from(i18n::msg!("shell-scanning-files")),
                        completed_items: 0,
                        total_items: None,
                        completed_bytes: 0,
                        total_bytes: None,
                        cancellable: true,
                    });
                });
                self.error_message = None;
                self.resolve_explorer_alert();
                self.notify_status(i18n::LocalizedText::from(i18n::msg!(
                    "shell-explorer-operation-started"
                )));
            }
            Err(ExplorerTaskSubmitError::Busy { .. }) => {
                let message = i18n::LocalizedText::from(i18n::msg!(
                    "shell-another-explorer-file-operation-is-still-running"
                ));
                let _ = self.update_explorer_state(|state| {
                    state.error = Some(message.clone());
                });
                self.error_message = Some(message.clone());
                self.notify_alert_with_key(
                    EXPLORER_ALERT_KEY,
                    message,
                    ui::NotificationTone::Error,
                );
            }
            Err(error) => self.report_explorer_task_error(error.localized().message),
        }
    }
    pub(in crate::session) fn preflight_explorer_permissions(
        &self,
        action: PermissionAction,
        resources: &[PathBuf],
    ) -> Result<(), i18n::LocalizedText> {
        let service = PermissionService::default();
        for resource in resources {
            let display = resource.display().to_string();
            let authorization =
                service.authorize(self.app.auth_session(), action, Some(display.as_str()));
            if !authorization.allowed {
                let reason = authorization
                    .reason
                    .unwrap_or_else(|| "permission_denied".to_string());
                return Err(i18n::LocalizedText::from(i18n::msg!(
                    "shell-permission-denied-for-arg1-reason",
                    arg1 = resource.display().to_string(),
                    reason = reason.to_string()
                )));
            }
        }
        Ok(())
    }

    pub(in crate::session) fn poll_explorer_background_tasks(&mut self, platform: &dyn Platform) {
        let events = self
            .explorer_task_runtime
            .as_ref()
            .map(ShellExplorerTaskRuntime::drain_events)
            .unwrap_or_default();
        for event in events {
            self.apply_explorer_task_event(event, platform);
        }
    }

    pub(in crate::session) fn apply_explorer_task_event(
        &mut self,
        event: ExplorerTaskEvent,
        platform: &dyn Platform,
    ) {
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
                if self.explorer_task_context(id).is_some() {
                    let _ = self.update_explorer_state(|state| {
                        if let Some(operation) = state.operation.as_mut() {
                            operation.phase = ExplorerOperationPhase::Scanning;
                            operation.total_items = usize::try_from(discovered_items).ok();
                            operation.total_bytes = Some(discovered_bytes);
                        }
                    });
                }
            }
            ExplorerTaskEvent::Progress { id, progress } => {
                if self.explorer_task_context(id).is_some() {
                    let _ = self.update_explorer_state(|state| {
                        if let Some(operation) = state.operation.as_mut() {
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
                    });
                }
            }
            ExplorerTaskEvent::ItemCompleted { .. }
            | ExplorerTaskEvent::ItemSkipped { .. }
            | ExplorerTaskEvent::ItemFailed { .. } => {}
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
                let detail = i18n::LocalizedText::from(i18n::msg!(
                    "shell-explorer-operation-stopped-after-an-internal-error-message-incident-incident-id-recovery-recovery",
                    message = message,
                    incident_id = incident_id,
                    recovery = format!("{:?}", recovery)
                ));
                let _ = self.update_explorer_state(|state| {
                    state.operation = None;
                    state.message = Some(detail.clone());
                    state.error = Some(detail.clone());
                });
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
                {
                    let _ = self.update_explorer_state(|state| {
                        if let Some(clipboard) = state.clipboard.as_mut()
                            && clipboard.mode == ExplorerClipboardMode::Cut
                        {
                            clipboard
                                .paths
                                .retain(|path| !succeeded_originals.contains(path));
                            if clipboard.paths.is_empty() {
                                state.clipboard = None;
                            }
                        }
                    });
                }
                if context.kind == ShellExplorerTaskKind::Delete {
                    let _ = self.update_explorer_state(|state| {
                        for path in &succeeded_originals {
                            state.selected_paths.remove(path);
                        }
                    });
                }

                self.apply_explorer_command(ExplorerCommand::Refresh, platform);
                let detail =
                    explorer_task_error_detail(summary.fatal_error.as_ref(), &summary.failures);
                let message = if summary.cancelled {
                    i18n::LocalizedText::from(i18n::msg!(
                        "shell-operation-cancelled-arg1-succeeded-arg2-failedarg3",
                        arg1 = summary.succeeded_items,
                        arg2 = summary.failed_items,
                        arg3 = detail
                            .as_ref()
                            .map(|detail| i18n::LocalizedText::from(i18n::msg!(
                                "shell-operation-error-detail-suffix",
                                detail = detail
                            )))
                            .unwrap_or_else(|| "".into())
                    ))
                } else if summary.failed_items > 0 || summary.fatal_error.is_some() {
                    i18n::LocalizedText::from(i18n::msg!(
                        "shell-operation-finished-with-errors-arg1-succeeded-arg2-failedarg3",
                        arg1 = summary.succeeded_items,
                        arg2 = summary.failed_items,
                        arg3 = detail
                            .as_ref()
                            .map(|detail| i18n::LocalizedText::from(i18n::msg!(
                                "shell-operation-error-detail-suffix",
                                detail = detail
                            )))
                            .unwrap_or_else(|| "".into())
                    ))
                } else {
                    i18n::LocalizedText::from(i18n::msg!(
                        "shell-operation-complete-arg1-succeeded-arg2-skipped",
                        arg1 = summary.succeeded_items,
                        arg2 = summary.skipped_items
                    ))
                };
                let has_error = summary.failed_items > 0
                    || (!summary.cancelled && summary.fatal_error.is_some());
                let _ = self.update_explorer_state(|state| {
                    state.operation = None;
                    state.message = Some(message.clone());
                    state.error = has_error.then_some(message.clone());
                });
                if has_error {
                    self.error_message = Some(message.clone());
                    self.notify_alert_with_key(
                        EXPLORER_ALERT_KEY,
                        message,
                        ui::NotificationTone::Error,
                    );
                } else {
                    self.error_message = None;
                    self.resolve_explorer_alert();
                    self.notify_status(message);
                }
            }
        }
    }
    pub(in crate::session) fn explorer_task_context(
        &self,
        id: ExplorerTaskId,
    ) -> Option<ShellExplorerTaskContext> {
        self.explorer_task_runtime
            .as_ref()
            .and_then(|runtime| runtime.context_for(id))
    }

    pub(in crate::session) fn update_explorer_task_phase(&mut self, phase: ExplorerTaskPhase) {
        let _ = self.update_explorer_state(|state| {
            if let Some(operation) = state.operation.as_mut() {
                match phase {
                    ExplorerTaskPhase::Planning => {
                        operation.phase = ExplorerOperationPhase::Scanning;
                        operation.label =
                            i18n::LocalizedText::from(i18n::msg!("shell-scanning-files"));
                    }
                    ExplorerTaskPhase::Executing => {
                        operation.phase = ExplorerOperationPhase::Executing;
                        operation.label =
                            i18n::LocalizedText::from(i18n::msg!("shell-applying-file-operation"));
                    }
                    ExplorerTaskPhase::CleaningUp => {
                        operation.phase = ExplorerOperationPhase::Executing;
                        operation.label =
                            i18n::LocalizedText::from(i18n::msg!("shell-cleaning-up-staged-files"));
                        operation.cancellable = false;
                    }
                }
            }
        });
    }
    pub(in crate::session) fn sync_explorer_background_conflict_notification(&mut self) {
        // Conflict interaction is owned by Explorer's clickable overlay. Keep the old global
        // notification key clear so it cannot mask or duplicate that dialog.
        self.notification_dismiss_modal_by_key(EXPLORER_CONFLICT_NOTIFICATION_KEY);
    }

    pub(in crate::session) fn report_explorer_task_error(
        &mut self,
        message: impl Into<i18n::LocalizedText>,
    ) {
        let message = message.into();
        let _ = self.update_explorer_state(|state| {
            state.operation = None;
            state.error = Some(message.clone());
        });
        self.error_message = Some(message.clone());
        self.notify_alert_with_key(EXPLORER_ALERT_KEY, message, ui::NotificationTone::Error);
    }
}

fn explorer_task_error_detail(
    fatal_error: Option<&app::explorer_tasks::ExplorerTaskError>,
    failures: &[app::explorer_tasks::ExplorerItemFailure],
) -> Option<i18n::LocalizedText> {
    fatal_error
        .map(|error| error.localized().message.into())
        .or_else(|| {
            failures.first().map(|failure| {
                i18n::msg!(
                    "shell-path-error-detail",
                    path = failure.source.display().to_string(),
                    reason = failure.error.localized().message
                )
                .into()
            })
        })
}

pub(in crate::session) fn waiting_for_conflict_progress(
    mode: ExplorerClipboardMode,
) -> ExplorerOperationProgress {
    ExplorerOperationProgress {
        operation: match mode {
            ExplorerClipboardMode::Copy => app::explorer_tasks::ExplorerTaskOperation::Copy,
            ExplorerClipboardMode::Cut => app::explorer_tasks::ExplorerTaskOperation::Move,
        },
        phase: ExplorerOperationPhase::WaitingForConflict,
        label: i18n::LocalizedText::from(i18n::msg!("shell-waiting-for-conflict-resolution")),
        completed_items: 0,
        total_items: None,
        completed_bytes: 0,
        total_bytes: None,
        cancellable: true,
    }
}

pub(in crate::session) fn collision_resolution(
    action: ExplorerConflictAction,
) -> ExplorerCollisionResolution {
    match action {
        ExplorerConflictAction::KeepBoth => ExplorerCollisionResolution::KeepBoth,
        ExplorerConflictAction::Replace => ExplorerCollisionResolution::Replace,
        ExplorerConflictAction::Skip => ExplorerCollisionResolution::Skip,
        ExplorerConflictAction::Cancel => ExplorerCollisionResolution::Cancel,
    }
}

#[cfg(test)]
#[path = "../../../tests/unit/session/controller/explorer_tasks/explorer_task_workflow_tests.rs"]
mod explorer_task_workflow_tests;
