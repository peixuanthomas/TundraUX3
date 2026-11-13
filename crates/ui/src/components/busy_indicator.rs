use std::time::Duration;

use ratatui::{
    Frame,
    buffer::Buffer,
    layout::Rect,
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Paragraph, Widget},
};

use crate::{ColorCapability, MotionFrame, RenderContext};

/// A one-cell spinner and an optional text highlight driven by the frame clock.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BusyIndicator {
    elapsed: Duration,
    reduced_motion: bool,
}

impl BusyIndicator {
    pub const FRAME_INTERVAL: Duration = Duration::from_millis(80);
    pub const LABEL_FRAME_INTERVAL: Duration = Duration::from_millis(40);

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

    pub(crate) fn label(self, label: &str, context: &RenderContext) -> Line<'static> {
        const CYCLE_MS: u128 = 1_200;
        const HEAD: f32 = 4.0;
        const TAIL: f32 = 18.0;
        let color = context.capabilities.color;
        let background = match color {
            ColorCapability::TrueColor => Color::Rgb(48, 48, 48),
            ColorCapability::Indexed => Color::Indexed(236),
            ColorCapability::Ansi => Color::DarkGray,
        };
        let width = super::terminal_width(label) as f32;
        let progress = (self.elapsed.as_millis() % CYCLE_MS) as f32 / CYCLE_MS as f32;
        let front = coast(progress) * (width + HEAD + TAIL) - HEAD;
        let label = Span::raw(label);
        let mut column = 0.0;
        let mut spans = vec![
            Span::styled(self.symbol(), Style::default().fg(context.theme.accent)),
            Span::raw(" "),
        ];
        for glyph in label.styled_graphemes(Style::default()) {
            let strength = if self.reduced_motion {
                0.0
            } else {
                let distance = front - column;
                let falloff = if distance < 0.0 { HEAD } else { TAIL };
                let t = (1.0 - distance.abs() / falloff).clamp(0.0, 1.0);
                t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
            };
            column += super::terminal_width(glyph.symbol) as f32;
            let gray = (170.0 + strength * 85.0).round() as u8;
            let foreground = match color {
                ColorCapability::TrueColor => Color::Rgb(gray, gray, gray),
                ColorCapability::Indexed => {
                    Color::Indexed(232 + gray.saturating_sub(8).div_ceil(10).min(23))
                }
                ColorCapability::Ansi if strength >= 0.65 => Color::White,
                ColorCapability::Ansi => Color::Gray,
            };
            spans.push(Span::styled(
                glyph.symbol.to_owned(),
                Style::default().fg(foreground).bg(background),
            ));
        }
        Line::from(spans)
    }

    pub(crate) fn render_label(
        self,
        frame: &mut Frame<'_>,
        area: Rect,
        label: &str,
        context: &RenderContext,
    ) {
        if !area.is_empty() {
            frame.render_widget(Paragraph::new(self.label(label, context)), area);
        }
    }
}

// OpenCode v2.2.6: see the attribution and MIT notice in the ui README.
// The sweep changes speed during the first and last fifth of its cycle.
fn coast(progress: f32) -> f32 {
    const RAMP: f32 = 0.2;
    if progress < RAMP {
        progress * progress / (2.0 * RAMP * (1.0 - RAMP))
    } else if progress > 1.0 - RAMP {
        1.0 - (1.0 - progress).powi(2) / (2.0 * RAMP * (1.0 - RAMP))
    } else {
        (progress - RAMP / 2.0) / (1.0 - RAMP)
    }
}
