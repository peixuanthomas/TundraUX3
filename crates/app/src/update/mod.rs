use platform::ManagedCommand;
use std::fmt;
use std::fs::{self, File};
use std::io::{self, Cursor, Read};
use std::path::{Component, Path, PathBuf};
use std::time::Duration;

use http::{Client, Response};
use platform::{Platform, PlatformKind, ProcessExit, ProcessSpec};
mod cancellation;
mod http;
#[cfg(any(windows, target_os = "linux"))]
use cancellation::copy_file;
pub use cancellation::{
    UpdateCancellation, cleanup_update_work, discard_staged_update, mark_staged_update_cancelled,
};
use cancellation::{WorkDirectory, copy_stream};
use semver::Version;
use serde::{Deserialize, Serialize};

pub const UPDATE_PROTOCOL_VERSION: u32 = 3;
pub const GITHUB_OWNER: &str = "peixuanthomas";
pub const GITHUB_REPO: &str = "TundraUX3";
pub const UPDATE_READY_FILE_ENV: &str = "TUNDRAUX3_UPDATE_READY_FILE";
pub const UPDATE_TARGET_SHA_ENV: &str = "TUNDRAUX3_UPDATE_TARGET_SHA";
pub const UPDATE_ROLLBACK_ENV: &str = "TUNDRAUX3_UPDATE_ROLLBACK";
const API_ROOT: &str = "https://api.github.com";
const USER_AGENT: &str = "TundraUX3-updater/1";
mod git;
#[cfg(target_os = "linux")]
mod release;
mod toolchain;
mod vim;
pub use vim::install::{
    VimInstallPhase, VimInstallProgress, install_vim_engine, installed_vim_directory,
};
const SHELL_FILE: &str = if cfg!(windows) {
    "tundra-shell.exe"
} else {
    "tundra-shell"
};
const CLI_FILE: &str = if cfg!(windows) {
    "tundra-cli.exe"
} else {
    "tundra-cli"
};
const HELPER_FILE: &str = if cfg!(windows) {
    "update-helper.exe"
} else {
    "update-helper"
};

pub fn supports_updates(kind: PlatformKind) -> bool {
    matches!(kind, PlatformKind::Windows | PlatformKind::Linux)
}

fn require_current_portable_installation() -> Result<(), UpdateError> {
    #[cfg(target_os = "linux")]
    {
        let installation = platform::installation::current_installation();
        if installation.backend != platform::installation::UpdateBackend::PortableUser {
            return Err(UpdateError::new(installation.reason.unwrap_or_else(|| {
                "Updates require a writable, user-owned portable installation".into()
            })));
        }
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildIdentity {
    pub package_version: String,
    pub commit_sha: Option<String>,
    pub dirty: bool,
}

pub fn current_build_identity() -> BuildIdentity {
    let commit = env!("TUNDRAUX3_BUILD_COMMIT");
    BuildIdentity {
        package_version: env!("CARGO_PKG_VERSION").to_owned(),
        commit_sha: (commit != "unknown").then(|| commit.to_owned()),
        dirty: env!("TUNDRAUX3_BUILD_DIRTY") == "true",
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateRelation {
    Identical,
    Behind { remote_ahead: u64 },
    Ahead { local_ahead: u64 },
    Diverged { remote_ahead: u64, local_ahead: u64 },
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateCommit {
    pub sha: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateCheckResult {
    pub release: Option<ReleaseUpdate>,
    pub default_branch: String,
    pub head_sha: String,
    pub relation: UpdateRelation,
    pub commits: Vec<UpdateCommit>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseUpdate {
    pub version: String,
    pub archive_name: String,
    pub archive_url: String,
    pub checksum_url: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdatePhase {
    Checking,
    Downloading,
    CheckingToolchain,
    Compiling,
    Staging,
    PreparingReplacement,
    WaitingForRestart,
    Cancelling,
    Cancelled,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateProgress {
    pub phase: UpdatePhase,
    pub message: String,
    pub detail: UpdateProgressDetail,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateProgressDetail {
    Status,
    Download {
        received: u64,
        total: Option<u64>,
        finished: bool,
    },
    Compilation {
        completed: u64,
        total: Option<u64>,
        finished: bool,
    },
    Output,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedUpdate {
    pub work_dir: PathBuf,
    pub target_sha: String,
    pub shell_exe: PathBuf,
    pub cli_exe: PathBuf,
}

#[derive(Debug)]
pub struct UpdateError {
    message: String,
    http_status: Option<u16>,
    cancelled: bool,
    retained_staging: Option<PathBuf>,
}

impl UpdateError {
    pub fn with_cleanup_failure(self, cleanup: Self) -> Self {
        let mut combined = Self::new(format!("{self}; {cleanup}"));
        combined.retained_staging = self.retained_staging.or(cleanup.retained_staging);
        combined
    }
    /// This manifest could not be safely marked cancelled. Keep it registered
    /// and retry cancellation before allowing a new update or session exit.
    pub fn retained_staging(&self) -> Option<&Path> {
        self.retained_staging.as_deref()
    }
    fn with_retained_staging(mut self, path: &Path) -> Self {
        self.retained_staging = Some(path.to_owned());
        self
    }
    pub fn is_cancelled(&self) -> bool {
        self.cancelled
    }
    pub fn cancelled() -> Self {
        Self {
            message: "Update cancelled".into(),
            http_status: None,
            cancelled: true,
            retained_staging: None,
        }
    }
    fn context(mut self, context: &str) -> Self {
        self.message = format!("{context}: {}", self.message);
        self
    }

    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            http_status: None,
            cancelled: false,
            retained_staging: None,
        }
    }

    fn http(status: u16, message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            http_status: Some(status),
            cancelled: false,
            retained_staging: None,
        }
    }
}
impl fmt::Display for UpdateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}
impl std::error::Error for UpdateError {}
impl From<io::Error> for UpdateError {
    fn from(value: io::Error) -> Self {
        if value
            .get_ref()
            .and_then(|error| error.downcast_ref::<UpdateError>())
            .is_some_and(UpdateError::is_cancelled)
        {
            Self::cancelled()
        } else {
            Self::new(value.to_string())
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StagedUpdate {
    pub manifest_path: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum TransactionState {
    Prepared,
    Applying,
    AwaitingReady,
    RollingBack,
    Committed,
    RolledBack,
    Cancelled,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct TransactionManifest {
    protocol: u32,
    target_sha: String,
    install_dir: PathBuf,
    transaction_dir: PathBuf,
    state: TransactionState,
    cli_replaced: bool,
    shell_replaced: bool,
    #[serde(default)]
    vim: Option<VimTransaction>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct VimTransaction {
    had_previous: bool,
    replaced: bool,
}

#[derive(Debug, Deserialize)]
struct Repository {
    default_branch: String,
}
#[derive(Deserialize)]
struct Branch {
    commit: ApiCommitRef,
}
#[derive(Deserialize)]
struct ApiCommitRef {
    sha: String,
}
#[derive(Deserialize)]
struct ApiCommit {
    sha: String,
    commit: CommitDetails,
}
#[derive(Deserialize)]
struct CommitDetails {
    message: String,
}
#[derive(Deserialize)]
struct Compare {
    status: String,
    ahead_by: u64,
    behind_by: u64,
    commits: Vec<ApiCommit>,
}

pub fn check_for_updates(identity: &BuildIdentity) -> Result<UpdateCheckResult, UpdateError> {
    check_for_updates_in_mode(identity, storage::LinuxUpdateMode::default())
}

pub fn check_for_updates_in_mode(
    identity: &BuildIdentity,
    mode: storage::LinuxUpdateMode,
) -> Result<UpdateCheckResult, UpdateError> {
    check_for_updates_in_mode_cancellable(identity, mode, &UpdateCancellation::default())
}

pub fn check_for_updates_in_mode_cancellable(
    identity: &BuildIdentity,
    mode: storage::LinuxUpdateMode,
    cancel: &UpdateCancellation,
) -> Result<UpdateCheckResult, UpdateError> {
    cancel.check()?;
    require_current_portable_installation()?;
    #[cfg(target_os = "linux")]
    if mode == storage::LinuxUpdateMode::Release {
        return release::check(identity, API_ROOT, cancel);
    }
    let _ = mode;
    check_with_fallback(identity, API_ROOT, || git::check(identity, cancel), cancel)
}

fn check_with_fallback(
    identity: &BuildIdentity,
    api_root: &str,
    fallback: impl FnOnce() -> Result<UpdateCheckResult, UpdateError>,
    cancel: &UpdateCancellation,
) -> Result<UpdateCheckResult, UpdateError> {
    check_using_api(identity, api_root, cancel).or_else(|api_error| {
        cancel.check()?;
        if api_error.is_cancelled() {
            return Err(api_error);
        }
        fallback()
            .map_err(|git_error| git_error.context(&format!("{api_error}; Git fallback failed")))
    })
}

fn check_using_api(
    identity: &BuildIdentity,
    api_root: &str,
    cancel: &UpdateCancellation,
) -> Result<UpdateCheckResult, UpdateError> {
    let client = Client::new(Duration::from_secs(20), cancel)?;
    #[cfg(not(target_os = "linux"))]
    let repository: Repository = get_json(
        &client,
        &format!("{api_root}/repos/{GITHUB_OWNER}/{GITHUB_REPO}"),
    )?;
    #[cfg(target_os = "linux")]
    let repository = Repository {
        default_branch: "master".into(),
    };
    let branch: Branch = get_json(
        &client,
        &format!(
            "{api_root}/repos/{GITHUB_OWNER}/{GITHUB_REPO}/branches/{}",
            repository.default_branch
        ),
    )?;
    let (relation, commits) = if let Some(local) = identity.commit_sha.as_deref() {
        match fetch_comparison_from(&client, api_root, local, &branch.commit.sha) {
            Ok(result) => result,
            Err(error) if error.http_status == Some(404) => (
                UpdateRelation::Unknown,
                fetch_recent_commits(&client, api_root, &branch.commit.sha)?,
            ),
            Err(error) => return Err(error),
        }
    } else {
        (
            UpdateRelation::Unknown,
            fetch_recent_commits(&client, api_root, &branch.commit.sha)?,
        )
    };
    Ok(UpdateCheckResult {
        release: None,
        default_branch: repository.default_branch,
        head_sha: branch.commit.sha,
        relation,
        commits,
    })
}

fn fetch_recent_commits(
    client: &Client,
    api_root: &str,
    head: &str,
) -> Result<Vec<UpdateCommit>, UpdateError> {
    let values: Vec<ApiCommit> = get_json(
        client,
        &format!("{api_root}/repos/{GITHUB_OWNER}/{GITHUB_REPO}/commits?sha={head}&per_page=20"),
    )?;
    Ok(map_commits(values))
}

fn fetch_comparison_from(
    client: &Client,
    api_root: &str,
    base: &str,
    head: &str,
) -> Result<(UpdateRelation, Vec<UpdateCommit>), UpdateError> {
    let mut page = 1;
    let mut all = Vec::new();
    let mut relation = None;
    loop {
        let url = format!(
            "{api_root}/repos/{GITHUB_OWNER}/{GITHUB_REPO}/compare/{base}...{head}?per_page=100&page={page}"
        );
        let response: Compare = get_json(client, &url)?;
        if relation.is_none() {
            relation = Some(relation_from_compare(
                &response.status,
                response.ahead_by,
                response.behind_by,
            ));
        }
        let count = response.commits.len();
        all.extend(map_commits(response.commits));
        if count < 100 {
            break;
        }
        page += 1;
    }
    Ok((relation.unwrap_or(UpdateRelation::Unknown), all))
}

fn relation_from_compare(status: &str, ahead: u64, behind: u64) -> UpdateRelation {
    match status {
        "identical" => UpdateRelation::Identical,
        "ahead" => UpdateRelation::Behind {
            remote_ahead: ahead,
        },
        "behind" => UpdateRelation::Ahead {
            local_ahead: behind,
        },
        "diverged" => UpdateRelation::Diverged {
            remote_ahead: ahead,
            local_ahead: behind,
        },
        _ => UpdateRelation::Unknown,
    }
}

fn map_commits(values: Vec<ApiCommit>) -> Vec<UpdateCommit> {
    values
        .into_iter()
        .map(|v| UpdateCommit {
            sha: v.sha,
            message: v.commit.message,
        })
        .collect()
}

fn get_json<T: serde::de::DeserializeOwned>(client: &Client, url: &str) -> Result<T, UpdateError> {
    checked(
        client
            .get(url)
            .send()
            .map_err(|e| e.context("GitHub request failed"))?,
    )?
    .json()
    .map_err(|e| e.context("GitHub returned invalid JSON"))
}

fn checked(response: Response) -> Result<Response, UpdateError> {
    if response.status().is_success() {
        return Ok(response);
    }
    let status = response.status();
    let limited = status.as_u16() == 403
        && response
            .headers()
            .get("x-ratelimit-remaining")
            .is_some_and(|v| v == "0");
    let body = response.text()?;
    if limited {
        Err(UpdateError::http(
            status.as_u16(),
            "GitHub API rate limit exceeded; try again after the limit resets",
        ))
    } else {
        Err(UpdateError::http(
            status.as_u16(),
            format!("GitHub returned HTTP {status}: {}", tail(&body, 512)),
        ))
    }
}

fn source_archive_url(sha: &str) -> Result<String, UpdateError> {
    if !git::is_commit_sha(sha) {
        return Err(UpdateError::new("invalid source commit SHA"));
    }
    Ok(format!(
        "https://codeload.github.com/{GITHUB_OWNER}/{GITHUB_REPO}/zip/{sha}"
    ))
}

pub fn prepare_update(
    platform: &dyn Platform,
    check: &UpdateCheckResult,
    progress: &mut dyn FnMut(UpdateProgress),
) -> Result<PreparedUpdate, UpdateError> {
    prepare_update_cancellable(platform, check, progress, &UpdateCancellation::default())
}

pub fn prepare_update_cancellable(
    platform: &dyn Platform,
    check: &UpdateCheckResult,
    progress: &mut dyn FnMut(UpdateProgress),
    cancel: &UpdateCancellation,
) -> Result<PreparedUpdate, UpdateError> {
    cancel.check()?;
    require_current_portable_installation()?;
    if !supports_updates(platform.kind()) {
        return Err(UpdateError::new(
            "automatic updates are supported only on Windows and Linux",
        ));
    }
    #[cfg(target_os = "linux")]
    if check.release.is_some() {
        return release::prepare(platform, check, progress, cancel);
    }
    prepare_portable_update(platform, check, progress, cancel)
}

// Called only after installation provenance is checked at the public boundary.
fn prepare_portable_update(
    platform: &dyn Platform,
    check: &UpdateCheckResult,
    progress: &mut dyn FnMut(UpdateProgress),
    cancel: &UpdateCancellation,
) -> Result<PreparedUpdate, UpdateError> {
    cancel.check()?;
    notify(
        progress,
        UpdatePhase::Downloading,
        "Downloading source archive",
    );
    // Linux runtime directories are small tmpfs mounts. A release build needs
    // disk-backed space for source, dependencies and compiler artifacts.
    let cache = platform
        .app_paths()
        .map_err(|error| UpdateError::new(format!("could not resolve update cache: {error}")))?;
    let work_dir = platform::create_temp_dir(&cache.cache_path().join("updates"), "update")
        .map_err(|e| UpdateError::new(format!("could not create private update directory: {e}")))?;
    let mut owned = WorkDirectory::new(work_dir.clone());
    let result = prepare_in(platform, check, progress, &work_dir, cancel);
    owned.finish(result)
}

fn prepare_in(
    platform: &dyn Platform,
    check: &UpdateCheckResult,
    progress: &mut dyn FnMut(UpdateProgress),
    work_dir: &Path,
    cancel: &UpdateCancellation,
) -> Result<PreparedUpdate, UpdateError> {
    cancel.check()?;
    let client = Client::new(Duration::from_secs(120), cancel)?;
    let url = source_archive_url(&check.head_sha)?;
    let response = checked(
        client
            .get(url)
            .send()
            .map_err(|e| e.context("source download failed"))?,
    )?;
    let total = response.content_length();
    let bytes = download_source(response, total, progress, cancel)?;
    let source_root = extract_archive(bytes.as_ref(), &work_dir.join("source"), cancel)?;
    prepare_extracted(platform, check, progress, work_dir, &source_root, cancel)
}

trait PreparationOperations {
    fn run(&self, spec: &ProcessSpec, name: &str) -> Result<ProcessExit, UpdateError>;
    fn probe(&self, executable: &Path, expected_sha: &str) -> Result<(), UpdateError>;
    fn tool(&self, name: &str) -> ProcessSpec {
        ProcessSpec::new(name)
    }
    fn run_live(
        &self,
        spec: &ProcessSpec,
        name: &str,
        _progress: &mut dyn FnMut(UpdateProgress),
    ) -> Result<ProcessExit, UpdateError> {
        self.run(spec, name)
    }
}

struct PlatformPreparationOperations<'a> {
    platform: &'a dyn Platform,
    toolchain: toolchain::Toolchain,
    cancel: &'a UpdateCancellation,
}

impl PreparationOperations for PlatformPreparationOperations<'_> {
    fn run(&self, spec: &ProcessSpec, name: &str) -> Result<ProcessExit, UpdateError> {
        run_checked(self.platform, spec.clone(), name, self.cancel)
    }

    fn probe(&self, executable: &Path, expected_sha: &str) -> Result<(), UpdateError> {
        validate_update_probe(executable, expected_sha, self.cancel)
    }

    fn tool(&self, name: &str) -> ProcessSpec {
        self.toolchain.spec(name)
    }

    fn run_live(
        &self,
        spec: &ProcessSpec,
        name: &str,
        progress: &mut dyn FnMut(UpdateProgress),
    ) -> Result<ProcessExit, UpdateError> {
        let exit = self
            .platform
            .spawn_streaming_cancellable(spec, &|| self.cancel.is_cancelled(), &mut |output| {
                let line = clean_output(&output.text);
                if let Some((completed, total)) = cargo_progress(&line) {
                    progress(UpdateProgress {
                        phase: UpdatePhase::Compiling,
                        message: format!("Compiling: {completed}/{total} units"),
                        detail: UpdateProgressDetail::Compilation {
                            completed,
                            total: Some(total),
                            finished: false,
                        },
                    });
                } else if !line.trim().is_empty() {
                    progress(UpdateProgress {
                        phase: UpdatePhase::Compiling,
                        message: line,
                        detail: UpdateProgressDetail::Output,
                    });
                }
            })
            .map_err(|error| process_error(error, name, self.cancel))?;
        checked_exit(exit, name)
    }
}

fn prepare_extracted(
    platform: &dyn Platform,
    check: &UpdateCheckResult,
    progress: &mut dyn FnMut(UpdateProgress),
    work_dir: &Path,
    source_root: &Path,
    cancel: &UpdateCancellation,
) -> Result<PreparedUpdate, UpdateError> {
    cancel.check()?;
    prepare_extracted_with_operations(
        check,
        progress,
        work_dir,
        source_root,
        &PlatformPreparationOperations {
            platform,
            cancel,
            toolchain: toolchain::Toolchain::discover()?,
        },
    )
}

fn prepare_extracted_with_operations(
    check: &UpdateCheckResult,
    progress: &mut dyn FnMut(UpdateProgress),
    work_dir: &Path,
    source_root: &Path,
    operations: &dyn PreparationOperations,
) -> Result<PreparedUpdate, UpdateError> {
    notify(
        progress,
        UpdatePhase::CheckingToolchain,
        "Checking Rust toolchain",
    );
    let required = required_rust_version(&source_root.join("Cargo.toml"))?;
    let rustc_spec = operations.tool("rustc").arg("-Vv");
    let cargo_spec = operations.tool("cargo");
    notify_output(
        progress,
        UpdatePhase::CheckingToolchain,
        &format!("Rust compiler: {}", rustc_spec.program().display()),
    );
    notify_output(
        progress,
        UpdatePhase::CheckingToolchain,
        &format!("Cargo: {}", cargo_spec.program().display()),
    );
    let rustc = operations.run(&rustc_spec, "rustc")?;
    if let Some(required) = required {
        let installed = parse_rustc_version(&rustc.stdout.utf8_lossy())?;
        if installed < required {
            return Err(UpdateError::new(format!(
                "rustc {installed} is too old; source requires {required}"
            )));
        }
    }
    operations.run(&cargo_spec.clone().arg("-V"), "cargo")?;
    notify(
        progress,
        UpdatePhase::Compiling,
        "Compiling release executables",
    );
    let target = work_dir.join("target");
    let build = cargo_spec
        .args([
            "build",
            "--release",
            "--locked",
            "-p",
            "shell",
            "-p",
            "cli",
            "--target-dir",
        ])
        .arg(target.to_string_lossy())
        .current_dir(&source_root)
        .env("TUNDRAUX3_BUILD_COMMIT", &check.head_sha)
        .env("CARGO_TERM_COLOR", "never")
        .env("CARGO_TERM_PROGRESS_WHEN", "always")
        .env("CARGO_TERM_PROGRESS_WIDTH", "80")
        .env("CARGO_TERM_PROGRESS_TERM_INTEGRATION", "false");
    progress(UpdateProgress {
        phase: UpdatePhase::Compiling,
        message: "Compiling release executables".into(),
        detail: UpdateProgressDetail::Compilation {
            completed: 0,
            total: None,
            finished: false,
        },
    });
    operations.run_live(&build, "cargo build", progress)?;
    notify(
        progress,
        UpdatePhase::Compiling,
        "Building bundled Vim runtime",
    );
    let build_vim = ProcessSpec::new(if cfg!(windows) { "python" } else { "python3" })
        .arg(source_root.join("scripts/build-vim.py").to_string_lossy())
        .arg("--output")
        .arg(target.join("release/vim").to_string_lossy())
        .current_dir(source_root);
    operations.run_live(&build_vim, "Vim build", progress)?;
    progress(UpdateProgress {
        phase: UpdatePhase::Compiling,
        message: "Compilation complete".into(),
        detail: UpdateProgressDetail::Compilation {
            completed: 1,
            total: Some(1),
            finished: true,
        },
    });
    notify(progress, UpdatePhase::Staging, "Validating compiled files");
    validate_products_with_operations(work_dir, &target, &check.head_sha, operations)
}

fn download_source(
    mut reader: impl Read,
    total: Option<u64>,
    progress: &mut dyn FnMut(UpdateProgress),
    cancel: &UpdateCancellation,
) -> Result<Vec<u8>, UpdateError> {
    let mut bytes = Vec::new();
    let mut buffer = [0u8; 65536];
    let mut last = std::time::Instant::now();
    loop {
        cancel.check()?;
        let count = reader
            .read(&mut buffer)
            .map_err(|error| UpdateError::from(error).context("source download failed"))?;
        bytes.extend_from_slice(&buffer[..count]);
        if count == 0 || last.elapsed() >= Duration::from_millis(100) {
            let received = bytes.len() as u64;
            if count == 0 && total.is_some_and(|total| total != received) {
                return Err(UpdateError::new(
                    "source download ended before the expected content length",
                ));
            }
            progress(UpdateProgress {
                phase: UpdatePhase::Downloading,
                message: if count == 0 {
                    "Download complete".into()
                } else {
                    format!("Downloading source: {received} bytes")
                },
                detail: UpdateProgressDetail::Download {
                    received,
                    total,
                    finished: count == 0,
                },
            });
            last = std::time::Instant::now();
        }
        if count == 0 {
            return Ok(bytes);
        }
    }
}

fn clean_output(text: &str) -> String {
    let mut chars = text.chars().peekable();
    let mut output = String::new();
    while let Some(ch) = chars.next() {
        if ch == '\u{1b}' && chars.peek() == Some(&'[') {
            chars.next();
            for ch in chars.by_ref() {
                if ('@'..='~').contains(&ch) {
                    break;
                }
            }
        } else if !ch.is_control() || ch == '\t' {
            output.push(ch);
        }
    }
    output
}

fn cargo_progress(line: &str) -> Option<(u64, u64)> {
    let line = line.trim().strip_prefix("Building")?;
    let count = line
        .split_once(']')?
        .1
        .split_whitespace()
        .next()?
        .trim_end_matches(':');
    let (completed, total) = count.split_once('/')?;
    let completed = completed.parse().ok()?;
    let total = total.parse().ok()?;
    (total > 0 && completed <= total).then_some((completed, total))
}

fn notify_output(progress: &mut dyn FnMut(UpdateProgress), phase: UpdatePhase, message: &str) {
    progress(UpdateProgress {
        phase,
        message: message.to_owned(),
        detail: UpdateProgressDetail::Output,
    });
}

fn notify(progress: &mut dyn FnMut(UpdateProgress), phase: UpdatePhase, message: &str) {
    progress(UpdateProgress {
        phase,
        message: message.to_owned(),
        detail: UpdateProgressDetail::Status,
    });
}

fn run_checked(
    platform: &dyn Platform,
    spec: ProcessSpec,
    name: &str,
    cancel: &UpdateCancellation,
) -> Result<ProcessExit, UpdateError> {
    let exit = platform
        .spawn_wait_cancellable(&spec, &|| cancel.is_cancelled())
        .map_err(|e| process_error(e, name, cancel))?;
    checked_exit(exit, name)
}

fn checked_exit(exit: ProcessExit, name: &str) -> Result<ProcessExit, UpdateError> {
    if exit.code == Some(0) {
        Ok(exit)
    } else {
        Err(UpdateError::new(format!(
            "{name} failed with exit code {:?}\nstdout: {}\nstderr: {}",
            exit.code,
            tail(&exit.stdout.utf8_lossy(), 4000),
            tail(&exit.stderr.utf8_lossy(), 4000)
        )))
    }
}

fn required_rust_version(manifest: &Path) -> Result<Option<Version>, UpdateError> {
    let text = fs::read_to_string(manifest)
        .map_err(|e| UpdateError::new(format!("could not read workspace Cargo.toml: {e}")))?;
    let value: toml::Value = toml::from_str(&text)
        .map_err(|e| UpdateError::new(format!("invalid workspace Cargo.toml: {e}")))?;
    value
        .get("workspace")
        .and_then(|v| v.get("package"))
        .and_then(|v| v.get("rust-version"))
        .and_then(|v| v.as_str())
        .map(|v| {
            parse_version(v).map_err(|e| UpdateError::new(format!("invalid rust-version {v}: {e}")))
        })
        .transpose()
}

fn parse_rustc_version(output: &str) -> Result<Version, UpdateError> {
    let value = output
        .lines()
        .find_map(|line| line.strip_prefix("release: "))
        .or_else(|| output.split_whitespace().nth(1))
        .ok_or_else(|| UpdateError::new("rustc did not report its version"))?;
    parse_version(value.split('-').next().unwrap_or(value))
        .map_err(|e| UpdateError::new(format!("invalid rustc version {value}: {e}")))
}

fn parse_version(value: &str) -> Result<Version, semver::Error> {
    let dots = value.bytes().filter(|byte| *byte == b'.').count();
    if dots == 1 {
        Version::parse(&format!("{value}.0"))
    } else {
        Version::parse(value)
    }
}

fn extract_archive(
    bytes: &[u8],
    destination: &Path,
    cancel: &UpdateCancellation,
) -> Result<PathBuf, UpdateError> {
    cancel.check()?;
    fs::create_dir_all(destination)?;
    let mut archive =
        zip::ZipArchive::new(cancellation::CancellableReader(Cursor::new(bytes), cancel)).map_err(
            |e| {
                if cancel.is_cancelled() {
                    UpdateError::cancelled()
                } else {
                    UpdateError::new(format!("invalid source ZIP: {e}"))
                }
            },
        )?;
    for index in 0..archive.len() {
        cancel.check()?;
        let mut entry = archive.by_index(index).map_err(|e| {
            if cancel.is_cancelled() {
                UpdateError::cancelled()
            } else {
                UpdateError::new(e.to_string())
            }
        })?;
        let name = Path::new(entry.name());
        if name.is_absolute()
            || name.components().any(|c| {
                matches!(
                    c,
                    Component::ParentDir | Component::Prefix(_) | Component::RootDir
                )
            })
        {
            return Err(UpdateError::new(format!(
                "unsafe path in source ZIP: {}",
                entry.name()
            )));
        }
        if entry
            .unix_mode()
            .is_some_and(|mode| mode & 0o170000 == 0o120000)
        {
            return Err(UpdateError::new(format!(
                "links are not allowed in source ZIP: {}",
                entry.name()
            )));
        }
        let output = destination.join(name);
        if entry.is_dir() {
            fs::create_dir_all(&output)?;
        } else {
            if let Some(parent) = output.parent() {
                fs::create_dir_all(parent)?;
            }
            let mut file = File::create(output)?;
            copy_stream(&mut entry, &mut file, cancel)?;
        }
    }
    let roots: Vec<PathBuf> = fs::read_dir(destination)?
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .map(|e| e.path())
        .collect();
    if roots.len() != 1 {
        return Err(UpdateError::new(format!(
            "source ZIP must contain exactly one root directory, found {}",
            roots.len()
        )));
    }
    Ok(roots[0].clone())
}

fn validate_products_with_operations(
    work_dir: &Path,
    target: &Path,
    sha: &str,
    operations: &dyn PreparationOperations,
) -> Result<PreparedUpdate, UpdateError> {
    let prepared = validate_product_paths(work_dir, target, sha)?;
    operations.probe(&prepared.cli_exe, sha)?;
    operations.probe(&prepared.shell_exe, sha)?;
    Ok(prepared)
}

fn validate_product_paths(
    work_dir: &Path,
    target: &Path,
    sha: &str,
) -> Result<PreparedUpdate, UpdateError> {
    let shell_exe = target.join("release").join(SHELL_FILE);
    let cli_exe = target.join("release").join(CLI_FILE);
    for (label, path) in [
        ("shell executable", &shell_exe),
        ("CLI executable", &cli_exe),
    ] {
        if !path.is_file() {
            return Err(UpdateError::new(format!(
                "compiled {label} is missing: {}",
                path.display()
            )));
        }
    }
    vim::validate_bundle(&target.join("release/vim"))?;
    Ok(PreparedUpdate {
        work_dir: work_dir.to_owned(),
        target_sha: sha.to_owned(),
        shell_exe,
        cli_exe,
    })
}

fn tail(value: &str, maximum: usize) -> String {
    if value.len() <= maximum {
        value.to_owned()
    } else {
        let mut start = value.len() - maximum;
        while !value.is_char_boundary(start) {
            start += 1;
        }
        value[start..].to_owned()
    }
}

pub fn stage_update_for_apply(
    prepared: &PreparedUpdate,
    install_dir: &Path,
) -> Result<StagedUpdate, UpdateError> {
    stage_update_for_apply_cancellable(prepared, install_dir, &UpdateCancellation::default())
}

pub fn stage_update_for_apply_cancellable(
    prepared: &PreparedUpdate,
    install_dir: &Path,
    cancel: &UpdateCancellation,
) -> Result<StagedUpdate, UpdateError> {
    cancel.check()?;
    #[cfg(target_os = "linux")]
    platform::linux::installation::require_portable_directory(install_dir)
        .map_err(|error| UpdateError::new(error.to_string()))?;
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        let _ = (prepared, install_dir);
        return Err(UpdateError::new(
            "automatic updates are supported only on Windows and Linux",
        ));
    }

    #[cfg(any(windows, target_os = "linux"))]
    {
        validate_update_probe(&prepared.cli_exe, &prepared.target_sha, cancel)?;
        let prepared_vim = vim::prepared_directory(prepared)?;
        vim::validate_bundle(&prepared_vim)?;
        let install_dir = fs::canonicalize(install_dir).map_err(|error| {
            UpdateError::new(format!("could not resolve installation directory: {error}"))
        })?;
        let installed_shell = install_dir.join(SHELL_FILE);
        let installed_cli = install_dir.join(CLI_FILE);
        let installed_vim = install_dir.join("vim");
        platform::validate_no_follow_path(&installed_vim, false).map_err(|error| {
            UpdateError::new(format!("unsafe installed Vim directory: {error}"))
        })?;
        if installed_vim.exists() && !installed_vim.is_dir() {
            return Err(UpdateError::new("installed Vim path is not a directory"));
        }
        for (label, path) in [
            ("installed Shell", &installed_shell),
            ("installed CLI", &installed_cli),
        ] {
            if !path.is_file() {
                return Err(UpdateError::new(format!(
                    "{label} is missing: {}",
                    path.display()
                )));
            }
        }

        let id = format!(
            "{}-{}-{}",
            prepared.target_sha.chars().take(12).collect::<String>(),
            std::process::id(),
            unix_millis()
        );
        let update_root = install_dir.join(".tundra-update");
        platform::validate_no_follow_path(&update_root, false).map_err(|error| {
            UpdateError::new(format!("unsafe installation update directory: {error}"))
        })?;
        fs::create_dir_all(&update_root)?;
        platform::validate_no_follow_path(&update_root, true).map_err(|error| {
            UpdateError::new(format!("unsafe installation update directory: {error}"))
        })?;
        for executable in [&prepared.shell_exe, &prepared.cli_exe] {
            platform::validate_no_follow_path(executable, true).map_err(|error| {
                UpdateError::new(format!("unsafe compiled program path: {error}"))
            })?;
        }
        let transaction_dir = update_root.join(id);
        cancel.check()?;
        fs::create_dir(&transaction_dir)?;
        let mut owned = WorkDirectory::new(transaction_dir.clone());
        let result = (|| {
            let new_dir = transaction_dir.join("new");
            let backup_dir = transaction_dir.join("backup");
            fs::create_dir_all(&new_dir)?;
            fs::create_dir_all(&backup_dir)?;
            copy_file(&prepared.shell_exe, &new_dir.join(SHELL_FILE), cancel)?;
            copy_file(&prepared.cli_exe, &new_dir.join(CLI_FILE), cancel)?;
            vim::copy_directory(&prepared_vim, &new_dir.join("vim"), cancel)?;
            vim::validate_bundle(&new_dir.join("vim"))?;
            // Run the validated new helper: an installed older CLI may still expect assets.
            copy_file(
                &prepared.cli_exe,
                &transaction_dir.join(HELPER_FILE),
                cancel,
            )?;

            let manifest_path = transaction_dir.join("transaction.json");
            let manifest = TransactionManifest {
                protocol: UPDATE_PROTOCOL_VERSION,
                target_sha: prepared.target_sha.clone(),
                install_dir,
                transaction_dir,
                state: TransactionState::Prepared,
                cli_replaced: false,
                shell_replaced: false,
                vim: Some(VimTransaction {
                    had_previous: installed_vim.is_dir(),
                    replaced: false,
                }),
            };
            write_manifest(&manifest_path, &manifest)?;
            cancel.check()?;
            Ok(StagedUpdate { manifest_path })
        })();
        finish_staged_update(&mut owned, result)
    }
}

#[cfg(any(windows, target_os = "linux"))]
fn finish_staged_update(
    owned: &mut WorkDirectory,
    result: Result<StagedUpdate, UpdateError>,
) -> Result<StagedUpdate, UpdateError> {
    // A failed cancellation journal must remain tracked by the caller. Deleting
    // files or forgetting the manifest would let startup apply a cancelled update.
    if let Err(error) = result {
        if owned.path.join("transaction.json").exists()
            && let Err(cleanup) = discard_staged_update(&owned.path.join("transaction.json"))
        {
            owned.retain();
            return Err(error.with_cleanup_failure(cleanup));
        }
        owned.finish(Err(error))
    } else {
        owned.finish(result)
    }
}

pub fn launch_update_helper(manifest_path: &Path, parent_pid: u32) -> Result<(), UpdateError> {
    launch_helper_mode(manifest_path, parent_pid, false)
}

pub fn recover_interrupted_update_from_current_exe(parent_pid: u32) -> Result<bool, UpdateError> {
    #[cfg(target_os = "linux")]
    if platform::installation::current_installation().backend
        != platform::installation::UpdateBackend::PortableUser
    {
        return Ok(false);
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        let _ = parent_pid;
        return Ok(false);
    }

    #[cfg(any(windows, target_os = "linux"))]
    {
        let executable = std::env::current_exe()
            .map_err(|error| UpdateError::new(format!("could not locate TundraUX: {error}")))?;
        let install_dir = executable
            .parent()
            .ok_or_else(|| UpdateError::new("TundraUX executable has no parent directory"))?;
        scan_update_recovery(install_dir, parent_pid, &launch_helper_mode)
    }
}

#[cfg(any(windows, target_os = "linux"))]
fn scan_update_recovery(
    install_dir: &Path,
    parent_pid: u32,
    launch_helper: &dyn Fn(&Path, u32, bool) -> Result<(), UpdateError>,
) -> Result<bool, UpdateError> {
    #[cfg(target_os = "linux")]
    platform::linux::installation::require_portable_directory(install_dir)
        .map_err(|error| UpdateError::new(error.to_string()))?;
    let install_dir = fs::canonicalize(install_dir).map_err(|error| {
        UpdateError::new(format!(
            "could not resolve installation directory for update recovery: {error}"
        ))
    })?;
    let root = install_dir.join(".tundra-update");
    let entries = match fs::read_dir(&root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(error) => {
            return Err(UpdateError::new(format!(
                "could not read update recovery directory {}: {error}",
                root.display()
            )));
        }
    };
    let mut manifests = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|error| {
            UpdateError::new(format!(
                "could not read an entry in update recovery directory {}: {error}",
                root.display()
            ))
        })?;
        let manifest_path = entry.path().join("transaction.json");
        if manifest_path.is_file() {
            manifests.push(manifest_path);
        }
    }
    manifests.sort();
    for manifest_path in manifests {
        let manifest = load_manifest(&manifest_path)?;
        match manifest.state {
            TransactionState::Committed
            | TransactionState::RolledBack
            | TransactionState::Cancelled => {
                let _ = fs::remove_dir_all(&manifest.transaction_dir);
            }
            TransactionState::Prepared => {
                launch_helper(&manifest_path, parent_pid, false)?;
                return Ok(true);
            }
            TransactionState::Applying
            | TransactionState::AwaitingReady
            | TransactionState::RollingBack => {
                launch_helper(&manifest_path, parent_pid, true)?;
                return Ok(true);
            }
            TransactionState::Failed => {
                return Err(UpdateError::new(format!(
                    "an update transaction requires manual recovery: {}",
                    manifest.transaction_dir.display()
                )));
            }
        }
    }
    Ok(false)
}

pub fn mark_update_ready_from_env() -> Result<(), UpdateError> {
    let Some(path) = std::env::var_os(UPDATE_READY_FILE_ENV).map(PathBuf::from) else {
        return Ok(());
    };
    platform::atomic_write_document(&path, b"ready\n")
        .map(|_| ())
        .map_err(|error| UpdateError::new(format!("could not mark update ready: {error}")))
}

pub fn apply_update_transaction(
    manifest_path: &Path,
    parent_pid: u32,
    recover_only: bool,
) -> Result<(), UpdateError> {
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        let _ = (manifest_path, parent_pid, recover_only);
        return Err(UpdateError::new(
            "automatic updates are supported only on Windows and Linux",
        ));
    }

    #[cfg(any(windows, target_os = "linux"))]
    {
        // A terminal Ctrl-C also reaches the helper. Let Shell finish its own
        // cleanup before the foreground job ends; exec resets these handlers.
        let _terminal_control = platform::TerminalControlHandler::install();
        // Linux execs the helper in place, preserving the foreground process group.
        // Windows synchronous launchers pass zero: their UI has already stopped,
        // but the original process must stay alive to keep PowerShell waiting.
        if parent_pid != 0 && (!cfg!(target_os = "linux") || parent_pid != std::process::id()) {
            wait_for_process_exit(parent_pid, Duration::from_secs(30))?;
        }
        let mut manifest = load_manifest(manifest_path)?;
        validate_running_helper(&manifest)?;
        let operations = NativeTransactionOperations::default();
        run_update_transaction(manifest_path, &mut manifest, recover_only, &operations)?;
        // Keep the foreground job alive until the new (or restored) Shell exits.
        if let Some(mut child) = operations.child.borrow_mut().take() {
            child.wait()?;
        }
        Ok(())
    }
}

#[cfg(any(windows, target_os = "linux"))]
trait TransactionOperations {
    fn stop_new_shell(&self) {}
    fn replace(&self, target: &Path, replacement: &Path, backup: &Path) -> Result<(), UpdateError>;
    fn replace_directory(
        &self,
        target: &Path,
        replacement: &Path,
        backup: &Path,
    ) -> Result<(), UpdateError> {
        vim::replace_directory(target, replacement, backup)
    }
    fn launch_new_and_wait(
        &self,
        paths: &TransactionPaths,
        target_sha: &str,
    ) -> Result<(), UpdateError>;
    fn launch_restored(&self, shell: &Path, reason: &str) -> Result<(), UpdateError>;
}

#[cfg(any(windows, target_os = "linux"))]
#[derive(Default)]
struct NativeTransactionOperations {
    child: std::cell::RefCell<Option<platform::ManagedChild>>,
}

#[cfg(any(windows, target_os = "linux"))]
impl TransactionOperations for NativeTransactionOperations {
    fn stop_new_shell(&self) {
        if let Some(mut child) = self.child.borrow_mut().take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
    fn replace(&self, target: &Path, replacement: &Path, backup: &Path) -> Result<(), UpdateError> {
        platform::replace_file_with_backup(target, replacement, backup)
            .map_err(|error| UpdateError::new(error.to_string()))
    }

    fn launch_new_and_wait(
        &self,
        paths: &TransactionPaths,
        target_sha: &str,
    ) -> Result<(), UpdateError> {
        let mut child = std::process::Command::new(&paths.installed_shell)
            .env(UPDATE_READY_FILE_ENV, &paths.ready)
            .env(UPDATE_TARGET_SHA_ENV, target_sha)
            .spawn_managed_inherited()
            .map_err(|error| UpdateError::new(format!("could not start updated Shell: {error}")))?;
        let deadline = std::time::Instant::now() + Duration::from_secs(60);
        while std::time::Instant::now() < deadline {
            if paths.ready.is_file() {
                *self.child.borrow_mut() = Some(child);
                return Ok(());
            }
            if let Some(status) = child.try_wait().map_err(|error| {
                UpdateError::new(format!("could not monitor updated Shell: {error}"))
            })? {
                return Err(UpdateError::new(format!(
                    "updated Shell exited before becoming ready: {status}"
                )));
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        let _ = child.kill();
        let _ = child.wait();
        Err(UpdateError::new(
            "updated Shell did not become ready within 60 seconds",
        ))
    }

    fn launch_restored(&self, shell: &Path, reason: &str) -> Result<(), UpdateError> {
        let child = std::process::Command::new(shell)
            .env(UPDATE_ROLLBACK_ENV, reason)
            .spawn_managed_inherited()
            .map_err(|error| {
                UpdateError::new(format!("could not restart restored Shell: {error}"))
            })?;
        *self.child.borrow_mut() = Some(child);
        Ok(())
    }
}

#[cfg(any(windows, target_os = "linux"))]
impl Drop for NativeTransactionOperations {
    fn drop(&mut self) {
        self.stop_new_shell();
    }
}

#[cfg(any(windows, target_os = "linux"))]
fn run_update_transaction(
    manifest_path: &Path,
    manifest: &mut TransactionManifest,
    recover_only: bool,
    operations: &dyn TransactionOperations,
) -> Result<(), UpdateError> {
    if recover_only {
        return rollback_and_restart_with_operations(
            manifest_path,
            manifest,
            "update was interrupted",
            operations,
        );
    }
    if manifest.protocol != UPDATE_PROTOCOL_VERSION {
        return rollback_and_restart_with_operations(
            manifest_path,
            manifest,
            "legacy update requires a complete portable installation with bundled Vim",
            operations,
        );
    }
    if manifest.state != TransactionState::Prepared {
        return rollback_and_restart_with_operations(
            manifest_path,
            manifest,
            "update transaction was not in the prepared state",
            operations,
        );
    }

    manifest.state = TransactionState::Applying;
    write_manifest(manifest_path, manifest)?;
    let apply_result = apply_prepared_files_with_operations(manifest_path, manifest, operations)
        .and_then(|_| {
            launch_and_verify_new_shell_with_operations(manifest_path, manifest, operations)
        });
    match apply_result {
        Ok(()) => Ok(()),
        Err(error) => match rollback_and_restart_with_operations(
            manifest_path,
            manifest,
            &error.to_string(),
            operations,
        ) {
            Ok(()) => Ok(()),
            Err(rollback) => Err(UpdateError::new(format!(
                "update failed: {error}; rollback also failed: {rollback}"
            ))),
        },
    }
}

#[cfg(any(windows, target_os = "linux"))]
fn validate_running_helper(manifest: &TransactionManifest) -> Result<(), UpdateError> {
    let executable = std::env::current_exe()
        .and_then(fs::canonicalize)
        .map_err(|error| UpdateError::new(format!("could not locate update helper: {error}")))?;
    let transaction_dir = fs::canonicalize(&manifest.transaction_dir).map_err(|error| {
        UpdateError::new(format!(
            "could not resolve update transaction directory: {error}"
        ))
    })?;
    if executable.parent() != Some(transaction_dir.as_path())
        || executable.file_name() != Some(std::ffi::OsStr::new(HELPER_FILE))
    {
        return Err(UpdateError::new(
            "the update transaction must be applied by its saved helper copy",
        ));
    }
    Ok(())
}

#[cfg(all(any(windows, target_os = "linux"), test))]
fn apply_prepared_files(
    manifest_path: &Path,
    manifest: &mut TransactionManifest,
) -> Result<(), UpdateError> {
    apply_prepared_files_with_operations(
        manifest_path,
        manifest,
        &NativeTransactionOperations::default(),
    )
}

#[cfg(any(windows, target_os = "linux"))]
fn apply_prepared_files_with_operations(
    manifest_path: &Path,
    manifest: &mut TransactionManifest,
    operations: &dyn TransactionOperations,
) -> Result<(), UpdateError> {
    if manifest.protocol != UPDATE_PROTOCOL_VERSION || manifest.vim.is_none() {
        return Err(UpdateError::new(
            "new update transactions require the bundled Vim payload",
        ));
    }
    let paths = transaction_paths(manifest);
    if paths.ready.exists() {
        fs::remove_file(&paths.ready)?;
    }
    vim::validate_bundle(&paths.new_vim)?;

    operations
        .replace(&paths.installed_cli, &paths.new_cli, &paths.backup_cli)
        .map_err(|error| UpdateError::new(format!("could not replace TundraUX CLI: {error}")))?;
    manifest.cli_replaced = true;
    write_manifest(manifest_path, manifest)?;

    operations
        .replace(
            &paths.installed_shell,
            &paths.new_shell,
            &paths.backup_shell,
        )
        .map_err(|error| UpdateError::new(format!("could not replace TundraUX Shell: {error}")))?;
    manifest.shell_replaced = true;
    write_manifest(manifest_path, manifest)?;
    if let Some(vim) = &mut manifest.vim {
        operations
            .replace_directory(&paths.installed_vim, &paths.new_vim, &paths.backup_vim)
            .map_err(|error| UpdateError::new(format!("could not replace bundled Vim: {error}")))?;
        vim.replaced = true;
    }
    manifest.state = TransactionState::AwaitingReady;
    write_manifest(manifest_path, manifest)
}

#[cfg(any(windows, target_os = "linux"))]
fn launch_and_verify_new_shell_with_operations(
    manifest_path: &Path,
    manifest: &mut TransactionManifest,
    operations: &dyn TransactionOperations,
) -> Result<(), UpdateError> {
    let paths = transaction_paths(manifest);
    operations.launch_new_and_wait(&paths, &manifest.target_sha)?;
    manifest.state = TransactionState::Committed;
    write_manifest(manifest_path, manifest)?;
    cleanup_committed_payload(manifest);
    Ok(())
}

#[cfg(any(windows, target_os = "linux"))]
fn cleanup_committed_payload(manifest: &TransactionManifest) {
    let paths = transaction_paths(manifest);
    for file in [
        paths.backup_shell,
        paths.backup_cli,
        paths.new_shell,
        paths.new_cli,
        paths.ready,
    ] {
        let _ = fs::remove_file(file);
    }
    for directory in [paths.backup_vim, paths.new_vim] {
        let _ = fs::remove_dir_all(directory);
    }
}

#[cfg(any(windows, target_os = "linux"))]
fn rollback_and_restart_with_operations(
    manifest_path: &Path,
    manifest: &mut TransactionManifest,
    reason: &str,
    operations: &dyn TransactionOperations,
) -> Result<(), UpdateError> {
    operations.stop_new_shell();
    rollback_files_with_operations(manifest_path, manifest, operations)?;
    let paths = transaction_paths(manifest);
    if let Err(error) = operations.launch_restored(&paths.installed_shell, reason) {
        manifest.state = TransactionState::Failed;
        write_manifest(manifest_path, manifest)?;
        return Err(error);
    }
    Ok(())
}

#[cfg(all(any(windows, target_os = "linux"), test))]
fn rollback_files(
    manifest_path: &Path,
    manifest: &mut TransactionManifest,
) -> Result<(), UpdateError> {
    rollback_files_with_operations(
        manifest_path,
        manifest,
        &NativeTransactionOperations::default(),
    )
}

#[cfg(any(windows, target_os = "linux"))]
fn rollback_files_with_operations(
    manifest_path: &Path,
    manifest: &mut TransactionManifest,
    operations: &dyn TransactionOperations,
) -> Result<(), UpdateError> {
    manifest.state = TransactionState::RollingBack;
    write_manifest(manifest_path, manifest)?;
    let paths = transaction_paths(manifest);
    fs::create_dir_all(manifest.transaction_dir.join("failed"))?;

    if let Some(vim) = &mut manifest.vim {
        if paths.backup_vim.is_dir() {
            operations
                .replace_directory(&paths.installed_vim, &paths.backup_vim, &paths.failed_vim)
                .map_err(|error| {
                    UpdateError::new(format!("could not restore bundled Vim: {error}"))
                })?;
            vim.replaced = false;
            write_manifest(manifest_path, manifest)?;
        } else if vim.replaced || (!vim.had_previous && !paths.new_vim.exists()) {
            if vim.had_previous && !paths.failed_vim.is_dir() {
                return Err(UpdateError::new(
                    "the Vim rollback backup is missing; transaction files were retained",
                ));
            }
            if !vim.had_previous && paths.installed_vim.exists() {
                fs::rename(&paths.installed_vim, &paths.failed_vim)?;
            }
            vim.replaced = false;
            write_manifest(manifest_path, manifest)?;
        }
    }

    if paths.backup_shell.is_file() {
        let source = rollback_source(&paths.backup_shell)?;
        operations
            .replace(&paths.installed_shell, &source, &paths.failed_shell)
            .map_err(|error| {
                UpdateError::new(format!("could not restore TundraUX Shell: {error}"))
            })?;
        manifest.shell_replaced = false;
        write_manifest(manifest_path, manifest)?;
    } else if manifest.shell_replaced {
        if !paths.failed_shell.is_file() {
            return Err(UpdateError::new(
                "the Shell rollback backup is missing; transaction files were retained",
            ));
        }
        manifest.shell_replaced = false;
        write_manifest(manifest_path, manifest)?;
    }
    if paths.backup_cli.is_file() {
        let source = rollback_source(&paths.backup_cli)?;
        operations
            .replace(&paths.installed_cli, &source, &paths.failed_cli)
            .map_err(|error| {
                UpdateError::new(format!("could not restore TundraUX CLI: {error}"))
            })?;
        manifest.cli_replaced = false;
        write_manifest(manifest_path, manifest)?;
    } else if manifest.cli_replaced {
        if !paths.failed_cli.is_file() {
            return Err(UpdateError::new(
                "the CLI rollback backup is missing; transaction files were retained",
            ));
        }
        manifest.cli_replaced = false;
        write_manifest(manifest_path, manifest)?;
    }
    manifest.state = TransactionState::RolledBack;
    write_manifest(manifest_path, manifest)
}

#[cfg(any(windows, target_os = "linux"))]
fn rollback_source(backup: &Path) -> Result<PathBuf, UpdateError> {
    #[cfg(windows)]
    {
        // The original Shell is still running from the backup image while it
        // waits for the helper. ReplaceFileW needs write access to its source,
        // so restore a copy and retain the loaded image until a later cleanup.
        let source = backup.with_extension("restore");
        fs::copy(backup, &source).map_err(|error| {
            UpdateError::new(format!("could not copy rollback backup: {error}"))
        })?;
        Ok(source)
    }
    #[cfg(not(windows))]
    Ok(backup.to_owned())
}

#[cfg(any(windows, target_os = "linux"))]
fn wait_for_process_exit(pid: u32, timeout: Duration) -> Result<(), UpdateError> {
    let platform = platform::native_platform();
    let deadline = std::time::Instant::now() + timeout;
    while std::time::Instant::now() < deadline {
        match platform.is_process_alive(pid) {
            Ok(false) => return Ok(()),
            Ok(true) => std::thread::sleep(Duration::from_millis(100)),
            Err(error) => {
                return Err(UpdateError::new(format!(
                    "could not wait for running Shell: {error}"
                )));
            }
        }
    }
    Err(UpdateError::new(
        "running Shell did not exit within 30 seconds",
    ))
}

fn launch_helper_mode(
    manifest_path: &Path,
    parent_pid: u32,
    recover_only: bool,
) -> Result<(), UpdateError> {
    let manifest = load_manifest(manifest_path)?;
    let helper = manifest.transaction_dir.join(HELPER_FILE);
    if !helper.is_file() {
        return Err(UpdateError::new(format!(
            "update helper is missing: {}",
            helper.display()
        )));
    }
    let command = if recover_only {
        "__recover-update"
    } else {
        "__apply-update"
    };
    let mut process = std::process::Command::new(helper);
    // The Windows caller has released the terminal and watchdog already. It
    // waits for this helper instead of exiting; do not wait for it in return.
    let helper_parent_pid = if cfg!(windows) { 0 } else { parent_pid };
    process
        .arg(command)
        .arg(manifest_path)
        .arg(helper_parent_pid.to_string())
        .env_remove(UPDATE_READY_FILE_ENV)
        .env_remove(UPDATE_TARGET_SHA_ENV)
        .env_remove(UPDATE_ROLLBACK_ENV);
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::process::CommandExt;
        Err(UpdateError::new(format!(
            "could not launch update helper: {}",
            process.exec()
        )))
    }
    #[cfg(windows)]
    {
        let _terminal_control = platform::TerminalControlHandler::install();
        let status = process
            .spawn_successor()
            .and_then(|mut child| child.wait())
            .map_err(|error| {
                UpdateError::new(format!("could not launch update helper: {error}"))
            })?;
        if !status.success() {
            return Err(UpdateError::new(format!("update helper failed: {status}")));
        }
        Ok(())
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    process
        .spawn_successor()
        .and_then(|mut child| child.wait())
        .map(|_| ())
        .map_err(|error| UpdateError::new(format!("could not launch update helper: {error}")))
}

fn validate_update_probe(
    executable: &Path,
    expected_sha: &str,
    cancel: &UpdateCancellation,
) -> Result<(), UpdateError> {
    let output = run_checked(
        platform::native_platform().as_ref(),
        ProcessSpec::new(executable).arg("__update-probe"),
        "update probe",
        cancel,
    )?;
    let stdout = output.stdout.utf8_lossy();
    let protocol = format!("protocol={UPDATE_PROTOCOL_VERSION}");
    let commit = format!("commit={expected_sha}");
    if !stdout.lines().any(|line| line == protocol) || !stdout.lines().any(|line| line == commit) {
        return Err(UpdateError::new(format!(
            "compiled program reported the wrong update protocol or commit: {}",
            tail(&stdout, 1000)
        )));
    }
    Ok(())
}

fn load_manifest(path: &Path) -> Result<TransactionManifest, UpdateError> {
    let bytes = fs::read(path)
        .map_err(|error| UpdateError::new(format!("could not read update transaction: {error}")))?;
    let manifest: TransactionManifest = serde_json::from_slice(&bytes)
        .map_err(|error| UpdateError::new(format!("invalid update transaction: {error}")))?;
    validate_manifest_location(path, &manifest)?;
    #[cfg(target_os = "linux")]
    platform::linux::installation::require_portable_directory(&manifest.install_dir)
        .map_err(|error| UpdateError::new(error.to_string()))?;
    Ok(manifest)
}

fn validate_manifest_location(
    path: &Path,
    manifest: &TransactionManifest,
) -> Result<(), UpdateError> {
    if manifest.protocol != UPDATE_PROTOCOL_VERSION && manifest.protocol != 2 {
        return Err(UpdateError::new(format!(
            "unsupported update protocol {}",
            manifest.protocol
        )));
    }
    if manifest.protocol == UPDATE_PROTOCOL_VERSION && manifest.vim.is_none() {
        return Err(UpdateError::new(
            "update transaction is missing bundled Vim state",
        ));
    }
    let expected = manifest.transaction_dir.join("transaction.json");
    let expected_root = manifest.install_dir.join(".tundra-update");
    if path != expected || manifest.transaction_dir.parent() != Some(expected_root.as_path()) {
        return Err(UpdateError::new(
            "update transaction is outside the installation update directory",
        ));
    }
    platform::validate_no_follow_path(path, true)
        .map_err(|error| UpdateError::new(format!("unsafe update transaction path: {error}")))
}

fn write_manifest(path: &Path, manifest: &TransactionManifest) -> Result<(), UpdateError> {
    let bytes = serde_json::to_vec_pretty(manifest).map_err(|error| {
        UpdateError::new(format!("could not encode update transaction: {error}"))
    })?;
    platform::atomic_write_document(path, &bytes)
        .map(|_| ())
        .map_err(|error| UpdateError::new(format!("could not save update transaction: {error}")))
}

#[cfg(any(test, windows, target_os = "linux"))]
fn unix_millis() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default()
}

#[cfg(any(windows, target_os = "linux"))]
struct TransactionPaths {
    installed_shell: PathBuf,
    installed_cli: PathBuf,
    new_shell: PathBuf,
    new_cli: PathBuf,
    backup_shell: PathBuf,
    backup_cli: PathBuf,
    failed_shell: PathBuf,
    failed_cli: PathBuf,
    installed_vim: PathBuf,
    new_vim: PathBuf,
    backup_vim: PathBuf,
    failed_vim: PathBuf,
    ready: PathBuf,
}

#[cfg(any(windows, target_os = "linux"))]
fn transaction_paths(manifest: &TransactionManifest) -> TransactionPaths {
    let new = manifest.transaction_dir.join("new");
    let backup = manifest.transaction_dir.join("backup");
    let failed = manifest.transaction_dir.join("failed");
    TransactionPaths {
        installed_shell: manifest.install_dir.join(SHELL_FILE),
        installed_cli: manifest.install_dir.join(CLI_FILE),
        new_shell: new.join(SHELL_FILE),
        new_cli: new.join(CLI_FILE),
        backup_shell: backup.join(SHELL_FILE),
        backup_cli: backup.join(CLI_FILE),
        failed_shell: failed.join(SHELL_FILE),
        failed_cli: failed.join(CLI_FILE),
        installed_vim: manifest.install_dir.join("vim"),
        new_vim: new.join("vim"),
        backup_vim: backup.join("vim"),
        failed_vim: failed.join("vim"),
        ready: manifest.transaction_dir.join("ready"),
    }
}

#[cfg(test)]
#[path = "../../tests/unit/update.rs"]
mod tests;

#[cfg(test)]
#[path = "../../tests/unit/update_cancellation.rs"]
mod cancellation_tests;

fn process_error(
    error: platform::PlatformError,
    name: &str,
    cancel: &UpdateCancellation,
) -> UpdateError {
    if cancel.is_cancelled() && matches!(error, platform::PlatformError::Interrupted { .. }) {
        UpdateError::cancelled()
    } else {
        UpdateError::new(format!("could not run {name}: {error}"))
    }
}
