use std::cell::RefCell;
use std::fmt::Write as _;
use std::num::NonZeroU16;

use ratatui::buffer::{Buffer, CellDiffOption};
use ratatui::layout::Rect;
use ratatui::style::Color;
use ratatui::widgets::Widget;
use unicode_width::UnicodeWidthChar as _;

thread_local! {
    static WORKSPACE_HEADINGS: RefCell<Option<(Rect, Vec<workspace_protocol::Heading>)>> = const { RefCell::new(None) };
}

pub struct WorkspaceHeadingCapture;

impl WorkspaceHeadingCapture {
    pub fn begin(content: Rect) -> Self {
        WORKSPACE_HEADINGS.with_borrow_mut(|current| *current = Some((content, Vec::new())));
        Self
    }

    pub fn finish(self) -> Vec<workspace_protocol::Heading> {
        WORKSPACE_HEADINGS.with_borrow_mut(|current| current.take().unwrap().1)
    }
}

impl Drop for WorkspaceHeadingCapture {
    fn drop(&mut self) {
        WORKSPACE_HEADINGS.with_borrow_mut(|current| *current = None);
    }
}

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

    fn text_sizing_sequence(&self, area_width: u16) -> (String, NonZeroU16) {
        let (numerator, denominator) = heading_size_ratio(self.tier);
        let mut output = String::new();
        let mut rendered_width = 0_u16;

        // Keep exact-width text from wrapping at the terminal edge.
        write!(
            output,
            "\x1b[{area_width}X\x1b[1B\x1b[{area_width}X\x1b[1A\x1b[?7l"
        )
        .expect("write to string");
        write_foreground_sequence(&mut output, self.color);

        let chars = self
            .text
            .chars()
            .filter(|character| !character.is_control())
            .collect::<Vec<_>>();
        for (chunk, chunk_width) in unicode_chunks(&chars, denominator) {
            let width = if chunk_width == denominator {
                numerator
            } else {
                chunk_width.saturating_mul(numerator).div_ceil(denominator)
            }
            .max(1);
            let Some(next) = rendered_width
                .checked_add(u16::from(width))
                .filter(|width| *width <= area_width)
            else {
                break;
            };
            rendered_width = next;
            write!(
                output,
                "\x1b]66;s=2:n={numerator}:d={denominator}:w={width};"
            )
            .expect("write to string");
            output.extend(chunk);
            output.push_str("\x1b\\");
        }

        output.push_str("\x1b[?7h\x1b[0m");
        (
            output,
            NonZeroU16::new(rendered_width.min(area_width).max(1))
                .expect("BigText always occupies at least one terminal cell"),
        )
    }

    fn render_native(&self, area: Rect, buffer: &mut Buffer) {
        let (sequence, rendered_width) = self.text_sizing_sequence(area.width);
        for y in area.top()..area.bottom() {
            for x in area.left()..area.right() {
                let cell = &mut buffer[(x, y)];
                if (x, y) == (area.x, area.y) {
                    cell.set_symbol(&sequence)
                        .set_diff_option(CellDiffOption::ForcedWidth(rendered_width));
                } else {
                    cell.set_diff_option(CellDiffOption::Skip);
                }
            }
        }
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
        if area.width == 0 || area.height != 2 || area.intersection(buffer.area) != area {
            return;
        }
        crate::automation::control("", "heading", self.text, area, Default::default(), &[]);
        let captured = WORKSPACE_HEADINGS.with_borrow_mut(|current| {
            let Some((content, headings)) = current else {
                return false;
            };
            if area.intersection(*content) == area {
                headings.push(workspace_protocol::Heading {
                    area: workspace_protocol::Rect {
                        x: area.x - content.x,
                        y: area.y - content.y,
                        width: area.width,
                        height: area.height,
                    },
                    text: self.text.into(),
                    level: self.tier,
                    foreground: crate::workspace::capture_color(self.color),
                });
            }
            true
        });
        if captured {
            buffer.set_stringn(
                area.x,
                area.y,
                self.text,
                area.width as usize,
                ratatui::style::Style::default().fg(self.color),
            );
        } else {
            self.render_native(area, buffer);
        }
    }
}

pub fn render_workspace_headings(
    frame: &mut ratatui::Frame<'_>,
    pane: Rect,
    headings: &[workspace_protocol::Heading],
    context: &crate::RenderContext,
) {
    let visible = pane.intersection(frame.area());
    let mut colors = crate::workspace::FrameColors::new(context);
    for heading in headings {
        let area = Rect::new(
            pane.x.saturating_add(heading.area.x),
            pane.y.saturating_add(heading.area.y),
            heading.area.width,
            heading.area.height,
        );
        if area.is_empty()
            || area.height != 2
            || area.intersection(visible) != area
            || !(1..=6).contains(&heading.level)
            || heading.text.contains(char::is_control)
        {
            continue;
        }
        let color = colors.render(heading.foreground);
        BigText::new(&heading.text, heading.level, color).render_native(area, frame.buffer_mut());
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
#[path = "../../tests/unit/components/big_text/tests.rs"]
mod tests;
