use crate::{BANNER_ASSET_KEY, ShellTerminalSizeRequirement, checked_current_terminal_size};
use ratatui::style::Color;
use std::io::{self, Write};
use std::thread;
use std::time::{Duration, Instant};
use storage::{CLOCK_DESCRIPTOR, CONFIG_DESCRIPTOR, SCHEMA_VERSION};
use terminal_runtime::ansi_foreground;

const TERMINAL_SIZE_POLL_INTERVAL: Duration = Duration::from_millis(50);

pub fn banner_lines() -> Result<Vec<String>, ui::AssetError> {
    let ascii_assets = ui::RuntimeAsciiAssets::load_default()?;
    Ok(ascii_assets.banner_lines(BANNER_ASSET_KEY)?.to_vec())
}

pub fn startup_lines() -> Vec<String> {
    vec![
        "TundraUX3 shell - Phase 0 smoke".to_string(),
        "Supported OS: Windows, macOS, and Linux (basic)".to_string(),
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
    let ascii_assets = ui::RuntimeAsciiAssets::load_default().map_err(asset_io_error)?;
    render_static_banner_with_assets_colored(output, &ascii_assets, color)
}

pub fn render_static_banner_with_assets(
    output: &mut impl Write,
    ascii_assets: &ui::RuntimeAsciiAssets,
) -> io::Result<()> {
    render_static_banner_with_assets_colored(output, ascii_assets, Color::White)
}

/// Renders the supplied static logo using `color` as its terminal foreground color.
pub fn render_static_banner_with_assets_colored(
    output: &mut impl Write,
    ascii_assets: &ui::RuntimeAsciiAssets,
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
    let ascii_assets = ui::RuntimeAsciiAssets::load_default().map_err(asset_io_error)?;
    display_animated_banner_with_assets_colored(output, total_duration, &ascii_assets, color)
}

pub fn display_animated_banner_with_assets(
    output: &mut impl Write,
    total_duration: Duration,
    ascii_assets: &ui::RuntimeAsciiAssets,
) -> io::Result<()> {
    display_animated_banner_with_assets_colored(output, total_duration, ascii_assets, Color::White)
}

/// Plays the supplied legacy line-by-line logo using `color` as its terminal
/// foreground color.
pub fn display_animated_banner_with_assets_colored(
    output: &mut impl Write,
    total_duration: Duration,
    ascii_assets: &ui::RuntimeAsciiAssets,
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
    ascii_assets: &ui::RuntimeAsciiAssets,
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

pub(crate) fn asset_io_error(error: ui::AssetError) -> io::Error {
    io::Error::other(error.to_string())
}

#[cfg(test)]
#[path = "../../tests/unit/banner/tests.rs"]
mod tests;
