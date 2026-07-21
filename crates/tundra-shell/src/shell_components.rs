use crate::{CellPosition, rect_contains};
use ratatui::layout::Rect;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ShellComponent {
    CompactHome,
    TopBar,
    Home,
    ClockButton,
    Clock,
    ClockNewButton,
    ClockEntryList,
    ClockCreateDialog,
    ClockCreateInput,
    ClockCreateAlarmButton,
    ClockCreateCountdownButton,
    Diagnostics,
    DiagnosticsRepairDialog,
    LoginUserList,
    LoginUsername,
    LoginPassword,
    LoginPasswordVisibility,
    HomeLogout,
    SetupLanguage,
    SetupTimezone,
    SetupAdminUsername,
    SetupAdminPassword,
    SetupAdminPasswordConfirm,
    SetupAdminHint,
    SetupSubmit,
    SetupAppearanceShape,
    SetupAppearanceThemeColor,
    SetupAppearanceThemeCustom,
    SetupAppearanceAccentColor,
    SetupAppearanceAccentCustom,
    SetupAppearanceSubmit,
    SetupCustomColorDialog,
    BootstrapUsername,
    BootstrapPassword,
    Explorer,
    Launcher,
    Editor,
    UserManagement,
    StatusBar,
    ExitDialog,
    TimeSyncDialog,
    NotificationDialog,
    ContextMenu,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ShellHitLayer {
    ShellModal,
    ShellChrome,
    AppOverlay,
    AppContent,
}

impl ShellHitLayer {
    const fn priority(self) -> u8 {
        match self {
            Self::ShellModal => 3,
            Self::ShellChrome => 2,
            Self::AppOverlay => 1,
            Self::AppContent => 0,
        }
    }
}

impl ShellComponent {
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::CompactHome => "CompactHome",
            Self::TopBar => "TopBar",
            Self::Home => "Home",
            Self::ClockButton => "ClockButton",
            Self::Clock => "Clock",
            Self::ClockNewButton => "ClockNewButton",
            Self::ClockEntryList => "ClockEntryList",
            Self::ClockCreateDialog => "ClockCreateDialog",
            Self::ClockCreateInput => "ClockCreateInput",
            Self::ClockCreateAlarmButton => "ClockCreateAlarmButton",
            Self::ClockCreateCountdownButton => "ClockCreateCountdownButton",
            Self::Diagnostics => "Diagnostics",
            Self::DiagnosticsRepairDialog => "DiagnosticsRepairDialog",
            Self::LoginUserList => "LoginUserList",
            Self::LoginUsername => "LoginUsername",
            Self::LoginPassword => "LoginPassword",
            Self::LoginPasswordVisibility => "LoginPasswordVisibility",
            Self::HomeLogout => "HomeLogout",
            Self::SetupLanguage => "SetupLanguage",
            Self::SetupTimezone => "SetupTimezone",
            Self::SetupAdminUsername => "SetupAdminUsername",
            Self::SetupAdminPassword => "SetupAdminPassword",
            Self::SetupAdminPasswordConfirm => "SetupAdminPasswordConfirm",
            Self::SetupAdminHint => "SetupAdminHint",
            Self::SetupSubmit => "SetupSubmit",
            Self::SetupAppearanceShape => "SetupAppearanceShape",
            Self::SetupAppearanceThemeColor => "SetupAppearanceThemeColor",
            Self::SetupAppearanceThemeCustom => "SetupAppearanceThemeCustom",
            Self::SetupAppearanceAccentColor => "SetupAppearanceAccentColor",
            Self::SetupAppearanceAccentCustom => "SetupAppearanceAccentCustom",
            Self::SetupAppearanceSubmit => "SetupAppearanceSubmit",
            Self::SetupCustomColorDialog => "SetupCustomColorDialog",
            Self::BootstrapUsername => "BootstrapUsername",
            Self::BootstrapPassword => "BootstrapPassword",
            Self::Explorer => "Explorer",
            Self::Launcher => "Launcher",
            Self::Editor => "Editor",
            Self::UserManagement => "UserManagement",
            Self::StatusBar => "StatusBar",
            Self::ExitDialog => "ExitDialog",
            Self::TimeSyncDialog => "TimeSyncDialog",
            Self::NotificationDialog => "NotificationDialog",
            Self::ContextMenu => "ContextMenu",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClickKind {
    Single,
    Double,
}

// Shell-local stand-in until the UI foundation exports hit testing and overlay
// ownership. Expected future imports: tundra_ui::{ComponentId, HitMap,
// HitRegion, OverlayCapture, OverlayLayer}.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShellHitRegion {
    pub component: ShellComponent,
    pub area: Rect,
    pub layer: ShellHitLayer,
}

impl ShellHitRegion {
    pub const fn new(component: ShellComponent, area: Rect, layer: ShellHitLayer) -> Self {
        Self {
            component,
            area,
            layer,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellHitMap {
    terminal_size: CellPosition,
    generation: u64,
    regions: Vec<ShellHitRegion>,
}

impl ShellHitMap {
    pub(crate) fn new(
        terminal_size: CellPosition,
        generation: u64,
        regions: Vec<ShellHitRegion>,
    ) -> Self {
        Self {
            terminal_size,
            generation,
            regions,
        }
    }

    pub(crate) fn empty(terminal_size: CellPosition) -> Self {
        Self::new(terminal_size, 0, Vec::new())
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn terminal_size(&self) -> CellPosition {
        self.terminal_size
    }

    pub fn regions(&self) -> &[ShellHitRegion] {
        &self.regions
    }

    pub fn region_at(&self, coordinates: CellPosition) -> Option<&ShellHitRegion> {
        self.regions
            .iter()
            .enumerate()
            .filter(|(_, region)| rect_contains(region.area, coordinates))
            .max_by_key(|(index, region)| (region.layer.priority(), *index))
            .map(|(_, region)| region)
    }

    pub fn target_at(&self, coordinates: CellPosition) -> Option<ShellComponent> {
        self.region_at(coordinates).map(|region| region.component)
    }

    pub fn layer_at(&self, coordinates: CellPosition) -> Option<ShellHitLayer> {
        self.region_at(coordinates).map(|region| region.layer)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShellPopup {
    pub owner: Option<ShellComponent>,
    pub anchor: CellPosition,
}
