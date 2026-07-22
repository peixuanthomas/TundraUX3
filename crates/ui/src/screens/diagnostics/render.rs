use ratatui::Frame;
use ratatui::layout::{Alignment, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::{Borders, Clear, Paragraph, Wrap};

use super::layout::{DiagnosticsLayout, DiagnosticsRepairDialogLayout, diagnostics_layout};
use super::model::{
    DiagnosticsCheckViewModel, DiagnosticsIncidentViewModel, DiagnosticsRepairDialogViewModel,
    DiagnosticsStatus, DiagnosticsTab, DiagnosticsViewModel,
};
use crate::screens::clock::render_clock_line;
use crate::screens::shell::{fit_cell, render_compact_home, render_status, render_top};
use crate::{ShellChromeViewModel, ShellLayout, TundraTheme, compute_shell_layout};
pub fn render_diagnostics(
    frame: &mut Frame<'_>,
    area: Rect,
    chrome: &ShellChromeViewModel,
    model: &DiagnosticsViewModel,
    theme: &TundraTheme,
) {
    match compute_shell_layout(area) {
        ShellLayout::Compact(compact) => render_compact_home(frame, compact, chrome, theme),
        ShellLayout::Full { top, main, status } => {
            render_top(frame, top, chrome, theme);
            render_diagnostics_main(frame, main, model, theme);
            render_status(frame, status, chrome, theme);
        }
    }
}

fn render_diagnostics_main(
    frame: &mut Frame<'_>,
    main: Rect,
    model: &DiagnosticsViewModel,
    theme: &TundraTheme,
) {
    let layout = diagnostics_layout(main, model);
    frame.render_widget(
        theme
            .block()
            .title("Diagnostics")
            .borders(Borders::ALL)
            .style(theme.body_style()),
        layout.panel,
    );

    render_diagnostics_header(frame, &layout, model, theme);
    render_diagnostics_tabs(frame, &layout, model, theme);
    frame.render_widget(
        theme
            .block()
            .title(match model.tab {
                DiagnosticsTab::Health => "Checks",
                DiagnosticsTab::Logs => "Logs",
                DiagnosticsTab::Incidents => "Incidents",
            })
            .borders(Borders::ALL)
            .style(theme.body_style()),
        layout.list_panel,
    );
    frame.render_widget(
        theme
            .block()
            .title("Details")
            .borders(Borders::ALL)
            .style(theme.body_style()),
        layout.detail_panel,
    );
    render_diagnostics_rows(frame, &layout, model, theme);
    render_diagnostics_detail(frame, &layout, model, theme);
    render_diagnostics_footer(frame, &layout, model, theme);

    if let (Some(dialog_layout), Some(dialog)) =
        (layout.repair_dialog.as_ref(), model.repair_dialog.as_ref())
    {
        render_diagnostics_repair_dialog(frame, dialog_layout, dialog, theme);
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
        Alignment::Left,
    );
}

fn render_diagnostics_tabs(
    frame: &mut Frame<'_>,
    layout: &DiagnosticsLayout,
    model: &DiagnosticsViewModel,
    theme: &TundraTheme,
) {
    for tab in &layout.tabs {
        let active = model.tab == tab.tab;
        render_clock_line(
            frame,
            tab.area,
            format!(
                "{} {} {}",
                if active { "[" } else { " " },
                tab.tab.label(),
                if active { "]" } else { " " },
            ),
            if active {
                theme.title_style()
            } else {
                theme.muted_style()
            },
            Alignment::Center,
        );
    }
}

fn render_diagnostics_rows(
    frame: &mut Frame<'_>,
    layout: &DiagnosticsLayout,
    model: &DiagnosticsViewModel,
    theme: &TundraTheme,
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
            Alignment::Left,
        );
        return;
    }

    for row in &layout.rows {
        let (text, status, selected) = match model.tab {
            DiagnosticsTab::Health => {
                let Some(check) = model.checks.get(row.index) else {
                    continue;
                };
                (
                    format!(
                        "{} {} [{}] {}",
                        if row.index == model.selected_check {
                            ">"
                        } else {
                            " "
                        },
                        check.status.marker(),
                        check.category,
                        check.label,
                    ),
                    check.status,
                    row.index == model.selected_check,
                )
            }
            DiagnosticsTab::Incidents => {
                let Some(incident) = model.incidents.get(row.index) else {
                    continue;
                };
                (
                    format!(
                        "{} {} {} — {}",
                        if row.index == model.selected_incident {
                            ">"
                        } else {
                            " "
                        },
                        incident.severity.marker(),
                        incident.occurred_at,
                        incident.app,
                    ),
                    incident.severity,
                    row.index == model.selected_incident,
                )
            }
            DiagnosticsTab::Logs => {
                let Some(log) = model.logs.get(row.index) else {
                    continue;
                };
                (
                    format!(
                        "{} {}  {}  {} bytes",
                        if row.index == model.selected_log {
                            ">"
                        } else {
                            " "
                        },
                        log.relative_path,
                        log.modified_at,
                        log.size_bytes,
                    ),
                    DiagnosticsStatus::Pass,
                    row.index == model.selected_log,
                )
            }
        };
        let style = diagnostics_status_style(status, theme, selected);
        render_clock_line(
            frame,
            row.area,
            fit_cell(&text, usize::from(row.area.width)),
            style,
            Alignment::Left,
        );
    }

    render_diagnostics_scrollbar(frame, layout, theme);
}

fn render_diagnostics_scrollbar(
    frame: &mut Frame<'_>,
    layout: &DiagnosticsLayout,
    theme: &TundraTheme,
) {
    let Some(scrollbar) = layout.list_scrollbar else {
        return;
    };

    for y in scrollbar.track.y..scrollbar.track.bottom() {
        render_clock_line(
            frame,
            Rect::new(scrollbar.track.x, y, 1, 1),
            "|".to_string(),
            theme.muted_style(),
            Alignment::Left,
        );
    }
    for y in scrollbar.thumb.y..scrollbar.thumb.bottom() {
        render_clock_line(
            frame,
            Rect::new(scrollbar.thumb.x, y, 1, 1),
            "#".to_string(),
            theme.title_style(),
            Alignment::Left,
        );
    }
}

fn render_diagnostics_detail(
    frame: &mut Frame<'_>,
    layout: &DiagnosticsLayout,
    model: &DiagnosticsViewModel,
    theme: &TundraTheme,
) {
    let inner = theme
        .block()
        .borders(Borders::ALL)
        .inner(layout.detail_panel);
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
        "Restart required · Enter Safe exit · Esc Home".to_string()
    } else if model.scanning {
        "Scanning... · Esc Home".to_string()
    } else {
        let mut actions = vec!["R Rescan", "Tab Switch", "C Copy", "Esc Home"];
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
        Alignment::Left,
    );
}

fn render_diagnostics_repair_dialog(
    frame: &mut Frame<'_>,
    layout: &DiagnosticsRepairDialogLayout,
    model: &DiagnosticsRepairDialogViewModel,
    theme: &TundraTheme,
) {
    frame.render_widget(Clear, layout.dialog);
    frame.render_widget(
        theme
            .block()
            .title("Repair preview")
            .borders(Borders::ALL)
            .style(theme.body_style()),
        layout.dialog,
    );
    frame.render_widget(
        Paragraph::new(vec![
            Line::styled("Review the changes before repair.", theme.title_style()),
            Line::from("Storage document repairs require a safe restart."),
        ])
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
            Alignment::Left,
        );
    } else {
        for row in &layout.rows {
            let Some(item) = model.items.get(row.index) else {
                continue;
            };
            let selected = row.index == model.selected;
            render_clock_line(
                frame,
                row.area,
                fit_cell(
                    &format!(
                        "{} {}. {}",
                        if selected { ">" } else { " " },
                        row.index.saturating_add(1),
                        item.label,
                    ),
                    usize::from(row.area.width),
                ),
                if selected {
                    theme.title_style()
                } else {
                    theme.body_style()
                },
                Alignment::Left,
            );
        }
    }
    render_clock_line(
        frame,
        layout.help,
        "Repairs run in order; completed independent repairs are kept.".to_string(),
        theme.muted_style(),
        Alignment::Left,
    );
    render_clock_line(
        frame,
        layout.confirm,
        "[ Confirm repair ]".to_string(),
        if model.confirm_selected {
            theme.title_style()
        } else {
            theme.body_style()
        },
        Alignment::Center,
    );
    render_clock_line(
        frame,
        layout.cancel,
        "[ Cancel ]".to_string(),
        if model.confirm_selected {
            theme.body_style()
        } else {
            theme.title_style()
        },
        Alignment::Center,
    );
}

fn diagnostics_status_style(
    status: DiagnosticsStatus,
    theme: &TundraTheme,
    selected: bool,
) -> Style {
    let style = match status {
        DiagnosticsStatus::Pass => theme.title_style(),
        DiagnosticsStatus::Warning => diagnostics_warning_style(theme),
        DiagnosticsStatus::Fail => theme.error_style(),
    };
    if selected {
        style.add_modifier(Modifier::BOLD)
    } else {
        style
    }
}

fn diagnostics_warning_style(theme: &TundraTheme) -> Style {
    Style::default()
        .fg(Color::Yellow)
        .bg(theme.background)
        .add_modifier(Modifier::BOLD)
}
