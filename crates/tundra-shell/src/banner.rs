use crate::{BANNER_ASSET_KEY, BANNER_DISPLAY_DURATION};
use std::io::{self, Write};
use std::thread;
use std::time::{Duration, Instant};
use tundra_storage::{CLOCK_DESCRIPTOR, CONFIG_DESCRIPTOR, SCHEMA_VERSION};

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
    let ascii_assets = tundra_ui::RuntimeAsciiAssets::load_default().map_err(asset_io_error)?;
    render_static_banner_with_assets(output, &ascii_assets)
}

pub fn render_static_banner_with_assets(
    output: &mut impl Write,
    ascii_assets: &tundra_ui::RuntimeAsciiAssets,
) -> io::Result<()> {
    for line in ascii_assets
        .banner_lines(BANNER_ASSET_KEY)
        .map_err(asset_io_error)?
    {
        writeln!(output, "{line}")?;
    }

    Ok(())
}

pub fn display_banner(output: &mut impl Write) -> io::Result<()> {
    let ascii_assets = tundra_ui::RuntimeAsciiAssets::load_default().map_err(asset_io_error)?;
    display_animated_banner_with_assets(output, BANNER_DISPLAY_DURATION, &ascii_assets)
}

pub fn display_animated_banner(
    output: &mut impl Write,
    total_duration: Duration,
) -> io::Result<()> {
    let ascii_assets = tundra_ui::RuntimeAsciiAssets::load_default().map_err(asset_io_error)?;
    display_animated_banner_with_assets(output, total_duration, &ascii_assets)
}

pub fn display_animated_banner_with_assets(
    output: &mut impl Write,
    total_duration: Duration,
    ascii_assets: &tundra_ui::RuntimeAsciiAssets,
) -> io::Result<()> {
    let banner_lines = ascii_assets
        .banner_lines(BANNER_ASSET_KEY)
        .map_err(asset_io_error)?;
    if banner_lines.is_empty() {
        return Ok(());
    }

    let started_at = Instant::now();
    let frame_delay = total_duration / (banner_lines.len() as u32 + 1);

    for revealed_lines in 1..=banner_lines.len() {
        write!(output, "\x1B[2J\x1B[H")?;
        for line in banner_lines.iter().take(revealed_lines) {
            writeln!(output, "{line}")?;
        }
        output.flush()?;

        if !frame_delay.is_zero() {
            thread::sleep(frame_delay);
        }
    }

    let elapsed = started_at.elapsed();
    if elapsed < total_duration {
        thread::sleep(total_duration - elapsed);
    }

    Ok(())
}

pub(crate) fn asset_io_error(error: tundra_ui::AssetError) -> io::Error {
    io::Error::other(error.to_string())
}
