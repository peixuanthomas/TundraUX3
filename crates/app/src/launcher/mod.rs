//! Domain logic for the globally-managed Launcher application.
//!
//! The module never opens a file itself. After revalidating an approved entry,
//! it returns an effect for the shell to perform the platform operation.

use std::fmt;
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use identity::{AuthSession, PermissionAction, PermissionService};
use platform::{ExecutableKind as PlatformExecutableKind, FileOpenPolicy, Platform};
use sha2::{Digest, Sha256};
use storage::{
    LauncherConfig, LauncherEntryRecord, LauncherExecutableKind, LauncherFingerprint, StorageError,
    StorageManager,
};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum LauncherViewMode {
    #[default]
    LargeIcons,
    Details,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LauncherItemStatus {
    Ready,
    Checking,
    Changed,
    Missing,
    NeedsApproval,
    Unsupported,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LauncherItem {
    pub record: LauncherEntryRecord,
    pub status: LauncherItemStatus,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LauncherState {
    pub items: Vec<LauncherItem>,
    pub message: Option<String>,
    pub error: Option<String>,
}

impl LauncherState {
    pub fn from_config(config: &LauncherConfig) -> Self {
        Self {
            items: config
                .entries
                .iter()
                .cloned()
                .map(|record| LauncherItem {
                    status: if record.fingerprint.is_some() && record.executable_kind.is_some() {
                        LauncherItemStatus::Checking
                    } else {
                        LauncherItemStatus::NeedsApproval
                    },
                    record,
                })
                .collect(),
            message: None,
            error: None,
        }
    }

    fn reset(&mut self, config: &LauncherConfig) {
        *self = Self::from_config(config);
    }

    pub fn set_item_status(&mut self, id: &str, status: LauncherItemStatus) {
        if let Some(item) = self.items.iter_mut().find(|item| item.record.id == id) {
            item.status = status;
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LauncherAddOutcome {
    Added { id: String },
    Duplicate,
    Rejected { reason: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LauncherAddResult {
    pub path: PathBuf,
    pub outcome: LauncherAddOutcome,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LauncherEffect {
    None,
    Added(Vec<LauncherAddResult>),
    OpenRequested {
        path: PathBuf,
    },
    ConfirmationRequired {
        id: String,
        path: PathBuf,
        kind: LauncherExecutableKind,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LauncherCommand {
    Refresh,
    AddPaths(Vec<PathBuf>),
    Reorder { id: String, insertion_index: usize },
    Remove(Vec<String>),
    Reapprove(Vec<String>),
    RequestLaunch(String),
    ConfirmLaunch(String),
}

#[derive(Debug)]
pub enum LauncherError {
    PermissionDenied(String),
    InvalidPath { path: PathBuf, reason: String },
    Io { path: PathBuf, message: String },
    Platform(String),
    Storage(StorageError),
    MissingEntry(String),
}

impl fmt::Display for LauncherError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PermissionDenied(reason) => write!(formatter, "permission denied: {reason}"),
            Self::InvalidPath { path, reason } => write!(
                formatter,
                "invalid Launcher path {}: {reason}",
                path.display()
            ),
            Self::Io { path, message } => {
                write!(formatter, "could not read {}: {message}", path.display())
            }
            Self::Platform(message) => formatter.write_str(message),
            Self::Storage(error) => error.fmt(formatter),
            Self::MissingEntry(id) => write!(formatter, "Launcher entry is missing: {id}"),
        }
    }
}

impl std::error::Error for LauncherError {}
impl From<StorageError> for LauncherError {
    fn from(value: StorageError) -> Self {
        Self::Storage(value)
    }
}

/// Owns Launcher authorization and persistence. Configuration mutations are
/// saved exactly once after a batch is completely evaluated.
#[derive(Debug, Clone, Default)]
pub struct LauncherController {
    permissions: PermissionService,
}

impl LauncherController {
    pub fn new(permissions: PermissionService) -> Self {
        Self { permissions }
    }

    pub fn load(&self, storage: &StorageManager) -> Result<LauncherState, LauncherError> {
        Ok(LauncherState::from_config(&storage.load_config()?.launcher))
    }

    pub fn apply(
        &self,
        state: &mut LauncherState,
        command: LauncherCommand,
        session: Option<&AuthSession>,
        platform: &dyn Platform,
        storage: &StorageManager,
    ) -> LauncherEffect {
        state.error = None;
        let result = match command {
            LauncherCommand::Refresh => self.refresh(state, platform),
            LauncherCommand::AddPaths(paths) => self.add(state, paths, session, platform, storage),
            LauncherCommand::Reorder {
                id,
                insertion_index,
            } => self.reorder(state, &id, insertion_index, session, storage),
            LauncherCommand::Remove(ids) => self.remove(state, &ids, session, storage),
            LauncherCommand::Reapprove(ids) => {
                self.reapprove(state, &ids, session, platform, storage)
            }
            LauncherCommand::RequestLaunch(id) => {
                self.launch(state, &id, session, platform, storage, false)
            }
            LauncherCommand::ConfirmLaunch(id) => {
                self.launch(state, &id, session, platform, storage, true)
            }
        };
        match result {
            Ok(effect) => effect,
            Err(error) => {
                state.error = Some(error.to_string());
                LauncherEffect::None
            }
        }
    }

    fn refresh(
        &self,
        state: &mut LauncherState,
        platform: &dyn Platform,
    ) -> Result<LauncherEffect, LauncherError> {
        for item in &mut state.items {
            item.status = verify(&item.record, platform)?;
        }
        Ok(LauncherEffect::None)
    }

    fn add(
        &self,
        state: &mut LauncherState,
        paths: Vec<PathBuf>,
        session: Option<&AuthSession>,
        platform: &dyn Platform,
        storage: &StorageManager,
    ) -> Result<LauncherEffect, LauncherError> {
        self.authorize(session, PermissionAction::ManageLauncher)?;
        let actor = session.expect("ManageLauncher requires an authenticated admin");
        let mut config = storage.load_config()?;
        let mut results = Vec::with_capacity(paths.len());
        let mut save = false;
        for supplied in paths {
            match self.new_record(&config.launcher, &supplied, actor, platform) {
                Ok(Some(record)) => {
                    let id = record.id.clone();
                    config.launcher.entries.push(record);
                    results.push(LauncherAddResult {
                        path: supplied,
                        outcome: LauncherAddOutcome::Added { id },
                    });
                    save = true;
                }
                Ok(None) => results.push(LauncherAddResult {
                    path: supplied,
                    outcome: LauncherAddOutcome::Duplicate,
                }),
                Err(error) => results.push(LauncherAddResult {
                    path: supplied,
                    outcome: LauncherAddOutcome::Rejected {
                        reason: error.to_string(),
                    },
                }),
            }
        }
        if save {
            storage.save_config(&config)?;
            state.reset(&config.launcher);
            self.refresh(state, platform)?;
        }
        let count = results
            .iter()
            .filter(|result| matches!(result.outcome, LauncherAddOutcome::Added { .. }))
            .count();
        state.message = Some(format!("{count} Launcher item(s) added"));
        Ok(LauncherEffect::Added(results))
    }

    fn reorder(
        &self,
        state: &mut LauncherState,
        id: &str,
        insertion_index: usize,
        session: Option<&AuthSession>,
        storage: &StorageManager,
    ) -> Result<LauncherEffect, LauncherError> {
        self.authorize(session, PermissionAction::ManageLauncher)?;
        let mut config = storage.load_config()?;
        let source_index = config
            .launcher
            .entries
            .iter()
            .position(|entry| entry.id == id)
            .ok_or_else(|| LauncherError::MissingEntry(id.to_string()))?;
        let boundary = insertion_index.min(config.launcher.entries.len());
        let destination_index = if source_index < boundary {
            boundary.saturating_sub(1)
        } else {
            boundary
        }
        .min(config.launcher.entries.len().saturating_sub(1));

        if destination_index == source_index {
            state.message = Some("Launcher item order unchanged".to_string());
            return Ok(LauncherEffect::None);
        }

        let entry = config.launcher.entries.remove(source_index);
        config.launcher.entries.insert(destination_index, entry);
        storage.save_config(&config)?;

        if let Some(current_index) = state.items.iter().position(|item| item.record.id == id) {
            let item = state.items.remove(current_index);
            state
                .items
                .insert(destination_index.min(state.items.len()), item);
        } else {
            state.reset(&config.launcher);
        }
        state.message = Some("Launcher item moved".to_string());
        Ok(LauncherEffect::None)
    }

    fn remove(
        &self,
        state: &mut LauncherState,
        ids: &[String],
        session: Option<&AuthSession>,
        storage: &StorageManager,
    ) -> Result<LauncherEffect, LauncherError> {
        self.authorize(session, PermissionAction::ManageLauncher)?;
        let mut config = storage.load_config()?;
        let before = config.launcher.entries.len();
        config
            .launcher
            .entries
            .retain(|entry| !ids.iter().any(|id| id == &entry.id));
        let removed = before - config.launcher.entries.len();
        if removed > 0 {
            storage.save_config(&config)?;
            state.reset(&config.launcher);
        }
        state.message = Some(format!("{removed} Launcher item(s) removed"));
        Ok(LauncherEffect::None)
    }

    fn reapprove(
        &self,
        state: &mut LauncherState,
        ids: &[String],
        session: Option<&AuthSession>,
        platform: &dyn Platform,
        storage: &StorageManager,
    ) -> Result<LauncherEffect, LauncherError> {
        self.authorize(session, PermissionAction::ManageLauncher)?;
        let mut config = storage.load_config()?;
        let mut count = 0usize;
        for entry in &mut config.launcher.entries {
            if ids.iter().any(|id| id == &entry.id) {
                let target = validate(Path::new(&entry.path), platform)?;
                entry.path = target.path.to_string_lossy().into_owned();
                entry.executable_kind = Some(target.kind);
                entry.fingerprint = Some(fingerprint(&target.path, target.kind)?);
                count += 1;
            }
        }
        if count > 0 {
            storage.save_config(&config)?;
            state.reset(&config.launcher);
            self.refresh(state, platform)?;
        }
        state.message = Some(format!("{count} Launcher item(s) re-approved"));
        Ok(LauncherEffect::None)
    }

    fn launch(
        &self,
        state: &mut LauncherState,
        id: &str,
        session: Option<&AuthSession>,
        platform: &dyn Platform,
        storage: &StorageManager,
        confirmed: bool,
    ) -> Result<LauncherEffect, LauncherError> {
        self.authorize(session, PermissionAction::OpenExternal)?;
        let config = storage.load_config()?;
        let entry = config
            .launcher
            .entries
            .iter()
            .find(|entry| entry.id == id)
            .ok_or_else(|| LauncherError::MissingEntry(id.to_string()))?;
        let status = verify(entry, platform)?;
        state.set_item_status(id, status);
        if status != LauncherItemStatus::Ready {
            return Ok(LauncherEffect::None);
        }
        let kind = entry
            .executable_kind
            .expect("a Ready Launcher entry has a kind");
        let path = PathBuf::from(&entry.path);
        if !confirmed && needs_confirmation(kind) {
            return Ok(LauncherEffect::ConfirmationRequired {
                id: id.to_string(),
                path,
                kind,
            });
        }
        Ok(LauncherEffect::OpenRequested { path })
    }

    fn new_record(
        &self,
        config: &LauncherConfig,
        path: &Path,
        actor: &AuthSession,
        platform: &dyn Platform,
    ) -> Result<Option<LauncherEntryRecord>, LauncherError> {
        let target = validate(path, platform)?;
        if config
            .entries
            .iter()
            .any(|entry| same_path(Path::new(&entry.path), &target.path))
        {
            return Ok(None);
        }
        let fingerprint = fingerprint(&target.path, target.kind)?;
        Ok(Some(LauncherEntryRecord {
            id: new_id(&target.path, &fingerprint, config),
            path: target.path.to_string_lossy().into_owned(),
            executable_kind: Some(target.kind),
            fingerprint: Some(fingerprint),
            added_by_user_id: actor.user_id.clone(),
            added_at_epoch_ms: epoch_millis(),
        }))
    }

    fn authorize(
        &self,
        session: Option<&AuthSession>,
        action: PermissionAction,
    ) -> Result<(), LauncherError> {
        let result = self.permissions.authorize(session, action, None);
        if result.allowed {
            Ok(())
        } else {
            Err(LauncherError::PermissionDenied(
                result
                    .reason
                    .unwrap_or_else(|| "not authorized".to_string()),
            ))
        }
    }
}

struct Target {
    path: PathBuf,
    kind: LauncherExecutableKind,
}

fn validate(path: &Path, platform: &dyn Platform) -> Result<Target, LauncherError> {
    if !path.is_absolute() {
        return Err(invalid(path, "Launcher paths must be absolute"));
    }
    let link = fs::symlink_metadata(path).map_err(|error| io_error(path, error))?;
    if link.file_type().is_symlink() {
        return Err(invalid(path, "symbolic links are not allowed"));
    }
    let canonical = fs::canonicalize(path).map_err(|error| io_error(path, error))?;
    let attributes = platform
        .file_attributes(&canonical)
        .map_err(|error| LauncherError::Platform(error.to_string()))?;
    if attributes.symlink || attributes.junction || attributes.reparse_point {
        return Err(invalid(
            path,
            "symbolic links, junctions, and reparse points are not allowed",
        ));
    }
    let kind = match platform.file_open_policy(&canonical, &attributes) {
        FileOpenPolicy::LauncherRequired { kind, .. } => convert_kind(kind),
        FileOpenPolicy::SystemDefault => {
            return Err(invalid(
                path,
                "only files requiring Launcher approval can be added",
            ));
        }
        FileOpenPolicy::Blocked { reason } => return Err(invalid(path, reason)),
    };
    if !attributes.is_file
        && !(attributes.is_dir && kind == LauncherExecutableKind::ApplicationBundle)
    {
        return Err(invalid(
            path,
            "entries must be files or application bundles",
        ));
    }
    Ok(Target {
        path: canonical,
        kind,
    })
}

fn verify(
    entry: &LauncherEntryRecord,
    platform: &dyn Platform,
) -> Result<LauncherItemStatus, LauncherError> {
    let (Some(expected), Some(kind)) = (&entry.fingerprint, entry.executable_kind) else {
        return Ok(LauncherItemStatus::NeedsApproval);
    };
    let target = match validate(Path::new(&entry.path), platform) {
        Ok(target) => target,
        Err(LauncherError::Io { .. }) => return Ok(LauncherItemStatus::Missing),
        Err(_) => return Ok(LauncherItemStatus::Unsupported),
    };
    if target.kind != kind {
        return Ok(LauncherItemStatus::Changed);
    }
    match fingerprint(&target.path, kind) {
        Ok(actual) if actual == *expected => Ok(LauncherItemStatus::Ready),
        Ok(_) => Ok(LauncherItemStatus::Changed),
        Err(LauncherError::Io { .. }) => Ok(LauncherItemStatus::Missing),
        Err(_) => Ok(LauncherItemStatus::Unsupported),
    }
}

/// Revalidates one persisted Launcher entry without mutating application state.
///
/// Shell runtimes use this entry-level API to perform the full content digest on
/// a worker thread and publish results back to the UI incrementally. Launch
/// authorization still calls the same verifier immediately before opening a
/// target, so moving list refreshes off the UI thread does not weaken integrity
/// checks.
pub fn verify_launcher_entry(
    entry: &LauncherEntryRecord,
    platform: &dyn Platform,
) -> Result<LauncherItemStatus, LauncherError> {
    verify(entry, platform)
}

/// Full SHA-256 of a regular, non-link file. Modification time is informational;
/// it is never used in place of the content digest during execution approval.
pub fn fingerprint_file(path: &Path) -> Result<LauncherFingerprint, LauncherError> {
    let metadata = fs::symlink_metadata(path).map_err(|error| io_error(path, error))?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(invalid(path, "fingerprints require regular non-link files"));
    }
    let mut file = fs::File::open(path).map_err(|error| io_error(path, error))?;
    let mut hash = Sha256::new();
    let mut buffer = [0u8; 262_144];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|error| io_error(path, error))?;
        if read == 0 {
            break;
        }
        hash.update(&buffer[..read]);
    }
    Ok(LauncherFingerprint {
        sha256: hex(hash.finalize().as_slice()),
        byte_len: metadata.len(),
        modified_at_epoch_ms: metadata.modified().ok().map(time_millis),
    })
}

/// Conservative bundle identity for macOS `.app` directories. It includes
/// `Contents/Info.plist` and every direct regular child in `Contents/MacOS`.
/// Nested bundle content is not covered by this v1 helper; bundle upgrades must
/// therefore be re-approved before launch.
pub fn fingerprint_application_bundle(path: &Path) -> Result<LauncherFingerprint, LauncherError> {
    let root = fs::canonicalize(path).map_err(|error| io_error(path, error))?;
    let macos = root.join("Contents").join("MacOS");
    let mut inputs = vec![root.join("Contents").join("Info.plist")];
    for item in fs::read_dir(&macos).map_err(|error| io_error(&macos, error))? {
        let child = item.map_err(|error| io_error(&macos, error))?.path();
        let metadata = fs::symlink_metadata(&child).map_err(|error| io_error(&child, error))?;
        if metadata.file_type().is_symlink() {
            return Err(invalid(&child, "bundle contains a symbolic link"));
        }
        if metadata.is_file() {
            inputs.push(child);
        }
    }
    inputs.sort();
    let mut hash = Sha256::new();
    let mut length = 0u64;
    let mut modified = None;
    for input in inputs {
        let part = fingerprint_file(&input)?;
        hash.update(
            input
                .strip_prefix(&root)
                .unwrap_or(&input)
                .to_string_lossy()
                .as_bytes(),
        );
        hash.update(part.sha256.as_bytes());
        length = length.saturating_add(part.byte_len);
        modified = modified.max(part.modified_at_epoch_ms);
    }
    Ok(LauncherFingerprint {
        sha256: hex(hash.finalize().as_slice()),
        byte_len: length,
        modified_at_epoch_ms: modified,
    })
}

fn fingerprint(
    path: &Path,
    kind: LauncherExecutableKind,
) -> Result<LauncherFingerprint, LauncherError> {
    if kind == LauncherExecutableKind::ApplicationBundle {
        fingerprint_application_bundle(path)
    } else {
        fingerprint_file(path)
    }
}
fn convert_kind(kind: PlatformExecutableKind) -> LauncherExecutableKind {
    match kind {
        PlatformExecutableKind::NativeBinary => LauncherExecutableKind::NativeBinary,
        PlatformExecutableKind::Installer => LauncherExecutableKind::Installer,
        PlatformExecutableKind::Script => LauncherExecutableKind::Script,
        PlatformExecutableKind::Shortcut => LauncherExecutableKind::Shortcut,
        PlatformExecutableKind::ApplicationBundle => LauncherExecutableKind::ApplicationBundle,
    }
}
fn needs_confirmation(kind: LauncherExecutableKind) -> bool {
    matches!(
        kind,
        LauncherExecutableKind::Script
            | LauncherExecutableKind::Installer
            | LauncherExecutableKind::Shortcut
    )
}
fn same_path(left: &Path, right: &Path) -> bool {
    #[cfg(windows)]
    {
        return left
            .to_string_lossy()
            .eq_ignore_ascii_case(&right.to_string_lossy());
    }
    #[cfg(not(windows))]
    {
        left == right
    }
}
fn epoch_millis() -> i64 {
    time_millis(SystemTime::now())
}
fn time_millis(time: SystemTime) -> i64 {
    time.duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0)
}
fn invalid(path: &Path, reason: impl Into<String>) -> LauncherError {
    LauncherError::InvalidPath {
        path: path.to_path_buf(),
        reason: reason.into(),
    }
}
fn io_error(path: &Path, error: io::Error) -> LauncherError {
    LauncherError::Io {
        path: path.to_path_buf(),
        message: error.to_string(),
    }
}
fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(DIGITS[(byte >> 4) as usize] as char);
        out.push(DIGITS[(byte & 15) as usize] as char);
    }
    out
}
fn new_id(path: &Path, fingerprint: &LauncherFingerprint, config: &LauncherConfig) -> String {
    let mut hash = Sha256::new();
    hash.update(path.to_string_lossy().as_bytes());
    hash.update(fingerprint.sha256.as_bytes());
    hash.update(epoch_millis().to_le_bytes());
    let base = format!("launcher-{}", hex(hash.finalize().as_slice()));
    if !config.entries.iter().any(|entry| entry.id == base) {
        return base;
    }
    for suffix in 2u32.. {
        let candidate = format!("{base}-{suffix}");
        if !config.entries.iter().any(|entry| entry.id == candidate) {
            return candidate;
        }
    }
    unreachable!("unbounded ID suffix")
}
