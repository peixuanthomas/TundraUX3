use crate::config::WatchdogConfig;
use crate::journal::{self, OperationGuard};
use crate::report::{
    ErrorDetails, ExecutionContext, IncidentRecord, PanicDetails, REPORT_SCHEMA_VERSION,
};
use crate::task::{GroupState, ManagedTaskGroup};
use crate::writer::{self, WriterCommand};
use crate::{
    AppDescriptor, AppId, BoundaryKind, BoundarySpec, Breadcrumb, ComponentId, ErrorContext,
    IncidentKind, IncidentReceipt, IncidentSeverity, IncidentTicket, OperationDescriptor,
    OperationKind, RecoveryHandler, RecoveryOutcome, RuntimeSnapshot, TaskId, TaskKind,
    WatchdogError,
};
use crate::{durable, report_catalog, sanitize};
use chrono::Utc;
use serde::Deserialize;
use std::backtrace::Backtrace;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet, VecDeque};
use std::error::Error;
use std::fs;
#[cfg(feature = "tokio")]
use std::future::Future;
use std::panic::{self, AssertUnwindSafe, PanicHookInfo, UnwindSafe};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock, mpsc};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// An infallible, non-blocking cleanup invoked from the process panic hook.
///
/// Implementations must not panic. Rust aborts a process when panic-hook code
/// panics while another panic is already being processed.
pub type EmergencyCleanup = Arc<dyn Fn() + Send + Sync + 'static>;

static GLOBAL_WATCHDOG: OnceLock<ProcessWatchdog> = OnceLock::new();
static RUNTIME_STARTED: AtomicBool = AtomicBool::new(false);

thread_local! {
    #[allow(clippy::missing_const_for_thread_local)]
    static THREAD_CONTEXT: RefCell<Vec<ExecutionContext>> = const { RefCell::new(Vec::new()) };
}

#[cfg(feature = "tokio")]
tokio::task_local! {
    static ASYNC_CONTEXT: ExecutionContext;
}

pub(crate) struct Shared {
    pub(crate) config: WatchdogConfig,
    pub(crate) run_id: String,
    command_tx: mpsc::Sender<WriterCommand>,
    incident_rx: Mutex<mpsc::Receiver<IncidentReceipt>>,
    apps: Mutex<HashMap<AppId, AppDescriptor>>,
    cleanups: Mutex<Vec<EmergencyCleanup>>,
    breadcrumbs: Mutex<VecDeque<Breadcrumb>>,
    runtime_snapshot: Mutex<RuntimeSnapshot>,
    last_heartbeat_sent: Mutex<Instant>,
    next_incident: AtomicU64,
    next_operation: AtomicU64,
    pub(crate) recovery_handlers: Mutex<HashMap<(AppId, OperationKind), Arc<dyn RecoveryHandler>>>,
    pub(crate) groups: Mutex<HashMap<String, Arc<GroupState>>>,
    pub(crate) active_operations: Mutex<HashSet<(AppId, String)>>,
    pub(crate) operation_contexts: Mutex<HashMap<String, Vec<OperationContext>>>,
    pub(crate) recovery_gate: Mutex<()>,
    pub(crate) confirmed_stale_runs: Mutex<HashSet<String>>,
    pub(crate) recovery_status: Mutex<HashMap<(AppId, OperationKind), RecoveryOutcome>>,
    pub(crate) running: AtomicBool,
}

#[derive(Debug, Clone)]
pub(crate) struct OperationContext {
    pub(crate) operation_id: String,
    pub(crate) recovery_handler_version: Option<String>,
}

pub struct WatchdogRuntime {
    process: ProcessWatchdog,
    writer: Option<JoinHandle<()>>,
    stopped: bool,
}

#[derive(Clone)]
pub struct ProcessWatchdog {
    pub(crate) shared: Arc<Shared>,
}

#[derive(Clone)]
pub struct AppWatchdog {
    pub(crate) process: ProcessWatchdog,
    pub(crate) descriptor: AppDescriptor,
    pub(crate) component: String,
}

pub struct CaughtPanic {
    process: ProcessWatchdog,
    context: Box<ExecutionContext>,
    fallback: Box<IncidentRecord>,
    finalized: bool,
}

#[derive(Debug, Deserialize)]
struct StaleRunMarker {
    schema_version: u32,
    run_id: String,
    process_id: u32,
    started_at_utc: String,
    last_heartbeat_utc: String,
    snapshot: RuntimeSnapshot,
}

pub(crate) struct ThreadContextGuard;

impl Drop for ThreadContextGuard {
    fn drop(&mut self) {
        THREAD_CONTEXT.with(|contexts| {
            contexts.borrow_mut().pop();
        });
    }
}

impl WatchdogRuntime {
    pub fn start(
        config: WatchdogConfig,
    ) -> Result<(WatchdogRuntime, ProcessWatchdog), WatchdogError> {
        if RUNTIME_STARTED
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return Err(WatchdogError::RuntimeAlreadyStarted);
        }
        match Self::start_inner(config) {
            Ok(runtime) => Ok(runtime),
            Err(error) => {
                RUNTIME_STARTED.store(false, Ordering::Release);
                Err(error)
            }
        }
    }

    #[cfg(any(test, feature = "test-support"))]
    #[doc(hidden)]
    pub fn start_isolated(
        config: WatchdogConfig,
    ) -> Result<(WatchdogRuntime, ProcessWatchdog), WatchdogError> {
        Self::start_inner(config)
    }

    fn start_inner(
        config: WatchdogConfig,
    ) -> Result<(WatchdogRuntime, ProcessWatchdog), WatchdogError> {
        let (command_tx, command_rx) = mpsc::channel();
        let (incident_tx, incident_rx) = mpsc::channel();
        let (ready_tx, ready_rx) = mpsc::channel();
        let run_id = format!(
            "run-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|duration| duration.as_nanos())
                .unwrap_or(0)
        );
        let shared = Arc::new(Shared {
            config: config.clone(),
            run_id: run_id.clone(),
            command_tx,
            incident_rx: Mutex::new(incident_rx),
            apps: Mutex::new(HashMap::new()),
            cleanups: Mutex::new(Vec::new()),
            breadcrumbs: Mutex::new(VecDeque::with_capacity(config.breadcrumb_capacity)),
            runtime_snapshot: Mutex::new(RuntimeSnapshot::default()),
            last_heartbeat_sent: Mutex::new(Instant::now()),
            next_incident: AtomicU64::new(1),
            next_operation: AtomicU64::new(1),
            recovery_handlers: Mutex::new(HashMap::new()),
            groups: Mutex::new(HashMap::new()),
            active_operations: Mutex::new(HashSet::new()),
            operation_contexts: Mutex::new(HashMap::new()),
            recovery_gate: Mutex::new(()),
            confirmed_stale_runs: Mutex::new(HashSet::new()),
            recovery_status: Mutex::new(HashMap::new()),
            running: AtomicBool::new(true),
        });
        let writer = thread::Builder::new()
            .name("tundra-watchdog-writer".to_string())
            .spawn(move || writer::writer_loop(config, run_id, command_rx, incident_tx, ready_tx))
            .map_err(WatchdogError::ThreadSpawn)?;
        match ready_rx.recv_timeout(Duration::from_secs(2)) {
            Ok(Ok(())) => {}
            Ok(Err(error)) => {
                let _ = writer.join();
                return Err(WatchdogError::Writer(error));
            }
            Err(_) => {
                let _ = writer.join();
                return Err(WatchdogError::IncidentTimeout);
            }
        }
        let process = ProcessWatchdog { shared };
        Ok((
            Self {
                process: process.clone(),
                writer: Some(writer),
                stopped: false,
            },
            process,
        ))
    }

    pub fn try_recv_incident(&self) -> Option<IncidentReceipt> {
        self.process.try_recv_incident()
    }

    pub fn shutdown(mut self) -> Result<(), WatchdogError> {
        self.stop()
    }

    fn stop(&mut self) -> Result<(), WatchdogError> {
        if self.stopped {
            return Ok(());
        }
        self.stopped = true;
        self.process.shared.running.store(false, Ordering::Release);
        let groups = self
            .process
            .shared
            .groups
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .iter()
            .map(|(name, state)| (name.clone(), state.clone()))
            .collect::<Vec<_>>();
        for (_, group) in &groups {
            group.close_and_cancel();
        }
        let can_block = {
            #[cfg(feature = "tokio")]
            {
                tokio::runtime::Handle::try_current().is_err()
            }
            #[cfg(not(feature = "tokio"))]
            {
                true
            }
        };
        for (name, group) in groups {
            let counts = if can_block {
                group.wait(self.process.shared.config.task_shutdown_timeout)
            } else {
                group.counts()
            };
            if counts.still_running > 0 {
                self.process
                    .report_shutdown_timeout(&name, &group.running_task_names());
            }
        }
        let (tx, rx) = mpsc::channel();
        let shutdown_result = self
            .process
            .shared
            .command_tx
            .send(WriterCommand::Shutdown(tx))
            .map_err(|_| WatchdogError::ChannelClosed);
        if shutdown_result.is_ok() {
            match rx.recv_timeout(Duration::from_secs(2)) {
                Ok(Ok(())) => {}
                Ok(Err(error)) => return Err(WatchdogError::Writer(error)),
                Err(_) => return Err(WatchdogError::IncidentTimeout),
            }
        }
        if let Some(writer) = self.writer.take() {
            let _ = writer.join();
        }
        shutdown_result
    }
}

impl Drop for WatchdogRuntime {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

impl ProcessWatchdog {
    pub fn install_global(self) -> Result<Self, WatchdogError> {
        GLOBAL_WATCHDOG
            .set(self.clone())
            .map_err(|_| WatchdogError::AlreadyInstalled)?;
        let previous = panic::take_hook();
        let process = self.clone();
        panic::set_hook(Box::new(move |panic_info| {
            let context = current_execution_context().unwrap_or_else(|| {
                ExecutionContext::process(process.next_incident_id(), process.clone())
            });
            let incident = process.panic_incident(&context, panic_info);
            writer::append_emergency(
                &process.shared.config,
                &format!(
                    "incident={} boundary={} panic={}",
                    incident.incident_id,
                    incident.boundary,
                    incident.summary()
                ),
            );
            if (context.owns_terminal || context.app.is_none())
                && let Ok(cleanups) = process.shared.cleanups.try_lock()
            {
                let callbacks = cleanups.clone();
                drop(cleanups);
                for cleanup in callbacks {
                    let _ = panic::catch_unwind(AssertUnwindSafe(|| cleanup()));
                }
            }
            let emit = !context.expects_finalize;
            let _ = process
                .shared
                .command_tx
                .send(WriterCommand::Record { incident, emit });
            if !context.expects_finalize {
                previous(panic_info);
            }
        }));
        Ok(self)
    }

    pub fn global() -> Option<Self> {
        GLOBAL_WATCHDOG
            .get()
            .filter(|process| process.shared.running.load(Ordering::Acquire))
            .cloned()
    }

    pub fn register_app(&self, descriptor: AppDescriptor) -> Result<AppWatchdog, WatchdogError> {
        if !self.shared.running.load(Ordering::Acquire) {
            return Err(WatchdogError::RuntimeStopped);
        }
        let mut apps = self
            .shared
            .apps
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(existing) = apps.get(&descriptor.id) {
            if existing != &descriptor {
                return Err(WatchdogError::ConflictingAppRegistration(
                    descriptor.id.to_string(),
                ));
            }
        } else {
            apps.insert(descriptor.id.clone(), descriptor.clone());
        }
        Ok(AppWatchdog {
            process: self.clone(),
            component: descriptor.id.to_string(),
            descriptor,
        })
    }

    pub fn register_emergency_cleanup(
        &self,
        cleanup: EmergencyCleanup,
    ) -> Result<(), WatchdogError> {
        self.shared
            .cleanups
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push(cleanup);
        Ok(())
    }

    pub fn try_recv_incident(&self) -> Option<IncidentReceipt> {
        self.shared
            .incident_rx
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .try_recv()
            .ok()
    }

    pub fn drain_incidents(&self) -> Vec<IncidentReceipt> {
        std::iter::from_fn(|| self.try_recv_incident()).collect()
    }

    pub fn list_incident_reports(&self) -> report_catalog::IncidentReportCatalog {
        report_catalog::list_incident_reports(&self.shared.config)
    }

    pub fn report_stale_runs(
        &self,
        is_process_alive: impl Fn(u32) -> bool,
    ) -> Result<usize, WatchdogError> {
        let directory = self.shared.config.data_dir.join("watchdog").join("runs");
        let entries = match fs::read_dir(&directory) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(0),
            Err(source) => {
                return Err(WatchdogError::Io {
                    operation: "read watchdog run markers",
                    path: directory,
                    source,
                });
            }
        };
        let mut reported = 0_usize;
        for entry in entries {
            let Ok(entry) = entry else {
                continue;
            };
            let path = entry.path();
            if path.extension().and_then(|value| value.to_str()) != Some("json") {
                continue;
            }
            let bytes = match fs::read(&path) {
                Ok(bytes) => bytes,
                Err(error) => {
                    self.report_corrupt_marker(&path, &error.to_string())?;
                    reported = reported.saturating_add(1);
                    continue;
                }
            };
            let marker = match serde_json::from_slice::<StaleRunMarker>(&bytes) {
                Ok(marker) if marker.schema_version == 1 && is_safe_run_id(&marker.run_id) => {
                    marker
                }
                Ok(marker) => {
                    self.report_corrupt_marker(
                        &path,
                        &format!(
                            "unsupported schema {} or invalid run identifier",
                            marker.schema_version
                        ),
                    )?;
                    reported = reported.saturating_add(1);
                    continue;
                }
                Err(error) => {
                    self.report_corrupt_marker(&path, &error.to_string())?;
                    reported = reported.saturating_add(1);
                    continue;
                }
            };
            if marker.run_id == self.shared.run_id {
                continue;
            }
            if marker.process_id != std::process::id() && is_process_alive(marker.process_id) {
                continue;
            }

            let context =
                ExecutionContext::process(format!("unclean-{}", marker.run_id), self.clone());
            let mut incident = self.incident_record(
                &context,
                IncidentKind::UncleanExit,
                IncidentSeverity::Critical,
                None,
                Some(ErrorDetails {
                    message: format!(
                        "previous run {} ended without a clean watchdog shutdown",
                        marker.run_id
                    ),
                    source_chain: vec![
                        format!("started at {}", marker.started_at_utc),
                        format!("last heartbeat at {}", marker.last_heartbeat_utc),
                        "the in-process watchdog cannot determine whether the cause was abort, forced termination, power loss, or a fatal native fault"
                            .to_string(),
                    ],
                    backtrace: "unavailable for an unclean process exit".to_string(),
                }),
            );
            incident.runtime = marker.snapshot;
            incident.recovery = RecoveryOutcome::Unrecoverable(
                "the previous process had already terminated; only its last heartbeat could be recovered"
                    .to_string(),
            );
            self.persist_incident_and_wait(incident)?;
            self.shared
                .confirmed_stale_runs
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .insert(marker.run_id);
            durable::remove_file(&path).map_err(|source| WatchdogError::Io {
                operation: "remove reported stale run marker",
                path,
                source,
            })?;
            reported = reported.saturating_add(1);
        }
        Ok(reported)
    }

    fn persist_incident_and_wait(
        &self,
        incident: IncidentRecord,
    ) -> Result<IncidentReceipt, WatchdogError> {
        let (tx, rx) = mpsc::channel();
        self.shared
            .command_tx
            .send(WriterCommand::RecordAndWait {
                incident,
                emit: true,
                response: tx,
            })
            .map_err(|_| WatchdogError::ChannelClosed)?;
        rx.recv_timeout(Duration::from_secs(2))
            .map_err(|_| WatchdogError::IncidentTimeout)?
            .map_err(WatchdogError::Writer)
    }

    fn report_corrupt_marker(
        &self,
        path: &std::path::Path,
        detail: &str,
    ) -> Result<(), WatchdogError> {
        let file_name = path
            .file_name()
            .and_then(|name| name.to_str())
            .map(sanitize::text)
            .unwrap_or_else(|| "unknown-marker".to_string());
        let context = ExecutionContext::process(
            format!("corrupt-marker-{}", self.next_incident_id()),
            self.clone(),
        );
        let mut incident = self.incident_record(
            &context,
            IncidentKind::UncleanExit,
            IncidentSeverity::Critical,
            None,
            Some(ErrorDetails {
                message: format!("watchdog run marker {file_name} is unreadable or corrupt"),
                source_chain: vec![sanitize::text(detail)],
                backtrace: "unavailable for a corrupt run marker".to_string(),
            }),
        );
        incident.recovery = RecoveryOutcome::ManualActionRequired(
            "the corrupt marker was quarantined; the previous exit reason is unknown".to_string(),
        );
        self.persist_incident_and_wait(incident)?;
        let quarantine = path.with_extension(format!(
            "corrupt-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|duration| duration.as_nanos())
                .unwrap_or(0)
        ));
        fs::rename(path, &quarantine).map_err(|source| WatchdogError::Io {
            operation: "quarantine corrupt run marker",
            path: path.to_path_buf(),
            source,
        })
    }

    pub(crate) fn next_incident_id(&self) -> String {
        let sequence = self.shared.next_incident.fetch_add(1, Ordering::Relaxed);
        format!("{}-{sequence}", self.shared.run_id)
    }

    pub(crate) fn next_operation_id(&self) -> String {
        let sequence = self.shared.next_operation.fetch_add(1, Ordering::Relaxed);
        format!("{}-op-{sequence}", self.shared.run_id)
    }

    fn clear_operation_context(&self, incident_id: &str) {
        self.shared
            .operation_contexts
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(incident_id);
    }

    fn report_shutdown_timeout(&self, group: &str, tasks: &[String]) {
        let context = ExecutionContext::process(self.next_incident_id(), self.clone());
        let mut incident = self.incident_record(
            &context,
            IncidentKind::Error,
            IncidentSeverity::Critical,
            None,
            Some(ErrorDetails {
                message: sanitize::text(format!(
                    "managed task group {group} did not stop before watchdog shutdown"
                )),
                source_chain: tasks.iter().map(sanitize::text).collect(),
                backtrace: Backtrace::force_capture().to_string(),
            }),
        );
        incident.recovery = RecoveryOutcome::Unrecoverable(
            "the process is shutting down with managed tasks still running".to_string(),
        );
        let _ = self.shared.command_tx.send(WriterCommand::Record {
            incident,
            emit: true,
        });
    }

    fn panic_incident(
        &self,
        context: &ExecutionContext,
        panic_info: &PanicHookInfo<'_>,
    ) -> IncidentRecord {
        let payload = panic_payload(panic_info.payload());
        let location = panic_info.location();
        self.incident_record(
            context,
            IncidentKind::Panic,
            IncidentSeverity::Critical,
            Some(PanicDetails {
                payload,
                source_file: location.map(|location| location.file().to_string()),
                source_line: location.map(|location| location.line()),
                source_column: location.map(|location| location.column()),
                backtrace: Backtrace::force_capture().to_string(),
            }),
            None,
        )
    }

    pub(crate) fn fallback_panic_incident(
        &self,
        context: &ExecutionContext,
        payload: String,
    ) -> IncidentRecord {
        self.incident_record(
            context,
            IncidentKind::Panic,
            IncidentSeverity::Critical,
            Some(PanicDetails {
                payload,
                source_file: None,
                source_line: None,
                source_column: None,
                backtrace: Backtrace::force_capture().to_string(),
            }),
            None,
        )
    }

    fn incident_record(
        &self,
        context: &ExecutionContext,
        kind: IncidentKind,
        severity: IncidentSeverity,
        panic: Option<PanicDetails>,
        error: Option<ErrorDetails>,
    ) -> IncidentRecord {
        let occurred_at = Utc::now();
        let report_stem = format!(
            "crash-{}-{}",
            occurred_at.format("%Y%m%dT%H%M%S%.3fZ"),
            context.incident_id
        );
        let operation = self
            .shared
            .operation_contexts
            .try_lock()
            .ok()
            .and_then(|operations| {
                operations
                    .get(&context.incident_id)
                    .and_then(|items| items.last())
                    .cloned()
            });
        IncidentRecord {
            schema_version: REPORT_SCHEMA_VERSION,
            incident_id: context.incident_id.clone(),
            report_stem,
            kind,
            severity,
            occurred_at,
            process_name: self.shared.config.process_name.clone(),
            process_version: self.shared.config.process_version.clone(),
            process_id: std::process::id(),
            run_id: self.shared.run_id.clone(),
            app: context.app.clone(),
            component: context.component.clone(),
            task_id: context.task_id.clone(),
            task_group: context.task_group.clone(),
            boundary: context.boundary.clone(),
            boundary_kind: context.boundary_kind,
            task_kind: context.task_kind,
            replay_safety: context.replay_safety.clone(),
            operation_kind: context.operation_kind.clone(),
            operation_id: context.operation_id.clone().or_else(|| {
                operation
                    .as_ref()
                    .map(|operation| operation.operation_id.clone())
            }),
            recovery_handler_version: context.recovery_handler_version.clone().or_else(|| {
                operation
                    .as_ref()
                    .and_then(|operation| operation.recovery_handler_version.clone())
            }),
            panic_action: context.panic_action,
            restart_policy: context.restart_policy.clone(),
            restart_attempt: context.restart_attempt,
            thread_name: thread::current().name().map(str::to_string),
            thread_id: format!("{:?}", thread::current().id()),
            panic,
            error,
            runtime: self
                .shared
                .runtime_snapshot
                .try_lock()
                .map(|snapshot| snapshot.clone())
                .unwrap_or_default(),
            breadcrumbs: self
                .shared
                .breadcrumbs
                .try_lock()
                .map(|breadcrumbs| breadcrumbs.iter().cloned().collect())
                .unwrap_or_default(),
            recovery: RecoveryOutcome::Pending,
            secondary_errors: Vec::new(),
        }
    }
}

impl AppWatchdog {
    pub fn child_component(&self, id: ComponentId) -> AppWatchdog {
        Self {
            process: self.process.clone(),
            descriptor: self.descriptor.clone(),
            component: format!("{}/{}", self.component, id),
        }
    }

    pub fn task_group(&self, name: &str) -> ManagedTaskGroup {
        ManagedTaskGroup::new(self.clone(), name)
    }

    pub fn run_boundary<T, F>(&self, spec: BoundarySpec, operation: F) -> Result<T, CaughtPanic>
    where
        F: FnOnce() -> T + UnwindSafe,
    {
        let mut context =
            self.execution_context(spec.id, spec.kind, spec.owns_terminal, None, None, None, 0);
        if let Some(parent) = current_execution_context() {
            context.task_id = parent.task_id;
            context.task_group = parent.task_group;
            context.task_kind = parent.task_kind;
            context.replay_safety = parent.replay_safety;
            context.operation_kind = parent.operation_kind;
            context.operation_id = parent.operation_id;
            context.recovery_handler_version = parent.recovery_handler_version;
            context.panic_action = parent.panic_action;
            context.restart_policy = parent.restart_policy;
            context.restart_attempt = parent.restart_attempt;
        }
        context.expects_finalize = true;
        let guard = push_thread_context(context.clone());
        let result = panic::catch_unwind(operation);
        drop(guard);
        match result {
            Ok(value) => Ok(value),
            Err(payload) => {
                let payload = panic_payload(payload.as_ref());
                let fallback = self
                    .process
                    .fallback_panic_incident(&context, payload.clone());
                Err(CaughtPanic {
                    process: self.process.clone(),
                    context: Box::new(context),
                    fallback: Box::new(fallback),
                    finalized: false,
                })
            }
        }
    }

    pub fn breadcrumb(&self, mut breadcrumb: Breadcrumb) {
        if self.process.shared.config.breadcrumb_capacity == 0 {
            return;
        }
        breadcrumb = sanitize::breadcrumb(breadcrumb);
        breadcrumb.category = format!("{}:{}", self.component, breadcrumb.category);
        let mut breadcrumbs = self
            .process
            .shared
            .breadcrumbs
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        while breadcrumbs.len() >= self.process.shared.config.breadcrumb_capacity {
            breadcrumbs.pop_front();
        }
        breadcrumbs.push_back(breadcrumb);
    }

    pub fn heartbeat(&self, snapshot: RuntimeSnapshot) {
        let snapshot = sanitize::snapshot(snapshot);
        *self
            .process
            .shared
            .runtime_snapshot
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = snapshot.clone();
        let mut last = self
            .process
            .shared
            .last_heartbeat_sent
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if last.elapsed() >= self.process.shared.config.heartbeat_flush_interval {
            *last = Instant::now();
            let _ = self
                .process
                .shared
                .command_tx
                .send(WriterCommand::Heartbeat(snapshot));
        }
    }

    pub fn report_error(&self, context: ErrorContext, error: &dyn Error) -> IncidentTicket {
        let execution = self.execution_context(
            context.boundary,
            BoundaryKind::Worker,
            false,
            None,
            None,
            None,
            0,
        );
        let mut source_chain = Vec::new();
        let mut source = error.source();
        while let Some(current) = source {
            source_chain.push(sanitize::text(current.to_string()));
            source = current.source();
        }
        let incident = self.process.incident_record(
            &execution,
            IncidentKind::Error,
            context.severity,
            None,
            Some(ErrorDetails {
                message: sanitize::text(error.to_string()),
                source_chain,
                backtrace: Backtrace::force_capture().to_string(),
            }),
        );
        let mut incident = incident;
        incident.recovery = RecoveryOutcome::Unrecoverable(
            "the error was reported; no automatic replay or recovery was attempted".to_string(),
        );
        let incident_id = incident.incident_id.clone();
        let _ = self.process.shared.command_tx.send(WriterCommand::Record {
            incident,
            emit: true,
        });
        IncidentTicket { incident_id }
    }

    pub fn begin_operation(
        &self,
        descriptor: OperationDescriptor,
    ) -> Result<OperationGuard, WatchdogError> {
        journal::begin_operation(self, descriptor)
    }

    pub fn register_recovery_handler(
        &self,
        operation_kind: OperationKind,
        handler: Arc<dyn RecoveryHandler>,
    ) -> Result<(), WatchdogError> {
        {
            let mut handlers = self
                .process
                .shared
                .recovery_handlers
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if let Some(existing) =
                handlers.get(&(self.descriptor.id.clone(), operation_kind.clone()))
                && existing.version() != handler.version()
            {
                return Err(WatchdogError::InvalidTaskPolicy(format!(
                    "recovery handler {} is already registered at version {}",
                    operation_kind,
                    existing.version()
                )));
            }
            handlers.insert(
                (self.descriptor.id.clone(), operation_kind.clone()),
                handler.clone(),
            );
        }
        let sweep = journal::recover_pending(
            self,
            &operation_kind,
            handler,
            journal::RecoveryScope::ConfirmedStaleRuns,
        )?;
        if sweep.examined() > 0 {
            let outcome = sweep.restart_outcome();
            self.set_recovery_status(&operation_kind, outcome.clone());
            self.report_recovery_outcome(&operation_kind, outcome);
        }
        Ok(())
    }

    pub(crate) fn reconcile_checkpointed(&self, operation_kind: &OperationKind) -> RecoveryOutcome {
        let handler = self
            .process
            .shared
            .recovery_handlers
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(&(self.descriptor.id.clone(), operation_kind.clone()))
            .cloned();
        let Some(handler) = handler else {
            return RecoveryOutcome::ManualActionRequired(format!(
                "no recovery handler is registered for checkpointed operation {operation_kind}"
            ));
        };
        let outcome = match journal::recover_pending(
            self,
            operation_kind,
            handler,
            journal::RecoveryScope::CurrentRunInterrupted,
        ) {
            Ok(sweep) => sweep.restart_outcome(),
            Err(error) => RecoveryOutcome::ManualActionRequired(format!(
                "checkpoint reconciliation failed: {error}"
            )),
        };
        self.set_recovery_status(operation_kind, outcome.clone());
        outcome
    }

    pub fn recovery_status(&self, operation_kind: &OperationKind) -> Option<RecoveryOutcome> {
        self.process
            .shared
            .recovery_status
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(&(self.descriptor.id.clone(), operation_kind.clone()))
            .cloned()
    }

    fn set_recovery_status(&self, operation_kind: &OperationKind, outcome: RecoveryOutcome) {
        self.process
            .shared
            .recovery_status
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(
                (self.descriptor.id.clone(), operation_kind.clone()),
                outcome,
            );
    }

    fn report_recovery_outcome(&self, operation_kind: &OperationKind, outcome: RecoveryOutcome) {
        let context = self.execution_context(
            format!("recovery.{operation_kind}"),
            BoundaryKind::Worker,
            false,
            None,
            None,
            Some(operation_kind.clone()),
            0,
        );
        let severity = if outcome.is_recovered() {
            IncidentSeverity::Error
        } else {
            IncidentSeverity::Critical
        };
        let mut incident = self.process.incident_record(
            &context,
            IncidentKind::Error,
            severity,
            None,
            Some(ErrorDetails {
                message: sanitize::text(format!(
                    "operation recovery completed with outcome {outcome:?}"
                )),
                source_chain: Vec::new(),
                backtrace: Backtrace::force_capture().to_string(),
            }),
        );
        incident.recovery = outcome;
        let _ = self.process.shared.command_tx.send(WriterCommand::Record {
            incident,
            emit: true,
        });
    }

    pub fn current() -> Option<Self> {
        let context = current_execution_context()?;
        let app = context.app?;
        let process = context.process?;
        if !process.shared.running.load(Ordering::Acquire) {
            return None;
        }
        Some(Self {
            process,
            descriptor: app,
            component: context.component.unwrap_or_else(|| "app".to_string()),
        })
    }

    pub fn descriptor(&self) -> &AppDescriptor {
        &self.descriptor
    }

    pub fn component_path(&self) -> &str {
        &self.component
    }

    #[cfg(feature = "tokio")]
    pub async fn scope_async<F: Future>(&self, future: F) -> F::Output {
        let context = current_execution_context().unwrap_or_else(|| {
            self.execution_context(
                "app.async-scope".to_string(),
                BoundaryKind::AsyncTask,
                false,
                None,
                None,
                None,
                0,
            )
        });
        ASYNC_CONTEXT.scope(context, future).await
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn execution_context(
        &self,
        boundary: String,
        boundary_kind: BoundaryKind,
        owns_terminal: bool,
        task_id: Option<TaskId>,
        task_kind: Option<TaskKind>,
        operation_kind: Option<OperationKind>,
        restart_attempt: usize,
    ) -> ExecutionContext {
        ExecutionContext {
            process: Some(self.process.clone()),
            incident_id: self.process.next_incident_id(),
            app: Some(self.descriptor.clone()),
            component: Some(self.component.clone()),
            task_id,
            task_group: None,
            boundary,
            boundary_kind,
            owns_terminal,
            task_kind,
            replay_safety: None,
            operation_kind,
            operation_id: None,
            recovery_handler_version: None,
            panic_action: None,
            restart_policy: None,
            restart_attempt,
            expects_finalize: false,
        }
    }

    pub(crate) fn caught_from_context(
        &self,
        context: ExecutionContext,
        payload: String,
    ) -> CaughtPanic {
        let fallback = self
            .process
            .fallback_panic_incident(&context, payload.clone());
        CaughtPanic {
            process: self.process.clone(),
            context: Box::new(context),
            fallback: Box::new(fallback),
            finalized: false,
        }
    }
}

impl CaughtPanic {
    pub fn incident_id(&self) -> &str {
        &self.context.incident_id
    }

    pub fn payload(&self) -> &str {
        self.fallback
            .panic
            .as_ref()
            .map(|panic| panic.payload.as_str())
            .unwrap_or("panic payload was unavailable")
    }

    pub fn finalize(mut self, recovery: RecoveryOutcome) -> Result<IncidentReceipt, WatchdogError> {
        let (tx, rx) = mpsc::channel();
        self.finalized = true;
        self.process
            .clear_operation_context(&self.context.incident_id);
        self.process
            .shared
            .command_tx
            .send(WriterCommand::Finalize {
                incident_id: self.context.incident_id.clone(),
                recovery,
                fallback: (*self.fallback).clone(),
                response: tx,
            })
            .map_err(|_| WatchdogError::ChannelClosed)?;
        rx.recv_timeout(Duration::from_secs(2))
            .map_err(|_| WatchdogError::IncidentTimeout)?
            .map_err(|_| WatchdogError::ChannelClosed)
    }

    pub(crate) fn finalize_detached(
        mut self,
        recovery: RecoveryOutcome,
    ) -> Result<IncidentTicket, WatchdogError> {
        let (tx, _rx) = mpsc::channel();
        let incident_id = self.context.incident_id.clone();
        self.finalized = true;
        self.process.clear_operation_context(&incident_id);
        self.process
            .shared
            .command_tx
            .send(WriterCommand::Finalize {
                incident_id: incident_id.clone(),
                recovery,
                fallback: (*self.fallback).clone(),
                response: tx,
            })
            .map_err(|_| WatchdogError::ChannelClosed)?;
        Ok(IncidentTicket { incident_id })
    }
}

impl Drop for CaughtPanic {
    fn drop(&mut self) {
        if self.finalized {
            return;
        }
        self.finalized = true;
        self.process
            .clear_operation_context(&self.context.incident_id);
        let (tx, _rx) = mpsc::channel();
        let _ = self
            .process
            .shared
            .command_tx
            .send(WriterCommand::Finalize {
                incident_id: self.context.incident_id.clone(),
                recovery: RecoveryOutcome::Unrecoverable(
                    "panic boundary was dropped without an explicit recovery outcome".to_string(),
                ),
                fallback: (*self.fallback).clone(),
                response: tx,
            });
    }
}

pub(crate) fn push_thread_context(context: ExecutionContext) -> ThreadContextGuard {
    THREAD_CONTEXT.with(|contexts| contexts.borrow_mut().push(context));
    ThreadContextGuard
}

#[cfg(feature = "tokio")]
pub(crate) async fn scope_task_context<F: Future>(
    context: ExecutionContext,
    future: F,
) -> F::Output {
    ASYNC_CONTEXT.scope(context, future).await
}

pub(crate) fn current_execution_context() -> Option<ExecutionContext> {
    #[cfg(feature = "tokio")]
    if let Ok(context) = ASYNC_CONTEXT.try_with(Clone::clone) {
        return Some(context);
    }
    THREAD_CONTEXT.with(|contexts| contexts.borrow().last().cloned())
}

fn panic_payload(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(message) = payload.downcast_ref::<&str>() {
        sanitize::text(*message)
    } else if let Some(message) = payload.downcast_ref::<String>() {
        sanitize::text(message)
    } else {
        "panic payload was not a string".to_string()
    }
}

fn is_safe_run_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 160
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

pub(crate) fn catch_factory<T>(
    context: ExecutionContext,
    operation: impl FnOnce() -> T,
) -> Result<T, Box<dyn std::any::Any + Send>> {
    let guard = push_thread_context(context);
    let result = panic::catch_unwind(AssertUnwindSafe(operation));
    drop(guard);
    result
}
