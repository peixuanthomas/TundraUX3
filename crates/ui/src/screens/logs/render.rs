use super::layout::{category_tabs, controls, section_tabs};
use super::{LogsCategory, LogsSection, LogsViewModel, logs_layout};
use crate::components::{Button, Surface};
use crate::screens::diagnostics::render_diagnostics_content_titled;
use crate::screens::shell::{render_compact_home, render_status, render_top};
use crate::{
    DiagnosticsCheckViewModel, DiagnosticsStatus, DiagnosticsTab, DiagnosticsViewModel,
    RenderContext, ShellChromeViewModel, ShellLayout, compute_shell_layout,
};
use ratatui::{
    Frame,
    layout::Rect,
    widgets::{Paragraph, Wrap},
};

pub(super) fn unavailable_reason(model: &LogsViewModel) -> Option<String> {
    if model.category == LogsCategory::Linux {
        if !model.linux_available {
            return Some(i18n::tr!(
                "ui-logs-linux-log-is-unavailable-on-windows-and-macos-ux-log-remains-available"
            ));
        }
        if !model.can_view_system {
            return Some(i18n::tr!(
                "ui-logs-linux-log-requires-administrator-access-and-operating-system-log-permissions"
            ));
        }
    } else if !model.diagnostics.can_view_details {
        return Some(i18n::tr!(
            "ui-logs-sign-in-to-view-your-ux-log-guest-access-is-disabled"
        ));
    }
    None
}

/// Adapts event data to the existing list/details presentation, without I/O or new widgets.
pub(super) fn content_model(model: &LogsViewModel) -> DiagnosticsViewModel {
    let mut content = model.diagnostics.clone();
    content.repair_dialog = None;
    content.can_repair = false;
    if model.category == LogsCategory::Linux || model.section == LogsSection::Events {
        content.tab = DiagnosticsTab::Health;
        content.selected_check = model.selected_event;
        content.list_window_start = model.scroll_offset;
        content.checks = model
            .events
            .iter()
            .map(|event| DiagnosticsCheckViewModel {
                id: event.id.clone(),
                label: format!("{} {}", event.timestamp, event.operation),
                category: event.module.clone(),
                status: match event.level.to_ascii_lowercase().as_str() {
                    "error" | "critical" | "fatal" | "alert" | "emergency" => {
                        DiagnosticsStatus::Fail
                    }
                    "warning" | "warn" => DiagnosticsStatus::Warning,
                    _ => DiagnosticsStatus::Pass,
                },
                summary: event.summary.clone(),
                detail: i18n::tr!(
                    "ui-logs-event-detail",
                    time = event.timestamp.clone(),
                    level = event.level.clone(),
                    event = event.id.clone(),
                    operation = event.operation.clone(),
                    detail = event.detail.clone(),
                    incident = event
                        .incident_id
                        .as_ref()
                        .map(|id| i18n::tr!("ui-logs-incident-link", id = id.as_str()))
                        .unwrap_or_default()
                ),
                remediation: String::new(),
                repairable: false,
            })
            .collect();
    } else {
        content.tab = match model.section {
            LogsSection::Files => DiagnosticsTab::Logs,
            LogsSection::Incidents => DiagnosticsTab::Incidents,
            LogsSection::Events => unreachable!(),
        };
    }
    if unavailable_reason(model).is_some() {
        content.checks.clear();
        content.logs.clear();
        content.incidents.clear();
    }
    content
}

pub fn render_logs_with_context(
    frame: &mut Frame<'_>,
    area: Rect,
    chrome: &ShellChromeViewModel,
    model: &LogsViewModel,
    context: &RenderContext,
) {
    let theme = context.compatibility_theme();
    match compute_shell_layout(area) {
        ShellLayout::Compact(compact) => render_compact_home(frame, compact, chrome, &theme),
        ShellLayout::Full { top, main, status } => {
            render_top(frame, top, chrome, &theme);
            let layout = logs_layout(main, model);
            Surface::new()
                .titled(i18n::tr!("ui-logs-logs"))
                .bordered(false)
                .render_frame(frame, layout.panel, context);
            let mut categories = category_tabs();
            categories.set_selected(Some(usize::from(model.category == LogsCategory::Linux)));
            categories.render_borderless_frame(frame, layout.category_tabs_area, &theme);
            if model.category == LogsCategory::Ux {
                let mut sections = section_tabs();
                sections.set_selected(Some(match model.section {
                    LogsSection::Events => 0,
                    LogsSection::Files => 1,
                    LogsSection::Incidents => 2,
                }));
                sections.render_borderless_frame(frame, layout.section_tabs_area, &theme);
            }
            let unavailable = unavailable_reason(model);
            let content = content_model(model);
            for (control, (target, label)) in layout.controls.iter().zip(controls()) {
                let mut button = Button::new(format!("logs.{target:?}"), label);
                button.set_disabled(
                    unavailable.is_some()
                        || model.loading
                        || (target == super::LogsHitTarget::Open && content.item_count() == 0),
                );
                button.render_borderless_frame(frame, control.area, &theme);
            }
            frame.render_widget(
                Paragraph::new(if model.loading {
                    i18n::tr!("ui-logs-loading-logs")
                } else {
                    model.filter_summary.clone()
                })
                .style(theme.muted_style()),
                layout.filter_summary,
            );
            if let Some(reason) = unavailable {
                frame.render_widget(
                    Paragraph::new(reason)
                        .style(theme.muted_style())
                        .wrap(Wrap { trim: true }),
                    Rect::new(
                        layout.content.list_panel.x,
                        layout.content.list_panel.y,
                        layout
                            .content
                            .detail_panel
                            .right()
                            .saturating_sub(layout.content.list_panel.x),
                        layout.content.list_panel.height,
                    ),
                );
            } else {
                let title = if model.category == LogsCategory::Linux {
                    i18n::tr!("ui-logs-linux-events")
                } else {
                    match model.section {
                        LogsSection::Events => i18n::tr!("ui-logs-ux-events"),
                        LogsSection::Files => i18n::tr!("ui-logs-log-files"),
                        LogsSection::Incidents => i18n::tr!("ui-logs-incidents"),
                    }
                };
                render_diagnostics_content_titled(
                    frame,
                    &layout.content,
                    &content,
                    &theme,
                    context,
                    &title,
                    (model.category == LogsCategory::Linux || model.section == LogsSection::Events)
                        .then_some((
                            if model.loading {
                                i18n::tr!("ui-logs-loading-events")
                            } else {
                                i18n::tr!("ui-logs-no-events-match-the-current-query")
                            },
                            i18n::tr!("ui-logs-select-an-event-to-inspect-its-operation-and-correlation-identifiers"),
                        )),
                );
            }
            frame.render_widget(
                Paragraph::new(model.feedback.as_deref().unwrap_or(
                    &i18n::tr!("ui-logs-esc-back-category-tab-section-enter-o-open-read-only-r-refresh-i-e-link"),
                ))
                .style(theme.muted_style()),
                layout.footer,
            );
            render_status(frame, status, chrome, &theme);
        }
    }
}
