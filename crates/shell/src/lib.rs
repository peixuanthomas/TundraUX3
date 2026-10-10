mod input;
mod previews;
pub(crate) use app::clock as clock_scheduler;
pub use auto_admin_preview::run_auto_admin_style_preview;
pub(crate) use previews::auto_admin as auto_admin_preview;
pub(crate) use previews::screen_keyboard;
pub use screen_keyboard::run_screen_keyboard;
pub use ui::AutoAdminPreviewStyle;

use std::time::Duration;

pub use platform::{ENTER_FULLSCREEN_SEQUENCE, EXIT_FULLSCREEN_SEQUENCE};
pub use time::TIME_SYNC_INTERVAL;

pub const BANNER_ENTER_DURATION: Duration = Duration::from_millis(720);
pub const BANNER_HOLD_DURATION: Duration = Duration::from_secs(2);
pub const BANNER_EXIT_DURATION: Duration = Duration::from_millis(560);
pub const BANNER_DISPLAY_DURATION: Duration = Duration::from_secs(2);
pub const LOGIN_IDLE_TIMEOUT: Duration = Duration::from_secs(60);
pub const PASSWORD_REVEAL_DURATION: Duration = Duration::from_secs(5);
/// Internal CLI-to-Shell request to exercise the real fullscreen panic boundary.
pub const COMMAND_LINE_PANIC_EXIT_CODE: u32 = 76;
const BANNER_ASSET_KEY: &str = "tundraux3";

// Public models and low-coupling services live in regular modules. Re-exports
// preserve the crate-root API used by the binary and integration tests.
pub(crate) use input::input_events;
pub(crate) use startup::banner;
pub(crate) use startup::first_run_banner;
pub(crate) use startup::launch_args;
pub(crate) use terminal_runtime::ansi_foreground;
mod notification_center;
pub(crate) use input::shell_commands;
pub(crate) use input::shell_components;
pub(crate) use input::shortcuts;
mod spring_style;
mod startup;
pub(crate) use input::terminal_events;
pub(crate) use previews::style as style_preview;
pub(crate) use startup::startup_banner;
pub(crate) use startup::terminal_size;
pub(crate) use terminal_runtime::session as terminal_session;

pub use style_preview::run_ui_style_preview;
pub use ui::style_preview::UiStyleVersion;

pub use banner::*;
pub use first_run_banner::*;
pub use input_events::*;
pub use launch_args::*;
pub use notification_center::*;
pub use shell_commands::*;
pub use shell_components::*;
pub use shortcuts::*;
pub use startup::*;
pub use startup_banner::*;
pub use terminal_events::crossterm_event_to_input;
pub use terminal_session::{
    TerminalGuard, detect_terminal_graphics_protocol, probe_terminal_graphics_protocol,
    restore_terminal_best_effort,
};
pub use terminal_size::{ShellTerminalSizeError, ShellTerminalSizeRequirement};

pub(crate) use banner::asset_io_error;
pub(crate) use input_events::DOUBLE_CLICK_CELL_TOLERANCE;
pub(crate) use terminal_events::resets_login_idle_timeout;
#[cfg(test)]
pub(crate) use terminal_events::{key_event_to_label, mouse_event_to_input};
pub(crate) use terminal_size::checked_current_terminal_size;

mod session;

pub use session::*;

#[cfg(target_os = "linux")]
pub use linux_startup::confirm_linux_startup;
#[cfg(target_os = "linux")]
pub(crate) use startup::linux_startup;
