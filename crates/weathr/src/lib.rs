pub mod animation;
pub mod animation_manager;
pub mod app;
pub mod app_state;
mod assets;
mod embedded;
pub mod error;
pub use embedded::EmbeddedDisplay;
mod launch;
mod localization;
pub mod render;
pub mod scene;
pub mod theme;
pub use localization::LocalizationProvider;

pub use launch::{
    ClockFormat, DisplayControl, ExitSemantic, ShellLockscreenResult, WeathrDisplayInput,
    WeathrRunError, restore_terminal_best_effort, run_display, run_display_blocking,
    run_display_blocking_with_control,
};
