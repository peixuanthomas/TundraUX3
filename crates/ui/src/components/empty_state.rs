use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::layout::{HorizontalAlignment, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::{Paragraph, Widget};

use crate::RenderContext;

/// Standard empty state used instead of bespoke one-off placeholder blocks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmptyState {
    pub title: String,
    pub detail: Option<String>,
}

impl EmptyState {
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            detail: None,
        }
    }
    pub fn detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }

    pub fn render(&self, area: Rect, buffer: &mut Buffer, context: &RenderContext) {
        let tokens = context.theme;
        let mut lines = vec![Line::styled(
            self.title.as_str(),
            Style::default()
                .fg(tokens.text)
                .add_modifier(Modifier::BOLD),
        )];
        if let Some(detail) = &self.detail {
            lines.push(Line::styled(
                detail.as_str(),
                Style::default().fg(tokens.muted),
            ));
        }
        Paragraph::new(lines)
            .alignment(HorizontalAlignment::Center)
            .style(Style::default().bg(tokens.surface))
            .render(area, buffer);
    }

    pub fn render_frame(&self, frame: &mut Frame<'_>, area: Rect, context: &RenderContext) {
        self.render(area, frame.buffer_mut(), context);
    }
}
