use ratatui::layout::Rect;

use super::model::{SystemStatusTab, SystemStatusViewModel};
use crate::screens::shell::{inset_rect, line_in_rect, rect_contains, usize_to_u16};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SystemStatusTabLayout {
    pub tab: SystemStatusTab,
    pub area: Rect,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SystemStatusRowLayout {
    pub index: usize,
    pub area: Rect,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SystemStatusHitTarget {
    Tab(SystemStatusTab),
    Row(usize),
    Refresh,
    Scrollbar,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SystemStatusLayout {
    pub panel: Rect,
    pub header: Rect,
    pub tabs_area: Rect,
    pub tabs: Vec<SystemStatusTabLayout>,
    pub content_panel: Rect,
    pub rows_area: Rect,
    pub footer: Rect,
    pub refresh_button: Rect,
    pub scrollbar: Option<Rect>,
    pub rows: Vec<SystemStatusRowLayout>,
    pub visible_start: usize,
    pub visible_capacity: usize,
}

pub fn system_status_layout(main: Rect, model: &SystemStatusViewModel) -> SystemStatusLayout {
    let panel = main;
    let inner = inset_rect(panel, 1);
    let header = line_in_rect(inner, inner.y);
    let tabs_area = if model.is_admin() {
        line_in_rect(inner, header.bottom())
    } else {
        Rect::new(inner.x, header.bottom(), inner.width, 0)
    };
    let footer = line_in_rect(inner, inner.bottom().saturating_sub(1));
    let content_y = tabs_area.bottom();
    let content_panel = Rect::new(
        inner.x,
        content_y,
        inner.width,
        footer.y.saturating_sub(content_y),
    );
    let table_inner = inset_rect(content_panel, 1);
    let item_count = model.item_count();
    let table_header = u16::from(item_count > 0);
    let visible_capacity = usize::from(table_inner.height.saturating_sub(table_header));
    let max_start = item_count.saturating_sub(visible_capacity);
    let mut visible_start = model.scroll_offset.min(max_start);
    if let Some(selected) = model.selected_index() {
        if selected < visible_start {
            visible_start = selected;
        } else if visible_capacity > 0 && selected >= visible_start + visible_capacity {
            visible_start = selected + 1 - visible_capacity;
        }
    }
    let scrollbar = (item_count > visible_capacity
        && table_inner.width >= 3
        && visible_capacity > 0)
        .then(|| {
            Rect::new(
                table_inner.right().saturating_sub(1),
                table_inner.y,
                1,
                table_inner.height,
            )
        });
    let rows_area = Rect::new(
        table_inner.x,
        table_inner.y,
        table_inner
            .width
            .saturating_sub(if scrollbar.is_some() { 2 } else { 0 }),
        table_inner.height,
    );
    let rows = (visible_start..item_count)
        .take(visible_capacity)
        .enumerate()
        .map(|(offset, index)| SystemStatusRowLayout {
            index,
            area: Rect::new(
                rows_area.x,
                rows_area
                    .y
                    .saturating_add(table_header)
                    .saturating_add(usize_to_u16(offset)),
                rows_area.width,
                1,
            ),
        })
        .collect();
    let refresh_width = 11.min(footer.width);
    let refresh_button = Rect::new(
        footer.right().saturating_sub(refresh_width),
        footer.y,
        refresh_width,
        footer.height,
    );
    let mut tab_x = tabs_area.x;
    let tabs = if model.is_admin() {
        SystemStatusTab::ALL
            .into_iter()
            .map(|tab| {
                let width = usize_to_u16(tab.label().chars().count())
                    .saturating_add(4)
                    .min(tabs_area.right().saturating_sub(tab_x));
                let area = Rect::new(tab_x, tabs_area.y, width, tabs_area.height);
                tab_x = tab_x.saturating_add(width);
                SystemStatusTabLayout { tab, area }
            })
            .collect()
    } else {
        Vec::new()
    };

    SystemStatusLayout {
        panel,
        header,
        tabs_area,
        tabs,
        content_panel,
        rows_area,
        footer,
        refresh_button,
        scrollbar,
        rows,
        visible_start,
        visible_capacity,
    }
}

pub fn system_status_hit_test(
    layout: &SystemStatusLayout,
    coordinates: (u16, u16),
) -> Option<SystemStatusHitTarget> {
    let (x, y) = coordinates;
    if let Some(tab) = layout.tabs.iter().find(|tab| rect_contains(tab.area, x, y)) {
        return Some(SystemStatusHitTarget::Tab(tab.tab));
    }
    if rect_contains(layout.refresh_button, x, y) {
        return Some(SystemStatusHitTarget::Refresh);
    }
    if layout
        .scrollbar
        .is_some_and(|area| rect_contains(area, x, y))
    {
        return Some(SystemStatusHitTarget::Scrollbar);
    }
    layout
        .rows
        .iter()
        .find(|row| rect_contains(row.area, x, y))
        .map(|row| SystemStatusHitTarget::Row(row.index))
}
