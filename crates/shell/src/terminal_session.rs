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
use std::io::{self, Write};

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
/// the detected inline graphics protocol label. Non-interactive stdio and
/// unanswered handshakes are reported as probe errors rather than as explicit
/// text-only support.
pub fn detect_terminal_graphics_protocol() -> Result<Option<&'static str>, String> {
    match probe_terminal_graphics_protocol().status() {
        ui::TerminalGraphicsProbeStatus::Verified(protocol) => Ok(Some(protocol.label())),
        ui::TerminalGraphicsProbeStatus::Unsupported => Ok(None),
        ui::TerminalGraphicsProbeStatus::NoResponse { reason } => Err(reason.clone()),
    }
}

/// Performs the process-level terminal graphics handshake. Callers must ensure
/// that no other thread is reading terminal events until this function
/// returns.
pub fn probe_terminal_graphics_protocol() -> ui::TerminalGraphicsProbe {
    let mut raw_mode = match TemporaryRawMode::enter() {
        Ok(raw_mode) => raw_mode,
        Err(error) => return ui::TerminalGraphicsProbe::no_response(error),
    };
    let capabilities = platform::probe_terminal_graphics_capabilities();
    let detected = map_terminal_graphics_capabilities(capabilities);
    match raw_mode.restore() {
        Ok(()) => detected,
        Err(error) => {
            let restore_error =
                format!("could not restore terminal mode after graphics capability probe: {error}");
            match detected.status() {
                ui::TerminalGraphicsProbeStatus::NoResponse { reason } => {
                    ui::TerminalGraphicsProbe::no_response(format!("{reason}; {restore_error}"))
                }
                _ => ui::TerminalGraphicsProbe::no_response(restore_error),
            }
        }
    }
}

fn map_terminal_graphics_capabilities(
    capabilities: platform::TerminalGraphicsCapabilities,
) -> ui::TerminalGraphicsProbe {
    let text_sizing_protocol = capabilities.text_sizing_protocol;
    match capabilities.status {
        platform::TerminalGraphicsProbeStatus::Verified(protocol) => {
            let protocol = match protocol {
                platform::TerminalGraphicsProtocol::Kitty => ui::EditorGraphicsProtocol::Kitty,
                platform::TerminalGraphicsProtocol::Sixel => ui::EditorGraphicsProtocol::Sixel,
                platform::TerminalGraphicsProtocol::Iterm2 => ui::EditorGraphicsProtocol::Iterm2,
            };
            let cell_size = capabilities
                .cell_size
                .unwrap_or(platform::TerminalCellSize {
                    width: 10,
                    height: 20,
                });
            ui::TerminalGraphicsProbe::from_terminal_capabilities(
                protocol,
                cell_size.width,
                cell_size.height,
                capabilities.is_tmux,
                text_sizing_protocol,
            )
        }
        platform::TerminalGraphicsProbeStatus::Unsupported => {
            ui::TerminalGraphicsProbe::unsupported().with_text_sizing_protocol(text_sizing_protocol)
        }
        platform::TerminalGraphicsProbeStatus::NoResponse { reason } => {
            ui::TerminalGraphicsProbe::no_response(reason)
                .with_text_sizing_protocol(text_sizing_protocol)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn platform_capabilities_map_explicitly_to_ui_probe() {
        let probe = map_terminal_graphics_capabilities(platform::TerminalGraphicsCapabilities {
            status: platform::TerminalGraphicsProbeStatus::Verified(
                platform::TerminalGraphicsProtocol::Sixel,
            ),
            cell_size: Some(platform::TerminalCellSize {
                width: 8,
                height: 16,
            }),
            is_tmux: true,
            text_sizing_protocol: true,
        });
        assert_eq!(
            probe.status(),
            &ui::TerminalGraphicsProbeStatus::Verified(ui::EditorGraphicsProtocol::Sixel)
        );
        assert!(probe.picker().is_some());
        assert!(probe.text_sizing_protocol());
        let prepared = probe
            .picker()
            .unwrap()
            .prepare_rgba(
                100,
                100,
                vec![255; 100 * 100 * 4],
                ratatui::layout::Rect::new(0, 0, 40, 40),
            )
            .unwrap();
        assert_eq!(prepared.render_size(), ratatui::layout::Size::new(13, 7));

        let unsupported =
            map_terminal_graphics_capabilities(platform::TerminalGraphicsCapabilities {
                status: platform::TerminalGraphicsProbeStatus::Unsupported,
                cell_size: None,
                is_tmux: false,
                text_sizing_protocol: true,
            });
        assert_eq!(
            unsupported.status(),
            &ui::TerminalGraphicsProbeStatus::Unsupported
        );
        assert!(unsupported.text_sizing_protocol());

        let no_response =
            map_terminal_graphics_capabilities(platform::TerminalGraphicsCapabilities {
                status: platform::TerminalGraphicsProbeStatus::NoResponse {
                    reason: "timed out".to_string(),
                },
                cell_size: None,
                is_tmux: false,
                text_sizing_protocol: false,
            });
        assert_eq!(
            no_response.status(),
            &ui::TerminalGraphicsProbeStatus::NoResponse {
                reason: "timed out".to_string(),
            }
        );
    }
}
