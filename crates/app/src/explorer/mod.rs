pub mod tasks;
pub use tasks::ExplorerTaskOperation;

use i18n::{LocalizedError, LocalizedText, msg};

mod state;

mod controller;

mod files;

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsStr;
use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use identity::{AuthSession, PermissionAction, PermissionService};
use platform::{
    ExecutableKind, FileAttributes, FileOpenPolicy, Platform, PlatformError, TrashEntry,
    TrashEntryId, TrashRestoreTarget,
};
use storage::{
    ExplorerConfig, ExplorerDateZone, ExplorerSizeFormat,
    ExplorerSortDirection as StoredSortDirection, ExplorerSortField as StoredSortField,
    StorageError, StorageManager,
};

use crate::editor::is_log_document_path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExplorerLocation {
    Directory(PathBuf),
    Trash,
}

impl ExplorerLocation {
    pub fn directory(path: impl Into<PathBuf>) -> Self {
        Self::Directory(path.into())
    }

    pub const fn is_trash(&self) -> bool {
        matches!(self, Self::Trash)
    }

    pub fn path(&self) -> Option<&Path> {
        match self {
            Self::Directory(path) => Some(path),
            Self::Trash => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExplorerState {
    pub current_path: PathBuf,
    pub current_location: ExplorerLocation,
    pub all_entries: Vec<ExplorerEntry>,
    pub entries: Vec<ExplorerEntry>,
    pub selected_index: usize,
    pub selected_paths: BTreeSet<PathBuf>,
    pub selection_anchor: Option<PathBuf>,
    pub selection_cleared: bool,
    pub query: String,
    pub show_hidden: bool,
    pub show_system: bool,
    pub show_extensions: bool,
    pub folders_first: bool,
    pub case_sensitive_sort: bool,
    pub size_format: ExplorerSizeFormat,
    pub date_zone: ExplorerDateZone,
    pub confirm_delete: bool,
    pub confirm_name_conflicts: bool,
    pub show_sidebar: bool,
    pub sort_field: ExplorerSortField,
    pub sort_direction: ExplorerSortDirection,
    pub viewport_offset: usize,
    pub viewport_follows_focus: bool,
    pub listing_warning_count: usize,
    pub back_history: Vec<ExplorerLocation>,
    pub forward_history: Vec<ExplorerLocation>,
    pub quick_locations: Vec<ExplorerQuickLocation>,
    pub clipboard: Option<ExplorerClipboard>,
    pub pending_dialog: Option<ExplorerDialog>,
    pub pending_conflict: Option<ExplorerConflict>,
    pub pending_restore: Option<ExplorerPendingRestore>,
    pub pending_transfer: Option<ExplorerPendingTransfer>,
    pub drag: Option<ExplorerDragState>,
    pub operation: Option<ExplorerOperationProgress>,
    pub message: Option<LocalizedText>,
    pub error: Option<LocalizedText>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExplorerEntry {
    pub name: String,
    pub path: PathBuf,
    pub trash_id: Option<TrashEntryId>,
    pub original_path: Option<PathBuf>,
    pub kind: ExplorerEntryKind,
    pub size: u64,
    pub modified: Option<SystemTime>,
    pub attributes: FileAttributes,
    pub open_policy: FileOpenPolicy,
    pub type_label: String,
    pub icon_key: String,
    pub metadata_warning: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExplorerQuickLocation {
    pub id: String,
    pub label: LocalizedText,
    pub path: PathBuf,
    pub icon_key: String,
    pub kind: ExplorerQuickLocationKind,
    pub enabled: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ExplorerQuickLocationKind {
    #[default]
    Directory,
    Volume,
    Trash,
}

impl ExplorerQuickLocation {
    pub fn new(
        id: impl Into<String>,
        label: impl Into<LocalizedText>,
        path: impl Into<PathBuf>,
        icon_key: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            path: path.into(),
            icon_key: icon_key.into(),
            kind: ExplorerQuickLocationKind::Directory,
            enabled: true,
        }
    }

    pub fn volume(
        id: impl Into<String>,
        label: impl Into<LocalizedText>,
        path: impl Into<PathBuf>,
    ) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            path: path.into(),
            icon_key: "drive".to_string(),
            kind: ExplorerQuickLocationKind::Volume,
            enabled: true,
        }
    }

    pub fn trash() -> Self {
        Self {
            id: "trash".to_string(),
            label: msg!("app-explorer-quick-trash").into(),
            path: PathBuf::new(),
            icon_key: "trash".to_string(),
            kind: ExplorerQuickLocationKind::Trash,
            enabled: true,
        }
    }

    pub const fn is_trash(&self) -> bool {
        matches!(self.kind, ExplorerQuickLocationKind::Trash)
    }

    pub fn localized_label(&self) -> LocalizedText {
        // Explicit caller translations and raw volume names retain their identity.
        if matches!(self.label, LocalizedText::Message(_))
            || self.kind == ExplorerQuickLocationKind::Volume
        {
            return self.label.clone();
        }
        match self.id.as_str() {
            "desktop" => msg!("app-explorer-quick-desktop").into(),
            "documents" => msg!("app-explorer-quick-documents").into(),
            "downloads" => msg!("app-explorer-quick-downloads").into(),
            "pictures" => msg!("app-explorer-quick-pictures").into(),
            "music" => msg!("app-explorer-quick-music").into(),
            "videos" => msg!("app-explorer-quick-videos").into(),
            "trash" => msg!("app-explorer-quick-trash").into(),
            _ => self.label.clone(),
        }
    }
}

impl ExplorerEntry {
    pub fn file_type(&self) -> ExplorerFileType {
        if self.trash_id.is_some() {
            return if self.kind == ExplorerEntryKind::Directory {
                ExplorerFileType::TrashedDirectory
            } else {
                ExplorerFileType::TrashedFile
            };
        }
        if let FileOpenPolicy::LauncherRequired { kind, .. } = &self.open_policy {
            return ExplorerFileType::Executable(*kind);
        }
        match self.kind {
            ExplorerEntryKind::Directory => ExplorerFileType::Directory,
            ExplorerEntryKind::Other => ExplorerFileType::Other,
            ExplorerEntryKind::File => ExplorerFileType::File {
                extension: self
                    .path
                    .extension()
                    .and_then(OsStr::to_str)
                    .filter(|extension| !extension.is_empty())
                    .map(str::to_ascii_uppercase),
            },
        }
    }

    /// Display text is independent of the retained, locale-stable `type_label` sort key.
    pub fn localized_type_label(&self) -> LocalizedText {
        self.file_type().localized_label()
    }

    fn from_metadata(
        path: PathBuf,
        name: String,
        attributes: Option<FileAttributes>,
        open_policy: FileOpenPolicy,
    ) -> Self {
        let metadata_warning = attributes
            .is_none()
            .then(|| "metadata unavailable".to_string());
        let attributes = attributes.unwrap_or_else(|| unknown_file_attributes(path.clone()));
        let kind = if attributes.is_dir {
            ExplorerEntryKind::Directory
        } else if attributes.is_file {
            ExplorerEntryKind::File
        } else {
            ExplorerEntryKind::Other
        };

        let type_label = explorer_type_label(&path, kind, &open_policy);
        let icon_key = explorer_icon_key(&path, kind, &attributes, &open_policy).to_string();

        Self {
            name,
            path,
            trash_id: None,
            original_path: None,
            kind,
            size: attributes.len,
            modified: attributes.modified,
            attributes,
            open_policy,
            type_label,
            icon_key,
            metadata_warning,
        }
    }

    fn from_trash(entry: TrashEntry) -> Self {
        let synthetic_path = PathBuf::from(format!("trash:{}", entry.id.as_str()));
        let attributes = FileAttributes {
            path: synthetic_path.clone(),
            is_file: !entry.is_directory,
            is_dir: entry.is_directory,
            len: entry.size,
            readonly: true,
            modified: entry.deleted_at,
            hidden: false,
            system: false,
            archive: false,
            symlink: false,
            junction: false,
            reparse_point: false,
            shortcut: false,
        };
        let kind = if entry.is_directory {
            ExplorerEntryKind::Directory
        } else {
            ExplorerEntryKind::File
        };
        Self {
            name: entry.display_name,
            path: synthetic_path,
            trash_id: Some(entry.id),
            original_path: entry.original_path,
            kind,
            size: entry.size,
            modified: entry.deleted_at,
            attributes,
            open_policy: FileOpenPolicy::blocked("Trash items must be restored before opening"),
            type_label: if entry.is_directory {
                "Trashed folder".to_string()
            } else {
                "Trashed file".to_string()
            },
            icon_key: if entry.is_directory { "folder" } else { "file" }.to_string(),
            metadata_warning: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExplorerSelectionMode {
    Replace,
    Toggle,
    Range,
    AddRange,
    FocusOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExplorerSortField {
    Name,
    Type,
    Size,
    Modified,
}

impl From<StoredSortField> for ExplorerSortField {
    fn from(value: StoredSortField) -> Self {
        match value {
            StoredSortField::Name => Self::Name,
            StoredSortField::Type => Self::Type,
            StoredSortField::Size => Self::Size,
            StoredSortField::Modified => Self::Modified,
        }
    }
}

impl From<ExplorerSortField> for StoredSortField {
    fn from(value: ExplorerSortField) -> Self {
        match value {
            ExplorerSortField::Name => Self::Name,
            ExplorerSortField::Type => Self::Type,
            ExplorerSortField::Size => Self::Size,
            ExplorerSortField::Modified => Self::Modified,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ExplorerSortDirection {
    #[default]
    Ascending,
    Descending,
}

impl From<StoredSortDirection> for ExplorerSortDirection {
    fn from(value: StoredSortDirection) -> Self {
        match value {
            StoredSortDirection::Ascending => Self::Ascending,
            StoredSortDirection::Descending => Self::Descending,
        }
    }
}

impl From<ExplorerSortDirection> for StoredSortDirection {
    fn from(value: ExplorerSortDirection) -> Self {
        match value {
            ExplorerSortDirection::Ascending => Self::Ascending,
            ExplorerSortDirection::Descending => Self::Descending,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExplorerEntryKind {
    Directory,
    File,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExplorerFileType {
    Directory,
    File { extension: Option<String> },
    Other,
    Executable(ExecutableKind),
    TrashedDirectory,
    TrashedFile,
}

impl ExplorerFileType {
    pub fn localized_label(&self) -> LocalizedText {
        match self {
            Self::Directory => msg!("app-explorer-type-folder"),
            Self::File {
                extension: Some(extension),
            } => msg!("app-explorer-type-extension", extension = extension.clone()),
            Self::File { extension: None } => msg!("app-explorer-type-file"),
            Self::Other => msg!("app-explorer-type-other"),
            Self::Executable(kind) => match kind {
                ExecutableKind::NativeBinary => msg!("app-explorer-type-executable"),
                ExecutableKind::Installer => msg!("app-explorer-type-installer"),
                ExecutableKind::Script => msg!("app-explorer-type-script"),
                ExecutableKind::Shortcut => msg!("app-explorer-type-shortcut"),
                ExecutableKind::ApplicationBundle => msg!("app-explorer-type-application"),
            },
            Self::TrashedDirectory => msg!("app-explorer-type-trashed-folder"),
            Self::TrashedFile => msg!("app-explorer-type-trashed-file"),
        }
        .into()
    }
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
    pub paths: Vec<PathBuf>,
    pub mode: ExplorerClipboardMode,
}

impl ExplorerClipboard {
    pub fn first_path(&self) -> Option<&Path> {
        self.paths.first().map(PathBuf::as_path)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExplorerClipboardMode {
    Copy,
    Cut,
}

impl From<ExplorerClipboardMode> for ExplorerTaskOperation {
    fn from(mode: ExplorerClipboardMode) -> Self {
        match mode {
            ExplorerClipboardMode::Copy => Self::Copy,
            ExplorerClipboardMode::Cut => Self::Move,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExplorerDialog {
    pub kind: ExplorerDialogKind,
    pub title: LocalizedText,
    pub message: LocalizedText,
    /// Immutable snapshot of the paths covered by a delete confirmation.
    ///
    /// Confirming must never re-read the live selection: keyboard navigation or a delayed shell
    /// notification could otherwise delete a different item than the one named by the dialog.
    pub targets: Vec<PathBuf>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExplorerDialogKind {
    DeleteToTrash,
    DumpTrash,
}

impl ExplorerDialog {
    pub fn delete(path: &Path) -> Self {
        Self {
            kind: ExplorerDialogKind::DeleteToTrash,
            title: msg!("app-explorer-delete-title").into(),
            message: msg!(
                "app-explorer-delete-confirm",
                path = path.display().to_string()
            )
            .into(),
            targets: vec![path.to_path_buf()],
        }
    }

    pub fn delete_many(paths: &[PathBuf]) -> Self {
        if paths.len() == 1 {
            return Self::delete(&paths[0]);
        }
        Self {
            kind: ExplorerDialogKind::DeleteToTrash,
            title: msg!("app-explorer-delete-title").into(),
            message: msg!(
                "app-explorer-delete-many-confirm",
                count = paths.len() as i64
            )
            .into(),
            targets: paths.to_vec(),
        }
    }

    pub fn dump_trash(item_count: usize) -> Self {
        Self {
            kind: ExplorerDialogKind::DumpTrash,
            title: msg!("app-explorer-dump-title").into(),
            message: msg!("app-explorer-dump-confirm", count = item_count as i64).into(),
            targets: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExplorerPendingRestore {
    pub id: TrashEntryId,
    pub display_name: String,
    pub target: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExplorerTransferMode {
    Copy,
    Move,
}

impl From<ExplorerTransferMode> for ExplorerClipboardMode {
    fn from(value: ExplorerTransferMode) -> Self {
        match value {
            ExplorerTransferMode::Copy => Self::Copy,
            ExplorerTransferMode::Move => Self::Cut,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExplorerDragState {
    pub sources: Vec<PathBuf>,
    pub target: Option<PathBuf>,
    pub mode: ExplorerTransferMode,
    pub active: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExplorerConflictAction {
    KeepBoth,
    Replace,
    Skip,
    Cancel,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExplorerConflict {
    pub source: PathBuf,
    pub target: PathBuf,
    pub remaining: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExplorerPendingTransfer {
    pub clipboard: ExplorerClipboard,
    pub destination: PathBuf,
    pub conflicts: Vec<(PathBuf, PathBuf)>,
    pub current_conflict: usize,
    pub resolutions: BTreeMap<PathBuf, ExplorerConflictAction>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExplorerOperationPhase {
    Scanning,
    WaitingForConflict,
    Executing,
    Completed,
    Cancelled,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExplorerOperationProgress {
    pub operation: ExplorerTaskOperation,
    pub phase: ExplorerOperationPhase,
    pub label: LocalizedText,
    pub completed_items: usize,
    pub total_items: Option<usize>,
    pub completed_bytes: u64,
    pub total_bytes: Option<u64>,
    pub cancellable: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExplorerOpenTarget {
    SystemDefault,
    Editor,
    Launcher,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExplorerOpenRequest {
    pub path: PathBuf,
    pub target: ExplorerOpenTarget,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum ExplorerEffect {
    #[default]
    None,
    OpenRequested(ExplorerOpenRequest),
    PersistConfig(ExplorerConfig),
}

pub trait ExplorerOpenRouteResolver: Send + Sync + fmt::Debug {
    fn route(&self, path: &Path, attributes: &FileAttributes) -> ExplorerOpenTarget;
}

#[derive(Debug, Default)]
pub struct SystemDefaultOpenRouteResolver;

impl ExplorerOpenRouteResolver for SystemDefaultOpenRouteResolver {
    fn route(&self, _path: &Path, _attributes: &FileAttributes) -> ExplorerOpenTarget {
        ExplorerOpenTarget::SystemDefault
    }
}

/// Routes documents supported by the built-in editor while leaving every
/// other file type with the operating system.
#[derive(Debug, Clone)]
pub struct EditorAwareOpenRouteResolver {
    extensions: Vec<String>,
}

impl EditorAwareOpenRouteResolver {
    pub fn new(extensions: Vec<String>) -> Self {
        let extensions =
            extensions
                .into_iter()
                .fold(Vec::new(), |mut normalized_extensions, extension| {
                    if let Some(extension) =
                        storage::normalize_editor_explorer_open_extension(&extension)
                        && normalized_extensions.len()
                            < storage::MAX_EDITOR_EXPLORER_OPEN_EXTENSIONS
                        && !normalized_extensions.contains(&extension)
                    {
                        normalized_extensions.push(extension);
                    }
                    normalized_extensions
                });
        Self { extensions }
    }
}

impl Default for EditorAwareOpenRouteResolver {
    fn default() -> Self {
        Self::new(
            storage::DEFAULT_EDITOR_EXPLORER_OPEN_EXTENSIONS
                .iter()
                .map(|extension| (*extension).to_string())
                .collect(),
        )
    }
}

impl ExplorerOpenRouteResolver for EditorAwareOpenRouteResolver {
    fn route(&self, path: &Path, attributes: &FileAttributes) -> ExplorerOpenTarget {
        if attributes.is_file && is_editor_document_path_with_extensions(path, &self.extensions) {
            ExplorerOpenTarget::Editor
        } else {
            ExplorerOpenTarget::SystemDefault
        }
    }
}

pub fn is_editor_document_path(path: &Path) -> bool {
    is_editor_document_path_with_extensions(path, storage::DEFAULT_EDITOR_EXPLORER_OPEN_EXTENSIONS)
}

pub fn is_editor_document_path_with_extensions(
    path: &Path,
    extensions: &[impl AsRef<str>],
) -> bool {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    let name = name.to_ascii_lowercase();
    extensions.iter().any(|extension| {
        let extension = extension.as_ref();
        (extension.eq_ignore_ascii_case("log") && is_log_document_path(path))
            || name.ends_with(&format!(".{}", extension.to_ascii_lowercase()))
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExplorerCommand {
    OpenSelected,
    OpenParent,
    OpenBack,
    OpenForward,
    Navigate(PathBuf),
    NavigateTrash,
    SelectNext,
    SelectPrevious,
    SelectIndex(usize),
    SelectIndexWithMode(usize, ExplorerSelectionMode),
    SelectAll,
    InvertSelection,
    ClearSelection,
    ToggleFocused,
    Search(String),
    ToggleHidden,
    ToggleSystem,
    ToggleExtensions,
    ToggleFoldersFirst,
    ToggleCaseSensitiveSort,
    ToggleSidebar,
    SetSort(ExplorerSortField),
    ToggleSizeFormat,
    ToggleDateZone,
    ToggleDeleteConfirmation,
    ToggleConflictConfirmation,
    NewFolder(String),
    NewTextFile(String),
    Rename(String),
    ConfirmDelete,
    DeleteToTrash,
    DumpTrash,
    ConfirmDumpTrash,
    RestoreSelected,
    RestoreSelectedToDirectory(PathBuf),
    ResolveRestoreConflict(ExplorerConflictAction),
    Copy,
    Cut,
    Paste,
    BeginDrag,
    UpdateDrag {
        target: Option<PathBuf>,
        mode: ExplorerTransferMode,
    },
    DropDrag,
    CancelDrag,
    ResolveConflict {
        action: ExplorerConflictAction,
        apply_to_all: bool,
    },
    CancelOperation,
    Refresh,
}

#[derive(Debug, Clone)]
pub struct ExplorerController {
    file_service: ExplorerFileService,
    open_resolver: Arc<dyn ExplorerOpenRouteResolver>,
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

impl Default for ExplorerFileService {
    fn default() -> Self {
        Self::new(PermissionService::default())
    }
}

#[derive(Debug)]
pub enum ExplorerError {
    Localized(LocalizedError),
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
}

impl fmt::Display for ExplorerError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Localized(error) => formatter.write_str(&i18n::render_diagnostic(&error.message)),
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
        }
    }
}

impl ExplorerError {
    /// Retain message identity until the presentation boundary renders it.
    pub fn localized(&self) -> LocalizedError {
        match self {
            Self::Localized(error) => error.clone(),
            Self::PermissionDenied {
                action,
                reason,
                path,
            } => LocalizedError::new(
                "EXPLORER_PERMISSION_DENIED",
                msg!(
                    "app-explorer-permission-denied",
                    action = action.to_string(),
                    reason = reason.clone(),
                    path = path.display().to_string()
                ),
            ),
            Self::BlockedPath(message)
            | Self::InvalidName(message)
            | Self::InvalidOperation(message) => LocalizedError::new(
                "EXPLORER_INVALID_OPERATION",
                msg!("app-explorer-detail", detail = message.clone()),
            ),
            Self::Io {
                operation,
                path,
                message,
            } => LocalizedError::new(
                "EXPLORER_IO",
                msg!(
                    "app-explorer-io",
                    operation = *operation,
                    path = path.display().to_string(),
                    detail = message.clone()
                ),
            ),
            Self::Platform(error) => LocalizedError::new(
                "EXPLORER_PLATFORM",
                msg!("app-explorer-platform-error", detail = error.to_string()),
            ),
            Self::Storage(error) => LocalizedError::new(
                "EXPLORER_STORAGE",
                msg!("app-explorer-storage-error", detail = error.to_string()),
            ),
        }
    }
}

impl std::error::Error for ExplorerError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Localized(error) => Some(error),
            Self::Platform(error) => Some(error),
            Self::Storage(error) => Some(error),
            _ => None,
        }
    }
}

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

fn selected_paths_or_error(state: &ExplorerState) -> Result<Vec<PathBuf>, ExplorerError> {
    let paths = state.effective_selected_paths();
    if paths.is_empty() {
        Err(ExplorerError::Localized(LocalizedError::new(
            "EXPLORER_INVALID_OPERATION",
            msg!("app-explorer-nothing-selected"),
        )))
    } else {
        Ok(paths)
    }
}

fn selected_trash_entry(state: &ExplorerState) -> Result<ExplorerEntry, ExplorerError> {
    let selected = state.effective_selected_paths();
    if selected.len() != 1 {
        return Err(ExplorerError::Localized(LocalizedError::new(
            "EXPLORER_INVALID_OPERATION",
            msg!("app-explorer-restore-selection"),
        )));
    }
    let entry = state
        .entries
        .iter()
        .find(|entry| entry.path == selected[0])
        .cloned()
        .ok_or_else(|| {
            ExplorerError::Localized(LocalizedError::new(
                "EXPLORER_INVALID_OPERATION",
                msg!("app-explorer-trash-item-missing"),
            ))
        })?;
    if entry.trash_id.is_none() {
        return Err(ExplorerError::Localized(LocalizedError::new(
            "EXPLORER_INVALID_OPERATION",
            msg!("app-explorer-not-trash-item"),
        )));
    }
    Ok(entry)
}

fn ensure_filesystem_location(state: &ExplorerState) -> Result<(), ExplorerError> {
    if state.current_location.is_trash() {
        Err(ExplorerError::Localized(LocalizedError::new(
            "EXPLORER_INVALID_OPERATION",
            msg!("app-explorer-unavailable-in-trash"),
        )))
    } else {
        Ok(())
    }
}

fn ensure_trash_location(state: &ExplorerState) -> Result<(), ExplorerError> {
    if state.current_location.is_trash() {
        Ok(())
    } else {
        Err(ExplorerError::Localized(LocalizedError::new(
            "EXPLORER_INVALID_OPERATION",
            msg!("app-explorer-trash-only"),
        )))
    }
}

fn restore_target_in_directory(directory: &Path, name: &str) -> Result<PathBuf, ExplorerError> {
    if !directory.is_absolute() {
        return Err(ExplorerError::Localized(LocalizedError::new(
            "EXPLORER_INVALID_OPERATION",
            msg!("app-explorer-restore-directory-required"),
        )));
    }
    validate_child_name(name).map_err(|_| {
        ExplorerError::Localized(LocalizedError::new(
            "EXPLORER_INVALID_OPERATION",
            msg!("app-explorer-trash-invalid-name"),
        ))
    })?;
    Ok(directory.join(name))
}

fn commit_location_listing(
    state: &mut ExplorerState,
    location: ExplorerLocation,
    entries: Vec<ExplorerEntry>,
    warning_count: usize,
) {
    if let ExplorerLocation::Directory(path) = &location {
        state.current_path = path.clone();
    }
    state.current_location = location;
    state.all_entries = entries;
    state.listing_warning_count = warning_count;
    state.query.clear();
    state.clear_selection();
    state.selection_cleared = false;
    state.selected_index = 0;
    state.viewport_offset = 0;
    state.viewport_follows_focus = true;
    state.apply_projection();
    if warning_count > 0 {
        state.message = Some(
            msg!(
                "app-explorer-incomplete-metadata",
                count = warning_count as i64
            )
            .into(),
        );
    }
}

fn clear_location_listing(state: &mut ExplorerState) {
    state.all_entries.clear();
    state.entries.clear();
    state.clear_selection();
    state.selected_index = 0;
    state.viewport_offset = 0;
    state.viewport_follows_focus = true;
    state.listing_warning_count = 0;
}

fn compare_entries(state: &ExplorerState, left: &ExplorerEntry, right: &ExplorerEntry) -> Ordering {
    if state.folders_first {
        let directory_order = directory_rank(left.kind).cmp(&directory_rank(right.kind));
        if directory_order != Ordering::Equal {
            return directory_order;
        }
    }

    let primary = match state.sort_field {
        ExplorerSortField::Name => directional_order(
            natural_name_compare(&left.name, &right.name, state.case_sensitive_sort),
            state.sort_direction,
        ),
        ExplorerSortField::Type => directional_order(
            natural_name_compare(
                &left.type_label,
                &right.type_label,
                state.case_sensitive_sort,
            ),
            state.sort_direction,
        ),
        ExplorerSortField::Size => compare_optional(
            (left.kind == ExplorerEntryKind::File).then_some(left.size),
            (right.kind == ExplorerEntryKind::File).then_some(right.size),
            state.sort_direction,
        ),
        ExplorerSortField::Modified => {
            compare_optional(left.modified, right.modified, state.sort_direction)
        }
    };

    primary
        .then_with(|| natural_name_compare(&left.name, &right.name, state.case_sensitive_sort))
        .then_with(|| left.path.cmp(&right.path))
}

fn directional_order(order: Ordering, direction: ExplorerSortDirection) -> Ordering {
    match direction {
        ExplorerSortDirection::Ascending => order,
        ExplorerSortDirection::Descending => order.reverse(),
    }
}

fn compare_optional<T: Ord + Copy>(
    left: Option<T>,
    right: Option<T>,
    direction: ExplorerSortDirection,
) -> Ordering {
    match (left, right) {
        (Some(left), Some(right)) => directional_order(left.cmp(&right), direction),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}

fn natural_name_compare(left: &str, right: &str, case_sensitive: bool) -> Ordering {
    if case_sensitive {
        return natural_byte_compare(left.as_bytes(), right.as_bytes(), false);
    }

    if left.is_ascii() && right.is_ascii() {
        return natural_byte_compare(left.as_bytes(), right.as_bytes(), true);
    }

    let left = left.to_lowercase();
    let right = right.to_lowercase();
    natural_byte_compare(left.as_bytes(), right.as_bytes(), false)
}

fn natural_byte_compare(left: &[u8], right: &[u8], fold_ascii_case: bool) -> Ordering {
    let mut left_index = 0usize;
    let mut right_index = 0usize;

    while left_index < left.len() && right_index < right.len() {
        if left[left_index].is_ascii_digit() && right[right_index].is_ascii_digit() {
            let left_end = digit_run_end(left, left_index);
            let right_end = digit_run_end(right, right_index);
            let left_digits = &left[left_index..left_end];
            let right_digits = &right[right_index..right_end];
            let left_trimmed = trim_leading_zeroes(left_digits);
            let right_trimmed = trim_leading_zeroes(right_digits);
            let order = left_trimmed
                .len()
                .cmp(&right_trimmed.len())
                .then_with(|| left_trimmed.cmp(right_trimmed))
                .then_with(|| left_digits.len().cmp(&right_digits.len()));
            if order != Ordering::Equal {
                return order;
            }
            left_index = left_end;
            right_index = right_end;
            continue;
        }

        let left_byte = if fold_ascii_case {
            left[left_index].to_ascii_lowercase()
        } else {
            left[left_index]
        };
        let right_byte = if fold_ascii_case {
            right[right_index].to_ascii_lowercase()
        } else {
            right[right_index]
        };
        let order = left_byte.cmp(&right_byte);
        if order != Ordering::Equal {
            return order;
        }
        left_index += 1;
        right_index += 1;
    }

    left.len().cmp(&right.len())
}

fn digit_run_end(bytes: &[u8], start: usize) -> usize {
    let mut end = start;
    while end < bytes.len() && bytes[end].is_ascii_digit() {
        end += 1;
    }
    end
}

fn trim_leading_zeroes(bytes: &[u8]) -> &[u8] {
    let first_non_zero = bytes
        .iter()
        .position(|byte| *byte != b'0')
        .unwrap_or(bytes.len().saturating_sub(1));
    &bytes[first_non_zero..]
}

fn unknown_file_attributes(path: PathBuf) -> FileAttributes {
    FileAttributes {
        path,
        is_file: false,
        is_dir: false,
        len: 0,
        readonly: true,
        modified: None,
        hidden: false,
        system: false,
        archive: false,
        symlink: false,
        junction: false,
        reparse_point: false,
        shortcut: false,
    }
}

fn explorer_type_label(
    path: &Path,
    kind: ExplorerEntryKind,
    open_policy: &FileOpenPolicy,
) -> String {
    if let FileOpenPolicy::LauncherRequired { kind, .. } = open_policy {
        return match kind {
            ExecutableKind::NativeBinary => "Executable".to_string(),
            ExecutableKind::Installer => "Installer".to_string(),
            ExecutableKind::Script => "Script".to_string(),
            ExecutableKind::Shortcut => "Shortcut".to_string(),
            ExecutableKind::ApplicationBundle => "Application".to_string(),
        };
    }
    match kind {
        ExplorerEntryKind::Directory => "Folder".to_string(),
        ExplorerEntryKind::Other => "Other".to_string(),
        ExplorerEntryKind::File => path
            .extension()
            .and_then(OsStr::to_str)
            .filter(|extension| !extension.is_empty())
            .map(|extension| format!("{} file", extension.to_ascii_uppercase()))
            .unwrap_or_else(|| "File".to_string()),
    }
}

fn explorer_icon_key(
    path: &Path,
    kind: ExplorerEntryKind,
    attributes: &FileAttributes,
    open_policy: &FileOpenPolicy,
) -> &'static str {
    if matches!(open_policy, FileOpenPolicy::LauncherRequired { .. }) {
        return "executable";
    }
    if attributes.symlink || attributes.junction || attributes.reparse_point || attributes.shortcut
    {
        return "link";
    }
    if kind == ExplorerEntryKind::Directory {
        return "folder";
    }
    if kind == ExplorerEntryKind::Other {
        return "other";
    }

    let extension = path
        .extension()
        .and_then(OsStr::to_str)
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    match extension.as_str() {
        "txt" | "md" | "log" | "ini" | "cfg" | "conf" | "toml" | "yaml" | "yml" | "json"
        | "xml" | "csv" => "text",
        "rs" | "c" | "h" | "cpp" | "hpp" | "py" | "pyw" | "js" | "ts" | "html" | "css" | "sh"
        | "bash" | "zsh" | "ps1" => "code",
        "pdf" | "doc" | "docx" | "xls" | "xlsx" | "ppt" | "pptx" | "odt" | "ods" | "odp" => {
            "document"
        }
        "png" | "jpg" | "jpeg" | "gif" | "bmp" | "webp" | "svg" | "ico" => "image",
        "mp3" | "wav" | "flac" | "ogg" | "m4a" | "aac" => "audio",
        "mp4" | "mkv" | "avi" | "mov" | "webm" => "video",
        "zip" | "7z" | "rar" | "tar" | "gz" | "bz2" | "xz" => "archive",
        _ => "file",
    }
}

fn directory_rank(kind: ExplorerEntryKind) -> u8 {
    match kind {
        ExplorerEntryKind::Directory => 0,
        ExplorerEntryKind::File | ExplorerEntryKind::Other => 1,
    }
}

fn child_path(parent: &Path, name: &str) -> Result<PathBuf, ExplorerError> {
    validate_child_name(name)?;
    Ok(parent.join(name))
}

fn validate_transfer_destination(source: &Path, target: &Path) -> Result<(), ExplorerError> {
    if source == target {
        return Err(ExplorerError::Localized(LocalizedError::new(
            "EXPLORER_INVALID_OPERATION",
            msg!("app-explorer-same-path"),
        )));
    }
    let metadata = fs::symlink_metadata(source).map_err(|error| {
        ExplorerError::Platform(PlatformError::from_io(
            "inspect transfer source",
            Some(source.to_path_buf()),
            &error,
        ))
    })?;
    if metadata.file_type().is_symlink() {
        return Err(ExplorerError::Localized(LocalizedError::new(
            "EXPLORER_BLOCKED_PATH",
            msg!("app-explorer-blocked-link-transfer"),
        )));
    }
    if metadata.is_dir() && target.starts_with(source) {
        return Err(ExplorerError::Localized(LocalizedError::new(
            "EXPLORER_INVALID_OPERATION",
            msg!("app-explorer-descendant-transfer"),
        )));
    }
    if target
        .parent()
        .and_then(|parent| fs::metadata(parent).ok())
        .is_some_and(|metadata| metadata.permissions().readonly())
    {
        return Err(ExplorerError::Localized(LocalizedError::new(
            "EXPLORER_INVALID_OPERATION",
            msg!("app-explorer-destination-read-only"),
        )));
    }
    Ok(())
}

fn unique_sibling_path(path: &Path) -> Result<PathBuf, ExplorerError> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let stem = path.file_stem().and_then(OsStr::to_str).unwrap_or("item");
    let extension = path.extension().and_then(OsStr::to_str);
    for suffix in 2u32.. {
        let name = match extension {
            Some(extension) if !extension.is_empty() => {
                format!("{stem} ({suffix}).{extension}")
            }
            _ => format!("{stem} ({suffix})"),
        };
        let candidate = parent.join(name);
        if !path_exists_no_follow(&candidate)? {
            return Ok(candidate);
        }
    }
    unreachable!("u32 suffix iterator is unbounded for practical purposes")
}

fn path_exists_no_follow(path: &Path) -> Result<bool, ExplorerError> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(ExplorerError::Platform(PlatformError::from_io(
            "inspect destination",
            Some(path.to_path_buf()),
            &error,
        ))),
    }
}

fn copy_path_staged(source: &Path, target: &Path) -> Result<(), ExplorerError> {
    let parent = target.parent().ok_or_else(|| {
        ExplorerError::Localized(LocalizedError::new(
            "EXPLORER_INVALID_OPERATION",
            msg!("app-explorer-transfer-no-parent"),
        ))
    })?;
    let temporary = parent.join(format!(
        ".tundra-part-{}-{}",
        std::process::id(),
        unix_millis()
    ));
    let result = if fs::symlink_metadata(source)
        .map(|metadata| metadata.is_dir())
        .unwrap_or(false)
    {
        copy_directory(source, &temporary)
    } else {
        copy_file_chunked(source, &temporary)
    }
    .and_then(|()| {
        fs::rename(&temporary, target).map_err(|error| {
            ExplorerError::Platform(PlatformError::from_io(
                "commit staged copy",
                Some(target.to_path_buf()),
                &error,
            ))
        })
    });
    if result.is_err() {
        let cleanup = if temporary.is_dir() {
            fs::remove_dir_all(&temporary)
        } else {
            fs::remove_file(&temporary)
        };
        if let Err(error) = cleanup {
            if error.kind() != std::io::ErrorKind::NotFound {
                log_secondary_explorer("copy_cleanup", None, &error, Some(&temporary));
            }
        }
    }
    result
}

fn copy_file_chunked(source: &Path, target: &Path) -> Result<(), ExplorerError> {
    let metadata = fs::symlink_metadata(source).map_err(|error| {
        ExplorerError::Platform(PlatformError::from_io(
            "inspect copy source",
            Some(source.to_path_buf()),
            &error,
        ))
    })?;
    if metadata.file_type().is_symlink() {
        return Err(ExplorerError::Localized(LocalizedError::new(
            "EXPLORER_BLOCKED_PATH",
            msg!("app-explorer-blocked-link-copy"),
        )));
    }
    let mut input = fs::File::open(source).map_err(|error| {
        ExplorerError::Platform(PlatformError::from_io(
            "open copy source",
            Some(source.to_path_buf()),
            &error,
        ))
    })?;
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(target)
        .map_err(|error| {
            ExplorerError::Platform(PlatformError::from_io(
                "create staged copy",
                Some(target.to_path_buf()),
                &error,
            ))
        })?;
    let mut buffer = vec![0u8; 256 * 1024];
    loop {
        let read = input.read(&mut buffer).map_err(|error| {
            ExplorerError::Platform(PlatformError::from_io(
                "read copy source",
                Some(source.to_path_buf()),
                &error,
            ))
        })?;
        if read == 0 {
            break;
        }
        output.write_all(&buffer[..read]).map_err(|error| {
            ExplorerError::Platform(PlatformError::from_io(
                "write staged copy",
                Some(target.to_path_buf()),
                &error,
            ))
        })?;
    }
    output.sync_all().map_err(|error| {
        ExplorerError::Platform(PlatformError::from_io(
            "sync staged copy",
            Some(target.to_path_buf()),
            &error,
        ))
    })
}

fn remove_source_path(path: &Path) -> Result<(), ExplorerError> {
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        ExplorerError::Platform(PlatformError::from_io(
            "inspect move source",
            Some(path.to_path_buf()),
            &error,
        ))
    })?;
    let result = if metadata.is_dir() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    };
    result.map_err(|error| {
        ExplorerError::Platform(PlatformError::from_io(
            "remove committed move source",
            Some(path.to_path_buf()),
            &error,
        ))
    })
}

fn move_existing_to_trash(platform: &dyn Platform, path: &Path) -> Result<(), ExplorerError> {
    platform
        .move_to_trash(&[path.to_path_buf()])
        .map_err(Into::into)
}

fn create_restore_rollback_directory(parent: &Path) -> Result<PathBuf, ExplorerError> {
    let prefix = format!(".tundra-restore-{}-{}", std::process::id(), unix_millis());
    for suffix in 0u32.. {
        let name = if suffix == 0 {
            prefix.clone()
        } else {
            format!("{prefix}-{suffix}")
        };
        let candidate = parent.join(name);
        match fs::create_dir(&candidate) {
            Ok(()) => return Ok(candidate),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(ExplorerError::Platform(PlatformError::from_io(
                    "create restore rollback directory",
                    Some(candidate),
                    &error,
                )));
            }
        }
    }
    unreachable!("u32 suffix iterator is unbounded for practical purposes")
}

fn validate_child_name(name: &str) -> Result<(), ExplorerError> {
    let trimmed = name.trim();
    let mut components = Path::new(name).components();
    let is_single_normal_component = matches!(
        components.next(),
        Some(std::path::Component::Normal(component)) if component == OsStr::new(name)
    ) && components.next().is_none();
    if trimmed.is_empty()
        || trimmed == "."
        || trimmed == ".."
        || trimmed.contains('/')
        || trimmed.contains('\\')
        || !is_single_normal_component
    {
        return Err(ExplorerError::Localized(LocalizedError::new(
            "EXPLORER_INVALID_OPERATION",
            msg!("app-explorer-invalid-name", name = name),
        )));
    }
    Ok(())
}

fn copy_path(source: &Path, target: &Path) -> Result<(), ExplorerError> {
    let metadata = fs::symlink_metadata(source).map_err(|error| {
        ExplorerError::Platform(PlatformError::from_io(
            "read copy source",
            Some(source.to_path_buf()),
            &error,
        ))
    })?;

    if metadata.file_type().is_symlink() {
        return Err(ExplorerError::Localized(LocalizedError::new(
            "EXPLORER_BLOCKED_PATH",
            msg!("app-explorer-blocked-link-copy"),
        )));
    }

    if metadata.is_dir() {
        copy_directory(source, target)
    } else {
        copy_file_chunked(source, target)
    }
}

fn copy_directory(source: &Path, target: &Path) -> Result<(), ExplorerError> {
    fs::create_dir(target).map_err(|error| {
        ExplorerError::Platform(PlatformError::from_io(
            "copy directory",
            Some(target.to_path_buf()),
            &error,
        ))
    })?;

    for entry in fs::read_dir(source).map_err(|error| {
        ExplorerError::Platform(PlatformError::from_io(
            "read copy directory",
            Some(source.to_path_buf()),
            &error,
        ))
    })? {
        let entry = entry.map_err(|error| {
            ExplorerError::Platform(PlatformError::from_io(
                "read copy directory entry",
                Some(source.to_path_buf()),
                &error,
            ))
        })?;
        let source_child = entry.path();
        let target_child = target.join(entry.file_name());
        copy_path(&source_child, &target_child)?;
    }

    Ok(())
}

fn unix_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
#[path = "../../tests/unit/explorer/natural_sort_tests.rs"]
mod natural_sort_tests;

fn log_secondary_explorer(
    operation: &str,
    session: Option<&AuthSession>,
    error: &(dyn std::error::Error + 'static),
    path: Option<&Path>,
) {
    let app = watchdog::AppWatchdog::current();
    let mut context = app
        .as_ref()
        .map(|app| app.log_context(operation))
        .unwrap_or_else(|| runtime_log::LogContext {
            app: "explorer".into(),
            operation: operation.into(),
            ..runtime_log::LogContext::default()
        });
    context.module = "ux.explorer".into();
    context.owner_id = session
        .map(|session| session.user_id.clone())
        .or(context.owner_id);
    let mut event = runtime_log::RuntimeLogEvent::new(
        context,
        runtime_log::LogLevel::Warning,
        runtime_log::LogPhase::Degraded,
        "Explorer secondary operation failed",
    );
    event.source_path = path.map(Path::to_path_buf);
    event.error_code = Some(format!("EXPLORER_{}", operation.to_ascii_uppercase()));
    watchdog::capture_error(&mut event, error);
    if let Some(error) = error.downcast_ref::<PlatformError>() {
        event.os_error_code = error.raw_os_error().map(i64::from);
    }
    if let Some(app) = app {
        app.record_log(event);
    } else {
        runtime_log::record(event);
    }
}

fn emit_explorer_event(event: runtime_log::RuntimeLogEvent) {
    if let Some(app) = watchdog::AppWatchdog::current() {
        app.record_log(event);
    } else {
        runtime_log::record(event);
    }
}
