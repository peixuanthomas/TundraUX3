use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::{Paragraph, Sparkline};

use super::{ComponentState, EmptyState, Surface, tone_color};
use crate::{
    RenderContext, SystemStatusWidgetSize, SystemStatusWidgetState, SystemStatusWidgetViewModel,
};

/// Theme-aware dashboard card composed from the shared Surface and Ratatui widgets.
pub struct MetricCard<'a> {
    pub model: &'a SystemStatusWidgetViewModel,
    pub state: ComponentState,
}
impl<'a> MetricCard<'a> {
    pub const fn new(model: &'a SystemStatusWidgetViewModel) -> Self {
        Self {
            model,
            state: ComponentState {
                focused: false,
                hovered: false,
                active: false,
                selected: false,
                disabled: false,
            },
        }
    }
    pub fn render_frame(&self, frame: &mut Frame<'_>, area: Rect, context: &RenderContext) {
        if area.is_empty() {
            return;
        }
        let theme = &context.compatibility_theme();
        let mut surface = Surface::new()
            .titled(self.model.kind.label())
            .bordered(true)
            .raised(true);
        if self.state.focused || self.state.selected {
            surface = surface.border_shape(context.theme.border_shape);
        }
        let inner = surface.inner(area);
        surface.render_frame(frame, area, context);
        if self.state.selected {
            frame.render_widget(
                Paragraph::new("").style(Style::default().bg(context.theme.accent_soft)),
                inner,
            );
        }
        match &self.model.state {
            SystemStatusWidgetState::Loading => {
                EmptyState::new("Loading...").render_frame(frame, inner, context);
                return;
            }
            SystemStatusWidgetState::Unavailable { message } => {
                EmptyState::new("Unavailable")
                    .detail(message)
                    .render_frame(frame, inner, context);
                return;
            }
            SystemStatusWidgetState::Stale { message } if self.model.primary.is_empty() => {
                EmptyState::new("Stale data")
                    .detail(message)
                    .render_frame(frame, inner, context);
                return;
            }
            _ => {}
        }
        let stale = match &self.model.state {
            SystemStatusWidgetState::Stale { message } => Some(message.as_str()),
            _ => None,
        };
        let trend_h = if self.model.size != SystemStatusWidgetSize::Small
            && self.model.trend.is_some()
            && inner.height >= 3
        {
            1
        } else {
            0
        };
        let [text_area, trend_area] =
            Layout::vertical([Constraint::Min(0), Constraint::Length(trend_h)]).areas(inner);
        let mut lines = vec![Line::styled(
            self.model.primary.as_str(),
            Style::default()
                .fg(tone_color(self.model.tone, theme))
                .add_modifier(Modifier::BOLD),
        )];
        if self.model.size != SystemStatusWidgetSize::Small {
            for line in self
                .model
                .secondary
                .iter()
                .take(text_area.height.saturating_sub(1) as usize)
            {
                lines.push(Line::styled(line.as_str(), theme.muted_style()));
            }
        }
        if self.model.size == SystemStatusWidgetSize::Large {
            for row in self
                .model
                .compact_rows
                .iter()
                .take(text_area.height.saturating_sub(lines.len() as u16) as usize)
            {
                lines.push(Line::raw(row.join("  ")));
            }
        }
        if let Some(message) = stale {
            lines.push(Line::styled(
                format!("Stale: {message}"),
                Style::default().fg(context.theme.warning),
            ));
        }
        frame.render_widget(Paragraph::new(lines), text_area);
        if trend_h > 0 {
            if let Some(data) = &self.model.trend {
                frame.render_widget(
                    Sparkline::default()
                        .data(data)
                        .style(Style::default().fg(tone_color(self.model.tone, theme))),
                    trend_area,
                );
            }
        }
    }
}
