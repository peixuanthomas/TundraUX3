//! Background, cancellation-aware filesystem jobs used by Explorer.
//!
//! This module deliberately has no dependency on Explorer's UI state.  A caller submits an
//! immutable plan, polls the event receiver, and folds those events into whichever view model it
//! owns.  All filesystem mutations happen on one dedicated worker thread.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::panic::AssertUnwindSafe;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

use platform::{FileAttributes, Platform, PlatformError, PlatformKind};
use watchdog::{
    AppCriticality, AppDescriptor, AppId, AppWatchdog, BoundaryKind, BoundarySpec, ComponentId,
    ErrorContext, IncidentSeverity, ManagedThreadHandle, OperationCheckpoint, OperationDescriptor,
    OperationGuard, OperationKind, OperationRecord, PanicAction, ProcessWatchdog, RecoveryHandler,
    RecoveryOutcome, ReplaySafety, RestartPolicy, TaskId, TaskKind, TaskSpec, WatchdogError,
};

pub const DEFAULT_TRANSFER_CHUNK_SIZE: usize = 256 * 1024;
static ENGINE_SEQUENCE: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ExplorerTaskId(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExplorerTransferOperation {
    Copy,
    Move,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExplorerCollisionResolution {
    KeepBoth,
    Replace,
    Skip,
    Cancel,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExplorerCollisionPolicy {
    pub default: ExplorerCollisionResolution,
    /// Per-target decisions, keyed by the original colliding destination path.
    pub overrides: BTreeMap<PathBuf, ExplorerCollisionResolution>,
}

impl ExplorerCollisionPolicy {
    pub fn keep_both() -> Self {
        Self {
            default: ExplorerCollisionResolution::KeepBoth,
            overrides: BTreeMap::new(),
        }
    }

    pub fn replace() -> Self {
        Self {
            default: ExplorerCollisionResolution::Replace,
            overrides: BTreeMap::new(),
        }
    }

    fn resolution_for(&self, target: &Path) -> ExplorerCollisionResolution {
        self.overrides.get(target).copied().unwrap_or(self.default)
    }
}

impl Default for ExplorerCollisionPolicy {
    fn default() -> Self {
        Self::keep_both()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExplorerTransferPlan {
    pub operation: ExplorerTransferOperation,
    pub sources: Vec<PathBuf>,
    pub destination: PathBuf,
    pub collisions: ExplorerCollisionPolicy,
    pub chunk_size: usize,
}

impl ExplorerTransferPlan {
    pub fn new(
        operation: ExplorerTransferOperation,
        sources: Vec<PathBuf>,
        destination: impl Into<PathBuf>,
    ) -> Self {
        Self {
            operation,
            sources,
            destination: destination.into(),
            collisions: ExplorerCollisionPolicy::default(),
            chunk_size: DEFAULT_TRANSFER_CHUNK_SIZE,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExplorerDeletePlan {
    pub paths: Vec<PathBuf>,
}

impl ExplorerDeletePlan {
    pub fn new(paths: Vec<PathBuf>) -> Self {
        Self { paths }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExplorerTaskPlan {
    Transfer(ExplorerTransferPlan),
    DeleteToTrash(ExplorerDeletePlan),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExplorerTaskPhase {
    Planning,
    Executing,
    CleaningUp,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExplorerTaskProgress {
    pub phase: ExplorerTaskPhase,
    pub processed_items: u64,
    pub total_items: u64,
    pub processed_bytes: u64,
    pub total_bytes: u64,
    pub current_path: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExplorerItemFailure {
    pub source: PathBuf,
    pub target: Option<PathBuf>,
    pub error: ExplorerTaskError,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExplorerTaskSummary {
    pub total_items: u64,
    pub succeeded_items: u64,
    pub skipped_items: u64,
    pub failed_items: u64,
    pub processed_bytes: u64,
    pub total_bytes: u64,
    pub succeeded_sources: Vec<PathBuf>,
    pub failed_sources: Vec<PathBuf>,
    pub failures: Vec<ExplorerItemFailure>,
    pub cancelled: bool,
    /// A planning or infrastructure failure which prevented normal per-item execution.
    pub fatal_error: Option<ExplorerTaskError>,
}

impl ExplorerTaskSummary {
    fn empty() -> Self {
        Self {
            total_items: 0,
            succeeded_items: 0,
            skipped_items: 0,
            failed_items: 0,
            processed_bytes: 0,
            total_bytes: 0,
            succeeded_sources: Vec::new(),
            failed_sources: Vec::new(),
            failures: Vec::new(),
            cancelled: false,
            fatal_error: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExplorerTaskEvent {
    Accepted {
        id: ExplorerTaskId,
    },
    PhaseChanged {
        id: ExplorerTaskId,
        phase: ExplorerTaskPhase,
    },
    PlanningProgress {
        id: ExplorerTaskId,
        discovered_items: u64,
        discovered_bytes: u64,
        current_path: PathBuf,
    },
    Progress {
        id: ExplorerTaskId,
        progress: ExplorerTaskProgress,
    },
    ItemCompleted {
        id: ExplorerTaskId,
        source: PathBuf,
        target: Option<PathBuf>,
    },
    ItemSkipped {
        id: ExplorerTaskId,
        source: PathBuf,
        target: Option<PathBuf>,
    },
    ItemFailed {
        id: ExplorerTaskId,
        failure: ExplorerItemFailure,
    },
    Finished {
        id: ExplorerTaskId,
        summary: ExplorerTaskSummary,
    },
    Panicked {
        id: ExplorerTaskId,
        incident_id: String,
        message: String,
        recovery: RecoveryOutcome,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExplorerTaskError {
    InvalidPlan {
        message: String,
    },
    Io {
        operation: &'static str,
        path: PathBuf,
        message: String,
    },
    Platform(PlatformError),
    UnsafeLink {
        path: PathBuf,
    },
    TrashUnavailable {
        path: PathBuf,
    },
    CollisionCancelled {
        path: PathBuf,
    },
    DestinationChanged {
        path: PathBuf,
    },
    PartialMove {
        path: PathBuf,
    },
    Cancelled,
    WorkerStopped,
    Journal {
        message: String,
    },
    RecoveryRequired {
        message: String,
    },
}

impl fmt::Display for ExplorerTaskError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidPlan { message } => formatter.write_str(message),
            Self::Io {
                operation,
                path,
                message,
            } => write!(
                formatter,
                "{operation} failed for {}: {message}",
                path.display()
            ),
            Self::Platform(error) => error.fmt(formatter),
            Self::UnsafeLink { path } => write!(
                formatter,
                "copying {} is blocked because it is a symlink, junction, or reparse point",
                path.display()
            ),
            Self::TrashUnavailable { path } => write!(
                formatter,
                "moving {} to trash is unavailable",
                path.display()
            ),
            Self::CollisionCancelled { path } => {
                write!(
                    formatter,
                    "the collision at {} cancelled the task",
                    path.display()
                )
            }
            Self::DestinationChanged { path } => write!(
                formatter,
                "the destination {} changed after planning",
                path.display()
            ),
            Self::PartialMove { path } => write!(
                formatter,
                "{} was only partially moved; its source was preserved",
                path.display()
            ),
            Self::Cancelled => formatter.write_str("the task was cancelled"),
            Self::WorkerStopped => formatter.write_str("the Explorer task worker has stopped"),
            Self::Journal { message } => {
                write!(formatter, "Explorer recovery journal failed: {message}")
            }
            Self::RecoveryRequired { message } => {
                write!(
                    formatter,
                    "Explorer operation requires manual recovery: {message}"
                )
            }
        }
    }
}

impl std::error::Error for ExplorerTaskError {}

impl From<PlatformError> for ExplorerTaskError {
    fn from(value: PlatformError) -> Self {
        Self::Platform(value)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExplorerTaskSubmitError {
    Busy { active: ExplorerTaskId },
    WorkerStopped,
    RecoveryRequired,
}

impl fmt::Display for ExplorerTaskSubmitError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Busy { active } => {
                write!(formatter, "Explorer task {} is still running", active.0)
            }
            Self::WorkerStopped => formatter.write_str("the Explorer task worker has stopped"),
            Self::RecoveryRequired => formatter.write_str(
                "Explorer mutations are disabled until the interrupted operation is reconciled",
            ),
        }
    }
}

impl std::error::Error for ExplorerTaskSubmitError {}

#[derive(Debug, Clone)]
pub struct ExplorerCancellationToken {
    cancelled: Arc<AtomicBool>,
}

impl ExplorerCancellationToken {
    fn new() -> Self {
        Self {
            cancelled: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }
}

#[derive(Debug, Clone)]
pub struct ExplorerTaskHandle {
    pub id: ExplorerTaskId,
    pub cancellation: ExplorerCancellationToken,
}

/// A trash adapter used while replacing an existing transfer destination.
///
/// User-initiated delete tasks bypass this adapter and call [`Platform::move_to_trash`] directly.
/// Native shells should use [`SystemExplorerTrash`] so replacement victims also enter the
/// operating system Trash instead of a Tundra-private directory.
pub trait ExplorerTrash: Send + Sync + 'static {
    fn move_to_trash(
        &self,
        platform: &dyn Platform,
        path: &Path,
    ) -> Result<PathBuf, ExplorerTaskError>;

    /// Whether the returned path can be renamed back to the original destination.
    ///
    /// Native system Trash APIs intentionally do not expose their internal storage path, so the
    /// safe recovery rule is to leave the previous item in Trash if a later staged commit fails.
    fn has_rollback_path(&self) -> bool {
        true
    }
}

#[derive(Debug, Default)]
pub struct UnavailableExplorerTrash;

impl ExplorerTrash for UnavailableExplorerTrash {
    fn move_to_trash(
        &self,
        _platform: &dyn Platform,
        path: &Path,
    ) -> Result<PathBuf, ExplorerTaskError> {
        Err(ExplorerTaskError::TrashUnavailable {
            path: path.to_path_buf(),
        })
    }
}

/// Sends replacement victims to the operating system Trash.
///
/// The returned path is only a best-effort rollback hint. Native Trash APIs intentionally hide
/// their storage paths; if the subsequent transfer commit fails, the original remains safely in
/// the system Trash and can be restored by the user.
#[derive(Debug, Default)]
pub struct SystemExplorerTrash;

impl ExplorerTrash for SystemExplorerTrash {
    fn move_to_trash(
        &self,
        platform: &dyn Platform,
        path: &Path,
    ) -> Result<PathBuf, ExplorerTaskError> {
        platform.move_to_trash(&[path.to_path_buf()])?;
        Ok(path.to_path_buf())
    }

    fn has_rollback_path(&self) -> bool {
        false
    }
}

enum WorkerCommand {
    Run {
        id: ExplorerTaskId,
        plan: ExplorerTaskPlan,
        cancellation: ExplorerCancellationToken,
    },
    Shutdown,
}

struct ActiveTask {
    id: ExplorerTaskId,
    cancellation: ExplorerCancellationToken,
}

/// Dedicated single-worker queue. `submit` rejects a second mutation instead of queueing it.
pub struct ExplorerTaskEngine {
    command_tx: mpsc::Sender<WorkerCommand>,
    event_rx: mpsc::Receiver<ExplorerTaskEvent>,
    busy: Arc<AtomicBool>,
    active: Arc<Mutex<Option<ActiveTask>>>,
    mutations_blocked: Arc<AtomicBool>,
    next_id: AtomicU64,
    worker: Option<ManagedThreadHandle<()>>,
}

impl ExplorerTaskEngine {
    pub fn new(platform: Arc<dyn Platform>, trash: Arc<dyn ExplorerTrash>) -> Self {
        let process = ProcessWatchdog::global()
            .expect("the process watchdog must be installed before starting Explorer tasks");
        let watchdog = process
            .register_app(explorer_watchdog_descriptor())
            .expect("Explorer watchdog registration must be consistent");
        Self::new_managed(platform, trash, watchdog)
            .expect("the managed Explorer worker must start")
    }

    pub fn new_managed(
        platform: Arc<dyn Platform>,
        trash: Arc<dyn ExplorerTrash>,
        watchdog: AppWatchdog,
    ) -> Result<Self, WatchdogError> {
        let (command_tx, command_rx) = mpsc::channel();
        let (event_tx, event_rx) = mpsc::channel();
        let busy = Arc::new(AtomicBool::new(false));
        let active = Arc::new(Mutex::new(None));
        let worker_busy = Arc::clone(&busy);
        let worker_active = Arc::clone(&active);
        let mutations_blocked = Arc::new(AtomicBool::new(false));
        let worker_mutations_blocked = Arc::clone(&mutations_blocked);
        watchdog.register_recovery_handler(
            explorer_operation_kind(),
            Arc::new(ExplorerRecoveryHandler),
        )?;
        let worker_watchdog = watchdog.child_component(ComponentId::from_static("filesystem"));
        let task_group = worker_watchdog.task_group(&format!(
            "worker-{}",
            ENGINE_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        let mut worker_inputs = Some((
            platform,
            trash,
            command_rx,
            event_tx,
            worker_busy,
            worker_active,
            worker_mutations_blocked,
            worker_watchdog,
        ));
        let worker = task_group.spawn_thread(
            TaskSpec {
                id: TaskId::from_static("event-loop"),
                kind: TaskKind::LongRunning,
                panic_action: PanicAction::ReportOnly,
                replay_safety: ReplaySafety::Checkpointed(explorer_operation_kind()),
                restart_policy: RestartPolicy::never(),
            },
            move || {
                let (
                    platform,
                    trash,
                    command_rx,
                    event_tx,
                    worker_busy,
                    worker_active,
                    worker_mutations_blocked,
                    worker_watchdog,
                ) = worker_inputs
                    .take()
                    .expect("the non-restartable Explorer worker factory runs once");
                worker_loop(
                    platform,
                    trash,
                    command_rx,
                    event_tx,
                    worker_busy,
                    worker_active,
                    worker_mutations_blocked,
                    worker_watchdog,
                )
            },
        )?;
        Ok(Self {
            command_tx,
            event_rx,
            busy,
            active,
            mutations_blocked,
            next_id: AtomicU64::new(1),
            worker: Some(worker),
        })
    }

    pub fn without_trash(platform: Arc<dyn Platform>) -> Self {
        Self::new(platform, Arc::new(UnavailableExplorerTrash))
    }

    pub fn submit(
        &self,
        plan: ExplorerTaskPlan,
    ) -> Result<ExplorerTaskHandle, ExplorerTaskSubmitError> {
        if self.mutations_blocked.load(Ordering::Acquire) {
            return Err(ExplorerTaskSubmitError::RecoveryRequired);
        }
        if self
            .busy
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            let active = self
                .active
                .lock()
                .expect("Explorer active-task lock poisoned")
                .as_ref()
                .map(|task| task.id)
                .unwrap_or(ExplorerTaskId(0));
            return Err(ExplorerTaskSubmitError::Busy { active });
        }

        let id = ExplorerTaskId(self.next_id.fetch_add(1, Ordering::Relaxed));
        let cancellation = ExplorerCancellationToken::new();
        *self
            .active
            .lock()
            .expect("Explorer active-task lock poisoned") = Some(ActiveTask {
            id,
            cancellation: cancellation.clone(),
        });
        if self
            .command_tx
            .send(WorkerCommand::Run {
                id,
                plan,
                cancellation: cancellation.clone(),
            })
            .is_err()
        {
            self.busy.store(false, Ordering::Release);
            *self
                .active
                .lock()
                .expect("Explorer active-task lock poisoned") = None;
            return Err(ExplorerTaskSubmitError::WorkerStopped);
        }
        Ok(ExplorerTaskHandle { id, cancellation })
    }

    pub fn is_busy(&self) -> bool {
        self.busy.load(Ordering::Acquire)
    }

    pub fn active_task_id(&self) -> Option<ExplorerTaskId> {
        self.active
            .lock()
            .expect("Explorer active-task lock poisoned")
            .as_ref()
            .map(|task| task.id)
    }

    pub fn cancel_active(&self) -> bool {
        let guard = self
            .active
            .lock()
            .expect("Explorer active-task lock poisoned");
        if let Some(active) = guard.as_ref() {
            active.cancellation.cancel();
            true
        } else {
            false
        }
    }

    pub fn try_recv(&self) -> Result<ExplorerTaskEvent, mpsc::TryRecvError> {
        self.event_rx.try_recv()
    }

    pub fn recv_timeout(
        &self,
        timeout: Duration,
    ) -> Result<ExplorerTaskEvent, mpsc::RecvTimeoutError> {
        self.event_rx.recv_timeout(timeout)
    }
}

impl Drop for ExplorerTaskEngine {
    fn drop(&mut self) {
        self.cancel_active();
        let _ = self.command_tx.send(WorkerCommand::Shutdown);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn worker_loop(
    platform: Arc<dyn Platform>,
    trash: Arc<dyn ExplorerTrash>,
    command_rx: mpsc::Receiver<WorkerCommand>,
    event_tx: mpsc::Sender<ExplorerTaskEvent>,
    busy: Arc<AtomicBool>,
    active: Arc<Mutex<Option<ActiveTask>>>,
    mutations_blocked: Arc<AtomicBool>,
    watchdog: AppWatchdog,
) {
    while let Ok(command) = command_rx.recv() {
        match command {
            WorkerCommand::Run {
                id,
                plan,
                cancellation,
            } => {
                send_event(&event_tx, ExplorerTaskEvent::Accepted { id });
                let operation_descriptor = operation_descriptor(id, &plan);
                let mut journal = match watchdog.begin_operation(operation_descriptor) {
                    Ok(journal) => journal,
                    Err(error) => {
                        let mut summary = ExplorerTaskSummary::empty();
                        summary.fatal_error = Some(ExplorerTaskError::Journal {
                            message: error.to_string(),
                        });
                        clear_active_task(&active, &busy);
                        send_event(&event_tx, ExplorerTaskEvent::Finished { id, summary });
                        continue;
                    }
                };
                let result = watchdog.run_boundary(
                    BoundarySpec::new(format!("explorer.operation.{}", id.0), BoundaryKind::Worker),
                    AssertUnwindSafe(|| {
                        execute_task(
                            id,
                            plan,
                            cancellation,
                            platform.as_ref(),
                            trash.as_ref(),
                            &event_tx,
                            &mut journal,
                        )
                    }),
                );
                let summary = match result {
                    Ok(mut summary) => {
                        let incomplete = summary.cancelled
                            || summary.fatal_error.is_some()
                            || summary.failed_items > 0;
                        if incomplete {
                            let recovery = ExplorerRecoveryHandler.recover(journal.record());
                            mutations_blocked.store(!recovery.is_recovered(), Ordering::Release);
                            if recovery.is_recovered() {
                                if let Err(error) =
                                    journal.commit("Explorer incomplete operation reconciled")
                                {
                                    mutations_blocked.store(true, Ordering::Release);
                                    summary
                                        .fatal_error
                                        .get_or_insert(ExplorerTaskError::Journal {
                                            message: error.to_string(),
                                        });
                                }
                            } else {
                                let recovery_error = ExplorerTaskError::RecoveryRequired {
                                    message: format!("{recovery:?}"),
                                };
                                watchdog.report_error(
                                    ErrorContext::new(
                                        format!("explorer.operation.{}.recovery", id.0),
                                        IncidentSeverity::Critical,
                                    ),
                                    &recovery_error,
                                );
                                summary.fatal_error.get_or_insert(recovery_error);
                            }
                        } else if let Err(error) = journal.commit("Explorer operation finished") {
                            mutations_blocked.store(true, Ordering::Release);
                            summary.fatal_error = Some(ExplorerTaskError::Journal {
                                message: error.to_string(),
                            });
                        }
                        Some(summary)
                    }
                    Err(caught) => {
                        let incident_id = caught.incident_id().to_string();
                        let message = caught.payload().to_string();
                        let recovery = ExplorerRecoveryHandler.recover(journal.record());
                        mutations_blocked.store(!recovery.is_recovered(), Ordering::Release);
                        let _ = caught.finalize(recovery.clone());
                        if recovery.is_recovered() {
                            let _ = journal.commit("Explorer panic checkpoint reconciled");
                        }
                        send_event(
                            &event_tx,
                            ExplorerTaskEvent::Panicked {
                                id,
                                incident_id,
                                message,
                                recovery,
                            },
                        );
                        None
                    }
                };
                clear_active_task(&active, &busy);
                if let Some(summary) = summary {
                    send_event(&event_tx, ExplorerTaskEvent::Finished { id, summary });
                }
            }
            WorkerCommand::Shutdown => break,
        }
    }
}

pub fn explorer_watchdog_descriptor() -> AppDescriptor {
    AppDescriptor::new(
        AppId::from_static("explorer"),
        "Tundra Explorer",
        env!("CARGO_PKG_VERSION"),
        AppCriticality::SessionCritical,
    )
}

pub fn explorer_operation_kind() -> OperationKind {
    OperationKind::from_static("explorer.filesystem.v1")
}

fn clear_active_task(active: &Mutex<Option<ActiveTask>>, busy: &AtomicBool) {
    *active
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = None;
    busy.store(false, Ordering::Release);
}

fn operation_descriptor(id: ExplorerTaskId, plan: &ExplorerTaskPlan) -> OperationDescriptor {
    let (summary, payload) = match plan {
        ExplorerTaskPlan::Transfer(plan) => (
            format!("{:?} {} item(s)", plan.operation, plan.sources.len()),
            serde_json::json!({
                "task_id": id.0,
                "operation": format!("{:?}", plan.operation),
                "sources": plan.sources.iter().map(|path| path.display().to_string()).collect::<Vec<_>>(),
                "destination": plan.destination.display().to_string(),
            }),
        ),
        ExplorerTaskPlan::DeleteToTrash(plan) => (
            format!("Delete {} item(s) to Trash", plan.paths.len()),
            serde_json::json!({
                "task_id": id.0,
                "operation": "delete_to_trash",
                "sources": plan.paths.iter().map(|path| path.display().to_string()).collect::<Vec<_>>(),
            }),
        ),
    };
    OperationDescriptor::new(explorer_operation_kind(), summary, payload)
}

struct ExplorerRecoveryHandler;

impl RecoveryHandler for ExplorerRecoveryHandler {
    fn version(&self) -> &str {
        "1"
    }

    fn recover(&self, record: &OperationRecord) -> RecoveryOutcome {
        let phase = record.checkpoint.phase.as_str();
        let payload = &record.checkpoint.payload;
        let source = checkpoint_path(payload, "source");
        let target = checkpoint_path(payload, "target");
        let staging = checkpoint_path(payload, "staging");

        match phase {
            "planned" | "planning" => RecoveryOutcome::Recovered(
                "the operation had not reached a filesystem mutation".to_string(),
            ),
            "copy_staging_writing" | "copy_staging_synced" => {
                let Some(staging) = staging else {
                    return missing_checkpoint_path(record, "staging");
                };
                match remove_exact_staging(&staging) {
                    Ok(()) => RecoveryOutcome::Recovered(
                        "the uncommitted staged copy was removed; the source and old destination were preserved"
                            .to_string(),
                    ),
                    Err(error) => RecoveryOutcome::ManualActionRequired(error),
                }
            }
            "move_rename_pending" => match (source.as_ref(), target.as_ref()) {
                (Some(source), Some(target)) if source.exists() && !target.exists() => {
                    RecoveryOutcome::Recovered(
                        "the move had not committed and the source remains in place".to_string(),
                    )
                }
                (Some(source), Some(target)) if !source.exists() && target.exists() => {
                    RecoveryOutcome::RecoveredWithWarnings(
                        "the atomic move committed before the panic".to_string(),
                    )
                }
                _ => RecoveryOutcome::ManualActionRequired(
                    "the move source/target state is ambiguous; no path was changed during recovery"
                        .to_string(),
                ),
            },
            "move_source_staging" | "move_source_staged" => {
                let (Some(source), Some(target), Some(staging)) = (source, target, staging) else {
                    return missing_checkpoint_path(record, "source/target/staging");
                };
                if source.exists() && !staging.exists() {
                    return RecoveryOutcome::Recovered(
                        "the source was not moved into staging".to_string(),
                    );
                }
                if !source.exists() && staging.exists() && target.exists() {
                    return match fs::rename(&staging, &source) {
                        Ok(()) => RecoveryOutcome::Recovered(
                            "the staged move source was restored to its original path".to_string(),
                        ),
                        Err(error) => RecoveryOutcome::ManualActionRequired(format!(
                            "could not restore {} to {}: {error}",
                            staging.display(),
                            source.display()
                        )),
                    };
                }
                RecoveryOutcome::ManualActionRequired(
                    "the replacement move changed externally; staged and destination paths were preserved"
                        .to_string(),
                )
            }
            "destination_trash_pending" => {
                let (Some(target), Some(staging)) = (target, staging) else {
                    return missing_checkpoint_path(record, "target/staging");
                };
                if !target.exists() && staging.exists() {
                    return match fs::rename(&staging, &target) {
                        Ok(()) => RecoveryOutcome::RecoveredWithWarnings(
                            "the synchronized staged replacement was committed after the previous destination entered Trash"
                                .to_string(),
                        ),
                        Err(error) => RecoveryOutcome::ManualActionRequired(format!(
                            "could not commit staged replacement {} to {}: {error}",
                            staging.display(),
                            target.display()
                        )),
                    };
                }
                if target.exists() && staging.exists() {
                    return match remove_exact_staging(&staging) {
                        Ok(()) => RecoveryOutcome::Recovered(
                            "the previous destination remained in place and the uncommitted staged replacement was removed"
                                .to_string(),
                        ),
                        Err(error) => RecoveryOutcome::ManualActionRequired(error),
                    };
                }
                RecoveryOutcome::ManualActionRequired(
                    "the destination changed while entering Trash; all remaining paths were preserved"
                        .to_string(),
                )
            }
            "destination_trashed" => {
                let (Some(target), Some(staging)) = (target, staging) else {
                    return missing_checkpoint_path(record, "target/staging");
                };
                if !target.exists() && staging.exists() {
                    return match fs::rename(&staging, &target) {
                        Ok(()) => RecoveryOutcome::RecoveredWithWarnings(
                            "the fully staged replacement was committed after the previous destination entered Trash"
                                .to_string(),
                        ),
                        Err(error) => RecoveryOutcome::ManualActionRequired(format!(
                            "could not commit staged replacement {} to {}: {error}",
                            staging.display(),
                            target.display()
                        )),
                    };
                }
                if target.exists() && !staging.exists() {
                    return RecoveryOutcome::RecoveredWithWarnings(
                        "the replacement target was already committed".to_string(),
                    );
                }
                RecoveryOutcome::ManualActionRequired(
                    "the destination and staging state is ambiguous; both were preserved".to_string(),
                )
            }
            "target_committed" | "source_removed" => match target {
                Some(target) if target.exists() => RecoveryOutcome::RecoveredWithWarnings(
                    "the destination commit completed before the panic".to_string(),
                ),
                _ => RecoveryOutcome::ManualActionRequired(
                    "the journal says the target committed but it is no longer present".to_string(),
                ),
            },
            "cross_volume_target_committed" => match (source, target) {
                (Some(source), Some(target)) if source.exists() && target.exists() => {
                    RecoveryOutcome::ManualActionRequired(format!(
                        "both {} and {} were preserved; verify the copy before removing the old source",
                        source.display(),
                        target.display()
                    ))
                }
                (Some(source), Some(target)) if !source.exists() && target.exists() => {
                    RecoveryOutcome::RecoveredWithWarnings(
                        "the cross-volume move completed before the panic".to_string(),
                    )
                }
                _ => RecoveryOutcome::ManualActionRequired(
                    "the cross-volume move state is ambiguous; no recovery deletion was attempted"
                        .to_string(),
                ),
            },
            "delete_pending" => match source {
                Some(source) if source.exists() => RecoveryOutcome::Recovered(
                    "the delete had not committed; the source remains present".to_string(),
                ),
                Some(_) => RecoveryOutcome::RecoveredWithWarnings(
                    "the item is absent and is treated as already moved to Trash".to_string(),
                ),
                None => missing_checkpoint_path(record, "source"),
            },
            "delete_committed" => RecoveryOutcome::RecoveredWithWarnings(
                "the item was already moved to Trash".to_string(),
            ),
            "directory_replace_pending" => match target {
                Some(target) if target.exists() => RecoveryOutcome::Recovered(
                    "the destination directory was not moved to Trash".to_string(),
                ),
                Some(_) => RecoveryOutcome::ManualActionRequired(
                    "the previous destination directory may have entered Trash; no automatic replacement was attempted"
                        .to_string(),
                ),
                None => missing_checkpoint_path(record, "target"),
            },
            "directory_destination_trashed" => RecoveryOutcome::ManualActionRequired(
                "the previous destination directory entered Trash before the new directory was complete; existing paths were preserved"
                    .to_string(),
            ),
            "directory_create_pending" => {
                let replaced = payload
                    .get("replaced")
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or(false);
                match target {
                    Some(target) if target.exists() => RecoveryOutcome::ManualActionRequired(
                        "the destination directory may contain only part of the requested tree; it was preserved for inspection"
                            .to_string(),
                    ),
                    Some(_) if replaced => RecoveryOutcome::ManualActionRequired(
                        "the previous destination entered Trash and the replacement directory was not created"
                            .to_string(),
                    ),
                    Some(_) => RecoveryOutcome::Recovered(
                        "the destination directory was not created, so no filesystem mutation remains"
                            .to_string(),
                    ),
                    None => missing_checkpoint_path(record, "target"),
                }
            }
            "directory_target_created" => RecoveryOutcome::ManualActionRequired(
                "the destination directory may contain a partially completed child set; it was preserved and writes remain disabled pending review"
                    .to_string(),
            ),
            _ => RecoveryOutcome::ManualActionRequired(format!(
                "no safe recovery rule exists for Explorer checkpoint phase {phase}"
            )),
        }
    }
}

fn checkpoint_path(payload: &serde_json::Value, key: &str) -> Option<PathBuf> {
    payload
        .get(key)
        .and_then(|value| value.as_str())
        .map(PathBuf::from)
}

fn missing_checkpoint_path(record: &OperationRecord, field: &str) -> RecoveryOutcome {
    RecoveryOutcome::ManualActionRequired(format!(
        "operation {} is missing recovery field {field}",
        record.operation_id
    ))
}

fn remove_exact_staging(path: &Path) -> Result<(), String> {
    if !path.exists() {
        return Ok(());
    }
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    if !name.starts_with(".tundra-stage-") {
        return Err(format!(
            "refused to remove non-staging recovery path {}",
            path.display()
        ));
    }
    if path.is_dir() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    }
    .map_err(|error| format!("could not remove staged path {}: {error}", path.display()))
}

fn execute_task(
    id: ExplorerTaskId,
    plan: ExplorerTaskPlan,
    cancellation: ExplorerCancellationToken,
    platform: &dyn Platform,
    trash: &dyn ExplorerTrash,
    event_tx: &mpsc::Sender<ExplorerTaskEvent>,
    journal: &mut OperationGuard,
) -> ExplorerTaskSummary {
    let _ = journal.checkpoint(OperationCheckpoint::new(
        "planning",
        serde_json::json!({ "task_id": id.0 }),
    ));
    send_event(
        event_tx,
        ExplorerTaskEvent::PhaseChanged {
            id,
            phase: ExplorerTaskPhase::Planning,
        },
    );
    if cancellation.is_cancelled() {
        let mut summary = ExplorerTaskSummary::empty();
        summary.cancelled = true;
        summary.fatal_error = Some(ExplorerTaskError::Cancelled);
        return summary;
    }

    match plan {
        ExplorerTaskPlan::Transfer(plan) => {
            let prepared = match prepare_transfer(id, platform, &plan, &cancellation, event_tx) {
                Ok(prepared) => prepared,
                Err(error) => return fatal_summary(error),
            };
            let mut context = ExecutionContext::new(
                id,
                platform,
                trash,
                cancellation,
                event_tx,
                prepared.total_items,
                prepared.total_bytes,
                plan.chunk_size,
                journal,
            );
            context.phase(ExplorerTaskPhase::Executing);
            for node in &prepared.roots {
                if let Err(ExplorerTaskError::Cancelled) = context.check_cancel() {
                    context.summary.cancelled = true;
                    break;
                }
                let failures_before = context.summary.failed_items;
                let clean = context.run_node(node, plan.operation);
                if clean && context.summary.failed_items == failures_before {
                    context.summary.succeeded_sources.push(node.source.clone());
                } else {
                    context.summary.failed_sources.push(node.source.clone());
                }
                if context.summary.cancelled {
                    break;
                }
            }
            context.summary.processed_bytes = context.progress.processed_bytes;
            context.summary
        }
        ExplorerTaskPlan::DeleteToTrash(plan) => {
            let paths = match prepare_delete(id, platform, &plan, &cancellation, event_tx) {
                Ok(paths) => paths,
                Err(error) => return fatal_summary(error),
            };
            let total_bytes = paths.iter().map(|(_, attributes)| attributes.len).sum();
            let mut context = ExecutionContext::new(
                id,
                platform,
                trash,
                cancellation,
                event_tx,
                paths.len() as u64,
                total_bytes,
                DEFAULT_TRANSFER_CHUNK_SIZE,
                journal,
            );
            context.phase(ExplorerTaskPhase::Executing);
            for (path, attributes) in paths {
                if context.check_cancel().is_err() {
                    context.summary.cancelled = true;
                    break;
                }
                if let Err(error) = context.checkpoint(
                    "delete_pending",
                    serde_json::json!({ "source": path.display().to_string() }),
                ) {
                    context.record_failure(&path, None, error);
                    context.summary.failed_sources.push(path);
                    continue;
                }
                match platform.move_to_trash(&[path.clone()]) {
                    Ok(()) => {
                        let _ = context.checkpoint(
                            "delete_committed",
                            serde_json::json!({ "source": path.display().to_string() }),
                        );
                        context.progress.processed_bytes = context
                            .progress
                            .processed_bytes
                            .saturating_add(attributes.len);
                        context.record_success(&path, None);
                        context.summary.succeeded_sources.push(path);
                    }
                    Err(error) => {
                        context.record_failure(&path, None, error.into());
                        context.summary.failed_sources.push(path);
                    }
                }
            }
            context.summary.processed_bytes = context.progress.processed_bytes;
            context.summary
        }
    }
}

fn fatal_summary(error: ExplorerTaskError) -> ExplorerTaskSummary {
    let mut summary = ExplorerTaskSummary::empty();
    summary.cancelled = matches!(error, ExplorerTaskError::Cancelled);
    summary.fatal_error = Some(error);
    summary
}

#[derive(Debug, Clone)]
struct PreparedTransfer {
    roots: Vec<PreparedNode>,
    total_items: u64,
    total_bytes: u64,
}

#[derive(Debug, Clone)]
struct PreparedNode {
    source: PathBuf,
    target: PathBuf,
    attributes: FileAttributes,
    disposition: PreparedDisposition,
}

#[derive(Debug, Clone)]
enum PreparedDisposition {
    Skip,
    File {
        replace: bool,
    },
    Link {
        replace: bool,
    },
    Directory {
        replace: bool,
        merge: bool,
        children: Vec<PreparedNode>,
    },
}

impl PreparedNode {
    fn item_count(&self) -> u64 {
        match &self.disposition {
            PreparedDisposition::Directory { children, .. } => {
                1 + children.iter().map(Self::item_count).sum::<u64>()
            }
            _ => 1,
        }
    }

    fn byte_count(&self) -> u64 {
        match &self.disposition {
            PreparedDisposition::File { .. } => self.attributes.len,
            PreparedDisposition::Directory { children, .. } => {
                children.iter().map(Self::byte_count).sum()
            }
            _ => 0,
        }
    }

    fn contains_unsafe_link(&self) -> bool {
        match &self.disposition {
            PreparedDisposition::Link { .. } => true,
            PreparedDisposition::Directory { children, .. } => {
                children.iter().any(Self::contains_unsafe_link)
            }
            _ => false,
        }
    }

    fn merges_directory(&self) -> bool {
        matches!(
            self.disposition,
            PreparedDisposition::Directory { merge: true, .. }
        )
    }
}

struct PlanningContext<'a> {
    id: ExplorerTaskId,
    platform: &'a dyn Platform,
    collision_policy: &'a ExplorerCollisionPolicy,
    cancellation: &'a ExplorerCancellationToken,
    event_tx: &'a mpsc::Sender<ExplorerTaskEvent>,
    reserved_targets: BTreeSet<PathBuf>,
    windows_paths: bool,
    discovered_items: u64,
    discovered_bytes: u64,
}

fn prepare_transfer(
    id: ExplorerTaskId,
    platform: &dyn Platform,
    plan: &ExplorerTransferPlan,
    cancellation: &ExplorerCancellationToken,
    event_tx: &mpsc::Sender<ExplorerTaskEvent>,
) -> Result<PreparedTransfer, ExplorerTaskError> {
    if plan.sources.is_empty() {
        return Err(ExplorerTaskError::InvalidPlan {
            message: "a transfer requires at least one source".to_string(),
        });
    }
    if plan.chunk_size == 0 {
        return Err(ExplorerTaskError::InvalidPlan {
            message: "the transfer chunk size must be greater than zero".to_string(),
        });
    }

    let destination =
        canonical_existing_no_follow(&plan.destination, "resolve transfer destination")?;
    let destination_attributes = platform.file_attributes(&destination)?;
    if !destination_attributes.is_dir || is_unsafe_link(&destination_attributes) {
        return Err(ExplorerTaskError::InvalidPlan {
            message: format!(
                "the transfer destination {} is not a safe directory",
                destination.display()
            ),
        });
    }
    if destination_attributes.readonly {
        return Err(ExplorerTaskError::InvalidPlan {
            message: format!(
                "the transfer destination {} is read-only",
                destination.display()
            ),
        });
    }

    let windows_paths = platform.kind() == PlatformKind::Windows;
    let mut sources = Vec::with_capacity(plan.sources.len());
    for source in &plan.sources {
        if cancellation.is_cancelled() {
            return Err(ExplorerTaskError::Cancelled);
        }
        let canonical = canonical_existing_no_follow(source, "resolve transfer source")?;
        let attributes = platform.file_attributes(&canonical)?;
        sources.push((canonical, attributes));
    }
    validate_source_set(&sources, &destination, windows_paths)?;

    let mut planning = PlanningContext {
        id,
        platform,
        collision_policy: &plan.collisions,
        cancellation,
        event_tx,
        reserved_targets: BTreeSet::new(),
        windows_paths,
        discovered_items: 0,
        discovered_bytes: 0,
    };
    let mut roots = Vec::with_capacity(sources.len());
    for (source, attributes) in sources {
        let name = source
            .file_name()
            .ok_or_else(|| ExplorerTaskError::InvalidPlan {
                message: format!("{} has no transferable file name", source.display()),
            })?;
        let target = destination.join(name);
        if plan.operation == ExplorerTransferOperation::Move
            && paths_equal(&source, &target, windows_paths)
        {
            return Err(ExplorerTaskError::InvalidPlan {
                message: format!("{} is already in the destination", source.display()),
            });
        }
        let node = planning.prepare_node(source, target, attributes, false)?;
        if plan.operation == ExplorerTransferOperation::Copy && node.contains_unsafe_link() {
            return Err(ExplorerTaskError::UnsafeLink {
                path: first_unsafe_path(&node).unwrap_or_else(|| node.source.clone()),
            });
        }
        // A merged directory is committed child-by-child rather than through one atomic rename.
        // Without a portable no-mutation volume probe, allowing a link/reparse child could move
        // earlier siblings before a later cross-volume link is rejected. Block the whole plan at
        // planning time so unsafe trees never produce partial moves.
        if plan.operation == ExplorerTransferOperation::Move
            && node.merges_directory()
            && node.contains_unsafe_link()
        {
            return Err(ExplorerTaskError::UnsafeLink {
                path: first_unsafe_path(&node).unwrap_or_else(|| node.source.clone()),
            });
        }
        roots.push(node);
    }
    let total_items = roots.iter().map(PreparedNode::item_count).sum();
    let total_bytes = roots.iter().map(PreparedNode::byte_count).sum();
    Ok(PreparedTransfer {
        roots,
        total_items,
        total_bytes,
    })
}

impl PlanningContext<'_> {
    fn prepare_node(
        &mut self,
        source: PathBuf,
        requested_target: PathBuf,
        attributes: FileAttributes,
        assume_target_absent: bool,
    ) -> Result<PreparedNode, ExplorerTaskError> {
        if self.cancellation.is_cancelled() {
            return Err(ExplorerTaskError::Cancelled);
        }
        self.discovered_items = self.discovered_items.saturating_add(1);
        if attributes.is_file && !is_unsafe_link(&attributes) {
            self.discovered_bytes = self.discovered_bytes.saturating_add(attributes.len);
        }
        send_event(
            self.event_tx,
            ExplorerTaskEvent::PlanningProgress {
                id: self.id,
                discovered_items: self.discovered_items,
                discovered_bytes: self.discovered_bytes,
                current_path: source.clone(),
            },
        );
        let source_is_directory = attributes.is_dir && !is_unsafe_link(&attributes);
        let target_attributes = if assume_target_absent {
            None
        } else {
            optional_attributes(self.platform, &requested_target)?
        };
        let already_reserved = self
            .reserved_targets
            .iter()
            .any(|reserved| paths_equal(reserved, &requested_target, self.windows_paths));

        let self_target = paths_equal(&source, &requested_target, self.windows_paths);
        let target_is_safe_directory = target_attributes
            .as_ref()
            .is_some_and(|target| target.is_dir && !is_unsafe_link(target));
        let merge =
            source_is_directory && target_is_safe_directory && !already_reserved && !self_target;
        let (target, replace, skip) = if merge || (target_attributes.is_none() && !already_reserved)
        {
            (requested_target.clone(), false, false)
        } else {
            match self.collision_policy.resolution_for(&requested_target) {
                ExplorerCollisionResolution::KeepBoth => (
                    unique_keep_both_path(
                        &requested_target,
                        &self.reserved_targets,
                        self.windows_paths,
                    ),
                    false,
                    false,
                ),
                ExplorerCollisionResolution::Replace if self_target => {
                    return Err(ExplorerTaskError::InvalidPlan {
                        message: format!("{} cannot replace itself", source.display()),
                    });
                }
                ExplorerCollisionResolution::Replace if already_reserved => {
                    return Err(ExplorerTaskError::InvalidPlan {
                        message: format!(
                            "multiple sources would replace the same target {}",
                            requested_target.display()
                        ),
                    });
                }
                ExplorerCollisionResolution::Replace => (requested_target.clone(), true, false),
                ExplorerCollisionResolution::Skip => (requested_target.clone(), false, true),
                ExplorerCollisionResolution::Cancel => {
                    return Err(ExplorerTaskError::CollisionCancelled {
                        path: requested_target,
                    });
                }
            }
        };
        self.reserved_targets.insert(target.clone());

        if skip {
            return Ok(PreparedNode {
                source,
                target,
                attributes,
                disposition: PreparedDisposition::Skip,
            });
        }
        if is_unsafe_link(&attributes) {
            return Ok(PreparedNode {
                source,
                target,
                attributes,
                disposition: PreparedDisposition::Link { replace },
            });
        }
        if attributes.is_dir {
            let listing = self.platform.read_directory(&source)?;
            if !listing.warnings.is_empty() {
                return Err(ExplorerTaskError::InvalidPlan {
                    message: format!(
                        "{} could not be completely scanned: {}",
                        source.display(),
                        listing.warnings[0].message
                    ),
                });
            }
            let mut entries = listing.entries;
            entries.sort_by(|left, right| left.name.cmp(&right.name));
            let mut children = Vec::with_capacity(entries.len());
            for entry in entries {
                let child_attributes =
                    entry
                        .attributes
                        .ok_or_else(|| ExplorerTaskError::InvalidPlan {
                            message: format!(
                                "metadata is unavailable for {}",
                                entry.path.display()
                            ),
                        })?;
                let child_target = target.join(&entry.name);
                children.push(self.prepare_node(
                    entry.path,
                    child_target,
                    child_attributes,
                    !merge,
                )?);
            }
            Ok(PreparedNode {
                source,
                target,
                attributes,
                disposition: PreparedDisposition::Directory {
                    replace,
                    merge,
                    children,
                },
            })
        } else {
            Ok(PreparedNode {
                source,
                target,
                attributes,
                disposition: PreparedDisposition::File { replace },
            })
        }
    }
}

fn prepare_delete(
    id: ExplorerTaskId,
    platform: &dyn Platform,
    plan: &ExplorerDeletePlan,
    cancellation: &ExplorerCancellationToken,
    event_tx: &mpsc::Sender<ExplorerTaskEvent>,
) -> Result<Vec<(PathBuf, FileAttributes)>, ExplorerTaskError> {
    if plan.paths.is_empty() {
        return Err(ExplorerTaskError::InvalidPlan {
            message: "a delete task requires at least one path".to_string(),
        });
    }
    let windows_paths = platform.kind() == PlatformKind::Windows;
    let mut paths: Vec<(PathBuf, FileAttributes)> = Vec::with_capacity(plan.paths.len());
    for path in &plan.paths {
        if cancellation.is_cancelled() {
            return Err(ExplorerTaskError::Cancelled);
        }
        let canonical = canonical_existing_no_follow(path, "resolve delete path")?;
        let attributes = platform.file_attributes(&canonical)?;
        let discovered_items = paths.len() as u64 + 1;
        let discovered_bytes = paths
            .iter()
            .map(|(_, attributes)| attributes.len)
            .sum::<u64>()
            .saturating_add(attributes.len);
        send_event(
            event_tx,
            ExplorerTaskEvent::PlanningProgress {
                id,
                discovered_items,
                discovered_bytes,
                current_path: canonical.clone(),
            },
        );
        paths.push((canonical, attributes));
    }
    for left in 0..paths.len() {
        for right in (left + 1)..paths.len() {
            if path_is_within(&paths[left].0, &paths[right].0, windows_paths)
                || path_is_within(&paths[right].0, &paths[left].0, windows_paths)
            {
                return Err(ExplorerTaskError::InvalidPlan {
                    message: "delete paths must not duplicate or contain one another".to_string(),
                });
            }
        }
    }
    Ok(paths)
}

fn validate_source_set(
    sources: &[(PathBuf, FileAttributes)],
    destination: &Path,
    windows_paths: bool,
) -> Result<(), ExplorerTaskError> {
    for (source, attributes) in sources {
        if paths_equal(source, destination, windows_paths) {
            return Err(ExplorerTaskError::InvalidPlan {
                message: format!("{} cannot be transferred into itself", source.display()),
            });
        }
        if attributes.is_dir
            && !is_unsafe_link(attributes)
            && path_is_within(destination, source, windows_paths)
        {
            return Err(ExplorerTaskError::InvalidPlan {
                message: format!(
                    "{} cannot be transferred into its own descendant {}",
                    source.display(),
                    destination.display()
                ),
            });
        }
    }
    for left in 0..sources.len() {
        for right in (left + 1)..sources.len() {
            if path_is_within(&sources[left].0, &sources[right].0, windows_paths)
                || path_is_within(&sources[right].0, &sources[left].0, windows_paths)
            {
                return Err(ExplorerTaskError::InvalidPlan {
                    message: "transfer sources must not duplicate or contain one another"
                        .to_string(),
                });
            }
        }
    }
    Ok(())
}

struct ExecutionContext<'a> {
    id: ExplorerTaskId,
    platform: &'a dyn Platform,
    trash: &'a dyn ExplorerTrash,
    cancellation: ExplorerCancellationToken,
    event_tx: &'a mpsc::Sender<ExplorerTaskEvent>,
    progress: ExplorerTaskProgress,
    summary: ExplorerTaskSummary,
    chunk_size: usize,
    staging_sequence: u64,
    journal: &'a mut OperationGuard,
}

impl<'a> ExecutionContext<'a> {
    #[allow(clippy::too_many_arguments)]
    fn new(
        id: ExplorerTaskId,
        platform: &'a dyn Platform,
        trash: &'a dyn ExplorerTrash,
        cancellation: ExplorerCancellationToken,
        event_tx: &'a mpsc::Sender<ExplorerTaskEvent>,
        total_items: u64,
        total_bytes: u64,
        chunk_size: usize,
        journal: &'a mut OperationGuard,
    ) -> Self {
        let mut summary = ExplorerTaskSummary::empty();
        summary.total_items = total_items;
        summary.total_bytes = total_bytes;
        Self {
            id,
            platform,
            trash,
            cancellation,
            event_tx,
            progress: ExplorerTaskProgress {
                phase: ExplorerTaskPhase::Planning,
                processed_items: 0,
                total_items,
                processed_bytes: 0,
                total_bytes,
                current_path: None,
            },
            summary,
            chunk_size,
            staging_sequence: 0,
            journal,
        }
    }

    fn checkpoint(
        &mut self,
        phase: &str,
        payload: serde_json::Value,
    ) -> Result<(), ExplorerTaskError> {
        self.journal
            .checkpoint(OperationCheckpoint::new(phase, payload))
            .map_err(|error| ExplorerTaskError::Journal {
                message: error.to_string(),
            })
    }

    fn phase(&mut self, phase: ExplorerTaskPhase) {
        self.progress.phase = phase;
        send_event(
            self.event_tx,
            ExplorerTaskEvent::PhaseChanged { id: self.id, phase },
        );
        self.emit_progress();
    }

    fn check_cancel(&self) -> Result<(), ExplorerTaskError> {
        if self.cancellation.is_cancelled() {
            Err(ExplorerTaskError::Cancelled)
        } else {
            Ok(())
        }
    }

    fn emit_progress(&self) {
        send_event(
            self.event_tx,
            ExplorerTaskEvent::Progress {
                id: self.id,
                progress: self.progress.clone(),
            },
        );
    }

    fn record_success(&mut self, source: &Path, target: Option<&Path>) {
        self.progress.processed_items = self.progress.processed_items.saturating_add(1);
        self.progress.current_path = Some(source.to_path_buf());
        self.summary.succeeded_items = self.summary.succeeded_items.saturating_add(1);
        send_event(
            self.event_tx,
            ExplorerTaskEvent::ItemCompleted {
                id: self.id,
                source: source.to_path_buf(),
                target: target.map(Path::to_path_buf),
            },
        );
        self.emit_progress();
    }

    fn record_skip(&mut self, source: &Path, target: Option<&Path>) {
        self.progress.processed_items = self.progress.processed_items.saturating_add(1);
        self.progress.current_path = Some(source.to_path_buf());
        self.summary.skipped_items = self.summary.skipped_items.saturating_add(1);
        send_event(
            self.event_tx,
            ExplorerTaskEvent::ItemSkipped {
                id: self.id,
                source: source.to_path_buf(),
                target: target.map(Path::to_path_buf),
            },
        );
        self.emit_progress();
    }

    fn record_failure(&mut self, source: &Path, target: Option<&Path>, error: ExplorerTaskError) {
        self.progress.processed_items = self.progress.processed_items.saturating_add(1);
        self.progress.current_path = Some(source.to_path_buf());
        self.summary.failed_items = self.summary.failed_items.saturating_add(1);
        let failure = ExplorerItemFailure {
            source: source.to_path_buf(),
            target: target.map(Path::to_path_buf),
            error,
        };
        self.summary.failures.push(failure.clone());
        send_event(
            self.event_tx,
            ExplorerTaskEvent::ItemFailed {
                id: self.id,
                failure,
            },
        );
        self.emit_progress();
    }

    fn run_node(&mut self, node: &PreparedNode, operation: ExplorerTransferOperation) -> bool {
        if self.check_cancel().is_err() {
            self.summary.cancelled = true;
            return false;
        }
        let result = match operation {
            ExplorerTransferOperation::Copy => self.copy_node(node, true),
            ExplorerTransferOperation::Move => self.move_node(node),
        };
        match result {
            Ok(clean) => clean,
            Err(ExplorerTaskError::Cancelled) => {
                self.summary.cancelled = true;
                false
            }
            Err(error) => {
                self.record_failure(&node.source, Some(&node.target), error);
                false
            }
        }
    }

    fn copy_node(
        &mut self,
        node: &PreparedNode,
        mark_self: bool,
    ) -> Result<bool, ExplorerTaskError> {
        self.check_cancel()?;
        match &node.disposition {
            PreparedDisposition::Skip => {
                if mark_self {
                    self.record_skip(&node.source, Some(&node.target));
                }
                Ok(false)
            }
            PreparedDisposition::Link { .. } => Err(ExplorerTaskError::UnsafeLink {
                path: node.source.clone(),
            }),
            PreparedDisposition::File { replace } => {
                self.copy_file_staged(&node.source, &node.target, *replace)?;
                if mark_self {
                    self.record_success(&node.source, Some(&node.target));
                }
                Ok(true)
            }
            PreparedDisposition::Directory {
                replace,
                merge,
                children,
            } => {
                let trashed = if *replace {
                    self.checkpoint(
                        "directory_replace_pending",
                        serde_json::json!({
                            "source": node.source.display().to_string(),
                            "target": node.target.display().to_string(),
                        }),
                    )?;
                    let trashed = self.trash.move_to_trash(self.platform, &node.target)?;
                    self.checkpoint(
                        "directory_destination_trashed",
                        serde_json::json!({
                            "source": node.source.display().to_string(),
                            "target": node.target.display().to_string(),
                        }),
                    )?;
                    Some(trashed)
                } else {
                    None
                };
                if !*merge || *replace {
                    if path_exists_no_follow(&node.target) {
                        if self.trash.has_rollback_path() {
                            if let Some(trashed) = trashed {
                                let _ = self.platform.rename_path(&trashed, &node.target);
                            }
                        }
                        return Err(ExplorerTaskError::DestinationChanged {
                            path: node.target.clone(),
                        });
                    }
                    self.checkpoint(
                        "directory_create_pending",
                        serde_json::json!({
                            "source": node.source.display().to_string(),
                            "target": node.target.display().to_string(),
                            "replaced": *replace,
                        }),
                    )?;
                    if let Err(error) = fs::create_dir(&node.target) {
                        if let Some(trashed) = trashed {
                            let _ = self.platform.rename_path(&trashed, &node.target);
                        }
                        return Err(io_error(
                            "create destination directory",
                            &node.target,
                            error,
                        ));
                    }
                    self.checkpoint(
                        "directory_target_created",
                        serde_json::json!({
                            "source": node.source.display().to_string(),
                            "target": node.target.display().to_string(),
                            "replaced": *replace,
                        }),
                    )?;
                }
                let mut clean = true;
                for child in children {
                    if !self.run_node(child, ExplorerTransferOperation::Copy) {
                        clean = false;
                    }
                    if self.summary.cancelled {
                        return Err(ExplorerTaskError::Cancelled);
                    }
                }
                if let Ok(metadata) = fs::metadata(&node.source) {
                    let _ = fs::set_permissions(&node.target, metadata.permissions());
                }
                if mark_self {
                    self.record_success(&node.source, Some(&node.target));
                }
                Ok(clean)
            }
        }
    }

    fn move_node(&mut self, node: &PreparedNode) -> Result<bool, ExplorerTaskError> {
        self.check_cancel()?;
        match &node.disposition {
            PreparedDisposition::Skip => {
                self.record_skip(&node.source, Some(&node.target));
                Ok(false)
            }
            PreparedDisposition::Directory {
                merge: true,
                children,
                ..
            } => {
                let mut clean = true;
                for child in children {
                    if !self.run_node(child, ExplorerTransferOperation::Move) {
                        clean = false;
                    }
                    if self.summary.cancelled {
                        return Err(ExplorerTaskError::Cancelled);
                    }
                }
                if clean {
                    fs::remove_dir(&node.source).map_err(|error| {
                        io_error("remove moved source directory", &node.source, error)
                    })?;
                }
                self.record_success(&node.source, Some(&node.target));
                Ok(clean)
            }
            _ => match self.fast_move(node)? {
                FastMove::Moved => {
                    self.record_renamed_subtree(node);
                    Ok(true)
                }
                FastMove::CrossDevice => {
                    if node.contains_unsafe_link() {
                        return Err(ExplorerTaskError::UnsafeLink {
                            path: first_unsafe_path(node).unwrap_or_else(|| node.source.clone()),
                        });
                    }
                    let clean = self.copy_node(node, false)?;
                    if !clean {
                        return Err(ExplorerTaskError::PartialMove {
                            path: node.source.clone(),
                        });
                    }
                    self.check_cancel()?;
                    self.checkpoint(
                        "cross_volume_target_committed",
                        serde_json::json!({
                            "source": node.source.display().to_string(),
                            "target": node.target.display().to_string(),
                        }),
                    )?;
                    remove_path_no_follow(self.platform, &node.source)?;
                    self.checkpoint(
                        "source_removed",
                        serde_json::json!({
                            "source": node.source.display().to_string(),
                            "target": node.target.display().to_string(),
                        }),
                    )?;
                    self.record_success(&node.source, Some(&node.target));
                    Ok(true)
                }
            },
        }
    }

    fn fast_move(&mut self, node: &PreparedNode) -> Result<FastMove, ExplorerTaskError> {
        let replace = match node.disposition {
            PreparedDisposition::File { replace }
            | PreparedDisposition::Link { replace }
            | PreparedDisposition::Directory { replace, .. } => replace,
            PreparedDisposition::Skip => false,
        };
        if !replace {
            self.checkpoint(
                "move_rename_pending",
                serde_json::json!({
                    "source": node.source.display().to_string(),
                    "target": node.target.display().to_string(),
                }),
            )?;
            return match self.platform.rename_path(&node.source, &node.target) {
                Ok(()) => {
                    self.checkpoint(
                        "target_committed",
                        serde_json::json!({
                            "source": node.source.display().to_string(),
                            "target": node.target.display().to_string(),
                        }),
                    )?;
                    Ok(FastMove::Moved)
                }
                Err(PlatformError::CrossDevice { .. }) => Ok(FastMove::CrossDevice),
                Err(error) => Err(error.into()),
            };
        }

        let staging = self.unique_staging_path(&node.target)?;
        self.checkpoint(
            "move_source_staging",
            serde_json::json!({
                "source": node.source.display().to_string(),
                "target": node.target.display().to_string(),
                "staging": staging.display().to_string(),
            }),
        )?;
        match self.platform.rename_path(&node.source, &staging) {
            Ok(()) => {}
            Err(PlatformError::CrossDevice { .. }) => return Ok(FastMove::CrossDevice),
            Err(error) => return Err(error.into()),
        }
        self.checkpoint(
            "move_source_staged",
            serde_json::json!({
                "source": node.source.display().to_string(),
                "target": node.target.display().to_string(),
                "staging": staging.display().to_string(),
            }),
        )?;
        let trashed = match self.trash.move_to_trash(self.platform, &node.target) {
            Ok(path) => path,
            Err(error) => {
                let _ = self.platform.rename_path(&staging, &node.source);
                return Err(error);
            }
        };
        self.checkpoint(
            "destination_trashed",
            serde_json::json!({
                "source": node.source.display().to_string(),
                "target": node.target.display().to_string(),
                "staging": staging.display().to_string(),
            }),
        )?;
        if let Err(error) = self.platform.rename_path(&staging, &node.target) {
            let _ = self.platform.rename_path(&staging, &node.source);
            if self.trash.has_rollback_path() {
                let _ = self.platform.rename_path(&trashed, &node.target);
            }
            return Err(error.into());
        }
        self.checkpoint(
            "target_committed",
            serde_json::json!({
                "source": node.source.display().to_string(),
                "target": node.target.display().to_string(),
                "staging": staging.display().to_string(),
            }),
        )?;
        Ok(FastMove::Moved)
    }

    fn copy_file_staged(
        &mut self,
        source: &Path,
        target: &Path,
        replace: bool,
    ) -> Result<(), ExplorerTaskError> {
        let staging = self.unique_staging_path(target)?;
        self.checkpoint(
            "copy_staging_writing",
            serde_json::json!({
                "source": source.display().to_string(),
                "target": target.display().to_string(),
                "staging": staging.display().to_string(),
                "replace": replace,
            }),
        )?;
        let result = self.write_staging_file(source, &staging);
        if let Err(error) = result {
            self.cleanup_staging(&staging);
            return Err(error);
        }
        self.checkpoint(
            "copy_staging_synced",
            serde_json::json!({
                "source": source.display().to_string(),
                "target": target.display().to_string(),
                "staging": staging.display().to_string(),
                "replace": replace,
            }),
        )?;
        if self.check_cancel().is_err() {
            self.cleanup_staging(&staging);
            return Err(ExplorerTaskError::Cancelled);
        }

        let trashed = if replace {
            self.checkpoint(
                "destination_trash_pending",
                serde_json::json!({
                    "source": source.display().to_string(),
                    "target": target.display().to_string(),
                    "staging": staging.display().to_string(),
                    "replace": true,
                }),
            )?;
            match self.trash.move_to_trash(self.platform, target) {
                Ok(path) => Some(path),
                Err(error) => {
                    self.cleanup_staging(&staging);
                    return Err(error);
                }
            }
        } else {
            if path_exists_no_follow(target) {
                self.cleanup_staging(&staging);
                return Err(ExplorerTaskError::DestinationChanged {
                    path: target.to_path_buf(),
                });
            }
            None
        };
        if replace {
            self.checkpoint(
                "destination_trashed",
                serde_json::json!({
                    "source": source.display().to_string(),
                    "target": target.display().to_string(),
                    "staging": staging.display().to_string(),
                    "replace": true,
                }),
            )?;
        }
        if let Err(error) = self.platform.rename_path(&staging, target) {
            self.cleanup_staging(&staging);
            if self.trash.has_rollback_path() {
                if let Some(trashed) = trashed {
                    let _ = self.platform.rename_path(&trashed, target);
                }
            }
            return Err(error.into());
        }
        self.checkpoint(
            "target_committed",
            serde_json::json!({
                "source": source.display().to_string(),
                "target": target.display().to_string(),
                "staging": staging.display().to_string(),
            }),
        )?;
        Ok(())
    }

    fn write_staging_file(
        &mut self,
        source: &Path,
        staging: &Path,
    ) -> Result<(), ExplorerTaskError> {
        let mut input =
            File::open(source).map_err(|error| io_error("open copy source", source, error))?;
        let mut output = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(staging)
            .map_err(|error| io_error("create staged copy", staging, error))?;
        let mut buffer = vec![0_u8; self.chunk_size];
        loop {
            self.check_cancel()?;
            let read = input
                .read(&mut buffer)
                .map_err(|error| io_error("read copy source", source, error))?;
            if read == 0 {
                break;
            }
            output
                .write_all(&buffer[..read])
                .map_err(|error| io_error("write staged copy", staging, error))?;
            self.progress.processed_bytes =
                self.progress.processed_bytes.saturating_add(read as u64);
            self.progress.current_path = Some(source.to_path_buf());
            self.emit_progress();
        }
        output
            .flush()
            .map_err(|error| io_error("flush staged copy", staging, error))?;
        output
            .sync_all()
            .map_err(|error| io_error("sync staged copy", staging, error))?;
        if let Ok(metadata) = fs::metadata(source) {
            fs::set_permissions(staging, metadata.permissions())
                .map_err(|error| io_error("copy file permissions", staging, error))?;
        }
        Ok(())
    }

    #[allow(clippy::permissions_set_readonly_false)] // Correct for Windows FILE_ATTRIBUTE_READONLY.
    fn cleanup_staging(&mut self, staging: &Path) {
        let old_phase = self.progress.phase;
        self.progress.phase = ExplorerTaskPhase::CleaningUp;
        self.progress.current_path = Some(staging.to_path_buf());
        self.emit_progress();
        #[cfg(windows)]
        if let Ok(metadata) = fs::metadata(staging) {
            let mut permissions = metadata.permissions();
            if permissions.readonly() {
                permissions.set_readonly(false);
                let _ = fs::set_permissions(staging, permissions);
            }
        }
        let _ = if staging.is_dir() {
            fs::remove_dir_all(staging)
        } else {
            fs::remove_file(staging)
        };
        self.progress.phase = old_phase;
    }

    fn unique_staging_path(&mut self, target: &Path) -> Result<PathBuf, ExplorerTaskError> {
        let parent = target
            .parent()
            .ok_or_else(|| ExplorerTaskError::InvalidPlan {
                message: format!("{} has no destination parent", target.display()),
            })?;
        let name = target
            .file_name()
            .map(|name| name.to_string_lossy())
            .unwrap_or_default();
        loop {
            let sequence = self.staging_sequence;
            self.staging_sequence = self.staging_sequence.saturating_add(1);
            let path = parent.join(format!(".tundra-stage-{}-{sequence}-{name}", self.id.0));
            if !path_exists_no_follow(&path) {
                return Ok(path);
            }
        }
    }

    fn record_renamed_subtree(&mut self, node: &PreparedNode) {
        match &node.disposition {
            PreparedDisposition::Directory { children, .. } => {
                for child in children {
                    self.record_renamed_subtree(child);
                }
            }
            PreparedDisposition::File { .. } => {
                self.progress.processed_bytes = self
                    .progress
                    .processed_bytes
                    .saturating_add(node.attributes.len);
            }
            PreparedDisposition::Skip | PreparedDisposition::Link { .. } => {}
        }
        self.record_success(&node.source, Some(&node.target));
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FastMove {
    Moved,
    CrossDevice,
}

fn optional_attributes(
    platform: &dyn Platform,
    path: &Path,
) -> Result<Option<FileAttributes>, ExplorerTaskError> {
    match fs::symlink_metadata(path) {
        Ok(_) => platform.file_attributes(path).map(Some).map_err(Into::into),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(io_error("inspect destination", path, error)),
    }
}

fn canonical_existing_no_follow(
    path: &Path,
    operation: &'static str,
) -> Result<PathBuf, ExplorerTaskError> {
    fs::symlink_metadata(path).map_err(|error| io_error(operation, path, error))?;
    match (path.parent(), path.file_name()) {
        (Some(parent), Some(name)) => fs::canonicalize(parent)
            .map(|parent| parent.join(name))
            .map_err(|error| io_error(operation, path, error)),
        _ => fs::canonicalize(path).map_err(|error| io_error(operation, path, error)),
    }
}

fn path_exists_no_follow(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok()
}

fn is_unsafe_link(attributes: &FileAttributes) -> bool {
    attributes.symlink || attributes.junction || attributes.reparse_point
}

fn first_unsafe_path(node: &PreparedNode) -> Option<PathBuf> {
    match &node.disposition {
        PreparedDisposition::Link { .. } => Some(node.source.clone()),
        PreparedDisposition::Directory { children, .. } => {
            children.iter().find_map(first_unsafe_path)
        }
        _ => None,
    }
}

fn unique_keep_both_path(
    requested: &Path,
    reserved: &BTreeSet<PathBuf>,
    windows_paths: bool,
) -> PathBuf {
    let parent = requested.parent().unwrap_or_else(|| Path::new(""));
    let stem = requested
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_else(|| "item".to_string());
    let extension = requested
        .extension()
        .map(|extension| extension.to_string_lossy().into_owned());
    for suffix in 2_u64.. {
        let name = match extension.as_deref() {
            Some(extension) => format!("{stem} ({suffix}).{extension}"),
            None => format!("{stem} ({suffix})"),
        };
        let candidate = parent.join(name);
        if !path_exists_no_follow(&candidate)
            && !reserved
                .iter()
                .any(|path| paths_equal(path, &candidate, windows_paths))
        {
            return candidate;
        }
    }
    unreachable!("the keep-both suffix space is exhausted")
}

fn paths_equal(left: &Path, right: &Path, windows_paths: bool) -> bool {
    if windows_paths {
        left.to_string_lossy()
            .eq_ignore_ascii_case(&right.to_string_lossy())
    } else {
        left == right
    }
}

fn path_is_within(child: &Path, parent: &Path, windows_paths: bool) -> bool {
    if windows_paths {
        let child = child
            .to_string_lossy()
            .replace('/', "\\")
            .to_ascii_lowercase();
        let mut parent = parent
            .to_string_lossy()
            .replace('/', "\\")
            .trim_end_matches('\\')
            .to_ascii_lowercase();
        parent.push('\\');
        child == parent.trim_end_matches('\\') || child.starts_with(&parent)
    } else {
        child == parent || child.starts_with(parent)
    }
}

fn remove_path_no_follow(platform: &dyn Platform, path: &Path) -> Result<(), ExplorerTaskError> {
    let attributes = platform.file_attributes(path)?;
    if is_unsafe_link(&attributes) {
        return Err(ExplorerTaskError::UnsafeLink {
            path: path.to_path_buf(),
        });
    }
    if attributes.is_dir {
        let directory = fs::read_dir(path)
            .map_err(|error| io_error("read moved source directory", path, error))?;
        for entry in directory {
            let entry = entry.map_err(|error| io_error("read moved source entry", path, error))?;
            remove_path_no_follow(platform, &entry.path())?;
        }
        fs::remove_dir(path).map_err(|error| io_error("remove moved source directory", path, error))
    } else {
        fs::remove_file(path).map_err(|error| io_error("remove moved source file", path, error))
    }
}

fn io_error(operation: &'static str, path: &Path, error: std::io::Error) -> ExplorerTaskError {
    ExplorerTaskError::Io {
        operation,
        path: path.to_path_buf(),
        message: error.to_string(),
    }
}

fn send_event(sender: &mpsc::Sender<ExplorerTaskEvent>, event: ExplorerTaskEvent) {
    let _ = sender.send(event);
}

#[cfg(test)]
mod recovery_tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static SEQUENCE: AtomicU64 = AtomicU64::new(1);

    fn fixture(label: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "tundra-explorer-recovery-{label}-{}-{}",
            std::process::id(),
            SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn record(phase: &str, payload: serde_json::Value) -> OperationRecord {
        serde_json::from_value(serde_json::json!({
            "schema_version": 1,
            "run_id": "test-run",
            "app_id": "explorer",
            "component": "explorer/filesystem",
            "operation_id": "test-operation",
            "kind": "explorer.filesystem.v1",
            "replay_safety": {
                "kind": "checkpointed",
                "operation": "explorer.filesystem.v1"
            },
            "recovery_handler_version": "1",
            "summary": "test",
            "checkpoint_sequence": 2,
            "checkpoint": {
                "phase": phase,
                "payload": payload
            },
            "status": "active",
            "started_at": "2026-07-12T00:00:00Z",
            "updated_at": "2026-07-12T00:00:01Z"
        }))
        .unwrap()
    }

    #[test]
    fn recovery_commits_a_synced_stage_after_destination_was_trashed() {
        let root = fixture("forward-commit");
        let target = root.join("target.txt");
        let staging = root.join(".tundra-stage-test-target.txt");
        fs::write(&staging, b"new").unwrap();
        let outcome = ExplorerRecoveryHandler.recover(&record(
            "destination_trashed",
            serde_json::json!({
                "target": target.display().to_string(),
                "staging": staging.display().to_string()
            }),
        ));

        assert!(outcome.is_recovered());
        assert_eq!(fs::read(&target).unwrap(), b"new");
        assert!(!staging.exists());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn recovery_preserves_both_cross_volume_copies_for_manual_review() {
        let root = fixture("cross-volume");
        let source = root.join("source.txt");
        let target = root.join("target.txt");
        fs::write(&source, b"data").unwrap();
        fs::write(&target, b"data").unwrap();
        let outcome = ExplorerRecoveryHandler.recover(&record(
            "cross_volume_target_committed",
            serde_json::json!({
                "source": source.display().to_string(),
                "target": target.display().to_string()
            }),
        ));

        assert!(matches!(outcome, RecoveryOutcome::ManualActionRequired(_)));
        assert!(source.exists());
        assert!(target.exists());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn recovery_removes_only_an_exact_uncommitted_stage_path() {
        let root = fixture("stage-cleanup");
        let staging = root.join(".tundra-stage-test-target.txt");
        fs::write(&staging, b"partial").unwrap();
        let outcome = ExplorerRecoveryHandler.recover(&record(
            "copy_staging_writing",
            serde_json::json!({ "staging": staging.display().to_string() }),
        ));
        assert!(outcome.is_recovered());
        assert!(!staging.exists());

        let ordinary = root.join("ordinary.txt");
        fs::write(&ordinary, b"keep").unwrap();
        let outcome = ExplorerRecoveryHandler.recover(&record(
            "copy_staging_writing",
            serde_json::json!({ "staging": ordinary.display().to_string() }),
        ));
        assert!(matches!(outcome, RecoveryOutcome::ManualActionRequired(_)));
        assert!(ordinary.exists());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn recovery_preserves_a_partially_created_directory_for_manual_review() {
        let root = fixture("partial-directory");
        let target = root.join("target");
        fs::create_dir(&target).unwrap();
        fs::write(target.join("completed-child.txt"), b"keep").unwrap();

        let outcome = ExplorerRecoveryHandler.recover(&record(
            "directory_target_created",
            serde_json::json!({
                "target": target.display().to_string(),
                "replaced": false
            }),
        ));

        assert!(matches!(outcome, RecoveryOutcome::ManualActionRequired(_)));
        assert_eq!(
            fs::read(target.join("completed-child.txt")).unwrap(),
            b"keep"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn recovery_forward_commits_when_trash_move_finished_before_its_checkpoint() {
        let root = fixture("trash-checkpoint-gap");
        let target = root.join("target.txt");
        let staging = root.join(".tundra-stage-gap-target.txt");
        fs::write(&staging, b"new").unwrap();

        let outcome = ExplorerRecoveryHandler.recover(&record(
            "destination_trash_pending",
            serde_json::json!({
                "target": target.display().to_string(),
                "staging": staging.display().to_string(),
                "replace": true
            }),
        ));

        assert!(outcome.is_recovered());
        assert_eq!(fs::read(&target).unwrap(), b"new");
        assert!(!staging.exists());
        let _ = fs::remove_dir_all(root);
    }
}
