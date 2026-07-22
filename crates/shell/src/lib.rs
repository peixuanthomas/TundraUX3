mod clock_scheduler;

use std::time::Duration;

pub use platform::{ENTER_FULLSCREEN_SEQUENCE, EXIT_FULLSCREEN_SEQUENCE};
pub use time::TIME_SYNC_INTERVAL;

pub const BANNER_ENTER_DURATION: Duration = Duration::from_millis(720);
pub const BANNER_HOLD_DURATION: Duration = Duration::from_secs(2);
pub const BANNER_EXIT_DURATION: Duration = Duration::from_millis(560);
pub const BANNER_DISPLAY_DURATION: Duration = Duration::from_secs(2);
pub const LOGIN_IDLE_TIMEOUT: Duration = Duration::from_secs(60);
pub const PASSWORD_REVEAL_DURATION: Duration = Duration::from_secs(5);
const BANNER_ASSET_KEY: &str = "tundraux3";

// Public models and low-coupling services live in regular modules. Re-exports
// preserve the crate-root API used by the binary and integration tests.
mod banner;
mod first_run_banner;
mod input_events;
mod launch_args;
mod notification_center;
mod shell_commands;
mod shell_components;
mod shortcuts;
mod startup;
mod startup_banner;
mod terminal_events;
mod terminal_session;
mod terminal_size;

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
pub use terminal_session::{TerminalGuard, restore_terminal_best_effort};
pub use terminal_size::{ShellTerminalSizeError, ShellTerminalSizeRequirement};

pub(crate) use banner::asset_io_error;
pub(crate) use input_events::DOUBLE_CLICK_CELL_TOLERANCE;
pub(crate) use terminal_events::resets_login_idle_timeout;
#[cfg(test)]
pub(crate) use terminal_events::{key_event_to_label, mouse_event_to_input};
pub(crate) use terminal_size::checked_current_terminal_size;

mod session;

pub use session::*;
