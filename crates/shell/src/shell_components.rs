use crate::CellPosition;
use ratatui::layout::Rect;
use ui::{HitKind, HitLayer, HitMap, HitTarget, Point, UiId};

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
    Settings,
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

impl From<ShellHitLayer> for HitLayer {
    fn from(layer: ShellHitLayer) -> Self {
        match layer {
            ShellHitLayer::ShellModal => Self::ShellModal,
            ShellHitLayer::ShellChrome => Self::ShellChrome,
            ShellHitLayer::AppOverlay => Self::AppOverlay,
            ShellHitLayer::AppContent => Self::AppContent,
        }
    }
}

impl ShellComponent {
    pub(crate) fn focus_id(self) -> UiId {
        UiId::new(self.label())
    }

    pub(crate) fn from_focus_id(id: &UiId, focus_order: &[Self]) -> Option<Self> {
        focus_order
            .iter()
            .copied()
            .find(|component| component.label() == id.as_str())
    }

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
            Self::Settings => "Settings",
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

// Shell-facing hit region data retained for compatibility with existing routing APIs.
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
    hit_map: HitMap,
    region_ids: Vec<UiId>,
}

impl ShellHitMap {
    pub(crate) fn new(
        terminal_size: CellPosition,
        generation: u64,
        regions: Vec<ShellHitRegion>,
    ) -> Self {
        let mut hit_map = HitMap::new();
        let mut region_ids = Vec::with_capacity(regions.len());

        for (index, region) in regions.iter().enumerate() {
            let id = UiId::new(format!("shell-hit-region-{index}"));
            hit_map.register(
                HitTarget::new(id.clone(), region.area, HitKind::custom("shell-hit-region"))
                    .with_layer(region.layer.into()),
            );
            region_ids.push(id);
        }

        Self {
            terminal_size,
            generation,
            regions,
            hit_map,
            region_ids,
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
        let target = self.hit_map.hit(Point::new(coordinates.0, coordinates.1))?;
        let index = self.region_ids.iter().position(|id| id == &target.id)?;
        self.regions.get(index)
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
