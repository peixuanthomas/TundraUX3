#[cfg(not(windows))]
compile_error!("TundraUX3 phase 1 supports Windows 11 only.");

mod layout;
mod render;
mod theme;
mod view_model;

pub use layout::{ShellLayout, compute_shell_layout};
pub use render::{render_exit_confirmation, render_home};
pub use theme::TundraTheme;
pub use view_model::{
    DebugDiagnosticsViewModel, ExitConfirmViewModel, HomeDisplayMode, HomeViewModel,
    ShellChromeViewModel, ShellEntry, StatusViewModel,
};
