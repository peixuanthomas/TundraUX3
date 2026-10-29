//! Domain logic for the globally-managed Launcher application.
//!
//! After checking that an approved entry still exists at its recorded path,
//! it returns an effect for the shell to perform the platform operation.

use i18n::{LocalizedError, LocalizedText, msg};

use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use identity::{AuthSession, PermissionAction, PermissionService};
use platform::{ExecutableKind as PlatformExecutableKind, FileOpenPolicy, Platform};
use sha2::{Digest, Sha256};
use storage::{
    LauncherConfig, LauncherEntryRecord, LauncherExecutableKind, LauncherTerminalSettings,
    StorageError, StorageManager,
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

impl LauncherItemStatus {
    pub fn localized_label(self) -> LocalizedText {
        match self {
            Self::Ready => msg!("app-launcher-ready"),
            Self::Checking => msg!("app-launcher-checking"),
            Self::Changed => msg!("app-launcher-changed"),
            Self::Missing => msg!("app-launcher-missing"),
            Self::NeedsApproval => msg!("app-launcher-needs-approval"),
            Self::Unsupported => msg!("app-launcher-unsupported"),
        }
        .into()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LauncherItem {
    pub record: LauncherEntryRecord,
    pub status: LauncherItemStatus,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LauncherState {
    pub items: Vec<LauncherItem>,
    pub message: Option<LocalizedText>,
    pub error: Option<LocalizedText>,
}

impl LauncherState {
    pub fn from_config(config: &LauncherConfig) -> Self {
        Self {
            items: config
                .entries
                .iter()
                .cloned()
                .map(|record| LauncherItem {
                    status: if record.custom.is_some() {
                        LauncherItemStatus::Ready
                    } else if record.executable_kind.is_some() {
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
    Rejected { reason: LocalizedText },
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
        kind: PlatformExecutableKind,
    },
    OpenTerminalRequested {
        path: PathBuf,
        args: Vec<String>,
        directory: Option<PathBuf>,
        /// None preserves the existing exit handling for approved executables.
        wait_for_enter: Option<bool>,
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
    AddCustom(storage::LauncherCustomCommand),
    EditEntry {
        id: String,
        edit: LauncherEntryEdit,
    },
    SetOrder(Vec<String>),
    Reorder {
        id: String,
        insertion_index: usize,
    },
    Remove(Vec<String>),
    SetTerminalSettings {
        id: String,
        settings: Option<LauncherTerminalSettings>,
    },
    RequestLaunch(String),
    ConfirmLaunch(String),
}

#[derive(Debug)]
pub enum LauncherError {
    Localized(LocalizedError),
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
            Self::Localized(error) => formatter.write_str(&i18n::render_diagnostic(&error.message)),
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

impl LauncherError {
    pub fn localized(&self) -> LocalizedError {
        match self {
            Self::Localized(error) => error.clone(),
            Self::PermissionDenied(reason) => LocalizedError::new(
                "LAUNCHER_PERMISSION_DENIED",
                msg!("app-launcher-permission-denied", reason = reason.clone()),
            ),
            Self::InvalidPath { path, reason } => LocalizedError::new(
                "LAUNCHER_INVALID_PATH",
                msg!(
                    "app-launcher-invalid-path",
                    path = path.display().to_string(),
                    reason = reason.clone()
                ),
            ),
            Self::Io { path, message } => LocalizedError::new(
                "LAUNCHER_IO",
                msg!(
                    "app-launcher-io",
                    path = path.display().to_string(),
                    detail = message.clone()
                ),
            ),
            Self::Platform(detail) => LocalizedError::new(
                "LAUNCHER_PLATFORM",
                msg!("app-launcher-platform-error", detail = detail.clone()),
            ),
            Self::Storage(error) => LocalizedError::new(
                "LAUNCHER_STORAGE",
                msg!("app-launcher-storage-error", detail = error.to_string()),
            ),
            Self::MissingEntry(id) => LocalizedError::new(
                "LAUNCHER_MISSING_ENTRY",
                msg!("app-launcher-missing-entry", id = id.clone()),
            ),
        }
    }
}

impl std::error::Error for LauncherError {}
impl From<StorageError> for LauncherError {
    fn from(value: StorageError) -> Self {
        Self::Storage(value)
    }
}

pub const LAUNCHER_ICON_WIDTH: usize = 5;
pub const LAUNCHER_ICON_HEIGHT: usize = 2;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LauncherEntryEdit {
    Name(String),
    Command(String),
    Icon(String),
}

pub fn validate_launcher_icon(icon: &str) -> bool {
    use unicode_width::UnicodeWidthStr;
    icon.split('\n').count() <= LAUNCHER_ICON_HEIGHT
        && icon
            .split('\n')
            .all(|line| !line.chars().any(char::is_control) && line.width() <= LAUNCHER_ICON_WIDTH)
}

pub fn launcher_entry_name(entry: &LauncherEntryRecord) -> &str {
    entry.name.as_deref().unwrap_or_else(|| {
        entry.custom.as_ref().map_or_else(
            || {
                Path::new(&entry.path)
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or(&entry.path)
            },
            |custom| custom.name.as_str(),
        )
    })
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
            LauncherCommand::AddCustom(custom) => self.add_custom(state, custom, session, storage),
            LauncherCommand::EditEntry { id, edit } => {
                self.edit_entry(state, &id, edit, session, storage)
            }
            LauncherCommand::SetOrder(order) => self.set_order(order, session, storage),
            LauncherCommand::AddPaths(paths) => self.add(state, paths, session, platform, storage),
            LauncherCommand::Reorder {
                id,
                insertion_index,
            } => self.reorder(state, &id, insertion_index, session, storage),
            LauncherCommand::Remove(ids) => self.remove(state, &ids, session, storage),
            LauncherCommand::SetTerminalSettings { id, settings } => {
                self.set_terminal_settings(state, &id, settings, session, platform, storage)
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
                state.error = Some(error.localized().message.into());
                LauncherEffect::None
            }
        }
    }

    fn add_custom(
        &self,
        state: &mut LauncherState,
        custom: storage::LauncherCustomCommand,
        session: Option<&AuthSession>,
        storage: &StorageManager,
    ) -> Result<LauncherEffect, LauncherError> {
        self.authorize(session, PermissionAction::ManageLauncher)?;
        self.authorize(session, PermissionAction::ExecuteCommandLine)?;
        if custom.name.trim().is_empty() || custom.command.trim().is_empty() {
            return Err(LauncherError::Localized(LocalizedError::new(
                "UX_LAUNCH_INVALID_COMMAND",
                msg!("app-launcher-custom-required"),
            )));
        }
        if !validate_launcher_icon(&custom.icon) {
            return Err(LauncherError::Localized(LocalizedError::new(
                "LAUNCHER_INVALID_ICON",
                msg!("app-launcher-icon-bounds"),
            )));
        }
        let mut config = storage.load_config()?;
        let id = new_id(Path::new(&custom.command), &config.launcher);
        config.launcher.entries.push(LauncherEntryRecord {
            id,
            custom: Some(custom),
            added_by_user_id: session.unwrap().user_id.clone(),
            added_at_epoch_ms: epoch_millis(),
            ..Default::default()
        });
        storage.save_config(&config)?;
        state.reset(&config.launcher);
        Ok(LauncherEffect::None)
    }

    fn edit_entry(
        &self,
        state: &mut LauncherState,
        id: &str,
        edit: LauncherEntryEdit,
        session: Option<&AuthSession>,
        storage: &StorageManager,
    ) -> Result<LauncherEffect, LauncherError> {
        self.authorize(session, PermissionAction::ManageLauncher)?;
        let mut config = storage.load_config()?;
        let entry = config
            .launcher
            .entries
            .iter_mut()
            .find(|entry| entry.id == id)
            .ok_or_else(|| LauncherError::MissingEntry(id.to_owned()))?;
        match edit {
            LauncherEntryEdit::Name(name) => {
                if name.trim().is_empty() || name.chars().any(char::is_control) {
                    return Err(LauncherError::Localized(LocalizedError::new(
                        "LAUNCHER_INVALID_NAME",
                        msg!("app-launcher-custom-required"),
                    )));
                }
                if let Some(custom) = &mut entry.custom {
                    custom.name = name;
                    entry.name = None;
                } else {
                    entry.name = Some(name);
                }
            }
            LauncherEntryEdit::Command(command) => {
                self.authorize(session, PermissionAction::ExecuteCommandLine)?;
                if command.trim().is_empty() || command.contains('\0') {
                    return Err(LauncherError::Localized(LocalizedError::new(
                        "LAUNCHER_INVALID_COMMAND",
                        msg!("app-launcher-custom-required"),
                    )));
                }
                if entry.custom.is_none() {
                    entry.custom = Some(storage::LauncherCustomCommand {
                        name: launcher_entry_name(entry).to_owned(),
                        icon: entry.icon.take().unwrap_or_default(),
                        ..Default::default()
                    });
                    entry.name = None;
                }
                entry.custom.as_mut().unwrap().command = command;
                entry.executable_kind = None;
            }
            LauncherEntryEdit::Icon(icon) => {
                if !validate_launcher_icon(&icon) {
                    return Err(LauncherError::Localized(LocalizedError::new(
                        "LAUNCHER_INVALID_ICON",
                        msg!("app-launcher-icon-bounds"),
                    )));
                }
                if let Some(custom) = &mut entry.custom {
                    custom.icon = icon;
                    entry.icon = None;
                } else {
                    entry.icon = Some(icon);
                }
            }
        }
        let saved = entry.clone();
        storage.save_config(&config)?;
        if let Some(item) = state.items.iter_mut().find(|item| item.record.id == id) {
            item.record = saved;
            if item.record.custom.is_some() {
                item.status = LauncherItemStatus::Ready;
            }
        }
        state.message = Some(msg!("app-launcher-entry-saved").into());
        Ok(LauncherEffect::None)
    }

    fn set_order(
        &self,
        order: Vec<String>,
        session: Option<&AuthSession>,
        storage: &StorageManager,
    ) -> Result<LauncherEffect, LauncherError> {
        self.authorize(session, PermissionAction::ManageLauncher)?;
        let mut config = storage.load_config()?;
        config.launcher.order = order;
        storage.save_config(&config)?;
        Ok(LauncherEffect::None)
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
                        reason: error.localized().message.into(),
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
        state.message = Some(msg!("app-launcher-added", count = count as i64).into());
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
            state.message = Some(msg!("app-launcher-order-unchanged").into());
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
        state.message = Some(msg!("app-launcher-moved").into());
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
        state.message = Some(msg!("app-launcher-removed", count = removed as i64).into());
        Ok(LauncherEffect::None)
    }

    fn set_terminal_settings(
        &self,
        state: &mut LauncherState,
        id: &str,
        settings: Option<LauncherTerminalSettings>,
        session: Option<&AuthSession>,
        platform: &dyn Platform,
        storage: &StorageManager,
    ) -> Result<LauncherEffect, LauncherError> {
        self.authorize(session, PermissionAction::ManageLauncher)?;
        let mut config = storage.load_config()?;
        let entry = config
            .launcher
            .entries
            .iter_mut()
            .find(|entry| entry.id == id)
            .ok_or_else(|| LauncherError::MissingEntry(id.to_string()))?;
        if let Some(settings) = &settings {
            validate_terminal_settings(entry, settings, platform)?;
        }
        entry.terminal = settings;
        let saved = entry.clone();
        storage.save_config(&config)?;
        if let Some(item) = state.items.iter_mut().find(|item| item.record.id == id) {
            item.record = saved;
        } else {
            state.reset(&config.launcher);
        }
        state.message = Some(msg!("app-launcher-terminal-settings-saved").into());
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
        if let Some(custom) = &entry.custom {
            self.authorize(session, PermissionAction::ExecuteCommandLine)?;
            let (path, args) = if cfg!(windows) {
                (
                    PathBuf::from("cmd.exe"),
                    vec!["/C".into(), custom.command.clone()],
                )
            } else {
                (
                    PathBuf::from("/bin/sh"),
                    vec!["-c".into(), custom.command.clone()],
                )
            };
            return Ok(LauncherEffect::OpenTerminalRequested {
                path,
                args,
                directory: entry
                    .terminal
                    .as_ref()
                    .and_then(|settings| settings.working_directory.as_deref().map(PathBuf::from)),
                wait_for_enter: Some(custom.wait_for_enter),
            });
        }
        let status = verify(entry, platform)?;
        state.set_item_status(id, status);
        if status != LauncherItemStatus::Ready {
            return Ok(LauncherEffect::None);
        }
        let kind = entry
            .executable_kind
            .expect("a Ready Launcher entry has a kind");
        let path = PathBuf::from(&entry.path);
        if let Some(settings) = &entry.terminal {
            self.authorize(session, PermissionAction::ExecuteCommandLine)?;
            validate_terminal_settings(entry, settings, platform)?;
        }
        if !confirmed && needs_confirmation(kind) {
            return Ok(LauncherEffect::ConfirmationRequired {
                id: id.to_string(),
                path,
                kind,
            });
        }
        if let Some(settings) = &entry.terminal {
            return Ok(LauncherEffect::OpenTerminalRequested {
                path,
                args: settings.arguments.clone(),
                directory: settings
                    .working_directory
                    .as_deref()
                    .filter(|directory| !directory.is_empty())
                    .map(PathBuf::from),
                wait_for_enter: None,
            });
        }
        Ok(LauncherEffect::OpenRequested {
            path,
            kind: platform_kind(kind),
        })
    }

    fn new_record(
        &self,
        config: &LauncherConfig,
        path: &Path,
        actor: &AuthSession,
        platform: &dyn Platform,
    ) -> Result<Option<LauncherEntryRecord>, LauncherError> {
        prepare_launcher_entry(config, path, &actor.user_id, platform)
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

/// Validate a user-approved pin without launching it or writing configuration.
/// Callers must authorize the edit: the UI uses ManageLauncher; the CLI edits
/// the current OS user's own configuration, just like `config set`.
pub fn prepare_launcher_entry(
    config: &LauncherConfig,
    path: &Path,
    added_by_user_id: &str,
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
    Ok(Some(LauncherEntryRecord {
        id: new_id(&target.path, config),
        path: target.path.to_string_lossy().into_owned(),
        executable_kind: Some(target.kind),
        fingerprint: None,
        added_by_user_id: added_by_user_id.to_string(),
        added_at_epoch_ms: epoch_millis(),
        terminal: None,
        custom: None,
        name: None,
        icon: None,
    }))
}

fn validate_terminal_settings(
    entry: &LauncherEntryRecord,
    settings: &LauncherTerminalSettings,
    platform: &dyn Platform,
) -> Result<(), LauncherError> {
    let supported = match entry.executable_kind {
        Some(LauncherExecutableKind::NativeBinary) => true,
        Some(LauncherExecutableKind::Script) => {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                matches!(
                    platform.kind(),
                    platform::PlatformKind::Linux | platform::PlatformKind::Macos
                ) && fs::metadata(&entry.path).is_ok_and(|metadata| {
                    metadata.is_file() && metadata.permissions().mode() & 0o111 != 0
                })
            }
            #[cfg(not(unix))]
            {
                let _ = platform;
                false
            }
        }
        _ => false,
    };
    if !supported {
        return Err(LauncherError::Localized(LocalizedError::new(
            "LAUNCHER_TERMINAL_UNSUPPORTED",
            msg!("app-launcher-terminal-unsupported"),
        )));
    }
    if let Some(directory) = settings
        .working_directory
        .as_deref()
        .filter(|directory| !directory.is_empty())
    {
        let path = Path::new(directory);
        if !path.is_absolute() || !path.is_dir() {
            return Err(LauncherError::Localized(LocalizedError::new(
                "LAUNCHER_TERMINAL_INVALID_DIRECTORY",
                msg!("app-launcher-terminal-directory-invalid", path = directory),
            )));
        }
    }
    Ok(())
}

struct Target {
    path: PathBuf,
    kind: LauncherExecutableKind,
}

fn validate(path: &Path, platform: &dyn Platform) -> Result<Target, LauncherError> {
    if !path.is_absolute() {
        return Err(LauncherError::Localized(LocalizedError::new(
            "LAUNCHER_INVALID_PATH",
            msg!(
                "app-launcher-absolute-path-required",
                path = path.display().to_string()
            ),
        )));
    }
    let link = fs::symlink_metadata(path).map_err(|error| io_error(path, error))?;
    if link.file_type().is_symlink() {
        return Err(LauncherError::Localized(LocalizedError::new(
            "LAUNCHER_INVALID_PATH",
            msg!(
                "app-launcher-symlink-blocked",
                path = path.display().to_string()
            ),
        )));
    }
    let canonical = fs::canonicalize(path).map_err(|error| io_error(path, error))?;
    let attributes = platform
        .file_attributes(&canonical)
        .map_err(|error| LauncherError::Platform(error.to_string()))?;
    if attributes.symlink || attributes.junction || attributes.reparse_point {
        return Err(LauncherError::Localized(LocalizedError::new(
            "LAUNCHER_INVALID_PATH",
            msg!(
                "app-launcher-unsafe-link-blocked",
                path = path.display().to_string()
            ),
        )));
    }
    let kind = match platform.file_open_policy(&canonical, &attributes) {
        FileOpenPolicy::LauncherRequired { kind, .. } => convert_kind(kind),
        FileOpenPolicy::SystemDefault => {
            return Err(LauncherError::Localized(LocalizedError::new(
                "LAUNCHER_INVALID_PATH",
                msg!(
                    "app-launcher-approval-required",
                    path = path.display().to_string()
                ),
            )));
        }
        FileOpenPolicy::Blocked { reason } => return Err(invalid(path, reason)),
    };
    if !(attributes.is_file
        || attributes.is_dir && kind == LauncherExecutableKind::ApplicationBundle)
    {
        return Err(LauncherError::Localized(LocalizedError::new(
            "LAUNCHER_INVALID_PATH",
            msg!(
                "app-launcher-file-required",
                path = path.display().to_string()
            ),
        )));
    }
    Ok(Target {
        path: canonical,
        kind,
    })
}

fn verify(
    entry: &LauncherEntryRecord,
    _platform: &dyn Platform,
) -> Result<LauncherItemStatus, LauncherError> {
    if entry.custom.is_some() {
        return Ok(LauncherItemStatus::Ready);
    }
    if entry.executable_kind.is_none() {
        return Ok(LauncherItemStatus::NeedsApproval);
    }
    let path = Path::new(&entry.path);
    match path.try_exists() {
        Ok(true) => Ok(LauncherItemStatus::Ready),
        Ok(false) => Ok(LauncherItemStatus::Missing),
        Err(error) => Err(io_error(path, error)),
    }
}

/// Checks whether a persisted Launcher target still exists at its recorded path.
///
/// Background refreshes and launch requests share this presence check. Stored
/// fingerprints, content, modification times, and classification changes are
/// deliberately not compared.
pub fn verify_launcher_entry(
    entry: &LauncherEntryRecord,
    platform: &dyn Platform,
) -> Result<LauncherItemStatus, LauncherError> {
    verify(entry, platform)
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

fn platform_kind(kind: LauncherExecutableKind) -> PlatformExecutableKind {
    match kind {
        LauncherExecutableKind::NativeBinary => PlatformExecutableKind::NativeBinary,
        LauncherExecutableKind::Installer => PlatformExecutableKind::Installer,
        LauncherExecutableKind::Script => PlatformExecutableKind::Script,
        LauncherExecutableKind::Shortcut => PlatformExecutableKind::Shortcut,
        LauncherExecutableKind::ApplicationBundle => PlatformExecutableKind::ApplicationBundle,
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
        left.to_string_lossy()
            .eq_ignore_ascii_case(&right.to_string_lossy())
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
fn new_id(path: &Path, config: &LauncherConfig) -> String {
    let mut hash = Sha256::new();
    hash.update(path.to_string_lossy().as_bytes());
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
