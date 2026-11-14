//! Host-terminal setup and isolated child terminals shared by Shell and AutoAdmin.
//! Screen navigation and application state remain in their callers.
mod ansi;
mod emulator;
pub mod input;
mod input_queue;
pub mod pty;
pub mod session;
pub mod snapshot;
pub use ansi::ansi_foreground;
#[cfg(unix)]
pub use platform::pty_exec::{run_pty_exec_shim, set_pty_exec_gate_directory};
pub use session::{
    TerminalGuard, detect_terminal_graphics_protocol, probe_terminal_graphics_protocol,
    restore_terminal_best_effort,
};

/// Dispatches the internal Unix PTY launch before the normal executable entry point.
#[cfg(not(unix))]
pub fn run_pty_exec_shim() -> Option<std::io::Error> {
    None
}
