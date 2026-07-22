use ratatui::layout::Rect;

use super::model::*;
use crate::screens::shell::{centered_rect, inset_rect, line_in_rect, rect_contains, usize_to_u16};

const EXPLORER_SIDEBAR_MIN_WIDTH: u16 = 96;
const EXPLORER_DETAILED_MIN_WIDTH: u16 = 72;
const EXPLORER_SIDEBAR_WIDTH: u16 = 20;
const EXPLORER_FOOTER_TALL_HEIGHT: u16 = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExplorerScrollbarLayout {
    pub track: Rect,
    pub thumb: Rect,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExplorerLayoutMode {
    DetailedWithSidebar,
    Detailed,
    Compact,
}

impl ExplorerLayoutMode {
    pub const fn shows_all_columns(self) -> bool {
        !matches!(self, Self::Compact)
    }

    pub const fn shows_sidebar(self) -> bool {
        matches!(self, Self::DetailedWithSidebar)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExplorerToolbarButtonLayout {
    pub action: ExplorerToolbarAction,
    pub area: Rect,
    pub show_label: bool,
    pub enabled: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExplorerBreadcrumbLayout {
    pub index: usize,
    pub area: Rect,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExplorerQuickLocationLayout {
    pub index: usize,
    pub area: Rect,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExplorerColumnLayout {
    pub column: ExplorerSortColumn,
    pub area: Rect,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExplorerRowLayout {
    pub index: usize,
    pub area: Rect,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExplorerOverlayControl {
    ContextItem(usize),
    NameInput,
    Confirm,
    Cancel,
    Option(usize),
    OptionsClose,
    ConflictChoice(ExplorerConflictChoice),
    ApplyToRemaining,
    PropertiesClose,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExplorerOverlayControlLayout {
    pub control: ExplorerOverlayControl,
    pub area: Rect,
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExplorerOverlayLayout {
    pub area: Rect,
    pub content: Rect,
    pub controls: Vec<ExplorerOverlayControlLayout>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExplorerHitTarget {
    Toolbar(ExplorerToolbarAction),
    Address,
    Breadcrumb(usize),
    Search,
    QuickLocation(usize),
    Column(ExplorerSortColumn),
    Entry(usize),
    Scrollbar,
    CancelOperation,
    Overlay(ExplorerOverlayControl),
    OverlaySurface,
    EmptyTable,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExplorerLayout {
    pub mode: ExplorerLayoutMode,
    pub panel: Rect,
    pub toolbar: Rect,
    pub toolbar_buttons: Vec<ExplorerToolbarButtonLayout>,
    pub path_bar: Rect,
    pub address_button: Rect,
    pub address_input: Rect,
    pub breadcrumbs: Vec<ExplorerBreadcrumbLayout>,
    pub search: Rect,
    pub sidebar: Option<Rect>,
    pub sidebar_header: Option<Rect>,
    pub quick_locations: Vec<ExplorerQuickLocationLayout>,
    pub table: Rect,
    pub table_header: Rect,
    pub columns: Vec<ExplorerColumnLayout>,
    pub table_body: Rect,
    pub rows: Vec<ExplorerRowLayout>,
    pub scrollbar: Option<ExplorerScrollbarLayout>,
    pub footer: Rect,
    pub cancel_operation: Option<Rect>,
    pub visible_start: usize,
    pub visible_capacity: usize,
    pub overlay: Option<ExplorerOverlayLayout>,
}

impl ExplorerLayout {
    pub fn hit_test(&self, x: u16, y: u16) -> Option<ExplorerHitTarget> {
        if let Some(overlay) = &self.overlay {
            if let Some(control) = overlay
                .controls
                .iter()
                .find(|control| control.enabled && rect_contains(control.area, x, y))
            {
                return Some(ExplorerHitTarget::Overlay(control.control.clone()));
            }
            return rect_contains(overlay.area, x, y).then_some(ExplorerHitTarget::OverlaySurface);
        }

        if let Some(button) = self
            .toolbar_buttons
            .iter()
            .find(|button| button.enabled && rect_contains(button.area, x, y))
        {
            return Some(ExplorerHitTarget::Toolbar(button.action));
        }
        if rect_contains(self.search, x, y) {
            return Some(ExplorerHitTarget::Search);
        }
        if rect_contains(self.address_button, x, y) {
            return Some(ExplorerHitTarget::Address);
        }
        if let Some(crumb) = self
            .breadcrumbs
            .iter()
            .find(|crumb| rect_contains(crumb.area, x, y))
        {
            return Some(ExplorerHitTarget::Breadcrumb(crumb.index));
        }
        if let Some(location) = self
            .quick_locations
            .iter()
            .find(|location| rect_contains(location.area, x, y))
        {
            return Some(ExplorerHitTarget::QuickLocation(location.index));
        }
        if let Some(column) = self
            .columns
            .iter()
            .find(|column| rect_contains(column.area, x, y))
        {
            return Some(ExplorerHitTarget::Column(column.column));
        }
        if let Some(row) = self.rows.iter().find(|row| rect_contains(row.area, x, y)) {
            return Some(ExplorerHitTarget::Entry(row.index));
        }
        if self
            .scrollbar
            .is_some_and(|scrollbar| rect_contains(scrollbar.track, x, y))
        {
            return Some(ExplorerHitTarget::Scrollbar);
        }
        if self
            .cancel_operation
            .is_some_and(|cancel| rect_contains(cancel, x, y))
        {
            return Some(ExplorerHitTarget::CancelOperation);
        }
        rect_contains(self.table_body, x, y).then_some(ExplorerHitTarget::EmptyTable)
    }

    pub fn row_index_at(&self, x: u16, y: u16) -> Option<usize> {
        self.rows
            .iter()
            .find_map(|row| rect_contains(row.area, x, y).then_some(row.index))
    }
}

/// Computes the complete Explorer render and mouse-hit geometry.
///
/// Widths are intentionally based on the page area rather than terminal-global
/// coordinates: `>=96` enables the 20-cell quick-access sidebar, `>=72` keeps
/// every metadata column, and narrower pages show only Name and Type.
pub fn explorer_layout(area: Rect, model: &ExplorerViewModel) -> ExplorerLayout {
    let mode = if area.width >= EXPLORER_SIDEBAR_MIN_WIDTH && model.show_sidebar {
        ExplorerLayoutMode::DetailedWithSidebar
    } else if area.width >= EXPLORER_DETAILED_MIN_WIDTH {
        ExplorerLayoutMode::Detailed
    } else {
        ExplorerLayoutMode::Compact
    };
    let panel = area;
    let inner = inset_rect(panel, 1);
    let toolbar = line_in_rect(inner, inner.y);
    let path_bar = line_in_rect(inner, inner.y.saturating_add(1));
    let toolbar_buttons = explorer_toolbar_button_layouts(toolbar, model, mode);

    let search_width = if path_bar.width >= 96 {
        32
    } else if path_bar.width >= 72 {
        26
    } else {
        18.min(path_bar.width / 2)
    }
    .min(path_bar.width);
    let search = Rect::new(
        path_bar
            .x
            .saturating_add(path_bar.width.saturating_sub(search_width)),
        path_bar.y,
        search_width,
        path_bar.height,
    );
    let address_area = Rect::new(
        path_bar.x,
        path_bar.y,
        path_bar.width.saturating_sub(search_width),
        path_bar.height,
    );
    let address_button_width = 6.min(address_area.width);
    let address_gap = u16::from(address_area.width > address_button_width);
    let address_button = Rect::new(
        address_area.x,
        address_area.y,
        address_button_width,
        address_area.height,
    );
    let address_input = Rect::new(
        address_area
            .x
            .saturating_add(address_button_width)
            .saturating_add(address_gap),
        address_area.y,
        address_area
            .width
            .saturating_sub(address_button_width)
            .saturating_sub(address_gap),
        address_area.height,
    );
    let breadcrumbs = if model.address_editing {
        Vec::new()
    } else {
        explorer_breadcrumb_layouts(address_input, model)
    };

    let remaining_height = inner.height.saturating_sub(2);
    let footer_height = if remaining_height >= 11 {
        EXPLORER_FOOTER_TALL_HEIGHT
    } else if remaining_height >= 4 {
        2
    } else {
        u16::from(remaining_height > 0)
    };
    let body_y = inner.y.saturating_add(2);
    let body_height = remaining_height.saturating_sub(footer_height);
    let body = Rect::new(inner.x, body_y, inner.width, body_height);
    let footer = Rect::new(
        inner.x,
        body_y.saturating_add(body_height),
        inner.width,
        footer_height,
    );

    let (sidebar, table) = if mode.shows_sidebar() {
        let sidebar_width = EXPLORER_SIDEBAR_WIDTH.min(body.width);
        let gap = u16::from(body.width > sidebar_width);
        (
            Some(Rect::new(body.x, body.y, sidebar_width, body.height)),
            Rect::new(
                body.x.saturating_add(sidebar_width).saturating_add(gap),
                body.y,
                body.width.saturating_sub(sidebar_width).saturating_sub(gap),
                body.height,
            ),
        )
    } else {
        (None, body)
    };
    let sidebar_header = sidebar.map(|sidebar| line_in_rect(sidebar, sidebar.y));
    let quick_locations = sidebar.map_or_else(Vec::new, |sidebar| {
        model
            .quick_locations
            .iter()
            .enumerate()
            .take(usize::from(sidebar.height.saturating_sub(1)))
            .map(|(index, _)| ExplorerQuickLocationLayout {
                index,
                area: Rect::new(
                    sidebar.x,
                    sidebar
                        .y
                        .saturating_add(1)
                        .saturating_add(usize_to_u16(index)),
                    sidebar.width,
                    1,
                ),
            })
            .collect()
    });

    let table_header = line_in_rect(table, table.y);
    let table_body = Rect::new(
        table.x,
        table.y.saturating_add(table_header.height),
        table.width,
        table.height.saturating_sub(table_header.height),
    );
    let visible_capacity = usize::from(table_body.height);
    let visible_start = explorer_visible_start(model, visible_capacity);
    let show_scrollbar = visible_capacity > 0 && model.entries.len() > visible_capacity;
    let scrollbar = show_scrollbar.then(|| {
        let track = Rect::new(
            table_body
                .x
                .saturating_add(table_body.width.saturating_sub(1)),
            table_body.y,
            u16::from(table_body.width > 0),
            table_body.height,
        );
        let track_height = usize::from(track.height);
        let total = model.entries.len().max(1);
        let thumb_height = track_height
            .saturating_mul(visible_capacity)
            .checked_div(total)
            .unwrap_or_default()
            .clamp(1, track_height.max(1));
        let travel = track_height.saturating_sub(thumb_height);
        let max_start = total.saturating_sub(visible_capacity).max(1);
        let thumb_start = travel.saturating_mul(visible_start) / max_start;
        ExplorerScrollbarLayout {
            track,
            thumb: Rect::new(
                track.x,
                track.y.saturating_add(usize_to_u16(thumb_start)),
                track.width,
                usize_to_u16(thumb_height),
            ),
        }
    });
    let row_width = table_body.width.saturating_sub(u16::from(show_scrollbar));
    let rows = (visible_start..model.entries.len())
        .take(visible_capacity)
        .enumerate()
        .map(|(offset, index)| ExplorerRowLayout {
            index,
            area: Rect::new(
                table_body.x,
                table_body.y.saturating_add(usize_to_u16(offset)),
                row_width,
                1,
            ),
        })
        .collect();
    let columns = explorer_column_layouts(
        Rect::new(
            table_header.x,
            table_header.y,
            row_width,
            table_header.height,
        ),
        mode,
    );

    let cancel_operation = model.operation.as_ref().and_then(|operation| {
        if !operation.cancellable || footer.width == 0 || footer.height == 0 {
            return None;
        }
        let desired = usize_to_u16(operation.cancel_label.chars().count())
            .saturating_add(4)
            .min(footer.width);
        Some(Rect::new(
            footer
                .x
                .saturating_add(footer.width.saturating_sub(desired)),
            footer.y,
            desired,
            1,
        ))
    });
    let overlay = explorer_overlay_layout(area, model);

    ExplorerLayout {
        mode,
        panel,
        toolbar,
        toolbar_buttons,
        path_bar,
        address_button,
        address_input,
        breadcrumbs,
        search,
        sidebar,
        sidebar_header,
        quick_locations,
        table,
        table_header,
        columns,
        table_body,
        rows,
        scrollbar,
        footer,
        cancel_operation,
        visible_start,
        visible_capacity,
        overlay,
    }
}

pub fn explorer_hit_test(
    layout: &ExplorerLayout,
    coordinates: (u16, u16),
) -> Option<ExplorerHitTarget> {
    layout.hit_test(coordinates.0, coordinates.1)
}

fn explorer_toolbar_button_layouts(
    area: Rect,
    model: &ExplorerViewModel,
    mode: ExplorerLayoutMode,
) -> Vec<ExplorerToolbarButtonLayout> {
    let compact = matches!(mode, ExplorerLayoutMode::Compact);
    let labelled_width = model
        .toolbar
        .buttons
        .iter()
        .map(|button| usize_to_u16(button.label.chars().count()).saturating_add(4))
        .fold(0u16, u16::saturating_add)
        .saturating_add(model.toolbar.buttons.len().saturating_sub(1) as u16);
    // Either label every action or collapse every action to its asset icon.  A greedy mix made
    // the important trailing Rename/Delete/Sort/Options controls disappear at common widths.
    let show_labels = !compact && labelled_width <= area.width;
    let mut x = area.x;
    model
        .toolbar
        .buttons
        .iter()
        .filter_map(|button| {
            let remaining = area.x.saturating_add(area.width).saturating_sub(x);
            if remaining < 3 || area.height == 0 {
                return None;
            }
            let labelled_width = usize_to_u16(button.label.chars().count()).saturating_add(4);
            let show_label = show_labels;
            let width = if show_label { labelled_width } else { 3 }.min(remaining);
            let layout = ExplorerToolbarButtonLayout {
                action: button.action,
                area: Rect::new(x, area.y, width, 1),
                show_label,
                enabled: button.enabled,
            };
            x = x
                .saturating_add(width)
                .saturating_add(u16::from(remaining > width));
            Some(layout)
        })
        .collect()
}

fn explorer_breadcrumb_layouts(
    area: Rect,
    model: &ExplorerViewModel,
) -> Vec<ExplorerBreadcrumbLayout> {
    let mut x = area.x;
    model
        .breadcrumbs
        .iter()
        .enumerate()
        .filter_map(|(index, crumb)| {
            let remaining = area.x.saturating_add(area.width).saturating_sub(x);
            if remaining == 0 || area.height == 0 {
                return None;
            }
            let desired = usize_to_u16(crumb.label.chars().count()).saturating_add(3);
            let width = desired.min(remaining);
            let result = ExplorerBreadcrumbLayout {
                index,
                area: Rect::new(x, area.y, width, 1),
            };
            x = x.saturating_add(width);
            Some(result)
        })
        .collect()
}

fn explorer_visible_start(model: &ExplorerViewModel, capacity: usize) -> usize {
    if model.entries.is_empty() || capacity == 0 {
        return 0;
    }
    let max_start = model.entries.len().saturating_sub(capacity);
    let mut start = model.viewport_offset.min(max_start);
    if model.viewport_follows_focus
        && let Some(focused) = model
            .selected_index
            .filter(|index| *index < model.entries.len())
    {
        if focused < start {
            start = focused;
        } else if focused >= start.saturating_add(capacity) {
            start = focused.saturating_add(1).saturating_sub(capacity);
        }
    }
    start.min(max_start)
}

fn explorer_column_layouts(area: Rect, mode: ExplorerLayoutMode) -> Vec<ExplorerColumnLayout> {
    if area.width == 0 || area.height == 0 {
        return Vec::new();
    }
    let widths = if mode.shows_all_columns() {
        let kind = 16.min(area.width);
        let size = 12.min(area.width.saturating_sub(kind));
        let modified = 18.min(area.width.saturating_sub(kind).saturating_sub(size));
        let name = area
            .width
            .saturating_sub(kind)
            .saturating_sub(size)
            .saturating_sub(modified);
        vec![
            (ExplorerSortColumn::Name, name),
            (ExplorerSortColumn::Type, kind),
            (ExplorerSortColumn::Size, size),
            (ExplorerSortColumn::Modified, modified),
        ]
    } else {
        let kind = 16.min(area.width / 3).max(u16::from(area.width > 0));
        vec![
            (ExplorerSortColumn::Name, area.width.saturating_sub(kind)),
            (ExplorerSortColumn::Type, kind),
        ]
    };
    let mut x = area.x;
    widths
        .into_iter()
        .filter_map(|(column, width)| {
            if width == 0 {
                return None;
            }
            let result = ExplorerColumnLayout {
                column,
                area: Rect::new(x, area.y, width, 1),
            };
            x = x.saturating_add(width);
            Some(result)
        })
        .collect()
}

fn explorer_overlay_layout(area: Rect, model: &ExplorerViewModel) -> Option<ExplorerOverlayLayout> {
    match model.overlay.as_ref() {
        Some(ExplorerOverlayViewModel::ContextMenu(menu)) => {
            let content_width = menu
                .items
                .iter()
                .map(|item| {
                    item.label.chars().count().saturating_add(
                        item.shortcut
                            .as_ref()
                            .map_or(0, |value| value.chars().count().saturating_add(2)),
                    )
                })
                .max()
                .unwrap_or(12);
            let width = usize_to_u16(content_width)
                .saturating_add(4)
                .clamp(16, area.width.max(16));
            let height = usize_to_u16(menu.items.len())
                .saturating_add(2)
                .min(area.height);
            let x = menu
                .x
                .min(area.x.saturating_add(area.width.saturating_sub(width)));
            let y = menu
                .y
                .min(area.y.saturating_add(area.height.saturating_sub(height)));
            let dialog = Rect::new(x, y, width.min(area.width), height);
            let content = inset_rect(dialog, 1);
            let controls = menu
                .items
                .iter()
                .enumerate()
                .take(usize::from(content.height))
                .map(|(index, item)| ExplorerOverlayControlLayout {
                    control: ExplorerOverlayControl::ContextItem(index),
                    area: Rect::new(
                        content.x,
                        content.y.saturating_add(usize_to_u16(index)),
                        content.width,
                        1,
                    ),
                    enabled: item.enabled,
                })
                .collect();
            Some(ExplorerOverlayLayout {
                area: dialog,
                content,
                controls,
            })
        }
        Some(ExplorerOverlayViewModel::Name(_)) => {
            let dialog = centered_rect(area, area.width.min(60), area.height.min(9));
            let content = inset_rect(dialog, 1);
            let button_y = content.y.saturating_add(content.height.saturating_sub(1));
            let gap = u16::from(content.width > 2);
            let button_width = content.width.saturating_sub(gap) / 2;
            Some(ExplorerOverlayLayout {
                area: dialog,
                content,
                controls: vec![
                    ExplorerOverlayControlLayout {
                        control: ExplorerOverlayControl::NameInput,
                        area: line_in_rect(content, content.y.saturating_add(2)),
                        enabled: true,
                    },
                    ExplorerOverlayControlLayout {
                        control: ExplorerOverlayControl::Confirm,
                        area: Rect::new(content.x, button_y, button_width, 1),
                        enabled: true,
                    },
                    ExplorerOverlayControlLayout {
                        control: ExplorerOverlayControl::Cancel,
                        area: Rect::new(
                            content.x.saturating_add(button_width).saturating_add(gap),
                            button_y,
                            content
                                .width
                                .saturating_sub(button_width)
                                .saturating_sub(gap),
                            1,
                        ),
                        enabled: true,
                    },
                ],
            })
        }
        Some(ExplorerOverlayViewModel::Options(options)) => {
            let desired_height = usize_to_u16(options.options.len()).saturating_add(4);
            let dialog = centered_rect(area, area.width.min(64), area.height.min(desired_height));
            let content = inset_rect(dialog, 1);
            let option_capacity = usize::from(content.height.saturating_sub(1));
            let mut controls = options
                .options
                .iter()
                .enumerate()
                .take(option_capacity)
                .map(|(index, option)| ExplorerOverlayControlLayout {
                    control: ExplorerOverlayControl::Option(index),
                    area: Rect::new(
                        content.x,
                        content.y.saturating_add(usize_to_u16(index)),
                        content.width,
                        1,
                    ),
                    enabled: option.enabled,
                })
                .collect::<Vec<_>>();
            controls.push(ExplorerOverlayControlLayout {
                control: ExplorerOverlayControl::OptionsClose,
                area: line_in_rect(
                    content,
                    content.y.saturating_add(content.height.saturating_sub(1)),
                ),
                enabled: true,
            });
            Some(ExplorerOverlayLayout {
                area: dialog,
                content,
                controls,
            })
        }
        Some(ExplorerOverlayViewModel::Conflict(conflict)) => {
            let dialog = centered_rect(area, area.width.min(68), area.height.min(10));
            let content = inset_rect(dialog, 1);
            let choices_y = content
                .y
                .saturating_add(4.min(content.height.saturating_sub(1)));
            let count = usize_to_u16(conflict.choices.len()).max(1);
            let choice_width = content.width / count;
            let mut controls = conflict
                .choices
                .iter()
                .enumerate()
                .map(|(index, choice)| ExplorerOverlayControlLayout {
                    control: ExplorerOverlayControl::ConflictChoice(*choice),
                    area: Rect::new(
                        content
                            .x
                            .saturating_add(usize_to_u16(index).saturating_mul(choice_width)),
                        choices_y,
                        if index + 1 == conflict.choices.len() {
                            content
                                .width
                                .saturating_sub(usize_to_u16(index).saturating_mul(choice_width))
                        } else {
                            choice_width
                        },
                        1,
                    ),
                    enabled: true,
                })
                .collect::<Vec<_>>();
            if conflict.allow_apply_to_remaining {
                controls.push(ExplorerOverlayControlLayout {
                    control: ExplorerOverlayControl::ApplyToRemaining,
                    area: line_in_rect(content, choices_y.saturating_add(1)),
                    enabled: true,
                });
            }
            Some(ExplorerOverlayLayout {
                area: dialog,
                content,
                controls,
            })
        }
        Some(ExplorerOverlayViewModel::Properties(_)) => {
            let dialog = centered_rect(area, area.width.min(64), area.height.min(12));
            let content = inset_rect(dialog, 1);
            Some(ExplorerOverlayLayout {
                area: dialog,
                content,
                controls: vec![ExplorerOverlayControlLayout {
                    control: ExplorerOverlayControl::PropertiesClose,
                    area: line_in_rect(
                        content,
                        content.y.saturating_add(content.height.saturating_sub(1)),
                    ),
                    enabled: true,
                }],
            })
        }
        None if model.pending_dialog.is_some() => {
            let dialog = centered_rect(area, area.width.min(56), area.height.min(7));
            let content = inset_rect(dialog, 1);
            let button_y = content.y.saturating_add(content.height.saturating_sub(1));
            let gap = u16::from(content.width > 2);
            let confirm_width = content.width.saturating_sub(gap) / 2;
            Some(ExplorerOverlayLayout {
                area: dialog,
                content,
                controls: vec![
                    ExplorerOverlayControlLayout {
                        control: ExplorerOverlayControl::Confirm,
                        area: Rect::new(content.x, button_y, confirm_width, 1),
                        enabled: true,
                    },
                    ExplorerOverlayControlLayout {
                        control: ExplorerOverlayControl::Cancel,
                        area: Rect::new(
                            content.x.saturating_add(confirm_width).saturating_add(gap),
                            button_y,
                            content
                                .width
                                .saturating_sub(confirm_width)
                                .saturating_sub(gap),
                            1,
                        ),
                        enabled: true,
                    },
                ],
            })
        }
        None => None,
    }
}
