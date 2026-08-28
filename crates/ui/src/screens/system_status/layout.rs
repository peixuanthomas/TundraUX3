use super::model::*;
use crate::screens::shell::{inset_rect, line_in_rect, rect_contains, usize_to_u16};
use crate::{
    DiagnosticsContentLayout, DiagnosticsHitTarget, DiagnosticsRepairDialogLayout,
    diagnostics_content_hit_test, diagnostics_content_layout, diagnostics_repair_dialog_hit_test,
    diagnostics_repair_dialog_layout,
};
use ratatui::layout::Rect;

pub const LOGICAL_ROW_HEIGHT: u16 = 2;
pub const LOGICAL_ROW_GAP: u16 = 1;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SystemStatusWidgetLayout {
    pub kind: SystemStatusWidgetKind,
    pub size: SystemStatusWidgetSize,
    pub logical_column: u16,
    pub logical_row: u16,
    pub area: Rect,
    pub preview: bool,
    pub preview_valid: bool,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SystemStatusRowLayout {
    pub index: usize,
    pub area: Rect,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SystemStatusHitTarget {
    Widget(SystemStatusWidgetKind),
    PickerItem(usize),
    Row(usize),
    Diagnostics(DiagnosticsHitTarget),
    Refresh,
    Edit,
    Add,
    Size,
    Remove,
    Save,
    Cancel,
    Scrollbar,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SystemStatusLayout {
    pub panel: Rect,
    pub header: Rect,
    pub content_panel: Rect,
    pub canvas: Rect,
    pub footer: Rect,
    pub profile: SystemStatusDashboardProfile,
    pub column_count: u16,
    pub widgets: Vec<SystemStatusWidgetLayout>,
    pub picker_items: Vec<SystemStatusRowLayout>,
    pub visible_row_start: u16,
    pub visible_row_end: u16,
    pub scrollbar: Option<Rect>,
    pub empty_canvas: bool,
    pub refresh_button: Rect,
    pub edit_button: Rect,
    pub add_button: Rect,
    pub size_button: Rect,
    pub remove_button: Rect,
    pub save_button: Rect,
    pub cancel_button: Rect,
    pub notice_area: Option<Rect>,
    pub rows_area: Rect,
    pub rows: Vec<SystemStatusRowLayout>,
    pub visible_start: usize,
    pub visible_capacity: usize,
    pub diagnostics_content: Option<DiagnosticsContentLayout>,
    pub diagnostics_repair_dialog: Option<DiagnosticsRepairDialogLayout>,
}
fn button_from_right(footer: Rect, right: &mut u16, width: u16) -> Rect {
    let w = width.min(*right - footer.x);
    *right = right.saturating_sub(w);
    let r = Rect::new(*right, footer.y, w, footer.height);
    *right = right.saturating_sub(u16::from(*right > footer.x));
    r
}
pub fn system_status_layout(main: Rect, model: &SystemStatusViewModel) -> SystemStatusLayout {
    let panel = main;
    let inner = inset_rect(panel, 1);
    let header = line_in_rect(inner, inner.y);
    let footer = line_in_rect(inner, inner.bottom().saturating_sub(1));
    let content_panel = Rect::new(
        inner.x,
        header.bottom(),
        inner.width,
        footer.y.saturating_sub(header.bottom()),
    );
    let canvas = inset_rect(content_panel, 1);
    let wide = content_panel.width.saturating_sub(7) / 8 >= 10;
    let profile = if wide {
        SystemStatusDashboardProfile::Wide
    } else {
        SystemStatusDashboardProfile::Narrow
    };
    let column_count: u16 = if wide { 8 } else { 4 };
    let mut right = footer.right();
    let refresh_button = button_from_right(footer, &mut right, 11);
    let edit_button = button_from_right(footer, &mut right, 8);
    right = footer.right();
    let cancel_button = button_from_right(footer, &mut right, 10);
    let save_button = button_from_right(footer, &mut right, 8);
    let remove_button = button_from_right(footer, &mut right, 10);
    let size_button = button_from_right(footer, &mut right, 8);
    let add_button = button_from_right(footer, &mut right, 7);
    let empty_canvas = canvas.height < 5;
    let visible_row_start = model.dashboard.scroll_row;
    let visible_rows =
        canvas.height.saturating_add(LOGICAL_ROW_GAP) / (LOGICAL_ROW_HEIGHT + LOGICAL_ROW_GAP);
    let visible_row_end = visible_row_start.saturating_add(visible_rows);
    let all = model.dashboard.widgets(profile);
    let max_row = all
        .iter()
        .map(|w| w.row.saturating_add(w.size.rows()))
        .max()
        .unwrap_or(0);
    let scrollbar = (max_row > visible_rows && canvas.width > 2 && !empty_canvas)
        .then(|| Rect::new(canvas.right().saturating_sub(1), canvas.y, 1, canvas.height));
    let grid_width = canvas
        .width
        .saturating_sub(if scrollbar.is_some() { 2 } else { 0 });
    let gaps = column_count.saturating_sub(1);
    let cell_w = grid_width.saturating_sub(gaps) / column_count;
    let rect_for = |column: u16, row: u16, size: SystemStatusWidgetSize| {
        let rel = row.saturating_sub(visible_row_start);
        Rect::new(
            canvas.x.saturating_add(column.saturating_mul(cell_w + 1)),
            canvas
                .y
                .saturating_add(rel.saturating_mul(LOGICAL_ROW_HEIGHT + LOGICAL_ROW_GAP)),
            size.cols()
                .saturating_mul(cell_w)
                .saturating_add(size.cols().saturating_sub(1)),
            size.rows()
                .saturating_mul(LOGICAL_ROW_HEIGHT)
                .saturating_add(
                    size.rows()
                        .saturating_sub(1)
                        .saturating_mul(LOGICAL_ROW_GAP),
                ),
        )
    };
    let mut widgets = if empty_canvas {
        vec![]
    } else {
        all.iter()
            .filter(|w| {
                w.row < visible_row_end
                    && w.row.saturating_add(w.size.rows()) > visible_row_start
                    && w.column.saturating_add(w.size.cols()) <= column_count
            })
            .map(|w| SystemStatusWidgetLayout {
                kind: w.kind,
                size: w.size,
                logical_column: w.column,
                logical_row: w.row,
                area: rect_for(w.column, w.row, w.size).intersection(canvas),
                preview: false,
                preview_valid: true,
            })
            .collect()
    };
    if let Some(d) = model.dashboard.dragging.filter(|_| model.dashboard.editing) {
        if let Some(source) = all.iter().find(|w| w.kind == d.kind) {
            widgets.push(SystemStatusWidgetLayout {
                kind: d.kind,
                size: source.size,
                logical_column: d.column,
                logical_row: d.row,
                area: rect_for(d.column, d.row, source.size).intersection(canvas),
                preview: true,
                preview_valid: d.valid,
            });
        }
    }
    // Detail tables and diagnostics reuse the established inner content geometry.
    let detail = match model.route {
        SystemStatusRoute::Detail(d) => Some(d),
        _ => None,
    };
    let item_count = model.item_count();
    let table_inner = canvas;
    let visible_capacity =
        usize::from(table_inner.height.saturating_sub(u16::from(item_count > 0)));
    let visible_start = model
        .scroll_offset
        .min(item_count.saturating_sub(visible_capacity));
    let rows_area = Rect::new(
        table_inner.x,
        table_inner.y,
        table_inner
            .width
            .saturating_sub(u16::from(item_count > visible_capacity) * 2),
        table_inner.height,
    );
    let rows = (visible_start..item_count)
        .take(visible_capacity)
        .enumerate()
        .map(|(o, index)| SystemStatusRowLayout {
            index,
            area: Rect::new(
                rows_area.x,
                rows_area
                    .y
                    .saturating_add(1)
                    .saturating_add(usize_to_u16(o)),
                rows_area.width,
                1,
            ),
        })
        .collect();
    let picker_items = model
        .dashboard
        .picker
        .as_ref()
        .map(|p| {
            let w = panel.width.min(42);
            let h = panel
                .height
                .min((p.items.len() as u16).saturating_add(2).max(5));
            let area = Rect::new(
                panel.x + (panel.width - w) / 2,
                panel.y + (panel.height - h) / 2,
                w,
                h,
            );
            p.items
                .iter()
                .enumerate()
                .take(area.height.saturating_sub(2) as usize)
                .map(|(index, _)| SystemStatusRowLayout {
                    index,
                    area: Rect::new(
                        area.x.saturating_add(1),
                        area.y.saturating_add(1 + index as u16),
                        area.width.saturating_sub(2),
                        1,
                    ),
                })
                .collect()
        })
        .unwrap_or_default();
    let diagnostics_content = matches!(
        detail,
        Some(SystemStatusDetail::Diagnostics | SystemStatusDetail::Activity)
    )
    .then(|| diagnostics_content_layout(canvas, &model.diagnostics));
    let diagnostics_repair_dialog = diagnostics_content.as_ref().and_then(|_| {
        model
            .diagnostics
            .repair_dialog
            .as_ref()
            .map(|d| diagnostics_repair_dialog_layout(main, d))
    });
    SystemStatusLayout {
        panel,
        header,
        content_panel,
        canvas,
        footer,
        profile,
        column_count,
        widgets,
        picker_items,
        visible_row_start,
        visible_row_end,
        scrollbar,
        empty_canvas,
        refresh_button,
        edit_button,
        add_button,
        size_button,
        remove_button,
        save_button,
        cancel_button,
        notice_area: None,
        rows_area,
        rows,
        visible_start,
        visible_capacity,
        diagnostics_content,
        diagnostics_repair_dialog,
    }
}
pub fn system_status_hit_test(
    l: &SystemStatusLayout,
    p: (u16, u16),
) -> Option<SystemStatusHitTarget> {
    let (x, y) = p;
    if !l.picker_items.is_empty() {
        return l
            .picker_items
            .iter()
            .find(|r| rect_contains(r.area, x, y))
            .map(|r| SystemStatusHitTarget::PickerItem(r.index));
    }
    if let Some(d) = &l.diagnostics_repair_dialog {
        return diagnostics_repair_dialog_hit_test(d, p).map(SystemStatusHitTarget::Diagnostics);
    }
    if let Some(d) = &l.diagnostics_content {
        if let Some(t) = diagnostics_content_hit_test(d, p) {
            return Some(SystemStatusHitTarget::Diagnostics(t));
        }
    }
    for (a, t) in [
        (l.refresh_button, SystemStatusHitTarget::Refresh),
        (l.edit_button, SystemStatusHitTarget::Edit),
        (l.add_button, SystemStatusHitTarget::Add),
        (l.size_button, SystemStatusHitTarget::Size),
        (l.remove_button, SystemStatusHitTarget::Remove),
        (l.save_button, SystemStatusHitTarget::Save),
        (l.cancel_button, SystemStatusHitTarget::Cancel),
    ] {
        if rect_contains(a, x, y) {
            return Some(t);
        }
    }
    if l.scrollbar.is_some_and(|a| rect_contains(a, x, y)) {
        return Some(SystemStatusHitTarget::Scrollbar);
    }
    if let Some(w) = l
        .widgets
        .iter()
        .rev()
        .find(|w| !w.preview && rect_contains(w.area, x, y))
    {
        return Some(SystemStatusHitTarget::Widget(w.kind));
    }
    l.rows
        .iter()
        .find(|r| rect_contains(r.area, x, y))
        .map(|r| SystemStatusHitTarget::Row(r.index))
}
