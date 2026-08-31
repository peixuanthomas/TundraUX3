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
    prepare_extracted(platform, check, progress, work_dir, &source_root)
}

trait PreparationOperations {
    fn run(&self, spec: &ProcessSpec, name: &str) -> Result<ProcessExit, UpdateError>;
    fn probe(&self, executable: &Path, expected_sha: &str) -> Result<(), UpdateError>;
}

struct PlatformPreparationOperations<'a>(&'a dyn Platform);

impl PreparationOperations for PlatformPreparationOperations<'_> {
    fn run(&self, spec: &ProcessSpec, name: &str) -> Result<ProcessExit, UpdateError> {
        run_checked(self.0, spec.clone(), name)
    }

    fn probe(&self, executable: &Path, expected_sha: &str) -> Result<(), UpdateError> {
        validate_update_probe(executable, expected_sha)
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
        &PlatformPreparationOperations(platform),
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
    let rustc = operations.run(&ProcessSpec::new("rustc").arg("-Vv"), "rustc")?;
    if let Some(required) = required {
        let installed = parse_rustc_version(&rustc.stdout.utf8_lossy())?;
        if installed < required {
            return Err(UpdateError::new(format!(
                "rustc {installed} is too old; source requires {required}"
            )));
        }
    }
    operations.run(&ProcessSpec::new("cargo").arg("-V"), "cargo")?;
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
    operations.run(&build, "cargo build")?;
    notify(progress, UpdatePhase::Staging, "Validating compiled files");
    validate_products_with_operations(work_dir, source_root, &target, &check.head_sha, operations)
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

fn validate_products_with_operations(
    work_dir: &Path,
    source_root: &Path,
    target: &Path,
    sha: &str,
    operations: &dyn PreparationOperations,
) -> Result<PreparedUpdate, UpdateError> {
    let prepared = validate_product_paths(work_dir, source_root, target, sha)?;
    operations.probe(&prepared.cli_exe, sha)?;
    operations.probe(&prepared.shell_exe, sha)?;
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
        scan_update_recovery(install_dir, parent_pid, &launch_helper_mode)
    }
}

#[cfg(windows)]
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
        run_update_transaction(
            manifest_path,
            &mut manifest,
            recover_only,
            &WindowsTransactionOperations,
        )
    }
}

#[cfg(windows)]
trait TransactionOperations {
    fn rename(&self, source: &Path, target: &Path) -> Result<(), UpdateError>;
    fn replace(&self, target: &Path, replacement: &Path, backup: &Path) -> Result<(), UpdateError>;
    fn launch_new_and_wait(
        &self,
        paths: &TransactionPaths,
        target_sha: &str,
    ) -> Result<(), UpdateError>;
    fn launch_restored(&self, shell: &Path, reason: &str) -> Result<(), UpdateError>;
}

#[cfg(windows)]
struct WindowsTransactionOperations;

#[cfg(windows)]
impl TransactionOperations for WindowsTransactionOperations {
    fn rename(&self, source: &Path, target: &Path) -> Result<(), UpdateError> {
        fs::rename(source, target).map_err(UpdateError::from)
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
        std::process::Command::new(shell)
            .env(UPDATE_ROLLBACK_ENV, reason)
            .spawn()
            .map(|_| ())
            .map_err(|error| UpdateError::new(format!("could not restart restored Shell: {error}")))
    }
}

#[cfg(windows)]
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

#[cfg(all(windows, test))]
fn apply_prepared_files(
    manifest_path: &Path,
    manifest: &mut TransactionManifest,
) -> Result<(), UpdateError> {
    apply_prepared_files_with_operations(manifest_path, manifest, &WindowsTransactionOperations)
}

#[cfg(windows)]
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
        .rename(&paths.installed_assets, &paths.backup_assets)
        .map_err(|error| UpdateError::new(format!("could not back up default assets: {error}")))?;
    operations
        .rename(&paths.new_assets, &paths.installed_assets)
        .map_err(|error| {
            let _ = operations.rename(&paths.backup_assets, &paths.installed_assets);
            UpdateError::new(format!("could not install default assets: {error}"))
        })?;
    manifest.assets_replaced = true;
    write_manifest(manifest_path, manifest)?;

    operations
        .replace(&paths.installed_cli, &paths.new_cli, &paths.backup_cli)
        .map_err(|error| UpdateError::new(format!("could not replace tundra-cli.exe: {error}")))?;
    manifest.cli_replaced = true;
    write_manifest(manifest_path, manifest)?;

    operations
        .replace(
            &paths.installed_shell,
            &paths.new_shell,
            &paths.backup_shell,
        )
        .map_err(|error| {
            UpdateError::new(format!("could not replace tundra-shell.exe: {error}"))
        })?;
    manifest.shell_replaced = true;
    manifest.state = TransactionState::AwaitingReady;
    write_manifest(manifest_path, manifest)
}

#[cfg(windows)]
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
fn rollback_and_restart_with_operations(
    manifest_path: &Path,
    manifest: &mut TransactionManifest,
    reason: &str,
    operations: &dyn TransactionOperations,
) -> Result<(), UpdateError> {
    rollback_files_with_operations(manifest_path, manifest, operations)?;
    let paths = transaction_paths(manifest);
    if let Err(error) = operations.launch_restored(&paths.installed_shell, reason) {
        manifest.state = TransactionState::Failed;
        write_manifest(manifest_path, manifest)?;
        return Err(error);
    }
    Ok(())
}

#[cfg(all(windows, test))]
fn rollback_files(
    manifest_path: &Path,
    manifest: &mut TransactionManifest,
) -> Result<(), UpdateError> {
    rollback_files_with_operations(manifest_path, manifest, &WindowsTransactionOperations)
}

#[cfg(windows)]
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
        operations
            .replace(&paths.installed_cli, &paths.backup_cli, &paths.failed_cli)
            .map_err(|error| {
                UpdateError::new(format!("could not restore tundra-cli.exe: {error}"))
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
    if paths.backup_assets.is_dir() {
        if paths.installed_assets.exists() {
            operations
                .rename(&paths.installed_assets, &paths.failed_assets)
                .map_err(|error| {
                    UpdateError::new(format!("could not move failed default assets: {error}"))
                })?;
        }
        operations
            .rename(&paths.backup_assets, &paths.installed_assets)
            .map_err(|error| {
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
    use platform::ProcessStream;
    use std::cell::Cell;
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

    #[derive(Clone, Copy)]
    enum PreparationFailure {
        MissingRustc,
        MissingCargo,
        RustcTooOld,
        Locked,
        Compile,
        MissingProduct,
        CliProbe,
        ShellProbe,
    }

    struct FakePreparationOperations {
        failure: PreparationFailure,
    }

    impl PreparationOperations for FakePreparationOperations {
        fn run(&self, spec: &ProcessSpec, name: &str) -> Result<ProcessExit, UpdateError> {
            if name == "rustc" && matches!(self.failure, PreparationFailure::MissingRustc) {
                return Err(UpdateError::new("could not run rustc: missing"));
            }
            if name == "cargo" && matches!(self.failure, PreparationFailure::MissingCargo) {
                return Err(UpdateError::new("could not run cargo: missing"));
            }
            let failed_build = name == "cargo build"
                && matches!(
                    self.failure,
                    PreparationFailure::Locked | PreparationFailure::Compile
                );
            let stdout = if name == "rustc" {
                if matches!(self.failure, PreparationFailure::RustcTooOld) {
                    b"rustc 1.70.0\nrelease: 1.70.0\n".to_vec()
                } else {
                    b"rustc 1.90.0\nrelease: 1.90.0\n".to_vec()
                }
            } else {
                Vec::new()
            };
            assert!(name != "cargo build" || spec.args_slice().iter().any(|arg| arg == "--locked"));
            if failed_build {
                return Err(UpdateError::new(
                    if matches!(self.failure, PreparationFailure::Locked) {
                        "cargo build failed: lock file needs to be updated"
                    } else {
                        "cargo build failed: compiler error"
                    },
                ));
            }
            Ok(ProcessExit {
                code: Some(0),
                stdout: ProcessStream::from_bytes(stdout),
                stderr: ProcessStream::from_bytes(Vec::new()),
            })
        }

        fn probe(&self, executable: &Path, _expected_sha: &str) -> Result<(), UpdateError> {
            let cli = executable
                .file_name()
                .is_some_and(|name| name == "tundra-cli.exe");
            if (cli && matches!(self.failure, PreparationFailure::CliProbe))
                || (!cli && matches!(self.failure, PreparationFailure::ShellProbe))
            {
                Err(UpdateError::new(
                    "compiled program reported the wrong update protocol or commit",
                ))
            } else {
                Ok(())
            }
        }
    }

    #[test]
    fn update_preparation_failures_never_touch_installation() {
        for (index, failure) in [
            PreparationFailure::MissingRustc,
            PreparationFailure::MissingCargo,
            PreparationFailure::RustcTooOld,
            PreparationFailure::Locked,
            PreparationFailure::Compile,
            PreparationFailure::MissingProduct,
            PreparationFailure::CliProbe,
            PreparationFailure::ShellProbe,
        ]
        .into_iter()
        .enumerate()
        {
            let root = update_test_root(&format!("prepare-failure-{index}"));
            let source = root.join("source");
            let target = root.join("target/release");
            let install = root.join("unrelated-install");
            fs::create_dir_all(source.join("assets/themes/default")).unwrap();
            fs::create_dir_all(&target).unwrap();
            fs::create_dir_all(&install).unwrap();
            fs::write(
                source.join("Cargo.toml"),
                "[workspace.package]\nrust-version = \"1.80\"\n",
            )
            .unwrap();
            fs::write(install.join("sentinel"), b"unchanged").unwrap();
            if !matches!(failure, PreparationFailure::MissingProduct) {
                fs::write(target.join("tundra-shell.exe"), b"shell").unwrap();
                fs::write(target.join("tundra-cli.exe"), b"cli").unwrap();
            }
            let check = UpdateCheckResult {
                default_branch: "master".to_owned(),
                head_sha: "target-sha".to_owned(),
                relation: UpdateRelation::Behind { remote_ahead: 1 },
                commits: Vec::new(),
            };
            let error = prepare_extracted_with_operations(
                &check,
                &mut |_| {},
                &root,
                &source,
                &FakePreparationOperations { failure },
            )
            .unwrap_err();
            assert!(!error.to_string().is_empty());
            assert_eq!(fs::read(install.join("sentinel")).unwrap(), b"unchanged");
            fs::remove_dir_all(root).unwrap();
        }
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

    #[cfg(windows)]
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum TransactionFailure {
        Assets,
        Cli,
        Shell,
        NewLaunch,
        ReadyTimeout,
        RestoredLaunch,
        None,
    }

    #[cfg(windows)]
    struct FakeTransactionOperations {
        failure: TransactionFailure,
        injected: Cell<bool>,
    }

    #[cfg(windows)]
    impl TransactionOperations for FakeTransactionOperations {
        fn rename(&self, source: &Path, target: &Path) -> Result<(), UpdateError> {
            if self.failure == TransactionFailure::Assets
                && !self.injected.get()
                && source.ends_with("new/default-assets")
            {
                self.injected.set(true);
                return Err(UpdateError::new("injected asset replacement failure"));
            }
            fs::rename(source, target).map_err(UpdateError::from)
        }

        fn replace(
            &self,
            target: &Path,
            replacement: &Path,
            backup: &Path,
        ) -> Result<(), UpdateError> {
            let failure = if target
                .file_name()
                .is_some_and(|name| name == "tundra-cli.exe")
            {
                TransactionFailure::Cli
            } else {
                TransactionFailure::Shell
            };
            if self.failure == failure
                && !self.injected.get()
                && replacement.to_string_lossy().contains("\\new\\")
            {
                self.injected.set(true);
                return Err(UpdateError::new("injected executable replacement failure"));
            }
            if target.exists() {
                fs::rename(target, backup)?;
            }
            fs::rename(replacement, target)?;
            Ok(())
        }

        fn launch_new_and_wait(
            &self,
            _paths: &TransactionPaths,
            _target_sha: &str,
        ) -> Result<(), UpdateError> {
            match self.failure {
                TransactionFailure::NewLaunch | TransactionFailure::RestoredLaunch => {
                    Err(UpdateError::new("injected new Shell launch failure"))
                }
                TransactionFailure::ReadyTimeout => Err(UpdateError::new(
                    "updated Shell did not become ready within 60 seconds",
                )),
                _ => Ok(()),
            }
        }

        fn launch_restored(&self, _shell: &Path, _reason: &str) -> Result<(), UpdateError> {
            if self.failure == TransactionFailure::RestoredLaunch {
                Err(UpdateError::new("injected restored Shell launch failure"))
            } else {
                Ok(())
            }
        }
    }

    #[cfg(windows)]
    fn transaction_fixture(name: &str) -> (PathBuf, PathBuf, TransactionManifest) {
        let root = update_test_root(name);
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
        let manifest_path = transaction_dir.join("transaction.json");
        let manifest = TransactionManifest {
            protocol: UPDATE_PROTOCOL_VERSION,
            target_sha: "abc".to_owned(),
            install_dir: install,
            transaction_dir,
            state: TransactionState::Prepared,
            assets_replaced: false,
            cli_replaced: false,
            shell_replaced: false,
        };
        write_manifest(&manifest_path, &manifest).unwrap();
        (root, manifest_path, manifest)
    }

    #[cfg(windows)]
    fn assert_old_install_preserved(manifest: &TransactionManifest) {
        assert_eq!(
            fs::read(manifest.install_dir.join("tundra-shell.exe")).unwrap(),
            b"old shell"
        );
        assert_eq!(
            fs::read(manifest.install_dir.join("tundra-cli.exe")).unwrap(),
            b"old cli"
        );
        assert_eq!(
            fs::read(manifest.install_dir.join("assets/themes/default/theme.txt")).unwrap(),
            b"old theme"
        );
        assert_eq!(
            fs::read(manifest.install_dir.join("assets/themes/custom/theme.txt")).unwrap(),
            b"custom theme"
        );
    }

    #[cfg(windows)]
    #[test]
    fn update_injected_transaction_failures_restore_every_installed_file() {
        for failure in [
            TransactionFailure::Assets,
            TransactionFailure::Cli,
            TransactionFailure::Shell,
            TransactionFailure::NewLaunch,
            TransactionFailure::ReadyTimeout,
            TransactionFailure::RestoredLaunch,
        ] {
            let (root, path, mut manifest) = transaction_fixture("injected-transaction");
            let operations = FakeTransactionOperations {
                failure,
                injected: Cell::new(false),
            };
            let result = run_update_transaction(&path, &mut manifest, false, &operations);
            if failure == TransactionFailure::RestoredLaunch {
                assert!(result.is_err());
                assert_eq!(
                    load_manifest(&path).unwrap().state,
                    TransactionState::Failed
                );
            } else {
                assert!(result.is_ok());
            }
            assert_old_install_preserved(&manifest);
            assert!(path.is_file(), "transaction journal must be retained");
            fs::remove_dir_all(root).unwrap();
        }
    }

    #[cfg(windows)]
    #[test]
    fn update_interrupted_journal_recovers_old_installation() {
        let (root, path, mut manifest) = transaction_fixture("interrupted");
        let operations = FakeTransactionOperations {
            failure: TransactionFailure::None,
            injected: Cell::new(false),
        };
        manifest.state = TransactionState::Applying;
        write_manifest(&path, &manifest).unwrap();
        apply_prepared_files_with_operations(&path, &mut manifest, &operations).unwrap();
        run_update_transaction(&path, &mut manifest, true, &operations).unwrap();
        assert_old_install_preserved(&manifest);
        assert_eq!(
            load_manifest(&path).unwrap().state,
            TransactionState::RolledBack
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(windows)]
    fn recovery_scan_fixture(name: &str, state: TransactionState) -> (PathBuf, PathBuf, PathBuf) {
        let root = update_test_root(name);
        let install = root.join("install");
        fs::create_dir_all(&install).unwrap();
        let canonical_install = fs::canonicalize(&install).unwrap();
        assert_ne!(install, canonical_install);
        let transaction_dir = canonical_install.join(".tundra-update/tx");
        fs::create_dir_all(&transaction_dir).unwrap();
        let manifest_path = transaction_dir.join("transaction.json");
        let manifest = TransactionManifest {
            protocol: UPDATE_PROTOCOL_VERSION,
            target_sha: "abc".to_owned(),
            install_dir: canonical_install,
            transaction_dir,
            state,
            assets_replaced: false,
            cli_replaced: false,
            shell_replaced: false,
        };
        write_manifest(&manifest_path, &manifest).unwrap();
        (root, install, manifest_path)
    }

    #[cfg(windows)]
    #[test]
    fn update_recovery_scan_canonicalizes_install_path_for_cleanup_and_recovery() {
        let (root, install, manifest_path) =
            recovery_scan_fixture("recovery-scan-committed", TransactionState::Committed);
        let launches = std::sync::Mutex::new(Vec::new());
        assert!(
            !scan_update_recovery(&install, 41, &|path, pid, recover_only| {
                launches
                    .lock()
                    .unwrap()
                    .push((path.to_owned(), pid, recover_only));
                Ok(())
            })
            .unwrap()
        );
        assert!(!manifest_path.parent().unwrap().exists());
        assert!(launches.lock().unwrap().is_empty());
        fs::remove_dir_all(root).unwrap();

        for state in [TransactionState::Applying, TransactionState::AwaitingReady] {
            let (root, install, manifest_path) = recovery_scan_fixture("recovery-scan", state);
            let launches = std::sync::Mutex::new(Vec::new());
            assert!(
                scan_update_recovery(&install, 42, &|path, pid, recover_only| {
                    launches
                        .lock()
                        .unwrap()
                        .push((path.to_owned(), pid, recover_only));
                    Ok(())
                })
                .unwrap()
            );
            assert_eq!(*launches.lock().unwrap(), vec![(manifest_path, 42, true)]);
            fs::remove_dir_all(root).unwrap();
        }
    }
}
