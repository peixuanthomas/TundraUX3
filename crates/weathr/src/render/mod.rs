mod capabilities;
pub mod clock;

use crate::error::TerminalError;
use capabilities::TerminalCapabilities;
use crossterm::{
    cursor, execute, queue,
    style::{Color, Print, ResetColor, SetForegroundColor},
    terminal::{self, Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui_core::{
    buffer::Buffer,
    layout::Rect,
    style::{Color as BufferColor, Style},
};
use ratatui_crossterm::{FromCrossterm, IntoCrossterm};
use std::io::{self, BufWriter, IsTerminal, Stdout, Write};
use unicode_width::UnicodeWidthStr;

pub const MIN_TERMINAL_WIDTH: u16 = 70;
pub const MIN_TERMINAL_HEIGHT: u16 = 20;

const MAX_TERMINAL_WIDTH: u16 = 1000;
const MAX_TERMINAL_HEIGHT: u16 = 500;

fn clamp_terminal_size(width: u16, height: u16) -> (u16, u16) {
    (
        width.min(MAX_TERMINAL_WIDTH),
        height.min(MAX_TERMINAL_HEIGHT),
    )
}

pub fn validate_terminal_size(width: u16, height: u16) -> Result<(), TerminalError> {
    validate_terminal_size_with_minimum(width, height, MIN_TERMINAL_WIDTH, MIN_TERMINAL_HEIGHT)
}

pub fn validate_terminal_size_with_minimum(
    width: u16,
    height: u16,
    min_width: u16,
    min_height: u16,
) -> Result<(), TerminalError> {
    let min_width = min_width.max(MIN_TERMINAL_WIDTH);
    let min_height = min_height.max(MIN_TERMINAL_HEIGHT);
    if min_width > MAX_TERMINAL_WIDTH || min_height > MAX_TERMINAL_HEIGHT {
        return Err(TerminalError::RequirementTooLarge {
            min_width,
            min_height,
            max_width: MAX_TERMINAL_WIDTH,
            max_height: MAX_TERMINAL_HEIGHT,
        });
    }
    if width < min_width || height < min_height {
        return Err(TerminalError::TooSmall {
            width,
            height,
            min_width,
            min_height,
        });
    }

    Ok(())
}

pub(crate) fn centered_column(width: u16, text_width: usize) -> u16 {
    (usize::from(width).saturating_sub(text_width) / 2) as u16
}

pub struct TerminalRenderer {
    stdout: BufWriter<Stdout>,
    width: u16,
    height: u16,
    buffer: Buffer,
    last_buffer: Buffer,
    last_buffer_valid: bool,
    capabilities: TerminalCapabilities,
    min_width: u16,
    min_height: u16,
}

impl TerminalRenderer {
    pub fn new() -> Result<Self, TerminalError> {
        Self::new_with_minimum((MIN_TERMINAL_WIDTH, MIN_TERMINAL_HEIGHT))
    }

    pub fn new_with_minimum((min_width, min_height): (u16, u16)) -> Result<Self, TerminalError> {
        if !io::stdout().is_terminal() {
            return Err(TerminalError::NotATty);
        }

        let (width, height) = terminal::size().map_err(TerminalError::SizeError)?;
        let min_width = min_width.max(MIN_TERMINAL_WIDTH);
        let min_height = min_height.max(MIN_TERMINAL_HEIGHT);

        validate_terminal_size_with_minimum(width, height, min_width, min_height)?;

        let (width, height) = clamp_terminal_size(width, height);

        let stdout = BufWriter::new(io::stdout());
        let area = Rect::new(0, 0, width, height);
        let capabilities = TerminalCapabilities::detect();

        Ok(Self {
            stdout,
            width,
            height,
            buffer: Buffer::empty(area),
            last_buffer: Buffer::empty(area),
            last_buffer_valid: true,
            capabilities,
            min_width,
            min_height,
        })
    }

    pub fn init(&mut self) -> Result<(), TerminalError> {
        terminal::enable_raw_mode().map_err(TerminalError::RawModeError)?;
        execute!(self.stdout, EnterAlternateScreen, cursor::Hide)
            .map_err(TerminalError::InitError)?;
        Ok(())
    }

    pub fn cleanup(&mut self) -> io::Result<()> {
        execute!(self.stdout, LeaveAlternateScreen, cursor::Show, ResetColor)?;
        terminal::disable_raw_mode()?;
        Ok(())
    }

    pub fn manual_resize(&mut self, width: u16, height: u16) -> io::Result<()> {
        validate_terminal_size_with_minimum(width, height, self.min_width, self.min_height)
            .map_err(io::Error::other)?;
        let (width, height) = clamp_terminal_size(width, height);
        if width != self.width || height != self.height {
            self.width = width;
            self.height = height;
            let area = Rect::new(0, 0, width, height);
            self.buffer = Buffer::empty(area);
            self.last_buffer = Buffer::empty(area);
            self.last_buffer_valid = false;
            execute!(self.stdout, Clear(ClearType::All))?;
            self.last_buffer_valid = true;
        }
        Ok(())
    }

    pub fn get_size(&self) -> (u16, u16) {
        (self.width, self.height)
    }

    pub fn clear(&mut self) -> io::Result<()> {
        self.buffer.reset();
        Ok(())
    }

    pub fn render_centered_colored(
        &mut self,
        lines: &[String],
        start_row: u16,
        color: Color,
    ) -> io::Result<()> {
        let max_width = lines.iter().map(|line| line.width()).max().unwrap_or(0);
        let start_col = centered_column(self.width, max_width);
        for (idx, line) in lines.iter().enumerate() {
            let row = usize::from(start_row).saturating_add(idx);
            if row >= usize::from(self.height) {
                break;
            }
            self.render_line_colored(start_col, row as u16, line, color)?;
        }
        Ok(())
    }

    pub fn render_line_colored(
        &mut self,
        x: u16,
        y: u16,
        text: &str,
        color: Color,
    ) -> io::Result<()> {
        let color = self.capabilities.adjust_color(color);
        write_line(&mut self.buffer, x, y, text, color);
        Ok(())
    }

    pub fn render_char(&mut self, x: u16, y: u16, ch: char, color: Color) -> io::Result<()> {
        let mut encoded = [0; 4];
        self.render_line_colored(x, y, ch.encode_utf8(&mut encoded), color)
    }

    pub fn flash_screen(&mut self) -> io::Result<()> {
        let flash_color = self.capabilities.adjust_color(Color::White);
        for cell in &mut self.buffer.content {
            cell.fg = BufferColor::from_crossterm(flash_color);
        }
        Ok(())
    }

    pub fn flush(&mut self) -> io::Result<()> {
        let result = self.flush_changed_cells();
        self.last_buffer_valid = result.is_ok();
        result
    }

    fn flush_changed_cells(&mut self) -> io::Result<()> {
        if !self.last_buffer_valid {
            queue!(self.stdout, ResetColor, Clear(ClearType::All))?;
            self.last_buffer.reset();
        }
        write_changed_cells(&mut self.stdout, &self.last_buffer, &self.buffer)?;
        self.last_buffer.clone_from(&self.buffer);
        Ok(())
    }
}

fn write_line(buffer: &mut Buffer, x: u16, y: u16, text: &str, color: Color) {
    if !buffer.area.contains((x, y).into()) {
        return;
    }
    // Ratatui handles grapheme boundaries, display widths, and clipping. Reuse its
    // buffer rather than splitting translated text into one cell per Unicode scalar.
    buffer.set_stringn(
        x,
        y,
        text,
        usize::from(buffer.area.right().saturating_sub(x)),
        Style::default().fg(BufferColor::from_crossterm(color)),
    );
}

fn write_changed_cells(
    output: &mut impl Write,
    previous: &Buffer,
    next: &Buffer,
) -> io::Result<()> {
    let mut current_color = Color::Reset;
    let mut expected_pos = None;
    for (x, y, cell) in previous.diff_iter(next) {
        if expected_pos != Some((x, y)) {
            queue!(output, cursor::MoveTo(x, y))?;
        }
        let color = cell.fg.into_crossterm();
        if color != current_color {
            queue!(output, SetForegroundColor(color))?;
            current_color = color;
        }
        queue!(output, Print(cell.symbol()))?;
        expected_pos = Some((x.saturating_add(cell.symbol().width() as u16), y));
    }
    if current_color != Color::Reset {
        queue!(output, ResetColor)?;
    }
    output.flush()
}

impl Drop for TerminalRenderer {
    fn drop(&mut self) {
        let _ = self.cleanup();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn centered_text_uses_display_cells_instead_of_bytes_or_scalars() {
        assert_eq!(centered_column(20, "按空格键开始".width()), 4);
        assert_eq!(centered_column(20, "e\u{301}".width()), 9);
        assert_eq!(centered_column(4, "按空格键开始".width()), 0);
    }

    #[test]
    fn translated_lines_keep_graphemes_and_clip_whole_wide_characters() {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 7, 1));
        write_line(&mut buffer, 0, 0, "中e\u{301}文AB", Color::Cyan);
        assert_eq!(buffer[(0, 0)].symbol(), "中");
        assert_eq!(buffer[(2, 0)].symbol(), "e\u{301}");
        assert_eq!(buffer[(3, 0)].symbol(), "文");
        assert_eq!(buffer[(5, 0)].symbol(), "A");
        assert_eq!(buffer[(6, 0)].symbol(), "B");

        buffer.reset();
        write_line(&mut buffer, 0, 0, "中文中文", Color::Cyan);
        assert_eq!(buffer[(4, 0)].symbol(), "中");
        assert_eq!(buffer[(6, 0)].symbol(), " ");
        write_line(&mut buffer, u16::MAX, u16::MAX, "outside", Color::Red);
    }

    #[test]
    fn differential_output_skips_wide_tails_and_clears_old_text() {
        let empty = Buffer::empty(Rect::new(0, 0, 8, 1));
        let mut chinese = empty.clone();
        write_line(&mut chinese, 0, 0, "中文A", Color::Cyan);
        let mut output = Vec::new();
        write_changed_cells(&mut output, &empty, &chinese).unwrap();
        let output = String::from_utf8(output).unwrap();
        assert!(output.contains("中文A"));
        assert_eq!(output.matches("\x1b[1;1H").count(), 1);

        let mut english = empty.clone();
        write_line(&mut english, 0, 0, "OK", Color::Cyan);
        let updates = chinese.diff(&english);
        assert!(
            updates
                .iter()
                .any(|(x, _, cell)| *x == 4 && cell.symbol() == " ")
        );
        let mut output = Vec::new();
        write_changed_cells(&mut output, &english, &english).unwrap();
        assert!(output.is_empty());
    }

    #[test]
    fn terminal_size_validation_accepts_the_boundary_and_larger_sizes() {
        assert!(validate_terminal_size(MIN_TERMINAL_WIDTH, MIN_TERMINAL_HEIGHT).is_ok());
        assert!(validate_terminal_size(u16::MAX, u16::MAX).is_ok());
    }

    #[test]
    fn terminal_size_validation_rejects_each_undersized_dimension() {
        for (width, height) in [
            (MIN_TERMINAL_WIDTH - 1, MIN_TERMINAL_HEIGHT),
            (MIN_TERMINAL_WIDTH, MIN_TERMINAL_HEIGHT - 1),
            (MIN_TERMINAL_WIDTH - 1, MIN_TERMINAL_HEIGHT - 1),
        ] {
            assert!(matches!(
                validate_terminal_size(width, height),
                Err(TerminalError::TooSmall {
                    width: actual_width,
                    height: actual_height,
                    min_width: MIN_TERMINAL_WIDTH,
                    min_height: MIN_TERMINAL_HEIGHT,
                }) if actual_width == width && actual_height == height
            ));
        }
    }

    #[test]
    fn terminal_size_validation_honors_a_larger_embedded_shell_requirement() {
        assert!(matches!(
            validate_terminal_size_with_minimum(107, 20, 108, 20),
            Err(TerminalError::TooSmall {
                width: 107,
                height: 20,
                min_width: 108,
                min_height: 20,
            })
        ));
        assert!(validate_terminal_size_with_minimum(108, 20, 108, 20).is_ok());
    }

    #[test]
    fn terminal_size_validation_rejects_requirements_above_renderer_capacity() {
        assert!(matches!(
            validate_terminal_size_with_minimum(1200, 20, 1200, 20),
            Err(TerminalError::RequirementTooLarge {
                min_width: 1200,
                min_height: 20,
                max_width: MAX_TERMINAL_WIDTH,
                max_height: MAX_TERMINAL_HEIGHT,
            })
        ));
    }

    #[test]
    fn terminal_too_small_message_is_one_actionable_line() {
        let error = TerminalError::TooSmall {
            width: 69,
            height: 19,
            min_width: MIN_TERMINAL_WIDTH,
            min_height: MIN_TERMINAL_HEIGHT,
        };
        let display = error.to_string();
        let message = error.user_friendly_message();

        for line in [display, message] {
            assert_eq!(line.lines().count(), 1);
            assert!(line.contains("69x19"));
            assert!(line.contains("70x20"));
            assert!(line.contains("resize"));
        }
    }
}
