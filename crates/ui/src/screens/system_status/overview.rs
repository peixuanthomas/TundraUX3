use super::{layout::SystemStatusLayout, model::*};
use crate::RenderContext;
use crate::components::{EmptyState, Surface, tone_color};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::{Gauge, Paragraph, Sparkline};

pub(super) fn render_overview(
    frame: &mut Frame<'_>,
    layout: &SystemStatusLayout,
    model: &SystemStatusViewModel,
    context: &RenderContext,
) {
    let metrics = &model.dashboard.overview_metrics;
    if metrics.is_empty() {
        EmptyState::new("Loading system metrics...").render_frame(frame, layout.canvas, context);
        return;
    }
    let [summary, grid] =
        Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).areas(layout.canvas);
    if let Some(overview) = model.detail_widget(SystemStatusDetail::Overview) {
        let identity = overview
            .compact_rows
            .iter()
            .filter(|row| {
                row.first()
                    .is_some_and(|label| matches!(label.as_str(), "System" | "OS"))
            })
            .filter_map(|row| row.get(1))
            .cloned()
            .collect::<Vec<_>>()
            .join(" · ");
        frame.render_widget(
            Paragraph::new(if identity.is_empty() {
                overview.primary.clone()
            } else {
                format!("{} · {identity}", overview.primary)
            })
            .style(Style::default().fg(tone_color(overview.tone, &context.compatibility_theme()))),
            summary,
        );
    }
    let columns = if grid.width >= 104 || (grid.width >= 72 && grid.height < 20) {
        4
    } else {
        2
    };
    let rows = metrics.len().div_ceil(columns);
    let row_areas = Layout::vertical(vec![Constraint::Ratio(1, rows as u32); rows]).split(grid);
    for (row, chunk) in metrics.chunks(columns).enumerate() {
        let areas = Layout::horizontal(vec![Constraint::Ratio(1, columns as u32); columns])
            .split(row_areas[row]);
        for (metric, area) in chunk.iter().zip(areas.iter()) {
            render_metric(frame, *area, metric, context);
        }
    }
}

fn render_metric(
    frame: &mut Frame<'_>,
    area: Rect,
    metric: &SystemStatusWidgetViewModel,
    context: &RenderContext,
) {
    if area.height < 3 || area.width < 3 {
        frame.render_widget(
            Paragraph::new(format!("{}: {}", metric.kind.label(), metric.primary)),
            area,
        );
        return;
    }
    let stale = matches!(metric.state, SystemStatusWidgetState::Stale { .. });
    let title = format!(
        "{}{}",
        if stale { "Stale " } else { "" },
        if metric.kind == SystemStatusWidgetKind::Network {
            "Network · Down"
        } else {
            metric.kind.label()
        },
    );
    let surface = Surface::new().titled(title).bordered(true).raised(true);
    surface.render_frame(frame, area, context);
    let inner = surface.inner(area);
    if inner.is_empty() {
        return;
    }
    let placeholder = match &metric.state {
        SystemStatusWidgetState::Loading => Some(("Loading...", "Waiting for a sample")),
        SystemStatusWidgetState::Unavailable { message } => Some(("Unavailable", message.as_str())),
        SystemStatusWidgetState::Stale { message } if metric.primary.is_empty() => {
            Some(("Stale data", message.as_str()))
        }
        _ => None,
    };
    if let Some((label, message)) = placeholder {
        frame.render_widget(
            Paragraph::new(vec![Line::raw(label), Line::raw(message)])
                .style(context.compatibility_theme().muted_style()),
            inner,
        );
        return;
    }
    let color = if stale {
        context.theme.warning
    } else {
        match (metric.kind, metric.progress_percent) {
            (SystemStatusWidgetKind::Cpu | SystemStatusWidgetKind::Memory, Some(90..)) => {
                context.theme.danger
            }
            (SystemStatusWidgetKind::Cpu | SystemStatusWidgetKind::Memory, Some(75..)) => {
                context.theme.warning
            }
            (SystemStatusWidgetKind::Battery, Some(0..=10)) => context.theme.danger,
            (SystemStatusWidgetKind::Battery, Some(11..=20)) => context.theme.warning,
            _ => tone_color(metric.tone, &context.compatibility_theme()),
        }
    };
    let style = Style::default().fg(color);
    let trend = metric
        .trend
        .as_ref()
        .filter(|data| !data.is_empty())
        .map(|data| &data[data.len().saturating_sub(usize::from(inner.width))..]);
    let has_graph = metric.progress_percent.is_some() || trend.is_some();
    let graph_height = if !has_graph {
        0
    } else if inner.height >= 5 {
        2
    } else if metric.progress_percent.is_some() || inner.height >= 3 {
        1
    } else {
        0
    };
    let [text, graph] =
        Layout::vertical([Constraint::Min(0), Constraint::Length(graph_height)]).areas(inner);
    let mut lines = vec![Line::styled(
        if metric.primary.is_empty() {
            "No readings"
        } else {
            &metric.primary
        },
        style.add_modifier(Modifier::BOLD),
    )];
    lines.extend(metric.secondary.iter().map(|line| Line::raw(line.as_str())));
    frame.render_widget(Paragraph::new(lines), text);
    if let Some(percent) = metric.progress_percent {
        // A fixed 0–100 scale makes CPU, RAM, disk and battery comparable.
        frame.render_widget(
            Gauge::default()
                .ratio(
                    f64::from(
                        metric
                            .display_basis_points
                            .unwrap_or(percent.min(100) * 100)
                            .min(10_000),
                    ) / 10_000.0,
                )
                .use_unicode(true)
                .label(metric.primary.as_str())
                .gauge_style(style),
            Rect::new(graph.x, graph.y, graph.width, u16::from(graph.height > 0)),
        );
        if graph.height > 1 {
            if let Some(data) = trend {
                frame.render_widget(
                    Sparkline::default().data(data).max(100).style(style),
                    Rect::new(graph.x, graph.y + 1, graph.width, 1),
                );
            }
        }
    } else if let Some(data) = trend {
        let label = match metric.kind {
            SystemStatusWidgetKind::Network => "Download trend",
            SystemStatusWidgetKind::Temperature => "Temperature trend",
            _ => "Recent samples",
        };
        frame.render_widget(Sparkline::default().data(data).style(style), graph);
        if usize::from(text.height) > metric.secondary.len() + 1 {
            frame.render_widget(
                Paragraph::new(label).style(context.compatibility_theme().muted_style()),
                Rect::new(text.x, text.bottom() - 1, text.width, 1),
            );
        }
    }
}
