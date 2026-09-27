use super::{layout::SystemStatusLayout, model::*};
use crate::RenderContext;
use crate::components::terminal_width;
use crate::screens::editor::terminal_safe_text;
use crate::screens::shell::fit_cell;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

pub(super) fn render_processes(
    frame: &mut Frame<'_>,
    layout: &SystemStatusLayout,
    model: &SystemStatusViewModel,
    processes: &SystemStatusWidgetViewModel,
    context: &RenderContext,
) {
    let summary = layout.detail_summary_area;
    for (row, kind, color) in [
        (0, SystemStatusWidgetKind::Cpu, Color::Green),
        (1, SystemStatusWidgetKind::Memory, Color::Magenta),
    ] {
        if row >= summary.height {
            continue;
        }
        let metric = model
            .dashboard
            .overview_metrics
            .iter()
            .find(|metric| metric.kind == kind)
            .or_else(|| model.detail_widget(kind.detail()));
        let area = Rect::new(summary.x, summary.y + row, summary.width, 1);
        render_meter(frame, area, kind, metric, color, context);
    }
    if summary.height > 2 {
        frame.render_widget(
            Paragraph::new(terminal_safe_text(&processes.primary)).style(
                Style::default()
                    .fg(context.theme.text)
                    .add_modifier(Modifier::BOLD),
            ),
            Rect::new(summary.x, summary.y + 2, summary.width, 1),
        );
    }
    if summary.height > 3 {
        let (text, color) = match &processes.state {
            SystemStatusWidgetState::Stale { message } => (
                i18n::tr!("ui-system-status-stale-message", message = message),
                context.theme.warning,
            ),
            _ => (
                i18n::tr!("ui-system-status-process-colors"),
                context.theme.muted,
            ),
        };
        frame.render_widget(
            Paragraph::new(text).style(Style::default().fg(color)),
            Rect::new(summary.x, summary.y + 3, summary.width, 1),
        );
    }
    let area = layout.rows_area;
    if area.is_empty() {
        return;
    }
    let headers = [
        i18n::tr!("ui-system-status-pid"),
        "CPU%".into(),
        i18n::tr!("ui-system-status-memory"),
        i18n::tr!("ui-system-status-sort"),
        i18n::tr!("ui-system-status-process"),
    ];
    let header = row_line(&headers, [context.theme.text; 5], area.width);
    frame.render_widget(
        Paragraph::new(header).style(
            Style::default()
                .bg(context.theme.raised)
                .add_modifier(Modifier::BOLD),
        ),
        Rect::new(area.x, area.y, area.width, 1),
    );
    for row_layout in &layout.rows {
        let Some(row) = processes
            .compact_rows
            .get(row_layout.index)
            .filter(|row| row.len() >= 5)
        else {
            continue;
        };
        let cpu = row[3].trim_end_matches('%').parse::<f32>().ok();
        let cpu_color = match cpu {
            Some(value) if value >= 80.0 => Color::Red,
            Some(value) if value >= 40.0 => Color::Yellow,
            Some(_) => Color::Green,
            None => context.theme.muted,
        };
        let fields = [
            row[1].clone(),
            row[3].clone(),
            row[4].clone(),
            row[0].clone(),
            row[2].clone(),
        ];
        let colors = [
            Color::Cyan,
            cpu_color,
            Color::Magenta,
            context.theme.muted,
            context.theme.text,
        ];
        let selected = model.selected_index() == Some(row_layout.index);
        let mut style = Style::default().bg(if selected {
            context.theme.accent_soft
        } else {
            context.theme.surface
        });
        if selected {
            style = style.add_modifier(Modifier::BOLD | Modifier::UNDERLINED);
        }
        frame.render_widget(
            Paragraph::new(row_line(&fields, colors, area.width)).style(style),
            row_layout.area,
        );
    }
}

fn row_line(fields: &[String; 5], colors: [Color; 5], width: u16) -> Line<'static> {
    // Keep numeric fields compact and right aligned; leave the rest for names.
    let widths = [7usize, 8, 11, 6, usize::from(width).saturating_sub(32)];
    let spans = fields
        .iter()
        .zip(colors)
        .zip(widths)
        .enumerate()
        .map(|(index, ((text, color), width))| {
            let text = terminal_safe_text(text);
            let fitted = fit_cell(&text, width.saturating_sub(1));
            let value = if index < 3 {
                let text = fitted.trim_end();
                let padding = width.saturating_sub(1 + terminal_width(text));
                format!("{}{text} ", " ".repeat(padding))
            } else {
                format!("{fitted} ")
            };
            Span::styled(value, Style::default().fg(color))
        })
        .collect::<Vec<_>>();
    Line::from(spans)
}

fn render_meter(
    frame: &mut Frame<'_>,
    area: Rect,
    kind: SystemStatusWidgetKind,
    metric: Option<&SystemStatusWidgetViewModel>,
    color: Color,
    context: &RenderContext,
) {
    let available = metric.filter(|metric| {
        matches!(
            metric.state,
            SystemStatusWidgetState::Ready | SystemStatusWidgetState::Stale { .. }
        )
    });
    let label = format!("{} ", kind.label());
    let value = available
        .map(|metric| metric.primary.clone())
        .unwrap_or_else(|| {
            if matches!(
                metric.map(|metric| &metric.state),
                Some(SystemStatusWidgetState::Loading)
            ) {
                i18n::tr!("ui-system-status-loading")
            } else {
                i18n::tr!("ui-system-status-unavailable")
            }
        });
    let bar_width = usize::from(area.width / 3).min(24);
    let mut spans = vec![Span::styled(
        label,
        Style::default().fg(color).add_modifier(Modifier::BOLD),
    )];
    if let Some(percent) = available.and_then(|metric| metric.progress_percent) {
        let filled = bar_width * usize::from(percent.min(100)) / 100;
        spans.push(Span::raw("["));
        for position in 0..bar_width {
            let fill_color = if kind == SystemStatusWidgetKind::Cpu {
                if position * 100 >= bar_width * 80 {
                    Color::Red
                } else if position * 100 >= bar_width * 40 {
                    Color::Yellow
                } else {
                    Color::Green
                }
            } else {
                color
            };
            spans.push(Span::styled(
                if position < filled { "|" } else { "·" },
                Style::default().fg(if position < filled {
                    fill_color
                } else {
                    context.theme.muted
                }),
            ));
        }
        spans.push(Span::raw("] "));
    }
    if matches!(
        available.map(|metric| &metric.state),
        Some(SystemStatusWidgetState::Stale { .. })
    ) {
        spans.push(Span::styled(
            format!("{} · ", i18n::tr!("ui-system-status-stale-data")),
            Style::default().fg(context.theme.warning),
        ));
    }
    spans.push(Span::styled(
        terminal_safe_text(&value).into_owned(),
        Style::default().fg(color),
    ));
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}
