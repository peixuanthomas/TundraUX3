use std::fmt;
use std::fs::{self, File};
use std::io::{self, Cursor};
use std::path::{Component, Path, PathBuf};
use std::time::Duration;

use platform::{Platform, PlatformKind, ProcessExit, ProcessSpec};
use reqwest::blocking::{Client, Response};
use semver::Version;
use serde::Deserialize;

pub const UPDATE_PROTOCOL_VERSION: u32 = 1;
pub const GITHUB_OWNER: &str = "peixuanthomas";
pub const GITHUB_REPO: &str = "TundraUX3";
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
}

impl UpdateError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
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

#[derive(Deserialize)]
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
        fetch_comparison(&client, local, &branch.commit.sha)?
    } else {
        let values: Vec<ApiCommit> = get_json(
            &client,
            &format!(
                "{API_ROOT}/repos/{GITHUB_OWNER}/{GITHUB_REPO}/commits?sha={}&per_page=20",
                repository.default_branch
            ),
        )?;
        (UpdateRelation::Unknown, map_commits(values))
    };
    Ok(UpdateCheckResult {
        default_branch: repository.default_branch,
        head_sha: branch.commit.sha,
        relation,
        commits,
    })
}

fn fetch_comparison(
    client: &Client,
    base: &str,
    head: &str,
) -> Result<(UpdateRelation, Vec<UpdateCommit>), UpdateError> {
    let mut page = 1;
    let mut all = Vec::new();
    let mut relation = None;
    loop {
        let url = format!(
            "{API_ROOT}/repos/{GITHUB_OWNER}/{GITHUB_REPO}/compare/{base}...{head}?per_page=100&page={page}"
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
        Err(UpdateError::new(
            "GitHub API rate limit exceeded; try again after the limit resets",
        ))
    } else {
        Err(UpdateError::new(format!(
            "GitHub returned HTTP {status}: {}",
            tail(&body, 512)
        )))
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
            validate_products(&root, &root.join("source"), &root.join("target"), "abc").is_err()
        );
        fs::write(root.join("target/release/tundra-cli.exe"), b"cli").unwrap();
        assert!(
            validate_products(&root, &root.join("source"), &root.join("target"), "abc").is_ok()
        );
        fs::remove_dir_all(root).unwrap();
    }
}
