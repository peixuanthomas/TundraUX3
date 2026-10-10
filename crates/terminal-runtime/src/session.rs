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

#[cfg(unix)]
static KEYBOARD_MODES: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);
#[cfg(unix)]
const KITTY_MODE: u8 = 1;
#[cfg(unix)]
const MODIFY_KEYS_MODE: u8 = 2;
#[cfg(unix)]
const WIN32_MODE: u8 = 4;

/// Shared text-only capability detection; image protocol probing is opt-in.
pub fn text_render_capabilities() -> ui::RenderCapabilities {
    let true_color = std::env::var("COLORTERM").is_ok_and(|value| {
        value.eq_ignore_ascii_case("truecolor") || value.eq_ignore_ascii_case("24bit")
    }) || std::env::var("TERM").is_ok_and(|value| {
        let value = value.to_ascii_lowercase();
        value.contains("truecolor") || value.contains("direct")
    }) || std::env::var_os("WT_SESSION").is_some();
    ui::RenderCapabilities {
        color: if true_color {
            ui::ColorCapability::TrueColor
        } else {
            ui::ColorCapability::Ansi
        },
        image_protocol: false,
    }
}

pub struct TerminalGuard<W: Write> {
    terminal: Terminal<CrosstermBackend<W>>,
    restored: bool,
    #[cfg(unix)]
    keyboard_reporting: bool,
    #[cfg(unix)]
    modified_keys: bool,
    #[cfg(unix)]
    win32_input: bool,
    keyboard_reporting_requested: bool,
}

impl<W: Write> TerminalGuard<W> {
    pub fn enter(output: W) -> io::Result<Self> {
        crossterm::event::set_input_error_handler(log_discarded_terminal_input);
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
            #[cfg(unix)]
            keyboard_reporting: false,
            #[cfg(unix)]
            modified_keys: false,
            #[cfg(unix)]
            win32_input: false,
            keyboard_reporting_requested: false,
        })
    }

    pub fn terminal_mut(&mut self) -> &mut Terminal<CrosstermBackend<W>> {
        &mut self.terminal
    }

    /// Preserve modified Enter and Shift+Control letters before decoding input.
    pub fn enable_keyboard_reporting(&mut self) -> io::Result<bool> {
        self.keyboard_reporting_requested = true;
        #[cfg(unix)]
        {
            use crossterm::event::{
                KeyboardEnhancementFlags as Flags, PushKeyboardEnhancementFlags,
            };
            // Windows Terminal/ConPTY can retain the original virtual key for
            // WSL. Legacy VT alone encodes both Ctrl+Enter and Ctrl+J as LF.
            if !self.win32_input && windows_terminal_input_available() {
                self.win32_input = true;
                KEYBOARD_MODES.fetch_or(WIN32_MODE, std::sync::atomic::Ordering::Relaxed);
                self.terminal.backend_mut().write_all(b"\x1b[?9001h")?;
                self.terminal.backend_mut().flush()?;
            }
            if self.win32_input {
                return Ok(true);
            }
            if !self.keyboard_reporting
                && crossterm::terminal::supports_keyboard_enhancement().unwrap_or(false)
            {
                // Set this before writing so Drop also restores a partial write.
                self.keyboard_reporting = true;
                KEYBOARD_MODES.fetch_or(KITTY_MODE, std::sync::atomic::Ordering::Relaxed);
                execute!(
                    self.terminal.backend_mut(),
                    PushKeyboardEnhancementFlags(
                        Flags::DISAMBIGUATE_ESCAPE_CODES
                            | Flags::REPORT_EVENT_TYPES
                            | Flags::REPORT_ALL_KEYS_AS_ESCAPE_CODES
                    )
                )?;
            }
            if !self.keyboard_reporting && !self.modified_keys {
                // xterm-compatible terminals without Kitty keyboard reporting.
                // Unsupported terminals ignore this request; never relabel a
                // legacy Ctrl+J or Ctrl+X as a different physical key.
                self.modified_keys = true;
                KEYBOARD_MODES.fetch_or(MODIFY_KEYS_MODE, std::sync::atomic::Ordering::Relaxed);
                self.terminal.backend_mut().write_all(b"\x1b[>4;2m")?;
                self.terminal.backend_mut().flush()?;
            }
            Ok(self.keyboard_reporting)
        }
        #[cfg(not(unix))]
        {
            // The Windows console already supplies key-up records.
            Ok(cfg!(windows))
        }
    }

    pub fn restore(&mut self) -> io::Result<()> {
        if self.restored {
            return Ok(());
        }

        #[cfg(unix)]
        let keyboard_result = if std::mem::take(&mut self.keyboard_reporting)
            && KEYBOARD_MODES.fetch_and(!KITTY_MODE, std::sync::atomic::Ordering::Relaxed)
                & KITTY_MODE
                != 0
        {
            execute!(
                self.terminal.backend_mut(),
                crossterm::event::PopKeyboardEnhancementFlags
            )
        } else {
            Ok(())
        };
        #[cfg(not(unix))]
        let keyboard_result = Ok(());
        #[cfg(unix)]
        let modified_result = if std::mem::take(&mut self.modified_keys)
            && KEYBOARD_MODES.fetch_and(!MODIFY_KEYS_MODE, std::sync::atomic::Ordering::Relaxed)
                & MODIFY_KEYS_MODE
                != 0
        {
            self.terminal.backend_mut().write_all(b"\x1b[>4;0m")
        } else {
            Ok(())
        };
        #[cfg(not(unix))]
        let modified_result = Ok(());
        #[cfg(unix)]
        let win32_result = if std::mem::take(&mut self.win32_input)
            && KEYBOARD_MODES.fetch_and(!WIN32_MODE, std::sync::atomic::Ordering::Relaxed)
                & WIN32_MODE
                != 0
        {
            self.terminal.backend_mut().write_all(b"\x1b[?9001l")
        } else {
            Ok(())
        };
        #[cfg(not(unix))]
        let win32_result = Ok(());
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
        #[cfg(unix)]
        KEYBOARD_MODES.store(0, std::sync::atomic::Ordering::Relaxed);

        keyboard_result
            .and(modified_result)
            .and(win32_result)
            .and(terminal_result)
            .and(raw_mode_result)
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
        if self.keyboard_reporting_requested {
            self.enable_keyboard_reporting()?;
        }
        self.terminal.clear()?;
        Ok(())
    }
}

#[cfg(unix)]
fn windows_terminal_input_available() -> bool {
    (std::env::var_os("WT_SESSION").is_some()
        || std::env::var("TERM_PROGRAM").is_ok_and(|program| program == "Windows_Terminal"))
        && std::env::var_os("TMUX").is_none()
        && std::env::var_os("STY").is_none()
}

fn discarded_terminal_input_log(
    error: crossterm::event::InputError,
) -> runtime_log::RuntimeLogEvent {
    let context = watchdog::ProcessWatchdog::global()
        .map(|process| process.log_context("ux.terminal.input", "discard_report"))
        .unwrap_or_else(|| runtime_log::LogContext {
            app: "tundra-shell".into(),
            module: "ux.terminal.input".into(),
            operation: "discard_report".into(),
            ..Default::default()
        });
    let code = match error.kind {
        crossterm::event::InputErrorKind::Malformed => "UX_TERMINAL_INPUT_MALFORMED",
        crossterm::event::InputErrorKind::Incomplete => "UX_TERMINAL_INPUT_INCOMPLETE",
        crossterm::event::InputErrorKind::TooLong => "UX_TERMINAL_INPUT_TOO_LONG",
    };
    let mut event = runtime_log::RuntimeLogEvent::new(
        context,
        runtime_log::LogLevel::Warning,
        runtime_log::LogPhase::Degraded,
        format!(
            "Discarded terminal input report: {:?}; protocol={}; buffered_bytes={}",
            error.kind, error.protocol, error.buffered_bytes
        ),
    );
    event.error_code = Some(code.into());
    event.alert_key = Some(code.into());
    event
}

fn log_discarded_terminal_input(error: crossterm::event::InputError) {
    // The existing logger queues writes and coalesces repeated warning keys.
    // Never include terminal bytes: they may contain authentication or paste data.
    runtime_log::record(discarded_terminal_input_log(error));
}

#[cfg(test)]
#[path = "../tests/unit/terminal_input_logging.rs"]
mod terminal_input_logging_tests;

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

pub struct TemporaryRawMode {
    enabled_here: bool,
}

impl TemporaryRawMode {
    pub fn enter() -> Result<Self, String> {
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

    pub fn restore(&mut self) -> io::Result<()> {
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
    #[cfg(unix)]
    {
        let modes = KEYBOARD_MODES.swap(0, std::sync::atomic::Ordering::Relaxed);
        if modes & WIN32_MODE != 0 {
            let _ = stderr.write_all(b"\x1b[?9001l");
        }
        if modes & MODIFY_KEYS_MODE != 0 {
            let _ = stderr.write_all(b"\x1b[>4;0m");
        }
        if modes & KITTY_MODE != 0 {
            let _ = stderr.write_all(b"\x1b[<u");
        }
    }
    let _ = execute!(
        stderr,
        Show,
        DisableBracketedPaste,
        DisableFocusChange,
        DisableMouseCapture,
        LeaveAlternateScreen
    );
}

#[cfg(all(test, unix))]
#[path = "../tests/unit/terminal_keyboard_pty.rs"]
mod terminal_keyboard_pty_tests;
