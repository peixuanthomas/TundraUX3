use std::env;
use std::io::{self, Write};
use std::sync::Arc;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::PlatformKind;
use crate::diagnostics::{CheckStatus, EnvironmentCheck};

pub const ENTER_FULLSCREEN_SEQUENCE: &str = "\x1B[?1049h\x1B[?25l\x1B[2J\x1B[H";
pub const EXIT_FULLSCREEN_SEQUENCE: &str = "\x1B[?25h\x1B[?1049l";

static TERMINAL_SHUTDOWN_REQUESTED: OnceLock<Arc<AtomicBool>> = OnceLock::new();

fn shared_shutdown_flag() -> &'static Arc<AtomicBool> {
    TERMINAL_SHUTDOWN_REQUESTED.get_or_init(|| Arc::new(AtomicBool::new(false)))
}

#[cfg(target_os = "linux")]
pub(crate) fn request_process_shutdown() {
    shared_shutdown_flag().store(true, Ordering::SeqCst);
}

pub fn with_terminal_fullscreen<W, T>(
    output: &mut W,
    body: impl FnOnce(&mut W) -> io::Result<T>,
) -> io::Result<T>
where
    W: Write,
{
    write!(output, "{ENTER_FULLSCREEN_SEQUENCE}")?;
    output.flush()?;

    let body_result = body(output);
    let exit_result = write!(output, "{EXIT_FULLSCREEN_SEQUENCE}").and_then(|_| output.flush());

    match (body_result, exit_result) {
        (Ok(value), Ok(())) => Ok(value),
        (Err(error), _) => Err(error),
        (Ok(_), Err(error)) => Err(error),
    }
}

pub fn terminal_environment_check(kind: PlatformKind) -> EnvironmentCheck {
    let wt_session = env::var("WT_SESSION").ok();
    terminal_environment_check_with(kind, wt_session.as_deref())
}

pub fn terminal_environment_check_with(
    kind: PlatformKind,
    wt_session: Option<&str>,
) -> EnvironmentCheck {
    terminal_environment_check_with_graphics_protocol(kind, wt_session, None)
}

/// Builds the terminal diagnostics result from an already-probed inline
/// graphics protocol. Merely identifying a terminal emulator is not enough:
/// the current UI requires capabilities such as native image rendering before
/// the terminal check can pass.
pub fn terminal_environment_check_with_graphics_protocol(
    kind: PlatformKind,
    wt_session: Option<&str>,
    graphics_protocol: Option<&str>,
) -> EnvironmentCheck {
    if let Some(protocol) = graphics_protocol.filter(|value| !value.trim().is_empty()) {
        return EnvironmentCheck {
            label: "Terminal".to_string(),
            status: CheckStatus::Pass,
            message: format!(
                "{} graphics protocol detected; image and advanced UI features are supported",
                protocol.trim()
            ),
        };
    }

    match kind {
        PlatformKind::Windows => {
            if is_windows_terminal_session(wt_session) {
                EnvironmentCheck {
                    label: "Terminal".to_string(),
                    status: CheckStatus::Warning,
                    message: "Windows Terminal detected, but no inline graphics protocol was detected; text-only UI is available"
                        .to_string(),
                }
            } else {
                EnvironmentCheck {
                    label: "Terminal".to_string(),
                    status: CheckStatus::Warning,
                    message: "No inline graphics protocol detected; this terminal is text-only and advanced UI features are unavailable"
                        .to_string(),
                }
            }
        }
        PlatformKind::Macos | PlatformKind::Linux => EnvironmentCheck {
            label: "Terminal".to_string(),
            status: CheckStatus::Warning,
            message: "No inline graphics protocol detected; this terminal is text-only and advanced UI features are unavailable"
                .to_string(),
        },
        PlatformKind::Unsupported => EnvironmentCheck {
            label: "Terminal".to_string(),
            status: CheckStatus::Warning,
            message: "No supported inline graphics protocol detected on this platform; only text UI can be assumed"
                .to_string(),
        },
    }
}

pub fn is_windows_terminal_session(wt_session: Option<&str>) -> bool {
    wt_session
        .map(|value| !value.trim().is_empty())
        .unwrap_or(false)
}

#[derive(Debug)]
pub struct TerminalControlHandler {
    #[cfg(windows)]
    installed: bool,
    #[cfg(unix)]
    signal_ids: Vec<signal_hook::SigId>,
}

impl TerminalControlHandler {
    pub fn install() -> Self {
        shared_shutdown_flag().store(false, Ordering::SeqCst);

        #[cfg(windows)]
        {
            let installed =
                unsafe { SetConsoleCtrlHandler(Some(handle_console_control), true.into()) != 0 };

            Self { installed }
        }

        #[cfg(unix)]
        {
            use signal_hook::consts::signal::{SIGHUP, SIGINT, SIGTERM};
            let signal_ids = [SIGINT, SIGTERM, SIGHUP]
                .into_iter()
                .filter_map(|signal| {
                    signal_hook::flag::register(signal, Arc::clone(shared_shutdown_flag())).ok()
                })
                .collect();
            Self { signal_ids }
        }

        #[cfg(not(any(windows, unix)))]
        {
            Self {}
        }
    }

    pub fn shutdown_requested(&self) -> bool {
        shared_shutdown_flag().load(Ordering::SeqCst)
    }

    /// Returns the process-wide termination source shared by the lock screen
    /// and the main Shell session.
    pub fn shutdown_flag(&self) -> Arc<AtomicBool> {
        Arc::clone(shared_shutdown_flag())
    }
}

impl Drop for TerminalControlHandler {
    fn drop(&mut self) {
        #[cfg(windows)]
        {
            if self.installed {
                unsafe {
                    SetConsoleCtrlHandler(Some(handle_console_control), false.into());
                }
            }
        }

        #[cfg(unix)]
        {
            for signal_id in self.signal_ids.drain(..) {
                signal_hook::low_level::unregister(signal_id);
            }
        }
    }
}

#[cfg(windows)]
unsafe extern "system" fn handle_console_control(control_type: u32) -> i32 {
    match control_type {
        CTRL_C_EVENT | CTRL_BREAK_EVENT | CTRL_CLOSE_EVENT | CTRL_LOGOFF_EVENT
        | CTRL_SHUTDOWN_EVENT => {
            if let Some(shutdown) = TERMINAL_SHUTDOWN_REQUESTED.get() {
                shutdown.store(true, Ordering::SeqCst);
            }
            true.into()
        }
        _ => false.into(),
    }
}

#[cfg(windows)]
const CTRL_C_EVENT: u32 = 0;
#[cfg(windows)]
const CTRL_BREAK_EVENT: u32 = 1;
#[cfg(windows)]
const CTRL_CLOSE_EVENT: u32 = 2;
#[cfg(windows)]
const CTRL_LOGOFF_EVENT: u32 = 5;
#[cfg(windows)]
const CTRL_SHUTDOWN_EVENT: u32 = 6;

#[cfg(windows)]
#[link(name = "kernel32")]
unsafe extern "system" {
    fn SetConsoleCtrlHandler(
        handler_routine: Option<unsafe extern "system" fn(u32) -> i32>,
        add: i32,
    ) -> i32;
}
