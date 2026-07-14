use std::collections::HashSet;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;

use chrono::{DateTime, Utc};
use tundra_platform::{AppPaths, CheckStatus, Platform};
use tundra_storage::{
    StorageDocumentKind, StorageDocumentStatus, StorageManager, StorageRepairReport,
};
use tundra_watchdog::{
    AppCriticality, AppDescriptor, AppId, AppWatchdog, IncidentReportSummary, ManagedThreadHandle,
    PanicAction, ProcessWatchdog, ReplaySafety, RestartPolicy, TaskId, TaskKind, TaskSpec,
    WatchdogError,
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
    Warning,
    Fail,
}

impl DiagnosticStatus {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Pass => "PASS",
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
    RepairStorageDocument(StorageDocumentKind),
}

impl DiagnosticsRepairAction {
    pub fn label(&self) -> String {
        match self {
            Self::CreateDirectory { label, .. } => format!("Create {label}"),
            Self::RepairStorageDocument(kind) => {
                format!("Repair {} storage document", storage_document_label(*kind))
            }
        }
    }

    fn order_key(&self) -> (u8, String) {
        match self {
            Self::CreateDirectory { path, .. } => (0, path.display().to_string()),
            Self::RepairStorageDocument(kind) => (1, storage_document_label(*kind).to_string()),
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiagnosticsSnapshot {
    pub scanned_at: DateTime<Utc>,
    pub checks: Vec<DiagnosticCheck>,
    pub incidents: Vec<IncidentReportSummary>,
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
        } else {
            DiagnosticStatus::Pass
        }
    }

    pub fn status_counts(&self) -> (usize, usize, usize) {
        self.checks.iter().fold((0, 0, 0), |mut counts, check| {
            match check.status {
                DiagnosticStatus::Pass => counts.0 += 1,
                DiagnosticStatus::Warning => counts.1 += 1,
                DiagnosticStatus::Fail => counts.2 += 1,
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
    RepairProgress {
        completed: usize,
        total: usize,
        label: String,
    },
    RepairCompleted {
        results: Vec<DiagnosticsRepairResult>,
        snapshot: Option<DiagnosticsSnapshot>,
        restart_required: bool,
    },
}

#[derive(Debug)]
enum WorkerCommand {
    Scan,
    Repair(Vec<DiagnosticsRepairAction>),
    Shutdown,
}

pub struct DiagnosticsTaskRuntime {
    command_tx: mpsc::Sender<WorkerCommand>,
    event_rx: mpsc::Receiver<DiagnosticsTaskEvent>,
    busy: Arc<AtomicBool>,
    restart_required: Arc<AtomicBool>,
    worker: Option<ManagedThreadHandle<()>>,
}

impl DiagnosticsTaskRuntime {
    pub fn new_managed(
        platform: Arc<dyn Platform>,
        storage: StorageManager,
        process: ProcessWatchdog,
        watchdog: AppWatchdog,
    ) -> Result<Self, WatchdogError> {
        let (command_tx, command_rx) = mpsc::channel();
        let (event_tx, event_rx) = mpsc::channel();
        let busy = Arc::new(AtomicBool::new(false));
        let restart_required = Arc::new(AtomicBool::new(false));
        let worker_restart_required = Arc::clone(&restart_required);
        let group = watchdog.task_group("diagnostics-worker");
        let mut worker_inputs = Some((
            platform,
            storage,
            process,
            command_rx,
            event_tx,
            worker_restart_required,
        ));
        let worker = group.spawn_thread(
            TaskSpec {
                id: TaskId::from_static("event-loop"),
                kind: TaskKind::LongRunning,
                panic_action: PanicAction::ReportOnly,
                replay_safety: ReplaySafety::Never,
                restart_policy: RestartPolicy::never(),
            },
            move || {
                let (platform, storage, process, command_rx, event_tx, worker_restart_required) =
                    worker_inputs
                        .take()
                        .expect("the non-restartable Diagnostics worker factory runs once");
                diagnostics_worker_loop(
                    platform,
                    storage,
                    process,
                    command_rx,
                    event_tx,
                    worker_restart_required,
                )
            },
        )?;
        Ok(Self {
            command_tx,
            event_rx,
            busy,
            restart_required,
            worker: Some(worker),
        })
    }

    pub fn request_scan(&self) -> Result<(), DiagnosticsTaskError> {
        self.submit(WorkerCommand::Scan)
    }

    pub fn request_repair(
        &self,
        actions: Vec<DiagnosticsRepairAction>,
    ) -> Result<(), DiagnosticsTaskError> {
        if self.restart_required() {
            return Err(DiagnosticsTaskError::RestartRequired);
        }
        if actions.is_empty() {
            return Err(DiagnosticsTaskError::EmptyRepairPlan);
        }
        self.submit(WorkerCommand::Repair(actions))
    }

    fn submit(&self, command: WorkerCommand) -> Result<(), DiagnosticsTaskError> {
        if self.restart_required() {
            return Err(DiagnosticsTaskError::RestartRequired);
        }
        if self
            .busy
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return Err(if self.restart_required() {
                DiagnosticsTaskError::RestartRequired
            } else {
                DiagnosticsTaskError::Busy
            });
        }
        if self.restart_required() {
            self.busy.store(false, Ordering::Release);
            return Err(DiagnosticsTaskError::RestartRequired);
        }
        if self.command_tx.send(command).is_err() {
            self.busy.store(false, Ordering::Release);
            return Err(DiagnosticsTaskError::WorkerStopped);
        }
        Ok(())
    }

    pub fn is_busy(&self) -> bool {
        self.busy.load(Ordering::Acquire)
    }

    pub fn restart_required(&self) -> bool {
        self.restart_required.load(Ordering::Acquire)
    }

    pub fn drain_events(&self) -> Vec<DiagnosticsTaskEvent> {
        let mut events = Vec::new();
        while let Ok(event) = self.event_rx.try_recv() {
            if matches!(
                event,
                DiagnosticsTaskEvent::ScanCompleted(_)
                    | DiagnosticsTaskEvent::RepairCompleted { .. }
            ) {
                self.busy.store(false, Ordering::Release);
            }
            events.push(event);
        }
        events
    }
}

impl Drop for DiagnosticsTaskRuntime {
    fn drop(&mut self) {
        let _ = self.command_tx.send(WorkerCommand::Shutdown);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
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

pub fn scan_diagnostics(
    platform: &dyn Platform,
    storage: &StorageManager,
    process: &ProcessWatchdog,
) -> Result<DiagnosticsSnapshot, String> {
    let app_paths = platform.app_paths().map_err(|error| error.to_string())?;
    let expected_paths = expected_app_directories(&app_paths);
    let missing_paths = expected_paths
        .iter()
        .filter(|(_, path)| !path.exists())
        .map(|(_, path)| path.clone())
        .collect::<HashSet<_>>();
    let doctor = tundra_platform::run_doctor_with(platform).map_err(|error| error.to_string())?;

    let mut checks = doctor
        .environment_checks
        .into_iter()
        .map(|check| DiagnosticCheck {
            id: stable_id("environment", &check.label),
            category: DiagnosticCategory::Environment,
            label: check.label,
            status: check.status.into(),
            summary: check.message.clone(),
            detail: check.message,
            remediation: remediation_for_environment(check.status),
            repair: None,
        })
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
            id: stable_id("path", &check.label),
            category: DiagnosticCategory::Paths,
            label: check.label,
            status,
            summary,
            detail: format!("{} — {}", check.path.display(), check.message),
            remediation: match status {
                DiagnosticStatus::Pass => None,
                DiagnosticStatus::Warning if repair.is_some() => {
                    Some("Create the missing application directory".to_string())
                }
                _ => Some("Check the path and its read/write permissions".to_string()),
            },
            repair,
        }
    }));

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

    let mut warnings = Vec::new();
    match tundra_ascii_assets::asset_root_from_env_or_current_exe() {
        Ok(root) => {
            checks.extend(diagnostic_asset_checks(&root));
        }
        Err(error) => {
            warnings.push(format!("Could not resolve ASCII asset root: {error}"));
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
    Ok(DiagnosticsSnapshot {
        scanned_at: Utc::now(),
        checks,
        incidents: incident_catalog.reports,
        warnings,
    })
}

fn diagnostic_asset_checks(root: &Path) -> Vec<DiagnosticCheck> {
    // StorageConfig::theme names the UI color palette (for example, "dark").
    // Runtime ASCII art is loaded from its independently named default theme.
    let report =
        tundra_ascii_assets::check_required_assets(root, tundra_ascii_assets::DEFAULT_THEME_ID);
    report
        .checks
        .into_iter()
        .map(|check| {
            let status = match check.status {
                tundra_ascii_assets::AssetCheckStatus::Pass => DiagnosticStatus::Pass,
                tundra_ascii_assets::AssetCheckStatus::Warning => DiagnosticStatus::Warning,
            };
            DiagnosticCheck {
                id: stable_id("asset", &check.key),
                category: DiagnosticCategory::Assets,
                label: check.key,
                status,
                summary: check.message.clone(),
                detail: format!("{} — {}", check.path.display(), check.message),
                remediation: (status != DiagnosticStatus::Pass)
                    .then(|| "Reinstall the matching TundraUX asset package".to_string()),
                repair: None,
            }
        })
        .collect()
}

fn diagnostics_worker_loop(
    platform: Arc<dyn Platform>,
    storage: StorageManager,
    process: ProcessWatchdog,
    command_rx: mpsc::Receiver<WorkerCommand>,
    event_tx: mpsc::Sender<DiagnosticsTaskEvent>,
    restart_required: Arc<AtomicBool>,
) {
    while let Ok(command) = command_rx.recv() {
        match command {
            WorkerCommand::Scan => {
                let result = scan_diagnostics(platform.as_ref(), &storage, &process);
                let _ = event_tx.send(DiagnosticsTaskEvent::ScanCompleted(result));
            }
            WorkerCommand::Repair(actions) => {
                let total = actions.len();
                let mut task_restart_required = false;
                let mut results = Vec::with_capacity(total);
                for (index, action) in actions.into_iter().enumerate() {
                    let label = action.label();
                    let _ = event_tx.send(DiagnosticsTaskEvent::RepairProgress {
                        completed: index,
                        total,
                        label,
                    });
                    let result = execute_repair(platform.as_ref(), &storage, action);
                    task_restart_required |= result.success
                        && result.changed
                        && matches!(
                            &result.action,
                            DiagnosticsRepairAction::RepairStorageDocument(_)
                        );
                    results.push(result);
                }
                let snapshot = if task_restart_required {
                    None
                } else {
                    scan_diagnostics(platform.as_ref(), &storage, &process).ok()
                };
                if task_restart_required {
                    restart_required.store(true, Ordering::Release);
                }
                let _ = event_tx.send(DiagnosticsTaskEvent::RepairCompleted {
                    results,
                    snapshot,
                    restart_required: task_restart_required,
                });
            }
            WorkerCommand::Shutdown => break,
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
                    let check = tundra_platform::check_directory_read_write("Repaired path", path);
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

fn stable_id(prefix: &str, label: &str) -> String {
    let normalized = label
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string();
    format!("{prefix}.{normalized}")
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
mod tests {
    use super::*;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};
    use tundra_platform::{AppPaths, PlatformKind, UserDirs};
    use tundra_watchdog::{WatchdogConfig, WatchdogRuntime};

    #[test]
    fn overall_status_and_repair_plan_are_deterministic() {
        let snapshot = DiagnosticsSnapshot {
            scanned_at: Utc::now(),
            checks: vec![
                DiagnosticCheck {
                    id: "storage.state".to_string(),
                    category: DiagnosticCategory::Storage,
                    label: "State".to_string(),
                    status: DiagnosticStatus::Fail,
                    summary: "bad".to_string(),
                    detail: "bad".to_string(),
                    remediation: None,
                    repair: Some(DiagnosticsRepairAction::RepairStorageDocument(
                        StorageDocumentKind::State,
                    )),
                },
                DiagnosticCheck {
                    id: "path.data".to_string(),
                    category: DiagnosticCategory::Paths,
                    label: "Data".to_string(),
                    status: DiagnosticStatus::Warning,
                    summary: "missing".to_string(),
                    detail: "missing".to_string(),
                    remediation: None,
                    repair: Some(DiagnosticsRepairAction::CreateDirectory {
                        label: "Data".to_string(),
                        path: PathBuf::from("z-data"),
                    }),
                },
            ],
            incidents: Vec::new(),
            warnings: Vec::new(),
        };

        assert_eq!(snapshot.overall_status(), DiagnosticStatus::Fail);
        let plan = snapshot.repair_plan();
        assert!(matches!(
            plan.first(),
            Some(DiagnosticsRepairAction::CreateDirectory { .. })
        ));
        assert!(matches!(
            plan.last(),
            Some(DiagnosticsRepairAction::RepairStorageDocument(_))
        ));
    }

    #[test]
    fn asset_diagnostics_validate_the_runtime_theme_instead_of_the_ui_palette() {
        let checks = diagnostic_asset_checks(Path::new(tundra_ascii_assets::CANONICAL_ASSETS_DIR));
        let runtime_theme_path = Path::new("themes")
            .join(tundra_ascii_assets::DEFAULT_THEME_ID)
            .display()
            .to_string();

        assert_eq!(checks.len(), tundra_ascii_assets::required_assets().len());
        assert!(
            checks
                .iter()
                .all(|check| check.status == DiagnosticStatus::Pass)
        );
        assert!(
            checks
                .iter()
                .all(|check| check.detail.contains(&runtime_theme_path))
        );
    }

    #[test]
    fn terminal_event_keeps_runtime_busy_until_drain_consumes_it() {
        let (command_tx, command_rx) = mpsc::channel();
        let (event_tx, event_rx) = mpsc::channel();
        let runtime = DiagnosticsTaskRuntime {
            command_tx,
            event_rx,
            busy: Arc::new(AtomicBool::new(true)),
            restart_required: Arc::new(AtomicBool::new(false)),
            worker: None,
        };
        event_tx
            .send(DiagnosticsTaskEvent::ScanCompleted(Err(
                "expected test result".to_string(),
            )))
            .expect("terminal event should queue");

        assert!(runtime.is_busy());
        assert_eq!(runtime.request_scan(), Err(DiagnosticsTaskError::Busy));

        let events = runtime.drain_events();
        assert!(matches!(
            events.as_slice(),
            [DiagnosticsTaskEvent::ScanCompleted(Err(message))]
                if message == "expected test result"
        ));
        assert!(!runtime.is_busy());

        runtime
            .request_scan()
            .expect("a new scan should be accepted after consuming the terminal event");
        assert!(matches!(command_rx.try_recv(), Ok(WorkerCommand::Scan)));
    }

    #[test]
    fn managed_runtime_scans_and_latches_after_changed_storage_repair() {
        let root = std::env::temp_dir().join(format!(
            "tundra-diagnostics-runtime-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        let paths = AppPaths::from_parts(
            root.join("config/config.toml"),
            root.join("data"),
            root.join("cache"),
            root.join("logs"),
            root.join("temp"),
        )
        .expect("test paths");
        let layout = tundra_storage::StorageLayout::from_app_paths(&paths);
        let storage = StorageManager::open(paths.clone())
            .expect("test storage")
            .manager;
        let user_dirs = UserDirs::new(
            root.join("Desktop"),
            root.join("Documents"),
            root.join("Downloads"),
            root.join("Pictures"),
            root.join("Movies"),
            root.join("Music"),
            root.join("AppData"),
        )
        .expect("test user directories");
        let platform: Arc<dyn Platform> = Arc::new(
            tundra_platform::mock::MockPlatform::new(user_dirs, paths)
                .with_kind(PlatformKind::Macos),
        );
        let config = WatchdogConfig::new(
            root.join("watchdog/reports"),
            root.join("watchdog/fallback"),
            root.join("watchdog/data"),
            "diagnostics-test",
            "1.0.0",
        );
        let (watchdog_runtime, process) = WatchdogRuntime::start(config).expect("watchdog");
        let app = process
            .register_app(diagnostics_watchdog_descriptor())
            .expect("diagnostics app");
        let runtime = DiagnosticsTaskRuntime::new_managed(platform, storage, process, app)
            .expect("managed runtime");

        runtime.request_scan().expect("scan accepted");
        let mut result = None;
        for _ in 0..200 {
            if let Some(event) = runtime.drain_events().into_iter().next() {
                result = Some(event);
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        let DiagnosticsTaskEvent::ScanCompleted(Ok(snapshot)) = result.expect("scan event arrives")
        else {
            panic!("scan should complete successfully");
        };
        assert!(!snapshot.checks.is_empty());
        assert!(snapshot.checks.iter().any(|check| {
            check.category == DiagnosticCategory::Storage && check.status == DiagnosticStatus::Pass
        }));

        std::fs::remove_file(&layout.state_path).expect("state fixture should be removable");
        runtime
            .request_repair(vec![DiagnosticsRepairAction::RepairStorageDocument(
                StorageDocumentKind::State,
            )])
            .expect("storage repair accepted");
        for _ in 0..200 {
            if runtime.restart_required() {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }

        assert!(runtime.restart_required());
        assert!(runtime.is_busy());
        assert_eq!(
            runtime.request_scan(),
            Err(DiagnosticsTaskError::RestartRequired)
        );
        assert_eq!(
            runtime.request_repair(Vec::new()),
            Err(DiagnosticsTaskError::RestartRequired)
        );

        let mut completion_results = None;
        for _ in 0..200 {
            for event in runtime.drain_events() {
                if let DiagnosticsTaskEvent::RepairCompleted {
                    results,
                    restart_required: true,
                    ..
                } = event
                {
                    completion_results = Some(results);
                }
            }
            if completion_results.is_some() {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        let completion_results = completion_results.expect("repair completion should arrive");
        assert!(matches!(
            completion_results.as_slice(),
            [DiagnosticsRepairResult {
                action: DiagnosticsRepairAction::RepairStorageDocument(StorageDocumentKind::State),
                success: true,
                changed: true,
                ..
            }]
        ));
        assert!(!runtime.is_busy());
        assert!(runtime.restart_required());
        assert!(layout.state_path.is_file());
        assert_eq!(
            runtime.request_scan(),
            Err(DiagnosticsTaskError::RestartRequired)
        );

        drop(runtime);
        watchdog_runtime.shutdown().expect("watchdog shutdown");
        let _ = std::fs::remove_dir_all(root);
    }
}
