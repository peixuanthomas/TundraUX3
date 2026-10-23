use ratatui::{buffer::Buffer, layout::Rect, style::Color, widgets::Widget};
use ui::ThemeTokens;
use ui::components::ProgressGauge;

#[test]
fn solid_fills_choose_the_higher_contrast_black_or_white_text() {
    let tokens = ThemeTokens::default();
    for (background, expected) in [
        (Color::Rgb(255, 255, 0), Color::Black),
        (Color::Rgb(20, 30, 80), Color::White),
        // These straddle the sRGB contrast crossover, not a simple RGB midpoint.
        (Color::Rgb(117, 117, 117), Color::White),
        (Color::Rgb(118, 118, 118), Color::Black),
        (Color::Yellow, Color::Black),
        (Color::Blue, Color::White),
        (Color::White, Color::Black),
        (Color::Black, Color::White),
        (Color::Indexed(3), Color::Black),
        (Color::Indexed(4), Color::White),
        (Color::Indexed(16), Color::White),
        (Color::Indexed(21), Color::White),
        (Color::Indexed(226), Color::Black),
        (Color::Indexed(231), Color::Black),
        (Color::Indexed(232), Color::White),
        (Color::Indexed(255), Color::Black),
    ] {
        let style = tokens.filled_style(background);
        assert_eq!(style.fg, Some(expected), "{background:?}");
        assert_eq!(style.bg, Some(background));
    }
    assert_eq!(tokens.filled_style(Color::Reset).fg, Some(tokens.text));
}

#[test]
fn gauge_labels_follow_each_region_without_changing_the_track() {
    for fill in [Color::Yellow, Color::Blue, Color::Rgb(230, 240, 250)] {
        for track in [Color::Rgb(7, 17, 22), Color::White] {
            let tokens = ThemeTokens::default();
            for ratio in [0.0, 0.25, 0.5, 0.75, 1.0] {
                let area = Rect::new(2, 1, 20, 3);
                let mut buffer = Buffer::empty(Rect::new(0, 0, 24, 5));
                ProgressGauge::new("Progress", ratio, fill, track, &tokens)
                    .render(area, &mut buffer);
                let end = area.x + (f64::from(area.width) * ratio).floor() as u16;
                for (offset, symbol) in "Progress".chars().enumerate() {
                    let x = 8 + offset as u16;
                    let cell = &buffer[(x, 2)];
                    let background = if x < end { fill } else { track };
                    assert_eq!(cell.symbol(), symbol.to_string());
                    assert_eq!(cell.bg, background);
                    assert_eq!(cell.fg, tokens.filled_style(background).fg.unwrap());
                }
                assert_eq!(buffer[(21, 1)].bg, track);
                assert_eq!(buffer[(0, 0)].symbol(), " ");
            }
        }
    }
}

#[test]
fn gauge_handles_empty_narrow_and_unicode_labels() {
    let tokens = ThemeTokens::default();
    for width in 0..=8 {
        for height in 0..=2 {
            for label in ["", "abcdef", "下载 e\u{301}"] {
                let mut buffer = Buffer::empty(Rect::new(0, 0, 10, 4));
                ProgressGauge::new(label, 0.5, Color::Yellow, Color::Blue, &tokens)
                    .render(Rect::new(1, 1, width, height), &mut buffer);
                assert_eq!(buffer[(0, 0)].symbol(), " ");
                assert_eq!(buffer[(9, 3)].symbol(), " ");
            }
        }
    }
    let mut buffer = Buffer::empty(Rect::new(0, 0, 6, 1));
    ProgressGauge::new("下载 e\u{301}", 0.5, Color::Yellow, Color::Blue, &tokens)
        .render(buffer.area, &mut buffer);
    assert_eq!(buffer[(0, 0)].symbol(), "下");
    assert_eq!(buffer[(2, 0)].symbol(), "载");
    // The second wide glyph straddles the fill boundary at column 3.
    assert_eq!(buffer[(2, 0)].bg, Color::Yellow);
    assert_eq!(buffer[(2, 0)].fg, Color::Black);
    // Ratatui clears the continuation cell; the terminal displays the wide
    // glyph using the style of its first cell.
    assert_eq!(buffer[(3, 0)].symbol(), " ");
    assert_eq!(buffer[(5, 0)].symbol(), "e\u{301}");
    assert_eq!(buffer[(5, 0)].fg, Color::White);
    assert_eq!(buffer[(5, 0)].bg, Color::Blue);
}
