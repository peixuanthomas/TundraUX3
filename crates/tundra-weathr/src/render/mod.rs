mod capabilities;
pub mod clock;

use crate::error::TerminalError;
use capabilities::TerminalCapabilities;
use crossterm::{
    cursor, execute, queue,
    style::{Color, Print, ResetColor, SetForegroundColor},
    terminal::{self, Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen},
};
use std::io::{self, BufWriter, IsTerminal, Stdout, Write};

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

#[derive(Clone, Copy, PartialEq, Eq)]
struct Cell {
    character: char,
    color: Color,
}

impl Default for Cell {
    fn default() -> Self {
        Self {
            character: ' ',
            color: Color::Reset,
        }
    }
}

pub struct TerminalRenderer {
    stdout: BufWriter<Stdout>,
    width: u16,
    height: u16,
    buffer: Vec<Cell>,
    last_buffer: Vec<Cell>,
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
        let buffer_size = (width as usize) * (height as usize);
        let capabilities = TerminalCapabilities::detect();

        Ok(Self {
            stdout,
            width,
            height,
            buffer: vec![Cell::default(); buffer_size],
            last_buffer: vec![Cell::default(); buffer_size],
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
            let buffer_size = (width as usize) * (height as usize);
            self.buffer = vec![Cell::default(); buffer_size];
            self.last_buffer = vec![Cell::default(); buffer_size];
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
        self.buffer.fill(Cell::default());
        Ok(())
    }

    pub fn render_centered_colored(
        &mut self,
        lines: &[String],
        start_row: u16,
        color: Color,
    ) -> io::Result<()> {
        let max_width = lines.iter().map(|l| l.len()).max().unwrap_or(0);
        let start_col = if self.width as usize > max_width {
            (self.width as usize - max_width) / 2
        } else {
            0
        };
        let adjusted_color = self.capabilities.adjust_color(color);

        for (idx, line) in lines.iter().enumerate() {
            let row = start_row + idx as u16;
            if row < self.height {
                for (char_idx, ch) in line.chars().enumerate() {
                    let col = start_col as u16 + char_idx as u16;
                    if col < self.width {
                        let buffer_idx = (row as usize) * (self.width as usize) + (col as usize);
                        if buffer_idx < self.buffer.len() {
                            self.buffer[buffer_idx] = Cell {
                                character: ch,
                                color: adjusted_color,
                            };
                        }
                    }
                }
            }
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
        if y >= self.height {
            return Ok(());
        }
        let adjusted_color = self.capabilities.adjust_color(color);

        for (idx, ch) in text.chars().enumerate() {
            let col = x + idx as u16;
            if col < self.width {
                let buffer_idx = (y as usize) * (self.width as usize) + (col as usize);
                if buffer_idx < self.buffer.len() {
                    self.buffer[buffer_idx] = Cell {
                        character: ch,
                        color: adjusted_color,
                    };
                }
            }
        }
        Ok(())
    }

    pub fn render_char(&mut self, x: u16, y: u16, ch: char, color: Color) -> io::Result<()> {
        if x < self.width && y < self.height {
            let buffer_idx = (y as usize) * (self.width as usize) + (x as usize);
            if buffer_idx < self.buffer.len() {
                self.buffer[buffer_idx] = Cell {
                    character: ch,
                    color: self.capabilities.adjust_color(color),
                };
            }
        }
        Ok(())
    }

    pub fn flash_screen(&mut self) -> io::Result<()> {
        let flash_color = self.capabilities.adjust_color(Color::White);
        for cell in &mut self.buffer {
            cell.color = flash_color;
        }
        Ok(())
    }

    pub fn flush(&mut self) -> io::Result<()> {
        let result = self.flush_changed_cells();
        self.last_buffer_valid = result.is_ok();
        result
    }

    fn flush_changed_cells(&mut self) -> io::Result<()> {
        let mut current_color = Color::Reset;
        let mut last_pos: Option<(u16, u16)> = None;
        let width = usize::from(self.width);
        let redraw_all = !self.last_buffer_valid;

        if redraw_all {
            queue!(self.stdout, ResetColor)?;
        }

        debug_assert_eq!(self.buffer.len(), self.last_buffer.len());
        for (y, (row, last_row)) in self
            .buffer
            .chunks_exact(width)
            .zip(self.last_buffer.chunks_exact_mut(width))
            .enumerate()
        {
            for (x, (&cell, last_cell)) in row.iter().zip(last_row).enumerate() {
                if redraw_all || cell != *last_cell {
                    let x = x as u16;
                    let y = y as u16;
                    let expected_pos = last_pos.map(|(lx, ly)| (lx + 1, ly));
                    if expected_pos != Some((x, y)) {
                        queue!(self.stdout, cursor::MoveTo(x, y))?;
                    }

                    if cell.color != current_color {
                        queue!(self.stdout, SetForegroundColor(cell.color))?;
                        current_color = cell.color;
                    }

                    queue!(self.stdout, Print(cell.character))?;
                    last_pos = Some((x, y));
                    *last_cell = cell;
                }
            }
        }

        if current_color != Color::Reset {
            queue!(self.stdout, ResetColor)?;
        }

        self.stdout.flush()?;
        Ok(())
    }
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
