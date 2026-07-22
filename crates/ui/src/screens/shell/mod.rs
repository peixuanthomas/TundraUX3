mod layout;
mod model;
mod render;

pub use layout::{
    MIN_SHELL_TERMINAL_HEIGHT, MIN_SHELL_TERMINAL_WIDTH, ShellLayout, compute_shell_layout,
};
pub(crate) use layout::{centered_rect, inset_rect, line_in_rect, rect_contains, usize_to_u16};
pub use model::{
    ExitConfirmViewModel, ShellChromeViewModel, StatusViewModel, TimeSyncDialogViewModel,
};
pub(crate) use render::{fit_cell, render_compact_home, render_status, render_top};
pub use render::{
    render_editor_app, render_exit_confirmation, render_time_sync_failure_dialog,
    status_time_button_area,
};
