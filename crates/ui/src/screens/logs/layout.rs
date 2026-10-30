use ratatui::layout::Rect;

use super::{LogsCategory, LogsViewModel};
use crate::components::{
    ComponentTone, PageAction, PageActionBar, PageActionStatus, TabItem, Tabs,
};
use crate::screens::shell::{centered_rect, inset_rect, line_in_rect, rect_contains};
use crate::{DiagnosticsContentLayout, DiagnosticsHitTarget, diagnostics_content_layout};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogsHitTarget {
    Category(LogsCategory),
    Event(usize),
    Refresh,
    Open,
    OpenFolder,
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
    CloseMore,
    MoreScrollbar,
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
pub struct LogsControlLayout {
    pub target: LogsHitTarget,
    pub area: Rect,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogsLayout {
    pub panel: Rect,
    pub category_tabs_area: Rect,
    pub category_tabs: Vec<LogsCategoryTabLayout>,
    pub controls: Vec<LogsControlLayout>,
    pub action_bar: crate::components::PageActionBarLayout,
    pub filter_summary: Rect,
    pub content: DiagnosticsContentLayout,
    pub footer: Rect,
    pub visible_start: usize,
    pub visible_capacity: usize,
    pub detail_text: Rect,
    pub detail_scrollbar: Option<crate::ManagementScrollbar>,
    pub menu: Rect,
    pub menu_controls: Vec<LogsControlLayout>,
    pub menu_scrollbar: Option<crate::ManagementScrollbar>,
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
pub(super) fn controls(model: &LogsViewModel) -> Vec<(LogsHitTarget, String)> {
    let mut controls = vec![
        (LogsHitTarget::Refresh, i18n::tr!("ui-logs-refresh")),
        (
            LogsHitTarget::FilterLevel,
            i18n::tr!("ui-logs-level-filter", level = model.level_filter.as_str()),
        ),
        (LogsHitTarget::Open, i18n::tr!("ui-logs-open")),
        (
            LogsHitTarget::Follow,
            if model.following {
                i18n::tr!("ui-logs-pause-scroll")
            } else {
                i18n::tr!("ui-logs-back-latest", count = model.new_events)
            },
        ),
        (LogsHitTarget::Filters, i18n::tr!("ui-logs-source-filters")),
        (LogsHitTarget::More, i18n::tr!("ui-logs-more")),
    ];
    if model.category == LogsCategory::Ux {
        controls.push((LogsHitTarget::OpenFolder, i18n::tr!("ui-logs-open-folder")));
    }
    controls
}
pub fn logs_more_controls() -> Vec<(LogsHitTarget, String)> {
    vec![
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
        (LogsHitTarget::CloseMore, i18n::tr!("management-form-close")),
    ]
}

/// Geometry is derived from the same Tabs and diagnostics composition used to render.
pub fn logs_layout(main: Rect, model: &LogsViewModel) -> LogsLayout {
    let inner = inset_rect(main, 1);
    let bar = logs_action_bar(model);
    let bar_height = bar.height(inner.width).min(inner.height);
    let footer = Rect::new(
        inner.x,
        inner.bottom().saturating_sub(bar_height),
        inner.width,
        bar_height,
    );
    let refresh_label = i18n::tr!("ui-logs-refresh");
    let refresh_width =
        (crate::components::terminal_width(&refresh_label) as u16 + 2).min(inner.width);
    let refresh = Rect::new(inner.x, inner.y, refresh_width, u16::from(inner.height > 0));
    let level_label = i18n::tr!("ui-logs-level-filter", level = model.level_filter.as_str());
    let level = Rect::new(
        refresh.right(),
        inner.y,
        (crate::components::terminal_width(&level_label) as u16 + 2)
            .min(inner.width.saturating_sub(refresh.width)),
        refresh.height,
    );
    let filter_summary = Rect::new(
        level.right().min(inner.right()),
        inner.y,
        inner.width.saturating_sub(refresh.width + level.width),
        refresh.height,
    );
    let content_area = Rect::new(
        inner.x,
        refresh.bottom(),
        inner.width,
        footer.y.saturating_sub(refresh.bottom()),
    );
    let mut content =
        diagnostics_content_layout(content_area, &super::render::logs_content_model(model));
    let right = content.detail_panel;
    let action_height = 1.min(right.height);
    let actions = Rect::new(
        right.x,
        right.bottom().saturating_sub(action_height),
        right.width,
        action_height,
    );
    content.detail_panel.height = content.detail_panel.height.saturating_sub(action_height);
    let category_tabs_area = line_in_rect(actions, actions.y);
    let category_tabs = category_tabs()
        .borderless_item_areas(category_tabs_area)
        .into_iter()
        .zip([LogsCategory::Ux, LogsCategory::Linux])
        .map(|(area, category)| LogsCategoryTabLayout { area, category })
        .collect();
    let mut controls = vec![
        LogsControlLayout {
            target: LogsHitTarget::Refresh,
            area: refresh,
        },
        LogsControlLayout {
            target: LogsHitTarget::FilterLevel,
            area: level,
        },
    ];
    let action_bar = bar.layout(footer);
    controls.extend(action_bar.actions.iter().filter_map(|action| {
        crate::screens::logs::layout::controls(model)
            .into_iter()
            .find(|(target, _)| logs_action_id(model, *target) == action.id.as_str())
            .map(|(target, _)| LogsControlLayout {
                target,
                area: action.area,
            })
    }));
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
    let menu_bounds = if main.width >= 12 && main.height >= 8 {
        inset_rect(main, 1)
    } else {
        main
    };
    let menu_width = menu_items
        .iter()
        .map(|(_, label)| crate::components::terminal_width(label) as u16 + 9)
        .max()
        .unwrap_or(40)
        .max(40)
        .min(menu_bounds.width);
    let menu_height = (menu_items.len() as u16 + 5).min(menu_bounds.height);
    let menu = centered_rect(menu_bounds, menu_width, menu_height);
    let menu_inner = crate::components::Surface::new().bordered(true).inner(menu);
    let horizontal_padding = u16::from(menu_inner.width >= 8) * 2;
    let vertical_padding = u16::from(menu_inner.height >= menu_items.len() as u16 + 3);
    let menu_body = Rect::new(
        menu_inner.x + horizontal_padding,
        menu_inner.y + vertical_padding,
        menu_inner.width.saturating_sub(horizontal_padding * 2),
        menu_inner.height.saturating_sub(vertical_padding * 2),
    );
    let close = line_in_rect(menu_body, menu_body.bottom().saturating_sub(1));
    let gap = u16::from(menu_body.height >= menu_items.len() as u16 + 1);
    let capacity = usize::from(menu_body.height.saturating_sub(1 + gap));
    let option_count = menu_items.len().saturating_sub(1);
    let start = model
        .more_selected
        .unwrap_or(0)
        .min(option_count.saturating_sub(1))
        .saturating_sub(capacity.saturating_sub(1))
        .min(option_count.saturating_sub(capacity));
    let menu_scrollbar = if model.more_selected.is_some() {
        crate::management_scrollbar(
            crate::ManagementScrollTarget::Choices,
            Rect::new(
                menu_body.right().saturating_sub(1),
                menu_body.y,
                u16::from(menu_body.width > 0),
                capacity as u16,
            ),
            option_count,
            capacity,
            start,
            false,
        )
    } else {
        None
    };
    let menu_controls = if model.more_selected.is_some() {
        menu_items
            .into_iter()
            .take(option_count)
            .skip(start)
            .take(capacity)
            .enumerate()
            .map(|(index, (target, _))| LogsControlLayout {
                target,
                area: Rect::new(
                    menu_body.x,
                    menu_body.y + index as u16,
                    menu_body
                        .width
                        .saturating_sub(u16::from(menu_scrollbar.is_some())),
                    1,
                ),
            })
            .chain((!close.is_empty()).then_some(LogsControlLayout {
                target: LogsHitTarget::CloseMore,
                area: close,
            }))
            .collect()
    } else {
        vec![]
    };
    LogsLayout {
        panel: main,
        category_tabs_area,
        category_tabs,
        controls,
        action_bar,
        filter_summary,
        footer,
        visible_start: content.visible_start,
        visible_capacity: content.visible_capacity,
        content,
        detail_text,
        detail_scrollbar,
        menu,
        menu_controls,
        menu_scrollbar,
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
        if layout
            .menu_scrollbar
            .is_some_and(|bar| rect_contains(bar.track, x, y))
        {
            return Some(LogsHitTarget::MoreScrollbar);
        }
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
            DiagnosticsHitTarget::Scrollbar => Some(LogsHitTarget::Scrollbar),
            _ => None,
        })
}

fn logs_action_id(model: &LogsViewModel, target: LogsHitTarget) -> String {
    super::render::logs_control_id(model, target)
        .trim_start_matches("logs.")
        .to_owned()
}

pub(super) fn logs_action_bar(model: &LogsViewModel) -> PageActionBar {
    let primary = if super::render::logs_control_enabled(model, LogsHitTarget::Open) {
        LogsHitTarget::Open
    } else {
        LogsHitTarget::Follow
    };
    let mut labels = controls(model)
        .into_iter()
        .filter(|(target, _)| {
            !matches!(target, LogsHitTarget::Refresh | LogsHitTarget::FilterLevel)
        })
        .collect::<Vec<_>>();
    labels.sort_by_key(|(target, _)| *target != primary);
    let actions = labels
        .into_iter()
        .map(|(target, label)| {
            let action = PageAction::new(logs_action_id(model, target), label)
                .disabled(!super::render::logs_control_enabled(model, target))
                .latched(target == LogsHitTarget::Follow && model.following);
            if target == primary {
                action.primary()
            } else {
                action
            }
        })
        .collect();
    let status = model
        .feedback
        .iter()
        .map(|text| PageActionStatus {
            text: text.clone(),
            tone: ComponentTone::Muted,
        })
        .collect();
    PageActionBar::new("logs")
        .with_actions(actions)
        .with_status(status)
}
