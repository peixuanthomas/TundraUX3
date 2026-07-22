use crossterm::cursor::{Hide, Show};
use crossterm::event::{DisableMouseCapture, EnableMouseCapture};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use std::io::{self, Write};

pub struct TerminalGuard<W: Write> {
    terminal: Terminal<CrosstermBackend<W>>,
    restored: bool,
}

impl<W: Write> TerminalGuard<W> {
    pub fn enter(mut output: W) -> io::Result<Self> {
        enable_raw_mode()?;
        if let Err(error) = execute!(output, EnterAlternateScreen, EnableMouseCapture, Hide) {
            let _ = disable_raw_mode();
            return Err(error);
        }

        let terminal = Terminal::new(CrosstermBackend::new(output))?;

        Ok(Self {
            terminal,
            restored: false,
        })
    }

    pub fn terminal_mut(&mut self) -> &mut Terminal<CrosstermBackend<W>> {
        &mut self.terminal
    }

    pub fn restore(&mut self) -> io::Result<()> {
        if self.restored {
            return Ok(());
        }

        execute!(
            self.terminal.backend_mut(),
            Show,
            DisableMouseCapture,
            LeaveAlternateScreen
        )?;
        disable_raw_mode()?;
        self.restored = true;

        Ok(())
    }

    /// Prevents the drop guard from restoring the terminal after Windows has
    /// accepted an immediate system power-off request.
    pub fn skip_restore(&mut self) {
        self.restored = true;
    }
}

impl<W: Write> Drop for TerminalGuard<W> {
    fn drop(&mut self) {
        let _ = self.restore();
    }
}

pub fn restore_terminal_best_effort() {
    let _ = disable_raw_mode();
    let mut stderr = io::stderr();
    let _ = execute!(stderr, Show, DisableMouseCapture, LeaveAlternateScreen);
}
