use std::time::Duration;

use ratatui::{
    Frame,
    buffer::Buffer,
    layout::{HorizontalAlignment, Rect},
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Paragraph, Widget},
};
use unicode_width::UnicodeWidthStr;

use crate::{ColorCapability, MotionFrame, RenderContext};

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

    pub fn from_motion(frame: MotionFrame) -> Self {
        let speed = f64::from(frame.animation_speed_percent.max(1)) / 100.0;
        Self::new(frame.now.mul_f64(speed)).reduced_motion(frame.reduced_motion)
    }

    pub const fn reduced_motion(mut self, reduced_motion: bool) -> Self {
        self.reduced_motion = reduced_motion;
        self
    }

    pub fn symbol(self) -> &'static str {
        const FRAMES: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
        if self.reduced_motion {
            return "⋯";
        }
        let frame = self.elapsed.as_millis() / Self::FRAME_INTERVAL.as_millis();
        FRAMES[(frame % FRAMES.len() as u128) as usize]
    }

    pub fn redraw_interval(frame: MotionFrame, capability: ColorCapability) -> Option<Duration> {
        if frame.reduced_motion {
            return None;
        }
        let interval = match capability {
            ColorCapability::TrueColor => Duration::from_millis(50),
            ColorCapability::Ansi => Self::FRAME_INTERVAL,
        }
        .mul_f64(100.0 / f64::from(frame.animation_speed_percent.max(1)));
        Some(interval.max(crate::ANIMATION_FRAME_INTERVAL))
    }

    /// Keep the whole label readable while a moving highlight crosses its glyphs.
    pub fn label(self, label: &str, width: u16, context: &RenderContext) -> Line<'static> {
        let indicator = self.reduced_motion(self.reduced_motion || context.motion.reduced_motion);
        let text = super::truncate_to_terminal_width(
            &format!("{} {label}", indicator.symbol()),
            usize::from(width),
        );
        let base = Style::default().fg(context.theme.muted);
        if indicator.reduced_motion || context.capabilities.color == ColorCapability::Ansi {
            return Line::styled(text, base);
        }

        // The sweep timing follows OpenCode's shimmer-text and tab-pulse renderers.
        let time = (indicator.elapsed.as_millis() % 1_200) as f32 / 1_200.0;
        let coast = if time < 0.2 {
            time * time / 0.32
        } else if time > 0.8 {
            1.0 - (1.0 - time) * (1.0 - time) / 0.32
        } else {
            (time - 0.1) / 0.8
        };
        let front = -4.0 + coast * (text.width() as f32 + 22.0);
        let start = crate::theme::color_rgb(context.theme.muted)
            .map(|(r, g, b)| Color::Rgb(r, g, b))
            .unwrap_or(context.theme.muted);
        let highlight = crate::theme::color_rgb(context.theme.text)
            .map(|(r, g, b)| Color::Rgb(r, g, b))
            .unwrap_or(context.theme.text);
        let text = Span::raw(text);
        let mut column = 0.0;
        use animate::Interpolate;
        Line::from(
            text.styled_graphemes(base)
                .map(|glyph| {
                    let distance = front - column;
                    let strength = if distance < 0.0 {
                        1.0 + distance / 4.0
                    } else {
                        1.0 - distance / 18.0
                    }
                    .clamp(0.0, 1.0);
                    let eased = strength.powi(3) * (strength * (strength * 6.0 - 15.0) + 10.0);
                    column += glyph.symbol.width() as f32;
                    Span::styled(
                        glyph.symbol.to_owned(),
                        base.fg(Color::lerp(&start, &highlight, eased)),
                    )
                })
                .collect::<Vec<_>>(),
        )
    }

    pub fn render(self, area: Rect, buffer: &mut Buffer, context: &RenderContext) {
        if area.is_empty() {
            return;
        }
        Paragraph::new(
            self.reduced_motion(self.reduced_motion || context.motion.reduced_motion)
                .symbol(),
        )
        .style(Style::default().fg(context.theme.muted))
        .render(Rect::new(area.x, area.y, 1, 1), buffer);
    }

    pub fn render_frame(self, frame: &mut Frame<'_>, area: Rect, context: &RenderContext) {
        self.render(area, frame.buffer_mut(), context);
    }

    pub(crate) fn render_labeled(
        self,
        frame: &mut Frame<'_>,
        area: Rect,
        label: &str,
        alignment: HorizontalAlignment,
        context: &RenderContext,
    ) {
        crate::automation::control("", "status", label, area, Default::default(), &[]);
        frame.render_widget(
            Paragraph::new(self.label(label, area.width, context)).alignment(alignment),
            area,
        );
    }
}
