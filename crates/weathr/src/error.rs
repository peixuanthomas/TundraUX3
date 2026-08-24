use std::io;
use thiserror::Error as ThisError;

use crate::render::clock::ClockFontError;

#[derive(ThisError, Debug)]
pub enum WeatherAssetError {
    #[error("invalid bundled clock font: {0}")]
    ClockFont(#[from] ClockFontError),
}

#[derive(ThisError, Debug)]
pub enum TerminalError {
    #[error(
        "terminal is too small ({width}x{height}); resize it to at least {min_width}x{min_height} characters"
    )]
    TooSmall {
        width: u16,
        height: u16,
        min_width: u16,
        min_height: u16,
    },
    #[error(
        "terminal size requirement {min_width}x{min_height} exceeds the supported maximum {max_width}x{max_height}; reduce the configured asset size"
    )]
    RequirementTooLarge {
        min_width: u16,
        min_height: u16,
        max_width: u16,
        max_height: u16,
    },
    #[error("not running in a terminal (output is redirected or piped)")]
    NotATty,
    #[error("failed to enable raw mode")]
    RawModeError(#[source] io::Error),
    #[error("failed to get terminal size")]
    SizeError(#[source] io::Error),
    #[error("failed to initialize terminal")]
    InitError(#[source] io::Error),
    #[error("terminal I/O error")]
    IoError(#[from] io::Error),
}

impl TerminalError {
    pub fn user_friendly_message(&self) -> String {
        match self {
            Self::TooSmall { width, height, min_width, min_height } => format!("Terminal window is too small ({width}x{height}); resize it to at least {min_width}x{min_height} characters."),
            Self::RequirementTooLarge { min_width, min_height, max_width, max_height } => format!("Configured assets require {min_width}x{min_height} characters, but the renderer supports at most {max_width}x{max_height}; reduce the asset size."),
            Self::NotATty => "This application must be run in a terminal. It cannot work when output is redirected or piped.".to_string(),
            Self::RawModeError(_) => "Failed to initialize terminal raw mode. You may need to run this in a proper terminal emulator.".to_string(),
            Self::SizeError(_) => "Cannot detect terminal size. Make sure you're running in a standard terminal.".to_string(),
            _ => self.to_string(),
        }
    }
}
