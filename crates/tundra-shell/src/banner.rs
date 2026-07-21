use crate::{BANNER_ASSET_KEY, ShellTerminalSizeRequirement, checked_current_terminal_size};
use ratatui::style::Color;
use std::io::{self, Write};
use std::thread;
use std::time::{Duration, Instant};
use tundra_storage::{CLOCK_DESCRIPTOR, CONFIG_DESCRIPTOR, SCHEMA_VERSION};

const TERMINAL_SIZE_POLL_INTERVAL: Duration = Duration::from_millis(50);

pub fn banner_lines() -> Result<Vec<String>, tundra_ui::AssetError> {
    let ascii_assets = tundra_ui::RuntimeAsciiAssets::load_default()?;
    Ok(ascii_assets.banner_lines(BANNER_ASSET_KEY)?.to_vec())
}

pub fn startup_lines() -> Vec<String> {
    vec![
        "TundraUX3 shell - Phase 0 smoke".to_string(),
        "Supported OS: Windows and macOS".to_string(),
        "Target terminal: crossterm-compatible terminal".to_string(),
        format!(
            "Config format: {} (schema v{})",
            CONFIG_DESCRIPTOR.file_name, SCHEMA_VERSION
        ),
        format!(
            "State data: users, state, recent-files, sessions, {} use versioned JSON",
            CLOCK_DESCRIPTOR.name
        ),
    ]
}

pub fn render_static_banner(output: &mut impl Write) -> io::Result<()> {
    render_static_banner_colored(output, Color::White)
}

/// Renders the static logo using `color` as its terminal foreground color.
pub fn render_static_banner_colored(output: &mut impl Write, color: Color) -> io::Result<()> {
    let ascii_assets = tundra_ui::RuntimeAsciiAssets::load_default().map_err(asset_io_error)?;
    render_static_banner_with_assets_colored(output, &ascii_assets, color)
}

pub fn render_static_banner_with_assets(
    output: &mut impl Write,
    ascii_assets: &tundra_ui::RuntimeAsciiAssets,
) -> io::Result<()> {
    render_static_banner_with_assets_colored(output, ascii_assets, Color::White)
}

/// Renders the supplied static logo using `color` as its terminal foreground color.
pub fn render_static_banner_with_assets_colored(
    output: &mut impl Write,
    ascii_assets: &tundra_ui::RuntimeAsciiAssets,
    color: Color,
) -> io::Result<()> {
    write!(output, "{}", ansi_foreground(color))?;
    for line in ascii_assets
        .banner_lines(BANNER_ASSET_KEY)
        .map_err(asset_io_error)?
    {
        writeln!(output, "{line}")?;
    }
    write!(output, "\x1B[0m")?;

    Ok(())
}

pub fn display_banner(output: &mut impl Write) -> io::Result<()> {
    display_banner_colored(output, Color::White)
}

/// Plays the frost startup logo using `color` as its terminal foreground color.
pub fn display_banner_colored(output: &mut impl Write, color: Color) -> io::Result<()> {
    crate::startup_banner::display_startup_banner_colored(output, color)
}

pub fn display_animated_banner(
    output: &mut impl Write,
    total_duration: Duration,
) -> io::Result<()> {
    display_animated_banner_colored(output, total_duration, Color::White)
}

/// Plays the legacy line-by-line logo using `color` as its terminal foreground color.
pub fn display_animated_banner_colored(
    output: &mut impl Write,
    total_duration: Duration,
    color: Color,
) -> io::Result<()> {
    let ascii_assets = tundra_ui::RuntimeAsciiAssets::load_default().map_err(asset_io_error)?;
    display_animated_banner_with_assets_colored(output, total_duration, &ascii_assets, color)
}

pub fn display_animated_banner_with_assets(
    output: &mut impl Write,
    total_duration: Duration,
    ascii_assets: &tundra_ui::RuntimeAsciiAssets,
) -> io::Result<()> {
    display_animated_banner_with_assets_colored(output, total_duration, ascii_assets, Color::White)
}

/// Plays the supplied legacy line-by-line logo using `color` as its terminal
/// foreground color.
pub fn display_animated_banner_with_assets_colored(
    output: &mut impl Write,
    total_duration: Duration,
    ascii_assets: &tundra_ui::RuntimeAsciiAssets,
    color: Color,
) -> io::Result<()> {
    let requirement = ShellTerminalSizeRequirement::from_assets(ascii_assets);
    display_animated_banner_with_assets_and_size_check(
        output,
        total_duration,
        ascii_assets,
        color,
        || checked_current_terminal_size(requirement).map(|_| ()),
    )
}

fn display_animated_banner_with_assets_and_size_check(
    output: &mut impl Write,
    total_duration: Duration,
    ascii_assets: &tundra_ui::RuntimeAsciiAssets,
    color: Color,
    mut check_size: impl FnMut() -> io::Result<()>,
) -> io::Result<()> {
    check_size()?;
    let banner_lines = ascii_assets
        .banner_lines(BANNER_ASSET_KEY)
        .map_err(asset_io_error)?;
    if banner_lines.is_empty() {
        return Ok(());
    }

    let started_at = Instant::now();
    let frame_delay = total_duration / (banner_lines.len() as u32 + 1);

    for revealed_lines in 1..=banner_lines.len() {
        check_size()?;
        write!(output, "\x1B[2J\x1B[H")?;
        write!(output, "{}", ansi_foreground(color))?;
        for line in banner_lines.iter().take(revealed_lines) {
            writeln!(output, "{line}")?;
        }
        write!(output, "\x1B[0m")?;
        output.flush()?;

        wait_with_size_checks(frame_delay, &mut check_size)?;
    }

    let elapsed = started_at.elapsed();
    if elapsed < total_duration {
        wait_with_size_checks(total_duration - elapsed, &mut check_size)?;
    }

    Ok(())
}

fn wait_with_size_checks(
    duration: Duration,
    check_size: &mut impl FnMut() -> io::Result<()>,
) -> io::Result<()> {
    let started_at = Instant::now();
    while started_at.elapsed() < duration {
        check_size()?;
        let remaining = duration.saturating_sub(started_at.elapsed());
        thread::sleep(remaining.min(TERMINAL_SIZE_POLL_INTERVAL));
    }
    Ok(())
}

pub(crate) fn asset_io_error(error: tundra_ui::AssetError) -> io::Error {
    io::Error::other(error.to_string())
}

/// Encodes a ratatui color as an ANSI foreground sequence. Named colors use
/// standard ANSI SGR codes; RGB and indexed colors retain their exact values.
pub(crate) fn ansi_foreground(color: Color) -> String {
    match color {
        Color::Reset => "\x1B[39m".to_string(),
        Color::Black => "\x1B[30m".to_string(),
        Color::Red => "\x1B[31m".to_string(),
        Color::Green => "\x1B[32m".to_string(),
        Color::Yellow => "\x1B[33m".to_string(),
        Color::Blue => "\x1B[34m".to_string(),
        Color::Magenta => "\x1B[35m".to_string(),
        Color::Cyan => "\x1B[36m".to_string(),
        Color::Gray => "\x1B[37m".to_string(),
        Color::DarkGray => "\x1B[90m".to_string(),
        Color::LightRed => "\x1B[91m".to_string(),
        Color::LightGreen => "\x1B[92m".to_string(),
        Color::LightYellow => "\x1B[93m".to_string(),
        Color::LightBlue => "\x1B[94m".to_string(),
        Color::LightMagenta => "\x1B[95m".to_string(),
        Color::LightCyan => "\x1B[96m".to_string(),
        Color::White => "\x1B[97m".to_string(),
        Color::Rgb(red, green, blue) => format!("\x1B[38;2;{red};{green};{blue}m"),
        Color::Indexed(index) => format!("\x1B[38;5;{index}m"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn animated_banner_stops_rendering_when_a_size_check_fails() {
        let asset_root =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../tundra-ascii-assets/assets");
        let store = tundra_ui::AsciiAssetStore::load_with_root(asset_root, "default")
            .expect("canonical ASCII assets");
        let assets = tundra_ui::RuntimeAsciiAssets::from_store(store);
        let banner_lines = assets
            .banner_lines(BANNER_ASSET_KEY)
            .expect("default banner")
            .to_vec();
        let mut output = Vec::new();
        let mut checks = 0;

        let error = display_animated_banner_with_assets_and_size_check(
            &mut output,
            Duration::ZERO,
            &assets,
            Color::White,
            || {
                checks += 1;
                if checks >= 3 {
                    Err(io::Error::other("terminal became too small"))
                } else {
                    Ok(())
                }
            },
        )
        .expect_err("failed size check must stop the animation");

        let output = String::from_utf8(output).expect("banner output should be UTF-8");
        assert!(error.to_string().contains("too small"));
        assert!(output.contains(&banner_lines[0]));
        assert!(!output.contains(&banner_lines[1]));
    }

    #[test]
    fn ansi_foreground_preserves_named_and_rgb_colors() {
        assert_eq!(ansi_foreground(Color::Red), "\x1B[31m");
        assert_eq!(
            ansi_foreground(Color::Rgb(18, 52, 86)),
            "\x1B[38;2;18;52;86m"
        );
    }
}
