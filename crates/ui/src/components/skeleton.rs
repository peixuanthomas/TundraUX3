use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::layout::{HorizontalAlignment, Rect};
use ratatui::style::Style;
use ratatui::widgets::{Paragraph, Widget};

use crate::RenderContext;

/// Token-aware loading placeholder. The shimmer is a colour/segment change,
/// not simulated transparency, and remains static under Reduced Motion.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Skeleton;

impl Skeleton {
    pub fn render(&self, area: Rect, buffer: &mut Buffer, context: &RenderContext) {
        let tokens = context.theme;
        let phase = if context.motion.reduced_motion {
            0
        } else {
            (context.motion.now.as_millis() / 120 % 3) as u16
        };
        for row in 0..area.height {
            let width = area.width as usize;
            let glyphs = (0..width)
                .map(|column| {
                    if (column as u16 + row + phase) % 5 == 0 {
                        '▒'
                    } else {
                        '░'
                    }
                })
                .collect::<String>();
            Paragraph::new(glyphs)
                .alignment(HorizontalAlignment::Left)
                .style(Style::default().fg(tokens.muted).bg(tokens.raised))
                .render(
                    Rect::new(area.x, area.y.saturating_add(row), area.width, 1),
                    buffer,
                );
        }
    }

    pub fn render_frame(&self, frame: &mut Frame<'_>, area: Rect, context: &RenderContext) {
        self.render(area, frame.buffer_mut(), context);
    }
}
