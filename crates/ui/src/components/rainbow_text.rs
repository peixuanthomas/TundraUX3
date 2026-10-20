use std::time::Duration;

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    text::{Span, Text},
    widgets::Widget,
};

/// Rainbow foreground colors that cycle every three seconds.
///
/// The caller supplies elapsed animation time; pass zero for a static rainbow.
/// Existing text layout, background colors and modifiers are preserved.
pub struct RainbowText<'a> {
    text: Text<'a>,
    elapsed: Duration,
}

impl<'a> RainbowText<'a> {
    pub fn new(text: impl Into<Text<'a>>, elapsed: Duration) -> Self {
        Self {
            text: text.into(),
            elapsed,
        }
    }
}

impl Widget for RainbowText<'_> {
    fn render(mut self, area: Rect, buffer: &mut Buffer) {
        let phase = (self.elapsed.as_millis() % 3_000) as f32 * 360.0 / 3_000.0;
        for line in &mut self.text.lines {
            let mut column = 0;
            line.spans = line
                .styled_graphemes(self.text.style)
                .map(|grapheme| {
                    let hue = (phase + column as f32 * 30.0) % 360.0;
                    column += super::terminal_width(grapheme.symbol);
                    Span::styled(
                        grapheme.symbol.to_owned(),
                        grapheme
                            .style
                            .fg(tachyonfx::color_from_hsl(hue, 100.0, 65.0)),
                    )
                })
                .collect();
        }
        self.text.render(area, buffer);
    }
}
