use ratatui::Frame;
use ratatui::layout::{HorizontalAlignment, Rect};
use ratatui::style::Style;
use ratatui::widgets::{Clear, Paragraph, Wrap};

use crate::components::{
    Button, ComponentTone, DataTable, Scrollbar, Surface, terminal_width,
    truncate_to_terminal_width,
};
use crate::{AssetError, RenderContext, RuntimeAsciiAssets, TundraTheme};

const GRID_TILE_MIN_WIDTH: u16 = 20;
const GRID_TILE_HEIGHT: u16 = 9;
fn empty_message() -> String {
    i18n::tr!(
        "ui-launcher-no-launcher-items-go-to-explorer-select-a-file-then-right-click-and-choose-add-to-launcher"
    )
}

pub use app::launcher::{LauncherItemStatus, LauncherViewMode};

/// Describes where the Launcher entry originates. Built-in entries are supplied by
/// Tundra itself and are deliberately not written to the user Launcher store.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum LauncherItemSource {
    #[default]
    External,
    BuiltIn,
}

/// Operations that the current Launcher entry permits.
///
/// Keeping these on the view model makes the UI independent of the persistence
/// rules for an entry. In particular, built-in applications can be shown beside
/// external ones without accidentally exposing destructive management actions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LauncherItemCapabilities {
    pub removable: bool,
    pub reorderable: bool,
}

impl LauncherItemCapabilities {
    pub const EXTERNAL: Self = Self {
        removable: true,
        reorderable: true,
    };

    pub const BUILT_IN: Self = Self {
        removable: false,
        reorderable: false,
    };
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LauncherItemViewModel {
    pub id: String,
    pub name: String,
    pub path: String,
    pub type_label: String,
    pub status: LauncherItemStatus,
    pub source: LauncherItemSource,
    pub capabilities: LauncherItemCapabilities,
    pub selected: bool,
}

impl LauncherItemViewModel {
    pub fn new(
        id: impl Into<String>,
        name: impl Into<String>,
        path: impl Into<String>,
        type_label: impl Into<String>,
        status: LauncherItemStatus,
    ) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            path: path.into(),
            type_label: type_label.into(),
            status,
            source: LauncherItemSource::External,
            capabilities: LauncherItemCapabilities::EXTERNAL,
            selected: false,
        }
    }

    /// A non-persisted, fixed Launcher entry supplied by Tundra itself.
    pub fn built_in(descriptor: app::BuiltInApplicationDescriptor) -> Self {
        Self {
            id: descriptor.id.to_string(),
            name: descriptor.localized_name().render_current(),
            path: descriptor.localized_description().render_current(),
            type_label: descriptor.localized_type_label().render_current(),
            status: LauncherItemStatus::Ready,
            source: LauncherItemSource::BuiltIn,
            capabilities: LauncherItemCapabilities::BUILT_IN,
            selected: false,
        }
    }

    pub fn command_line() -> Self {
        Self::built_in(app::COMMAND_LINE_APPLICATION)
    }

    pub fn is_builtin(&self) -> bool {
        self.source == LauncherItemSource::BuiltIn
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LauncherToolbarAction {
    Open,
    Remove,
    Refresh,
    ToggleView,
}

impl LauncherToolbarAction {
    pub fn label(self) -> String {
        match self {
            Self::Open => i18n::tr!("ui-launcher-open"),
            Self::Remove => i18n::tr!("ui-launcher-remove"),
            Self::Refresh => i18n::tr!("ui-launcher-refresh"),
            Self::ToggleView => i18n::tr!("ui-launcher-view"),
        }
    }

    pub const fn shortcut(self) -> &'static str {
        match self {
            Self::Open => "Enter",
            Self::Remove => "Del",
            Self::Refresh => "R/F5",
            Self::ToggleView => "V",
        }
    }

    pub fn description(self) -> String {
        match self {
            Self::Open => i18n::tr!("ui-launcher-open-description"),
            Self::Remove => i18n::tr!("ui-launcher-remove-description"),
            Self::Refresh => i18n::tr!("ui-launcher-refresh-description"),
            Self::ToggleView => i18n::tr!("ui-launcher-view-description"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LauncherToolbarButtonViewModel {
    pub action: LauncherToolbarAction,
    pub label: String,
    pub description: String,
    pub enabled: bool,
}

impl LauncherToolbarButtonViewModel {
    pub fn new(action: LauncherToolbarAction, enabled: bool) -> Self {
        Self {
            action,
            label: action.label().to_string(),
            description: action.description(),
            enabled,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LauncherConfirmationKind {
    Launch,
    Remove,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LauncherConfirmationViewModel {
    pub kind: LauncherConfirmationKind,
    pub title: String,
    pub message: String,
    pub confirm_label: String,
    pub cancel_label: String,
    pub confirm_selected: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LauncherDropSide {
    Before,
    After,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LauncherDropTarget {
    pub item_index: usize,
    pub side: LauncherDropSide,
}

impl LauncherDropTarget {
    pub const fn insertion_index(self) -> usize {
        match self.side {
            LauncherDropSide::Before => self.item_index,
            LauncherDropSide::After => self.item_index.saturating_add(1),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LauncherViewModel {
    pub items: Vec<LauncherItemViewModel>,
    pub sort: Option<crate::TableSort>,
    pub selected_index: Option<usize>,
    pub view_mode: LauncherViewMode,
    pub viewport_offset: usize,
    pub toolbar: Vec<LauncherToolbarButtonViewModel>,
    pub message: Option<String>,
    pub error: Option<String>,
    pub confirmation: Option<LauncherConfirmationViewModel>,
    pub drop_target: Option<LauncherDropTarget>,
    ascii_assets: RuntimeAsciiAssets,
}

impl LauncherViewModel {
    pub fn new(
        items: Vec<LauncherItemViewModel>,
        selected_index: Option<usize>,
        view_mode: LauncherViewMode,
        can_manage: bool,
    ) -> Self {
        Self::try_new(items, selected_index, view_mode, can_manage)
            .expect("default ASCII Launcher assets must load")
    }

    pub fn try_new(
        items: Vec<LauncherItemViewModel>,
        selected_index: Option<usize>,
        view_mode: LauncherViewMode,
        can_manage: bool,
    ) -> Result<Self, AssetError> {
        let ascii_assets = RuntimeAsciiAssets::load_default()?;
        Ok(Self::with_ascii_assets(
            items,
            selected_index,
            view_mode,
            can_manage,
            ascii_assets,
        ))
    }

    pub fn with_ascii_assets(
        items: Vec<LauncherItemViewModel>,
        selected_index: Option<usize>,
        view_mode: LauncherViewMode,
        can_manage: bool,
        ascii_assets: RuntimeAsciiAssets,
    ) -> Self {
        let selected_index = selected_index
            .filter(|index| *index < items.len())
            .or_else(|| (!items.is_empty()).then_some(0));
        let selected_item = selected_index.and_then(|index| items.get(index));
        let can_remove = selected_item.is_some_and(|item| item.capabilities.removable);
        let can_open = selected_item.is_some_and(|item| {
            matches!(
                item.status,
                LauncherItemStatus::Ready
                    | LauncherItemStatus::Changed
                    | LauncherItemStatus::NeedsApproval
            )
        });
        let mut toolbar = vec![LauncherToolbarButtonViewModel::new(
            LauncherToolbarAction::Open,
            can_open,
        )];
        if can_manage && can_remove {
            toolbar.push(LauncherToolbarButtonViewModel::new(
                LauncherToolbarAction::Remove,
                true,
            ));
        }
        toolbar.push(LauncherToolbarButtonViewModel::new(
            LauncherToolbarAction::Refresh,
            true,
        ));
        toolbar.push(LauncherToolbarButtonViewModel::new(
            LauncherToolbarAction::ToggleView,
            true,
        ));

        Self {
            items,
            selected_index,
            view_mode,
            viewport_offset: 0,
            sort: None,
            toolbar,
            message: None,
            error: None,
            confirmation: None,
            drop_target: None,
            ascii_assets,
        }
    }

    pub fn selected_item(&self) -> Option<&LauncherItemViewModel> {
        self.selected_index.and_then(|index| self.items.get(index))
    }

    pub fn default_app_icon(&self) -> Option<&crate::HomeIcon> {
        self.ascii_assets
            .home_icon_catalog()
            .icon_for_key("default")
    }

    pub fn item_icon(&self, item: &LauncherItemViewModel) -> Option<&crate::HomeIcon> {
        if item.is_builtin() {
            self.ascii_assets
                .launcher_icon(&item.id)
                .or_else(|| self.default_app_icon())
        } else {
            self.default_app_icon()
        }
    }

    pub fn item_graphic_path(&self, item: &LauncherItemViewModel) -> Option<std::path::PathBuf> {
        if item.is_builtin() {
            self.ascii_assets.launcher_icon_image_path(&item.id)
        } else {
            None
        }
    }

    pub fn item_graphic_bytes(&self, item: &LauncherItemViewModel) -> Option<&[u8]> {
        if item.is_builtin() {
            self.ascii_assets.launcher_icon_image_bytes(&item.id)
        } else {
            None
        }
    }
}

pub trait LauncherIconRenderer {
    /// Returns true when a native image was rendered for `item_id`.
    fn render_icon(&self, item_id: &str, frame: &mut Frame<'_>, area: Rect) -> bool;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LauncherHitTarget {
    Toolbar(LauncherToolbarAction),
    Item(usize),
    Scrollbar,
    Confirm,
    Cancel,
    OverlaySurface,
    EmptyContent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LauncherToolbarButtonLayout {
    pub action: LauncherToolbarAction,
    pub area: Rect,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LauncherItemLayout {
    pub index: usize,
    pub area: Rect,
    pub icon_area: Rect,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LauncherConfirmationLayout {
    pub area: Rect,
    pub message: Rect,
    pub confirm: Rect,
    pub cancel: Rect,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LauncherLayout {
    pub panel: Rect,
    pub toolbar: Rect,
    pub content: Rect,
    pub footer: Rect,
    pub toolbar_buttons: Vec<LauncherToolbarButtonLayout>,
    pub items: Vec<LauncherItemLayout>,
    pub visible_start: usize,
    pub visible_capacity: usize,
    pub columns: usize,
    pub scroll_content_len: usize,
    pub scrollbar: Option<Rect>,
    pub drop_indicator: Option<Rect>,
    pub confirmation: Option<LauncherConfirmationLayout>,
}

impl LauncherLayout {
    pub fn hit_test(&self, x: u16, y: u16) -> Option<LauncherHitTarget> {
        if let Some(confirmation) = self.confirmation {
            if contains(confirmation.confirm, x, y) {
                return Some(LauncherHitTarget::Confirm);
            }
            if contains(confirmation.cancel, x, y) {
                return Some(LauncherHitTarget::Cancel);
            }
            return contains(confirmation.area, x, y).then_some(LauncherHitTarget::OverlaySurface);
        }
        if let Some(button) = self
            .toolbar_buttons
            .iter()
            .find(|button| contains(button.area, x, y))
        {
            return Some(LauncherHitTarget::Toolbar(button.action));
        }
        if let Some(item) = self.items.iter().find(|item| contains(item.area, x, y)) {
            return Some(LauncherHitTarget::Item(item.index));
        }
        if self.scrollbar.is_some_and(|area| contains(area, x, y)) {
            return Some(LauncherHitTarget::Scrollbar);
        }
        contains(self.content, x, y).then_some(LauncherHitTarget::EmptyContent)
    }

    pub fn large_icon_drop_target(&self, x: u16, y: u16) -> Option<LauncherDropTarget> {
        if !contains(self.content, x, y)
            || self.items.is_empty()
            || self.scrollbar.is_some_and(|area| contains(area, x, y))
        {
            return None;
        }

        let items_on_row = self
            .items
            .iter()
            .filter(|item| y >= item.area.y && y < item.area.bottom())
            .collect::<Vec<_>>();
        if let Some(last) = items_on_row.last() {
            for item in &items_on_row {
                let midpoint = item.area.x.saturating_add(item.area.width / 2);
                if x < midpoint {
                    return Some(LauncherDropTarget {
                        item_index: item.index,
                        side: LauncherDropSide::Before,
                    });
                }
            }
            return Some(LauncherDropTarget {
                item_index: last.index,
                side: LauncherDropSide::After,
            });
        }

        let first = self.items.first()?;
        let last = self.items.last()?;
        Some(if y < first.area.y {
            LauncherDropTarget {
                item_index: first.index,
                side: LauncherDropSide::Before,
            }
        } else {
            LauncherDropTarget {
                item_index: last.index,
                side: LauncherDropSide::After,
            }
        })
    }
}

pub fn launcher_layout(main: Rect, model: &LauncherViewModel) -> LauncherLayout {
    let panel = main;
    let inner = inset(panel, 1);
    let desired_footer_height = if inner.height >= 7 { 2 } else { 1 };
    let toolbar_height = launcher_toolbar_layout(Rect::new(0, 0, inner.width, u16::MAX), model)
        .iter()
        .map(|b| b.area.bottom())
        .max()
        .unwrap_or(0)
        .min(inner.height.saturating_sub(desired_footer_height + 1));
    let mut toolbar = Rect::new(inner.x, inner.y, inner.width, toolbar_height);
    let footer_height = desired_footer_height.min(inner.height.saturating_sub(toolbar.height));
    let footer = Rect::new(
        inner.x,
        inner
            .y
            .saturating_add(inner.height.saturating_sub(footer_height)),
        inner.width,
        footer_height,
    );
    toolbar.y = footer.y.saturating_sub(toolbar.height).max(inner.y);
    let content_y = inner.y.saturating_add(u16::from(inner.height > 0));
    let content = Rect::new(
        inner.x,
        content_y,
        inner.width,
        toolbar.y.saturating_sub(content_y),
    );
    let mut toolbar_buttons = launcher_toolbar_layout(toolbar, model);
    if let Some(refresh) = model
        .toolbar
        .iter()
        .find(|b| b.action == LauncherToolbarAction::Refresh)
    {
        toolbar_buttons.push(LauncherToolbarButtonLayout {
            action: refresh.action,
            area: Rect::new(
                inner.x,
                inner.y,
                (terminal_width(&format!(
                    "[{} {}]",
                    refresh.action.shortcut(),
                    refresh.label
                )) as u16)
                    .min(inner.width),
                u16::from(inner.height > 0),
            ),
        });
    }
    let (items, visible_start, visible_capacity, scrollbar) = match model.view_mode {
        LauncherViewMode::LargeIcons => launcher_grid_layout(content, model),
        LauncherViewMode::Details => launcher_details_layout(content, model),
    };
    let columns = match model.view_mode {
        LauncherViewMode::LargeIcons => usize::from(
            (content.width.saturating_sub(u16::from(scrollbar.is_some())) / GRID_TILE_MIN_WIDTH)
                .max(1),
        ),
        LauncherViewMode::Details => 1,
    };
    let scroll_content_len = model.items.len().div_ceil(columns).saturating_mul(columns);
    let confirmation = model
        .confirmation
        .as_ref()
        .map(|_| launcher_confirmation_layout(main));
    let drop_indicator = if model.view_mode == LauncherViewMode::LargeIcons {
        model.drop_target.and_then(|target| {
            items
                .iter()
                .find(|item| item.index == target.item_index)
                .map(|item| {
                    let x = match target.side {
                        LauncherDropSide::Before => item.area.x,
                        LauncherDropSide::After => item.area.right().saturating_sub(1),
                    };
                    Rect::new(
                        x,
                        item.area.y,
                        u16::from(item.area.width > 0),
                        item.area.height,
                    )
                })
        })
    } else {
        None
    };
    LauncherLayout {
        panel,
        toolbar,
        content,
        footer,
        toolbar_buttons,
        items,
        visible_start,
        visible_capacity,
        columns,
        scroll_content_len,
        scrollbar,
        drop_indicator,
        confirmation,
    }
}

fn launcher_toolbar_layout(
    area: Rect,
    model: &LauncherViewModel,
) -> Vec<LauncherToolbarButtonLayout> {
    let buttons = model
        .toolbar
        .iter()
        .filter(|b| b.action != LauncherToolbarAction::Refresh)
        .collect::<Vec<_>>();
    let widths = buttons
        .iter()
        .map(|b| (terminal_width(&format!("[{} {}]", b.action.shortcut(), b.label)) + 2) as u16)
        .collect::<Vec<_>>();
    buttons
        .into_iter()
        .zip(crate::right_aligned_actions(area, &widths))
        .map(|(button, area)| LauncherToolbarButtonLayout {
            action: button.action,
            area,
        })
        .collect()
}

fn launcher_grid_layout(
    content: Rect,
    model: &LauncherViewModel,
) -> (Vec<LauncherItemLayout>, usize, usize, Option<Rect>) {
    let mut columns = usize::from((content.width / GRID_TILE_MIN_WIDTH).max(1));
    let rows = usize::from((content.height / GRID_TILE_HEIGHT).max(1));
    let needs_scrollbar =
        model.items.len() > columns.saturating_mul(rows) && content.width > 0 && content.height > 0;
    let grid_width = content.width.saturating_sub(u16::from(needs_scrollbar));
    if needs_scrollbar {
        columns = usize::from((grid_width / GRID_TILE_MIN_WIDTH).max(1));
    }
    let capacity = columns.saturating_mul(rows).max(1);
    let start = visible_start(
        model.items.len(),
        model.selected_index,
        model.viewport_offset,
        capacity,
        columns,
    );
    let column_width = if columns == 0 {
        grid_width
    } else {
        grid_width / u16::try_from(columns).unwrap_or(u16::MAX).max(1)
    };
    let mut items = Vec::new();
    for (slot, index) in (start..model.items.len()).take(capacity).enumerate() {
        if grid_width == 0 || content.height == 0 {
            break;
        }
        let column = slot % columns;
        let row = slot / columns;
        let x = content.x.saturating_add(
            u16::try_from(column)
                .unwrap_or(u16::MAX)
                .saturating_mul(column_width),
        );
        let y = content.y.saturating_add(
            u16::try_from(row)
                .unwrap_or(u16::MAX)
                .saturating_mul(GRID_TILE_HEIGHT),
        );
        let width = if column + 1 == columns {
            grid_width.saturating_sub(x.saturating_sub(content.x))
        } else {
            column_width
        };
        let area = Rect::new(
            x,
            y,
            width.saturating_sub(if columns > 1 {
                crate::SpringStyle::CARD_GAP
            } else {
                0
            }),
            GRID_TILE_HEIGHT
                .saturating_sub(1)
                .min(content.bottom().saturating_sub(y)),
        );
        let inner = inset(area, 1);
        let icon_area = Rect::new(
            inner.x,
            inner.y,
            inner.width,
            inner.height.saturating_sub(2).min(4),
        );
        items.push(LauncherItemLayout {
            index,
            area,
            icon_area,
        });
    }
    let scrollbar = needs_scrollbar.then(|| {
        Rect::new(
            content.right().saturating_sub(1),
            content.y,
            1,
            content.height,
        )
    });
    (items, start, capacity, scrollbar)
}

fn launcher_details_layout(
    content: Rect,
    model: &LauncherViewModel,
) -> (Vec<LauncherItemLayout>, usize, usize, Option<Rect>) {
    let rows_area = Rect::new(
        content.x,
        content.y.saturating_add(u16::from(content.height > 0)),
        content.width,
        content.height.saturating_sub(1),
    );
    let capacity = usize::from(rows_area.height).max(1);
    let start = visible_start(
        model.items.len(),
        model.selected_index,
        model.viewport_offset,
        capacity,
        1,
    );
    let needs_scrollbar = model.items.len() > capacity;
    let row_width = rows_area.width.saturating_sub(u16::from(needs_scrollbar));
    let items = (start..model.items.len())
        .take(capacity)
        .take(usize::from(rows_area.height))
        .enumerate()
        .map(|(slot, index)| {
            let area = Rect::new(
                rows_area.x,
                rows_area
                    .y
                    .saturating_add(u16::try_from(slot).unwrap_or(u16::MAX)),
                row_width,
                1,
            );
            LauncherItemLayout {
                index,
                area,
                icon_area: Rect::new(area.x, area.y, 3.min(area.width), area.height),
            }
        })
        .collect();
    let scrollbar = needs_scrollbar.then(|| {
        Rect::new(
            rows_area.right().saturating_sub(1),
            rows_area.y,
            1,
            rows_area.height,
        )
    });
    (items, start, capacity, scrollbar)
}

fn visible_start(
    item_count: usize,
    selected_index: Option<usize>,
    requested: usize,
    capacity: usize,
    columns: usize,
) -> usize {
    if item_count == 0 || capacity == 0 {
        return 0;
    }
    let columns = columns.max(1);
    // Count the final partial row so the bottom scrollbar position can reveal
    // the last entry without placing it outside the grid.
    let max_start = item_count
        .div_ceil(columns)
        .saturating_mul(columns)
        .saturating_sub(capacity);
    let mut start = requested.min(max_start);
    start -= start % columns;
    if let Some(selected) = selected_index.filter(|selected| *selected < item_count) {
        if selected < start {
            start = selected - selected % columns;
        } else if selected >= start.saturating_add(capacity) {
            let selected_row = selected / columns;
            let visible_rows = (capacity / columns).max(1);
            start = selected_row
                .saturating_sub(visible_rows.saturating_sub(1))
                .saturating_mul(columns)
                .min(max_start);
            start -= start % columns;
        }
    }
    start
}

fn launcher_confirmation_layout(area: Rect) -> LauncherConfirmationLayout {
    let width = area.width.saturating_sub(4).clamp(1, 72);
    let height = area.height.saturating_sub(2).clamp(1, 9);
    let dialog = Rect::new(
        area.x.saturating_add(area.width.saturating_sub(width) / 2),
        area.y
            .saturating_add(area.height.saturating_sub(height) / 2),
        width,
        height,
    );
    let inner = inset(dialog, 1);
    let buttons_y = inner.bottom().saturating_sub(1);
    let button_width = inner.width.saturating_sub(1) / 2;
    LauncherConfirmationLayout {
        area: dialog,
        message: Rect::new(
            inner.x,
            inner.y,
            inner.width,
            inner.height.saturating_sub(2),
        ),
        confirm: Rect::new(inner.x, buttons_y, button_width, 1),
        cancel: Rect::new(
            inner.x.saturating_add(button_width).saturating_add(1),
            buttons_y,
            inner.width.saturating_sub(button_width).saturating_sub(1),
            1,
        ),
    }
}

pub fn render_launcher_content(
    frame: &mut Frame<'_>,
    main: Rect,
    model: &LauncherViewModel,
    context: &RenderContext,
    icons: Option<&dyn LauncherIconRenderer>,
) {
    let layout = launcher_layout(main, model);
    let theme = &context.compatibility_theme();
    Surface::new()
        .titled(i18n::tr!(
            "ui-launcher-title",
            mode = launcher_view_mode_label(model.view_mode)
        ))
        .bordered(false)
        .render_frame(frame, layout.panel, context);
    render_launcher_toolbar(frame, &layout, model, context);
    match model.view_mode {
        LauncherViewMode::LargeIcons => render_launcher_grid(frame, &layout, model, theme, icons),
        LauncherViewMode::Details => {
            render_launcher_details(frame, &layout, model, context);
            if let Some(icons) = icons {
                for item_layout in &layout.items {
                    if let Some(item) = model.items.get(item_layout.index) {
                        icons.render_icon(&item.id, frame, item_layout.icon_area);
                    }
                }
            }
        }
    }
    if let Some(indicator) = layout.drop_indicator {
        render_launcher_drop_indicator(frame, indicator, theme);
    }
    render_launcher_footer(frame, layout.footer, model, context);
    if let Some(scrollbar) = layout.scrollbar {
        render_launcher_scrollbar(frame, scrollbar, &layout, model, context);
    }
}

fn render_launcher_drop_indicator(frame: &mut Frame<'_>, area: Rect, theme: &TundraTheme) {
    for row in 0..area.height {
        frame.render_widget(
            Paragraph::new("┃")
                .alignment(HorizontalAlignment::Left)
                .style(theme.title_style()),
            Rect::new(area.x, area.y.saturating_add(row), area.width, 1),
        );
    }
}

fn render_launcher_toolbar(
    frame: &mut Frame<'_>,
    layout: &LauncherLayout,
    model: &LauncherViewModel,
    context: &RenderContext,
) {
    let theme = &context.compatibility_theme();
    for button_layout in &layout.toolbar_buttons {
        let Some(button) = model
            .toolbar
            .iter()
            .find(|button| button.action == button_layout.action)
        else {
            continue;
        };
        let mut component = Button::new(
            format!("launcher.toolbar.{:?}", button.action),
            format!("[{} {}]", button.action.shortcut(), button.label),
        );
        component.set_disabled(!button.enabled);
        component.render_borderless_frame(frame, button_layout.area, theme);
        if button_layout.area.height > 1 {
            frame.render_widget(
                Paragraph::new(fit_text(&button.description, button_layout.area.width))
                    .style(theme.muted_style()),
                Rect::new(
                    button_layout.area.x,
                    button_layout.area.y.saturating_add(1),
                    button_layout.area.width,
                    1,
                ),
            );
        }
    }
}

fn render_launcher_grid(
    frame: &mut Frame<'_>,
    layout: &LauncherLayout,
    model: &LauncherViewModel,
    theme: &TundraTheme,
    icons: Option<&dyn LauncherIconRenderer>,
) {
    if model.items.is_empty() {
        frame.render_widget(
            Paragraph::new(empty_message())
                .style(theme.muted_style())
                .alignment(HorizontalAlignment::Center)
                .wrap(Wrap { trim: true }),
            layout.content,
        );
        return;
    }
    for item_layout in &layout.items {
        let Some(item) = model.items.get(item_layout.index) else {
            continue;
        };
        let focused =
            theme.keyboard_focus_visible() && model.selected_index == Some(item_layout.index);
        let selected = focused || (theme.keyboard_focus_visible() && item.selected);
        let style = item_style(item.status, selected, theme).bg(theme.tokens().raised);
        let mut surface = Button::new(format!("launcher.item.{}", item.id), "");
        surface.set_focused(focused);
        surface.state.selected = selected;
        surface.set_disabled(item.status != LauncherItemStatus::Ready);
        if item_layout.area.height < 3 {
            let mut compact = Button::new(format!("launcher.item.{}", item.id), item.name.clone());
            compact.set_focused(focused);
            compact.state.selected = selected;
            compact.set_disabled(item.status != LauncherItemStatus::Ready);
            compact.render_borderless_frame(frame, item_layout.area, theme);
            continue;
        }
        surface.render_surface_frame(frame, item_layout.area, theme);
        let rendered_native = !surface.state.disabled
            && item_layout.icon_area.width > 0
            && item_layout.icon_area.height > 0
            && icons.is_some_and(|icons| icons.render_icon(&item.id, frame, item_layout.icon_area));
        if !rendered_native {
            render_default_ascii_icon(frame, item_layout.icon_area, model, item, style);
        }
        let inner = inset(item_layout.area, 1);
        let name_y = item_layout
            .icon_area
            .bottom()
            .min(inner.bottom().saturating_sub(2.min(inner.height)))
            .max(inner.y);
        frame.render_widget(
            Paragraph::new(fit_text(&item.name, inner.width))
                .style(if focused && !surface.state.disabled {
                    theme.title_style().bg(theme.tokens().raised)
                } else {
                    style
                })
                .alignment(HorizontalAlignment::Center),
            Rect::new(inner.x, name_y, inner.width, u16::from(inner.height > 0)),
        );
        frame.render_widget(
            Paragraph::new(if item.is_builtin() {
                fit_text(&item.path, inner.width)
            } else {
                launcher_status_label(item.status)
            })
            .style(style)
            .alignment(HorizontalAlignment::Center),
            Rect::new(
                inner.x,
                name_y.saturating_add(1).min(inner.bottom()),
                inner.width,
                u16::from(inner.height > 1 && name_y.saturating_add(1) < inner.bottom()),
            ),
        );
    }
}

fn render_default_ascii_icon(
    frame: &mut Frame<'_>,
    area: Rect,
    model: &LauncherViewModel,
    item: &LauncherItemViewModel,
    style: Style,
) {
    let Some(icon) = model.item_icon(item) else {
        return;
    };
    for (row, line) in icon
        .lines()
        .iter()
        .take(usize::from(area.height))
        .enumerate()
    {
        frame.render_widget(
            Paragraph::new(fit_text(line, area.width))
                .style(style)
                .alignment(HorizontalAlignment::Center),
            Rect::new(
                area.x,
                area.y
                    .saturating_add(u16::try_from(row).unwrap_or(u16::MAX)),
                area.width,
                1,
            ),
        );
    }
}

fn render_launcher_details(
    frame: &mut Frame<'_>,
    layout: &LauncherLayout,
    model: &LauncherViewModel,
    context: &RenderContext,
) {
    let theme = &context.compatibility_theme();
    let scrollbar_width = u16::from(layout.scrollbar.is_some());
    let width = layout.content.width.saturating_sub(scrollbar_width);
    if model.items.is_empty() {
        frame.render_widget(
            Paragraph::new(empty_message())
                .style(theme.muted_style())
                .alignment(HorizontalAlignment::Center)
                .wrap(Wrap { trim: true }),
            Rect::new(
                layout.content.x,
                layout.content.y,
                width,
                layout.content.height,
            ),
        );
        return;
    }
    let widths = detail_widths(width);
    let header = [
        fit_text(&i18n::tr!("ui-launcher-name"), widths[0]),
        fit_text(&i18n::tr!("ui-launcher-type"), widths[1]),
        fit_text(&i18n::tr!("ui-launcher-integrity"), widths[2]),
        fit_text(&i18n::tr!("ui-launcher-path"), widths[3]),
    ];
    let rows = model.items.iter().map(|item| {
        vec![
            fit_text(&format!("[A] {}", item.name), widths[0]),
            fit_text(&item.type_label, widths[1]),
            fit_text(&launcher_status_label(item.status), widths[2]),
            fit_text(&item.path, widths[3]),
        ]
    });
    let rows = rows.collect::<Vec<_>>();
    let tones = model
        .items
        .iter()
        .enumerate()
        .map(|(index, item)| match item.status {
            _ if theme.keyboard_focus_visible()
                && (model.selected_index == Some(index) || item.selected) =>
            {
                ComponentTone::Accent
            }
            LauncherItemStatus::Ready => ComponentTone::Default,
            _ => ComponentTone::Warning,
        })
        .collect();
    let mut table = DataTable::new("launcher.details", header.clone(), rows)
        .with_column_widths(widths.to_vec())
        .with_viewport_start(layout.visible_start)
        .with_row_tones(tones)
        .bordered(false);
    table.selected = model
        .selected_index
        .filter(|_| theme.keyboard_focus_visible());
    table.state.focused = false;
    let table_context = RenderContext {
        theme: crate::ThemeTokens {
            accent_strong: context.theme.accent,
            ..context.theme
        },
        ..context.clone()
    };
    table.render_frame(
        frame,
        Rect::new(
            layout.content.x,
            layout.content.y,
            width,
            layout.content.height,
        ),
        &table_context,
    );
    crate::render_table_headers(
        frame,
        &launcher_sort_headers(layout, model),
        &vec![
            i18n::tr!("ui-launcher-name"),
            i18n::tr!("ui-launcher-type"),
            i18n::tr!("ui-launcher-integrity"),
            i18n::tr!("ui-launcher-path"),
        ],
        model.sort,
        "launcher",
        context,
    );
}

fn detail_widths(width: u16) -> [u16; 4] {
    let name = (width.saturating_mul(28) / 100).max(8);
    let kind = (width.saturating_mul(16) / 100).max(6);
    let integrity = (width.saturating_mul(18) / 100).max(8);
    let used = name.saturating_add(kind).saturating_add(integrity);
    [name, kind, integrity, width.saturating_sub(used)]
}

fn render_launcher_footer(
    frame: &mut Frame<'_>,
    area: Rect,
    model: &LauncherViewModel,
    context: &RenderContext,
) {
    let theme = &context.compatibility_theme();
    if area.width == 0 || area.height == 0 {
        return;
    }
    let (text, style) = if let Some(error) = &model.error {
        (error.clone(), theme.error_style())
    } else if let Some(message) = &model.message {
        (message.clone(), theme.body_style())
    } else {
        (
            i18n::tr!("ui-launcher-item-count", count = model.items.len()),
            theme.muted_style(),
        )
    };
    frame.render_widget(
        Paragraph::new(fit_text(&text, area.width))
            .alignment(HorizontalAlignment::Left)
            .style(style),
        Rect::new(area.x, area.y, area.width, 1),
    );
    if area.height > 1
        && let Some(item) = model.selected_item()
    {
        frame.render_widget(
            Paragraph::new(fit_text(&item.path, area.width)).style(theme.muted_style()),
            Rect::new(area.x, area.y.saturating_add(1), area.width, 1),
        );
    }
}

fn render_launcher_scrollbar(
    frame: &mut Frame<'_>,
    area: Rect,
    layout: &LauncherLayout,
    model: &LauncherViewModel,
    context: &RenderContext,
) {
    if area.height == 0 || model.items.is_empty() {
        return;
    }
    Scrollbar::new(
        layout.scroll_content_len,
        layout.visible_capacity,
        layout.visible_start,
    )
    .render_frame(frame, area, context);
}

fn render_launcher_confirmation(
    frame: &mut Frame<'_>,
    layout: LauncherConfirmationLayout,
    dialog: &LauncherConfirmationViewModel,
    context: &RenderContext,
) {
    let theme = &context.compatibility_theme();
    frame.render_widget(Clear, layout.area);
    Surface::new()
        .titled(dialog.title.clone())
        .bordered(true)
        .raised(true)
        .render_frame(frame, layout.area, context);
    frame.render_widget(
        Paragraph::new(dialog.message.clone())
            .style(theme.body_style())
            .alignment(HorizontalAlignment::Center)
            .wrap(Wrap { trim: true }),
        layout.message,
    );
    let mut confirm = Button::new(
        "launcher.confirmation.confirm",
        format!("[Y {}]", dialog.confirm_label),
    );
    confirm.state.selected = dialog.confirm_selected;
    confirm.render_borderless_frame(frame, layout.confirm, theme);

    let mut cancel = Button::new(
        "launcher.confirmation.cancel",
        format!("[Esc {}]", dialog.cancel_label),
    );
    cancel.state.selected = !dialog.confirm_selected;
    cancel.render_borderless_frame(frame, layout.cancel, theme);
}

fn item_style(status: LauncherItemStatus, selected: bool, theme: &TundraTheme) -> Style {
    if status != LauncherItemStatus::Ready {
        theme.disabled_style()
    } else if selected {
        theme.title_style()
    } else {
        theme.body_style()
    }
}

fn launcher_view_mode_label(mode: LauncherViewMode) -> String {
    match mode {
        LauncherViewMode::LargeIcons => i18n::tr!("ui-launcher-large-icons"),
        LauncherViewMode::Details => i18n::tr!("ui-launcher-details"),
    }
}

fn launcher_status_label(status: LauncherItemStatus) -> String {
    match status {
        LauncherItemStatus::Ready => i18n::tr!("ui-launcher-ready"),
        LauncherItemStatus::Checking => i18n::tr!("ui-launcher-checking"),
        LauncherItemStatus::Changed => i18n::tr!("ui-launcher-changed"),
        LauncherItemStatus::Missing => i18n::tr!("ui-launcher-missing"),
        LauncherItemStatus::NeedsApproval => i18n::tr!("ui-launcher-needs-approval"),
        LauncherItemStatus::Unsupported => i18n::tr!("ui-launcher-unsupported"),
    }
}

fn fit_text(value: &str, width: u16) -> String {
    let width = usize::from(width);
    if width == 0 {
        return String::new();
    }
    if terminal_width(value) <= width {
        return value.to_string();
    }

    let content_width = width.saturating_sub(1);
    let mut fitted = truncate_to_terminal_width(value, content_width);
    fitted.push('…');
    fitted
}

fn inset(area: Rect, amount: u16) -> Rect {
    Rect::new(
        area.x.saturating_add(amount),
        area.y.saturating_add(amount),
        area.width.saturating_sub(amount.saturating_mul(2)),
        area.height.saturating_sub(amount.saturating_mul(2)),
    )
}

fn contains(area: Rect, x: u16, y: u16) -> bool {
    area.width > 0
        && area.height > 0
        && x >= area.x
        && x < area.right()
        && y >= area.y
        && y < area.bottom()
}

pub fn render_launcher_overlay(
    frame: &mut Frame<'_>,
    main: Rect,
    model: &LauncherViewModel,
    context: &RenderContext,
) {
    let layout = launcher_layout(main, model);
    if let (Some(dialog), Some(dialog_layout)) = (&model.confirmation, layout.confirmation) {
        render_launcher_confirmation(frame, dialog_layout, dialog, context);
    }
}

pub fn launcher_sort_headers(
    layout: &LauncherLayout,
    model: &LauncherViewModel,
) -> Vec<(usize, Rect)> {
    if model.view_mode != LauncherViewMode::Details || model.confirmation.is_some() {
        return vec![];
    }
    let width = layout
        .content
        .width
        .saturating_sub(u16::from(layout.scrollbar.is_some()));
    crate::table_header_areas(
        Rect::new(
            layout.content.x,
            layout.content.y,
            width,
            layout.content.height,
        ),
        &detail_widths(width),
    )
}
