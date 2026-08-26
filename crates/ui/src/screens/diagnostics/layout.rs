use ratatui::layout::Rect;

use super::model::{DiagnosticsRepairDialogViewModel, DiagnosticsTab, DiagnosticsViewModel};
use crate::screens::shell::{centered_rect, inset_rect, line_in_rect, rect_contains, usize_to_u16};

const DIAGNOSTICS_LIST_MIN_WIDTH: u16 = 28;
const DIAGNOSTICS_LIST_MAX_WIDTH: u16 = 44;
const DIAGNOSTICS_REPAIR_DIALOG_WIDTH: u16 = 68;
const DIAGNOSTICS_REPAIR_DIALOG_MAX_HEIGHT: u16 = 13;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiagnosticsTabLayout {
    pub tab: DiagnosticsTab,
    pub area: Rect,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiagnosticsRowLayout {
    pub index: usize,
    pub area: Rect,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiagnosticsScrollbarLayout {
    pub track: Rect,
    pub thumb: Rect,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiagnosticsRepairDialogLayout {
    pub dialog: Rect,
    pub prompt: Rect,
    pub items_area: Rect,
    pub rows: Vec<DiagnosticsRowLayout>,
    pub help: Rect,
    pub confirm: Rect,
    pub restart: Rect,
    pub cancel: Rect,
    pub visible_start: usize,
    pub visible_capacity: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiagnosticsContentLayout {
    pub active_tab: DiagnosticsTab,
    pub list_panel: Rect,
    pub list_rows_area: Rect,
    pub list_scrollbar: Option<DiagnosticsScrollbarLayout>,
    pub rows: Vec<DiagnosticsRowLayout>,
    pub detail_panel: Rect,
    pub visible_start: usize,
    pub visible_capacity: usize,
}

impl DiagnosticsContentLayout {
    pub fn hit_test(&self, x: u16, y: u16) -> Option<DiagnosticsHitTarget> {
        diagnostics_content_hit_test_parts(self.active_tab, self.list_scrollbar, &self.rows, x, y)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagnosticsHitTarget {
    Tab(DiagnosticsTab),
    Check(usize),
    Incident(usize),
    Log(usize),
    Scrollbar,
    RepairItem(usize),
    RepairConfirm,
    RepairRestart,
    RepairCancel,
    RepairDialogSurface,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiagnosticsLayout {
    pub panel: Rect,
    pub active_tab: DiagnosticsTab,
    pub header: Rect,
    pub tabs_area: Rect,
    pub tabs: Vec<DiagnosticsTabLayout>,
    pub list_panel: Rect,
    pub list_rows_area: Rect,
    pub list_scrollbar: Option<DiagnosticsScrollbarLayout>,
    pub rows: Vec<DiagnosticsRowLayout>,
    pub detail_panel: Rect,
    pub footer: Rect,
    pub visible_start: usize,
    pub visible_capacity: usize,
    pub repair_dialog: Option<DiagnosticsRepairDialogLayout>,
}

impl DiagnosticsLayout {
    pub fn content_layout(&self) -> DiagnosticsContentLayout {
        DiagnosticsContentLayout {
            active_tab: self.active_tab,
            list_panel: self.list_panel,
            list_rows_area: self.list_rows_area,
            list_scrollbar: self.list_scrollbar,
            rows: self.rows.clone(),
            detail_panel: self.detail_panel,
            visible_start: self.visible_start,
            visible_capacity: self.visible_capacity,
        }
    }

    pub fn hit_test(&self, x: u16, y: u16) -> Option<DiagnosticsHitTarget> {
        if let Some(dialog) = &self.repair_dialog {
            return diagnostics_repair_dialog_hit_test(dialog, (x, y));
        }

        if let Some(tab) = self.tabs.iter().find(|tab| rect_contains(tab.area, x, y)) {
            return Some(DiagnosticsHitTarget::Tab(tab.tab));
        }
        diagnostics_content_hit_test_parts(self.active_tab, self.list_scrollbar, &self.rows, x, y)
    }
}

pub fn diagnostics_repair_dialog_hit_test(
    dialog: &DiagnosticsRepairDialogLayout,
    coordinates: (u16, u16),
) -> Option<DiagnosticsHitTarget> {
    let (x, y) = coordinates;
    if let Some(row) = dialog.rows.iter().find(|row| rect_contains(row.area, x, y)) {
        return Some(DiagnosticsHitTarget::RepairItem(row.index));
    }
    if rect_contains(dialog.confirm, x, y) {
        return Some(DiagnosticsHitTarget::RepairConfirm);
    }
    if rect_contains(dialog.restart, x, y) {
        return Some(DiagnosticsHitTarget::RepairRestart);
    }
    if rect_contains(dialog.cancel, x, y) {
        return Some(DiagnosticsHitTarget::RepairCancel);
    }
    rect_contains(dialog.dialog, x, y).then_some(DiagnosticsHitTarget::RepairDialogSurface)
}

fn diagnostics_content_hit_test_parts(
    active_tab: DiagnosticsTab,
    list_scrollbar: Option<DiagnosticsScrollbarLayout>,
    rows: &[DiagnosticsRowLayout],
    x: u16,
    y: u16,
) -> Option<DiagnosticsHitTarget> {
    if list_scrollbar.is_some_and(|scrollbar| rect_contains(scrollbar.track, x, y)) {
        return Some(DiagnosticsHitTarget::Scrollbar);
    }
    rows.iter()
        .find(|row| rect_contains(row.area, x, y))
        .map(|row| match active_tab {
            DiagnosticsTab::Health => DiagnosticsHitTarget::Check(row.index),
            DiagnosticsTab::Incidents => DiagnosticsHitTarget::Incident(row.index),
            DiagnosticsTab::Logs => DiagnosticsHitTarget::Log(row.index),
        })
}

/// Computes the two-column Diagnostics page and modal hit geometry.
///
/// Callers should pass the `main` rectangle from [`compute_shell_layout`]. The
/// returned window start is clamped so the active selection remains visible.
pub fn diagnostics_layout(main: Rect, model: &DiagnosticsViewModel) -> DiagnosticsLayout {
    let panel = main;
    let inner = inset_rect(panel, 1);
    let inner_bottom = inner.y.saturating_add(inner.height);
    let header = line_in_rect(inner, inner.y);
    let tabs_area = line_in_rect(inner, inner.y.saturating_add(header.height));
    let footer = line_in_rect(inner, inner_bottom.saturating_sub(1));
    let content_y = tabs_area.y.saturating_add(tabs_area.height);
    let content_height = footer.y.saturating_sub(content_y);
    let content = Rect::new(inner.x, content_y, inner.width, content_height);

    let content_layout = diagnostics_content_layout(content, model);

    let mut tab_x = tabs_area.x;
    let tabs = DiagnosticsTab::ALL
        .into_iter()
        .map(|tab| {
            let desired = usize_to_u16(tab.label().chars().count()).saturating_add(4);
            let width = desired.min(tabs_area.right().saturating_sub(tab_x));
            let area = Rect::new(tab_x, tabs_area.y, width, tabs_area.height);
            tab_x = tab_x.saturating_add(width);
            DiagnosticsTabLayout { tab, area }
        })
        .collect();
    DiagnosticsLayout {
        panel,
        active_tab: content_layout.active_tab,
        header,
        tabs_area,
        tabs,
        list_panel: content_layout.list_panel,
        list_rows_area: content_layout.list_rows_area,
        list_scrollbar: content_layout.list_scrollbar,
        rows: content_layout.rows,
        detail_panel: content_layout.detail_panel,
        footer,
        visible_start: content_layout.visible_start,
        visible_capacity: content_layout.visible_capacity,
        repair_dialog: model
            .repair_dialog
            .as_ref()
            .map(|dialog| diagnostics_repair_dialog_layout(main, dialog)),
    }
}

/// Computes the reusable list-and-details region used by Diagnostics hosts.
pub fn diagnostics_content_layout(
    content: Rect,
    model: &DiagnosticsViewModel,
) -> DiagnosticsContentLayout {
    let column_gap = u16::from(content.width >= 3);
    let available_width = content.width.saturating_sub(column_gap);
    let list_width = if available_width >= DIAGNOSTICS_LIST_MIN_WIDTH.saturating_mul(2) {
        (available_width.saturating_mul(2) / 5)
            .clamp(DIAGNOSTICS_LIST_MIN_WIDTH, DIAGNOSTICS_LIST_MAX_WIDTH)
    } else {
        available_width / 2
    };
    let detail_width = available_width.saturating_sub(list_width);
    let list_panel = Rect::new(content.x, content.y, list_width, content.height);
    let detail_panel = Rect::new(
        content
            .x
            .saturating_add(list_width)
            .saturating_add(column_gap),
        content.y,
        detail_width,
        content.height,
    );
    let list_inner = inset_rect(list_panel, 1);
    let visible_capacity = usize::from(list_inner.height);
    let visible_start = diagnostics_visible_start(
        model.item_count(),
        model.selected_index(),
        model.list_window_start,
        visible_capacity,
        model.list_window_is_explicit,
    );
    let list_scrollbar = diagnostics_scrollbar_layout(
        list_inner,
        model.item_count(),
        visible_start,
        visible_capacity,
    );
    let list_rows_area = if list_scrollbar.is_some() {
        Rect::new(
            list_inner.x,
            list_inner.y,
            list_inner.width.saturating_sub(2),
            list_inner.height,
        )
    } else {
        list_inner
    };
    let rows = (visible_start..model.item_count())
        .take(visible_capacity)
        .enumerate()
        .map(|(offset, index)| DiagnosticsRowLayout {
            index,
            area: Rect::new(
                list_rows_area.x,
                list_rows_area.y.saturating_add(usize_to_u16(offset)),
                list_rows_area.width,
                1,
            ),
        })
        .collect();

    DiagnosticsContentLayout {
        active_tab: model.tab,
        list_panel,
        list_rows_area,
        list_scrollbar,
        rows,
        detail_panel,
        visible_start,
        visible_capacity,
    }
}

pub fn diagnostics_content_hit_test(
    layout: &DiagnosticsContentLayout,
    coordinates: (u16, u16),
) -> Option<DiagnosticsHitTarget> {
    layout.hit_test(coordinates.0, coordinates.1)
}

fn diagnostics_scrollbar_layout(
    list_inner: Rect,
    item_count: usize,
    visible_start: usize,
    visible_capacity: usize,
) -> Option<DiagnosticsScrollbarLayout> {
    if item_count <= visible_capacity || list_inner.width < 3 || list_inner.height == 0 {
        return None;
    }

    let track = Rect::new(
        list_inner.right().saturating_sub(1),
        list_inner.y,
        1,
        list_inner.height,
    );
    let track_height = usize::from(track.height);
    let visible_count = visible_capacity.min(item_count);
    let thumb_height = track_height
        .saturating_mul(visible_count)
        .saturating_add(item_count / 2)
        .checked_div(item_count)
        .unwrap_or_default()
        .max(1)
        .min(track_height);
    let max_thumb_start = track_height.saturating_sub(thumb_height);
    let max_visible_start = item_count.saturating_sub(visible_count);
    let thumb_start = visible_start
        .min(max_visible_start)
        .saturating_mul(max_thumb_start)
        .saturating_add(max_visible_start / 2)
        .checked_div(max_visible_start)
        .unwrap_or_default();
    let thumb = Rect::new(
        track.x,
        track.y.saturating_add(usize_to_u16(thumb_start)),
        1,
        usize_to_u16(thumb_height),
    );

    Some(DiagnosticsScrollbarLayout { track, thumb })
}

pub fn diagnostics_hit_test(
    layout: &DiagnosticsLayout,
    coordinates: (u16, u16),
) -> Option<DiagnosticsHitTarget> {
    layout.hit_test(coordinates.0, coordinates.1)
}

fn diagnostics_visible_start(
    item_count: usize,
    selected_index: usize,
    requested_start: usize,
    visible_capacity: usize,
    requested_start_is_explicit: bool,
) -> usize {
    if item_count == 0 || visible_capacity == 0 {
        return 0;
    }

    let selected = selected_index.min(item_count.saturating_sub(1));
    let max_start = item_count.saturating_sub(visible_capacity);
    let mut start = requested_start.min(max_start);
    if !requested_start_is_explicit {
        if selected < start {
            start = selected;
        } else if selected >= start.saturating_add(visible_capacity) {
            start = selected.saturating_add(1).saturating_sub(visible_capacity);
        }
    }
    start.min(max_start)
}

pub fn diagnostics_repair_dialog_layout(
    main: Rect,
    model: &DiagnosticsRepairDialogViewModel,
) -> DiagnosticsRepairDialogLayout {
    let desired_height = usize_to_u16(model.items.len().min(6))
        .saturating_add(7)
        .min(DIAGNOSTICS_REPAIR_DIALOG_MAX_HEIGHT);
    let dialog = centered_rect(
        main,
        main.width.min(DIAGNOSTICS_REPAIR_DIALOG_WIDTH),
        main.height.min(desired_height),
    );
    let inner = inset_rect(dialog, 1);
    let inner_bottom = inner.y.saturating_add(inner.height);
    let prompt_height = inner.height.min(2);
    let prompt = Rect::new(inner.x, inner.y, inner.width, prompt_height);
    let button_y = inner_bottom.saturating_sub(1);
    let help_y = button_y.saturating_sub(1).max(prompt.bottom());
    let help = line_in_rect(inner, help_y);
    let items_y = prompt.bottom();
    let items_area = Rect::new(
        inner.x,
        items_y,
        inner.width,
        help_y.saturating_sub(items_y),
    );
    let visible_capacity = usize::from(items_area.height);
    let visible_start = diagnostics_visible_start(
        model.items.len(),
        model.selected,
        model.scroll_offset,
        visible_capacity,
        false,
    );
    let rows = (visible_start..model.items.len())
        .take(visible_capacity)
        .enumerate()
        .map(|(offset, index)| DiagnosticsRowLayout {
            index,
            area: Rect::new(
                items_area.x,
                items_area.y.saturating_add(usize_to_u16(offset)),
                items_area.width,
                1,
            ),
        })
        .collect();
    let button_gap = u16::from(inner.width >= 5).saturating_mul(2);
    let buttons_width = inner.width.saturating_sub(button_gap.saturating_mul(2));
    let confirm_width = buttons_width / 3;
    let restart_width = buttons_width.saturating_sub(confirm_width) / 2;
    let cancel_width = buttons_width
        .saturating_sub(confirm_width)
        .saturating_sub(restart_width);
    let button_height = u16::from(inner.height > 0);
    let confirm = Rect::new(inner.x, button_y, confirm_width, button_height);
    let restart = Rect::new(
        inner
            .x
            .saturating_add(confirm_width)
            .saturating_add(button_gap),
        button_y,
        restart_width,
        button_height,
    );
    let cancel = Rect::new(
        restart
            .x
            .saturating_add(restart_width)
            .saturating_add(button_gap),
        button_y,
        cancel_width,
        button_height,
    );

    DiagnosticsRepairDialogLayout {
        dialog,
        prompt,
        items_area,
        rows,
        help,
        confirm,
        restart,
        cancel,
        visible_start,
        visible_capacity,
    }
}
