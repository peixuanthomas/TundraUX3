use ratatui::Frame;
use ratatui::layout::{Alignment, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Borders, Clear, Paragraph, Wrap};

use crate::render::{render_compact_home, render_status, render_top};
use crate::{
    AssetError, RuntimeAsciiAssets, ShellChromeViewModel, ShellLayout, TundraTheme,
    compute_shell_layout,
};

const GRID_TILE_MIN_WIDTH: u16 = 20;
const GRID_TILE_HEIGHT: u16 = 9;
const EMPTY_MESSAGE: &str = "No Launcher items. Go to Explorer, select a file, then right-click and choose Add to Launcher.";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum LauncherViewMode {
    #[default]
    LargeIcons,
    Details,
}

impl LauncherViewMode {
    pub const fn label(self) -> &'static str {
        match self {
            Self::LargeIcons => "Large icons",
            Self::Details => "Details",
        }
    }
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
    pub const fn label(self) -> &'static str {
        match self {
            Self::Ready => "Ready",
            Self::Checking => "Checking",
            Self::Changed => "Changed",
            Self::Missing => "Missing",
            Self::NeedsApproval => "Needs approval",
            Self::Unsupported => "Unsupported",
        }
    }

    pub const fn can_launch(self) -> bool {
        matches!(self, Self::Ready)
    }

    pub const fn requires_approval(self) -> bool {
        matches!(self, Self::Changed | Self::NeedsApproval)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LauncherItemViewModel {
    pub id: String,
    pub name: String,
    pub path: String,
    pub type_label: String,
    pub status: LauncherItemStatus,
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
            selected: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LauncherToolbarAction {
    Remove,
    Reapprove,
    Refresh,
    ToggleView,
}

impl LauncherToolbarAction {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Remove => "Remove",
            Self::Reapprove => "Reapprove",
            Self::Refresh => "Refresh",
            Self::ToggleView => "View",
        }
    }

    pub const fn shortcut(self) -> &'static str {
        match self {
            Self::Remove => "Del",
            Self::Reapprove => "Ctrl+R",
            Self::Refresh => "R",
            Self::ToggleView => "V",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LauncherToolbarButtonViewModel {
    pub action: LauncherToolbarAction,
    pub label: String,
    pub enabled: bool,
}

impl LauncherToolbarButtonViewModel {
    pub fn new(action: LauncherToolbarAction, enabled: bool) -> Self {
        Self {
            action,
            label: action.label().to_string(),
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
        let selected_index = selected_index
            .filter(|index| *index < items.len())
            .or_else(|| (!items.is_empty()).then_some(0));
        let has_selection = selected_index.is_some();
        let can_reapprove = selected_index
            .and_then(|index| items.get(index))
            .is_some_and(|item| item.status.requires_approval());
        let mut toolbar = Vec::new();
        if can_manage && has_selection {
            toolbar.push(LauncherToolbarButtonViewModel::new(
                LauncherToolbarAction::Remove,
                has_selection,
            ));
            toolbar.push(LauncherToolbarButtonViewModel::new(
                LauncherToolbarAction::Reapprove,
                can_reapprove,
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

        Ok(Self {
            items,
            selected_index,
            view_mode,
            viewport_offset: 0,
            toolbar,
            message: None,
            error: None,
            confirmation: None,
            drop_target: None,
            ascii_assets: RuntimeAsciiAssets::load_default()?,
        })
    }

    pub fn selected_item(&self) -> Option<&LauncherItemViewModel> {
        self.selected_index.and_then(|index| self.items.get(index))
    }

    pub fn default_app_icon(&self) -> Option<&crate::HomeIcon> {
        self.ascii_assets
            .home_icon_catalog()
            .icon_for_key("default")
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
        if !contains(self.content, x, y) || self.items.is_empty() {
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
    let toolbar = Rect::new(inner.x, inner.y, inner.width, u16::from(inner.height > 0));
    let footer = Rect::new(
        inner.x,
        inner.y.saturating_add(inner.height.saturating_sub(1)),
        inner.width,
        u16::from(inner.height > 1),
    );
    let content_y = toolbar.y.saturating_add(toolbar.height);
    let content = Rect::new(
        inner.x,
        content_y,
        inner.width,
        footer.y.saturating_sub(content_y),
    );
    let toolbar_buttons = launcher_toolbar_layout(toolbar, model);
    let (items, visible_start, visible_capacity, scrollbar) = match model.view_mode {
        LauncherViewMode::LargeIcons => launcher_grid_layout(content, model),
        LauncherViewMode::Details => launcher_details_layout(content, model),
    };
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
        scrollbar,
        drop_indicator,
        confirmation,
    }
}

fn launcher_toolbar_layout(
    area: Rect,
    model: &LauncherViewModel,
) -> Vec<LauncherToolbarButtonLayout> {
    let mut x = area.x;
    let end = area.x.saturating_add(area.width);
    let mut result = Vec::new();
    for button in &model.toolbar {
        let text = format!("[{} {}]", button.action.shortcut(), button.label);
        let width = u16::try_from(text.chars().count()).unwrap_or(u16::MAX);
        if x.saturating_add(width) > end {
            break;
        }
        result.push(LauncherToolbarButtonLayout {
            action: button.action,
            area: Rect::new(x, area.y, width, area.height),
        });
        x = x.saturating_add(width).saturating_add(1);
    }
    result
}

fn launcher_grid_layout(
    content: Rect,
    model: &LauncherViewModel,
) -> (Vec<LauncherItemLayout>, usize, usize, Option<Rect>) {
    let columns = usize::from((content.width / GRID_TILE_MIN_WIDTH).max(1));
    let rows = usize::from((content.height / GRID_TILE_HEIGHT).max(1));
    let capacity = columns.saturating_mul(rows).max(1);
    let start = visible_start(
        model.items.len(),
        model.selected_index,
        model.viewport_offset,
        capacity,
        columns,
    );
    let needs_scrollbar = model.items.len() > capacity;
    let grid_width = content.width.saturating_sub(u16::from(needs_scrollbar));
    let column_width = if columns == 0 {
        grid_width
    } else {
        grid_width / u16::try_from(columns).unwrap_or(u16::MAX).max(1)
    };
    let mut items = Vec::new();
    for (slot, index) in (start..model.items.len()).take(capacity).enumerate() {
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
            width,
            GRID_TILE_HEIGHT.min(content.bottom().saturating_sub(y)),
        );
        let inner = inset(area, 1);
        let icon_area = Rect::new(inner.x, inner.y, inner.width, inner.height.min(4));
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
    let max_start = item_count.saturating_sub(capacity);
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
    let width = area.width.saturating_sub(4).min(72).max(1);
    let height = area.height.saturating_sub(2).min(9).max(1);
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

pub fn render_launcher(
    frame: &mut Frame<'_>,
    area: Rect,
    chrome: &ShellChromeViewModel,
    model: &LauncherViewModel,
    theme: &TundraTheme,
) {
    render_launcher_with_icons(frame, area, chrome, model, theme, None);
}

pub fn render_launcher_with_icons(
    frame: &mut Frame<'_>,
    area: Rect,
    chrome: &ShellChromeViewModel,
    model: &LauncherViewModel,
    theme: &TundraTheme,
    icons: Option<&dyn LauncherIconRenderer>,
) {
    match compute_shell_layout(area) {
        ShellLayout::Compact(compact) => render_compact_home(frame, compact, chrome, theme),
        ShellLayout::Full { top, main, status } => {
            render_top(frame, top, chrome, theme);
            render_launcher_main(frame, main, model, theme, icons);
            render_status(frame, status, chrome, theme);
        }
    }
}

fn render_launcher_main(
    frame: &mut Frame<'_>,
    main: Rect,
    model: &LauncherViewModel,
    theme: &TundraTheme,
    icons: Option<&dyn LauncherIconRenderer>,
) {
    let layout = launcher_layout(main, model);
    frame.render_widget(
        theme
            .block()
            .title(format!("Launcher · {}", model.view_mode.label()))
            .borders(Borders::ALL)
            .style(theme.body_style()),
        layout.panel,
    );
    render_launcher_toolbar(frame, &layout, model, theme);
    match model.view_mode {
        LauncherViewMode::LargeIcons => render_launcher_grid(frame, &layout, model, theme, icons),
        LauncherViewMode::Details => render_launcher_details(frame, &layout, model, theme),
    }
    if let Some(indicator) = layout.drop_indicator {
        render_launcher_drop_indicator(frame, indicator, theme);
    }
    render_launcher_footer(frame, layout.footer, model, theme);
    if let Some(scrollbar) = layout.scrollbar {
        render_launcher_scrollbar(frame, scrollbar, &layout, model, theme);
    }
    if let (Some(dialog), Some(dialog_layout)) = (&model.confirmation, layout.confirmation) {
        render_launcher_confirmation(frame, dialog_layout, dialog, theme);
    }
}

fn render_launcher_drop_indicator(frame: &mut Frame<'_>, area: Rect, theme: &TundraTheme) {
    for row in 0..area.height {
        frame.render_widget(
            Paragraph::new("┃").style(theme.title_style()),
            Rect::new(area.x, area.y.saturating_add(row), area.width, 1),
        );
    }
}

fn render_launcher_toolbar(
    frame: &mut Frame<'_>,
    layout: &LauncherLayout,
    model: &LauncherViewModel,
    theme: &TundraTheme,
) {
    for button_layout in &layout.toolbar_buttons {
        let Some(button) = model
            .toolbar
            .iter()
            .find(|button| button.action == button_layout.action)
        else {
            continue;
        };
        let style = if button.enabled {
            theme.title_style()
        } else {
            theme.muted_style()
        };
        frame.render_widget(
            Paragraph::new(format!("[{} {}]", button.action.shortcut(), button.label)).style(style),
            button_layout.area,
        );
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
            Paragraph::new(EMPTY_MESSAGE)
                .style(theme.muted_style())
                .alignment(Alignment::Center)
                .wrap(Wrap { trim: true }),
            layout.content,
        );
        return;
    }
    for item_layout in &layout.items {
        let Some(item) = model.items.get(item_layout.index) else {
            continue;
        };
        let focused = model.selected_index == Some(item_layout.index);
        let selected = focused || item.selected;
        let style = item_style(item.status, selected, theme);
        frame.render_widget(
            theme
                .block()
                .borders(Borders::ALL)
                .style(style)
                .border_style(theme.selectable_border_style(selected)),
            item_layout.area,
        );
        let rendered_native =
            icons.is_some_and(|icons| icons.render_icon(&item.id, frame, item_layout.icon_area));
        if !rendered_native {
            render_default_ascii_icon(frame, item_layout.icon_area, model, style);
        }
        let inner = inset(item_layout.area, 1);
        let name_y = item_layout
            .icon_area
            .bottom()
            .min(inner.bottom().saturating_sub(2));
        frame.render_widget(
            Paragraph::new(fit_text(&item.name, inner.width))
                .style(if focused { theme.title_style() } else { style })
                .alignment(Alignment::Center),
            Rect::new(inner.x, name_y, inner.width, u16::from(inner.height > 0)),
        );
        frame.render_widget(
            Paragraph::new(item.status.label())
                .style(status_style(item.status, theme))
                .alignment(Alignment::Center),
            Rect::new(
                inner.x,
                name_y.saturating_add(1),
                inner.width,
                u16::from(inner.height > 1),
            ),
        );
    }
}

fn render_default_ascii_icon(
    frame: &mut Frame<'_>,
    area: Rect,
    model: &LauncherViewModel,
    style: Style,
) {
    let Some(icon) = model.default_app_icon() else {
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
                .alignment(Alignment::Center),
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
    theme: &TundraTheme,
) {
    let scrollbar_width = u16::from(layout.scrollbar.is_some());
    let width = layout.content.width.saturating_sub(scrollbar_width);
    if model.items.is_empty() {
        frame.render_widget(
            Paragraph::new(EMPTY_MESSAGE)
                .style(theme.muted_style())
                .alignment(Alignment::Center)
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
    frame.render_widget(
        Paragraph::new(detail_line("Name", "Type", "Integrity", "Path", widths))
            .style(theme.title_style()),
        Rect::new(
            layout.content.x,
            layout.content.y,
            width,
            u16::from(layout.content.height > 0),
        ),
    );
    for item_layout in &layout.items {
        let Some(item) = model.items.get(item_layout.index) else {
            continue;
        };
        let selected = model.selected_index == Some(item_layout.index) || item.selected;
        let style = item_style(item.status, selected, theme);
        let name = format!("[A] {}", item.name);
        frame.render_widget(
            Paragraph::new(detail_line(
                &name,
                &item.type_label,
                item.status.label(),
                &item.path,
                widths,
            ))
            .style(style),
            item_layout.area,
        );
    }
}

fn detail_widths(width: u16) -> [u16; 4] {
    let name = (width.saturating_mul(28) / 100).max(8);
    let kind = (width.saturating_mul(16) / 100).max(6);
    let integrity = (width.saturating_mul(18) / 100).max(8);
    let used = name.saturating_add(kind).saturating_add(integrity);
    [name, kind, integrity, width.saturating_sub(used)]
}

fn detail_line(
    name: &str,
    kind: &str,
    integrity: &str,
    path: &str,
    widths: [u16; 4],
) -> Line<'static> {
    Line::from(vec![
        Span::raw(pad_cell(name, widths[0])),
        Span::raw(pad_cell(kind, widths[1])),
        Span::raw(pad_cell(integrity, widths[2])),
        Span::raw(pad_cell(path, widths[3])),
    ])
}

fn render_launcher_footer(
    frame: &mut Frame<'_>,
    area: Rect,
    model: &LauncherViewModel,
    theme: &TundraTheme,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let (text, style) = if let Some(error) = &model.error {
        (error.clone(), theme.error_style())
    } else if let Some(message) = &model.message {
        (message.clone(), theme.body_style())
    } else {
        (
            format!(
                "{} item{} · Enter launch · Esc Home",
                model.items.len(),
                if model.items.len() == 1 { "" } else { "s" }
            ),
            theme.muted_style(),
        )
    };
    frame.render_widget(
        Paragraph::new(fit_text(&text, area.width)).style(style),
        area,
    );
}

fn render_launcher_scrollbar(
    frame: &mut Frame<'_>,
    area: Rect,
    layout: &LauncherLayout,
    model: &LauncherViewModel,
    theme: &TundraTheme,
) {
    if area.height == 0 || model.items.is_empty() {
        return;
    }
    let thumb_height = ((usize::from(area.height) * layout.visible_capacity) / model.items.len())
        .clamp(1, usize::from(area.height));
    let travel = usize::from(area.height).saturating_sub(thumb_height);
    let max_start = model
        .items
        .len()
        .saturating_sub(layout.visible_capacity)
        .max(1);
    let thumb_start = travel.saturating_mul(layout.visible_start) / max_start;
    for row in 0..usize::from(area.height) {
        let symbol = if (thumb_start..thumb_start.saturating_add(thumb_height)).contains(&row) {
            "█"
        } else {
            "│"
        };
        frame.render_widget(
            Paragraph::new(symbol).style(theme.muted_style()),
            Rect::new(
                area.x,
                area.y
                    .saturating_add(u16::try_from(row).unwrap_or(u16::MAX)),
                1,
                1,
            ),
        );
    }
}

fn render_launcher_confirmation(
    frame: &mut Frame<'_>,
    layout: LauncherConfirmationLayout,
    dialog: &LauncherConfirmationViewModel,
    theme: &TundraTheme,
) {
    frame.render_widget(Clear, layout.area);
    frame.render_widget(
        theme
            .block()
            .title(dialog.title.clone())
            .borders(Borders::ALL)
            .style(theme.body_style()),
        layout.area,
    );
    frame.render_widget(
        Paragraph::new(dialog.message.clone())
            .style(theme.body_style())
            .alignment(Alignment::Center)
            .wrap(Wrap { trim: true }),
        layout.message,
    );
    frame.render_widget(
        Paragraph::new(format!("[{}]", dialog.confirm_label))
            .style(if dialog.confirm_selected {
                theme.title_style()
            } else {
                theme.body_style()
            })
            .alignment(Alignment::Center),
        layout.confirm,
    );
    frame.render_widget(
        Paragraph::new(format!("[{}]", dialog.cancel_label))
            .style(if dialog.confirm_selected {
                theme.body_style()
            } else {
                theme.title_style()
            })
            .alignment(Alignment::Center),
        layout.cancel,
    );
}

fn item_style(status: LauncherItemStatus, selected: bool, theme: &TundraTheme) -> Style {
    if selected {
        theme.title_style()
    } else {
        status_style(status, theme)
    }
}

fn status_style(status: LauncherItemStatus, theme: &TundraTheme) -> Style {
    match status {
        LauncherItemStatus::Ready => theme.body_style(),
        LauncherItemStatus::Checking | LauncherItemStatus::NeedsApproval => theme.muted_style(),
        LauncherItemStatus::Changed
        | LauncherItemStatus::Missing
        | LauncherItemStatus::Unsupported => theme.error_style(),
    }
}

fn pad_cell(value: &str, width: u16) -> String {
    let mut value = fit_text(value, width);
    let len = value.chars().count();
    value.extend(std::iter::repeat_n(
        ' ',
        usize::from(width).saturating_sub(len),
    ));
    value
}

fn fit_text(value: &str, width: u16) -> String {
    let width = usize::from(width);
    if width == 0 {
        return String::new();
    }
    if value.chars().count() <= width {
        return value.to_string();
    }
    let mut result: String = value.chars().take(width.saturating_sub(1)).collect();
    result.push('…');
    result
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
