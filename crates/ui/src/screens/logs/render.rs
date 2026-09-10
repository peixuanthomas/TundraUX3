use super::layout::{CONTROLS, category_tabs, section_tabs};
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

pub(super) fn unavailable_reason(model: &LogsViewModel) -> Option<&'static str> {
    if model.category == LogsCategory::Linux {
        if !model.linux_available {
            return Some(
                "Linux log is unavailable on Windows and macOS. UX log remains available.",
            );
        }
        if !model.can_view_system {
            return Some(
                "Linux log requires administrator access and operating system log permissions.",
            );
        }
    } else if !model.diagnostics.can_view_details {
        return Some("Sign in to view your UX log. Guest access is disabled.");
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
                detail: format!(
                    "Time: {}\nLevel: {}\nEvent: {}\nOperation: {}\n{}{}",
                    event.timestamp,
                    event.level,
                    event.id,
                    event.operation,
                    event.detail,
                    event
                        .incident_id
                        .as_ref()
                        .map(|id| format!("\nIncident: {id}"))
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
            Surface::new().titled("Logs").bordered(false).render_frame(
                frame,
                layout.panel,
                context,
            );
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
            for (control, (target, label)) in layout.controls.iter().zip(CONTROLS) {
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
                    "Loading logs..."
                } else {
                    &model.filter_summary
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
                    "Linux events"
                } else {
                    match model.section {
                        LogsSection::Events => "UX events",
                        LogsSection::Files => "Log files",
                        LogsSection::Incidents => "Incidents",
                    }
                };
                render_diagnostics_content_titled(
                    frame,
                    &layout.content,
                    &content,
                    &theme,
                    context,
                    title,
                    (model.category == LogsCategory::Linux || model.section == LogsSection::Events)
                        .then_some((
                            if model.loading {
                                "Loading events..."
                            } else {
                                "No events match the current query"
                            },
                            "Select an event to inspect its operation and correlation identifiers.",
                        )),
                );
            }
            frame.render_widget(
                Paragraph::new(model.feedback.as_deref().unwrap_or(
                    "Esc Back · ←/→ Category · Tab Section · Enter/O Open read-only · R Refresh · I/E Link",
                ))
                .style(theme.muted_style()),
                layout.footer,
            );
            render_status(frame, status, chrome, &theme);
        }
    }
}
