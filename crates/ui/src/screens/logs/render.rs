use super::layout::controls;
use super::{LogsCategory, LogsHitTarget, LogsSection, LogsViewModel, logs_layout};
use crate::components::{Button, Surface};
use crate::screens::diagnostics::render_diagnostics_content_titled;
use crate::{
    DiagnosticsCheckViewModel, DiagnosticsStatus, DiagnosticsTab, DiagnosticsViewModel,
    RenderContext,
};
use ratatui::{
    Frame,
    layout::Rect,
    widgets::{Clear, Paragraph, Wrap},
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

pub fn logs_control_enabled(model: &LogsViewModel, target: LogsHitTarget) -> bool {
    if unavailable_reason(model).is_some() || model.loading {
        return false;
    }
    match target {
        LogsHitTarget::Open => content_model(model).item_count() > 0,
        LogsHitTarget::RelatedIncident => {
            model.category == LogsCategory::Ux
                && model.section == LogsSection::Events
                && model
                    .events
                    .get(model.selected_event)
                    .is_some_and(|event| event.incident_id.is_some())
        }
        LogsHitTarget::RelatedEvents => {
            model.category == LogsCategory::Ux
                && model.section == LogsSection::Incidents
                && !model.diagnostics.incidents.is_empty()
        }
        _ => true,
    }
}

pub fn logs_control_id(model: &LogsViewModel, target: LogsHitTarget) -> String {
    let selection = if model.category == LogsCategory::Linux || model.section == LogsSection::Events
    {
        model
            .events
            .get(model.selected_event)
            .map(|event| event.id.as_str())
    } else {
        match model.section {
            LogsSection::Files => model
                .diagnostics
                .selected_log()
                .map(|log| log.path.as_str()),
            LogsSection::Incidents => model
                .diagnostics
                .selected_incident()
                .map(|incident| incident.id.as_str()),
            LogsSection::Events => None,
        }
    };
    format!(
        "logs.{target:?}.{:?}.{:?}.{selection:?}",
        model.category, model.section
    )
}

pub fn logs_detail_text(model: &LogsViewModel) -> String {
    if let Some(reason) = unavailable_reason(model) {
        return reason;
    }
    if model.category == LogsCategory::Linux || model.section == LogsSection::Events {
        return model
            .events
            .get(model.selected_event)
            .map(|event| {
                format!(
                    "{} {}\n{}\n{}\n{}",
                    event.timestamp,
                    event.operation,
                    event.module,
                    event.summary,
                    i18n::tr!(
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
                    )
                )
            })
            .unwrap_or_else(|| {
                i18n::tr!(
                    "ui-logs-select-an-event-to-inspect-its-operation-and-correlation-identifiers"
                )
            });
    }
    if model.section == LogsSection::Files {
        return model
            .diagnostics
            .selected_log()
            .map(|log| {
                format!(
                    "{}\n{}\n{}\n{}",
                    log.relative_path,
                    i18n::tr!(
                        "ui-diagnostics-modified",
                        modified = log.modified_at.clone()
                    ),
                    i18n::tr!("ui-diagnostics-size-bytes", size = log.size_bytes),
                    i18n::tr!("ui-diagnostics-path", path = log.path.clone())
                )
            })
            .unwrap_or_else(|| i18n::tr!("ui-diagnostics-no-log-selected"));
    }
    model
        .diagnostics
        .selected_incident()
        .map(|incident| {
            if incident.restricted {
                format!(
                    "{}\n{}\n{}",
                    incident.app,
                    incident.occurred_at,
                    i18n::tr!(
                        "ui-diagnostics-details-and-report-path-are-restricted-to-administrators"
                    )
                )
            } else {
                format!(
                    "{}\n{}\n{}\n{}\n{}\n{}",
                    incident.id,
                    incident.app,
                    incident.occurred_at,
                    incident.recovery,
                    incident.summary,
                    incident.detail
                )
            }
        })
        .unwrap_or_else(|| i18n::tr!("ui-diagnostics-no-incident-selected"))
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

pub fn render_logs_content(
    frame: &mut Frame<'_>,
    main: Rect,
    model: &LogsViewModel,
    context: &RenderContext,
) {
    let theme = context.compatibility_theme();

    let layout = logs_layout(main, model);
    Surface::new()
        .titled(i18n::tr!("ui-logs-logs"))
        .bordered(false)
        .render_frame(frame, layout.panel, context);
    for tab in &layout.category_tabs {
        let label = if tab.category == LogsCategory::Ux {
            i18n::tr!("ui-logs-ux-log")
        } else {
            i18n::tr!("ui-logs-linux-log")
        };
        let mut button = Button::new(format!("logs.category.{:?}", tab.category), label);
        button.state.selected = tab.category == model.category;
        button.render_borderless_frame(frame, tab.area, &theme);
    }
    if model.category == LogsCategory::Ux {
        for tab in &layout.section_tabs {
            let label = match tab.section {
                LogsSection::Events => i18n::tr!("ui-logs-events"),
                LogsSection::Files => i18n::tr!("ui-logs-files"),
                LogsSection::Incidents => i18n::tr!("ui-logs-incidents"),
            };
            let mut button = Button::new(format!("logs.section.{:?}", tab.section), label);
            button.state.selected = tab.section == model.section;
            button.render_borderless_frame(frame, tab.area, &theme);
        }
    }
    let unavailable = unavailable_reason(model);
    let content = content_model(model);
    for (control, (target, label)) in layout.controls.iter().zip(controls()) {
        let mut button = Button::new(logs_control_id(model, target), label);
        button.set_disabled(!logs_control_enabled(model, target));
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
        frame.render_widget(Clear, layout.detail_text);
        Surface::new().render_frame(frame, layout.detail_text, context);
        let lines =
            crate::management_wrapped_lines(&logs_detail_text(model), layout.detail_text.width);
        let start = model.detail_scroll.min(
            lines
                .len()
                .saturating_sub(usize::from(layout.detail_text.height)),
        );
        frame.render_widget(
            Paragraph::new(
                lines
                    .into_iter()
                    .skip(start)
                    .map(ratatui::text::Line::from)
                    .collect::<Vec<_>>(),
            )
            .style(theme.body_style()),
            layout.detail_text,
        );
        if let Some(bar) = layout.detail_scrollbar {
            bar.render(frame, context);
        }
    }
    frame.render_widget(
        Paragraph::new(model.feedback.as_deref().unwrap_or(&i18n::tr!(
            "ui-logs-esc-back-category-tab-section-enter-o-open-read-only-r-refresh-i-e-link"
        )))
        .style(theme.muted_style()),
        layout.footer,
    );
}
