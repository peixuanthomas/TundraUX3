use std::fmt;
use std::fs::{self, File};
use std::io::{self, Cursor, Read};
use std::path::{Component, Path, PathBuf};
use std::time::Duration;

use platform::{Platform, PlatformKind, ProcessExit, ProcessSpec};
use reqwest::blocking::{Client, Response};
use semver::Version;
use serde::{Deserialize, Serialize};

pub const UPDATE_PROTOCOL_VERSION: u32 = 2;
pub const GITHUB_OWNER: &str = "peixuanthomas";
pub const GITHUB_REPO: &str = "TundraUX3";
pub const UPDATE_READY_FILE_ENV: &str = "TUNDRAUX3_UPDATE_READY_FILE";
pub const UPDATE_TARGET_SHA_ENV: &str = "TUNDRAUX3_UPDATE_TARGET_SHA";
pub const UPDATE_ROLLBACK_ENV: &str = "TUNDRAUX3_UPDATE_ROLLBACK";
const API_ROOT: &str = "https://api.github.com";
const USER_AGENT: &str = "TundraUX3-updater/1";
#[path = "update_git.rs"]
mod git;
#[path = "update_toolchain.rs"]
mod toolchain;
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
    pub default_branch: String,
    pub head_sha: String,
    pub relation: UpdateRelation,
    pub commits: Vec<UpdateCommit>,
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
}

impl UpdateError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            http_status: None,
        }
    }

    fn http(status: u16, message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            http_status: Some(status),
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
        Self::new(value.to_string())
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
    check_with_fallback(identity, API_ROOT, || git::check(identity))
}

fn check_with_fallback(
    identity: &BuildIdentity,
    api_root: &str,
    fallback: impl FnOnce() -> Result<UpdateCheckResult, UpdateError>,
) -> Result<UpdateCheckResult, UpdateError> {
    check_using_api(identity, api_root).or_else(|api_error| {
        fallback().map_err(|git_error| {
            UpdateError::new(format!("{api_error}; Git fallback failed: {git_error}"))
        })
    })
}

fn check_using_api(
    identity: &BuildIdentity,
    api_root: &str,
) -> Result<UpdateCheckResult, UpdateError> {
    let client = Client::builder()
        .user_agent(USER_AGENT)
        .timeout(Duration::from_secs(20))
        .build()
        .map_err(|e| UpdateError::new(format!("could not create GitHub client: {e}")))?;
    let repository: Repository = get_json(
        &client,
        &format!("{api_root}/repos/{GITHUB_OWNER}/{GITHUB_REPO}"),
    )?;
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
            .map_err(|e| UpdateError::new(format!("GitHub request failed: {e}")))?,
    )?
    .json()
    .map_err(|e| UpdateError::new(format!("GitHub returned invalid JSON: {e}")))
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
    let body = response.text().unwrap_or_default();
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
    if !supports_updates(platform.kind()) {
        return Err(UpdateError::new(
            "automatic updates are supported only on Windows and Linux",
        ));
    }
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
    let result = prepare_in(platform, check, progress, &work_dir);
    if let Err(error) = &result {
        notify(progress, UpdatePhase::Failed, &error.to_string());
        let _ = platform.cleanup_temp_path(&work_dir);
    }
    result
}

fn prepare_in(
    platform: &dyn Platform,
    check: &UpdateCheckResult,
    progress: &mut dyn FnMut(UpdateProgress),
    work_dir: &Path,
) -> Result<PreparedUpdate, UpdateError> {
    let client = Client::builder()
        .user_agent(USER_AGENT)
        .timeout(Duration::from_secs(120))
        .build()
        .map_err(|e| UpdateError::new(e.to_string()))?;
    let url = source_archive_url(&check.head_sha)?;
    let response = checked(
        client
            .get(url)
            .send()
            .map_err(|e| UpdateError::new(format!("source download failed: {e}")))?,
    )?;
    let total = response.content_length();
    let bytes = download_source(response, total, progress)?;
    let source_root = extract_archive(bytes.as_ref(), &work_dir.join("source"))?;
    prepare_extracted(platform, check, progress, work_dir, &source_root)
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
}

impl PreparationOperations for PlatformPreparationOperations<'_> {
    fn run(&self, spec: &ProcessSpec, name: &str) -> Result<ProcessExit, UpdateError> {
        run_checked(self.platform, spec.clone(), name)
    }

    fn probe(&self, executable: &Path, expected_sha: &str) -> Result<(), UpdateError> {
        validate_update_probe(executable, expected_sha)
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
            .spawn_streaming(spec, &mut |output| {
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
            .map_err(|error| UpdateError::new(format!("could not run {name}: {error}")))?;
        checked_exit(exit, name)
    }
}

fn prepare_extracted(
    platform: &dyn Platform,
    check: &UpdateCheckResult,
    progress: &mut dyn FnMut(UpdateProgress),
    work_dir: &Path,
    source_root: &Path,
) -> Result<PreparedUpdate, UpdateError> {
    prepare_extracted_with_operations(
        check,
        progress,
        work_dir,
        source_root,
        &PlatformPreparationOperations {
            platform,
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
) -> Result<Vec<u8>, UpdateError> {
    let mut bytes = Vec::new();
    let mut buffer = [0u8; 65536];
    let mut last = std::time::Instant::now();
    loop {
        let count = reader
            .read(&mut buffer)
            .map_err(|error| UpdateError::new(format!("source download failed: {error}")))?;
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
) -> Result<ProcessExit, UpdateError> {
    let exit = platform
        .spawn_wait(&spec)
        .map_err(|e| UpdateError::new(format!("could not run {name}: {e}")))?;
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
    let value: toml::Value = fs::read_to_string(manifest)
        .map_err(|e| UpdateError::new(format!("could not read workspace Cargo.toml: {e}")))?
        .parse()
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

fn extract_archive(bytes: &[u8], destination: &Path) -> Result<PathBuf, UpdateError> {
    fs::create_dir_all(destination)?;
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes))
        .map_err(|e| UpdateError::new(format!("invalid source ZIP: {e}")))?;
    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|e| UpdateError::new(e.to_string()))?;
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
            io::copy(&mut entry, &mut file)?;
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
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        let _ = (prepared, install_dir);
        return Err(UpdateError::new(
            "automatic updates are supported only on Windows and Linux",
        ));
    }

    #[cfg(any(windows, target_os = "linux"))]
    {
        validate_update_probe(&prepared.cli_exe, &prepared.target_sha)?;
        let install_dir = fs::canonicalize(install_dir).map_err(|error| {
            UpdateError::new(format!("could not resolve installation directory: {error}"))
        })?;
        let installed_shell = install_dir.join(SHELL_FILE);
        let installed_cli = install_dir.join(CLI_FILE);
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
        let new_dir = transaction_dir.join("new");
        let backup_dir = transaction_dir.join("backup");
        fs::create_dir_all(&new_dir)?;
        fs::create_dir_all(&backup_dir)?;
        fs::copy(&prepared.shell_exe, new_dir.join(SHELL_FILE))?;
        fs::copy(&prepared.cli_exe, new_dir.join(CLI_FILE))?;
        // Run the validated new helper: an installed older CLI may still expect assets.
        fs::copy(&prepared.cli_exe, transaction_dir.join(HELPER_FILE))?;

        let manifest_path = transaction_dir.join("transaction.json");
        let manifest = TransactionManifest {
            protocol: UPDATE_PROTOCOL_VERSION,
            target_sha: prepared.target_sha.clone(),
            install_dir,
            transaction_dir,
            state: TransactionState::Prepared,
            cli_replaced: false,
            shell_replaced: false,
        };
        write_manifest(&manifest_path, &manifest)?;
        Ok(StagedUpdate { manifest_path })
    }
}

pub fn launch_update_helper(manifest_path: &Path, parent_pid: u32) -> Result<(), UpdateError> {
    launch_helper_mode(manifest_path, parent_pid, false)
}

pub fn recover_interrupted_update_from_current_exe(parent_pid: u32) -> Result<bool, UpdateError> {
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
            TransactionState::Committed | TransactionState::RolledBack => {
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
        #[cfg(target_os = "linux")]
        let _terminal_control = platform::TerminalControlHandler::install();
        // Linux execs the helper in place, preserving the foreground process group.
        if !cfg!(target_os = "linux") || parent_pid != std::process::id() {
            wait_for_process_exit(parent_pid, Duration::from_secs(30))?;
        }
        let mut manifest = load_manifest(manifest_path)?;
        validate_running_helper(&manifest)?;
        let operations = NativeTransactionOperations::default();
        run_update_transaction(manifest_path, &mut manifest, recover_only, &operations)?;
        // Keep the foreground job alive until the new (or restored) Shell exits.
        #[cfg(target_os = "linux")]
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
    #[cfg(target_os = "linux")]
    child: std::cell::RefCell<Option<std::process::Child>>,
}

#[cfg(any(windows, target_os = "linux"))]
impl TransactionOperations for NativeTransactionOperations {
    fn stop_new_shell(&self) {
        #[cfg(target_os = "linux")]
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
            .spawn()
            .map_err(|error| UpdateError::new(format!("could not start updated Shell: {error}")))?;
        let deadline = std::time::Instant::now() + Duration::from_secs(60);
        while std::time::Instant::now() < deadline {
            if paths.ready.is_file() {
                #[cfg(target_os = "linux")]
                {
                    *self.child.borrow_mut() = Some(child);
                }
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
            .spawn()
            .map_err(|error| {
                UpdateError::new(format!("could not restart restored Shell: {error}"))
            })?;
        #[cfg(target_os = "linux")]
        {
            *self.child.borrow_mut() = Some(child);
        }
        #[cfg(not(target_os = "linux"))]
        let _ = child;
        Ok(())
    }
}

#[cfg(target_os = "linux")]
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
    let paths = transaction_paths(manifest);
    if paths.ready.exists() {
        fs::remove_file(&paths.ready)?;
    }

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

    if paths.backup_shell.is_file() {
        operations
            .replace(
                &paths.installed_shell,
                &paths.backup_shell,
                &paths.failed_shell,
            )
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
        operations
            .replace(&paths.installed_cli, &paths.backup_cli, &paths.failed_cli)
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
    process
        .arg(command)
        .arg(manifest_path)
        .arg(parent_pid.to_string())
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
    #[cfg(not(target_os = "linux"))]
    process
        .spawn()
        .map(|_| ())
        .map_err(|error| UpdateError::new(format!("could not launch update helper: {error}")))
}

fn validate_update_probe(executable: &Path, expected_sha: &str) -> Result<(), UpdateError> {
    let output = std::process::Command::new(executable)
        .arg("__update-probe")
        .output()
        .map_err(|error| UpdateError::new(format!("could not probe compiled CLI: {error}")))?;
    if !output.status.success() {
        return Err(UpdateError::new(format!(
            "compiled update probe failed for {}: {}",
            executable.display(),
            tail(&String::from_utf8_lossy(&output.stderr), 1000)
        )));
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
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
    Ok(manifest)
}

fn validate_manifest_location(
    path: &Path,
    manifest: &TransactionManifest,
) -> Result<(), UpdateError> {
    if manifest.protocol != UPDATE_PROTOCOL_VERSION {
        return Err(UpdateError::new(format!(
            "unsupported update protocol {}",
            manifest.protocol
        )));
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
        ready: manifest.transaction_dir.join("ready"),
    }
}

#[cfg(test)]
#[path = "tests/update.rs"]
mod tests;
