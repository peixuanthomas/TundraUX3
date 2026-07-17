use crate::{
    BANNER_ASSET_KEY, BANNER_ENTER_DURATION, BANNER_EXIT_DURATION, BANNER_HOLD_DURATION,
    ShellTerminalSizeRequirement, asset_io_error, checked_current_terminal_size,
};
use std::io::{self, Write};
use std::time::Duration;

const FROST_FRAME_INTERVAL: Duration = Duration::from_millis(33);
const CLEAR_SCREEN: &str = "\x1B[2J\x1B[H";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FrostPhase {
    Enter,
    Exit,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct StartupBannerTiming {
    enter: Duration,
    hold: Duration,
    exit: Duration,
}

impl StartupBannerTiming {
    const PRODUCTION: Self = Self {
        enter: BANNER_ENTER_DURATION,
        hold: BANNER_HOLD_DURATION,
        exit: BANNER_EXIT_DURATION,
    };

    #[cfg(test)]
    const ZERO: Self = Self {
        enter: Duration::ZERO,
        hold: Duration::ZERO,
        exit: Duration::ZERO,
    };
}

/// Plays the production startup sequence: frost crystallization, a two-second
/// readable hold, and frost sublimation.
pub fn display_startup_banner(output: &mut impl Write) -> io::Result<()> {
    let ascii_assets = tundra_ui::RuntimeAsciiAssets::load_default().map_err(asset_io_error)?;
    display_startup_banner_with_assets(output, &ascii_assets)
}

pub fn display_startup_banner_with_assets(
    output: &mut impl Write,
    ascii_assets: &tundra_ui::RuntimeAsciiAssets,
) -> io::Result<()> {
    let requirement = ShellTerminalSizeRequirement::from_assets(ascii_assets);
    display_startup_banner_with_timing_and_size_check(
        output,
        ascii_assets,
        StartupBannerTiming::PRODUCTION,
        || checked_current_terminal_size(requirement),
    )
}

fn display_startup_banner_with_timing_and_size_check(
    output: &mut impl Write,
    ascii_assets: &tundra_ui::RuntimeAsciiAssets,
    timing: StartupBannerTiming,
    mut check_size: impl FnMut() -> io::Result<(u16, u16)>,
) -> io::Result<()> {
    let _ = check_size()?;
    let banner_lines = ascii_assets
        .banner_lines(BANNER_ASSET_KEY)
        .map_err(asset_io_error)?;
    let banner_lines = visible_banner_lines(banner_lines);
    if banner_lines.is_empty() {
        return Ok(());
    }

    render_phase(
        output,
        banner_lines,
        FrostPhase::Enter,
        timing.enter,
        &mut check_size,
    )?;
    wait_with_size_checks(timing.hold, &mut check_size)?;
    render_phase(
        output,
        banner_lines,
        FrostPhase::Exit,
        timing.exit,
        &mut check_size,
    )?;
    write!(output, "{CLEAR_SCREEN}")?;
    output.flush()
}

fn render_phase(
    output: &mut impl Write,
    banner_lines: &[String],
    phase: FrostPhase,
    duration: Duration,
    check_size: &mut impl FnMut() -> io::Result<(u16, u16)>,
) -> io::Result<()> {
    let frame_interval_ms = FROST_FRAME_INTERVAL.as_millis().max(1);
    let frame_count =
        u32::try_from((duration.as_millis() / frame_interval_ms).max(1)).unwrap_or(u32::MAX);
    let frame_delay = duration / frame_count;

    for frame_index in 0..=frame_count {
        let terminal_size = check_size()?;
        let progress = frame_index as f32 / frame_count as f32;
        render_frame(
            output,
            &frost_frame(banner_lines, phase, progress),
            terminal_size,
        )?;
        if frame_index < frame_count {
            wait_with_size_checks(frame_delay, check_size)?;
        }
    }
    Ok(())
}

fn render_frame(
    output: &mut impl Write,
    frame: &[String],
    (terminal_width, terminal_height): (u16, u16),
) -> io::Result<()> {
    let frame_width = frame
        .iter()
        .map(|line| line.chars().count())
        .max()
        .unwrap_or(0);
    let top_padding = usize::from(terminal_height).saturating_sub(frame.len()) / 2;
    let left_padding = usize::from(terminal_width).saturating_sub(frame_width) / 2;

    write!(output, "{CLEAR_SCREEN}")?;
    for _ in 0..top_padding {
        writeln!(output)?;
    }
    let indent = " ".repeat(left_padding);
    for line in frame {
        writeln!(output, "{indent}{}", line.trim_end())?;
    }
    output.flush()
}

fn wait_with_size_checks(
    duration: Duration,
    check_size: &mut impl FnMut() -> io::Result<(u16, u16)>,
) -> io::Result<()> {
    let started_at = std::time::Instant::now();
    while started_at.elapsed() < duration {
        let _ = check_size()?;
        let remaining = duration.saturating_sub(started_at.elapsed());
        std::thread::sleep(remaining.min(Duration::from_millis(50)));
    }
    Ok(())
}

fn visible_banner_lines(banner_lines: &[String]) -> &[String] {
    let Some(first) = banner_lines.iter().position(|line| !line.trim().is_empty()) else {
        return &banner_lines[0..0];
    };
    let last = banner_lines
        .iter()
        .rposition(|line| !line.trim().is_empty())
        .expect("a first visible Banner line guarantees a last one");
    &banner_lines[first..=last]
}

fn frost_frame(banner_lines: &[String], phase: FrostPhase, progress: f32) -> Vec<String> {
    let progress = progress.clamp(0.0, 1.0);
    let width = banner_lines
        .iter()
        .map(|line| line.chars().count())
        .max()
        .unwrap_or(0);
    let source = padded_banner(banner_lines, width);

    match (phase, progress) {
        (FrostPhase::Enter, 0.0) | (FrostPhase::Exit, 1.0) => {
            return blank_frame(source.len(), width);
        }
        (FrostPhase::Enter, 1.0) | (FrostPhase::Exit, 0.0) => return source,
        _ => {}
    }

    source
        .iter()
        .enumerate()
        .map(|(row, line)| {
            line.chars()
                .enumerate()
                .map(|(column, glyph)| {
                    let threshold = 0.06 + unit_hash(row, column, 101) * 0.76;
                    match phase {
                        FrostPhase::Enter => crystallize(glyph, progress, threshold, row, column),
                        FrostPhase::Exit => sublimate(glyph, progress, threshold, row, column),
                    }
                    .unwrap_or(' ')
                })
                .collect()
        })
        .collect()
}

fn padded_banner(banner_lines: &[String], width: usize) -> Vec<String> {
    banner_lines
        .iter()
        .map(|line| {
            let mut cells = line.chars().collect::<Vec<_>>();
            cells.resize(width, ' ');
            cells.into_iter().collect()
        })
        .collect()
}

fn blank_frame(height: usize, width: usize) -> Vec<String> {
    vec![" ".repeat(width); height]
}

fn crystallize(
    glyph: char,
    progress: f32,
    threshold: f32,
    row: usize,
    column: usize,
) -> Option<char> {
    if glyph == ' ' {
        return ambient_crystal(progress, threshold, row, column);
    }
    if progress >= threshold + 0.12 {
        Some(glyph)
    } else if progress >= threshold {
        Some(shimmer(row, column, progress))
    } else if progress + 0.045 >= threshold {
        Some('.')
    } else {
        None
    }
}

fn sublimate(
    glyph: char,
    progress: f32,
    threshold: f32,
    row: usize,
    column: usize,
) -> Option<char> {
    if glyph == ' ' {
        return ambient_crystal(progress, threshold, row, column);
    }
    if progress < threshold {
        Some(glyph)
    } else if progress < threshold + 0.14 {
        Some(shimmer(row, column, progress))
    } else {
        None
    }
}

fn ambient_crystal(progress: f32, threshold: f32, row: usize, column: usize) -> Option<char> {
    let near_front = (progress - threshold).abs() < 0.035;
    (near_front && hash(row, column, 151).is_multiple_of(23)).then_some('.')
}

fn shimmer(row: usize, column: usize, progress: f32) -> char {
    const GLYPHS: [char; 4] = ['.', '+', '*', '+'];
    let tick = (progress * 24.0) as u32;
    GLYPHS[(hash(row, column, tick + 181) as usize) % GLYPHS.len()]
}

fn unit_hash(row: usize, column: usize, salt: u32) -> f32 {
    hash(row, column, salt) as f32 / u32::MAX as f32
}

fn hash(row: usize, column: usize, salt: u32) -> u32 {
    let mut value = (row as u32)
        .wrapping_mul(0x9E37_79B9)
        .wrapping_add((column as u32).wrapping_mul(0x85EB_CA6B))
        .wrapping_add(salt.wrapping_mul(0xC2B2_AE35));
    value ^= value >> 16;
    value = value.wrapping_mul(0x7FEB_352D);
    value ^= value >> 15;
    value = value.wrapping_mul(0x846C_A68B);
    value ^ (value >> 16)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assets() -> tundra_ui::RuntimeAsciiAssets {
        let asset_root =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../tundra-ascii-assets/assets");
        let store = tundra_ui::AsciiAssetStore::load_with_root(asset_root, "default")
            .expect("canonical ASCII assets");
        tundra_ui::RuntimeAsciiAssets::from_store(store)
    }

    #[test]
    fn frost_animation_has_clean_deterministic_endpoints() {
        let assets = assets();
        let lines = assets
            .banner_lines(BANNER_ASSET_KEY)
            .expect("default banner");
        let lines = visible_banner_lines(lines);
        let width = lines.iter().map(|line| line.chars().count()).max().unwrap();
        let exact = padded_banner(lines, width);

        assert_eq!(
            frost_frame(lines, FrostPhase::Enter, 0.0),
            blank_frame(lines.len(), width)
        );
        assert_eq!(frost_frame(lines, FrostPhase::Enter, 1.0), exact);
        assert_eq!(frost_frame(lines, FrostPhase::Exit, 0.0), exact);
        assert_eq!(
            frost_frame(lines, FrostPhase::Exit, 1.0),
            blank_frame(lines.len(), width)
        );
        let midpoint = frost_frame(lines, FrostPhase::Enter, 0.5);
        assert_eq!(midpoint, frost_frame(lines, FrostPhase::Enter, 0.5));
        assert_ne!(midpoint, exact);
        assert!(midpoint.iter().any(|line| !line.trim().is_empty()));
        assert!(midpoint.iter().all(|line| line.is_ascii()));
    }

    #[test]
    fn zero_timing_sequence_still_renders_logo_and_clears_screen() {
        let assets = assets();
        let first_visible_line = assets
            .banner_lines(BANNER_ASSET_KEY)
            .unwrap()
            .iter()
            .find(|line| !line.trim().is_empty())
            .unwrap();
        let mut output = Vec::new();

        display_startup_banner_with_timing_and_size_check(
            &mut output,
            &assets,
            StartupBannerTiming::ZERO,
            || Ok((120, 40)),
        )
        .expect("zero-duration frost sequence");

        let output = String::from_utf8(output).unwrap();
        assert!(output.contains(first_visible_line.trim_end()));
        assert!(output.ends_with(CLEAR_SCREEN));
    }

    #[test]
    fn production_sequence_holds_the_complete_banner_for_two_seconds() {
        assert_eq!(StartupBannerTiming::PRODUCTION.hold, Duration::from_secs(2));
    }

    #[test]
    fn frame_is_centered_using_the_visible_banner_bounds() {
        let frame = vec!["ABCDE".to_string(), "  X  ".to_string()];
        let mut output = Vec::new();

        render_frame(&mut output, &frame, (15, 8)).expect("centered frame renders");

        let output = String::from_utf8(output).unwrap();
        assert_eq!(
            output,
            format!("{CLEAR_SCREEN}\n\n\n     ABCDE\n       X\n")
        );
    }

    #[test]
    fn startup_animation_stops_when_terminal_size_check_fails() {
        let assets = assets();
        let mut output = Vec::new();
        let mut checks = 0;

        let error = display_startup_banner_with_timing_and_size_check(
            &mut output,
            &assets,
            StartupBannerTiming::ZERO,
            || {
                checks += 1;
                if checks >= 4 {
                    Err(io::Error::other("terminal became too small"))
                } else {
                    Ok((120, 40))
                }
            },
        )
        .expect_err("failed size check stops the frost sequence");

        let output = String::from_utf8(output).unwrap();
        assert!(error.to_string().contains("too small"));
        assert!(output.contains("ooooooooooooo"));
        assert!(!output.ends_with(CLEAR_SCREEN));
    }
}
