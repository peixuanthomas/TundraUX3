use std::fmt;
use std::fs::{self, File};
use std::io::{self, Cursor};
use std::path::{Component, Path, PathBuf};
use std::time::Duration;

use platform::{Platform, PlatformKind, ProcessExit, ProcessSpec};
use reqwest::blocking::{Client, Response};
use semver::Version;
use serde::{Deserialize, Serialize};

pub const UPDATE_PROTOCOL_VERSION: u32 = 1;
pub const GITHUB_OWNER: &str = "peixuanthomas";
pub const GITHUB_REPO: &str = "TundraUX3";
pub const UPDATE_READY_FILE_ENV: &str = "TUNDRAUX3_UPDATE_READY_FILE";
pub const UPDATE_TARGET_SHA_ENV: &str = "TUNDRAUX3_UPDATE_TARGET_SHA";
pub const UPDATE_ROLLBACK_ENV: &str = "TUNDRAUX3_UPDATE_ROLLBACK";
const API_ROOT: &str = "https://api.github.com";
const USER_AGENT: &str = "TundraUX3-updater/1";

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
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedUpdate {
    pub work_dir: PathBuf,
    pub target_sha: String,
    pub shell_exe: PathBuf,
    pub cli_exe: PathBuf,
    pub default_assets: PathBuf,
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
    assets_replaced: bool,
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
    let client = Client::builder()
        .user_agent(USER_AGENT)
        .timeout(Duration::from_secs(20))
        .build()
        .map_err(|e| UpdateError::new(format!("could not create GitHub client: {e}")))?;
    let repository: Repository = get_json(
        &client,
        &format!("{API_ROOT}/repos/{GITHUB_OWNER}/{GITHUB_REPO}"),
    )?;
    let branch: Branch = get_json(
        &client,
        &format!(
            "{API_ROOT}/repos/{GITHUB_OWNER}/{GITHUB_REPO}/branches/{}",
            repository.default_branch
        ),
    )?;
    let (relation, commits) = if let Some(local) = identity.commit_sha.as_deref() {
        match fetch_comparison(&client, local, &branch.commit.sha) {
            Ok(result) => result,
            Err(error) if error.http_status == Some(404) => (
                UpdateRelation::Unknown,
                fetch_recent_commits(&client, &repository.default_branch)?,
            ),
            Err(error) => return Err(error),
        }
    } else {
        (
            UpdateRelation::Unknown,
            fetch_recent_commits(&client, &repository.default_branch)?,
        )
    };
    Ok(UpdateCheckResult {
        default_branch: repository.default_branch,
        head_sha: branch.commit.sha,
        relation,
        commits,
    })
}

fn fetch_recent_commits(client: &Client, branch: &str) -> Result<Vec<UpdateCommit>, UpdateError> {
    let values: Vec<ApiCommit> = get_json(
        client,
        &format!("{API_ROOT}/repos/{GITHUB_OWNER}/{GITHUB_REPO}/commits?sha={branch}&per_page=20"),
    )?;
    Ok(map_commits(values))
}

fn fetch_comparison(
    client: &Client,
    base: &str,
    head: &str,
) -> Result<(UpdateRelation, Vec<UpdateCommit>), UpdateError> {
    fetch_comparison_from(client, API_ROOT, base, head)
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

pub fn prepare_update(
    platform: &dyn Platform,
    check: &UpdateCheckResult,
    progress: &mut dyn FnMut(UpdateProgress),
) -> Result<PreparedUpdate, UpdateError> {
    if platform.kind() != PlatformKind::Windows {
        return Err(UpdateError::new(
            "automatic updates are supported only on Windows",
        ));
    }
    notify(
        progress,
        UpdatePhase::Downloading,
        "Downloading source archive",
    );
    let work_dir = platform
        .create_temp_dir("update")
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
    let url = format!(
        "{API_ROOT}/repos/{GITHUB_OWNER}/{GITHUB_REPO}/zipball/{}",
        check.head_sha
    );
    let bytes = checked(
        client
            .get(url)
            .send()
            .map_err(|e| UpdateError::new(format!("source download failed: {e}")))?,
    )?
    .bytes()
    .map_err(|e| UpdateError::new(format!("source download failed: {e}")))?;
    let source_root = extract_archive(bytes.as_ref(), &work_dir.join("source"))?;
    notify(
        progress,
        UpdatePhase::CheckingToolchain,
        "Checking Rust toolchain",
    );
    let required = required_rust_version(&source_root.join("Cargo.toml"))?;
    let rustc = run_checked(platform, ProcessSpec::new("rustc").arg("-Vv"), "rustc")?;
    if let Some(required) = required {
        let installed = parse_rustc_version(&rustc.stdout.utf8_lossy())?;
        if installed < required {
            return Err(UpdateError::new(format!(
                "rustc {installed} is too old; source requires {required}"
            )));
        }
    }
    run_checked(platform, ProcessSpec::new("cargo").arg("-V"), "cargo")?;
    notify(
        progress,
        UpdatePhase::Compiling,
        "Compiling release executables",
    );
    let target = work_dir.join("target");
    let build = ProcessSpec::new("cargo")
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
        .env("TUNDRAUX3_BUILD_COMMIT", &check.head_sha);
    run_checked(platform, build, "cargo build")?;
    notify(progress, UpdatePhase::Staging, "Validating compiled files");
    validate_products(work_dir, &source_root, &target, &check.head_sha)
}

fn notify(progress: &mut dyn FnMut(UpdateProgress), phase: UpdatePhase, message: &str) {
    progress(UpdateProgress {
        phase,
        message: message.to_owned(),
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

fn validate_products(
    work_dir: &Path,
    source_root: &Path,
    target: &Path,
    sha: &str,
) -> Result<PreparedUpdate, UpdateError> {
    let prepared = validate_product_paths(work_dir, source_root, target, sha)?;
    validate_update_probe(&prepared.cli_exe, sha)?;
    validate_update_probe(&prepared.shell_exe, sha)?;
    Ok(prepared)
}

fn validate_product_paths(
    work_dir: &Path,
    source_root: &Path,
    target: &Path,
    sha: &str,
) -> Result<PreparedUpdate, UpdateError> {
    let shell_exe = target.join("release/tundra-shell.exe");
    let cli_exe = target.join("release/tundra-cli.exe");
    let default_assets = source_root.join("assets/themes/default");
    for (label, path, directory) in [
        ("shell executable", &shell_exe, false),
        ("CLI executable", &cli_exe, false),
        ("default theme assets", &default_assets, true),
    ] {
        if (directory && !path.is_dir()) || (!directory && !path.is_file()) {
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
        default_assets,
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
    #[cfg(not(windows))]
    {
        let _ = (prepared, install_dir);
        return Err(UpdateError::new(
            "automatic updates are supported only on Windows",
        ));
    }

    #[cfg(windows)]
    {
        validate_update_probe(&prepared.cli_exe, &prepared.target_sha)?;
        let install_dir = fs::canonicalize(install_dir).map_err(|error| {
            UpdateError::new(format!("could not resolve installation directory: {error}"))
        })?;
        let installed_shell = install_dir.join("tundra-shell.exe");
        let installed_cli = install_dir.join("tundra-cli.exe");
        let installed_assets = install_dir.join("assets/themes/default");
        for (label, path, directory) in [
            ("installed Shell", &installed_shell, false),
            ("installed CLI", &installed_cli, false),
            ("installed default assets", &installed_assets, true),
        ] {
            if (directory && !path.is_dir()) || (!directory && !path.is_file()) {
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
        fs::copy(&prepared.shell_exe, new_dir.join("tundra-shell.exe"))?;
        fs::copy(&prepared.cli_exe, new_dir.join("tundra-cli.exe"))?;
        copy_tree_checked(&prepared.default_assets, &new_dir.join("default-assets"))?;
        fs::copy(&installed_cli, transaction_dir.join("update-helper.exe"))?;

        let manifest_path = transaction_dir.join("transaction.json");
        let manifest = TransactionManifest {
            protocol: UPDATE_PROTOCOL_VERSION,
            target_sha: prepared.target_sha.clone(),
            install_dir,
            transaction_dir,
            state: TransactionState::Prepared,
            assets_replaced: false,
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
    #[cfg(not(windows))]
    {
        let _ = parent_pid;
        return Ok(false);
    }

    #[cfg(windows)]
    {
        let executable = std::env::current_exe()
            .map_err(|error| UpdateError::new(format!("could not locate TundraUX: {error}")))?;
        let install_dir = executable
            .parent()
            .ok_or_else(|| UpdateError::new("TundraUX executable has no parent directory"))?;
        let root = install_dir.join(".tundra-update");
        let Ok(entries) = fs::read_dir(&root) else {
            return Ok(false);
        };
        let mut manifests = entries
            .filter_map(Result::ok)
            .map(|entry| entry.path().join("transaction.json"))
            .filter(|path| path.is_file())
            .collect::<Vec<_>>();
        manifests.sort();
        for manifest_path in manifests {
            let manifest = load_manifest(&manifest_path)?;
            match manifest.state {
                TransactionState::Committed | TransactionState::RolledBack => {
                    let _ = fs::remove_dir_all(&manifest.transaction_dir);
                }
                TransactionState::Prepared => {
                    launch_helper_mode(&manifest_path, parent_pid, false)?;
                    return Ok(true);
                }
                TransactionState::Applying
                | TransactionState::AwaitingReady
                | TransactionState::RollingBack => {
                    launch_helper_mode(&manifest_path, parent_pid, true)?;
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
    #[cfg(not(windows))]
    {
        let _ = (manifest_path, parent_pid, recover_only);
        return Err(UpdateError::new(
            "automatic updates are supported only on Windows",
        ));
    }

    #[cfg(windows)]
    {
        wait_for_process_exit(parent_pid, Duration::from_secs(30))?;
        let mut manifest = load_manifest(manifest_path)?;
        validate_running_helper(&manifest)?;
        if recover_only {
            return rollback_and_restart(manifest_path, &mut manifest, "update was interrupted");
        }
        if manifest.state != TransactionState::Prepared {
            return rollback_and_restart(
                manifest_path,
                &mut manifest,
                "update transaction was not in the prepared state",
            );
        }

        manifest.state = TransactionState::Applying;
        write_manifest(manifest_path, &manifest)?;
        let apply_result = apply_prepared_files(manifest_path, &mut manifest)
            .and_then(|_| launch_and_verify_new_shell(manifest_path, &mut manifest));
        match apply_result {
            Ok(()) => Ok(()),
            Err(error) => {
                match rollback_and_restart(manifest_path, &mut manifest, &error.to_string()) {
                    Ok(()) => Ok(()),
                    Err(rollback) => Err(UpdateError::new(format!(
                        "update failed: {error}; rollback also failed: {rollback}"
                    ))),
                }
            }
        }
    }
}

#[cfg(windows)]
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
        || executable.file_name() != Some(std::ffi::OsStr::new("update-helper.exe"))
    {
        return Err(UpdateError::new(
            "the update transaction must be applied by its saved helper copy",
        ));
    }
    Ok(())
}

#[cfg(windows)]
fn apply_prepared_files(
    manifest_path: &Path,
    manifest: &mut TransactionManifest,
) -> Result<(), UpdateError> {
    let paths = transaction_paths(manifest);
    if paths.ready.exists() {
        fs::remove_file(&paths.ready)?;
    }

    fs::rename(&paths.installed_assets, &paths.backup_assets)
        .map_err(|error| UpdateError::new(format!("could not back up default assets: {error}")))?;
    fs::rename(&paths.new_assets, &paths.installed_assets).map_err(|error| {
        let _ = fs::rename(&paths.backup_assets, &paths.installed_assets);
        UpdateError::new(format!("could not install default assets: {error}"))
    })?;
    manifest.assets_replaced = true;
    write_manifest(manifest_path, manifest)?;

    platform::replace_file_with_backup(&paths.installed_cli, &paths.new_cli, &paths.backup_cli)
        .map_err(|error| UpdateError::new(format!("could not replace tundra-cli.exe: {error}")))?;
    manifest.cli_replaced = true;
    write_manifest(manifest_path, manifest)?;

    platform::replace_file_with_backup(
        &paths.installed_shell,
        &paths.new_shell,
        &paths.backup_shell,
    )
    .map_err(|error| UpdateError::new(format!("could not replace tundra-shell.exe: {error}")))?;
    manifest.shell_replaced = true;
    manifest.state = TransactionState::AwaitingReady;
    write_manifest(manifest_path, manifest)
}

#[cfg(windows)]
fn launch_and_verify_new_shell(
    manifest_path: &Path,
    manifest: &mut TransactionManifest,
) -> Result<(), UpdateError> {
    let paths = transaction_paths(manifest);
    let mut child = std::process::Command::new(&paths.installed_shell)
        .env(UPDATE_READY_FILE_ENV, &paths.ready)
        .env(UPDATE_TARGET_SHA_ENV, &manifest.target_sha)
        .spawn()
        .map_err(|error| UpdateError::new(format!("could not start updated Shell: {error}")))?;
    let deadline = std::time::Instant::now() + Duration::from_secs(60);
    while std::time::Instant::now() < deadline {
        if paths.ready.is_file() {
            manifest.state = TransactionState::Committed;
            write_manifest(manifest_path, manifest)?;
            cleanup_committed_payload(manifest);
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

#[cfg(windows)]
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
    for directory in [paths.backup_assets, paths.new_assets] {
        let _ = fs::remove_dir_all(directory);
    }
}

#[cfg(windows)]
fn rollback_and_restart(
    manifest_path: &Path,
    manifest: &mut TransactionManifest,
    reason: &str,
) -> Result<(), UpdateError> {
    rollback_files(manifest_path, manifest)?;
    let paths = transaction_paths(manifest);
    std::process::Command::new(&paths.installed_shell)
        .env(UPDATE_ROLLBACK_ENV, reason)
        .spawn()
        .map_err(|error| UpdateError::new(format!("could not restart restored Shell: {error}")))?;
    Ok(())
}

#[cfg(windows)]
fn rollback_files(
    manifest_path: &Path,
    manifest: &mut TransactionManifest,
) -> Result<(), UpdateError> {
    manifest.state = TransactionState::RollingBack;
    write_manifest(manifest_path, manifest)?;
    let paths = transaction_paths(manifest);
    fs::create_dir_all(manifest.transaction_dir.join("failed"))?;

    if paths.backup_shell.is_file() {
        platform::replace_file_with_backup(
            &paths.installed_shell,
            &paths.backup_shell,
            &paths.failed_shell,
        )
        .map_err(|error| {
            UpdateError::new(format!("could not restore tundra-shell.exe: {error}"))
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
        platform::replace_file_with_backup(
            &paths.installed_cli,
            &paths.backup_cli,
            &paths.failed_cli,
        )
        .map_err(|error| UpdateError::new(format!("could not restore tundra-cli.exe: {error}")))?;
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
    if paths.backup_assets.is_dir() {
        if paths.installed_assets.exists() {
            fs::rename(&paths.installed_assets, &paths.failed_assets).map_err(|error| {
                UpdateError::new(format!("could not move failed default assets: {error}"))
            })?;
        }
        fs::rename(&paths.backup_assets, &paths.installed_assets).map_err(|error| {
            UpdateError::new(format!("could not restore default assets: {error}"))
        })?;
        manifest.assets_replaced = false;
        write_manifest(manifest_path, manifest)?;
    } else if manifest.assets_replaced {
        if !paths.failed_assets.is_dir() {
            return Err(UpdateError::new(
                "the asset rollback backup is missing; transaction files were retained",
            ));
        }
        manifest.assets_replaced = false;
        write_manifest(manifest_path, manifest)?;
    }

    manifest.state = TransactionState::RolledBack;
    write_manifest(manifest_path, manifest)
}

#[cfg(windows)]
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
    let helper = manifest.transaction_dir.join("update-helper.exe");
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
    std::process::Command::new(helper)
        .arg(command)
        .arg(manifest_path)
        .arg(parent_pid.to_string())
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

fn copy_tree_checked(source: &Path, destination: &Path) -> Result<(), UpdateError> {
    let metadata = fs::symlink_metadata(source)?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() || metadata_is_reparse(&metadata) {
        return Err(UpdateError::new(format!(
            "update asset path is not a regular directory: {}",
            source.display()
        )));
    }
    fs::create_dir_all(destination)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let metadata = fs::symlink_metadata(entry.path())?;
        if metadata.file_type().is_symlink() || metadata_is_reparse(&metadata) {
            return Err(UpdateError::new(format!(
                "links are not allowed in update assets: {}",
                entry.path().display()
            )));
        }
        let target = destination.join(entry.file_name());
        if metadata.is_dir() {
            copy_tree_checked(&entry.path(), &target)?;
        } else if metadata.is_file() {
            fs::copy(entry.path(), target)?;
        } else {
            return Err(UpdateError::new(format!(
                "unsupported update asset: {}",
                entry.path().display()
            )));
        }
    }
    Ok(())
}

#[cfg(windows)]
fn metadata_is_reparse(metadata: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    metadata.file_attributes() & 0x0400 != 0
}

#[cfg(not(windows))]
fn metadata_is_reparse(_metadata: &fs::Metadata) -> bool {
    false
}

fn unix_millis() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default()
}

#[cfg(windows)]
struct TransactionPaths {
    installed_shell: PathBuf,
    installed_cli: PathBuf,
    installed_assets: PathBuf,
    new_shell: PathBuf,
    new_cli: PathBuf,
    new_assets: PathBuf,
    backup_shell: PathBuf,
    backup_cli: PathBuf,
    backup_assets: PathBuf,
    failed_shell: PathBuf,
    failed_cli: PathBuf,
    failed_assets: PathBuf,
    ready: PathBuf,
}

#[cfg(windows)]
fn transaction_paths(manifest: &TransactionManifest) -> TransactionPaths {
    let new = manifest.transaction_dir.join("new");
    let backup = manifest.transaction_dir.join("backup");
    let failed = manifest.transaction_dir.join("failed");
    TransactionPaths {
        installed_shell: manifest.install_dir.join("tundra-shell.exe"),
        installed_cli: manifest.install_dir.join("tundra-cli.exe"),
        installed_assets: manifest.install_dir.join("assets/themes/default"),
        new_shell: new.join("tundra-shell.exe"),
        new_cli: new.join("tundra-cli.exe"),
        new_assets: new.join("default-assets"),
        backup_shell: backup.join("tundra-shell.exe"),
        backup_cli: backup.join("tundra-cli.exe"),
        backup_assets: backup.join("default-assets"),
        failed_shell: failed.join("tundra-shell.exe"),
        failed_cli: failed.join("tundra-cli.exe"),
        failed_assets: failed.join("default-assets"),
        ready: manifest.transaction_dir.join("ready"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn update_compare_relations_are_mapped_from_local_to_remote() {
        assert_eq!(
            relation_from_compare("identical", 0, 0),
            UpdateRelation::Identical
        );
        assert_eq!(
            relation_from_compare("ahead", 3, 0),
            UpdateRelation::Behind { remote_ahead: 3 }
        );
        assert_eq!(
            relation_from_compare("behind", 0, 2),
            UpdateRelation::Ahead { local_ahead: 2 }
        );
        assert_eq!(
            relation_from_compare("diverged", 4, 2),
            UpdateRelation::Diverged {
                remote_ahead: 4,
                local_ahead: 2
            }
        );
        assert_eq!(
            relation_from_compare("mystery", 0, 0),
            UpdateRelation::Unknown
        );
    }

    #[test]
    fn update_rust_version_is_read_and_compared() {
        let root =
            std::env::temp_dir().join(format!("tundra-update-manifest-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let path = root.join("Cargo.toml");
        fs::write(&path, "[workspace.package]\nrust-version = \"1.82\"\n").unwrap();
        assert_eq!(
            required_rust_version(&path).unwrap(),
            Some(Version::new(1, 82, 0))
        );
        assert_eq!(
            parse_rustc_version("rustc 1.85.1\nrelease: 1.85.1\n").unwrap(),
            Version::new(1, 85, 1)
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn update_zip_rejects_parent_traversal() {
        let mut cursor = Cursor::new(Vec::new());
        {
            let mut writer = zip::ZipWriter::new(&mut cursor);
            writer
                .start_file("../escape", zip::write::SimpleFileOptions::default())
                .unwrap();
            writer.write_all(b"bad").unwrap();
            writer.finish().unwrap();
        }
        let root = std::env::temp_dir().join(format!("tundra-update-zip-{}", std::process::id()));
        let error = extract_archive(cursor.get_ref(), &root).unwrap_err();
        assert!(error.to_string().contains("unsafe path"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn update_product_validation_requires_all_outputs() {
        let root =
            std::env::temp_dir().join(format!("tundra-update-products-{}", std::process::id()));
        fs::create_dir_all(root.join("source/assets/themes/default")).unwrap();
        fs::create_dir_all(root.join("target/release")).unwrap();
        fs::write(root.join("target/release/tundra-shell.exe"), b"shell").unwrap();
        assert!(
            validate_product_paths(&root, &root.join("source"), &root.join("target"), "abc")
                .is_err()
        );
        fs::write(root.join("target/release/tundra-cli.exe"), b"cli").unwrap();
        assert!(
            validate_product_paths(&root, &root.join("source"), &root.join("target"), "abc")
                .is_ok()
        );
        fs::remove_dir_all(root).unwrap();
    }

    fn update_test_root(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "tundra-update-{name}-{}-{}",
            std::process::id(),
            unix_millis()
        ))
    }

    #[test]
    fn update_manifest_must_stay_below_the_install_update_directory() {
        let root = update_test_root("manifest-location");
        let install = root.join("install");
        let transaction_dir = install.join(".tundra-update/tx");
        fs::create_dir_all(&transaction_dir).unwrap();
        let path = transaction_dir.join("transaction.json");
        let manifest = TransactionManifest {
            protocol: UPDATE_PROTOCOL_VERSION,
            target_sha: "abc".to_string(),
            install_dir: install.clone(),
            transaction_dir: transaction_dir.clone(),
            state: TransactionState::Prepared,
            assets_replaced: false,
            cli_replaced: false,
            shell_replaced: false,
        };
        write_manifest(&path, &manifest).unwrap();
        assert_eq!(load_manifest(&path).unwrap(), manifest);

        let outside = install.join("outside");
        fs::create_dir_all(&outside).unwrap();
        let outside_path = outside.join("transaction.json");
        let mut invalid = manifest;
        invalid.transaction_dir = outside;
        write_manifest(&outside_path, &invalid).unwrap();
        assert!(
            load_manifest(&outside_path)
                .unwrap_err()
                .to_string()
                .contains("outside the installation update directory")
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn update_compare_fetches_every_commit_page() {
        use std::io::{Read, Write};
        use std::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            for page in 1..=2 {
                let (mut stream, _) = listener.accept().unwrap();
                let mut request = [0_u8; 2048];
                let length = stream.read(&mut request).unwrap();
                let request = String::from_utf8_lossy(&request[..length]);
                assert!(request.contains(&format!("page={page}")));
                let count = if page == 1 { 100 } else { 2 };
                let commits = (0..count)
                    .map(|index| {
                        serde_json::json!({
                            "sha": format!("{page:02}{index:038}"),
                            "commit": { "message": format!("commit {page}-{index}") }
                        })
                    })
                    .collect::<Vec<_>>();
                let body = serde_json::json!({
                    "status": "ahead",
                    "ahead_by": 102,
                    "behind_by": 0,
                    "commits": commits
                })
                .to_string();
                write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                )
                .unwrap();
            }
        });
        let client = Client::builder()
            .timeout(Duration::from_secs(2))
            .build()
            .unwrap();
        let (relation, commits) =
            fetch_comparison_from(&client, &format!("http://{address}"), "local", "remote")
                .unwrap();
        server.join().unwrap();
        assert_eq!(relation, UpdateRelation::Behind { remote_ahead: 102 });
        assert_eq!(commits.len(), 102);
        assert_eq!(commits.last().unwrap().message, "commit 2-1");
    }

    #[test]
    fn update_network_failure_is_reported_clearly() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        drop(listener);
        let client = Client::builder()
            .timeout(Duration::from_millis(250))
            .build()
            .unwrap();
        let error =
            get_json::<Repository>(&client, &format!("http://{address}/repository")).unwrap_err();
        assert!(error.to_string().contains("GitHub request failed"));
    }

    #[cfg(windows)]
    #[test]
    fn update_rollback_restores_programs_and_default_assets_but_keeps_custom_themes() {
        let root = update_test_root("rollback");
        let install = root.join("install");
        let transaction_dir = install.join(".tundra-update/tx");
        let new = transaction_dir.join("new");
        fs::create_dir_all(new.join("default-assets")).unwrap();
        fs::create_dir_all(transaction_dir.join("backup")).unwrap();
        fs::create_dir_all(install.join("assets/themes/default")).unwrap();
        fs::create_dir_all(install.join("assets/themes/custom")).unwrap();
        fs::write(install.join("tundra-shell.exe"), b"old shell").unwrap();
        fs::write(install.join("tundra-cli.exe"), b"old cli").unwrap();
        fs::write(
            install.join("assets/themes/default/theme.txt"),
            b"old theme",
        )
        .unwrap();
        fs::write(
            install.join("assets/themes/custom/theme.txt"),
            b"custom theme",
        )
        .unwrap();
        fs::write(new.join("tundra-shell.exe"), b"new shell").unwrap();
        fs::write(new.join("tundra-cli.exe"), b"new cli").unwrap();
        fs::write(new.join("default-assets/theme.txt"), b"new theme").unwrap();
        let path = transaction_dir.join("transaction.json");
        let mut manifest = TransactionManifest {
            protocol: UPDATE_PROTOCOL_VERSION,
            target_sha: "abc".to_string(),
            install_dir: install.clone(),
            transaction_dir,
            state: TransactionState::Prepared,
            assets_replaced: false,
            cli_replaced: false,
            shell_replaced: false,
        };
        write_manifest(&path, &manifest).unwrap();
        apply_prepared_files(&path, &mut manifest).unwrap();
        assert_eq!(
            fs::read(install.join("tundra-shell.exe")).unwrap(),
            b"new shell"
        );
        assert_eq!(
            fs::read(install.join("tundra-cli.exe")).unwrap(),
            b"new cli"
        );
        assert_eq!(
            fs::read(install.join("assets/themes/default/theme.txt")).unwrap(),
            b"new theme"
        );

        rollback_files(&path, &mut manifest).unwrap();
        assert_eq!(
            fs::read(install.join("tundra-shell.exe")).unwrap(),
            b"old shell"
        );
        assert_eq!(
            fs::read(install.join("tundra-cli.exe")).unwrap(),
            b"old cli"
        );
        assert_eq!(
            fs::read(install.join("assets/themes/default/theme.txt")).unwrap(),
            b"old theme"
        );
        assert_eq!(
            fs::read(install.join("assets/themes/custom/theme.txt")).unwrap(),
            b"custom theme"
        );
        fs::remove_dir_all(root).unwrap();
    }
}
