use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsStr;
use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use tundra_core::{
    AuditOutcome, AuditService, AuthSession, CoreError, PermissionAction, PermissionService,
};
use tundra_platform::{ExecutableKind, FileAttributes, FileOpenPolicy, Platform, PlatformError};
use tundra_storage::{
    ExplorerConfig, ExplorerDateZone, ExplorerSizeFormat,
    ExplorerSortDirection as StoredSortDirection, ExplorerSortField as StoredSortField,
    StorageError, StorageManager, TrashDocument, TrashRecord,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExplorerState {
    pub current_path: PathBuf,
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
    pub back_history: Vec<PathBuf>,
    pub forward_history: Vec<PathBuf>,
    pub quick_locations: Vec<ExplorerQuickLocation>,
    pub clipboard: Option<ExplorerClipboard>,
    pub pending_dialog: Option<ExplorerDialog>,
    pub pending_conflict: Option<ExplorerConflict>,
    pub pending_transfer: Option<ExplorerPendingTransfer>,
    pub drag: Option<ExplorerDragState>,
    pub operation: Option<ExplorerOperationProgress>,
    pub message: Option<String>,
    pub error: Option<String>,
}

impl ExplorerState {
    pub fn new(current_path: impl Into<PathBuf>, show_hidden: bool) -> Self {
        let config = ExplorerConfig {
            show_hidden,
            ..ExplorerConfig::default()
        };
        Self::with_config(current_path, &config)
    }

    pub fn with_config(current_path: impl Into<PathBuf>, config: &ExplorerConfig) -> Self {
        Self {
            current_path: current_path.into(),
            all_entries: Vec::new(),
            entries: Vec::new(),
            selected_index: 0,
            selected_paths: BTreeSet::new(),
            selection_anchor: None,
            selection_cleared: false,
            query: String::new(),
            show_hidden: config.show_hidden,
            show_system: config.show_system,
            show_extensions: config.show_extensions,
            folders_first: config.folders_first,
            case_sensitive_sort: config.case_sensitive_sort,
            size_format: config.size_format,
            date_zone: config.date_zone,
            confirm_delete: config.confirm_delete,
            confirm_name_conflicts: config.confirm_name_conflicts,
            show_sidebar: config.show_sidebar,
            sort_field: config.sort_field.into(),
            sort_direction: config.sort_direction.into(),
            viewport_offset: 0,
            viewport_follows_focus: true,
            listing_warning_count: 0,
            back_history: Vec::new(),
            forward_history: Vec::new(),
            quick_locations: Vec::new(),
            clipboard: None,
            pending_dialog: None,
            pending_conflict: None,
            pending_transfer: None,
            drag: None,
            operation: None,
            message: None,
            error: None,
        }
    }

    pub fn selected_entry(&self) -> Option<&ExplorerEntry> {
        self.entries.get(self.selected_index)
    }

    pub fn selected_path(&self) -> Option<PathBuf> {
        self.selected_entry().map(|entry| entry.path.clone())
    }

    pub fn effective_selected_paths(&self) -> Vec<PathBuf> {
        if self.selection_cleared {
            Vec::new()
        } else if self.selected_paths.is_empty() {
            self.selected_path().into_iter().collect()
        } else {
            self.entries
                .iter()
                .filter(|entry| self.selected_paths.contains(&entry.path))
                .map(|entry| entry.path.clone())
                .collect()
        }
    }

    pub fn is_selected(&self, path: &Path) -> bool {
        if self.selection_cleared {
            false
        } else if self.selected_paths.is_empty() {
            self.selected_entry()
                .is_some_and(|entry| entry.path == path)
        } else {
            self.selected_paths.contains(path)
        }
    }

    pub fn select_index(&mut self, index: usize, mode: ExplorerSelectionMode) {
        if index >= self.entries.len() {
            return;
        }
        let path = self.entries[index].path.clone();
        match mode {
            ExplorerSelectionMode::Replace => {
                self.selected_paths.clear();
                self.selected_paths.insert(path.clone());
                self.selection_anchor = Some(path);
                self.selection_cleared = false;
            }
            ExplorerSelectionMode::Toggle => {
                if self.selected_paths.is_empty() && !self.selection_cleared {
                    self.selected_paths.extend(self.selected_path());
                }
                if !self.selected_paths.remove(&path) {
                    self.selected_paths.insert(path.clone());
                }
                self.selection_anchor = Some(path);
                self.selection_cleared = self.selected_paths.is_empty();
            }
            ExplorerSelectionMode::Range => {
                let fallback_index = self.selected_index.min(self.entries.len() - 1);
                let anchor_index = self
                    .selection_anchor
                    .as_ref()
                    .and_then(|anchor| self.entries.iter().position(|entry| &entry.path == anchor))
                    .unwrap_or(fallback_index);
                if self.selection_anchor.is_none() {
                    self.selection_anchor = self
                        .entries
                        .get(anchor_index)
                        .map(|entry| entry.path.clone());
                }
                let (start, end) = if anchor_index <= index {
                    (anchor_index, index)
                } else {
                    (index, anchor_index)
                };
                self.selected_paths.clear();
                self.selected_paths.extend(
                    self.entries[start..=end]
                        .iter()
                        .map(|entry| entry.path.clone()),
                );
                self.selection_cleared = false;
            }
            ExplorerSelectionMode::FocusOnly => {}
        }
        self.selected_index = index;
        self.viewport_follows_focus = true;
    }

    pub fn select_all(&mut self) {
        self.selected_paths = self
            .entries
            .iter()
            .map(|entry| entry.path.clone())
            .collect();
        if let Some(entry) = self.selected_entry() {
            self.selection_anchor = Some(entry.path.clone());
        }
        self.selection_cleared = self.selected_paths.is_empty();
        self.viewport_follows_focus = true;
    }

    pub fn clear_selection(&mut self) {
        self.selected_paths.clear();
        self.selection_anchor = None;
        self.selection_cleared = true;
    }

    pub fn apply_projection(&mut self) {
        let focused_path = self.selected_entry().map(|entry| entry.path.clone());
        let query = self.query.to_lowercase();
        let mut entries = self
            .all_entries
            .iter()
            .filter(|entry| self.show_hidden || !entry.attributes.hidden)
            .filter(|entry| self.show_system || !entry.attributes.system)
            .filter(|entry| query.is_empty() || entry.name.to_lowercase().contains(&query))
            .cloned()
            .collect::<Vec<_>>();
        entries.sort_by(|left, right| compare_entries(self, left, right));
        self.entries = entries;
        let had_selected_paths = !self.selected_paths.is_empty();
        self.selected_paths
            .retain(|path| self.entries.iter().any(|entry| &entry.path == path));
        if had_selected_paths && self.selected_paths.is_empty() {
            self.selection_cleared = true;
        }
        if let Some(focused_path) = focused_path
            && let Some(index) = self
                .entries
                .iter()
                .position(|entry| entry.path == focused_path)
        {
            self.selected_index = index;
        }
        self.clamp_selection();
    }

    pub fn to_config(&self) -> ExplorerConfig {
        ExplorerConfig {
            show_hidden: self.show_hidden,
            show_system: self.show_system,
            show_extensions: self.show_extensions,
            folders_first: self.folders_first,
            case_sensitive_sort: self.case_sensitive_sort,
            size_format: self.size_format,
            date_zone: self.date_zone,
            confirm_delete: self.confirm_delete,
            confirm_name_conflicts: self.confirm_name_conflicts,
            show_sidebar: self.show_sidebar,
            sort_field: self.sort_field.into(),
            sort_direction: self.sort_direction.into(),
        }
    }

    fn clamp_selection(&mut self) {
        if self.entries.is_empty() {
            self.selected_index = 0;
            self.viewport_offset = 0;
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
    pub open_policy: FileOpenPolicy,
    pub type_label: String,
    pub icon_key: String,
    pub metadata_warning: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExplorerQuickLocation {
    pub id: String,
    pub label: String,
    pub path: PathBuf,
    pub icon_key: String,
}

impl ExplorerQuickLocation {
    pub fn new(
        id: impl Into<String>,
        label: impl Into<String>,
        path: impl Into<PathBuf>,
        icon_key: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            path: path.into(),
            icon_key: icon_key.into(),
        }
    }
}

impl ExplorerEntry {
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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExplorerSelectionMode {
    Replace,
    Toggle,
    Range,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExplorerSortDirection {
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

    pub fn delete_many(paths: &[PathBuf]) -> Self {
        if paths.len() == 1 {
            return Self::delete(&paths[0]);
        }
        Self {
            title: "Delete to trash".to_string(),
            message: format!("Move {} selected items to TundraUX trash?", paths.len()),
        }
    }
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
    pub phase: ExplorerOperationPhase,
    pub label: String,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExplorerCommand {
    OpenSelected,
    OpenParent,
    OpenBack,
    OpenForward,
    Navigate(PathBuf),
    SelectNext,
    SelectPrevious,
    SelectIndex(usize),
    SelectIndexWithMode(usize, ExplorerSelectionMode),
    SelectAll,
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

impl ExplorerController {
    pub fn new(file_service: ExplorerFileService) -> Self {
        Self {
            file_service,
            open_resolver: Arc::new(SystemDefaultOpenRouteResolver),
        }
    }

    pub fn with_open_resolver(mut self, resolver: Arc<dyn ExplorerOpenRouteResolver>) -> Self {
        self.open_resolver = resolver;
        self
    }

    pub fn apply(
        &self,
        state: &mut ExplorerState,
        command: ExplorerCommand,
        session: Option<&AuthSession>,
        platform: &dyn Platform,
        storage: &StorageManager,
    ) -> ExplorerEffect {
        state.error = None;
        match self.try_apply(state, command, session, platform, storage) {
            Ok(effect) => {
                state.clamp_selection();
                effect
            }
            Err(error) => {
                state.set_error(error);
                ExplorerEffect::None
            }
        }
    }

    fn try_apply(
        &self,
        state: &mut ExplorerState,
        command: ExplorerCommand,
        session: Option<&AuthSession>,
        platform: &dyn Platform,
        storage: &StorageManager,
    ) -> Result<ExplorerEffect, ExplorerError> {
        let effect = match command {
            ExplorerCommand::Refresh => {
                self.file_service
                    .refresh(state, session, platform, storage)?;
                ExplorerEffect::None
            }
            ExplorerCommand::SelectNext => {
                if !state.entries.is_empty() {
                    let index = (state.selected_index + 1).min(state.entries.len() - 1);
                    state.select_index(index, ExplorerSelectionMode::Replace);
                }
                ExplorerEffect::None
            }
            ExplorerCommand::SelectPrevious => {
                let index = state.selected_index.saturating_sub(1);
                state.select_index(index, ExplorerSelectionMode::Replace);
                ExplorerEffect::None
            }
            ExplorerCommand::SelectIndex(index) => {
                state.select_index(index, ExplorerSelectionMode::Replace);
                ExplorerEffect::None
            }
            ExplorerCommand::SelectIndexWithMode(index, mode) => {
                state.select_index(index, mode);
                ExplorerEffect::None
            }
            ExplorerCommand::SelectAll => {
                state.select_all();
                ExplorerEffect::None
            }
            ExplorerCommand::ToggleFocused => {
                let index = state.selected_index;
                state.select_index(index, ExplorerSelectionMode::Toggle);
                ExplorerEffect::None
            }
            ExplorerCommand::Search(query) => {
                state.query = query;
                state.apply_projection();
                ExplorerEffect::None
            }
            ExplorerCommand::ToggleHidden => {
                state.show_hidden = !state.show_hidden;
                state.apply_projection();
                state.set_success(if state.show_hidden {
                    "Hidden files visible"
                } else {
                    "Hidden files hidden"
                });
                ExplorerEffect::PersistConfig(state.to_config())
            }
            ExplorerCommand::ToggleSystem => {
                state.show_system = !state.show_system;
                state.apply_projection();
                ExplorerEffect::PersistConfig(state.to_config())
            }
            ExplorerCommand::ToggleExtensions => {
                state.show_extensions = !state.show_extensions;
                ExplorerEffect::PersistConfig(state.to_config())
            }
            ExplorerCommand::ToggleFoldersFirst => {
                state.folders_first = !state.folders_first;
                state.apply_projection();
                ExplorerEffect::PersistConfig(state.to_config())
            }
            ExplorerCommand::ToggleCaseSensitiveSort => {
                state.case_sensitive_sort = !state.case_sensitive_sort;
                state.apply_projection();
                ExplorerEffect::PersistConfig(state.to_config())
            }
            ExplorerCommand::ToggleSidebar => {
                state.show_sidebar = !state.show_sidebar;
                ExplorerEffect::PersistConfig(state.to_config())
            }
            ExplorerCommand::SetSort(field) => {
                if state.sort_field == field {
                    state.sort_direction = match state.sort_direction {
                        ExplorerSortDirection::Ascending => ExplorerSortDirection::Descending,
                        ExplorerSortDirection::Descending => ExplorerSortDirection::Ascending,
                    };
                } else {
                    state.sort_field = field;
                    state.sort_direction = if field == ExplorerSortField::Modified {
                        ExplorerSortDirection::Descending
                    } else {
                        ExplorerSortDirection::Ascending
                    };
                }
                state.apply_projection();
                ExplorerEffect::PersistConfig(state.to_config())
            }
            ExplorerCommand::ToggleSizeFormat => {
                state.size_format = match state.size_format {
                    ExplorerSizeFormat::HumanBinary => ExplorerSizeFormat::Bytes,
                    ExplorerSizeFormat::Bytes => ExplorerSizeFormat::HumanBinary,
                };
                ExplorerEffect::PersistConfig(state.to_config())
            }
            ExplorerCommand::ToggleDateZone => {
                state.date_zone = match state.date_zone {
                    ExplorerDateZone::ConfiguredTimezone => ExplorerDateZone::Utc,
                    ExplorerDateZone::Utc => ExplorerDateZone::ConfiguredTimezone,
                };
                ExplorerEffect::PersistConfig(state.to_config())
            }
            ExplorerCommand::ToggleDeleteConfirmation => {
                state.confirm_delete = !state.confirm_delete;
                ExplorerEffect::PersistConfig(state.to_config())
            }
            ExplorerCommand::ToggleConflictConfirmation => {
                state.confirm_name_conflicts = !state.confirm_name_conflicts;
                ExplorerEffect::PersistConfig(state.to_config())
            }
            ExplorerCommand::OpenParent => {
                let parent = state
                    .current_path
                    .parent()
                    .ok_or_else(|| ExplorerError::InvalidOperation("no parent directory".into()))?
                    .to_path_buf();
                navigate_to(state, parent, true);
                self.file_service
                    .refresh(state, session, platform, storage)?;
                ExplorerEffect::None
            }
            ExplorerCommand::OpenBack => {
                let target = state
                    .back_history
                    .pop()
                    .ok_or_else(|| ExplorerError::InvalidOperation("no back history".into()))?;
                state.forward_history.push(state.current_path.clone());
                navigate_to(state, target, false);
                self.file_service
                    .refresh(state, session, platform, storage)?;
                ExplorerEffect::None
            }
            ExplorerCommand::OpenForward => {
                let target = state
                    .forward_history
                    .pop()
                    .ok_or_else(|| ExplorerError::InvalidOperation("no forward history".into()))?;
                state.back_history.push(state.current_path.clone());
                navigate_to(state, target, false);
                self.file_service
                    .refresh(state, session, platform, storage)?;
                ExplorerEffect::None
            }
            ExplorerCommand::Navigate(path) => {
                navigate_to(state, path, true);
                self.file_service
                    .refresh(state, session, platform, storage)?;
                ExplorerEffect::None
            }
            ExplorerCommand::OpenSelected => {
                let Some(entry) = state.selected_entry().cloned() else {
                    return Ok(ExplorerEffect::None);
                };
                self.file_service.open_entry(
                    state,
                    session,
                    platform,
                    storage,
                    &entry,
                    self.open_resolver.as_ref(),
                )?
            }
            ExplorerCommand::NewFolder(name) => {
                self.file_service
                    .create_folder(state, session, platform, storage, &name)?;
                ExplorerEffect::None
            }
            ExplorerCommand::NewTextFile(name) => {
                self.file_service
                    .create_text_file(state, session, platform, storage, &name)?;
                ExplorerEffect::None
            }
            ExplorerCommand::Rename(name) => {
                let paths = state.effective_selected_paths();
                if paths.len() != 1 {
                    return Err(ExplorerError::InvalidOperation(
                        "rename requires exactly one selected item".into(),
                    ));
                }
                self.file_service
                    .rename(state, session, platform, storage, &paths[0], &name)?;
                ExplorerEffect::None
            }
            ExplorerCommand::DeleteToTrash => {
                let paths = selected_paths_or_error(state)?;
                if state.confirm_delete {
                    state.pending_dialog = Some(ExplorerDialog::delete_many(&paths));
                } else {
                    self.file_service
                        .delete_many_to_trash(state, session, platform, storage, &paths)?;
                }
                ExplorerEffect::None
            }
            ExplorerCommand::ConfirmDelete => {
                let paths = selected_paths_or_error(state)?;
                self.file_service
                    .delete_many_to_trash(state, session, platform, storage, &paths)?;
                state.pending_dialog = None;
                ExplorerEffect::None
            }
            ExplorerCommand::Copy => {
                let paths = selected_paths_or_error(state)?;
                state.clipboard = Some(ExplorerClipboard {
                    paths,
                    mode: ExplorerClipboardMode::Copy,
                });
                state.set_success("Copied selection");
                ExplorerEffect::None
            }
            ExplorerCommand::Cut => {
                let paths = selected_paths_or_error(state)?;
                state.clipboard = Some(ExplorerClipboard {
                    paths,
                    mode: ExplorerClipboardMode::Cut,
                });
                state.set_success("Cut selection");
                ExplorerEffect::None
            }
            ExplorerCommand::Paste => {
                let clipboard = state
                    .clipboard
                    .clone()
                    .ok_or_else(|| ExplorerError::InvalidOperation("clipboard is empty".into()))?;
                self.file_service
                    .paste(state, session, platform, storage, clipboard)?;
                ExplorerEffect::None
            }
            ExplorerCommand::BeginDrag => {
                let sources = selected_paths_or_error(state)?;
                state.drag = Some(ExplorerDragState {
                    sources,
                    target: None,
                    mode: ExplorerTransferMode::Move,
                    active: false,
                });
                ExplorerEffect::None
            }
            ExplorerCommand::UpdateDrag { target, mode } => {
                if let Some(drag) = state.drag.as_mut() {
                    drag.target = target;
                    drag.mode = mode;
                    drag.active = true;
                }
                ExplorerEffect::None
            }
            ExplorerCommand::DropDrag => {
                let Some(drag) = state.drag.take() else {
                    return Ok(ExplorerEffect::None);
                };
                if drag.active {
                    let destination = drag.target.ok_or_else(|| {
                        ExplorerError::InvalidOperation("drag has no valid destination".into())
                    })?;
                    self.file_service.start_transfer(
                        state,
                        session,
                        platform,
                        storage,
                        ExplorerClipboard {
                            paths: drag.sources,
                            mode: drag.mode.into(),
                        },
                        destination,
                    )?;
                }
                ExplorerEffect::None
            }
            ExplorerCommand::CancelDrag => {
                state.drag = None;
                ExplorerEffect::None
            }
            ExplorerCommand::ResolveConflict {
                action,
                apply_to_all,
            } => {
                self.file_service.resolve_pending_transfer(
                    state,
                    session,
                    platform,
                    storage,
                    action,
                    apply_to_all,
                )?;
                ExplorerEffect::None
            }
            ExplorerCommand::CancelOperation => {
                if let Some(operation) = state.operation.as_mut() {
                    operation.phase = ExplorerOperationPhase::Cancelled;
                    operation.cancellable = false;
                }
                state.pending_conflict = None;
                ExplorerEffect::None
            }
        };

        Ok(effect)
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

        let listing = match platform.read_directory(&state.current_path) {
            Ok(listing) => listing,
            Err(error) => {
                // The path has already changed for navigation commands. Never leave actionable
                // rows from the previous directory under the new breadcrumb when listing fails.
                state.all_entries.clear();
                state.entries.clear();
                state.clear_selection();
                state.selected_index = 0;
                state.viewport_offset = 0;
                state.listing_warning_count = 0;
                return Err(error.into());
            }
        };
        state.listing_warning_count = listing.warnings.len();
        state.all_entries = listing
            .entries
            .into_iter()
            .map(|entry| {
                ExplorerEntry::from_metadata(
                    entry.path,
                    entry.name,
                    entry.attributes,
                    entry.open_policy,
                )
            })
            .collect();
        state.apply_projection();
        if state.listing_warning_count > 0 {
            state.message = Some(format!(
                "{} directory entries have incomplete metadata",
                state.listing_warning_count
            ));
        }
        Ok(())
    }

    fn open_entry(
        &self,
        state: &mut ExplorerState,
        session: Option<&AuthSession>,
        platform: &dyn Platform,
        storage: &StorageManager,
        entry: &ExplorerEntry,
        resolver: &dyn ExplorerOpenRouteResolver,
    ) -> Result<ExplorerEffect, ExplorerError> {
        match &entry.open_policy {
            FileOpenPolicy::Blocked { reason } => {
                self.audit(
                    storage,
                    session,
                    PermissionAction::OpenExternal,
                    &entry.path,
                    AuditOutcome::Denied,
                    "unsafe_path_blocked",
                )?;
                return Err(ExplorerError::BlockedPath(reason.clone()));
            }
            FileOpenPolicy::LauncherRequired { kind, reason } => {
                self.audit(
                    storage,
                    session,
                    PermissionAction::OpenExternal,
                    &entry.path,
                    AuditOutcome::Denied,
                    "launcher_required",
                )?;
                state.error = Some(format!(
                    "blocked: {reason}; Launcher is not implemented ({})",
                    kind.label()
                ));
                state.message = None;
                return Ok(ExplorerEffect::OpenRequested(ExplorerOpenRequest {
                    path: entry.path.clone(),
                    target: ExplorerOpenTarget::Launcher,
                }));
            }
            FileOpenPolicy::SystemDefault => {}
        }

        if entry.kind == ExplorerEntryKind::Directory {
            self.authorize(session, storage, PermissionAction::ReadFile, &entry.path)?;
            navigate_to(state, entry.path.clone(), true);
            self.refresh(state, session, platform, storage)?;
            return Ok(ExplorerEffect::None);
        }

        self.authorize(
            session,
            storage,
            PermissionAction::OpenExternal,
            &entry.path,
        )?;
        let target = resolver.route(&entry.path, &entry.attributes);
        if target == ExplorerOpenTarget::Editor {
            state.error = Some("Editor is not implemented".to_string());
            state.message = None;
        }
        Ok(ExplorerEffect::OpenRequested(ExplorerOpenRequest {
            path: entry.path.clone(),
            target,
        }))
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
        platform.rename_path(path, &target)?;
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

    fn delete_many_to_trash(
        &self,
        state: &mut ExplorerState,
        session: Option<&AuthSession>,
        platform: &dyn Platform,
        storage: &StorageManager,
        paths: &[PathBuf],
    ) -> Result<(), ExplorerError> {
        for path in paths {
            self.authorize(session, storage, PermissionAction::DeleteFile, path)?;
        }
        fs::create_dir_all(&storage.layout().trash_path).map_err(|error| ExplorerError::Io {
            operation: "create trash directory",
            path: storage.layout().trash_path.clone(),
            message: error.to_string(),
        })?;
        let mut trash = storage.load_trash()?;
        let actor = session
            .map(|session| session.username.clone())
            .unwrap_or_else(|| "Guest".to_string());
        state.operation = Some(ExplorerOperationProgress {
            phase: ExplorerOperationPhase::Executing,
            label: "Moving items to trash".to_string(),
            completed_items: 0,
            total_items: Some(paths.len()),
            completed_bytes: 0,
            total_bytes: None,
            cancellable: true,
        });
        for (index, path) in paths.iter().enumerate() {
            let trash_path = unique_trash_path(&storage.layout().trash_path, path);
            platform.rename_path(path, &trash_path)?;
            trash.records.push(TrashRecord {
                original_path: path.clone(),
                trash_path: trash_path.clone(),
                actor: actor.clone(),
                timestamp_epoch_ms: unix_millis(),
            });
            self.audit(
                storage,
                session,
                PermissionAction::DeleteFile,
                path,
                AuditOutcome::Success,
                "delete_to_trash",
            )?;
            if let Some(operation) = state.operation.as_mut() {
                operation.completed_items = index + 1;
            }
        }
        storage.save_trash(&trash)?;
        state.operation = None;
        state.clear_selection();
        state.set_success(format!("Moved {} item(s) to trash", paths.len()));
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
        let destination = state.current_path.clone();
        self.start_transfer(state, session, platform, storage, clipboard, destination)
    }

    #[allow(clippy::too_many_arguments)]
    fn start_transfer(
        &self,
        state: &mut ExplorerState,
        session: Option<&AuthSession>,
        platform: &dyn Platform,
        storage: &StorageManager,
        clipboard: ExplorerClipboard,
        destination: PathBuf,
    ) -> Result<(), ExplorerError> {
        if clipboard.paths.is_empty() {
            return Err(ExplorerError::InvalidOperation("clipboard is empty".into()));
        }
        let mut conflicts = Vec::new();
        for source in &clipboard.paths {
            let file_name = source.file_name().ok_or_else(|| {
                ExplorerError::InvalidOperation("clipboard path has no name".into())
            })?;
            let target = destination.join(file_name);
            if !(clipboard.mode == ExplorerClipboardMode::Copy
                && source.as_path() == target.as_path())
            {
                validate_transfer_destination(source, &target)?;
            }
            let permission = match clipboard.mode {
                ExplorerClipboardMode::Copy => PermissionAction::WriteFile,
                ExplorerClipboardMode::Cut => PermissionAction::MoveFile,
            };
            self.authorize(session, storage, permission, &target)?;
            if target.exists() {
                conflicts.push((source.clone(), target));
            }
        }

        if state.confirm_name_conflicts && !conflicts.is_empty() {
            let (source, target) = conflicts[0].clone();
            state.pending_conflict = Some(ExplorerConflict {
                source,
                target,
                remaining: conflicts.len(),
            });
            state.pending_transfer = Some(ExplorerPendingTransfer {
                clipboard,
                destination,
                conflicts,
                current_conflict: 0,
                resolutions: BTreeMap::new(),
            });
            state.operation = Some(ExplorerOperationProgress {
                phase: ExplorerOperationPhase::WaitingForConflict,
                label: "Waiting for conflict resolution".to_string(),
                completed_items: 0,
                total_items: None,
                completed_bytes: 0,
                total_bytes: None,
                cancellable: true,
            });
            return Ok(());
        }

        let resolutions = conflicts
            .into_iter()
            .map(|(_, target)| (target, ExplorerConflictAction::KeepBoth))
            .collect();
        self.execute_transfer(
            state,
            session,
            platform,
            storage,
            clipboard,
            destination,
            resolutions,
        )
    }

    fn resolve_pending_transfer(
        &self,
        state: &mut ExplorerState,
        session: Option<&AuthSession>,
        platform: &dyn Platform,
        storage: &StorageManager,
        action: ExplorerConflictAction,
        apply_to_all: bool,
    ) -> Result<(), ExplorerError> {
        if action == ExplorerConflictAction::Cancel {
            state.pending_conflict = None;
            state.pending_transfer = None;
            state.operation = None;
            state.set_success("Transfer cancelled");
            return Ok(());
        }

        let Some(pending) = state.pending_transfer.as_mut() else {
            return Err(ExplorerError::InvalidOperation(
                "no transfer conflict is pending".into(),
            ));
        };
        let (_, target) = pending
            .conflicts
            .get(pending.current_conflict)
            .cloned()
            .ok_or_else(|| ExplorerError::InvalidOperation("invalid conflict state".into()))?;
        pending.resolutions.insert(target, action);
        if apply_to_all {
            for (_, target) in pending.conflicts.iter().skip(pending.current_conflict + 1) {
                pending.resolutions.insert(target.clone(), action);
            }
            pending.current_conflict = pending.conflicts.len();
        } else {
            pending.current_conflict += 1;
        }

        if let Some((source, target)) = pending.conflicts.get(pending.current_conflict).cloned() {
            state.pending_conflict = Some(ExplorerConflict {
                source,
                target,
                remaining: pending.conflicts.len() - pending.current_conflict,
            });
            return Ok(());
        }

        let pending = state
            .pending_transfer
            .take()
            .expect("pending transfer checked above");
        state.pending_conflict = None;
        self.execute_transfer(
            state,
            session,
            platform,
            storage,
            pending.clipboard,
            pending.destination,
            pending.resolutions,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn execute_transfer(
        &self,
        state: &mut ExplorerState,
        session: Option<&AuthSession>,
        platform: &dyn Platform,
        storage: &StorageManager,
        clipboard: ExplorerClipboard,
        destination: PathBuf,
        resolutions: BTreeMap<PathBuf, ExplorerConflictAction>,
    ) -> Result<(), ExplorerError> {
        state.operation = Some(ExplorerOperationProgress {
            phase: ExplorerOperationPhase::Executing,
            label: match clipboard.mode {
                ExplorerClipboardMode::Copy => "Copying items".to_string(),
                ExplorerClipboardMode::Cut => "Moving items".to_string(),
            },
            completed_items: 0,
            total_items: Some(clipboard.paths.len()),
            completed_bytes: 0,
            total_bytes: None,
            cancellable: true,
        });

        let mut succeeded = Vec::new();
        let mut skipped = 0usize;
        for (index, source) in clipboard.paths.iter().enumerate() {
            let file_name = source.file_name().ok_or_else(|| {
                ExplorerError::InvalidOperation("clipboard path has no name".into())
            })?;
            let original_target = destination.join(file_name);
            let resolution = resolutions
                .get(&original_target)
                .copied()
                .unwrap_or(ExplorerConflictAction::KeepBoth);
            if resolution == ExplorerConflictAction::Skip {
                skipped += 1;
                if let Some(operation) = state.operation.as_mut() {
                    operation.completed_items = index + 1;
                }
                continue;
            }
            let target =
                if original_target.exists() && resolution == ExplorerConflictAction::KeepBoth {
                    unique_sibling_path(&original_target)
                } else {
                    original_target.clone()
                };
            if original_target.exists() && resolution == ExplorerConflictAction::Replace {
                move_existing_to_trash(storage, session, platform, &original_target)?;
            }

            match clipboard.mode {
                ExplorerClipboardMode::Copy => {
                    copy_path_staged(source, &target)?;
                    self.audit(
                        storage,
                        session,
                        PermissionAction::WriteFile,
                        &target,
                        AuditOutcome::Success,
                        "copy_paste",
                    )?;
                }
                ExplorerClipboardMode::Cut => {
                    match platform.rename_path(source, &target) {
                        Ok(()) => {}
                        Err(PlatformError::CrossDevice { .. }) => {
                            copy_path_staged(source, &target)?;
                            remove_source_path(source)?;
                        }
                        Err(error) => return Err(error.into()),
                    }
                    self.audit(
                        storage,
                        session,
                        PermissionAction::MoveFile,
                        &target,
                        AuditOutcome::Success,
                        "cut_paste",
                    )?;
                }
            }
            succeeded.push(target);
            if let Some(operation) = state.operation.as_mut() {
                operation.completed_items = index + 1;
            }
        }

        if clipboard.mode == ExplorerClipboardMode::Cut {
            state.clipboard = None;
        }
        state.operation = None;
        state.selected_paths = succeeded.into_iter().collect();
        state.set_success(format!(
            "Transferred {} item(s){}",
            state.selected_paths.len(),
            if skipped > 0 {
                format!(", skipped {skipped}")
            } else {
                String::new()
            }
        ));

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

fn selected_paths_or_error(state: &ExplorerState) -> Result<Vec<PathBuf>, ExplorerError> {
    let paths = state.effective_selected_paths();
    if paths.is_empty() {
        Err(ExplorerError::InvalidOperation("nothing selected".into()))
    } else {
        Ok(paths)
    }
}

fn navigate_to(state: &mut ExplorerState, target: PathBuf, push_history: bool) {
    if target == state.current_path {
        return;
    }
    if push_history {
        state.back_history.push(state.current_path.clone());
        state.forward_history.clear();
    }
    state.current_path = target;
    state.query.clear();
    state.clear_selection();
    state.selection_cleared = false;
    state.selected_index = 0;
    state.viewport_offset = 0;
    state.viewport_follows_focus = true;
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
    let left = if case_sensitive {
        left.to_string()
    } else {
        left.to_lowercase()
    };
    let right = if case_sensitive {
        right.to_string()
    } else {
        right.to_lowercase()
    };
    let left = left.as_bytes();
    let right = right.as_bytes();
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

        let order = left[left_index].cmp(&right[right_index]);
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
        return Err(ExplorerError::InvalidOperation(
            "source and destination are the same".into(),
        ));
    }
    let metadata = fs::symlink_metadata(source).map_err(|error| ExplorerError::Io {
        operation: "inspect transfer source",
        path: source.to_path_buf(),
        message: error.to_string(),
    })?;
    if metadata.file_type().is_symlink() {
        return Err(ExplorerError::BlockedPath(
            "copying or cross-filesystem moving symbolic links is blocked".into(),
        ));
    }
    if metadata.is_dir() && target.starts_with(source) {
        return Err(ExplorerError::InvalidOperation(
            "a directory cannot be transferred into itself or its descendant".into(),
        ));
    }
    if target
        .parent()
        .and_then(|parent| fs::metadata(parent).ok())
        .is_some_and(|metadata| metadata.permissions().readonly())
    {
        return Err(ExplorerError::InvalidOperation(
            "destination directory is read-only".into(),
        ));
    }
    Ok(())
}

fn unique_sibling_path(path: &Path) -> PathBuf {
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
        if !candidate.exists() {
            return candidate;
        }
    }
    unreachable!("u32 suffix iterator is unbounded for practical purposes")
}

fn copy_path_staged(source: &Path, target: &Path) -> Result<(), ExplorerError> {
    let parent = target.parent().ok_or_else(|| {
        ExplorerError::InvalidOperation("transfer target has no parent directory".into())
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
        fs::rename(&temporary, target).map_err(|error| ExplorerError::Io {
            operation: "commit staged copy",
            path: target.to_path_buf(),
            message: error.to_string(),
        })
    });
    if result.is_err() {
        let _ = if temporary.is_dir() {
            fs::remove_dir_all(&temporary)
        } else {
            fs::remove_file(&temporary)
        };
    }
    result
}

fn copy_file_chunked(source: &Path, target: &Path) -> Result<(), ExplorerError> {
    let metadata = fs::symlink_metadata(source).map_err(|error| ExplorerError::Io {
        operation: "inspect copy source",
        path: source.to_path_buf(),
        message: error.to_string(),
    })?;
    if metadata.file_type().is_symlink() {
        return Err(ExplorerError::BlockedPath(
            "copying symbolic links is blocked".into(),
        ));
    }
    let mut input = fs::File::open(source).map_err(|error| ExplorerError::Io {
        operation: "open copy source",
        path: source.to_path_buf(),
        message: error.to_string(),
    })?;
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(target)
        .map_err(|error| ExplorerError::Io {
            operation: "create staged copy",
            path: target.to_path_buf(),
            message: error.to_string(),
        })?;
    let mut buffer = vec![0u8; 256 * 1024];
    loop {
        let read = input.read(&mut buffer).map_err(|error| ExplorerError::Io {
            operation: "read copy source",
            path: source.to_path_buf(),
            message: error.to_string(),
        })?;
        if read == 0 {
            break;
        }
        output
            .write_all(&buffer[..read])
            .map_err(|error| ExplorerError::Io {
                operation: "write staged copy",
                path: target.to_path_buf(),
                message: error.to_string(),
            })?;
    }
    output.sync_all().map_err(|error| ExplorerError::Io {
        operation: "sync staged copy",
        path: target.to_path_buf(),
        message: error.to_string(),
    })
}

fn remove_source_path(path: &Path) -> Result<(), ExplorerError> {
    let metadata = fs::symlink_metadata(path).map_err(|error| ExplorerError::Io {
        operation: "inspect move source",
        path: path.to_path_buf(),
        message: error.to_string(),
    })?;
    let result = if metadata.is_dir() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    };
    result.map_err(|error| ExplorerError::Io {
        operation: "remove committed move source",
        path: path.to_path_buf(),
        message: error.to_string(),
    })
}

fn move_existing_to_trash(
    storage: &StorageManager,
    session: Option<&AuthSession>,
    platform: &dyn Platform,
    path: &Path,
) -> Result<(), ExplorerError> {
    fs::create_dir_all(&storage.layout().trash_path).map_err(|error| ExplorerError::Io {
        operation: "create trash directory",
        path: storage.layout().trash_path.clone(),
        message: error.to_string(),
    })?;
    let trash_path = unique_trash_path(&storage.layout().trash_path, path);
    platform.rename_path(path, &trash_path)?;
    let mut trash = storage.load_trash()?;
    trash.records.push(TrashRecord {
        original_path: path.to_path_buf(),
        trash_path,
        actor: session
            .map(|session| session.username.clone())
            .unwrap_or_else(|| "Guest".to_string()),
        timestamp_epoch_ms: unix_millis(),
    });
    storage.save_trash(&trash)?;
    Ok(())
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
    let metadata = fs::symlink_metadata(source).map_err(|error| ExplorerError::Io {
        operation: "read copy source",
        path: source.to_path_buf(),
        message: error.to_string(),
    })?;

    if metadata.file_type().is_symlink() {
        return Err(ExplorerError::BlockedPath(
            "copying symbolic links is blocked".into(),
        ));
    }

    if metadata.is_dir() {
        copy_directory(source, target)
    } else {
        copy_file_chunked(source, target)
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
