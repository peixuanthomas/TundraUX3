use crate::home_icons::{AssetError, RuntimeAsciiAssets};

pub use app::explorer::{ExplorerQuickLocationKind, ExplorerSortDirection};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ExplorerSortColumn {
    #[default]
    Name,
    Type,
    Size,
    Modified,
}

impl ExplorerSortColumn {
    pub const ALL: [Self; 4] = [Self::Name, Self::Type, Self::Size, Self::Modified];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Name => "Name",
            Self::Type => "Type",
            Self::Size => "Size",
            Self::Modified => "Modified",
        }
    }
}

pub(crate) const fn explorer_sort_direction_icon_key(
    direction: ExplorerSortDirection,
) -> &'static str {
    match direction {
        ExplorerSortDirection::Ascending => "sort_asc",
        ExplorerSortDirection::Descending => "sort_desc",
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExplorerToolbarAction {
    Back,
    Forward,
    Up,
    Refresh,
    New,
    Cut,
    Copy,
    Paste,
    Rename,
    Delete,
    Restore,
    DumpTrash,
    Sort,
    Options,
}

impl ExplorerToolbarAction {
    pub const ALL: [Self; 14] = [
        Self::Back,
        Self::Forward,
        Self::Up,
        Self::Refresh,
        Self::New,
        Self::Cut,
        Self::Copy,
        Self::Paste,
        Self::Rename,
        Self::Delete,
        Self::Restore,
        Self::DumpTrash,
        Self::Sort,
        Self::Options,
    ];

    pub const REGULAR: [Self; 12] = [
        Self::Back,
        Self::Forward,
        Self::Up,
        Self::Refresh,
        Self::New,
        Self::Cut,
        Self::Copy,
        Self::Paste,
        Self::Rename,
        Self::Delete,
        Self::Sort,
        Self::Options,
    ];

    pub const TRASH: [Self; 7] = [
        Self::Back,
        Self::Forward,
        Self::Refresh,
        Self::Restore,
        Self::DumpTrash,
        Self::Sort,
        Self::Options,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Back => "Back",
            Self::Forward => "Forward",
            Self::Up => "Up",
            Self::Refresh => "Refresh",
            Self::New => "New",
            Self::Cut => "Cut",
            Self::Copy => "Copy",
            Self::Paste => "Paste",
            Self::Rename => "Rename",
            Self::Delete => "Delete",
            Self::Restore => "Restore",
            Self::DumpTrash => "Dump Trash",
            Self::Sort => "Sort",
            Self::Options => "Options",
        }
    }

    pub const fn icon_key(self) -> &'static str {
        match self {
            Self::Back => "back",
            Self::Forward => "forward",
            Self::Up => "up",
            Self::Refresh => "refresh",
            Self::New => "new",
            Self::Cut => "cut",
            Self::Copy => "copy",
            Self::Paste => "paste",
            Self::Rename => "rename",
            Self::Delete => "delete",
            Self::Restore => "refresh",
            Self::DumpTrash => "delete",
            Self::Sort => "sort_asc",
            Self::Options => "options",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExplorerToolbarButtonViewModel {
    pub action: ExplorerToolbarAction,
    pub label: String,
    pub icon_key: String,
    pub enabled: bool,
    pub active: bool,
}

impl ExplorerToolbarButtonViewModel {
    pub fn new(action: ExplorerToolbarAction, enabled: bool) -> Self {
        Self {
            action,
            label: action.label().to_string(),
            icon_key: action.icon_key().to_string(),
            enabled,
            active: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ExplorerToolbarViewModel {
    pub buttons: Vec<ExplorerToolbarButtonViewModel>,
}

impl ExplorerToolbarViewModel {
    pub fn standard(can_go_back: bool, can_go_forward: bool) -> Self {
        Self {
            buttons: ExplorerToolbarAction::REGULAR
                .into_iter()
                .map(|action| {
                    let enabled = match action {
                        ExplorerToolbarAction::Back => can_go_back,
                        ExplorerToolbarAction::Forward => can_go_forward,
                        _ => true,
                    };
                    ExplorerToolbarButtonViewModel::new(action, enabled)
                })
                .collect(),
        }
    }

    pub fn trash(
        can_go_back: bool,
        can_go_forward: bool,
        can_restore: bool,
        can_dump: bool,
    ) -> Self {
        Self {
            buttons: ExplorerToolbarAction::TRASH
                .into_iter()
                .map(|action| {
                    let enabled = match action {
                        ExplorerToolbarAction::Back => can_go_back,
                        ExplorerToolbarAction::Forward => can_go_forward,
                        ExplorerToolbarAction::Restore => can_restore,
                        ExplorerToolbarAction::DumpTrash => can_dump,
                        _ => true,
                    };
                    ExplorerToolbarButtonViewModel::new(action, enabled)
                })
                .collect(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExplorerEntryViewModel {
    pub name: String,
    pub kind: String,
    pub size: Option<String>,
    pub modified: Option<String>,
    pub attributes: Vec<String>,
    pub selected: bool,
}

/// Optional stable identity and interaction state layered over a legacy entry.
/// The vector on [`ExplorerViewModel`] is index-aligned with `entries`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExplorerEntryPresentationViewModel {
    pub id: String,
    pub path: String,
    pub icon_key: String,
    pub is_directory: bool,
    pub selected: bool,
    pub focused: bool,
    pub cut: bool,
    pub drop_target: bool,
    pub metadata_warning: Option<String>,
    pub original_path: Option<String>,
}

impl ExplorerEntryPresentationViewModel {
    pub fn new(
        id: impl Into<String>,
        path: impl Into<String>,
        icon_key: impl Into<String>,
        is_directory: bool,
    ) -> Self {
        Self {
            id: id.into(),
            path: path.into(),
            icon_key: icon_key.into(),
            is_directory,
            selected: false,
            focused: false,
            cut: false,
            drop_target: false,
            metadata_warning: None,
            original_path: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExplorerQuickLocationViewModel {
    pub id: String,
    pub label: String,
    pub path: String,
    pub icon_key: String,
    pub kind: ExplorerQuickLocationKind,
    pub current: bool,
    pub enabled: bool,
    pub drop_target: bool,
}

impl ExplorerQuickLocationViewModel {
    pub fn new(
        id: impl Into<String>,
        label: impl Into<String>,
        path: impl Into<String>,
        icon_key: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            path: path.into(),
            icon_key: icon_key.into(),
            kind: ExplorerQuickLocationKind::Directory,
            current: false,
            enabled: true,
            drop_target: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExplorerBreadcrumbViewModel {
    pub id: String,
    pub label: String,
    pub path: String,
    pub enabled: bool,
    pub drop_target: bool,
}

impl ExplorerBreadcrumbViewModel {
    pub fn new(id: impl Into<String>, label: impl Into<String>, path: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            path: path.into(),
            enabled: true,
            drop_target: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExplorerProgressStage {
    Scanning,
    CheckingConflicts,
    Copying,
    Moving,
    Deleting,
    Finishing,
}

impl ExplorerProgressStage {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Scanning => "Scanning",
            Self::CheckingConflicts => "Checking conflicts",
            Self::Copying => "Copying",
            Self::Moving => "Moving",
            Self::Deleting => "Deleting",
            Self::Finishing => "Finishing",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExplorerOperationProgressViewModel {
    pub phase: ExplorerProgressStage,
    pub label: String,
    pub completed_items: u64,
    pub total_items: Option<u64>,
    pub completed_bytes: u64,
    pub total_bytes: Option<u64>,
    pub cancellable: bool,
    pub cancel_label: String,
}

impl ExplorerOperationProgressViewModel {
    pub fn percent(&self) -> Option<u16> {
        if let Some(total_bytes) = self.total_bytes.filter(|total| *total > 0) {
            return Some(
                self.completed_bytes
                    .saturating_mul(100)
                    .checked_div(total_bytes)
                    .unwrap_or(0)
                    .min(100) as u16,
            );
        }
        self.total_items.filter(|total| *total > 0).map(|total| {
            self.completed_items
                .saturating_mul(100)
                .checked_div(total)
                .unwrap_or(0)
                .min(100) as u16
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExplorerContextMenuItemViewModel {
    pub id: String,
    pub label: String,
    pub shortcut: Option<String>,
    pub enabled: bool,
    pub dangerous: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExplorerContextMenuViewModel {
    pub x: u16,
    pub y: u16,
    pub title: String,
    pub items: Vec<ExplorerContextMenuItemViewModel>,
    pub selected_index: Option<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExplorerNameDialogKind {
    NewFolder,
    NewTextFile,
    Rename,
    RestoreDestination,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExplorerNameDialogViewModel {
    pub kind: ExplorerNameDialogKind,
    pub title: String,
    pub prompt: String,
    pub value: String,
    pub error: Option<String>,
    pub confirm_label: String,
    pub cancel_label: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExplorerOptionViewModel {
    pub id: String,
    pub label: String,
    pub value: String,
    pub enabled: bool,
    pub selected: bool,
    pub focused: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExplorerOptionsViewModel {
    pub title: String,
    pub options: Vec<ExplorerOptionViewModel>,
    pub close_label: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExplorerConflictChoice {
    KeepBoth,
    Replace,
    Skip,
    Cancel,
}

impl ExplorerConflictChoice {
    pub const fn label(self) -> &'static str {
        match self {
            Self::KeepBoth => "Keep both",
            Self::Replace => "Replace",
            Self::Skip => "Skip",
            Self::Cancel => "Cancel",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExplorerConflictViewModel {
    pub title: String,
    pub source: String,
    pub destination: String,
    pub choices: Vec<ExplorerConflictChoice>,
    pub selected_choice: ExplorerConflictChoice,
    pub apply_to_remaining: bool,
    pub allow_apply_to_remaining: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExplorerPropertyViewModel {
    pub label: String,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExplorerPropertiesViewModel {
    pub title: String,
    pub properties: Vec<ExplorerPropertyViewModel>,
    pub close_label: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExplorerOverlayViewModel {
    ContextMenu(ExplorerContextMenuViewModel),
    Name(ExplorerNameDialogViewModel),
    Options(ExplorerOptionsViewModel),
    Conflict(ExplorerConflictViewModel),
    Properties(ExplorerPropertiesViewModel),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExplorerSearchViewModel {
    pub query: String,
    pub active: bool,
    pub match_count: Option<usize>,
}

impl ExplorerSearchViewModel {
    pub fn new(query: impl Into<String>, active: bool, match_count: Option<usize>) -> Self {
        Self {
            query: query.into(),
            active,
            match_count,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExplorerDialogViewModel {
    pub title: String,
    pub message: String,
    pub confirm_label: String,
    pub cancel_label: String,
}

impl ExplorerDialogViewModel {
    pub fn new(
        title: impl Into<String>,
        message: impl Into<String>,
        confirm_label: impl Into<String>,
        cancel_label: impl Into<String>,
    ) -> Self {
        Self {
            title: title.into(),
            message: message.into(),
            confirm_label: confirm_label.into(),
            cancel_label: cancel_label.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExplorerViewModel {
    pub current_path: String,
    pub address_value: String,
    pub address_editing: bool,
    pub is_trash: bool,
    pub entries: Vec<ExplorerEntryViewModel>,
    pub selected_index: Option<usize>,
    pub search: Option<ExplorerSearchViewModel>,
    pub show_hidden: bool,
    pub message: Option<String>,
    pub error: Option<String>,
    pub pending_dialog: Option<ExplorerDialogViewModel>,
    pub ascii_assets: Option<RuntimeAsciiAssets>,
    pub entry_presentations: Vec<ExplorerEntryPresentationViewModel>,
    pub toolbar: ExplorerToolbarViewModel,
    pub quick_locations: Vec<ExplorerQuickLocationViewModel>,
    pub breadcrumbs: Vec<ExplorerBreadcrumbViewModel>,
    pub sort_column: ExplorerSortColumn,
    pub sort_direction: ExplorerSortDirection,
    pub viewport_offset: usize,
    pub viewport_follows_focus: bool,
    pub show_sidebar: bool,
    pub can_go_back: bool,
    pub can_go_forward: bool,
    pub selected_count: usize,
    pub listing_warning_count: usize,
    pub operation: Option<ExplorerOperationProgressViewModel>,
    pub overlay: Option<ExplorerOverlayViewModel>,
}

impl ExplorerViewModel {
    pub fn new(
        current_path: impl Into<String>,
        entries: Vec<ExplorerEntryViewModel>,
        selected_index: Option<usize>,
    ) -> Self {
        Self::try_new(current_path, entries, selected_index)
            .expect("default ASCII Explorer assets must load")
    }

    pub fn try_new(
        current_path: impl Into<String>,
        entries: Vec<ExplorerEntryViewModel>,
        selected_index: Option<usize>,
    ) -> Result<Self, AssetError> {
        let ascii_assets = RuntimeAsciiAssets::load_default()?;
        Ok(Self::with_ascii_assets(
            current_path,
            entries,
            selected_index,
            ascii_assets,
        ))
    }

    pub fn with_ascii_assets(
        current_path: impl Into<String>,
        entries: Vec<ExplorerEntryViewModel>,
        selected_index: Option<usize>,
        ascii_assets: RuntimeAsciiAssets,
    ) -> Self {
        let selected_count = entries.iter().filter(|entry| entry.selected).count();
        let current_path = current_path.into();
        Self {
            address_value: current_path.clone(),
            current_path,
            address_editing: false,
            is_trash: false,
            entries,
            selected_index,
            search: None,
            show_hidden: false,
            message: None,
            error: None,
            pending_dialog: None,
            ascii_assets: Some(ascii_assets),
            entry_presentations: Vec::new(),
            toolbar: ExplorerToolbarViewModel::standard(false, false),
            quick_locations: Vec::new(),
            breadcrumbs: Vec::new(),
            sort_column: ExplorerSortColumn::Name,
            sort_direction: ExplorerSortDirection::Ascending,
            viewport_offset: 0,
            viewport_follows_focus: true,
            show_sidebar: true,
            can_go_back: false,
            can_go_forward: false,
            selected_count,
            listing_warning_count: 0,
            operation: None,
            overlay: None,
        }
    }

    pub fn set_history_availability(&mut self, can_go_back: bool, can_go_forward: bool) {
        self.can_go_back = can_go_back;
        self.can_go_forward = can_go_forward;
        for button in &mut self.toolbar.buttons {
            match button.action {
                ExplorerToolbarAction::Back => button.enabled = can_go_back,
                ExplorerToolbarAction::Forward => button.enabled = can_go_forward,
                _ => {}
            }
        }
    }

    pub fn entry_presentation(&self, index: usize) -> Option<&ExplorerEntryPresentationViewModel> {
        self.entry_presentations.get(index)
    }

    pub fn effective_selected_count(&self) -> usize {
        if self.selected_count > 0 {
            self.selected_count
        } else if !self.entry_presentations.is_empty() {
            self.entry_presentations
                .iter()
                .filter(|entry| entry.selected)
                .count()
        } else {
            self.entries.iter().filter(|entry| entry.selected).count()
        }
    }

    pub fn selected_entry(&self) -> Option<&ExplorerEntryViewModel> {
        self.selected_index
            .and_then(|selected_index| self.entries.get(selected_index))
    }
}
