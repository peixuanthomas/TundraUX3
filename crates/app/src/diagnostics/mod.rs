mod localization;

use std::collections::{BTreeMap, HashSet};
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};

use chrono::{DateTime, Utc};
use platform::{AppPaths, CheckStatus, Platform, validate_no_follow_path};
use storage::{StorageDocumentKind, StorageDocumentStatus, StorageManager, StorageRepairReport};
use watchdog::{
    AppCriticality, AppDescriptor, AppId, AppWatchdog, IncidentReportSummary, ManagedTaskGroup,
    ManagedThreadHandle, ProcessWatchdog, TaskId, TaskSpec, WatchdogError,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DiagnosticCategory {
    Environment,
    Paths,
    Storage,
    Assets,
}

impl DiagnosticCategory {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Environment => "Environment",
            Self::Paths => "Paths",
            Self::Storage => "Storage",
            Self::Assets => "Assets",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum DiagnosticStatus {
    Pass,
    Unsupported,
    Warning,
    Fail,
}

impl DiagnosticStatus {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Pass => "PASS",
            Self::Unsupported => "UNSUPPORTED",
            Self::Warning => "WARN",
            Self::Fail => "FAIL",
        }
    }
}

impl From<CheckStatus> for DiagnosticStatus {
    fn from(value: CheckStatus) -> Self {
        match value {
            CheckStatus::Pass => Self::Pass,
            CheckStatus::Warning => Self::Warning,
            CheckStatus::Fail => Self::Fail,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum DiagnosticsRepairAction {
    CreateDirectory { label: String, path: PathBuf },
    RestoreDefaultThemeFile { root: PathBuf, file_key: String },
    RepairStorageDocument(StorageDocumentKind),
}

impl DiagnosticsRepairAction {
    pub fn label(&self) -> String {
        match self {
            Self::CreateDirectory { label, .. } => format!("Create {label}"),
            Self::RestoreDefaultThemeFile { file_key, .. } => {
                format!("Restore default theme file {file_key}")
            }
            Self::RepairStorageDocument(kind) => {
                format!("Repair {} storage document", storage_document_label(*kind))
            }
        }
    }

    fn order_key(&self) -> (u8, String) {
        match self {
            Self::CreateDirectory { path, .. } => (0, path.display().to_string()),
            Self::RestoreDefaultThemeFile { root, file_key } => {
                (1, format!("{}/{}", root.display(), file_key))
            }
            Self::RepairStorageDocument(kind) => (2, storage_document_id(*kind).to_string()),
        }
    }

    pub const fn requires_restart(&self) -> bool {
        matches!(self, Self::RepairStorageDocument(_))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiagnosticCheck {
    pub id: String,
    pub category: DiagnosticCategory,
    pub label: String,
    pub status: DiagnosticStatus,
    pub summary: String,
    pub detail: String,
    pub remediation: Option<String>,
    pub repair: Option<DiagnosticsRepairAction>,
}

/// A regular log file retained beneath the application's logs directory.
///
/// `path` is the file system path used to open the file; `relative_path` is
/// retained separately so callers can display a non-sensitive, stable label.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiagnosticLogFile {
    pub path: PathBuf,
    pub relative_path: PathBuf,
    pub modified_at: DateTime<Utc>,
    pub size_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiagnosticsSnapshot {
    pub scanned_at: DateTime<Utc>,
    pub checks: Vec<DiagnosticCheck>,
    pub incidents: Vec<IncidentReportSummary>,
    pub logs: Vec<DiagnosticLogFile>,
    pub warnings: Vec<String>,
}

impl DiagnosticsSnapshot {
    pub fn overall_status(&self) -> DiagnosticStatus {
        if self
            .checks
            .iter()
            .any(|check| check.status == DiagnosticStatus::Fail)
        {
            DiagnosticStatus::Fail
        } else if self
            .checks
            .iter()
            .any(|check| check.status == DiagnosticStatus::Warning)
            || !self.warnings.is_empty()
        {
            DiagnosticStatus::Warning
        } else if self
            .checks
            .iter()
            .any(|check| check.status == DiagnosticStatus::Unsupported)
        {
            DiagnosticStatus::Unsupported
        } else {
            DiagnosticStatus::Pass
        }
    }

    pub fn status_counts(&self) -> (usize, usize, usize, usize) {
        self.checks.iter().fold((0, 0, 0, 0), |mut counts, check| {
            match check.status {
                DiagnosticStatus::Pass => counts.0 += 1,
                DiagnosticStatus::Unsupported => counts.1 += 1,
                DiagnosticStatus::Warning => counts.2 += 1,
                DiagnosticStatus::Fail => counts.3 += 1,
            }
            counts
        })
    }

    pub fn repair_plan(&self) -> Vec<DiagnosticsRepairAction> {
        let mut seen = HashSet::new();
        let mut actions = self
            .checks
            .iter()
            .filter_map(|check| check.repair.clone())
            .filter(|action| seen.insert(action.clone()))
            .collect::<Vec<_>>();
        actions.sort_by_key(DiagnosticsRepairAction::order_key);
        actions
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiagnosticsRepairResult {
    pub action: DiagnosticsRepairAction,
    pub success: bool,
    pub changed: bool,
    pub message: String,
    pub backup_path: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub enum DiagnosticsTaskEvent {
    ScanCompleted(Result<DiagnosticsSnapshot, String>),
    Cancelled,
    Failed(String),
    RepairProgress {
        completed: usize,
        total: usize,
        label: i18n::LocalizedText,
    },
    RepairCompleted {
        results: Vec<DiagnosticsRepairResult>,
        snapshot: Option<DiagnosticsSnapshot>,
        restart_required: bool,
        cancelled: bool,
    },
}

#[derive(Debug, Clone)]
pub struct DiagnosticsTaskEnvelope {
    pub request_id: u64,
    pub event: DiagnosticsTaskEvent,
}
impl DiagnosticsTaskEvent {
    pub fn is_terminal(&self) -> bool {
        !matches!(self, Self::RepairProgress { .. })
    }
}
#[derive(Debug, Clone)]
pub struct DiagnosticsTaskHandle {
    pub request_id: u64,
    cancelled: Arc<AtomicBool>,
    finished: Arc<AtomicBool>,
}
impl DiagnosticsTaskHandle {
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }
    pub fn is_finished(&self) -> bool {
        self.finished.load(Ordering::Acquire)
    }
}
impl PartialEq for DiagnosticsTaskHandle {
    fn eq(&self, other: &Self) -> bool {
        self.request_id == other.request_id
    }
}
impl Eq for DiagnosticsTaskHandle {}
#[derive(Debug, Clone)]
enum WorkerCommand {
    Scan,
    Repair(Vec<DiagnosticsRepairAction>),
}
struct DiagnosticsWorker {
    task: DiagnosticsTaskHandle,
    worker: ManagedThreadHandle<()>,
    slot: usize,
    repair: bool,
}
pub struct DiagnosticsTaskRuntime {
    platform: Arc<dyn Platform>,
    storage: StorageManager,
    process: ProcessWatchdog,
    group: ManagedTaskGroup,
    event_tx: mpsc::Sender<DiagnosticsTaskEnvelope>,
    event_rx: mpsc::Receiver<DiagnosticsTaskEnvelope>,
    workers: Mutex<BTreeMap<u64, DiagnosticsWorker>>,
    restart_required: Arc<AtomicBool>,
}
static NEXT_DIAGNOSTICS_REQUEST: AtomicU64 = AtomicU64::new(1);
impl DiagnosticsTaskRuntime {
    pub fn new_managed(
        platform: Arc<dyn Platform>,
        storage: StorageManager,
        process: ProcessWatchdog,
        watchdog: AppWatchdog,
    ) -> Result<Self, WatchdogError> {
        let (event_tx, event_rx) = mpsc::channel();
        Ok(Self {
            platform,
            storage,
            process,
            group: watchdog.task_group("diagnostics-worker"),
            event_tx,
            event_rx,
            workers: Mutex::new(BTreeMap::new()),
            restart_required: Arc::new(AtomicBool::new(false)),
        })
    }
    pub fn request_scan(&self) -> Result<DiagnosticsTaskHandle, DiagnosticsTaskError> {
        self.submit(WorkerCommand::Scan)
    }
    pub fn request_repair(
        &self,
        actions: Vec<DiagnosticsRepairAction>,
    ) -> Result<DiagnosticsTaskHandle, DiagnosticsTaskError> {
        if self.restart_required() {
            return Err(DiagnosticsTaskError::RestartRequired);
        }
        if actions.is_empty() {
            return Err(DiagnosticsTaskError::EmptyRepairPlan);
        }
        self.submit(WorkerCommand::Repair(actions))
    }
    fn submit(
        &self,
        command: WorkerCommand,
    ) -> Result<DiagnosticsTaskHandle, DiagnosticsTaskError> {
        let mut workers = self
            .workers
            .lock()
            .map_err(|_| DiagnosticsTaskError::WorkerStopped)?;
        if self.restart_required() {
            return Err(DiagnosticsTaskError::RestartRequired);
        }
        let repair = matches!(command, WorkerCommand::Repair(_));
        // Pages scan independently. A repair exclusively owns shared documents.
        if workers.values().any(|worker| worker.repair) || (repair && !workers.is_empty()) {
            return Err(DiagnosticsTaskError::Busy);
        }
        // Watchdog keeps completed task registrations. Reuse a finished slot
        // while keeping externally visible request ids unique.
        let slot = (0..)
            .find(|slot| workers.values().all(|worker| worker.slot != *slot))
            .unwrap();
        let id = NEXT_DIAGNOSTICS_REQUEST.fetch_add(1, Ordering::Relaxed);
        let task = DiagnosticsTaskHandle {
            request_id: id,
            cancelled: Arc::new(AtomicBool::new(false)),
            finished: Arc::new(AtomicBool::new(false)),
        };
        let worker_task = task.clone();
        let platform = self.platform.clone();
        let storage = self.storage.clone();
        let process = self.process.clone();
        let restart = self.restart_required.clone();
        let events = self.event_tx.clone();
        let worker = self
            .group
            .spawn_thread_with_cancellation(
                TaskSpec::one_shot(
                    TaskId::new(format!("diagnostics-{slot}"))
                        .map_err(|_| DiagnosticsTaskError::WorkerStopped)?,
                ),
                move |watchdog_cancel| {
                    struct Finished(Arc<AtomicBool>);
                    impl Drop for Finished {
                        fn drop(&mut self) {
                            self.0.store(true, Ordering::Release);
                        }
                    }
                    let _finished = Finished(worker_task.finished.clone());
                    let cancelled = || {
                        worker_task.cancelled.load(Ordering::Acquire)
                            || watchdog_cancel.is_cancelled()
                    };
                    let emit = |event| {
                        let _ = events.send(DiagnosticsTaskEnvelope {
                            request_id: id,
                            event,
                        });
                    };
                    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        run_diagnostics_task(
                            platform.as_ref(),
                            &storage,
                            &process,
                            command.clone(),
                            &restart,
                            &cancelled,
                            &emit,
                        )
                    }));
                    if let Err(payload) = result {
                        emit(DiagnosticsTaskEvent::Failed(
                            "Diagnostics worker panicked".into(),
                        ));
                        std::panic::resume_unwind(payload);
                    }
                },
            )
            .map_err(|_| DiagnosticsTaskError::WorkerStopped)?;
        workers.insert(
            id,
            DiagnosticsWorker {
                task: task.clone(),
                worker,
                slot,
                repair,
            },
        );
        Ok(task)
    }
    pub fn is_busy(&self) -> bool {
        self.workers.lock().is_ok_and(|workers| !workers.is_empty())
    }
    pub fn restart_required(&self) -> bool {
        self.restart_required.load(Ordering::Acquire)
    }
    pub fn drain_events(&self) -> Vec<DiagnosticsTaskEnvelope> {
        let mut events = Vec::new();
        let Ok(mut workers) = self.workers.lock() else {
            return events;
        };
        // Snapshot completion before reading the channel: a worker can send its
        // terminal result and finish between channel reads.
        let finished_ids: HashSet<_> = workers
            .iter()
            .filter(|(_, record)| record.worker.is_finished())
            .map(|(id, _)| *id)
            .collect();
        let pending: Vec<_> = self.event_rx.try_iter().collect();
        let terminal_ids: HashSet<_> = pending
            .iter()
            .filter(|event| event.event.is_terminal())
            .map(|event| event.request_id)
            .collect();
        for mut envelope in pending {
            if envelope.event.is_terminal() {
                if workers
                    .get(&envelope.request_id)
                    .is_some_and(|record| !record.worker.is_finished())
                {
                    let _ = self.event_tx.send(envelope);
                    continue;
                }
                if let Some(record) = workers.remove(&envelope.request_id) {
                    if record.task.cancelled.load(Ordering::Acquire)
                        && matches!(envelope.event, DiagnosticsTaskEvent::ScanCompleted(_))
                    {
                        envelope.event = DiagnosticsTaskEvent::Cancelled;
                    }
                    let _ = record.worker.join();
                    record.task.finished.store(true, Ordering::Release);
                }
            }
            events.push(envelope);
        }
        // Watchdog may cancel a queued worker before its closure ever starts.
        let stopped: Vec<_> = workers
            .iter()
            .filter(|(id, _)| finished_ids.contains(id) && !terminal_ids.contains(id))
            .map(|(id, _)| *id)
            .collect();
        for id in stopped {
            let record = workers.remove(&id).expect("stopped diagnostics worker");
            let _ = record.worker.join();
            record.task.finished.store(true, Ordering::Release);
            events.push(DiagnosticsTaskEnvelope {
                request_id: id,
                event: DiagnosticsTaskEvent::Cancelled,
            });
        }
        events
    }
}
impl Drop for DiagnosticsTaskRuntime {
    fn drop(&mut self) {
        // Watchdog owns the join; dropping UI state never blocks mid-repair.
        if let Ok(workers) = self.workers.get_mut() {
            for record in workers.values() {
                record.task.cancel();
                record.worker.cancel();
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiagnosticsTaskError {
    Busy,
    EmptyRepairPlan,
    RestartRequired,
    WorkerStopped,
}

impl fmt::Display for DiagnosticsTaskError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Busy => formatter.write_str("a diagnostics task is already running"),
            Self::EmptyRepairPlan => formatter.write_str("there are no repairable diagnostics"),
            Self::RestartRequired => {
                formatter.write_str("restart TundraUX before running more diagnostics tasks")
            }
            Self::WorkerStopped => formatter.write_str("the diagnostics worker stopped"),
        }
    }
}

impl std::error::Error for DiagnosticsTaskError {}

pub fn diagnostics_watchdog_descriptor() -> AppDescriptor {
    AppDescriptor::new(
        AppId::from_static("diagnostics"),
        "Tundra Diagnostics",
        env!("CARGO_PKG_VERSION"),
        AppCriticality::Optional,
    )
}

fn environment_diagnostic(check: platform::EnvironmentCheck) -> DiagnosticCheck {
    DiagnosticCheck {
        id: format!("environment.{}", check.id),
        category: DiagnosticCategory::Environment,
        label: check.label,
        status: check.status.into(),
        summary: check.message.clone(),
        detail: check.message,
        remediation: remediation_for_environment(check.status),
        repair: None,
    }
}

pub fn scan_diagnostics(
    platform: &dyn Platform,
    storage: &StorageManager,
    process: &ProcessWatchdog,
) -> Result<DiagnosticsSnapshot, String> {
    scan_diagnostics_cancellable(platform, storage, process, &|| false)
}

fn scan_diagnostics_cancellable(
    platform: &dyn Platform,
    storage: &StorageManager,
    process: &ProcessWatchdog,
    cancelled: &dyn Fn() -> bool,
) -> Result<DiagnosticsSnapshot, String> {
    check_diagnostics_cancel(cancelled)?;
    let app_paths = platform.app_paths().map_err(|error| error.to_string())?;
    let expected_paths = expected_app_directories(&app_paths);
    let missing_paths = expected_paths
        .iter()
        .filter(|(_, path)| !path.exists())
        .map(|(_, path)| path.clone())
        .collect::<HashSet<_>>();
    check_diagnostics_cancel(cancelled)?;
    let doctor = platform::run_doctor_with(platform).map_err(|error| error.to_string())?;

    let mut checks = doctor
        .environment_checks
        .into_iter()
        .map(environment_diagnostic)
        .collect::<Vec<_>>();

    checks.extend(doctor.path_checks.into_iter().map(|check| {
        let was_missing = missing_paths.contains(&check.path);
        let status = if was_missing && check.status == CheckStatus::Pass {
            DiagnosticStatus::Warning
        } else {
            check.status.into()
        };
        let repair = (was_missing && check.status != CheckStatus::Fail).then(|| {
            DiagnosticsRepairAction::CreateDirectory {
                label: check.label.clone(),
                path: check.path.clone(),
            }
        });
        let summary = if was_missing && check.status == CheckStatus::Pass {
            "Directory is missing but can be created".to_string()
        } else {
            check.message.clone()
        };
        DiagnosticCheck {
            id: format!("path.{}", check.id),
            category: DiagnosticCategory::Paths,
            label: check.label,
            status,
            summary,
            detail: format!("{} — {}", check.path.display(), check.message),
            remediation: match status {
                DiagnosticStatus::Pass => None,
                DiagnosticStatus::Unsupported => None,
                DiagnosticStatus::Warning if repair.is_some() => {
                    Some("Create the missing application directory".to_string())
                }
                _ => Some("Check the path and its read/write permissions".to_string()),
            },
            repair,
        }
    }));

    check_diagnostics_cancel(cancelled)?;
    let storage_checks = storage
        .check_documents()
        .map_err(|error| error.to_string())?;
    checks.extend(storage_checks.into_iter().map(|check| {
        let (status, remediation, repair) = match check.status {
            StorageDocumentStatus::Healthy => (DiagnosticStatus::Pass, None, None),
            StorageDocumentStatus::Missing => (
                DiagnosticStatus::Warning,
                Some("Create a default document".to_string()),
                Some(DiagnosticsRepairAction::RepairStorageDocument(check.kind)),
            ),
            StorageDocumentStatus::Corrupt => (
                DiagnosticStatus::Fail,
                Some("Back up the damaged document and rebuild a default".to_string()),
                Some(DiagnosticsRepairAction::RepairStorageDocument(check.kind)),
            ),
            StorageDocumentStatus::UnsupportedSchema => (
                DiagnosticStatus::Fail,
                Some(
                    "Use a compatible TundraUX version; automatic downgrade is blocked".to_string(),
                ),
                None,
            ),
        };
        DiagnosticCheck {
            id: format!("storage.{}", storage_document_id(check.kind)),
            category: DiagnosticCategory::Storage,
            label: storage_document_label(check.kind).to_string(),
            status,
            summary: check.message.clone(),
            detail: format!("{} — {}", check.path.display(), check.message),
            remediation,
            repair,
        }
    }));

    check_diagnostics_cancel(cancelled)?;
    let mut warnings = Vec::new();
    match ascii_assets::asset_root_from_env_or_current_exe() {
        Ok(root) => {
            checks.extend(diagnostic_asset_checks(&root));
        }
        Err(
            ascii_assets::AssetError::MissingRoot { path }
            | ascii_assets::AssetError::RootNotDirectory { path },
        ) => {
            checks.extend(diagnostic_asset_checks(&path));
        }
        Err(error) => {
            warnings.push(format!("Could not resolve asset root: {error}"));
            checks.push(DiagnosticCheck {
                id: "assets.root".to_string(),
                category: DiagnosticCategory::Assets,
                label: "Asset root".to_string(),
                status: DiagnosticStatus::Warning,
                summary: error.to_string(),
                detail: error.to_string(),
                remediation: Some("Reinstall TundraUX or configure its asset root".to_string()),
                repair: None,
            });
        }
    }

    let (logs, log_warnings) = scan_diagnostic_logs_cancellable(app_paths.logs_path(), cancelled);
    warnings.extend(log_warnings);

    check_diagnostics_cancel(cancelled)?;
    let incident_catalog = process.list_incident_reports();
    for (index, warning) in incident_catalog.warnings.into_iter().enumerate() {
        checks.push(DiagnosticCheck {
            id: format!("incident-history.warning-{index}"),
            category: DiagnosticCategory::Storage,
            label: "Incident history".to_string(),
            status: DiagnosticStatus::Warning,
            summary: "Some incident reports could not be loaded".to_string(),
            detail: warning.clone(),
            remediation: Some("Review or archive the unreadable incident report".to_string()),
            repair: None,
        });
        warnings.push(warning);
    }
    check_diagnostics_cancel(cancelled)?;
    Ok(DiagnosticsSnapshot {
        scanned_at: Utc::now(),
        checks,
        incidents: incident_catalog.reports,
        logs,
        warnings,
    })
}

/// Recursively enumerates regular `.log` and rotated `.log.*` files below
/// `logs_root`. The `crashes` directory and all symbolic links are skipped so
/// diagnostics never traverse or surface paths outside the application log
/// root. Traversal failures are returned as user-visible warnings.
pub fn scan_diagnostic_logs(logs_root: &Path) -> (Vec<DiagnosticLogFile>, Vec<String>) {
    scan_diagnostic_logs_cancellable(logs_root, &|| false)
}
fn scan_diagnostic_logs_cancellable(
    logs_root: &Path,
    cancelled: &dyn Fn() -> bool,
) -> (Vec<DiagnosticLogFile>, Vec<String>) {
    let mut logs = Vec::new();
    let mut warnings = Vec::new();

    if let Err(error) = validate_no_follow_path(logs_root, false) {
        warnings.push(format!(
            "Log history: could not validate log root {}: {error}",
            logs_root.display()
        ));
        return (logs, warnings);
    }
    if !logs_root.exists() {
        return (logs, warnings);
    }
    let mut directories = vec![logs_root.to_path_buf()];
    while let Some(directory) = directories.pop() {
        if cancelled() {
            break;
        }
        scan_diagnostic_log_directory(
            logs_root,
            &directory,
            &mut directories,
            &mut logs,
            &mut warnings,
            cancelled,
        );
    }
    logs.sort_by(|left, right| {
        right
            .modified_at
            .cmp(&left.modified_at)
            .then_with(|| left.relative_path.cmp(&right.relative_path))
    });
    (logs, warnings)
}

fn scan_diagnostic_log_directory(
    logs_root: &Path,
    directory: &Path,
    directories: &mut Vec<PathBuf>,
    logs: &mut Vec<DiagnosticLogFile>,
    warnings: &mut Vec<String>,
    cancelled: &dyn Fn() -> bool,
) {
    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) => {
            warnings.push(format!(
                "Log history: could not read diagnostic log directory {}: {error}",
                display_log_relative_path(logs_root, directory)
            ));
            return;
        }
    };

    for entry in entries {
        if cancelled() {
            return;
        }
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                warnings.push(format!(
                    "Log history: could not enumerate a log entry: {error}"
                ));
                continue;
            }
        };
        let path = entry.path();
        if let Err(error) = validate_no_follow_path(&path, true) {
            warnings.push(format!(
                "Log history: skipped unsafe log entry {}: {error}",
                display_log_relative_path(logs_root, &path)
            ));
            continue;
        }
        let file_type = match entry.file_type() {
            Ok(file_type) => file_type,
            Err(error) => {
                warnings.push(format!(
                    "Log history: could not inspect log entry {}: {error}",
                    display_log_relative_path(logs_root, &path)
                ));
                continue;
            }
        };

        // `file_type` reports links without following them. Skipping every
        // link prevents recursive cycles and links that escape `logs_root`.
        if file_type.is_symlink() {
            continue;
        }
        if file_type.is_dir() {
            if path
                .file_name()
                .is_some_and(|name| name.to_string_lossy().eq_ignore_ascii_case("crashes"))
            {
                continue;
            }
            directories.push(path);
            continue;
        }
        if !file_type.is_file() || !is_diagnostic_log_file(&path) {
            continue;
        }

        let metadata = match entry.metadata() {
            Ok(metadata) => metadata,
            Err(error) => {
                warnings.push(format!(
                    "Log history: could not read log metadata for {}: {error}",
                    display_log_relative_path(logs_root, &path)
                ));
                continue;
            }
        };
        let modified_at = match metadata.modified() {
            Ok(modified_at) => DateTime::<Utc>::from(modified_at),
            Err(error) => {
                warnings.push(format!(
                    "Log history: could not read log timestamp for {}: {error}",
                    display_log_relative_path(logs_root, &path)
                ));
                continue;
            }
        };
        let relative_path = path
            .strip_prefix(logs_root)
            .expect("log traversal only descends from its root")
            .to_path_buf();
        logs.push(DiagnosticLogFile {
            path,
            relative_path,
            modified_at,
            size_bytes: metadata.len(),
        });
    }
}

fn is_diagnostic_log_file(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| {
            let lower = name.to_ascii_lowercase();
            lower.ends_with(".log") || lower.contains(".log.")
        })
}

fn display_log_relative_path(logs_root: &Path, path: &Path) -> String {
    path.strip_prefix(logs_root)
        .ok()
        .filter(|relative| !relative.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
        .display()
        .to_string()
}

fn diagnostic_asset_checks(root: &Path) -> Vec<DiagnosticCheck> {
    // StorageConfig::theme names the UI color palette (for example, "dark").
    // Runtime ASCII art is loaded from its independently named default theme.
    let report = ascii_assets::check_default_theme(root);
    report
        .checks
        .into_iter()
        .map(|check| {
            let status = match check.status {
                ascii_assets::AssetCheckStatus::Pass => DiagnosticStatus::Pass,
                ascii_assets::AssetCheckStatus::Warning => DiagnosticStatus::Warning,
            };
            let repair = (status == DiagnosticStatus::Warning).then(|| {
                DiagnosticsRepairAction::RestoreDefaultThemeFile {
                    root: root.to_path_buf(),
                    file_key: check.key.clone(),
                }
            });
            DiagnosticCheck {
                id: format!("asset.{}", check.key),
                category: DiagnosticCategory::Assets,
                label: check.key,
                status,
                summary: check.message.clone(),
                detail: format!("{} — {}", check.path.display(), check.message),
                remediation: (status != DiagnosticStatus::Pass)
                    .then(|| "Restore this file from the built-in default theme".to_string()),
                repair,
            }
        })
        .collect()
}

fn run_diagnostics_task(
    platform: &dyn Platform,
    storage: &StorageManager,
    process: &ProcessWatchdog,
    command: WorkerCommand,
    restart_required: &AtomicBool,
    cancelled: &dyn Fn() -> bool,
    emit: &dyn Fn(DiagnosticsTaskEvent),
) {
    match command {
        WorkerCommand::Scan => {
            let result = scan_diagnostics_cancellable(platform, storage, process, cancelled);
            emit(if cancelled() {
                DiagnosticsTaskEvent::Cancelled
            } else {
                DiagnosticsTaskEvent::ScanCompleted(result)
            });
        }
        WorkerCommand::Repair(actions) => {
            let total = actions.len();
            let mut task_restart_required = false;
            let mut results = Vec::with_capacity(total);
            for (index, action) in actions.into_iter().enumerate() {
                if cancelled() {
                    break;
                }
                emit(DiagnosticsTaskEvent::RepairProgress {
                    completed: index,
                    total,
                    label: action.localized_label(),
                });
                if cancelled() {
                    break;
                }
                // Once an action starts, finish its save/backup before cancelling.
                let result = execute_repair(platform, storage, action);
                task_restart_required |= result.success
                    && result.changed
                    && matches!(
                        &result.action,
                        DiagnosticsRepairAction::RepairStorageDocument(_)
                    );
                if task_restart_required {
                    restart_required.store(true, Ordering::Release);
                }
                results.push(result);
            }
            let snapshot = if task_restart_required || cancelled() {
                None
            } else {
                scan_diagnostics_cancellable(platform, storage, process, cancelled).ok()
            };
            emit(DiagnosticsTaskEvent::RepairCompleted {
                results,
                snapshot,
                restart_required: task_restart_required,
                cancelled: cancelled(),
            });
        }
    }
}

fn execute_repair(
    _platform: &dyn Platform,
    storage: &StorageManager,
    action: DiagnosticsRepairAction,
) -> DiagnosticsRepairResult {
    match &action {
        DiagnosticsRepairAction::CreateDirectory { path, .. } => {
            let existed = path.exists();
            let result = std::fs::create_dir_all(path)
                .map_err(|error| error.to_string())
                .and_then(|()| {
                    let check = platform::check_directory_read_write("Repaired path", path);
                    if check.status == CheckStatus::Fail {
                        Err(check.message)
                    } else {
                        Ok(check.message)
                    }
                });
            match result {
                Ok(message) => DiagnosticsRepairResult {
                    action,
                    success: true,
                    changed: !existed,
                    message,
                    backup_path: None,
                },
                Err(message) => DiagnosticsRepairResult {
                    action,
                    success: false,
                    changed: false,
                    message,
                    backup_path: None,
                },
            }
        }
        DiagnosticsRepairAction::RestoreDefaultThemeFile { root, file_key } => {
            match ascii_assets::restore_default_theme_file(root, file_key) {
                Ok(report) => DiagnosticsRepairResult {
                    action,
                    success: true,
                    changed: report.changed,
                    message: if report.changed {
                        format!(
                            "Restored the default theme file at {}",
                            report.path.display()
                        )
                    } else {
                        "Theme file already matches the built-in default".to_string()
                    },
                    backup_path: None,
                },
                Err(error) => DiagnosticsRepairResult {
                    action,
                    success: false,
                    changed: false,
                    message: error.to_string(),
                    backup_path: None,
                },
            }
        }
        DiagnosticsRepairAction::RepairStorageDocument(kind) => {
            let result: Result<StorageRepairReport, _> = storage.repair_document(*kind);
            match result {
                Ok(report) => DiagnosticsRepairResult {
                    action,
                    success: true,
                    changed: report.created || report.rebuilt,
                    message: if report.rebuilt {
                        "Backed up and rebuilt the storage document".to_string()
                    } else if report.created {
                        "Created the missing storage document".to_string()
                    } else {
                        "Storage document is already healthy".to_string()
                    },
                    backup_path: report.backup_path,
                },
                Err(error) => DiagnosticsRepairResult {
                    action,
                    success: false,
                    changed: false,
                    message: error.to_string(),
                    backup_path: None,
                },
            }
        }
    }
}

fn expected_app_directories(paths: &AppPaths) -> Vec<(String, PathBuf)> {
    vec![
        (
            "Config parent".to_string(),
            paths
                .config_path()
                .parent()
                .unwrap_or_else(|| Path::new("."))
                .to_path_buf(),
        ),
        ("Data path".to_string(), paths.data_path().to_path_buf()),
        ("Cache path".to_string(), paths.cache_path().to_path_buf()),
        ("Logs path".to_string(), paths.logs_path().to_path_buf()),
        ("Temp path".to_string(), paths.temp_path().to_path_buf()),
    ]
}

fn remediation_for_environment(status: CheckStatus) -> Option<String> {
    match status {
        CheckStatus::Pass => None,
        CheckStatus::Warning => Some("Review the platform capability guidance".to_string()),
        CheckStatus::Fail => {
            Some("Use a supported platform and terminal configuration".to_string())
        }
    }
}

pub const fn storage_document_label(kind: StorageDocumentKind) -> &'static str {
    match kind {
        StorageDocumentKind::Config => "Configuration",
        StorageDocumentKind::Users => "Users",
        StorageDocumentKind::State => "State",
        StorageDocumentKind::RecentFiles => "Recent files",
        StorageDocumentKind::Sessions => "Sessions",
        StorageDocumentKind::Clock => "Clock",
        StorageDocumentKind::TrashManifest => "Trash manifest",
    }
}

const fn storage_document_id(kind: StorageDocumentKind) -> &'static str {
    match kind {
        StorageDocumentKind::Config => "config",
        StorageDocumentKind::Users => "users",
        StorageDocumentKind::State => "state",
        StorageDocumentKind::RecentFiles => "recent-files",
        StorageDocumentKind::Sessions => "sessions",
        StorageDocumentKind::Clock => "clock",
        StorageDocumentKind::TrashManifest => "trash-manifest",
    }
}

#[cfg(test)]
#[path = "../../tests/unit/diagnostics/tests.rs"]
mod tests;

fn check_diagnostics_cancel(cancelled: &dyn Fn() -> bool) -> Result<(), String> {
    if cancelled() {
        Err("Diagnostics cancelled".into())
    } else {
        Ok(())
    }
}
