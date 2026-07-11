use crate::{
    BANNER_ASSET_KEY, BANNER_DISPLAY_DURATION, ShellTerminalSizeRequirement,
    checked_current_terminal_size,
};
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
    let requirement = ShellTerminalSizeRequirement::from_assets(ascii_assets);
    display_animated_banner_with_assets_and_size_check(output, total_duration, ascii_assets, || {
        checked_current_terminal_size(requirement).map(|_| ())
    })
}

fn display_animated_banner_with_assets_and_size_check(
    output: &mut impl Write,
    total_duration: Duration,
    ascii_assets: &tundra_ui::RuntimeAsciiAssets,
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
        for line in banner_lines.iter().take(revealed_lines) {
            writeln!(output, "{line}")?;
        }
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
}
