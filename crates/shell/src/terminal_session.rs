use crossterm::cursor::{Hide, Show};
use crossterm::event::{
    DisableBracketedPaste, DisableFocusChange, DisableMouseCapture, EnableBracketedPaste,
    EnableFocusChange, EnableMouseCapture,
};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
    is_raw_mode_enabled,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use std::io::{self, IsTerminal, Write};

pub struct TerminalGuard<W: Write> {
    terminal: Terminal<CrosstermBackend<W>>,
    restored: bool,
}

impl<W: Write> TerminalGuard<W> {
    pub fn enter(output: W) -> io::Result<Self> {
        // Constructing `Terminal` probes the backend size and can fail. Do it
        // before changing any terminal mode so that this error path needs no
        // emergency cleanup.
        let mut terminal = Terminal::new(CrosstermBackend::new(output))?;
        enable_raw_mode()?;
        if let Err(error) = execute!(
            terminal.backend_mut(),
            EnterAlternateScreen,
            EnableMouseCapture,
            EnableFocusChange,
            EnableBracketedPaste,
            Hide
        ) {
            let _ = execute!(
                terminal.backend_mut(),
                Show,
                DisableBracketedPaste,
                DisableFocusChange,
                DisableMouseCapture,
                LeaveAlternateScreen
            );
            let _ = disable_raw_mode();
            return Err(error);
        }

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

        let terminal_result = execute!(
            self.terminal.backend_mut(),
            Show,
            DisableBracketedPaste,
            DisableFocusChange,
            DisableMouseCapture,
            LeaveAlternateScreen
        );
        let raw_mode_result = disable_raw_mode();
        self.restored = true;

        terminal_result.and(raw_mode_result)
    }

    /// Re-enters the full-screen terminal after a temporary restore, such as
    /// when an interactive power authorization was cancelled.
    pub fn resume(&mut self) -> io::Result<()> {
        if !self.restored {
            return Ok(());
        }

        enable_raw_mode()?;
        if let Err(error) = execute!(
            self.terminal.backend_mut(),
            EnterAlternateScreen,
            EnableMouseCapture,
            EnableFocusChange,
            EnableBracketedPaste,
            Hide
        ) {
            let _ = execute!(
                self.terminal.backend_mut(),
                Show,
                DisableBracketedPaste,
                DisableFocusChange,
                DisableMouseCapture,
                LeaveAlternateScreen
            );
            let _ = disable_raw_mode();
            return Err(error);
        }
        self.restored = false;
        self.terminal.clear()?;
        Ok(())
    }
}

impl<W: Write> Drop for TerminalGuard<W> {
    fn drop(&mut self) {
        let _ = self.restore();
    }
}

/// Uses the same live terminal query as the Shell image renderer and returns
/// the detected inline graphics protocol label. Non-interactive stdio cannot
/// be queried safely and is treated as text-only.
pub fn detect_terminal_graphics_protocol() -> Result<Option<&'static str>, String> {
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        return Ok(None);
    }

    let mut raw_mode = TemporaryRawMode::enter()?;
    let detected = ui::EditorImagePicker::detect_stdio()
        .map(|picker| picker.map(|picker| picker.protocol().label()))
        .map_err(|error| error.to_string());
    let restored = raw_mode.restore().map_err(|error| {
        format!("could not restore terminal mode after graphics capability probe: {error}")
    });

    match (detected, restored) {
        (Ok(protocol), Ok(())) => Ok(protocol),
        (Err(error), Ok(())) => Err(error),
        (Ok(_), Err(error)) => Err(error),
        (Err(detection_error), Err(restore_error)) => {
            Err(format!("{detection_error}; {restore_error}"))
        }
    }
}

struct TemporaryRawMode {
    enabled_here: bool,
}

impl TemporaryRawMode {
    fn enter() -> Result<Self, String> {
        let was_enabled = is_raw_mode_enabled()
            .map_err(|error| format!("could not inspect terminal raw mode: {error}"))?;
        if !was_enabled {
            enable_raw_mode()
                .map_err(|error| format!("could not enable terminal raw mode: {error}"))?;
        }
        Ok(Self {
            enabled_here: !was_enabled,
        })
    }

    fn restore(&mut self) -> io::Result<()> {
        if !self.enabled_here {
            return Ok(());
        }
        disable_raw_mode()?;
        self.enabled_here = false;
        Ok(())
    }
}

impl Drop for TemporaryRawMode {
    fn drop(&mut self) {
        let _ = self.restore();
    }
}

pub fn restore_terminal_best_effort() {
    let _ = disable_raw_mode();
    let mut stderr = io::stderr();
    let _ = execute!(
        stderr,
        Show,
        DisableBracketedPaste,
        DisableFocusChange,
        DisableMouseCapture,
        LeaveAlternateScreen
    );
}
