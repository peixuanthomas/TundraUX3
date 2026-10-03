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
pub(super) fn controls() -> Vec<(LogsHitTarget, String)> {
    vec![
        (LogsHitTarget::Refresh, i18n::tr!("ui-logs-r-refresh")),
        (LogsHitTarget::Open, i18n::tr!("ui-logs-o-open")),
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
    let controls = controls()
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
    }
}

pub fn logs_hit_test(
    main: Rect,
    model: &LogsViewModel,
    position: (u16, u16),
) -> Option<LogsHitTarget> {
    let layout = logs_layout(main, model);
    let (x, y) = position;
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
        if !super::render::control_enabled(model, control.target) {
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
