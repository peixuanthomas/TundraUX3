use super::model::*;
use crate::components::{Dialog, DialogAction, TabItem, Tabs};
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
pub struct SystemStatusActivityTabLayout {
    pub tab: crate::DiagnosticsTab,
    pub area: Rect,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SystemStatusHitTarget {
    Widget(SystemStatusWidgetKind),
    PickerItem(usize),
    SizePickerItem(usize),
    Row(usize),
    Diagnostics(DiagnosticsHitTarget),
    Refresh,
    Edit,
    Add,
    Size,
    Remove,
    Save,
    Cancel,
    DialogConfirm,
    DialogCancel,
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
    pub picker_viewport_start: usize,
    pub size_picker_items: Vec<SystemStatusRowLayout>,
    pub dialog_actions: Vec<SystemStatusRowLayout>,
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
    pub detail_summary_area: Rect,
    pub detail_trend_area: Rect,
    pub activity_tabs_area: Option<Rect>,
    pub activity_tabs: Vec<SystemStatusActivityTabLayout>,
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
    let mut refresh_button = button_from_right(footer, &mut right, 11);
    let mut edit_button = button_from_right(footer, &mut right, 8);
    right = footer.right();
    let mut cancel_button = button_from_right(footer, &mut right, 10);
    let mut save_button = button_from_right(footer, &mut right, 8);
    let mut remove_button = button_from_right(footer, &mut right, 10);
    let mut size_button = button_from_right(footer, &mut right, 8);
    let mut add_button = button_from_right(footer, &mut right, 7);
    if model.dashboard.editing {
        refresh_button = Rect::default();
        edit_button = Rect::default();
    } else {
        add_button = Rect::default();
        size_button = Rect::default();
        remove_button = Rect::default();
        save_button = Rect::default();
        cancel_button = Rect::default();
    }
    let empty_canvas = canvas.height < 5;
    let visible_rows =
        canvas.height.saturating_add(LOGICAL_ROW_GAP) / (LOGICAL_ROW_HEIGHT + LOGICAL_ROW_GAP);
    let all = model.dashboard.widgets(profile);
    let max_row = all
        .iter()
        .map(|w| w.row.saturating_add(w.size.rows()))
        .max()
        .unwrap_or(0);
    let visible_row_start = model
        .dashboard
        .scroll_row
        .min(max_row.saturating_sub(visible_rows));
    let visible_row_end = visible_row_start.saturating_add(visible_rows);
    let dashboard_scrollbar = (max_row > visible_rows && canvas.width > 2 && !empty_canvas)
        .then(|| Rect::new(canvas.right().saturating_sub(1), canvas.y, 1, canvas.height));
    let grid_width = canvas
        .width
        .saturating_sub(if dashboard_scrollbar.is_some() { 2 } else { 0 });
    let gaps = column_count.saturating_sub(1);
    let cell_w = grid_width.saturating_sub(gaps) / column_count;
    let rect_for = |column: u16, row: u16, size: SystemStatusWidgetSize| {
        let stride = i32::from(LOGICAL_ROW_HEIGHT + LOGICAL_ROW_GAP);
        let full_y = i32::from(canvas.y) + (i32::from(row) - i32::from(visible_row_start)) * stride;
        let full_height = i32::from(
            size.rows()
                .saturating_mul(LOGICAL_ROW_HEIGHT)
                .saturating_add(
                    size.rows()
                        .saturating_sub(1)
                        .saturating_mul(LOGICAL_ROW_GAP),
                ),
        );
        let clipped_y = full_y.max(i32::from(canvas.y));
        let clipped_bottom = full_y
            .saturating_add(full_height)
            .min(i32::from(canvas.bottom()))
            .max(clipped_y);
        Rect::new(
            canvas.x.saturating_add(column.saturating_mul(cell_w + 1)),
            u16::try_from(clipped_y).unwrap_or(canvas.y),
            size.cols()
                .saturating_mul(cell_w)
                .saturating_add(size.cols().saturating_sub(1)),
            u16::try_from(clipped_bottom.saturating_sub(clipped_y)).unwrap_or(0),
        )
        .intersection(canvas)
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
                area: rect_for(w.column, w.row, w.size),
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
    let formatted_detail = detail.is_some_and(|detail| {
        !matches!(
            detail,
            SystemStatusDetail::Storage
                | SystemStatusDetail::Network
                | SystemStatusDetail::Diagnostics
                | SystemStatusDetail::Activity
        )
    });
    let detail_summary_height = if formatted_detail {
        canvas.height.min(4)
    } else {
        0
    };
    let detail_trend_height = if formatted_detail
        && detail
            .and_then(|detail| model.detail_widget(detail))
            .and_then(|widget| widget.trend.as_ref())
            .is_some_and(|trend| !trend.is_empty())
        && canvas.height > detail_summary_height.saturating_add(2)
    {
        2
    } else {
        0
    };
    let detail_summary_area = Rect::new(canvas.x, canvas.y, canvas.width, detail_summary_height);
    let detail_trend_area = Rect::new(
        canvas.x,
        detail_summary_area.bottom(),
        canvas.width,
        detail_trend_height,
    );
    let table_inner = if formatted_detail {
        Rect::new(
            canvas.x,
            detail_trend_area.bottom(),
            canvas.width,
            canvas.bottom().saturating_sub(detail_trend_area.bottom()),
        )
    } else {
        canvas
    };
    let visible_capacity =
        usize::from(table_inner.height.saturating_sub(u16::from(item_count > 0)));
    let maximum_start = item_count.saturating_sub(visible_capacity);
    let requested_start = model.scroll_offset.min(maximum_start);
    let visible_start = if visible_capacity == 0 {
        requested_start
    } else if model.selected_row < requested_start {
        model.selected_row.min(maximum_start)
    } else if model.selected_row >= requested_start.saturating_add(visible_capacity) {
        model
            .selected_row
            .saturating_add(1)
            .saturating_sub(visible_capacity)
            .min(maximum_start)
    } else {
        requested_start
    };
    let detail_has_scroll = item_count > visible_capacity
        && !matches!(
            model.route,
            SystemStatusRoute::Detail(
                SystemStatusDetail::Diagnostics | SystemStatusDetail::Activity
            )
        );
    let rows_area = Rect::new(
        table_inner.x,
        table_inner.y,
        table_inner
            .width
            .saturating_sub(u16::from(detail_has_scroll) * 2),
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
    let mut picker_viewport_start = 0;
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
            let visible_height = area.height.saturating_sub(2) as usize;
            picker_viewport_start =
                crate::components::List::automatic_viewport_start(p.selected, visible_height);
            p.items
                .iter()
                .enumerate()
                .skip(picker_viewport_start)
                .take(visible_height)
                .enumerate()
                .map(|(visible_index, (index, _))| SystemStatusRowLayout {
                    index,
                    area: Rect::new(
                        area.x.saturating_add(1),
                        area.y.saturating_add(1 + visible_index as u16),
                        area.width.saturating_sub(2),
                        1,
                    ),
                })
                .collect()
        })
        .unwrap_or_default();
    let size_picker_items = model
        .dashboard
        .size_picker
        .map(|_| {
            let w = panel.width.min(42);
            let h = panel.height.min(5);
            let area = Rect::new(
                panel.x + (panel.width - w) / 2,
                panel.y + (panel.height - h) / 2,
                w,
                h,
            );
            (0..3)
                .map(|index| SystemStatusRowLayout {
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
    let dialog_actions = model
        .dashboard
        .dialog
        .as_ref()
        .map(|dialog_model| {
            let w = panel.width.min(48);
            let h = panel.height.min(8);
            let area = Rect::new(
                panel.x + (panel.width - w) / 2,
                panel.y + (panel.height - h) / 2,
                w,
                h,
            );
            let dialog = Dialog::new(
                "system-status.dialog",
                &dialog_model.title,
                &dialog_model.message,
                vec![
                    DialogAction::new(
                        "confirm",
                        if dialog_model.confirm_label.is_empty() {
                            "Confirm"
                        } else {
                            &dialog_model.confirm_label
                        },
                    ),
                    DialogAction::new(
                        "cancel",
                        if dialog_model.cancel_label.is_empty() {
                            "Cancel"
                        } else {
                            &dialog_model.cancel_label
                        },
                    ),
                ],
            );
            dialog
                .action_areas(area)
                .into_iter()
                .map(|(index, area)| SystemStatusRowLayout { index, area })
                .collect()
        })
        .unwrap_or_default();
    let activity_tabs_area = matches!(detail, Some(SystemStatusDetail::Activity))
        .then(|| Rect::new(canvas.x, canvas.y, canvas.width, canvas.height.min(1)));
    let activity_tabs = activity_tabs_area
        .map(|area| {
            let tabs = Tabs::new(
                "system-status.activity.tabs.geometry",
                vec![
                    TabItem::new("system-status.activity.logs", "Logs"),
                    TabItem::new("system-status.activity.incidents", "Incidents"),
                ],
            );
            [
                crate::DiagnosticsTab::Logs,
                crate::DiagnosticsTab::Incidents,
            ]
            .into_iter()
            .zip(tabs.borderless_item_areas(area))
            .map(|(tab, area)| SystemStatusActivityTabLayout { tab, area })
            .collect()
        })
        .unwrap_or_default();
    let diagnostics_area = activity_tabs_area.map_or(canvas, |tabs| {
        Rect::new(
            canvas.x,
            tabs.bottom(),
            canvas.width,
            canvas.bottom().saturating_sub(tabs.bottom()),
        )
    });
    let diagnostics_content = matches!(
        detail,
        Some(SystemStatusDetail::Diagnostics | SystemStatusDetail::Activity)
    )
    .then(|| {
        let mut diagnostics = model.diagnostics.clone();
        if matches!(detail, Some(SystemStatusDetail::Activity)) {
            diagnostics.tab = model.activity_tab();
        }
        diagnostics_content_layout(diagnostics_area, &diagnostics)
    });
    let diagnostics_repair_dialog = diagnostics_content.as_ref().and_then(|_| {
        model
            .diagnostics
            .repair_dialog
            .as_ref()
            .map(|d| diagnostics_repair_dialog_layout(main, d))
    });
    let scrollbar = match model.route {
        SystemStatusRoute::Dashboard => dashboard_scrollbar,
        SystemStatusRoute::Detail(
            SystemStatusDetail::Diagnostics | SystemStatusDetail::Activity,
        ) => None,
        SystemStatusRoute::Detail(_) => detail_has_scroll.then(|| {
            Rect::new(
                table_inner.right().saturating_sub(1),
                table_inner.y,
                1,
                table_inner.height,
            )
        }),
    };
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
        picker_viewport_start,
        size_picker_items,
        dialog_actions,
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
        detail_summary_area,
        detail_trend_area,
        activity_tabs_area,
        activity_tabs,
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
    if !l.dialog_actions.is_empty() {
        return l
            .dialog_actions
            .iter()
            .find(|action| rect_contains(action.area, x, y))
            .and_then(|action| match action.index {
                0 => Some(SystemStatusHitTarget::DialogConfirm),
                1 => Some(SystemStatusHitTarget::DialogCancel),
                _ => None,
            });
    }
    if !l.picker_items.is_empty() {
        return l
            .picker_items
            .iter()
            .find(|r| rect_contains(r.area, x, y))
            .map(|r| SystemStatusHitTarget::PickerItem(r.index));
    }
    if !l.size_picker_items.is_empty() {
        return l
            .size_picker_items
            .iter()
            .find(|r| rect_contains(r.area, x, y))
            .map(|r| SystemStatusHitTarget::SizePickerItem(r.index));
    }
    if let Some(d) = &l.diagnostics_repair_dialog {
        return diagnostics_repair_dialog_hit_test(d, p).map(SystemStatusHitTarget::Diagnostics);
    }
    if let Some(tab) = l
        .activity_tabs
        .iter()
        .find(|tab| rect_contains(tab.area, x, y))
    {
        return Some(SystemStatusHitTarget::Diagnostics(
            DiagnosticsHitTarget::Tab(tab.tab),
        ));
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
