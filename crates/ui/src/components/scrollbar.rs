use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::layout::{HorizontalAlignment, Rect};
use ratatui::style::Style;
use ratatui::widgets::{Paragraph, Widget};

use crate::RenderContext;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ScrollbarOrientation {
    #[default]
    VerticalRight,
    HorizontalBottom,
}

/// A token-aware scrollbar. The thumb geometry is entirely derived from the
/// current area, so callers can rebuild hit maps from the same values each
/// animation frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Scrollbar {
    pub content_len: usize,
    pub viewport_len: usize,
    pub offset: usize,
    pub orientation: ScrollbarOrientation,
}

impl Default for Scrollbar {
    fn default() -> Self {
        Self {
            content_len: 0,
            viewport_len: 0,
            offset: 0,
            orientation: ScrollbarOrientation::VerticalRight,
        }
    }
}

impl Scrollbar {
    pub const fn new(content_len: usize, viewport_len: usize, offset: usize) -> Self {
        Self {
            content_len,
            viewport_len,
            offset,
            orientation: ScrollbarOrientation::VerticalRight,
        }
    }

    pub const fn orientation(mut self, orientation: ScrollbarOrientation) -> Self {
        self.orientation = orientation;
        self
    }

    pub fn thumb_range(&self, area: Rect) -> (u16, u16) {
        let track_len = match self.orientation {
            ScrollbarOrientation::VerticalRight => area.height,
            ScrollbarOrientation::HorizontalBottom => area.width,
        };
        if track_len == 0 || self.content_len <= self.viewport_len || self.content_len == 0 {
            return (0, 0);
        }
        let thumb_len = ((track_len as usize * self.viewport_len).div_ceil(self.content_len))
            .max(1)
            .min(track_len as usize) as u16;
        let max_offset = self.content_len.saturating_sub(self.viewport_len).max(1);
        let start = (((track_len.saturating_sub(thumb_len) as usize) * self.offset.min(max_offset))
            / max_offset) as u16;
        (start, thumb_len)
    }

    pub fn render(&self, area: Rect, buffer: &mut Buffer, context: &RenderContext) {
        let tokens = context.theme;
        let (start, len) = self.thumb_range(area);
        if len == 0 {
            return;
        }
        match self.orientation {
            ScrollbarOrientation::VerticalRight => {
                let x = area.x.saturating_add(area.width.saturating_sub(1));
                for offset in 0..area.height {
                    let thumb = offset >= start && offset < start.saturating_add(len);
                    Paragraph::new(if thumb { "┃" } else { "│" })
                        .alignment(HorizontalAlignment::Left)
                        .style(
                            Style::default()
                                .fg(if thumb {
                                    tokens.accent_strong
                                } else {
                                    tokens.border
                                })
                                .bg(tokens.surface),
                        )
                        .render(Rect::new(x, area.y.saturating_add(offset), 1, 1), buffer);
                }
            }
            ScrollbarOrientation::HorizontalBottom => {
                let y = area.y.saturating_add(area.height.saturating_sub(1));
                for offset in 0..area.width {
                    let thumb = offset >= start && offset < start.saturating_add(len);
                    Paragraph::new(if thumb { "━" } else { "─" })
                        .alignment(HorizontalAlignment::Left)
                        .style(
                            Style::default()
                                .fg(if thumb {
                                    tokens.accent_strong
                                } else {
                                    tokens.border
                                })
                                .bg(tokens.surface),
                        )
                        .render(Rect::new(area.x.saturating_add(offset), y, 1, 1), buffer);
                }
            }
        }
    }

    pub fn render_frame(&self, frame: &mut Frame<'_>, area: Rect, context: &RenderContext) {
        self.render(area, frame.buffer_mut(), context);
    }
}
