use std::env;
use std::io::{self, Write};
use std::sync::atomic::{AtomicBool, Ordering};

use crate::PlatformKind;
use crate::diagnostics::{CheckStatus, EnvironmentCheck};

pub const ENTER_FULLSCREEN_SEQUENCE: &str = "\x1B[?1049h\x1B[?25l\x1B[2J\x1B[H";
pub const EXIT_FULLSCREEN_SEQUENCE: &str = "\x1B[?25h\x1B[?1049l";

static TERMINAL_RUNNING: AtomicBool = AtomicBool::new(true);

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
    match kind {
        PlatformKind::Windows => {
            if is_windows_terminal_session(wt_session) {
                EnvironmentCheck {
                    label: "Terminal".to_string(),
                    status: CheckStatus::Pass,
                    message: "Windows Terminal detected".to_string(),
                }
            } else {
                EnvironmentCheck {
                    label: "Terminal".to_string(),
                    status: CheckStatus::Warning,
                    message: "Windows Terminal not detected; conhost is best-effort only"
                        .to_string(),
                }
            }
        }
        PlatformKind::Macos => EnvironmentCheck {
            label: "Terminal".to_string(),
            status: CheckStatus::Pass,
            message: "macOS terminal session supported".to_string(),
        },
        PlatformKind::Unsupported => EnvironmentCheck {
            label: "Terminal".to_string(),
            status: CheckStatus::Warning,
            message: "terminal support is unsupported on this platform".to_string(),
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
}

impl TerminalControlHandler {
    pub fn install() -> Self {
        TERMINAL_RUNNING.store(true, Ordering::SeqCst);

        #[cfg(windows)]
        {
            let installed =
                unsafe { SetConsoleCtrlHandler(Some(handle_console_control), true.into()) != 0 };

            Self { installed }
        }

        #[cfg(not(windows))]
        {
            Self {}
        }
    }

    pub fn shutdown_requested(&self) -> bool {
        !TERMINAL_RUNNING.load(Ordering::SeqCst)
    }
}

#[cfg(windows)]
impl Drop for TerminalControlHandler {
    fn drop(&mut self) {
        if self.installed {
            unsafe {
                SetConsoleCtrlHandler(Some(handle_console_control), false.into());
            }
        }
    }
}

#[cfg(windows)]
unsafe extern "system" fn handle_console_control(control_type: u32) -> i32 {
    match control_type {
        CTRL_C_EVENT | CTRL_BREAK_EVENT | CTRL_CLOSE_EVENT | CTRL_LOGOFF_EVENT
        | CTRL_SHUTDOWN_EVENT => {
            TERMINAL_RUNNING.store(false, Ordering::SeqCst);
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
