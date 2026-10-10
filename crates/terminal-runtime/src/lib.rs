//! Host-terminal setup and isolated child terminals shared by Shell and AutoAdmin.
//! Screen navigation and application state remain in their callers.
mod ansi;
pub mod input;
pub mod pty;
pub mod session;
pub mod snapshot;
pub use ansi::ansi_foreground;
pub use session::{
    TerminalGuard, detect_terminal_graphics_protocol, probe_terminal_graphics_protocol,
    restore_terminal_best_effort,
};
