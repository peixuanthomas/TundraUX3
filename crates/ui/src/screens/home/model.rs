use crate::home_icons::{AssetError, HomeIcon, HomeIconCatalog, RuntimeAsciiAssets};
use crate::screens::diagnostics::DebugDiagnosticsViewModel;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HomeDisplayMode {
    Debug,
    User,
    Auth,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellEntry {
    pub label: String,
    pub description: String,
}

impl ShellEntry {
    pub fn new(label: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            description: description.into(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct HomeViewModel {
    display_mode: HomeDisplayMode,
    diagnostics: Option<DebugDiagnosticsViewModel>,
    home_icon_assets: Option<RuntimeAsciiAssets>,
    pub(crate) current_user: Option<String>,
    pub(crate) current_time: Option<String>,
    entries: Vec<ShellEntry>,
    selected_entry_index: usize,
    logout_visible: bool,
    logout_selected: bool,
}

impl PartialEq for HomeViewModel {
    fn eq(&self, other: &Self) -> bool {
        self.display_mode == other.display_mode
            && self.diagnostics == other.diagnostics
            && self.current_user == other.current_user
            && self.current_time == other.current_time
            && self.entries == other.entries
            && self.selected_entry_index == other.selected_entry_index
            && self.logout_visible == other.logout_visible
            && self.logout_selected == other.logout_selected
    }
}

impl Eq for HomeViewModel {}

impl HomeViewModel {
    pub fn debug(diagnostics: DebugDiagnosticsViewModel) -> Self {
        Self {
            display_mode: HomeDisplayMode::Debug,
            diagnostics: Some(diagnostics),
            home_icon_assets: None,
            current_user: None,
            current_time: None,
            entries: Vec::new(),
            selected_entry_index: 0,
            logout_visible: false,
            logout_selected: false,
        }
    }

    pub fn user(
        current_user: impl Into<String>,
        current_time: impl Into<String>,
        entries: Vec<ShellEntry>,
    ) -> Self {
        Self::user_with_selection(current_user, current_time, entries, 0)
    }

    pub fn user_with_selection(
        current_user: impl Into<String>,
        current_time: impl Into<String>,
        entries: Vec<ShellEntry>,
        selected_entry_index: usize,
    ) -> Self {
        Self::try_user_with_selection(current_user, current_time, entries, selected_entry_index)
            .expect("default ASCII home icon assets must load")
    }

    pub fn try_user(
        current_user: impl Into<String>,
        current_time: impl Into<String>,
        entries: Vec<ShellEntry>,
    ) -> Result<Self, AssetError> {
        Self::try_user_with_selection(current_user, current_time, entries, 0)
    }

    pub fn try_user_with_selection(
        current_user: impl Into<String>,
        current_time: impl Into<String>,
        entries: Vec<ShellEntry>,
        selected_entry_index: usize,
    ) -> Result<Self, AssetError> {
        let home_icon_assets = RuntimeAsciiAssets::load_default()?;
        Ok(Self::user_with_selection_and_icon_assets(
            current_user,
            current_time,
            entries,
            selected_entry_index,
            home_icon_assets,
        ))
    }

    pub fn user_with_icon_assets(
        current_user: impl Into<String>,
        current_time: impl Into<String>,
        entries: Vec<ShellEntry>,
        home_icon_assets: RuntimeAsciiAssets,
    ) -> Self {
        Self::user_with_selection_and_icon_assets(
            current_user,
            current_time,
            entries,
            0,
            home_icon_assets,
        )
    }

    pub fn user_with_selection_and_icon_assets(
        current_user: impl Into<String>,
        current_time: impl Into<String>,
        entries: Vec<ShellEntry>,
        selected_entry_index: usize,
        home_icon_assets: RuntimeAsciiAssets,
    ) -> Self {
        let selected_entry_index = if entries.is_empty() {
            0
        } else {
            selected_entry_index.min(entries.len() - 1)
        };

        Self {
            display_mode: HomeDisplayMode::User,
            diagnostics: None,
            home_icon_assets: Some(home_icon_assets),
            current_user: Some(current_user.into()),
            current_time: Some(current_time.into()),
            entries,
            selected_entry_index,
            logout_visible: false,
            logout_selected: false,
        }
    }

    /// Enables Debug presentation while preserving the normal Home content.
    pub fn with_debug_diagnostics(mut self, diagnostics: DebugDiagnosticsViewModel) -> Self {
        self.display_mode = HomeDisplayMode::Debug;
        self.diagnostics = Some(diagnostics);
        self
    }

    /// Adds an authenticated account summary and its Logout control.
    pub fn with_account_logout(mut self, current_user: impl Into<String>, selected: bool) -> Self {
        self.current_user = Some(current_user.into());
        self.logout_visible = true;
        self.logout_selected = selected;
        self
    }

    pub fn logout_visible(&self) -> bool {
        self.logout_visible
    }

    pub fn logout_selected(&self) -> bool {
        self.logout_selected
    }

    pub fn display_mode(&self) -> HomeDisplayMode {
        self.display_mode
    }

    pub fn diagnostics(&self) -> Option<&DebugDiagnosticsViewModel> {
        self.diagnostics.as_ref()
    }

    pub fn entries(&self) -> &[ShellEntry] {
        &self.entries
    }

    pub fn home_icon_catalog(&self) -> Option<&HomeIconCatalog> {
        self.home_icon_assets
            .as_ref()
            .map(RuntimeAsciiAssets::home_icon_catalog)
    }

    pub fn home_icon_for_label(&self, label: &str) -> Option<&HomeIcon> {
        self.home_icon_assets
            .as_ref()
            .and_then(|assets| assets.home_icon_for_label(label))
    }

    pub fn selected_entry_index(&self) -> usize {
        self.selected_entry_index
    }
}
