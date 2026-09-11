use ratatui::Frame;
use ratatui::layout::{HorizontalAlignment, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::{Clear, Paragraph, Wrap};

use super::layout::{
    DiagnosticsContentLayout, DiagnosticsLayout, DiagnosticsRepairDialogLayout, diagnostics_layout,
};
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
        .titled(i18n::tr!("ui-diagnostics-system-status-diagnostics"))
        .bordered(false)
        .render_frame(frame, layout.panel, context);

    render_diagnostics_header(frame, layout.header, model, theme);
    render_diagnostics_tabs(frame, &layout, model, context);
    render_diagnostics_content(frame, &layout.content_layout(), model, theme, context);
    render_diagnostics_footer(
        frame,
        layout.footer,
        model,
        theme,
        &i18n::tr!("ui-diagnostics-esc-system-status"),
    );

    if let (Some(dialog_layout), Some(dialog)) =
        (layout.repair_dialog.as_ref(), model.repair_dialog.as_ref())
    {
        render_diagnostics_repair_dialog(frame, dialog_layout, dialog, theme, context);
    }
}

pub(crate) fn render_diagnostics_header(
    frame: &mut Frame<'_>,
    area: Rect,
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
            i18n::tr!("ui-diagnostics-restart-required"),
            diagnostics_warning_style(theme),
        )
    } else if model.scanning {
        (
            i18n::tr!("ui-diagnostics-scanning-health-checks"),
            theme.title_style(),
        )
    } else if failure_count > 0 {
        (
            i18n::tr!(
                "ui-diagnostics-attention-failures",
                warnings = warning_count,
                failures = failure_count
            ),
            theme.error_style(),
        )
    } else if warning_count > 0 {
        (
            i18n::tr!("ui-diagnostics-attention-warnings", count = warning_count),
            diagnostics_warning_style(theme),
        )
    } else if unsupported_count > 0 {
        (
            i18n::tr!(
                "ui-diagnostics-unsupported-count",
                count = unsupported_count
            ),
            theme.muted_style(),
        )
    } else if model.checks.is_empty() {
        (
            i18n::tr!("ui-diagnostics-no-health-checks-available"),
            theme.muted_style(),
        )
    } else {
        (
            i18n::tr!("ui-diagnostics-system-healthy"),
            theme.title_style(),
        )
    };
    let scanned_at = model
        .scanned_at
        .clone()
        .unwrap_or_else(|| i18n::tr!("ui-diagnostics-not-yet-scanned"));
    render_clock_line(
        frame,
        area,
        fit_cell(
            &i18n::tr!(
                "ui-diagnostics-last-scan",
                state = state,
                scanned_at = scanned_at
            ),
            usize::from(area.width),
        ),
        style,
        HorizontalAlignment::Left,
    );
}

pub(crate) fn render_diagnostics_content(
    frame: &mut Frame<'_>,
    layout: &DiagnosticsContentLayout,
    model: &DiagnosticsViewModel,
    theme: &TundraTheme,
    context: &RenderContext,
) {
    let title = match model.tab {
        DiagnosticsTab::Health => i18n::tr!("ui-diagnostics-checks"),
        DiagnosticsTab::Logs => i18n::tr!("ui-diagnostics-logs"),
        DiagnosticsTab::Incidents => i18n::tr!("ui-diagnostics-incidents"),
    };
    render_diagnostics_content_titled(frame, layout, model, theme, context, &title, None);
}

/// Shares the diagnostics list, details and scrollbar with other application hosts.
pub(crate) fn render_diagnostics_content_titled(
    frame: &mut Frame<'_>,
    layout: &DiagnosticsContentLayout,
    model: &DiagnosticsViewModel,
    theme: &TundraTheme,
    context: &RenderContext,
    title: &str,
    empty_content: Option<(String, String)>,
) {
    Surface::new()
        .titled(title)
        .bordered(true)
        .render_frame(frame, layout.list_panel, context);
    Surface::new()
        .titled(i18n::tr!("ui-diagnostics-details"))
        .bordered(true)
        .render_frame(frame, layout.detail_panel, context);
    if model.item_count() == 0 {
        if let Some((list_message, detail_message)) = empty_content {
            frame.render_widget(
                Paragraph::new(list_message)
                    .style(theme.muted_style())
                    .wrap(Wrap { trim: true }),
                layout.list_rows_area,
            );
            frame.render_widget(
                Paragraph::new(detail_message)
                    .style(theme.muted_style())
                    .wrap(Wrap { trim: true }),
                Surface::new().bordered(true).inner(layout.detail_panel),
            );
            return;
        }
    }
    render_diagnostics_rows(frame, layout, model, theme, context);
    render_diagnostics_detail(frame, layout, model, theme);
}

fn render_diagnostics_tabs(
    frame: &mut Frame<'_>,
    layout: &DiagnosticsLayout,
    _model: &DiagnosticsViewModel,
    context: &RenderContext,
) {
    let items = [DiagnosticsTab::Health]
        .into_iter()
        .map(|tab| {
            TabItem::new(
                format!("diagnostics.tab.{tab:?}"),
                format!("[{}]", tab.label()),
            )
        })
        .collect();
    let mut tabs = Tabs::new("diagnostics.tabs", items);
    tabs.set_selected(Some(0));

    tabs.render_borderless_frame(frame, layout.tabs_area, &context.compatibility_theme());
}

fn render_diagnostics_rows(
    frame: &mut Frame<'_>,
    layout: &DiagnosticsContentLayout,
    model: &DiagnosticsViewModel,
    theme: &TundraTheme,
    context: &RenderContext,
) {
    if model.item_count() == 0 {
        let text = if model.scanning && model.tab == DiagnosticsTab::Health {
            i18n::tr!("ui-diagnostics-scanning-padded")
        } else {
            match model.tab {
                DiagnosticsTab::Health => i18n::tr!("ui-diagnostics-no-checks-available-padded"),
                DiagnosticsTab::Logs => {
                    if model.can_view_details {
                        i18n::tr!("ui-diagnostics-no-logs-found-padded")
                    } else {
                        i18n::tr!("ui-diagnostics-logs-are-restricted-to-administrators-padded")
                    }
                }
                DiagnosticsTab::Incidents => {
                    i18n::tr!("ui-diagnostics-no-incidents-recorded-padded")
                }
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
                        i18n::tr!(
                            "ui-diagnostics-log-row",
                            name = log.relative_path.clone(),
                            modified = log.modified_at.clone(),
                            size = log.size_bytes.clone()
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
    layout: &DiagnosticsContentLayout,
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
    layout: &DiagnosticsContentLayout,
    model: &DiagnosticsViewModel,
    theme: &TundraTheme,
) {
    let inner = Surface::new().bordered(true).inner(layout.detail_panel);
    if inner.width == 0 || inner.height == 0 {
        return;
    }

    let lines = match model.tab {
        DiagnosticsTab::Health => model.selected_check().map_or_else(
            || {
                vec![Line::styled(
                    i18n::tr!("ui-diagnostics-no-check-selected"),
                    theme.muted_style(),
                )]
            },
            |check| diagnostics_check_detail_lines(check, model, theme),
        ),
        DiagnosticsTab::Incidents => model.selected_incident().map_or_else(
            || {
                vec![Line::styled(
                    i18n::tr!("ui-diagnostics-no-incident-selected"),
                    theme.muted_style(),
                )]
            },
            |incident| diagnostics_incident_detail_lines(incident, model, theme),
        ),
        DiagnosticsTab::Logs if !model.can_view_details => vec![Line::styled(
            i18n::tr!("ui-diagnostics-logs-are-restricted-to-administrators"),
            theme.muted_style(),
        )],
        DiagnosticsTab::Logs => model.selected_log().map_or_else(
            || {
                vec![Line::styled(
                    i18n::tr!("ui-diagnostics-no-log-selected"),
                    theme.muted_style(),
                )]
            },
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
            i18n::tr!("ui-diagnostics-logs-are-restricted-to-administrators"),
            theme.muted_style(),
        )];
    }
    vec![
        Line::styled(log.relative_path.clone(), theme.title_style()),
        Line::from(i18n::tr!(
            "ui-diagnostics-modified",
            modified = log.modified_at.clone()
        )),
        Line::from(i18n::tr!(
            "ui-diagnostics-size-bytes",
            size = log.size_bytes.clone()
        )),
        Line::from(i18n::tr!("ui-diagnostics-path", path = log.path.clone())),
        Line::styled(
            i18n::tr!("ui-diagnostics-press-o-to-open-read-only-or-e-to-explore-the-log-folder"),
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
        Line::from(i18n::tr!(
            "ui-diagnostics-category",
            category = check.category.clone()
        )),
        Line::from(i18n::tr!(
            "ui-diagnostics-summary",
            summary = check.summary.clone()
        )),
    ];
    if model.can_view_details {
        lines.push(Line::from(i18n::tr!(
            "ui-diagnostics-detail",
            detail = check.detail.clone()
        )));
    } else {
        lines.push(Line::styled(
            i18n::tr!("ui-diagnostics-detail-restricted-to-administrators"),
            theme.muted_style(),
        ));
    }
    if !check.remediation.is_empty() {
        lines.push(Line::from(i18n::tr!(
            "ui-diagnostics-recommended",
            remediation = check.remediation.clone()
        )));
    }
    if check.repairable {
        let (message, style) = if model.restart_required {
            (
                i18n::tr!("ui-diagnostics-repair-disabled-until-restart"),
                theme.muted_style(),
            )
        } else if model.can_repair {
            (
                i18n::tr!("ui-diagnostics-repair-available-press-f"),
                theme.title_style(),
            )
        } else {
            (
                i18n::tr!("ui-diagnostics-repair-requires-administrator-access"),
                theme.muted_style(),
            )
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
        i18n::tr!(
            "ui-diagnostics-incident-id",
            severity = incident.severity.marker(),
            id = incident.id.clone()
        )
    } else {
        i18n::tr!(
            "ui-diagnostics-incident",
            severity = incident.severity.marker()
        )
    };
    let mut lines = vec![
        Line::styled(
            title,
            diagnostics_status_style(incident.severity, theme, true),
        ),
        Line::from(i18n::tr!(
            "ui-diagnostics-occurred",
            time = incident.occurred_at.clone()
        )),
        Line::from(i18n::tr!(
            "ui-diagnostics-application",
            app = incident.app.clone()
        )),
        Line::from(i18n::tr!(
            "ui-diagnostics-recovery",
            recovery = incident.recovery.clone()
        )),
    ];
    if model.can_view_details && !incident.restricted {
        lines.extend([
            Line::from(i18n::tr!(
                "ui-diagnostics-summary",
                summary = incident.summary.clone()
            )),
            Line::from(i18n::tr!(
                "ui-diagnostics-detail",
                detail = incident.detail.clone()
            )),
            Line::from(i18n::tr!(
                "ui-diagnostics-report",
                path = incident.report_path.clone()
            )),
        ]);
    } else {
        lines.push(Line::styled(
            i18n::tr!("ui-diagnostics-details-and-report-path-are-restricted-to-administrators"),
            theme.muted_style(),
        ));
    }
    lines
}

pub(crate) fn render_diagnostics_footer(
    frame: &mut Frame<'_>,
    area: Rect,
    model: &DiagnosticsViewModel,
    theme: &TundraTheme,
    close_hint: &str,
) {
    let help = if model.restart_required {
        i18n::tr!("ui-diagnostics-restart-help", close_hint = close_hint)
    } else if model.scanning {
        i18n::tr!("ui-diagnostics-scanning-help", close_hint = close_hint)
    } else {
        let mut actions = vec![
            i18n::tr!("ui-diagnostics-r-rescan"),
            i18n::tr!("ui-diagnostics-c-copy"),
            close_hint.to_owned(),
        ];
        if model.can_repair && model.tab == DiagnosticsTab::Health {
            actions.insert(1, i18n::tr!("ui-diagnostics-f-repair"));
            actions.insert(2, i18n::tr!("ui-diagnostics-a-repair-all"));
        }
        if model.tab != DiagnosticsTab::Health && model.can_view_details {
            actions.insert(
                actions.len().saturating_sub(1),
                match model.tab {
                    DiagnosticsTab::Health => unreachable!(),
                    DiagnosticsTab::Logs => i18n::tr!("ui-diagnostics-o-open-log"),
                    DiagnosticsTab::Incidents => i18n::tr!("ui-diagnostics-o-open-report"),
                },
            );
        }
        if model.can_view_details && model.tab != DiagnosticsTab::Health {
            actions.insert(
                actions.len().saturating_sub(1),
                i18n::tr!("ui-diagnostics-e-log-folder"),
            );
        }
        actions.insert(
            actions.len().saturating_sub(1),
            i18n::tr!("ui-diagnostics-x-restart"),
        );
        actions.join(" · ")
    };
    let text = model
        .feedback
        .as_ref()
        .map_or(help.clone(), |feedback| format!("{feedback} · {help}"));
    render_clock_line(
        frame,
        area,
        fit_cell(&text, usize::from(area.width)),
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

pub(crate) fn render_diagnostics_repair_dialog(
    frame: &mut Frame<'_>,
    layout: &DiagnosticsRepairDialogLayout,
    model: &DiagnosticsRepairDialogViewModel,
    theme: &TundraTheme,
    context: &RenderContext,
) {
    frame.render_widget(Clear, layout.dialog);
    Surface::new()
        .titled(i18n::tr!("ui-diagnostics-repair-preview"))
        .bordered(true)
        .raised(true)
        .render_frame(frame, layout.dialog, context);
    frame.render_widget(
        Paragraph::new(vec![
            Line::styled(
                i18n::tr!("ui-diagnostics-review-the-changes-before-repair"),
                theme.title_style(),
            ),
            Line::from(i18n::tr!(
                "ui-diagnostics-storage-document-repairs-require-a-safe-restart"
            )),
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
            i18n::tr!("ui-diagnostics-no-repair-actions-selected"),
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
        i18n::tr!(
            "ui-diagnostics-r-restart-repairs-run-in-order-completed-independent-repairs-are-kept"
        ),
        theme.muted_style(),
        HorizontalAlignment::Left,
    );
    render_diagnostics_button(
        frame,
        layout.confirm,
        "diagnostics.repair-confirm",
        &i18n::tr!("ui-diagnostics-confirm-repair-button"),
        model.confirm_selected,
        theme,
    );

    let mut restart_theme = *theme;
    restart_theme.foreground = diagnostics_warning_style(theme)
        .fg
        .unwrap_or(theme.foreground);
    let mut restart = Button::new(
        "diagnostics.repair-restart",
        i18n::tr!("ui-diagnostics-restart-button"),
    );
    restart.set_focused(true);
    restart.render_borderless_frame(frame, layout.restart, &restart_theme);

    render_diagnostics_button(
        frame,
        layout.cancel,
        "diagnostics.repair-cancel",
        &i18n::tr!("ui-diagnostics-cancel-button"),
        !model.confirm_selected,
        theme,
    );
}

fn render_diagnostics_button(
    frame: &mut Frame<'_>,
    area: Rect,
    id: &'static str,
    label: &str,
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
