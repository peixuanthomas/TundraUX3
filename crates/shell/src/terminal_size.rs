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

impl ShellTerminalSizeError {
    /// Retain the message identifier and arguments until an explicit UI render.
    pub fn localized_message(&self) -> i18n::LocalizedMessage {
        i18n::msg!(
            "early-terminal-too-small",
            width = self.width.to_string(),
            height = self.height.to_string(),
            required_width = self.required.width.to_string(),
            required_height = self.required.height.to_string(),
        )
    }
}

impl fmt::Display for ShellTerminalSizeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&i18n::render_diagnostic(&self.localized_message()))
    }
}

impl std::error::Error for ShellTerminalSizeError {}

#[derive(Debug)]
struct TerminalSizeDetectionError {
    source: io::Error,
}

impl TerminalSizeDetectionError {
    fn localized_message(&self) -> i18n::LocalizedMessage {
        i18n::msg!(
            "early-terminal-size-unavailable",
            error = self.source.to_string()
        )
    }
}

impl fmt::Display for TerminalSizeDetectionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&i18n::render_diagnostic(&self.localized_message()))
    }
}

impl std::error::Error for TerminalSizeDetectionError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.source)
    }
}

pub(crate) fn checked_current_terminal_size(
    requirement: ShellTerminalSizeRequirement,
) -> io::Result<(u16, u16)> {
    checked_terminal_size_with(requirement, crossterm::terminal::size)
}

fn checked_terminal_size_with(
    requirement: ShellTerminalSizeRequirement,
    detect_size: impl FnOnce() -> io::Result<(u16, u16)>,
) -> io::Result<(u16, u16)> {
    let size = detect_size()
        .map_err(|source| io::Error::new(source.kind(), TerminalSizeDetectionError { source }))?;
    requirement.validate(size).map_err(io::Error::other)?;
    Ok(size)
}

#[cfg(test)]
#[path = "tests/terminal_size.rs"]
mod tests;
