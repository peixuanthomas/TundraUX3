use std::time::Duration;

use ratatui::{
    Frame,
    buffer::Buffer,
    layout::Rect,
    style::Style,
    widgets::{Paragraph, Widget},
};

use crate::{MotionFrame, RenderContext};

/// A one-cell busy marker driven by the host's existing animation clock.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BusyIndicator {
    elapsed: Duration,
    reduced_motion: bool,
}

impl BusyIndicator {
    pub const FRAME_INTERVAL: Duration = Duration::from_millis(80);

    pub const fn new(elapsed: Duration) -> Self {
        Self {
            elapsed,
            reduced_motion: false,
        }
    }

    pub const fn from_motion(frame: MotionFrame) -> Self {
        Self::new(frame.now).reduced_motion(frame.reduced_motion)
    }

    pub const fn reduced_motion(mut self, reduced_motion: bool) -> Self {
        self.reduced_motion = reduced_motion;
        self
    }

    pub fn symbol(self) -> &'static str {
        const FRAMES: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
        if self.reduced_motion {
            return "○";
        }
        let frame = self.elapsed.as_millis() / Self::FRAME_INTERVAL.as_millis();
        FRAMES[(frame % FRAMES.len() as u128) as usize]
    }

    pub fn render(self, area: Rect, buffer: &mut Buffer, context: &RenderContext) {
        if area.is_empty() {
            return;
        }
        Paragraph::new(
            self.reduced_motion(self.reduced_motion || context.motion.reduced_motion)
                .symbol(),
        )
        .style(
            Style::default()
                .fg(context.theme.accent)
                .bg(context.theme.canvas),
        )
        .render(Rect::new(area.x, area.y, 1, 1), buffer);
    }

    pub fn render_frame(self, frame: &mut Frame<'_>, area: Rect, context: &RenderContext) {
        self.render(area, frame.buffer_mut(), context);
    }
}
