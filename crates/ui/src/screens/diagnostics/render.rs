use ratatui::Frame;
use ratatui::layout::{HorizontalAlignment, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::{Clear, Paragraph, Wrap};

use super::layout::{DiagnosticsLayout, DiagnosticsRepairDialogLayout, diagnostics_layout};
use super::model::{
    DiagnosticsCheckViewModel, DiagnosticsIncidentViewModel, DiagnosticsRepairDialogViewModel,
    DiagnosticsStatus, DiagnosticsTab, DiagnosticsViewModel,
};
use crate::components::{Button, ComponentTone, List, ListItem, Scrollbar, Surface, TabItem, Tabs};
use crate::screens::clock::render_clock_line;
use crate::screens::shell::{fit_cell, render_compact_home, render_status, render_top};
use crate::{RenderContext, ShellChromeViewModel, ShellLayout, TundraTheme, compute_shell_layout};
pub fn render_diagnostics(
    frame: &mut Frame<'_>,
    area: Rect,
    chrome: &ShellChromeViewModel,
    model: &DiagnosticsViewModel,
    theme: &TundraTheme,
) {
    let context = RenderContext::from_theme(theme, Default::default(), Default::default());
    render_diagnostics_contextual(frame, area, chrome, model, &context);
}

pub fn render_diagnostics_contextual(
    frame: &mut Frame<'_>,
    area: Rect,
    chrome: &ShellChromeViewModel,
    model: &DiagnosticsViewModel,
    context: &RenderContext,
) {
    let theme = &context.compatibility_theme();
    match compute_shell_layout(area) {
        ShellLayout::Compact(compact) => render_compact_home(frame, compact, chrome, theme),
        ShellLayout::Full { top, main, status } => {
            render_top(frame, top, chrome, theme);
            render_diagnostics_main(frame, main, model, theme, context);
            render_status(frame, status, chrome, theme);
        }
    }
}

fn render_diagnostics_main(
    frame: &mut Frame<'_>,
    main: Rect,
    model: &DiagnosticsViewModel,
    theme: &TundraTheme,
    context: &RenderContext,
) {
    let layout = diagnostics_layout(main, model);
    Surface::new()
        .titled("System Status / Diagnostics")
        .bordered(true)
        .render_frame(frame, layout.panel, context);

    render_diagnostics_header(frame, &layout, model, theme);
    render_diagnostics_tabs(frame, &layout, model, context);
    Surface::new()
        .titled(match model.tab {
            DiagnosticsTab::Health => "Checks",
            DiagnosticsTab::Logs => "Logs",
            DiagnosticsTab::Incidents => "Incidents",
        })
        .bordered(true)
        .render_frame(frame, layout.list_panel, context);
    Surface::new()
        .titled("Details")
        .bordered(true)
        .render_frame(frame, layout.detail_panel, context);
    render_diagnostics_rows(frame, &layout, model, theme, context);
    render_diagnostics_detail(frame, &layout, model, theme);
    render_diagnostics_footer(frame, &layout, model, theme);

    if let (Some(dialog_layout), Some(dialog)) =
        (layout.repair_dialog.as_ref(), model.repair_dialog.as_ref())
    {
        render_diagnostics_repair_dialog(frame, dialog_layout, dialog, theme, context);
    }
}

fn render_diagnostics_header(
    frame: &mut Frame<'_>,
    layout: &DiagnosticsLayout,
    model: &DiagnosticsViewModel,
    theme: &TundraTheme,
) {
    let warning_count = model
        .checks
        .iter()
        .filter(|check| check.status == DiagnosticsStatus::Warning)
        .count();
    let unsupported_count = model
        .checks
        .iter()
        .filter(|check| check.status == DiagnosticsStatus::Unsupported)
        .count();
    let failure_count = model
        .checks
        .iter()
        .filter(|check| check.status == DiagnosticsStatus::Fail)
        .count();
    let (state, style) = if model.restart_required {
        (
            "Restart required".to_string(),
            diagnostics_warning_style(theme),
        )
    } else if model.scanning {
        ("Scanning health checks...".to_string(), theme.title_style())
    } else if failure_count > 0 {
        (
            format!(
                "System needs attention — {warning_count} warning{} / {failure_count} failure{}",
                if warning_count == 1 { "" } else { "s" },
                if failure_count == 1 { "" } else { "s" },
            ),
            theme.error_style(),
        )
    } else if warning_count > 0 {
        (
            format!(
                "System needs attention — {warning_count} warning{}",
                if warning_count == 1 { "" } else { "s" },
            ),
            diagnostics_warning_style(theme),
        )
    } else if unsupported_count > 0 {
        (
            format!(
                "System healthy — {unsupported_count} unsupported {}",
                if unsupported_count == 1 {
                    "capability"
                } else {
                    "capabilities"
                },
            ),
            theme.muted_style(),
        )
    } else if model.checks.is_empty() {
        (
            "No health checks available".to_string(),
            theme.muted_style(),
        )
    } else {
        ("System healthy".to_string(), theme.title_style())
    };
    let scanned_at = model.scanned_at.as_deref().unwrap_or("not yet scanned");
    render_clock_line(
        frame,
        layout.header,
        fit_cell(
            &format!("{state}    Last scan: {scanned_at}"),
            usize::from(layout.header.width),
        ),
        style,
        HorizontalAlignment::Left,
    );
}

fn render_diagnostics_tabs(
    frame: &mut Frame<'_>,
    layout: &DiagnosticsLayout,
    model: &DiagnosticsViewModel,
    context: &RenderContext,
) {
    let items = DiagnosticsTab::ALL
        .into_iter()
        .map(|tab| {
            TabItem::new(
                format!("diagnostics.tab.{}", tab.label().to_ascii_lowercase()),
                format!("[{}]", tab.label()),
            )
        })
        .collect();
    let mut tabs = Tabs::new("diagnostics.tabs", items);
    tabs.set_selected(DiagnosticsTab::ALL.iter().position(|tab| *tab == model.tab));

    tabs.render_borderless_frame(frame, layout.tabs_area, &context.compatibility_theme());
}

fn render_diagnostics_rows(
    frame: &mut Frame<'_>,
    layout: &DiagnosticsLayout,
    model: &DiagnosticsViewModel,
    theme: &TundraTheme,
    context: &RenderContext,
) {
    if model.item_count() == 0 {
        let text = if model.scanning && model.tab == DiagnosticsTab::Health {
            "  Scanning..."
        } else {
            match model.tab {
                DiagnosticsTab::Health => "  No checks available",
                DiagnosticsTab::Logs => {
                    if model.can_view_details {
                        "  No logs found"
                    } else {
                        "  Logs are restricted to administrators"
                    }
                }
                DiagnosticsTab::Incidents => "  No incidents recorded",
            }
        };
        render_clock_line(
            frame,
            Rect::new(
                layout.list_rows_area.x,
                layout.list_rows_area.y,
                layout.list_rows_area.width,
                u16::from(layout.list_rows_area.height > 0),
            ),
            text.to_string(),
            theme.muted_style(),
            HorizontalAlignment::Left,
        );
        return;
    }

    let items = (0..model.item_count())
        .filter_map(|index| {
            let (text, status) = match model.tab {
                DiagnosticsTab::Health => {
                    let check = model.checks.get(index)?;
                    (
                        format!(
                            " {} [{}] {}",
                            check.status.marker(),
                            check.category,
                            check.label,
                        ),
                        check.status,
                    )
                }
                DiagnosticsTab::Incidents => {
                    let incident = model.incidents.get(index)?;
                    (
                        format!(
                            " {} {} — {}",
                            incident.severity.marker(),
                            incident.occurred_at,
                            incident.app,
                        ),
                        incident.severity,
                    )
                }
                DiagnosticsTab::Logs => {
                    let log = model.logs.get(index)?;
                    (
                        format!(
                            " {}  {}  {} bytes",
                            log.relative_path, log.modified_at, log.size_bytes,
                        ),
                        DiagnosticsStatus::Pass,
                    )
                }
            };
            Some(
                ListItem::new(
                    format!("diagnostics.row.{index}"),
                    fit_cell(
                        &text,
                        usize::from(layout.list_rows_area.width.saturating_sub(1)),
                    ),
                )
                .tone(diagnostics_status_tone(status)),
            )
        })
        .collect::<Vec<_>>();
    let mut list = List::new("diagnostics.rows", items).with_viewport_start(layout.visible_start);
    list.set_selected(Some(model.selected_index()));
    list.set_focused(true);
    list.render_borderless_frame(frame, layout.list_rows_area, theme);

    render_diagnostics_scrollbar(frame, layout, model, context);
}

fn render_diagnostics_scrollbar(
    frame: &mut Frame<'_>,
    layout: &DiagnosticsLayout,
    model: &DiagnosticsViewModel,
    context: &RenderContext,
) {
    let Some(scrollbar) = layout.list_scrollbar else {
        return;
    };

    Scrollbar::new(model.item_count(), layout.rows.len(), layout.visible_start).render_frame(
        frame,
        scrollbar.track,
        context,
    );
}

fn render_diagnostics_detail(
    frame: &mut Frame<'_>,
    layout: &DiagnosticsLayout,
    model: &DiagnosticsViewModel,
    theme: &TundraTheme,
) {
    let inner = Surface::new().bordered(true).inner(layout.detail_panel);
    if inner.width == 0 || inner.height == 0 {
        return;
    }

    let lines = match model.tab {
        DiagnosticsTab::Health => model.selected_check().map_or_else(
            || vec![Line::styled("No check selected", theme.muted_style())],
            |check| diagnostics_check_detail_lines(check, model, theme),
        ),
        DiagnosticsTab::Incidents => model.selected_incident().map_or_else(
            || vec![Line::styled("No incident selected", theme.muted_style())],
            |incident| diagnostics_incident_detail_lines(incident, model, theme),
        ),
        DiagnosticsTab::Logs if !model.can_view_details => vec![Line::styled(
            "Logs are restricted to administrators",
            theme.muted_style(),
        )],
        DiagnosticsTab::Logs => model.selected_log().map_or_else(
            || vec![Line::styled("No log selected", theme.muted_style())],
            |log| diagnostics_log_detail_lines(log, model, theme),
        ),
    };
    frame.render_widget(
        Paragraph::new(lines)
            .alignment(HorizontalAlignment::Left)
            .style(theme.body_style())
            .wrap(Wrap { trim: true }),
        inner,
    );
}

fn diagnostics_log_detail_lines(
    log: &crate::DiagnosticsLogViewModel,
    model: &DiagnosticsViewModel,
    theme: &TundraTheme,
) -> Vec<Line<'static>> {
    if !model.can_view_details {
        return vec![Line::styled(
            "Logs are restricted to administrators",
            theme.muted_style(),
        )];
    }
    vec![
        Line::styled(log.relative_path.clone(), theme.title_style()),
        Line::from(format!("Modified: {}", log.modified_at)),
        Line::from(format!("Size: {} bytes", log.size_bytes)),
        Line::from(format!("Path: {}", log.path)),
        Line::styled(
            "Press O to open read-only or E to explore the log folder",
            theme.muted_style(),
        ),
    ]
}

fn diagnostics_check_detail_lines(
    check: &DiagnosticsCheckViewModel,
    model: &DiagnosticsViewModel,
    theme: &TundraTheme,
) -> Vec<Line<'static>> {
    let mut lines = vec![
        Line::styled(
            format!("{} {}", check.status.marker(), check.label),
            diagnostics_status_style(check.status, theme, true),
        ),
        Line::from(format!("Category: {}", check.category)),
        Line::from(format!("Summary: {}", check.summary)),
    ];
    if model.can_view_details {
        lines.push(Line::from(format!("Detail: {}", check.detail)));
    } else {
        lines.push(Line::styled(
            "Detail: Restricted to administrators",
            theme.muted_style(),
        ));
    }
    if !check.remediation.is_empty() {
        lines.push(Line::from(format!("Recommended: {}", check.remediation)));
    }
    if check.repairable {
        let (message, style) = if model.restart_required {
            ("Repair disabled until restart", theme.muted_style())
        } else if model.can_repair {
            ("Repair available — press F", theme.title_style())
        } else {
            ("Repair requires administrator access", theme.muted_style())
        };
        lines.push(Line::styled(message, style));
    }
    lines
}

fn diagnostics_incident_detail_lines(
    incident: &DiagnosticsIncidentViewModel,
    model: &DiagnosticsViewModel,
    theme: &TundraTheme,
) -> Vec<Line<'static>> {
    let title = if model.can_view_details && !incident.restricted {
        format!("{} Incident {}", incident.severity.marker(), incident.id)
    } else {
        format!("{} Incident", incident.severity.marker())
    };
    let mut lines = vec![
        Line::styled(
            title,
            diagnostics_status_style(incident.severity, theme, true),
        ),
        Line::from(format!("Occurred: {}", incident.occurred_at)),
        Line::from(format!("Application: {}", incident.app)),
        Line::from(format!("Recovery: {}", incident.recovery)),
    ];
    if model.can_view_details && !incident.restricted {
        lines.extend([
            Line::from(format!("Summary: {}", incident.summary)),
            Line::from(format!("Detail: {}", incident.detail)),
            Line::from(format!("Report: {}", incident.report_path)),
        ]);
    } else {
        lines.push(Line::styled(
            "Details and report path are restricted to administrators",
            theme.muted_style(),
        ));
    }
    lines
}

fn render_diagnostics_footer(
    frame: &mut Frame<'_>,
    layout: &DiagnosticsLayout,
    model: &DiagnosticsViewModel,
    theme: &TundraTheme,
) {
    let help = if model.restart_required {
        "Restart required · Enter/R Restart · E Safe exit · Esc System Status".to_string()
    } else if model.scanning {
        "Scanning... · Esc System Status".to_string()
    } else {
        let mut actions = vec!["R Rescan", "Tab Switch", "C Copy", "Esc System Status"];
        if model.can_repair && model.tab == DiagnosticsTab::Health {
            actions.insert(1, "F Repair");
            actions.insert(2, "A Repair all");
        }
        if model.tab == DiagnosticsTab::Health || model.can_view_details {
            actions.insert(
                actions.len().saturating_sub(1),
                match model.tab {
                    DiagnosticsTab::Health => "O Open logs",
                    DiagnosticsTab::Logs => "O Open log",
                    DiagnosticsTab::Incidents => "O Open report",
                },
            );
        }
        if model.can_view_details {
            actions.insert(actions.len().saturating_sub(1), "E Log folder");
        }
        actions.insert(actions.len().saturating_sub(1), "X Restart");
        actions.join(" · ")
    };
    let text = model
        .feedback
        .as_ref()
        .map_or(help.clone(), |feedback| format!("{feedback} · {help}"));
    render_clock_line(
        frame,
        layout.footer,
        fit_cell(&text, usize::from(layout.footer.width)),
        if model.restart_required {
            diagnostics_warning_style(theme)
        } else if model.feedback.is_some() {
            theme.title_style()
        } else {
            theme.muted_style()
        },
        HorizontalAlignment::Left,
    );
}

fn render_diagnostics_repair_dialog(
    frame: &mut Frame<'_>,
    layout: &DiagnosticsRepairDialogLayout,
    model: &DiagnosticsRepairDialogViewModel,
    theme: &TundraTheme,
    context: &RenderContext,
) {
    frame.render_widget(Clear, layout.dialog);
    Surface::new()
        .titled("Repair preview")
        .bordered(true)
        .raised(true)
        .render_frame(frame, layout.dialog, context);
    frame.render_widget(
        Paragraph::new(vec![
            Line::styled("Review the changes before repair.", theme.title_style()),
            Line::from("Storage document repairs require a safe restart."),
        ])
        .alignment(HorizontalAlignment::Left)
        .style(theme.body_style())
        .wrap(Wrap { trim: true }),
        layout.prompt,
    );

    if model.items.is_empty() {
        render_clock_line(
            frame,
            Rect::new(
                layout.items_area.x,
                layout.items_area.y,
                layout.items_area.width,
                u16::from(layout.items_area.height > 0),
            ),
            "No repair actions selected".to_string(),
            theme.muted_style(),
            HorizontalAlignment::Left,
        );
    } else {
        let items = model
            .items
            .iter()
            .enumerate()
            .map(|(index, item)| {
                ListItem::new(
                    format!("diagnostics.repair.{index}"),
                    fit_cell(
                        &format!(" {}. {}", index.saturating_add(1), item.label),
                        usize::from(layout.items_area.width.saturating_sub(1)),
                    ),
                )
            })
            .collect::<Vec<_>>();
        let mut list =
            List::new("diagnostics.repair.items", items).with_viewport_start(layout.visible_start);
        list.set_selected(Some(model.selected));
        list.set_focused(true);
        list.render_borderless_frame(frame, layout.items_area, theme);
    }
    render_clock_line(
        frame,
        layout.help,
        "R Restart · Repairs run in order; completed independent repairs are kept.".to_string(),
        theme.muted_style(),
        HorizontalAlignment::Left,
    );
    render_diagnostics_button(
        frame,
        layout.confirm,
        "diagnostics.repair-confirm",
        "[ Confirm repair ]",
        model.confirm_selected,
        theme,
    );

    let mut restart_theme = *theme;
    restart_theme.foreground = diagnostics_warning_style(theme)
        .fg
        .unwrap_or(theme.foreground);
    let mut restart = Button::new("diagnostics.repair-restart", "[ Restart ]");
    restart.set_focused(true);
    restart.render_borderless_frame(frame, layout.restart, &restart_theme);

    render_diagnostics_button(
        frame,
        layout.cancel,
        "diagnostics.repair-cancel",
        "[ Cancel ]",
        !model.confirm_selected,
        theme,
    );
}

fn render_diagnostics_button(
    frame: &mut Frame<'_>,
    area: Rect,
    id: &'static str,
    label: &'static str,
    focused: bool,
    theme: &TundraTheme,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let mut button = Button::new(id, label);
    button.set_focused(focused);
    button.state.hovered = focused;
    button.render_borderless_frame(frame, area, theme);
}

fn diagnostics_status_style(
    status: DiagnosticsStatus,
    theme: &TundraTheme,
    selected: bool,
) -> Style {
    let style = match status {
        DiagnosticsStatus::Pass => theme.title_style(),
        DiagnosticsStatus::Unsupported => theme.muted_style(),
        DiagnosticsStatus::Warning => diagnostics_warning_style(theme),
        DiagnosticsStatus::Fail => theme.error_style(),
    };
    if selected {
        style.add_modifier(Modifier::BOLD)
    } else {
        style
    }
}

fn diagnostics_status_tone(status: DiagnosticsStatus) -> ComponentTone {
    match status {
        DiagnosticsStatus::Pass => ComponentTone::Success,
        DiagnosticsStatus::Unsupported => ComponentTone::Muted,
        DiagnosticsStatus::Warning => ComponentTone::Warning,
        DiagnosticsStatus::Fail => ComponentTone::Danger,
    }
}

fn diagnostics_warning_style(theme: &TundraTheme) -> Style {
    Style::default()
        .fg(theme.accent_color)
        .bg(theme.background)
        .add_modifier(Modifier::BOLD)
}
