pub mod animation;
pub mod animation_manager;
pub mod app;
pub mod app_state;
mod assets;
pub mod error;
mod launch;
mod localization;
pub mod render;
pub mod scene;
pub mod theme;
pub use localization::LocalizationProvider;

pub use launch::{
    ClockFormat, ExitSemantic, ShellLockscreenResult, WeathrDisplayInput, WeathrRunError,
    restore_terminal_best_effort, run_display, run_display_blocking,
};
