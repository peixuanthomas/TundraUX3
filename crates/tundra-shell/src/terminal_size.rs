use std::fmt;
use std::io;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShellTerminalSizeRequirement {
    pub width: usize,
    pub height: usize,
}

impl ShellTerminalSizeRequirement {
    pub fn from_assets(assets: &tundra_ui::RuntimeAsciiAssets) -> Self {
        Self::from_asset_dimensions(assets.max_asset_dimensions())
    }

    pub fn from_asset_dimensions(asset_dimensions: tundra_ui::AssetDimensions) -> Self {
        Self {
            width: asset_dimensions
                .width
                .max(usize::from(tundra_ui::MIN_SHELL_TERMINAL_WIDTH))
                .max(usize::from(tundra_weathr::render::MIN_TERMINAL_WIDTH)),
            height: asset_dimensions
                .height
                .max(usize::from(tundra_ui::MIN_SHELL_TERMINAL_HEIGHT))
                .max(usize::from(tundra_weathr::render::MIN_TERMINAL_HEIGHT)),
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
mod tests {
    use super::*;

    #[test]
    fn terminal_size_requirement_covers_assets_shell_and_lockscreen() {
        let asset_root =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../tundra-ascii-assets/assets");
        let store = tundra_ui::AsciiAssetStore::load_with_root(asset_root, "default")
            .expect("canonical ASCII assets");
        let assets = tundra_ui::RuntimeAsciiAssets::from_store(store);

        assert_eq!(
            ShellTerminalSizeRequirement::from_assets(&assets),
            ShellTerminalSizeRequirement {
                width: 108,
                height: 20,
            }
        );
    }

    #[test]
    fn terminal_size_validation_accepts_the_boundary_and_larger_sizes() {
        let requirement = ShellTerminalSizeRequirement {
            width: 108,
            height: 20,
        };

        assert!(requirement.validate((108, 20)).is_ok());
        assert!(requirement.validate((160, 48)).is_ok());
    }

    #[test]
    fn terminal_size_requirement_tracks_larger_assets_and_keeps_layout_floors() {
        assert_eq!(
            ShellTerminalSizeRequirement::from_asset_dimensions(tundra_ui::AssetDimensions {
                width: 137,
                height: 23,
            }),
            ShellTerminalSizeRequirement {
                width: 137,
                height: 23,
            }
        );
        assert_eq!(
            ShellTerminalSizeRequirement::from_asset_dimensions(tundra_ui::AssetDimensions {
                width: 1,
                height: 1,
            }),
            ShellTerminalSizeRequirement {
                width: 70,
                height: 20,
            }
        );
    }

    #[test]
    fn terminal_size_validation_rejects_each_undersized_dimension() {
        let requirement = ShellTerminalSizeRequirement {
            width: 108,
            height: 20,
        };

        for size in [(107, 20), (108, 19), (107, 19)] {
            let error = requirement
                .validate(size)
                .expect_err("undersized terminal must be rejected");
            assert_eq!((error.width, error.height), size);
            assert_eq!(error.required, requirement);
        }
    }

    #[test]
    fn terminal_size_error_is_one_actionable_line() {
        let error = ShellTerminalSizeRequirement {
            width: 108,
            height: 20,
        }
        .validate((80, 18))
        .expect_err("undersized terminal must be rejected")
        .to_string();

        assert_eq!(error.lines().count(), 1);
        assert!(error.contains("80x18"));
        assert!(error.contains("108x20"));
        assert!(error.contains("resize"));
    }

    #[test]
    fn checked_terminal_size_rejects_before_the_caller_can_render() {
        let requirement = ShellTerminalSizeRequirement {
            width: 108,
            height: 20,
        };

        let error = checked_terminal_size_with(requirement, || Ok((107, 20)))
            .expect_err("undersized detected terminal must be rejected");
        assert!(error.to_string().contains("resize"));
        assert_eq!(
            checked_terminal_size_with(requirement, || Ok((108, 20)))
                .expect("boundary terminal should pass"),
            (108, 20)
        );
    }

    #[test]
    fn checked_terminal_size_does_not_replace_detection_failures_with_a_fallback() {
        let requirement = ShellTerminalSizeRequirement {
            width: 108,
            height: 20,
        };
        let error = checked_terminal_size_with(requirement, || {
            Err(io::Error::new(io::ErrorKind::NotConnected, "no terminal"))
        })
        .expect_err("terminal detection failure must stop startup");

        assert_eq!(error.kind(), io::ErrorKind::NotConnected);
        assert!(
            error
                .to_string()
                .contains("could not determine terminal size")
        );
    }
}
