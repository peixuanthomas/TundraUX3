use std::collections::VecDeque;
use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use chrono::{DateTime, Utc};
use platform::{
    AppPaths, ProcessSpec, ProcessStatus, SupervisedChild, TerminalControlHandler, native_platform,
    pump_desktop_shutdown_events, spawn_supervised,
};
use serde::{Deserialize, Serialize};
use watchdog::{
    RecoveryComponentVersionsV1, RecoveryHandoffInputV1, RecoveryHandoffV1,
    RecoveryProcessFailureV1, WatchdogConfig,
};

use crate::{BUNDLE_PROTOCOL_VERSION, BundleError, BundleLayout};

pub const NORMAL_EXIT: i32 = 0;
pub const RESTART_EXIT: i32 = 74;
pub const RESET_EXIT: i32 = 75;
/// Returned by the patched kiosk fork when this process only activated the
/// already-running primary window and did not create a PTY child.
pub const ACTIVATED_EXISTING_EXIT: i32 = 76;
pub const BUNDLED_WEZTERM_REVISION: &str = "e378176fd3aa8204ace298157599b5a3b8496ca4";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionSpec {
    pub session_id: String,
    pub outcome_path: PathBuf,
    pub shutdown_path: PathBuf,
    /// Wall-clock start time used to exclude incidents from an earlier
    /// session when selecting the watchdog's recovery projection.
    pub started_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecoveryAction {
    pub incident_id: String,
    pub handoff_path: PathBuf,
    pub outcome_path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Exit(i32),
    Missing,
    Malformed(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChildStatus {
    pub code: Option<i32>,
    pub signal: Option<i32>,
}

impl From<ProcessStatus> for ChildStatus {
    fn from(status: ProcessStatus) -> Self {
        Self {
            code: status.code,
            signal: status.signal,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LauncherError {
    Bundle(BundleError),
    Io {
        operation: &'static str,
        path: Option<PathBuf>,
        message: String,
    },
    Platform(String),
    Reset(String),
    Child(String),
}

impl fmt::Display for LauncherError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Bundle(error) => error.fmt(formatter),
            Self::Io {
                operation,
                path,
                message,
            } => match path {
                Some(path) => write!(
                    formatter,
                    "{operation} failed for {}: {message}",
                    path.display()
                ),
                None => write!(formatter, "{operation} failed: {message}"),
            },
            Self::Platform(message) | Self::Reset(message) | Self::Child(message) => {
                formatter.write_str(message)
            }
        }
    }
}

impl std::error::Error for LauncherError {}

impl From<BundleError> for LauncherError {
    fn from(error: BundleError) -> Self {
        Self::Bundle(error)
    }
}

/// A waitable GUI process. It is intentionally small so supervisor behavior
/// can be tested without opening a real terminal window.
pub trait SessionChild: Send {
    fn wait(&mut self) -> Result<ChildStatus, LauncherError>;

    /// Test doubles and non-native children can retain their simple blocking
    /// wait. The platform child overrides this to observe host termination.
    fn wait_or_shutdown(&mut self, _shutdown: &AtomicBool) -> Result<SessionWait, LauncherError> {
        self.wait().map(SessionWait::Exited)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionWait {
    Exited(ChildStatus),
    HostShutdown,
}

pub trait ChildFactory: Send {
    fn launch_session(
        &mut self,
        spec: &SessionSpec,
    ) -> Result<Box<dyn SessionChild>, LauncherError>;
    fn launch_recovery(
        &mut self,
        action: &RecoveryAction,
    ) -> Result<Box<dyn SessionChild>, LauncherError>;
}

pub trait ResetCallback: Send {
    fn reset(&mut self) -> Result<(), LauncherError>;
}

#[derive(Default)]
pub struct NoopReset;

impl ResetCallback for NoopReset {
    fn reset(&mut self) -> Result<(), LauncherError> {
        Ok(())
    }
}

pub struct ProductStorageReset {
    paths: AppPaths,
}

impl ProductStorageReset {
    pub fn new(paths: AppPaths) -> Self {
        Self { paths }
    }
}

impl ResetCallback for ProductStorageReset {
    fn reset(&mut self) -> Result<(), LauncherError> {
        storage::reset_saved_content(&self.paths)
            .map(|_| ())
            .map_err(|error| LauncherError::Reset(error.to_string()))
    }
}

pub trait Clock: Send {
    fn now(&self) -> Duration;
    fn sleep(&self, duration: Duration);

    /// Returns false when host shutdown interrupted the backoff. The default
    /// preserves deterministic fake-clock behavior used by unit tests.
    fn sleep_or_shutdown(&self, duration: Duration, shutdown: &AtomicBool) -> bool {
        self.sleep(duration);
        !shutdown.load(Ordering::SeqCst)
    }
}

pub struct SystemClock {
    started: Instant,
}

impl Default for SystemClock {
    fn default() -> Self {
        Self {
            started: Instant::now(),
        }
    }
}

impl Clock for SystemClock {
    fn now(&self) -> Duration {
        self.started.elapsed()
    }

    fn sleep(&self, duration: Duration) {
        std::thread::sleep(duration);
    }

    fn sleep_or_shutdown(&self, duration: Duration, shutdown: &AtomicBool) -> bool {
        let deadline = Instant::now() + duration;
        while Instant::now() < deadline {
            pump_desktop_shutdown_events();
            if shutdown.load(Ordering::SeqCst) {
                return false;
            }
            std::thread::sleep(
                deadline
                    .saturating_duration_since(Instant::now())
                    .min(Duration::from_millis(25)),
            );
        }
        !shutdown.load(Ordering::SeqCst)
    }
}

#[derive(Debug, Clone)]
pub struct RecoveryPolicy {
    /// Number of automatic restarts permitted after the first failure.
    pub maximum_failures: usize,
    pub window: Duration,
    pub backoffs: Vec<Duration>,
    pub stable_after: Duration,
}

impl Default for RecoveryPolicy {
    fn default() -> Self {
        Self {
            maximum_failures: 3,
            window: Duration::from_secs(60),
            backoffs: vec![
                Duration::from_millis(500),
                Duration::from_secs(2),
                Duration::from_secs(5),
            ],
            stable_after: Duration::from_secs(60),
        }
    }
}

/// The top-level process watchdog. A result file is mandatory: a clean GUI
/// exit without a matching, atomically-written shell outcome is a failure.
pub struct LauncherSupervisor<F, C, R> {
    factory: F,
    clock: C,
    reset: R,
    policy: RecoveryPolicy,
    outcome_directory: PathBuf,
    diagnostics_directory: PathBuf,
    incident_source: Option<WatchdogConfig>,
    shutdown: Option<Arc<AtomicBool>>,
    terminal_control: Option<TerminalControlHandler>,
    next_session: u64,
}

impl<F, C, R> LauncherSupervisor<F, C, R>
where
    F: ChildFactory,
    C: Clock,
    R: ResetCallback,
{
    pub fn new(
        factory: F,
        clock: C,
        reset: R,
        outcome_directory: PathBuf,
        diagnostics_directory: PathBuf,
    ) -> Self {
        Self {
            factory,
            clock,
            reset,
            policy: RecoveryPolicy::default(),
            outcome_directory,
            diagnostics_directory,
            incident_source: None,
            shutdown: None,
            terminal_control: None,
            next_session: 1,
        }
    }

    pub fn with_policy(mut self, policy: RecoveryPolicy) -> Self {
        self.policy = policy;
        self
    }

    /// Uses the newest valid watchdog incident created during a failed
    /// session as the recovery UI's privacy-preserving source of truth.
    pub fn with_incident_source(mut self, config: WatchdogConfig) -> Self {
        self.incident_source = Some(config);
        self
    }

    /// Installs a caller-provided host shutdown source. This is also useful to
    /// deterministic tests and alternate desktop hosts.
    pub fn with_shutdown_flag(mut self, shutdown: Arc<AtomicBool>) -> Self {
        self.shutdown = Some(shutdown);
        self
    }

    /// Retains the platform signal/control handler for the full supervisor
    /// lifetime, so Windows logoff/shutdown and Unix TERM/HUP safely interrupt
    /// session waits and retry backoffs.
    pub fn with_terminal_control(mut self, control: TerminalControlHandler) -> Self {
        self.shutdown = Some(control.shutdown_flag());
        self.terminal_control = Some(control);
        self
    }

    /// Runs until the shell closes normally or this launch only activates an
    /// existing kiosk instance.
    pub fn run(&mut self) -> Result<(), LauncherError> {
        self.ensure_directories()?;
        report_stale_run_markers(&self.outcome_directory, &self.diagnostics_directory);
        let mut run_marker = RunMarkerGuard::create(&self.outcome_directory)?;
        let mut failures = VecDeque::new();
        let mut post_recovery_probation = false;

        loop {
            if self.shutdown_requested() {
                return Ok(());
            }
            let started = self.clock.now();
            let session = self.next_session();
            let decision = self.observe_session(&session);
            let elapsed = self.clock.now().saturating_sub(started);

            if elapsed >= self.policy.stable_after {
                failures.clear();
                post_recovery_probation = false;
            }

            match decision {
                SessionDecision::Exit
                | SessionDecision::ActivatedExisting
                | SessionDecision::HostShutdown => return Ok(()),
                SessionDecision::Restart => continue,
                SessionDecision::Reset => {
                    if self.shutdown_requested() {
                        return Ok(());
                    }
                    self.reset.reset()?;
                    self.ensure_directories()?;
                    run_marker.refresh()?;
                    continue;
                }
                SessionDecision::Failure(failure) => {
                    if post_recovery_probation {
                        if self.recover(&session, failure)? == RecoveryDecision::HostShutdown {
                            return Ok(());
                        }
                        continue;
                    }

                    let now = self.clock.now();
                    while failures
                        .front()
                        .is_some_and(|failure| now.saturating_sub(*failure) > self.policy.window)
                    {
                        failures.pop_front();
                    }
                    failures.push_back(now);

                    // The first failure plus three automatic restart attempts
                    // yields a panic page on the fourth consecutive failure.
                    if failures.len() > self.policy.maximum_failures {
                        if self.recover(&session, failure)? == RecoveryDecision::HostShutdown {
                            return Ok(());
                        }
                        failures.clear();
                        post_recovery_probation = true;
                        continue;
                    }

                    let backoff = self
                        .policy
                        .backoffs
                        .get(failures.len() - 1)
                        .copied()
                        .unwrap_or_default();
                    if !self.clock.sleep_or_shutdown(backoff, self.shutdown_flag()) {
                        return Ok(());
                    }
                }
            }
        }
    }

    fn ensure_directories(&self) -> Result<(), LauncherError> {
        fs::create_dir_all(&self.outcome_directory).map_err(|error| {
            io_error(
                "create session outcome directory",
                &self.outcome_directory,
                error,
            )
        })?;
        fs::create_dir_all(&self.diagnostics_directory).map_err(|error| {
            io_error(
                "create diagnostics directory",
                &self.diagnostics_directory,
                error,
            )
        })
    }

    fn observe_session(&mut self, session: &SessionSpec) -> SessionDecision {
        let mut child = match self.factory.launch_session(session) {
            Ok(child) => child,
            Err(error) => {
                return SessionDecision::Failure(SessionFailure::launcher_start(error.to_string()));
            }
        };
        let status = match child.wait_or_shutdown(self.shutdown_flag()) {
            Ok(SessionWait::Exited(status)) => status,
            Ok(SessionWait::HostShutdown) => return SessionDecision::HostShutdown,
            Err(error) => {
                return SessionDecision::Failure(SessionFailure::launcher_wait(error.to_string()));
            }
        };
        classify(status, read_outcome(session))
    }

    fn next_session(&mut self) -> SessionSpec {
        let id = format!("tundra-{}-{}", std::process::id(), self.next_session);
        self.next_session += 1;
        SessionSpec {
            outcome_path: self.outcome_directory.join(format!("{id}.outcome.json")),
            shutdown_path: self.outcome_directory.join(format!("{id}.shutdown")),
            session_id: id,
            started_at: Utc::now(),
        }
    }

    fn recover(
        &mut self,
        session: &SessionSpec,
        failure: SessionFailure,
    ) -> Result<RecoveryDecision, LauncherError> {
        if self.shutdown_requested() {
            return Ok(RecoveryDecision::HostShutdown);
        }
        self.ensure_directories()?;
        let incident_id = format!("panic-{}-{}", unix_seconds(), self.next_session);
        let occurred_at = Utc::now();
        let report_path = self
            .diagnostics_directory
            .join(format!("{incident_id}.log"));
        let report_available = fs::write(
            &report_path,
            format!(
                "incident_id={incident_id}\nsession_id={}\nutc={}\nsource={}\nexit_code={}\nsignal={}\nautomatic_restarts={}\ndetail={}\n",
                session.session_id,
                occurred_at.to_rfc3339(),
                failure.source,
                failure
                    .exit_code
                    .map_or_else(|| "unavailable".to_owned(), |code| code.to_string()),
                failure.signal.as_deref().unwrap_or("unavailable"),
                self.policy.maximum_failures,
                failure.private_detail,
            ),
        )
        .is_ok();

        let input = RecoveryHandoffInputV1 {
            incident_id: incident_id.clone(),
            session_id: session.session_id.clone(),
            occurred_at,
            failure: RecoveryProcessFailureV1::new(
                &failure.source,
                failure.exit_code,
                failure.signal.clone(),
            ),
            components: RecoveryComponentVersionsV1::new(
                env!("CARGO_PKG_VERSION"),
                env!("CARGO_PKG_VERSION"),
                &BUNDLED_WEZTERM_REVISION[..12],
            ),
            restart_count: self.policy.maximum_failures as u32,
            summary: failure.public_summary,
            traceback_frames: Vec::new(),
            report_available,
        };
        // Never pass our private outer-supervisor log to the recovery UI.
        // When a shell watchdog incident exists for this session it replaces
        // the provisional fields; otherwise the recovery surface receives a
        // deliberately generic missing-report handoff.
        let handoff = self
            .incident_source
            .as_ref()
            .and_then(|config| {
                RecoveryHandoffV1::from_latest_incident(config, session.started_at, input.clone())
            })
            .unwrap_or_else(|| {
                if report_available {
                    RecoveryHandoffV1::new(input)
                } else {
                    RecoveryHandoffV1::missing_report(input)
                }
            });
        let recovery_incident_id = handoff.incident_id.clone();
        let handoff_path = self
            .outcome_directory
            .join(format!("{recovery_incident_id}.handoff.json"));
        let recovery_outcome_path = self
            .outcome_directory
            .join(format!("{recovery_incident_id}.recovery-outcome.json"));
        // If this write fails, recovery still starts. Its bounded parser will
        // show the generic "details unavailable" capsule for the missing file.
        let _ = handoff.write_atomic(&handoff_path);

        if self.shutdown_requested() {
            let _ = fs::remove_file(handoff_path);
            return Ok(RecoveryDecision::HostShutdown);
        }

        let action = RecoveryAction {
            incident_id: recovery_incident_id,
            handoff_path: handoff_path.clone(),
            outcome_path: recovery_outcome_path.clone(),
        };
        let mut recovery = self.factory.launch_recovery(&action)?;
        let status = match recovery.wait_or_shutdown(self.shutdown_flag())? {
            SessionWait::Exited(status) => status,
            SessionWait::HostShutdown => {
                let _ = fs::remove_file(handoff_path);
                let _ = fs::remove_file(recovery_outcome_path);
                return Ok(RecoveryDecision::HostShutdown);
            }
        };
        let _ = fs::remove_file(handoff_path);
        let restart_requested = read_recovery_restart(&recovery_outcome_path, &action.incident_id);
        let _ = fs::remove_file(recovery_outcome_path);
        match (status.code, restart_requested) {
            // The kiosk GUI normally normalizes its foreground child's 74 to
            // zero after ExitBehavior::Close. Accept 74 for test/future forks
            // that explicitly propagate the child status.
            (Some(NORMAL_EXIT | RESTART_EXIT), true) if status.signal.is_none() => {
                Ok(RecoveryDecision::Restart)
            }
            _ => Err(LauncherError::Child(
                "TundraUX3 recovery screen closed without an Enter restart request".to_owned(),
            )),
        }
    }

    fn shutdown_requested(&self) -> bool {
        pump_desktop_shutdown_events();
        self.shutdown
            .as_ref()
            .is_some_and(|shutdown| shutdown.load(Ordering::SeqCst))
    }

    fn shutdown_flag(&self) -> &AtomicBool {
        static NEVER_SHUTDOWN: AtomicBool = AtomicBool::new(false);
        self.shutdown.as_deref().unwrap_or(&NEVER_SHUTDOWN)
    }
}

#[derive(Debug, PartialEq, Eq)]
enum SessionDecision {
    Exit,
    ActivatedExisting,
    HostShutdown,
    Restart,
    Reset,
    Failure(SessionFailure),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RecoveryDecision {
    Restart,
    HostShutdown,
}

#[derive(Debug, PartialEq, Eq)]
struct SessionFailure {
    source: String,
    exit_code: Option<i32>,
    signal: Option<String>,
    public_summary: String,
    private_detail: String,
}

impl SessionFailure {
    fn launcher_start(detail: String) -> Self {
        Self {
            source: "wezterm-gui startup".to_owned(),
            exit_code: None,
            signal: None,
            public_summary: "The bundled terminal could not be started".to_owned(),
            private_detail: detail,
        }
    }

    fn launcher_wait(detail: String) -> Self {
        Self {
            source: "wezterm-gui supervisor".to_owned(),
            exit_code: None,
            signal: None,
            public_summary: "The bundled terminal process could not be supervised".to_owned(),
            private_detail: detail,
        }
    }

    fn process(source: &str, status: ChildStatus, summary: String) -> Self {
        Self {
            source: source.to_owned(),
            exit_code: status.code,
            signal: status.signal.map(|signal| format!("signal {signal}")),
            public_summary: summary.clone(),
            private_detail: summary,
        }
    }
}

fn classify(status: ChildStatus, outcome: Outcome) -> SessionDecision {
    if status.code == Some(ACTIVATED_EXISTING_EXIT)
        && status.signal.is_none()
        && matches!(outcome, Outcome::Missing)
    {
        return SessionDecision::ActivatedExisting;
    }

    match outcome {
        Outcome::Exit(NORMAL_EXIT)
            if status.code == Some(NORMAL_EXIT) && status.signal.is_none() =>
        {
            SessionDecision::Exit
        }
        Outcome::Exit(RESTART_EXIT)
            if status.code == Some(NORMAL_EXIT) && status.signal.is_none() =>
        {
            SessionDecision::Restart
        }
        Outcome::Exit(RESET_EXIT)
            if status.code == Some(NORMAL_EXIT) && status.signal.is_none() =>
        {
            SessionDecision::Reset
        }
        Outcome::Exit(code) => SessionDecision::Failure(SessionFailure {
            source: "tundra-shell".to_owned(),
            exit_code: Some(code),
            signal: status.signal.map(|signal| format!("signal {signal}")),
            public_summary: format!("The managed Shell reported failure code {code}"),
            private_detail: format!(
                "managed shell outcome={code}; wezterm exit={:?}; signal={:?}",
                status.code, status.signal
            ),
        }),
        Outcome::Missing => SessionDecision::Failure(SessionFailure::process(
            "wezterm-gui/session",
            status,
            "The managed session ended without a result handoff".to_owned(),
        )),
        Outcome::Malformed(message) => SessionDecision::Failure(SessionFailure {
            source: "managed session protocol".to_owned(),
            exit_code: status.code,
            signal: status.signal.map(|signal| format!("signal {signal}")),
            public_summary: "The managed session returned an invalid result handoff".to_owned(),
            private_detail: message,
        }),
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SessionOutcomeV1 {
    schema_version: u32,
    session_id: String,
    origin: String,
    kind: String,
    code: i32,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RecoveryOutcomeV1 {
    schema_version: u32,
    incident_id: String,
    origin: String,
    kind: String,
    code: i32,
}

fn read_outcome(session: &SessionSpec) -> Outcome {
    const MAX_OUTCOME_BYTES: u64 = 4 * 1024;
    let metadata = match fs::metadata(&session.outcome_path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Outcome::Missing,
        Err(error) => return Outcome::Malformed(error.to_string()),
    };
    if metadata.len() > MAX_OUTCOME_BYTES {
        let _ = fs::remove_file(&session.outcome_path);
        return Outcome::Malformed("outcome exceeds protocol limit".to_owned());
    }
    let contents = match fs::read(&session.outcome_path) {
        Ok(contents) => contents,
        Err(error) => return Outcome::Malformed(error.to_string()),
    };
    let _ = fs::remove_file(&session.outcome_path);
    let value = match serde_json::from_slice::<SessionOutcomeV1>(&contents) {
        Ok(value) => value,
        Err(error) => return Outcome::Malformed(error.to_string()),
    };
    if value.schema_version != 1 {
        return Outcome::Malformed("schema mismatch".to_owned());
    }
    if value.session_id != session.session_id {
        return Outcome::Malformed("session mismatch".to_owned());
    }
    if value.origin != "shell" || value.kind != "exit" {
        return Outcome::Malformed("unexpected outcome origin or kind".to_owned());
    }
    Outcome::Exit(value.code)
}

fn read_recovery_restart(path: &Path, incident_id: &str) -> bool {
    const MAX_RECOVERY_OUTCOME_BYTES: u64 = 4 * 1024;
    let Ok(metadata) = fs::metadata(path) else {
        return false;
    };
    if metadata.len() > MAX_RECOVERY_OUTCOME_BYTES {
        return false;
    }
    let Ok(bytes) = fs::read(path) else {
        return false;
    };
    let Ok(value) = serde_json::from_slice::<RecoveryOutcomeV1>(&bytes) else {
        return false;
    };
    value.schema_version == 1
        && value.incident_id == incident_id
        && value.origin == "recovery"
        && value.kind == "restart"
        && value.code == RESTART_EXIT
}

fn io_error(operation: &'static str, path: &Path, error: std::io::Error) -> LauncherError {
    LauncherError::Io {
        operation,
        path: Some(path.to_path_buf()),
        message: error.to_string(),
    }
}

fn unix_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[derive(Debug, Serialize, Deserialize)]
struct RunMarkerV1 {
    schema_version: u32,
    pid: u32,
    started_utc_seconds: u64,
}

struct RunMarkerGuard {
    path: PathBuf,
}

impl RunMarkerGuard {
    fn create(directory: &Path) -> Result<Self, LauncherError> {
        let path = directory.join(format!("host-{}.run.json", std::process::id()));
        let mut guard = Self { path };
        guard.refresh()?;
        Ok(guard)
    }

    fn refresh(&mut self) -> Result<(), LauncherError> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| io_error("create launcher marker directory", parent, error))?;
        }
        let marker = RunMarkerV1 {
            schema_version: 1,
            pid: std::process::id(),
            started_utc_seconds: unix_seconds(),
        };
        let bytes = serde_json::to_vec(&marker).map_err(|error| LauncherError::Io {
            operation: "serialize launcher run marker",
            path: Some(self.path.clone()),
            message: error.to_string(),
        })?;
        let temporary = self.path.with_extension("run.json.tmp");
        let mut file = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(&temporary)
            .map_err(|error| io_error("create launcher run marker", &temporary, error))?;
        file.write_all(&bytes)
            .and_then(|_| file.sync_all())
            .map_err(|error| io_error("write launcher run marker", &temporary, error))?;
        drop(file);
        if self.path.exists() {
            fs::remove_file(&self.path)
                .map_err(|error| io_error("replace launcher run marker", &self.path, error))?;
        }
        fs::rename(&temporary, &self.path)
            .map_err(|error| io_error("publish launcher run marker", &self.path, error))
    }
}

impl Drop for RunMarkerGuard {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

fn report_stale_run_markers(directory: &Path, diagnostics: &Path) {
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };
    let platform = native_platform();
    for entry in entries.flatten() {
        let path = entry.path();
        let is_marker = path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with("host-") && name.ends_with(".run.json"));
        if !is_marker {
            continue;
        }
        let Ok(bytes) = fs::read(&path) else {
            continue;
        };
        let Ok(marker) = serde_json::from_slice::<RunMarkerV1>(&bytes) else {
            continue;
        };
        if marker.pid == std::process::id() || platform.is_process_alive(marker.pid).unwrap_or(true)
        {
            continue;
        }
        let incident = format!("unclean-launcher-{}-{}", marker.pid, unix_seconds());
        let _ = fs::write(
            diagnostics.join(format!("{incident}.log")),
            format!(
                "incident_id={incident}\nsource=tundra-launcher\nprevious_pid={}\nstarted_utc_seconds={}\nsummary=The previous launcher did not shut down cleanly\n",
                marker.pid, marker.started_utc_seconds
            ),
        );
        let _ = fs::remove_file(path);
    }
}

/// Production factory for the bundled kiosk WezTerm. Only installation-
/// relative absolute paths are used; no shell or PATH lookup is involved.
pub struct BundledWezTerm {
    layout: BundleLayout,
}

impl BundledWezTerm {
    pub fn new(layout: BundleLayout) -> Self {
        Self { layout }
    }

    fn base_command(&self, foreground: &Path) -> ProcessSpec {
        ProcessSpec::new(&self.layout.wezterm_gui)
            .current_dir(self.layout.runtime_root())
            .env(
                "WEZTERM_CONFIG_FILE",
                self.layout.wezterm_config.to_string_lossy(),
            )
            .env("TUNDRA_MANAGED_SESSION", "1")
            .env("TUNDRA_HOST_PROTOCOL", BUNDLE_PROTOCOL_VERSION)
            .env("TUNDRA_LAUNCHER_PID", std::process::id().to_string())
            .args([
                "start".to_owned(),
                "--".to_owned(),
                foreground.to_string_lossy().into_owned(),
            ])
    }

    fn command_for_session(&self, session: &SessionSpec) -> ProcessSpec {
        self.base_command(&self.layout.shell)
            .env("TUNDRA_SESSION_ID", &session.session_id)
            .env(
                "TUNDRA_SESSION_OUTCOME_PATH",
                session.outcome_path.to_string_lossy(),
            )
            .env(
                "TUNDRA_SESSION_SHUTDOWN_PATH",
                session.shutdown_path.to_string_lossy(),
            )
            .env(
                "TUNDRA_ASCII_ASSETS_DIR",
                self.layout.assets.to_string_lossy(),
            )
            .env("TUNDRA_CLI_PATH", self.layout.cli.to_string_lossy())
    }

    fn command_for_recovery(&self, action: &RecoveryAction) -> ProcessSpec {
        self.base_command(&self.layout.recovery)
            .arg(format!(
                "--handoff={}",
                action.handoff_path.to_string_lossy()
            ))
            .env(
                "TUNDRA_RECOVERY_HANDOFF_PATH",
                action.handoff_path.to_string_lossy(),
            )
            .env(
                "TUNDRA_RECOVERY_OUTCOME_PATH",
                action.outcome_path.to_string_lossy(),
            )
            .env("TUNDRA_RECOVERY_INCIDENT_ID", &action.incident_id)
    }
}

impl ChildFactory for BundledWezTerm {
    fn launch_session(
        &mut self,
        session: &SessionSpec,
    ) -> Result<Box<dyn SessionChild>, LauncherError> {
        self.layout.preflight()?;
        Ok(Box::new(PlatformSessionChild {
            child: spawn_supervised(&self.command_for_session(session), cfg!(windows))
                .map_err(|error| LauncherError::Platform(error.to_string()))?,
            shutdown_request: Some(session.shutdown_path.clone()),
        }))
    }

    fn launch_recovery(
        &mut self,
        action: &RecoveryAction,
    ) -> Result<Box<dyn SessionChild>, LauncherError> {
        self.layout.preflight()?;
        Ok(Box::new(PlatformSessionChild {
            child: spawn_supervised(&self.command_for_recovery(action), cfg!(windows))
                .map_err(|error| LauncherError::Platform(error.to_string()))?,
            shutdown_request: None,
        }))
    }
}

struct PlatformSessionChild {
    child: SupervisedChild,
    shutdown_request: Option<PathBuf>,
}

impl SessionChild for PlatformSessionChild {
    fn wait(&mut self) -> Result<ChildStatus, LauncherError> {
        self.child
            .wait()
            .map(ChildStatus::from)
            .map_err(|error| LauncherError::Platform(error.to_string()))
    }

    fn wait_or_shutdown(&mut self, shutdown: &AtomicBool) -> Result<SessionWait, LauncherError> {
        loop {
            pump_desktop_shutdown_events();
            if shutdown.load(Ordering::SeqCst) {
                if let Some(path) = self.shutdown_request.as_deref() {
                    // The process-tree signal below remains authoritative if
                    // storage is already unavailable during OS shutdown.
                    let _ = fs::write(path, b"shutdown\n");
                }
                self.child
                    .terminate_and_wait()
                    .map_err(|error| LauncherError::Platform(error.to_string()))?;
                return Ok(SessionWait::HostShutdown);
            }
            if let Some(status) = self
                .child
                .try_wait()
                .map_err(|error| LauncherError::Platform(error.to_string()))?
            {
                return Ok(SessionWait::Exited(ChildStatus::from(status)));
            }
            std::thread::sleep(Duration::from_millis(25));
        }
    }
}

impl Drop for PlatformSessionChild {
    fn drop(&mut self) {
        if let Some(path) = self.shutdown_request.as_deref() {
            let _ = fs::remove_file(path);
        }
    }
}

pub type ProductionSupervisor =
    LauncherSupervisor<BundledWezTerm, SystemClock, ProductStorageReset>;

/// Builds a production supervisor using app-owned state directories.
pub fn production_supervisor(layout: BundleLayout) -> Result<ProductionSupervisor, LauncherError> {
    layout.preflight()?;
    let paths = native_platform()
        .app_paths()
        .map_err(|error| LauncherError::Platform(error.to_string()))?;
    // Keep this source exactly aligned with the parent-managed shell's
    // watchdog report locations. The launcher reads incidents but does not
    // create an independent unclean-run marker.
    let fallback = std::env::temp_dir().join("TundraUX3").join("watchdog");
    let incident_source = WatchdogConfig::new(
        paths.logs_path().join("crashes"),
        fallback.join("crashes"),
        paths.data_path(),
        "tundra-shell",
        env!("CARGO_PKG_VERSION"),
    )
    .with_unclean_exit_tracking(false);
    Ok(LauncherSupervisor::new(
        BundledWezTerm::new(layout),
        SystemClock::default(),
        ProductStorageReset::new(paths.clone()),
        paths.data_path().join("launcher-outcomes"),
        paths.logs_path().join("launcher"),
    )
    .with_incident_source(incident_source)
    .with_terminal_control(TerminalControlHandler::install()))
}

pub fn show_critical_fallback(error: &LauncherError) {
    let incident_id = format!("startup-{}-{}", unix_seconds(), std::process::id());
    if let Ok(paths) = native_platform().app_paths() {
        let directory = paths.logs_path().join("launcher");
        let _ = fs::create_dir_all(&directory);
        let _ = fs::write(
            directory.join(format!("{incident_id}.log")),
            format!("incident_id={incident_id}\nsource=tundra-launcher-startup\ndetail={error}\n"),
        );
    }
    let _ = native_platform().show_critical_error(
        "TundraUX3 could not start",
        &format!(
            "The bundled terminal recovery screen could not be started.\n\nIncident ID: {incident_id}\nOpen Diagnostics > Logs for details."
        ),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::{Arc, Mutex};

    static SEQUENCE: AtomicU64 = AtomicU64::new(1);

    #[derive(Clone)]
    struct FakeClock {
        now: Arc<Mutex<Duration>>,
        sleeps: Arc<Mutex<Vec<Duration>>>,
    }

    impl Clock for FakeClock {
        fn now(&self) -> Duration {
            *self.now.lock().unwrap()
        }

        fn sleep(&self, duration: Duration) {
            self.sleeps.lock().unwrap().push(duration);
            *self.now.lock().unwrap() += duration;
        }
    }

    struct FakeChild(ChildStatus);

    impl SessionChild for FakeChild {
        fn wait(&mut self) -> Result<ChildStatus, LauncherError> {
            Ok(self.0)
        }
    }

    struct HostShutdownChild;

    impl SessionChild for HostShutdownChild {
        fn wait(&mut self) -> Result<ChildStatus, LauncherError> {
            unreachable!("the shutdown-aware path must be used")
        }

        fn wait_or_shutdown(
            &mut self,
            shutdown: &AtomicBool,
        ) -> Result<SessionWait, LauncherError> {
            shutdown.store(true, Ordering::SeqCst);
            Ok(SessionWait::HostShutdown)
        }
    }

    struct HostShutdownFactory;

    impl ChildFactory for HostShutdownFactory {
        fn launch_session(
            &mut self,
            _session: &SessionSpec,
        ) -> Result<Box<dyn SessionChild>, LauncherError> {
            Ok(Box::new(HostShutdownChild))
        }

        fn launch_recovery(
            &mut self,
            _action: &RecoveryAction,
        ) -> Result<Box<dyn SessionChild>, LauncherError> {
            panic!("host shutdown must not open recovery")
        }
    }

    #[derive(Clone)]
    struct FakeLaunch {
        status: ChildStatus,
        outcome: Option<i32>,
    }

    struct FakeFactory {
        launches: VecDeque<FakeLaunch>,
        recovery: VecDeque<ChildStatus>,
        sessions: Arc<Mutex<Vec<SessionSpec>>>,
        recovery_handoffs: Arc<Mutex<Vec<PathBuf>>>,
        recovery_payloads: Arc<Mutex<Vec<RecoveryHandoffV1>>>,
    }

    impl ChildFactory for FakeFactory {
        fn launch_session(
            &mut self,
            session: &SessionSpec,
        ) -> Result<Box<dyn SessionChild>, LauncherError> {
            self.sessions.lock().unwrap().push(session.clone());
            let launch = self.launches.pop_front().unwrap_or(FakeLaunch {
                status: clean_status(),
                outcome: Some(0),
            });
            if let Some(code) = launch.outcome {
                fs::write(
                    &session.outcome_path,
                    format!(
                        "{{\"schema_version\":1,\"session_id\":\"{}\",\"origin\":\"shell\",\"kind\":\"exit\",\"code\":{code}}}",
                        session.session_id
                    ),
                )
                .unwrap();
            }
            Ok(Box::new(FakeChild(launch.status)))
        }

        fn launch_recovery(
            &mut self,
            action: &RecoveryAction,
        ) -> Result<Box<dyn SessionChild>, LauncherError> {
            self.recovery_handoffs
                .lock()
                .unwrap()
                .push(action.handoff_path.clone());
            assert!(action.handoff_path.exists());
            self.recovery_payloads
                .lock()
                .unwrap()
                .push(RecoveryHandoffV1::read(&action.handoff_path).unwrap());
            fs::write(
                &action.outcome_path,
                format!(
                    "{{\"schema_version\":1,\"incident_id\":\"{}\",\"origin\":\"recovery\",\"kind\":\"restart\",\"code\":74}}",
                    action.incident_id
                ),
            )
            .unwrap();
            Ok(Box::new(FakeChild(
                self.recovery.pop_front().unwrap_or(clean_status()),
            )))
        }
    }

    #[derive(Default)]
    struct CountReset(usize);

    impl ResetCallback for CountReset {
        fn reset(&mut self) -> Result<(), LauncherError> {
            self.0 += 1;
            Ok(())
        }
    }

    fn clean_status() -> ChildStatus {
        ChildStatus {
            code: Some(0),
            signal: None,
        }
    }

    fn root() -> PathBuf {
        std::env::temp_dir().join(format!(
            "tundra-supervisor-{}-{}",
            std::process::id(),
            SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ))
    }

    fn factory(launches: Vec<FakeLaunch>) -> FakeFactory {
        FakeFactory {
            launches: launches.into(),
            recovery: VecDeque::new(),
            sessions: Arc::new(Mutex::new(Vec::new())),
            recovery_handoffs: Arc::new(Mutex::new(Vec::new())),
            recovery_payloads: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn clock() -> (FakeClock, Arc<Mutex<Vec<Duration>>>) {
        let sleeps = Arc::new(Mutex::new(Vec::new()));
        (
            FakeClock {
                now: Arc::new(Mutex::new(Duration::ZERO)),
                sleeps: sleeps.clone(),
            },
            sleeps,
        )
    }

    #[test]
    fn normal_outcome_exits_without_retry() {
        let root = root();
        let sessions = Arc::new(Mutex::new(Vec::new()));
        let mut f = factory(vec![FakeLaunch {
            status: clean_status(),
            outcome: Some(0),
        }]);
        f.sessions = sessions.clone();
        let (clock, _) = clock();
        let mut supervisor = LauncherSupervisor::new(
            f,
            clock,
            CountReset::default(),
            root.join("out"),
            root.join("logs"),
        );
        supervisor.run().unwrap();
        assert_eq!(sessions.lock().unwrap().len(), 1);
        assert!(
            !root
                .join(format!("out/host-{}.run.json", std::process::id()))
                .exists()
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn host_shutdown_ends_normally_without_recovery() {
        let root = root();
        let shutdown = Arc::new(AtomicBool::new(false));
        let (clock, sleeps) = clock();
        let mut supervisor = LauncherSupervisor::new(
            HostShutdownFactory,
            clock,
            CountReset::default(),
            root.join("out"),
            root.join("logs"),
        )
        .with_shutdown_flag(shutdown.clone());

        supervisor.run().unwrap();
        assert!(shutdown.load(Ordering::SeqCst));
        assert!(sleeps.lock().unwrap().is_empty());
        assert!(fs::read_dir(root.join("logs")).unwrap().next().is_none());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn restart_reset_and_activation_are_not_faults() {
        assert_eq!(
            classify(clean_status(), Outcome::Exit(RESTART_EXIT)),
            SessionDecision::Restart
        );
        assert_eq!(
            classify(clean_status(), Outcome::Exit(RESET_EXIT)),
            SessionDecision::Reset
        );
        assert_eq!(
            classify(
                ChildStatus {
                    code: Some(ACTIVATED_EXISTING_EXIT),
                    signal: None
                },
                Outcome::Missing
            ),
            SessionDecision::ActivatedExisting
        );
    }

    #[test]
    fn three_automatic_restarts_use_exact_backoffs_then_recovery_handoff() {
        let root = root();
        let recovery_handoffs = Arc::new(Mutex::new(Vec::new()));
        let recovery_payloads = Arc::new(Mutex::new(Vec::new()));
        let mut f = factory(vec![
            FakeLaunch {
                status: ChildStatus {
                    code: Some(1),
                    signal: None,
                },
                outcome: None,
            },
            FakeLaunch {
                status: ChildStatus {
                    code: Some(1),
                    signal: None,
                },
                outcome: None,
            },
            FakeLaunch {
                status: ChildStatus {
                    code: Some(1),
                    signal: None,
                },
                outcome: None,
            },
            FakeLaunch {
                status: ChildStatus {
                    code: Some(1),
                    signal: None,
                },
                outcome: None,
            },
            FakeLaunch {
                status: clean_status(),
                outcome: Some(0),
            },
        ]);
        f.recovery_handoffs = recovery_handoffs.clone();
        f.recovery_payloads = recovery_payloads.clone();
        let (clock, sleeps) = clock();
        let mut supervisor = LauncherSupervisor::new(
            f,
            clock,
            CountReset::default(),
            root.join("out"),
            root.join("logs"),
        );
        supervisor.run().unwrap();
        assert_eq!(
            *sleeps.lock().unwrap(),
            vec![
                Duration::from_millis(500),
                Duration::from_secs(2),
                Duration::from_secs(5)
            ]
        );
        assert_eq!(recovery_handoffs.lock().unwrap().len(), 1);
        let payloads = recovery_payloads.lock().unwrap();
        assert_eq!(payloads.len(), 1);
        assert!(payloads[0].report_available);
        assert_eq!(
            payloads[0].summary,
            "The managed session ended without a result handoff"
        );
        let log = fs::read_to_string(
            fs::read_dir(root.join("logs"))
                .unwrap()
                .next()
                .unwrap()
                .unwrap()
                .path(),
        )
        .unwrap();
        assert!(log.contains("automatic_restarts=3"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn a_failure_during_post_recovery_probation_returns_directly_to_panic() {
        let root = root();
        let recovery_handoffs = Arc::new(Mutex::new(Vec::new()));
        let failures = (0..5).map(|_| FakeLaunch {
            status: ChildStatus {
                code: Some(1),
                signal: None,
            },
            outcome: None,
        });
        let mut launches: Vec<_> = failures.collect();
        launches.push(FakeLaunch {
            status: clean_status(),
            outcome: Some(0),
        });
        let mut f = factory(launches);
        f.recovery_handoffs = recovery_handoffs.clone();
        f.recovery = [clean_status(), clean_status()].into();
        let (clock, _) = clock();
        let mut supervisor = LauncherSupervisor::new(
            f,
            clock,
            CountReset::default(),
            root.join("out"),
            root.join("logs"),
        );
        supervisor.run().unwrap();
        assert_eq!(recovery_handoffs.lock().unwrap().len(), 2);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn malformed_or_wrong_session_outcome_is_never_trusted() {
        let root = root();
        fs::create_dir_all(&root).unwrap();
        let session = SessionSpec {
            session_id: "safe".to_owned(),
            outcome_path: root.join("result"),
            shutdown_path: root.join("shutdown"),
            started_at: Utc::now(),
        };
        fs::write(
            &session.outcome_path,
            "{\"schema_version\":1,\"session_id\":\"other\",\"origin\":\"shell\",\"kind\":\"exit\",\"code\":0}",
        )
        .unwrap();
        assert!(matches!(read_outcome(&session), Outcome::Malformed(_)));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn recovery_gui_exit_without_enter_credential_is_not_a_restart() {
        let root = root();
        fs::create_dir_all(&root).unwrap();
        let path = root.join("restart.json");
        assert!(!read_recovery_restart(&path, "incident-1"));
        fs::write(
            &path,
            "{\"schema_version\":1,\"incident_id\":\"other\",\"origin\":\"recovery\",\"kind\":\"restart\",\"code\":74}",
        )
        .unwrap();
        assert!(!read_recovery_restart(&path, "incident-1"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn recovery_command_binds_the_expected_incident_out_of_band() {
        let root = root();
        fs::create_dir_all(root.join("runtime")).unwrap();
        let launcher_path = root.join(if cfg!(windows) {
            "tundra.exe"
        } else {
            "tundra"
        });
        fs::write(&launcher_path, b"").unwrap();
        let layout = BundleLayout::from_launcher_executable(&launcher_path).unwrap();
        let action = RecoveryAction {
            incident_id: "panic-session-42".to_owned(),
            handoff_path: root.join("handoff.json"),
            outcome_path: root.join("restart.json"),
        };

        let command = BundledWezTerm::new(layout).command_for_recovery(&action);
        assert_eq!(
            command
                .env_map()
                .get("TUNDRA_RECOVERY_INCIDENT_ID")
                .map(String::as_str),
            Some("panic-session-42")
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn session_command_is_bound_to_the_absolute_private_runtime() {
        let root = root();
        fs::create_dir_all(root.join("runtime/wezterm")).unwrap();
        let launcher_path = root.join(if cfg!(windows) {
            "tundra.exe"
        } else {
            "tundra"
        });
        fs::write(&launcher_path, b"").unwrap();
        let layout = BundleLayout::from_launcher_executable(&launcher_path).unwrap();
        let session = SessionSpec {
            session_id: "session-private-runtime".to_owned(),
            outcome_path: root.join("outcome.json"),
            shutdown_path: root.join("shutdown.request"),
            started_at: Utc::now(),
        };

        let command = BundledWezTerm::new(layout.clone()).command_for_session(&session);
        assert_eq!(command.program(), layout.wezterm_gui);
        assert!(command.program().is_absolute());
        assert_eq!(command.current_dir_path(), Some(layout.runtime_root()));
        assert_eq!(
            command.args_slice(),
            [
                "start".to_owned(),
                "--".to_owned(),
                layout.shell.to_string_lossy().into_owned(),
            ]
        );
        assert!(layout.shell.is_absolute());
        assert_eq!(
            command.env_map().get("WEZTERM_CONFIG_FILE"),
            Some(&layout.wezterm_config.to_string_lossy().into_owned())
        );
        assert_eq!(
            command.env_map().get("TUNDRA_ASCII_ASSETS_DIR"),
            Some(&layout.assets.to_string_lossy().into_owned())
        );
        assert_eq!(
            command
                .env_map()
                .get("TUNDRA_SESSION_ID")
                .map(String::as_str),
            Some(session.session_id.as_str())
        );
        assert_eq!(
            command
                .env_map()
                .get("TUNDRA_SESSION_OUTCOME_PATH")
                .map(String::as_str),
            Some(session.outcome_path.to_string_lossy().as_ref())
        );
        let _ = fs::remove_dir_all(root);
    }
}
