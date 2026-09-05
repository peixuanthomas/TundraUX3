use std::fmt;
use std::io;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShellTerminalSizeRequirement {
    pub width: usize,
    pub height: usize,
}

impl ShellTerminalSizeRequirement {
    pub fn from_assets(assets: &ui::RuntimeAsciiAssets) -> Self {
        Self::from_asset_dimensions(assets.max_asset_dimensions())
    }

    pub fn from_asset_dimensions(asset_dimensions: ui::AssetDimensions) -> Self {
        Self {
            width: asset_dimensions
                .width
                .max(usize::from(ui::MIN_SHELL_TERMINAL_WIDTH))
                .max(usize::from(weathr::render::MIN_TERMINAL_WIDTH)),
            height: asset_dimensions
                .height
                .max(usize::from(ui::MIN_SHELL_TERMINAL_HEIGHT))
                .max(usize::from(weathr::render::MIN_TERMINAL_HEIGHT)),
        }
    }

    pub fn validate(self, (width, height): (u16, u16)) -> Result<(), ShellTerminalSizeError> {
        if usize::from(width) < self.width || usize::from(height) < self.height {
            return Err(ShellTerminalSizeError {
                width,
                height,
                required: self,
            });
        }

        Ok(())
    }

    pub fn as_terminal_size(self) -> (u16, u16) {
        (
            u16::try_from(self.width).unwrap_or(u16::MAX),
            u16::try_from(self.height).unwrap_or(u16::MAX),
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShellTerminalSizeError {
    pub width: u16,
    pub height: u16,
    pub required: ShellTerminalSizeRequirement,
}

impl fmt::Display for ShellTerminalSizeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "terminal is too small ({}x{}); resize it to at least {}x{} and try again",
            self.width, self.height, self.required.width, self.required.height
        )
    }
}

impl std::error::Error for ShellTerminalSizeError {}

pub(crate) fn checked_current_terminal_size(
    requirement: ShellTerminalSizeRequirement,
) -> io::Result<(u16, u16)> {
    checked_terminal_size_with(requirement, crossterm::terminal::size)
}

fn checked_terminal_size_with(
    requirement: ShellTerminalSizeRequirement,
    detect_size: impl FnOnce() -> io::Result<(u16, u16)>,
) -> io::Result<(u16, u16)> {
    let size = detect_size().map_err(|error| {
        io::Error::new(
            error.kind(),
            format!("could not determine terminal size: {error}"),
        )
    })?;
    requirement.validate(size).map_err(io::Error::other)?;
    Ok(size)
}

#[cfg(test)]
#[path = "tests/terminal_size.rs"]
mod tests;
