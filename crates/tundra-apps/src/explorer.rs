use std::ffi::OsStr;
use std::fmt;
use std::fs::{self, OpenOptions};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use tundra_core::{
    AuditOutcome, AuditService, AuthSession, CoreError, PermissionAction, PermissionService,
};
use tundra_platform::{FileAttributes, Platform, PlatformError};
use tundra_storage::{StorageError, StorageManager, TrashDocument, TrashRecord};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExplorerState {
    pub current_path: PathBuf,
    pub entries: Vec<ExplorerEntry>,
    pub selected_index: usize,
    pub query: String,
    pub show_hidden: bool,
    pub clipboard: Option<ExplorerClipboard>,
    pub pending_dialog: Option<ExplorerDialog>,
    pub message: Option<String>,
    pub error: Option<String>,
}

impl ExplorerState {
    pub fn new(current_path: impl Into<PathBuf>, show_hidden: bool) -> Self {
        Self {
            current_path: current_path.into(),
            entries: Vec::new(),
            selected_index: 0,
            query: String::new(),
            show_hidden,
            clipboard: None,
            pending_dialog: None,
            message: None,
            error: None,
        }
    }

    pub fn selected_entry(&self) -> Option<&ExplorerEntry> {
        self.entries.get(self.selected_index)
    }

    fn selected_path(&self) -> Option<PathBuf> {
        self.selected_entry().map(|entry| entry.path.clone())
    }

    fn clamp_selection(&mut self) {
        if self.entries.is_empty() {
            self.selected_index = 0;
        } else if self.selected_index >= self.entries.len() {
            self.selected_index = self.entries.len() - 1;
        }
    }

    fn set_success(&mut self, message: impl Into<String>) {
        self.message = Some(message.into());
        self.error = None;
    }

    fn set_error(&mut self, error: ExplorerError) {
        self.error = Some(error.to_string());
        self.message = None;
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExplorerEntry {
    pub name: String,
    pub path: PathBuf,
    pub kind: ExplorerEntryKind,
    pub size: u64,
    pub modified: Option<SystemTime>,
    pub attributes: FileAttributes,
}

impl ExplorerEntry {
    fn from_attributes(path: PathBuf, attributes: FileAttributes) -> Self {
        let name = path
            .file_name()
            .and_then(OsStr::to_str)
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| path.display().to_string());
        let kind = if attributes.is_dir {
            ExplorerEntryKind::Directory
        } else if attributes.is_file {
            ExplorerEntryKind::File
        } else {
            ExplorerEntryKind::Other
        };

        Self {
            name,
            path,
            kind,
            size: attributes.len,
            modified: attributes.modified,
            attributes,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExplorerEntryKind {
    Directory,
    File,
    Other,
}

impl ExplorerEntryKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Directory => "dir",
            Self::File => "file",
            Self::Other => "other",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExplorerClipboard {
    pub path: PathBuf,
    pub mode: ExplorerClipboardMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExplorerClipboardMode {
    Copy,
    Cut,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExplorerDialog {
    pub title: String,
    pub message: String,
}

impl ExplorerDialog {
    pub fn delete(path: &Path) -> Self {
        Self {
            title: "Delete to trash".to_string(),
            message: format!("Move {} to TundraUX trash?", path.display()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExplorerCommand {
    OpenSelected,
    OpenParent,
    SelectNext,
    SelectPrevious,
    SelectIndex(usize),
    Search(String),
    ToggleHidden,
    NewFolder(String),
    NewTextFile(String),
    Rename(String),
    ConfirmDelete,
    DeleteToTrash,
    Copy,
    Cut,
    Paste,
    Refresh,
}

#[derive(Debug, Clone)]
pub struct ExplorerController {
    file_service: ExplorerFileService,
}

impl ExplorerController {
    pub fn new(file_service: ExplorerFileService) -> Self {
        Self { file_service }
    }

    pub fn apply(
        &self,
        state: &mut ExplorerState,
        command: ExplorerCommand,
        session: Option<&AuthSession>,
        platform: &dyn Platform,
        storage: &StorageManager,
    ) {
        state.error = None;
        match self.try_apply(state, command, session, platform, storage) {
            Ok(()) => state.clamp_selection(),
            Err(error) => state.set_error(error),
        }
    }

    fn try_apply(
        &self,
        state: &mut ExplorerState,
        command: ExplorerCommand,
        session: Option<&AuthSession>,
        platform: &dyn Platform,
        storage: &StorageManager,
    ) -> Result<(), ExplorerError> {
        match command {
            ExplorerCommand::Refresh => {
                self.file_service
                    .refresh(state, session, platform, storage)?;
            }
            ExplorerCommand::SelectNext => {
                if !state.entries.is_empty() {
                    state.selected_index = (state.selected_index + 1).min(state.entries.len() - 1);
                }
            }
            ExplorerCommand::SelectPrevious => {
                state.selected_index = state.selected_index.saturating_sub(1);
            }
            ExplorerCommand::SelectIndex(index) => {
                if index < state.entries.len() {
                    state.selected_index = index;
                }
            }
            ExplorerCommand::Search(query) => {
                state.query = query;
                self.file_service
                    .refresh(state, session, platform, storage)?;
            }
            ExplorerCommand::ToggleHidden => {
                state.show_hidden = !state.show_hidden;
                self.file_service
                    .refresh(state, session, platform, storage)?;
                state.set_success(if state.show_hidden {
                    "Hidden files visible"
                } else {
                    "Hidden files hidden"
                });
            }
            ExplorerCommand::OpenParent => {
                let parent = state
                    .current_path
                    .parent()
                    .ok_or_else(|| ExplorerError::InvalidOperation("no parent directory".into()))?
                    .to_path_buf();
                state.current_path = parent;
                self.file_service
                    .refresh(state, session, platform, storage)?;
            }
            ExplorerCommand::OpenSelected => {
                let Some(entry) = state.selected_entry().cloned() else {
                    return Ok(());
                };
                self.file_service
                    .open_entry(state, session, platform, storage, &entry)?;
            }
            ExplorerCommand::NewFolder(name) => {
                self.file_service
                    .create_folder(state, session, platform, storage, &name)?;
            }
            ExplorerCommand::NewTextFile(name) => {
                self.file_service
                    .create_text_file(state, session, platform, storage, &name)?;
            }
            ExplorerCommand::Rename(name) => {
                let path = state
                    .selected_path()
                    .ok_or_else(|| ExplorerError::InvalidOperation("nothing selected".into()))?;
                self.file_service
                    .rename(state, session, platform, storage, &path, &name)?;
            }
            ExplorerCommand::DeleteToTrash => {
                let path = state
                    .selected_path()
                    .ok_or_else(|| ExplorerError::InvalidOperation("nothing selected".into()))?;
                state.pending_dialog = Some(ExplorerDialog::delete(&path));
            }
            ExplorerCommand::ConfirmDelete => {
                let path = state
                    .selected_path()
                    .ok_or_else(|| ExplorerError::InvalidOperation("nothing selected".into()))?;
                self.file_service
                    .delete_to_trash(state, session, platform, storage, &path)?;
                state.pending_dialog = None;
            }
            ExplorerCommand::Copy => {
                let path = state
                    .selected_path()
                    .ok_or_else(|| ExplorerError::InvalidOperation("nothing selected".into()))?;
                state.clipboard = Some(ExplorerClipboard {
                    path,
                    mode: ExplorerClipboardMode::Copy,
                });
                state.set_success("Copied selection");
            }
            ExplorerCommand::Cut => {
                let path = state
                    .selected_path()
                    .ok_or_else(|| ExplorerError::InvalidOperation("nothing selected".into()))?;
                state.clipboard = Some(ExplorerClipboard {
                    path,
                    mode: ExplorerClipboardMode::Cut,
                });
                state.set_success("Cut selection");
            }
            ExplorerCommand::Paste => {
                let clipboard = state
                    .clipboard
                    .clone()
                    .ok_or_else(|| ExplorerError::InvalidOperation("clipboard is empty".into()))?;
                self.file_service
                    .paste(state, session, platform, storage, clipboard)?;
            }
        }

        Ok(())
    }
}

impl Default for ExplorerController {
    fn default() -> Self {
        Self::new(ExplorerFileService::default())
    }
}

#[derive(Debug, Clone)]
pub struct ExplorerFileService {
    permission_service: PermissionService,
}

impl ExplorerFileService {
    pub fn new(permission_service: PermissionService) -> Self {
        Self { permission_service }
    }

    pub fn refresh(
        &self,
        state: &mut ExplorerState,
        session: Option<&AuthSession>,
        platform: &dyn Platform,
        storage: &StorageManager,
    ) -> Result<(), ExplorerError> {
        self.authorize(
            session,
            storage,
            PermissionAction::ReadFile,
            &state.current_path,
        )?;

        let mut entries = Vec::new();
        let directory = fs::read_dir(&state.current_path).map_err(|error| ExplorerError::Io {
            operation: "read directory",
            path: state.current_path.clone(),
            message: error.to_string(),
        })?;
        let query = state.query.to_ascii_lowercase();

        for entry in directory {
            let entry = entry.map_err(|error| ExplorerError::Io {
                operation: "read directory entry",
                path: state.current_path.clone(),
                message: error.to_string(),
            })?;
            let path = entry.path();
            let attributes = platform.file_attributes(&path)?;
            let explorer_entry = ExplorerEntry::from_attributes(path, attributes);
            if !state.show_hidden && explorer_entry.attributes.hidden {
                continue;
            }
            if !query.is_empty()
                && !explorer_entry
                    .name
                    .to_ascii_lowercase()
                    .contains(query.as_str())
            {
                continue;
            }
            entries.push(explorer_entry);
        }

        entries.sort_by(|left, right| {
            directory_rank(left.kind)
                .cmp(&directory_rank(right.kind))
                .then(
                    left.name
                        .to_ascii_lowercase()
                        .cmp(&right.name.to_ascii_lowercase()),
                )
        });

        state.entries = entries;
        state.clamp_selection();
        Ok(())
    }

    fn open_entry(
        &self,
        state: &mut ExplorerState,
        session: Option<&AuthSession>,
        platform: &dyn Platform,
        storage: &StorageManager,
        entry: &ExplorerEntry,
    ) -> Result<(), ExplorerError> {
        if entry.kind == ExplorerEntryKind::Directory {
            if entry.attributes.reparse_point || entry.attributes.junction {
                self.audit(
                    storage,
                    session,
                    PermissionAction::ReadFile,
                    &entry.path,
                    AuditOutcome::Denied,
                    "reparse_directory_blocked",
                )?;
                return Err(ExplorerError::BlockedPath(
                    "reparse point directories are blocked in Explorer MVP".into(),
                ));
            }
            self.authorize(session, storage, PermissionAction::ReadFile, &entry.path)?;
            state.current_path = entry.path.clone();
            self.refresh(state, session, platform, storage)?;
            return Ok(());
        }

        let external_open_policy = platform.external_open_policy(&entry.path, &entry.attributes);
        if let Some(reason) = external_open_policy.blocked_reason() {
            self.audit(
                storage,
                session,
                PermissionAction::OpenExternal,
                &entry.path,
                AuditOutcome::Denied,
                "external_open_blocked",
            )?;
            return Err(ExplorerError::BlockedPath(reason.to_string()));
        }

        self.authorize(
            session,
            storage,
            PermissionAction::OpenExternal,
            &entry.path,
        )?;
        match platform.open_path(&entry.path) {
            Ok(()) => {
                self.audit(
                    storage,
                    session,
                    PermissionAction::OpenExternal,
                    &entry.path,
                    AuditOutcome::Success,
                    "open_path",
                )?;
                state.set_success(format!("Opened {}", entry.name));
                Ok(())
            }
            Err(error) => {
                self.audit(
                    storage,
                    session,
                    PermissionAction::OpenExternal,
                    &entry.path,
                    AuditOutcome::Failure,
                    "open_path_failed",
                )?;
                Err(error.into())
            }
        }
    }

    fn create_folder(
        &self,
        state: &mut ExplorerState,
        session: Option<&AuthSession>,
        platform: &dyn Platform,
        storage: &StorageManager,
        name: &str,
    ) -> Result<(), ExplorerError> {
        let path = child_path(&state.current_path, name)?;
        self.authorize(session, storage, PermissionAction::WriteFile, &path)?;
        fs::create_dir(&path).map_err(|error| ExplorerError::Io {
            operation: "create folder",
            path: path.clone(),
            message: error.to_string(),
        })?;
        self.audit(
            storage,
            session,
            PermissionAction::WriteFile,
            &path,
            AuditOutcome::Success,
            "create_folder",
        )?;
        state.set_success(format!("Created folder {}", path.display()));
        self.refresh(state, session, platform, storage)
    }

    fn create_text_file(
        &self,
        state: &mut ExplorerState,
        session: Option<&AuthSession>,
        platform: &dyn Platform,
        storage: &StorageManager,
        name: &str,
    ) -> Result<(), ExplorerError> {
        let path = child_path(&state.current_path, name)?;
        self.authorize(session, storage, PermissionAction::WriteFile, &path)?;
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .and_then(|file| file.sync_all())
            .map_err(|error| ExplorerError::Io {
                operation: "create text file",
                path: path.clone(),
                message: error.to_string(),
            })?;
        self.audit(
            storage,
            session,
            PermissionAction::WriteFile,
            &path,
            AuditOutcome::Success,
            "create_text_file",
        )?;
        state.set_success(format!("Created file {}", path.display()));
        self.refresh(state, session, platform, storage)
    }

    fn rename(
        &self,
        state: &mut ExplorerState,
        session: Option<&AuthSession>,
        platform: &dyn Platform,
        storage: &StorageManager,
        path: &Path,
        name: &str,
    ) -> Result<(), ExplorerError> {
        let parent = path
            .parent()
            .ok_or_else(|| ExplorerError::InvalidOperation("selected item has no parent".into()))?;
        let target = child_path(parent, name)?;
        self.authorize(session, storage, PermissionAction::WriteFile, path)?;
        fs::rename(path, &target).map_err(|error| ExplorerError::Io {
            operation: "rename",
            path: path.to_path_buf(),
            message: error.to_string(),
        })?;
        self.audit(
            storage,
            session,
            PermissionAction::WriteFile,
            &target,
            AuditOutcome::Success,
            "rename",
        )?;
        state.set_success(format!("Renamed to {}", target.display()));
        self.refresh(state, session, platform, storage)
    }

    fn delete_to_trash(
        &self,
        state: &mut ExplorerState,
        session: Option<&AuthSession>,
        platform: &dyn Platform,
        storage: &StorageManager,
        path: &Path,
    ) -> Result<(), ExplorerError> {
        self.authorize(session, storage, PermissionAction::DeleteFile, path)?;
        fs::create_dir_all(&storage.layout().trash_path).map_err(|error| ExplorerError::Io {
            operation: "create trash directory",
            path: storage.layout().trash_path.clone(),
            message: error.to_string(),
        })?;
        let trash_path = unique_trash_path(&storage.layout().trash_path, path);
        fs::rename(path, &trash_path).map_err(|error| ExplorerError::Io {
            operation: "move to trash",
            path: path.to_path_buf(),
            message: error.to_string(),
        })?;

        let mut trash = storage.load_trash()?;
        trash.records.push(TrashRecord {
            original_path: path.to_path_buf(),
            trash_path: trash_path.clone(),
            actor: session
                .map(|session| session.username.clone())
                .unwrap_or_else(|| "Guest".to_string()),
            timestamp_epoch_ms: unix_millis(),
        });
        storage.save_trash(&trash)?;

        self.audit(
            storage,
            session,
            PermissionAction::DeleteFile,
            path,
            AuditOutcome::Success,
            "delete_to_trash",
        )?;
        state.set_success(format!("Moved {} to trash", path.display()));
        self.refresh(state, session, platform, storage)
    }

    fn paste(
        &self,
        state: &mut ExplorerState,
        session: Option<&AuthSession>,
        platform: &dyn Platform,
        storage: &StorageManager,
        clipboard: ExplorerClipboard,
    ) -> Result<(), ExplorerError> {
        let file_name = clipboard
            .path
            .file_name()
            .ok_or_else(|| ExplorerError::InvalidOperation("clipboard path has no name".into()))?;
        let target = state.current_path.join(file_name);
        if target.exists() {
            return Err(ExplorerError::InvalidOperation(format!(
                "{} already exists",
                target.display()
            )));
        }

        match clipboard.mode {
            ExplorerClipboardMode::Copy => {
                self.authorize(session, storage, PermissionAction::WriteFile, &target)?;
                copy_path(&clipboard.path, &target)?;
                self.audit(
                    storage,
                    session,
                    PermissionAction::WriteFile,
                    &target,
                    AuditOutcome::Success,
                    "copy_paste",
                )?;
                state.set_success(format!("Copied to {}", target.display()));
            }
            ExplorerClipboardMode::Cut => {
                self.authorize(session, storage, PermissionAction::MoveFile, &target)?;
                fs::rename(&clipboard.path, &target).map_err(|error| ExplorerError::Io {
                    operation: "move paste",
                    path: clipboard.path.clone(),
                    message: error.to_string(),
                })?;
                self.audit(
                    storage,
                    session,
                    PermissionAction::MoveFile,
                    &target,
                    AuditOutcome::Success,
                    "cut_paste",
                )?;
                state.clipboard = None;
                state.set_success(format!("Moved to {}", target.display()));
            }
        }

        self.refresh(state, session, platform, storage)
    }

    fn authorize(
        &self,
        session: Option<&AuthSession>,
        storage: &StorageManager,
        action: PermissionAction,
        resource: &Path,
    ) -> Result<(), ExplorerError> {
        let resource_display = resource.display().to_string();
        let authorization =
            self.permission_service
                .authorize(session, action, Some(resource_display.as_str()));
        if authorization.allowed {
            return Ok(());
        }

        let reason = authorization
            .reason
            .unwrap_or_else(|| "permission_denied".to_string());
        self.audit(
            storage,
            session,
            action,
            resource,
            AuditOutcome::Denied,
            reason.as_str(),
        )?;
        Err(ExplorerError::PermissionDenied {
            action,
            reason,
            path: resource.to_path_buf(),
        })
    }

    fn audit(
        &self,
        storage: &StorageManager,
        session: Option<&AuthSession>,
        action: PermissionAction,
        resource: &Path,
        outcome: AuditOutcome,
        reason: &str,
    ) -> Result<(), ExplorerError> {
        AuditService::with_permission_service(storage.clone(), self.permission_service.clone())
            .record(
                session,
                action,
                Some(resource.display().to_string().as_str()),
                outcome,
                Some(reason),
            )?;
        Ok(())
    }
}

impl Default for ExplorerFileService {
    fn default() -> Self {
        Self::new(PermissionService::default())
    }
}

#[derive(Debug)]
pub enum ExplorerError {
    PermissionDenied {
        action: PermissionAction,
        reason: String,
        path: PathBuf,
    },
    BlockedPath(String),
    InvalidName(String),
    InvalidOperation(String),
    Io {
        operation: &'static str,
        path: PathBuf,
        message: String,
    },
    Platform(PlatformError),
    Storage(StorageError),
    Core(CoreError),
}

impl fmt::Display for ExplorerError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PermissionDenied {
                action,
                reason,
                path,
            } => write!(
                formatter,
                "{action} denied for {}: {reason}",
                path.display()
            ),
            Self::BlockedPath(message)
            | Self::InvalidName(message)
            | Self::InvalidOperation(message) => formatter.write_str(message),
            Self::Io {
                operation,
                path,
                message,
            } => write!(
                formatter,
                "{operation} failed for {}: {message}",
                path.display()
            ),
            Self::Platform(error) => write!(formatter, "{error}"),
            Self::Storage(error) => write!(formatter, "{error}"),
            Self::Core(error) => write!(formatter, "{error}"),
        }
    }
}

impl std::error::Error for ExplorerError {}

impl From<PlatformError> for ExplorerError {
    fn from(value: PlatformError) -> Self {
        Self::Platform(value)
    }
}

impl From<StorageError> for ExplorerError {
    fn from(value: StorageError) -> Self {
        Self::Storage(value)
    }
}

impl From<CoreError> for ExplorerError {
    fn from(value: CoreError) -> Self {
        Self::Core(value)
    }
}

fn directory_rank(kind: ExplorerEntryKind) -> u8 {
    match kind {
        ExplorerEntryKind::Directory => 0,
        ExplorerEntryKind::File => 1,
        ExplorerEntryKind::Other => 2,
    }
}

fn child_path(parent: &Path, name: &str) -> Result<PathBuf, ExplorerError> {
    validate_child_name(name)?;
    Ok(parent.join(name))
}

fn validate_child_name(name: &str) -> Result<(), ExplorerError> {
    let trimmed = name.trim();
    if trimmed.is_empty()
        || trimmed == "."
        || trimmed == ".."
        || trimmed.contains('/')
        || trimmed.contains('\\')
    {
        return Err(ExplorerError::InvalidName(format!(
            "invalid file name: {name}"
        )));
    }
    Ok(())
}

fn copy_path(source: &Path, target: &Path) -> Result<(), ExplorerError> {
    let metadata = fs::metadata(source).map_err(|error| ExplorerError::Io {
        operation: "read copy source",
        path: source.to_path_buf(),
        message: error.to_string(),
    })?;

    if metadata.is_dir() {
        copy_directory(source, target)
    } else {
        fs::copy(source, target).map_err(|error| ExplorerError::Io {
            operation: "copy file",
            path: source.to_path_buf(),
            message: error.to_string(),
        })?;
        Ok(())
    }
}

fn copy_directory(source: &Path, target: &Path) -> Result<(), ExplorerError> {
    fs::create_dir(target).map_err(|error| ExplorerError::Io {
        operation: "copy directory",
        path: target.to_path_buf(),
        message: error.to_string(),
    })?;

    for entry in fs::read_dir(source).map_err(|error| ExplorerError::Io {
        operation: "read copy directory",
        path: source.to_path_buf(),
        message: error.to_string(),
    })? {
        let entry = entry.map_err(|error| ExplorerError::Io {
            operation: "read copy directory entry",
            path: source.to_path_buf(),
            message: error.to_string(),
        })?;
        let source_child = entry.path();
        let target_child = target.join(entry.file_name());
        copy_path(&source_child, &target_child)?;
    }

    Ok(())
}

fn unique_trash_path(trash_path: &Path, original_path: &Path) -> PathBuf {
    let stem = original_path
        .file_name()
        .and_then(OsStr::to_str)
        .unwrap_or("item")
        .replace(['/', '\\', ':'], "_");
    let mut candidate = trash_path.join(format!("{}-{stem}", unix_millis()));
    let mut suffix = 1u32;
    while candidate.exists() {
        candidate = trash_path.join(format!("{}-{suffix}-{stem}", unix_millis()));
        suffix = suffix.saturating_add(1);
    }
    candidate
}

fn unix_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

#[allow(dead_code)]
fn empty_trash_document() -> TrashDocument {
    TrashDocument::default()
}
