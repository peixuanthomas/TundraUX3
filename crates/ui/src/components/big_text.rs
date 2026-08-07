use std::fmt::Write as _;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Color;
use ratatui::widgets::Widget;
use unicode_width::UnicodeWidthChar as _;

/// Terminal-native, multi-cell text rendered with the Kitty Text Sizing Protocol.
///
/// The caller must only render this widget after capability probing has confirmed
/// protocol support. Every heading occupies two terminal rows; `tier` controls
/// the fractional scale used for H1 through H6.
pub(crate) struct BigText<'a> {
    text: &'a str,
    tier: u8,
    color: Color,
}

impl<'a> BigText<'a> {
    pub(crate) const fn new(text: &'a str, tier: u8, color: Color) -> Self {
        Self { text, tier, color }
    }

    fn text_sizing_sequence(&self, area_width: u16) -> String {
        let (numerator, denominator) = heading_size_ratio(self.tier);
        let mut output = String::new();

        // Clear the complete two-row allocation before emitting a multi-cell
        // character and disable automatic wrapping for the protocol payload.
        write!(output, "\x1b[{area_width}X\x1b[?7l\x1b[1B").expect("write to string");
        write!(output, "\x1b[{area_width}X\x1b[?7l\x1b[1A").expect("write to string");
        write_foreground_sequence(&mut output, self.color);

        let chars = self.text.chars().collect::<Vec<_>>();
        for (chunk, chunk_width) in unicode_chunks(&chars, denominator) {
            let width = if chunk_width == denominator {
                numerator
            } else {
                chunk_width.saturating_mul(numerator).div_ceil(denominator)
            };
            write!(
                output,
                "\x1b]66;s=2:n={numerator}:d={denominator}:w={width};"
            )
            .expect("write to string");
            output.extend(chunk);
            output.push_str("\x1b\\");
        }

        output.push_str("\x1b[0m");
        output
    }
}

pub(crate) const fn heading_size_ratio(tier: u8) -> (u8, u8) {
    match tier {
        1 => (7, 7),
        2 => (5, 6),
        3 => (3, 4),
        4 => (2, 3),
        5 => (3, 5),
        _ => (1, 3),
    }
}

impl Widget for BigText<'_> {
    fn render(self, area: Rect, buffer: &mut Buffer) {
        if area.width == 0 || area.height == 0 {
            return;
        }

        let sequence = self.text_sizing_sequence(area.width);
        let mut first = true;
        for y in area.top()..area.bottom() {
            for x in area.left()..area.right() {
                let Some(cell) = buffer.cell_mut((x, y)) else {
                    continue;
                };
                if first {
                    first = false;
                    cell.set_skip(false).set_symbol(&sequence);
                } else {
                    // Ratatui must not overwrite the cells occupied by the
                    // terminal's multi-cell character during buffer diffing.
                    cell.set_skip(true);
                }
            }
        }
    }
}

fn unicode_chunks(chars: &[char], max_width: u8) -> impl Iterator<Item = (&[char], u8)> {
    let mut start = 0usize;
    std::iter::from_fn(move || {
        if start >= chars.len() {
            return None;
        }

        let mut end = start;
        let mut width = 0u8;
        while end < chars.len() {
            let char_width = chars[end].width().unwrap_or(1) as u8;
            if char_width > 1 {
                if width > 0 {
                    break;
                }
                width = char_width;
                end += 1;
                break;
            }
            if width.saturating_add(char_width) > max_width {
                break;
            }
            width = width.saturating_add(char_width);
            end += 1;
        }

        let chunk = &chars[start..end];
        start = end;
        Some((chunk, width))
    })
}

fn write_foreground_sequence(output: &mut String, color: Color) {
    let code = match color {
        Color::Reset => "39".to_string(),
        Color::Black => "30".to_string(),
        Color::Red => "31".to_string(),
        Color::Green => "32".to_string(),
        Color::Yellow => "33".to_string(),
        Color::Blue => "34".to_string(),
        Color::Magenta => "35".to_string(),
        Color::Cyan => "36".to_string(),
        Color::Gray => "37".to_string(),
        Color::DarkGray => "90".to_string(),
        Color::LightRed => "91".to_string(),
        Color::LightGreen => "92".to_string(),
        Color::LightYellow => "93".to_string(),
        Color::LightBlue => "94".to_string(),
        Color::LightMagenta => "95".to_string(),
        Color::LightCyan => "96".to_string(),
        Color::White => "97".to_string(),
        Color::Indexed(index) => format!("38;5;{index}"),
        Color::Rgb(red, green, blue) => format!("38;2;{red};{green};{blue}"),
    };
    write!(output, "\x1b[{code}m").expect("write to string");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn widget_emits_protocol_once_and_skips_the_remaining_cells() {
        let area = Rect::new(0, 0, 12, 2);
        let mut buffer = Buffer::empty(area);

        BigText::new("Title", 1, Color::Gray).render(area, &mut buffer);

        assert!(
            buffer[(0, 0)]
                .symbol()
                .contains("]66;s=2:n=7:d=7:w=5;Title")
        );
        assert!(!buffer[(0, 0)].skip);
        assert!(buffer[(1, 0)].skip);
        assert!(buffer[(0, 1)].skip);
    }
}
