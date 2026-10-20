use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::Widget,
};
use std::time::Duration;
use ui::components::RainbowText;

fn render(text: impl Into<Text<'static>>, elapsed: Duration) -> Buffer {
    let area = Rect::new(0, 0, 20, 3);
    let mut buffer = Buffer::empty(area);
    RainbowText::new(text, elapsed).render(area, &mut buffer);
    buffer
}

#[test]
fn rainbow_changes_colors_over_time_and_returns_to_the_start() {
    let first = render("Rainbow", Duration::ZERO);
    let later = render("Rainbow", Duration::from_secs(1));
    assert_ne!(first[(0, 0)].fg, first[(1, 0)].fg);
    assert_ne!(first[(0, 0)].fg, later[(0, 0)].fg);
    assert_eq!(first[(0, 0)].symbol(), later[(0, 0)].symbol());
    assert_eq!(first, render("Rainbow", Duration::from_secs(3)));
}

#[test]
fn rainbow_preserves_wide_characters_combining_marks_and_existing_styles() {
    let content = Text::from(Line::from(Span::styled(
        "中e\u{301}🙂Z",
        Style::new().bg(Color::Blue).add_modifier(Modifier::BOLD),
    )));
    let buffer = render(content, Duration::ZERO);
    for (column, symbol) in [(0, "中"), (2, "e\u{301}"), (3, "🙂"), (5, "Z")] {
        assert_eq!(buffer[(column, 0)].symbol(), symbol);
        assert_eq!(buffer[(column, 0)].bg, Color::Blue);
        assert!(buffer[(column, 0)].modifier.contains(Modifier::BOLD));
    }
    let narrow = render("abZ", Duration::ZERO);
    assert_eq!(buffer[(2, 0)].fg, narrow[(2, 0)].fg);
}
