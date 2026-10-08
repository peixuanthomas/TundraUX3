use ratatui::layout::Rect;

use super::{LogsCategory, LogsSection, LogsViewModel};
use crate::components::{TabItem, Tabs};
use crate::screens::shell::{inset_rect, line_in_rect, rect_contains};
use crate::{DiagnosticsContentLayout, DiagnosticsHitTarget, diagnostics_content_layout};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogsHitTarget {
    Category(LogsCategory),
    Section(LogsSection),
    Event(usize),
    File(usize),
    Incident(usize),
    Refresh,
    Open,
    FilterLevel,
    FilterModule,
    FilterTime,
    ClearFilters,
    RelatedIncident,
    RelatedEvents,
    DetailScrollbar,
    Scrollbar,
    Follow,
    More,
    Filters,
    FormField(usize),
    FormApply,
    FormCancel,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LogsCategoryTabLayout {
    pub category: LogsCategory,
    pub area: Rect,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LogsSectionTabLayout {
    pub section: LogsSection,
    pub area: Rect,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LogsControlLayout {
    pub target: LogsHitTarget,
    pub area: Rect,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogsLayout {
    pub panel: Rect,
    pub category_tabs_area: Rect,
    pub category_tabs: Vec<LogsCategoryTabLayout>,
    pub section_tabs_area: Rect,
    pub section_tabs: Vec<LogsSectionTabLayout>,
    pub controls: Vec<LogsControlLayout>,
    pub filter_summary: Rect,
    pub content: DiagnosticsContentLayout,
    pub footer: Rect,
    pub visible_start: usize,
    pub visible_capacity: usize,
    pub detail_text: Rect,
    pub detail_scrollbar: Option<crate::ManagementScrollbar>,
    pub menu: Rect,
    pub menu_controls: Vec<LogsControlLayout>,
}

pub(super) fn category_tabs() -> Tabs {
    Tabs::new(
        "logs.categories",
        vec![
            TabItem::new("logs.ux", i18n::tr!("ui-logs-ux-log")),
            TabItem::new("logs.linux", i18n::tr!("ui-logs-linux-log")),
        ],
    )
}
pub(super) fn section_tabs() -> Tabs {
    Tabs::new(
        "logs.sections",
        vec![
            TabItem::new("logs.events", i18n::tr!("ui-logs-events")),
            TabItem::new("logs.files", i18n::tr!("ui-logs-files")),
            TabItem::new("logs.incidents", i18n::tr!("ui-logs-incidents")),
        ],
    )
}
pub(super) fn controls(model: &LogsViewModel) -> Vec<(LogsHitTarget, String)> {
    vec![
        (LogsHitTarget::Refresh, i18n::tr!("ui-logs-refresh")),
        (LogsHitTarget::Open, i18n::tr!("ui-logs-open")),
        (
            LogsHitTarget::Follow,
            if model.following {
                i18n::tr!("ui-logs-pause-scroll")
            } else {
                i18n::tr!("ui-logs-back-latest", count = model.new_events)
            },
        ),
        (LogsHitTarget::More, i18n::tr!("ui-logs-more")),
    ]
}
pub fn logs_more_controls() -> Vec<(LogsHitTarget, String)> {
    vec![
        (LogsHitTarget::Filters, i18n::tr!("ui-logs-source-filters")),
        (LogsHitTarget::FilterLevel, i18n::tr!("ui-logs-l-level")),
        (LogsHitTarget::FilterModule, i18n::tr!("ui-logs-m-module")),
        (LogsHitTarget::FilterTime, i18n::tr!("ui-logs-t-time")),
        (
            LogsHitTarget::ClearFilters,
            i18n::tr!("ui-logs-clear-filters"),
        ),
        (
            LogsHitTarget::RelatedIncident,
            i18n::tr!("ui-logs-show-incident"),
        ),
        (
            LogsHitTarget::RelatedEvents,
            i18n::tr!("ui-logs-show-events"),
        ),
    ]
}

/// Geometry is derived from the same Tabs and diagnostics composition used to render.
pub fn logs_layout(main: Rect, model: &LogsViewModel) -> LogsLayout {
    let inner = inset_rect(main, 1);
    let category_tabs_area = line_in_rect(inner, inner.y);
    let section_tabs_area = if model.category == LogsCategory::Ux {
        line_in_rect(inner, category_tabs_area.bottom())
    } else {
        Rect::new(inner.x, category_tabs_area.bottom(), inner.width, 0)
    };
    let toolbar = Rect::new(
        inner.x,
        section_tabs_area.bottom(),
        inner.width,
        inner.bottom().saturating_sub(section_tabs_area.bottom()),
    );
    let footer = line_in_rect(inner, inner.bottom().saturating_sub(1));
    let category_tabs = category_tabs()
        .borderless_item_areas(category_tabs_area)
        .into_iter()
        .zip([LogsCategory::Ux, LogsCategory::Linux])
        .map(|(area, category)| LogsCategoryTabLayout { area, category })
        .collect();
    let section_tabs = if model.category == LogsCategory::Ux {
        section_tabs()
            .borderless_item_areas(section_tabs_area)
            .into_iter()
            .zip([
                LogsSection::Events,
                LogsSection::Files,
                LogsSection::Incidents,
            ])
            .map(|(area, section)| LogsSectionTabLayout { area, section })
            .collect()
    } else {
        Vec::new()
    };
    let mut x = toolbar.x;
    let mut y = toolbar.y;
    let controls = controls(model)
        .into_iter()
        .map(|(target, label)| {
            let width = (unicode_width::UnicodeWidthStr::width(label.as_str()) as u16 + 2)
                .min(toolbar.width);
            if x != toolbar.x && x.saturating_add(width) > toolbar.right() {
                x = toolbar.x;
                y = y.saturating_add(1);
            }
            let area = Rect::new(
                x,
                y.min(toolbar.bottom()),
                width,
                u16::from(y < toolbar.bottom()),
            );
            x = x.saturating_add(width).saturating_add(1);
            LogsControlLayout { target, area }
        })
        .collect::<Vec<_>>();
    let summary_y = controls
        .iter()
        .map(|control| control.area.bottom())
        .max()
        .unwrap_or(toolbar.y);
    let filter_summary = line_in_rect(inner, summary_y);
    let content_area = Rect::new(
        inner.x,
        filter_summary.bottom(),
        inner.width,
        footer.y.saturating_sub(filter_summary.bottom()),
    );
    let content = diagnostics_content_layout(content_area, &super::render::content_model(model));
    let detail_inner = crate::components::Surface::new()
        .bordered(true)
        .inner(content.detail_panel);
    let detail_text = Rect::new(
        detail_inner.x,
        detail_inner.y,
        detail_inner.width.saturating_sub(1),
        detail_inner.height,
    );
    let detail_scrollbar = crate::management_scrollbar(
        crate::ManagementScrollTarget::Details,
        Rect::new(
            detail_inner.right().saturating_sub(1),
            detail_inner.y,
            u16::from(detail_inner.width > 0),
            detail_inner.height,
        ),
        crate::management_wrapped_lines(&super::render::logs_detail_text(model), detail_text.width)
            .len(),
        usize::from(detail_text.height),
        model.detail_scroll,
        false,
    );
    let menu_items = logs_more_controls();
    let menu_width = menu_items
        .iter()
        .map(|(_, label)| crate::components::terminal_width(label) as u16 + 4)
        .max()
        .unwrap_or(20)
        .min(main.width);
    let menu_height = (menu_items.len() as u16 + 2).min(main.height);
    let menu = Rect::new(
        main.right().saturating_sub(menu_width),
        main.y,
        menu_width,
        menu_height,
    );
    let menu_controls = if model.more_selected.is_some() {
        let capacity = usize::from(menu.height.saturating_sub(2));
        let start = model
            .more_selected
            .unwrap_or(0)
            .saturating_sub(capacity.saturating_sub(1));
        menu_items
            .into_iter()
            .skip(start)
            .take(capacity)
            .enumerate()
            .map(|(index, (target, _))| LogsControlLayout {
                target,
                area: Rect::new(
                    menu.x + 1,
                    menu.y + 1 + index as u16,
                    menu.width.saturating_sub(2),
                    1,
                ),
            })
            .collect()
    } else {
        vec![]
    };
    LogsLayout {
        panel: main,
        category_tabs_area,
        category_tabs,
        section_tabs_area,
        section_tabs,
        controls,
        filter_summary,
        footer,
        visible_start: content.visible_start,
        visible_capacity: content.visible_capacity,
        content,
        detail_text,
        detail_scrollbar,
        menu,
        menu_controls,
    }
}

pub fn logs_hit_test(
    main: Rect,
    model: &LogsViewModel,
    position: (u16, u16),
) -> Option<LogsHitTarget> {
    let layout = logs_layout(main, model);
    let (x, y) = position;
    if let Some(form) = &model.filter_form {
        let form_model = crate::ManagementViewModel {
            form: Some(form.clone()),
            ..Default::default()
        };
        let form_layout = crate::management_layout(main, &form_model);
        if rect_contains(form_layout.submit, x, y) {
            return Some(LogsHitTarget::FormApply);
        }
        if rect_contains(form_layout.cancel, x, y) {
            return Some(LogsHitTarget::FormCancel);
        }
        return form_layout
            .fields
            .iter()
            .find(|(_, area)| rect_contains(*area, x, y))
            .map(|(index, _)| LogsHitTarget::FormField(*index));
    }
    if model.more_selected.is_some() {
        return layout
            .menu_controls
            .iter()
            .find(|control| {
                rect_contains(control.area, x, y)
                    && super::render::logs_control_enabled(model, control.target)
            })
            .map(|control| control.target);
    }
    if let Some(tab) = layout
        .category_tabs
        .iter()
        .find(|tab| rect_contains(tab.area, x, y))
    {
        return Some(LogsHitTarget::Category(tab.category));
    }
    if let Some(tab) = layout
        .section_tabs
        .iter()
        .find(|tab| rect_contains(tab.area, x, y))
    {
        return Some(LogsHitTarget::Section(tab.section));
    }
    if let Some(control) = layout
        .controls
        .iter()
        .find(|control| rect_contains(control.area, x, y))
    {
        if !super::render::logs_control_enabled(model, control.target) {
            return None;
        }
        return Some(control.target);
    }
    if super::render::unavailable_reason(model).is_some() {
        return None;
    }
    if layout
        .detail_scrollbar
        .is_some_and(|bar| rect_contains(bar.track, x, y))
    {
        return Some(LogsHitTarget::DetailScrollbar);
    }
    layout
        .content
        .hit_test(x, y)
        .and_then(|target| match target {
            DiagnosticsHitTarget::Check(index) => Some(LogsHitTarget::Event(index)),
            DiagnosticsHitTarget::Log(index) => Some(LogsHitTarget::File(index)),
            DiagnosticsHitTarget::Incident(index) => Some(LogsHitTarget::Incident(index)),
            DiagnosticsHitTarget::Scrollbar => Some(LogsHitTarget::Scrollbar),
            _ => None,
        })
}
