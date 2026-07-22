use crate::{
    BANNER_ASSET_KEY, ShellTerminalSizeRequirement, ansi_foreground, asset_io_error,
    checked_current_terminal_size,
};
use ratatui::style::Color;
use std::fmt::Write as _;
use std::io::{self, Write};
use std::time::{Duration, Instant};

const MATRIX_FRAME_INTERVAL: Duration = Duration::from_millis(33);
const MATRIX_RAIN_DURATION: Duration = Duration::from_secs(1);
const MATRIX_ASSEMBLE_DURATION: Duration = Duration::from_millis(1_200);
const MATRIX_HOLD_DURATION: Duration = Duration::from_secs(1);
const TERMINAL_SIZE_POLL_INTERVAL: Duration = Duration::from_millis(50);

const CLEAR_SCREEN: &str = "\x1B[2J\x1B[H";
const RESET_STYLE: &str = "\x1B[0m";
const BLACK_BACKGROUND: &str = "\x1B[40m";
const CLEAR_LINE: &str = "\x1B[2K";

const MATRIX_GLYPHS: &[u8] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ@#$%&*+-=<>[]{}";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MatrixTone {
    DimGreen,
    Green,
    BrightGreen,
    White,
    Banner(Color),
}

impl MatrixTone {
    fn ansi(self) -> String {
        match self {
            Self::DimGreen => "\x1B[38;2;0;82;32m".to_string(),
            Self::Green => "\x1B[38;2;0;178;70m".to_string(),
            Self::BrightGreen => "\x1B[1;38;2;88;255;144m".to_string(),
            Self::White => "\x1B[1;38;2;255;255;255m".to_string(),
            Self::Banner(color) => ansi_foreground(color),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct MatrixCell {
    glyph: char,
    tone: MatrixTone,
}

type MatrixFrame = Vec<Vec<Option<MatrixCell>>>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MatrixPhase {
    Rain,
    Assemble,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct MatrixTiming {
    rain: Duration,
    assemble: Duration,
    hold: Duration,
}

impl MatrixTiming {
    const PRODUCTION: Self = Self {
        rain: MATRIX_RAIN_DURATION,
        assemble: MATRIX_ASSEMBLE_DURATION,
        hold: MATRIX_HOLD_DURATION,
    };

    #[cfg(test)]
    const ZERO: Self = Self {
        rain: Duration::ZERO,
        assemble: Duration::ZERO,
        hold: Duration::ZERO,
    };
}

/// Plays the first-run-only Matrix sequence with the default white Banner.
/// Ambient glyphs keep their independent Matrix palette while copies of the
/// Banner glyphs descend into their final positions before the screen clears.
pub fn display_first_run_banner_with_assets(
    output: &mut impl Write,
    ascii_assets: &ui::RuntimeAsciiAssets,
) -> io::Result<()> {
    display_first_run_banner_with_assets_colored(output, ascii_assets, Color::White)
}

/// Plays the first-run-only Matrix sequence using `color` for the assembled
/// logo while preserving the Matrix rain's original green tones.
pub fn display_first_run_banner_with_assets_colored(
    output: &mut impl Write,
    ascii_assets: &ui::RuntimeAsciiAssets,
    color: Color,
) -> io::Result<()> {
    let requirement = ShellTerminalSizeRequirement::from_assets(ascii_assets);
    display_first_run_banner_with_timing_and_size_check(
        output,
        ascii_assets,
        MatrixTiming::PRODUCTION,
        color,
        || checked_current_terminal_size(requirement),
    )
}

fn display_first_run_banner_with_timing_and_size_check(
    output: &mut impl Write,
    ascii_assets: &ui::RuntimeAsciiAssets,
    timing: MatrixTiming,
    color: Color,
    mut check_size: impl FnMut() -> io::Result<(u16, u16)>,
) -> io::Result<()> {
    let banner_lines = ascii_assets
        .banner_lines(BANNER_ASSET_KEY)
        .map_err(asset_io_error)?;
    let banner_lines = visible_banner_lines(banner_lines);
    if banner_lines.is_empty() {
        return Ok(());
    }

    write!(output, "{RESET_STYLE}{BLACK_BACKGROUND}{CLEAR_SCREEN}")?;
    output.flush()?;
    let mut frame_buffer = MatrixFrame::new();
    let mut terminal_buffer = String::new();
    render_matrix_phase(
        output,
        banner_lines,
        MatrixPhase::Rain,
        timing.rain,
        color,
        &mut check_size,
        &mut frame_buffer,
        &mut terminal_buffer,
    )?;
    render_matrix_phase(
        output,
        banner_lines,
        MatrixPhase::Assemble,
        timing.assemble,
        color,
        &mut check_size,
        &mut frame_buffer,
        &mut terminal_buffer,
    )?;
    wait_with_size_checks(timing.hold, &mut check_size)?;
    write!(output, "{RESET_STYLE}{CLEAR_SCREEN}")?;
    output.flush()
}

fn render_matrix_phase(
    output: &mut impl Write,
    banner_lines: &[String],
    phase: MatrixPhase,
    duration: Duration,
    color: Color,
    check_size: &mut impl FnMut() -> io::Result<(u16, u16)>,
    frame: &mut MatrixFrame,
    terminal_buffer: &mut String,
) -> io::Result<()> {
    let interval_ms = MATRIX_FRAME_INTERVAL.as_millis().max(1);
    let frame_count =
        u32::try_from(duration.as_millis().div_ceil(interval_ms).max(1)).unwrap_or(u32::MAX);
    let started_at = Instant::now();

    for frame_index in 0..frame_count {
        if frame_index > 0 {
            let deadline_progress = frame_index as f64 / (frame_count - 1) as f64;
            let deadline = started_at + duration.mul_f64(deadline_progress);
            wait_until_with_size_checks(deadline, check_size)?;
        }
        let terminal_size = check_size()?;
        // Start with visible motion instead of spending the first interval on
        // an empty frame. The final frame still lands exactly at 1.0.
        let progress = (frame_index + 1) as f32 / frame_count as f32;
        let (rain_progress, banner_progress) = match phase {
            MatrixPhase::Rain => (progress, 0.0),
            MatrixPhase::Assemble => (1.0 + progress * 0.45, progress),
        };
        update_matrix_frame(
            frame,
            banner_lines,
            terminal_size,
            rain_progress,
            banner_progress,
            color,
        );
        render_matrix_frame(output, frame, terminal_buffer)?;
    }

    Ok(())
}

#[cfg(test)]
fn matrix_frame(
    banner_lines: &[String],
    (terminal_width, terminal_height): (u16, u16),
    rain_progress: f32,
    banner_progress: f32,
    color: Color,
) -> MatrixFrame {
    let mut frame = MatrixFrame::new();
    update_matrix_frame(
        &mut frame,
        banner_lines,
        (terminal_width, terminal_height),
        rain_progress,
        banner_progress,
        color,
    );
    frame
}

fn update_matrix_frame(
    frame: &mut MatrixFrame,
    banner_lines: &[String],
    (terminal_width, terminal_height): (u16, u16),
    rain_progress: f32,
    banner_progress: f32,
    color: Color,
) {
    let width = usize::from(terminal_width);
    let height = usize::from(terminal_height);
    let rain_progress = rain_progress.max(0.0);
    let banner_progress = banner_progress.clamp(0.0, 1.0);
    if frame.len() != height || frame.first().is_some_and(|row| row.len() != width) {
        *frame = vec![vec![None; width]; height];
    } else {
        for row in frame.iter_mut() {
            row.fill(None);
        }
    }

    render_ambient_rain(frame, rain_progress, 1.0 - banner_progress);
    render_falling_banner_glyphs(frame, banner_lines, banner_progress, color);
}

fn render_ambient_rain(frame: &mut MatrixFrame, rain_progress: f32, opacity: f32) {
    if rain_progress <= 0.0 || opacity <= 0.0 || frame.is_empty() {
        return;
    }

    let height = frame.len();
    let width = frame[0].len();
    let tick = (rain_progress * 150.0) as u32;
    let fade = (rain_progress / 0.08).clamp(0.0, 1.0) * opacity.clamp(0.0, 1.0);

    for column in 0..width {
        if hash(0, column, 17).is_multiple_of(5) {
            continue;
        }

        let trail_length = 4 + (hash(0, column, 23) as usize % 10);
        let speed = 1.05 + unit_hash(0, column, 29) * 0.95;
        let stream_span = height as f32 + trail_length as f32 + 8.0;
        let stagger = unit_hash(0, column, 31) * stream_span;
        let travel = rain_progress * height as f32 * 2.2 * speed + stagger;
        let head = travel.rem_euclid(stream_span) - trail_length as f32;

        for trail_index in 0..trail_length {
            let row = head.floor() as isize - trail_index as isize;
            if row < 0 || row >= height as isize {
                continue;
            }
            let visibility = 1.0 - trail_index as f32 / trail_length as f32;
            if visibility * fade < unit_hash(row as usize, column, tick + 37) * 0.42 {
                continue;
            }

            let tone = if trail_index == 0 {
                MatrixTone::BrightGreen
            } else if hash(row as usize, column, tick + 41).is_multiple_of(43) {
                MatrixTone::White
            } else if trail_index <= 3 {
                MatrixTone::Green
            } else {
                MatrixTone::DimGreen
            };
            frame[row as usize][column] = Some(MatrixCell {
                glyph: matrix_glyph(row as usize, column, tick),
                tone,
            });
        }
    }
}

fn render_falling_banner_glyphs(
    frame: &mut MatrixFrame,
    banner_lines: &[String],
    progress: f32,
    color: Color,
) {
    if frame.is_empty() || frame[0].is_empty() {
        return;
    }

    let banner_width = banner_lines
        .iter()
        .map(|line| line.chars().count())
        .max()
        .unwrap_or(0);
    let origin_row = frame.len().saturating_sub(banner_lines.len()) / 2;
    let origin_column = frame[0].len().saturating_sub(banner_width) / 2;

    for (banner_row, line) in banner_lines.iter().enumerate() {
        for (banner_column, glyph) in line.chars().enumerate() {
            if glyph == ' ' {
                continue;
            }

            let target_row = origin_row + banner_row;
            let target_column = origin_column + banner_column;
            if target_row >= frame.len() || target_column >= frame[0].len() {
                continue;
            }

            let landing = 0.42 + unit_hash(banner_row, banner_column, 313) * 0.54;
            let start =
                (landing - 0.38 - unit_hash(banner_row, banner_column, 317) * 0.12).max(0.04);

            if progress >= landing {
                frame[target_row][target_column] = Some(MatrixCell {
                    glyph,
                    tone: MatrixTone::Banner(color),
                });
                continue;
            }
            if progress < start {
                continue;
            }

            let local_progress = ((progress - start) / (landing - start)).clamp(0.0, 1.0);
            let eased_progress = local_progress * local_progress * (3.0 - 2.0 * local_progress);
            let falling_row = (target_row as f32 * eased_progress).round() as usize;
            if falling_row < frame.len() {
                frame[falling_row][target_column] = Some(MatrixCell {
                    glyph,
                    tone: MatrixTone::Banner(color),
                });
            }
        }
    }
}

fn render_matrix_frame(
    output: &mut impl Write,
    frame: &MatrixFrame,
    terminal_buffer: &mut String,
) -> io::Result<()> {
    terminal_buffer.clear();
    let estimated_width = frame.first().map_or(0, Vec::len);
    terminal_buffer.reserve(frame.len().saturating_mul(estimated_width + 24));
    let mut active_tone = None;

    for (row, cells) in frame.iter().enumerate() {
        write!(terminal_buffer, "\x1B[{};1H{CLEAR_LINE}", row + 1)
            .expect("writing terminal commands to a String cannot fail");
        let Some(last_visible) = cells.iter().rposition(Option::is_some) else {
            continue;
        };

        for cell in &cells[..=last_visible] {
            match cell {
                Some(cell) => {
                    if active_tone != Some(cell.tone) {
                        terminal_buffer.push_str(&cell.tone.ansi());
                        active_tone = Some(cell.tone);
                    }
                    terminal_buffer.push(cell.glyph);
                }
                None => terminal_buffer.push(' '),
            }
        }
    }
    if active_tone.is_some() {
        terminal_buffer.push_str(RESET_STYLE);
        terminal_buffer.push_str(BLACK_BACKGROUND);
    }

    output.write_all(terminal_buffer.as_bytes())?;
    output.flush()
}

fn wait_until_with_size_checks(
    deadline: Instant,
    check_size: &mut impl FnMut() -> io::Result<(u16, u16)>,
) -> io::Result<()> {
    loop {
        let now = Instant::now();
        if now >= deadline {
            return Ok(());
        }

        let remaining = deadline.duration_since(now);
        if remaining > TERMINAL_SIZE_POLL_INTERVAL {
            std::thread::sleep(TERMINAL_SIZE_POLL_INTERVAL);
            let _ = check_size()?;
        } else {
            std::thread::sleep(remaining);
        }
    }
}

fn wait_with_size_checks(
    duration: Duration,
    check_size: &mut impl FnMut() -> io::Result<(u16, u16)>,
) -> io::Result<()> {
    let started_at = std::time::Instant::now();
    while started_at.elapsed() < duration {
        let _ = check_size()?;
        let remaining = duration.saturating_sub(started_at.elapsed());
        std::thread::sleep(remaining.min(TERMINAL_SIZE_POLL_INTERVAL));
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
        .expect("a first visible banner line guarantees a last one");
    &banner_lines[first..=last]
}

fn matrix_glyph(row: usize, column: usize, tick: u32) -> char {
    MATRIX_GLYPHS[(hash(row, column, tick + 101) as usize) % MATRIX_GLYPHS.len()] as char
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

    #[derive(Default)]
    struct CountingWriter {
        bytes: Vec<u8>,
        writes: usize,
        flushes: usize,
    }

    impl Write for CountingWriter {
        fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
            self.writes += 1;
            self.bytes.extend_from_slice(buffer);
            Ok(buffer.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            self.flushes += 1;
            Ok(())
        }
    }

    fn assets() -> ui::RuntimeAsciiAssets {
        let asset_root =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../ascii-assets/assets");
        let store = ui::AsciiAssetStore::load_with_root(asset_root, "default")
            .expect("canonical ASCII assets");
        ui::RuntimeAsciiAssets::from_store(store)
    }

    #[test]
    fn matrix_final_frame_is_only_the_centered_white_banner() {
        let assets = assets();
        let banner = visible_banner_lines(
            assets
                .banner_lines(BANNER_ASSET_KEY)
                .expect("default banner"),
        );
        let frame = matrix_frame(banner, (120, 40), 1.45, 1.0, Color::White);
        let banner_width = banner
            .iter()
            .map(|line| line.chars().count())
            .max()
            .unwrap();
        let origin_row = (40 - banner.len()) / 2;
        let origin_column = (120 - banner_width) / 2;

        for (row, cells) in frame.iter().enumerate() {
            for (column, cell) in cells.iter().enumerate() {
                let expected = row
                    .checked_sub(origin_row)
                    .and_then(|banner_row| banner.get(banner_row))
                    .and_then(|line| {
                        column
                            .checked_sub(origin_column)
                            .map(|column| (line, column))
                    })
                    .and_then(|(line, column)| line.chars().nth(column))
                    .filter(|glyph| *glyph != ' ');
                match (cell, expected) {
                    (Some(cell), Some(glyph)) => {
                        assert_eq!(cell.glyph, glyph);
                        assert_eq!(cell.tone, MatrixTone::Banner(Color::White));
                    }
                    (None, None) => {}
                    _ => panic!("matrix final frame differs at ({column}, {row})"),
                }
            }
        }
    }

    #[test]
    fn matrix_midpoint_has_full_screen_green_rain_and_falling_white_glyphs() {
        let assets = assets();
        let banner = visible_banner_lines(
            assets
                .banner_lines(BANNER_ASSET_KEY)
                .expect("default banner"),
        );
        let frame = matrix_frame(banner, (120, 40), 1.225, 0.5, Color::White);
        let green_count = frame
            .iter()
            .flatten()
            .flatten()
            .filter(|cell| !matches!(cell.tone, MatrixTone::Banner(_)))
            .count();
        let white_count = frame
            .iter()
            .flatten()
            .flatten()
            .filter(|cell| cell.tone == MatrixTone::Banner(Color::White))
            .count();
        let occupied_rows = frame
            .iter()
            .filter(|row| row.iter().any(Option::is_some))
            .count();

        assert!(green_count > white_count);
        assert!(white_count > 0);
        assert!(occupied_rows > frame.len() / 2);
    }

    #[test]
    fn matrix_frame_is_submitted_to_the_terminal_in_one_write() {
        let frame = vec![
            vec![
                Some(MatrixCell {
                    glyph: 'A',
                    tone: MatrixTone::Green,
                }),
                None,
            ],
            vec![
                None,
                Some(MatrixCell {
                    glyph: 'B',
                    tone: MatrixTone::White,
                }),
            ],
        ];
        let mut output = CountingWriter::default();
        let mut terminal_buffer = String::new();

        render_matrix_frame(&mut output, &frame, &mut terminal_buffer).expect("rendered frame");

        assert_eq!(output.writes, 1);
        assert_eq!(output.flushes, 1);
        assert_eq!(output.bytes, terminal_buffer.as_bytes());
    }

    #[test]
    fn production_timing_runs_rain_before_assembly_and_holds_for_one_second() {
        assert_eq!(MatrixTiming::PRODUCTION.rain, Duration::from_secs(1));
        assert_eq!(
            MatrixTiming::PRODUCTION.assemble,
            Duration::from_millis(1_200)
        );
        assert_eq!(MatrixTiming::PRODUCTION.hold, Duration::from_secs(1));

        let mut frame = vec![vec![None; 120]; 40];
        let assets = assets();
        let banner = visible_banner_lines(
            assets
                .banner_lines(BANNER_ASSET_KEY)
                .expect("default banner"),
        );
        render_falling_banner_glyphs(&mut frame, banner, 0.0, Color::White);
        assert!(frame.iter().flatten().all(Option::is_none));
    }

    #[test]
    fn zero_timing_sequence_renders_white_banner_and_resets_the_terminal() {
        let assets = assets();
        let first_visible_line = assets
            .banner_lines(BANNER_ASSET_KEY)
            .unwrap()
            .iter()
            .find(|line| !line.trim().is_empty())
            .unwrap();
        let mut output = Vec::new();

        display_first_run_banner_with_timing_and_size_check(
            &mut output,
            &assets,
            MatrixTiming::ZERO,
            Color::White,
            || Ok((120, 40)),
        )
        .expect("zero-duration Matrix sequence");

        let output = String::from_utf8(output).unwrap();
        assert!(output.contains(&MatrixTone::Banner(Color::White).ansi()));
        assert!(output.contains(first_visible_line.trim_end()));
        assert!(output.ends_with(&format!("{RESET_STYLE}{CLEAR_SCREEN}")));
    }

    #[test]
    fn matrix_animation_stops_when_terminal_size_check_fails() {
        let assets = assets();
        let mut output = Vec::new();
        let mut checks = 0;

        let error = display_first_run_banner_with_timing_and_size_check(
            &mut output,
            &assets,
            MatrixTiming::ZERO,
            Color::White,
            || {
                checks += 1;
                if checks >= 2 {
                    Err(io::Error::other("terminal became too small"))
                } else {
                    Ok((120, 40))
                }
            },
        )
        .expect_err("failed size check stops the Matrix sequence");

        assert!(error.to_string().contains("too small"));
        assert!(!String::from_utf8(output).unwrap().ends_with(CLEAR_SCREEN));
    }

    #[test]
    fn matrix_banner_uses_requested_rgb_tone_while_rain_stays_green() {
        let assets = assets();
        let banner = visible_banner_lines(
            assets
                .banner_lines(BANNER_ASSET_KEY)
                .expect("default banner"),
        );
        let color = Color::Rgb(18, 52, 86);
        let frame = matrix_frame(banner, (120, 40), 1.225, 0.5, color);
        assert!(
            frame
                .iter()
                .flatten()
                .flatten()
                .any(|cell| cell.tone == MatrixTone::Banner(color))
        );
        assert!(frame.iter().flatten().flatten().any(|cell| {
            matches!(
                cell.tone,
                MatrixTone::DimGreen | MatrixTone::Green | MatrixTone::BrightGreen
            )
        }));

        let mut output = CountingWriter::default();
        let mut terminal_buffer = String::new();
        render_matrix_frame(&mut output, &frame, &mut terminal_buffer)
            .expect("colored Matrix frame renders");
        let output = String::from_utf8(output.bytes).expect("UTF-8 terminal output");
        assert!(output.contains("\x1B[38;2;18;52;86m"));
        assert!(output.contains(MatrixTone::Green.ansi().as_str()));
        assert!(output.ends_with(BLACK_BACKGROUND));
    }
}
