use crate::config::ClockFormat;
use crate::render::TerminalRenderer;
use chrono::{DateTime, NaiveDateTime, NaiveTime, Timelike};
use crossterm::style::Color;
use serde::Deserialize;
use std::collections::HashMap;
use std::io;
use std::sync::OnceLock;
use std::time::Duration;

const CLOCK_FONT_SOURCE: &str = include_str!("assets/clock_font.toml");

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClockLayout {
    pub col: u16,
    pub row: u16,
}

#[derive(Debug)]
struct ClockFont {
    height: usize,
    spacing: usize,
    separator_spacing: usize,
    glyphs: HashMap<char, Vec<String>>,
}

#[derive(Debug, Deserialize)]
struct ClockFontFile {
    #[allow(dead_code)]
    name: Option<String>,
    height: usize,
    #[serde(default = "default_spacing")]
    spacing: usize,
    #[serde(default = "default_separator_spacing")]
    separator_spacing: usize,
    glyphs: HashMap<String, Vec<String>>,
}

fn default_spacing() -> usize {
    1
}

fn default_separator_spacing() -> usize {
    default_spacing()
}

pub fn parse_local_datetime(timestamp: &str) -> Option<NaiveDateTime> {
    if let Ok(dt) = DateTime::parse_from_rfc3339(timestamp) {
        return Some(dt.naive_local());
    }

    if let Ok(dt) = NaiveDateTime::parse_from_str(timestamp, "%Y-%m-%dT%H:%M:%S") {
        return Some(dt);
    }

    if let Ok(dt) = NaiveDateTime::parse_from_str(timestamp, "%Y-%m-%dT%H:%M") {
        return Some(dt);
    }

    None
}

pub fn advance_time(anchor: NaiveDateTime, elapsed: Duration) -> NaiveTime {
    let elapsed = chrono::Duration::from_std(elapsed).unwrap_or_else(|_| chrono::Duration::zero());
    (anchor + elapsed).time()
}

pub fn format_time(time: NaiveTime, format: ClockFormat) -> String {
    match format {
        ClockFormat::TwentyFourHour => format!("{:02}:{:02}", time.hour(), time.minute()),
        ClockFormat::TwelveHour => {
            let hour = time.hour();
            let suffix = if hour < 12 { "AM" } else { "PM" };
            let hour = match hour % 12 {
                0 => 12,
                value => value,
            };
            format!("{:02}:{:02} {}", hour, time.minute(), suffix)
        }
    }
}

pub fn format_local_time(time: NaiveTime, format: ClockFormat) -> String {
    format_time(time, format)
}

pub fn ascii_lines(text: &str) -> Vec<String> {
    let font = clock_font();
    let mut lines = vec![String::new(); font.height];
    let chars: Vec<char> = text.chars().collect();

    for (idx, ch) in chars.iter().copied().enumerate() {
        if idx > 0 {
            let gap = if ch == ':' || chars[idx - 1] == ':' {
                font.separator_spacing
            } else {
                font.spacing
            };
            let spacing = " ".repeat(gap);
            for line in &mut lines {
                line.push_str(&spacing);
            }
        }

        let glyph = font
            .glyphs
            .get(&ch)
            .or_else(|| font.glyphs.get(&' '))
            .expect("bundled clock font must include a space glyph");
        for (line, segment) in lines.iter_mut().zip(glyph.iter()) {
            line.push_str(segment);
        }
    }

    lines
}

pub fn ascii_clock_lines(text: &str) -> Vec<String> {
    ascii_lines(text)
}

pub fn clock_height() -> usize {
    clock_font().height
}

pub fn center_above_start(
    content_width: u16,
    content_height: u16,
    area_width: u16,
    area_height: u16,
) -> ClockLayout {
    let col = area_width.saturating_sub(content_width) / 2;
    let target_row = area_height / 3;
    let row = target_row.saturating_sub(content_height / 2);

    ClockLayout { col, row }
}

pub fn centered_layout(lines: &[String], width: u16, height: u16) -> ClockLayout {
    let max_width = lines
        .iter()
        .map(|line| line.chars().count() as u16)
        .max()
        .unwrap_or(0);

    let clock_height = lines.len() as u16;
    center_above_start(max_width, clock_height, width, height)
}

pub fn separator_anchored_layout(
    text: &str,
    lines: &[String],
    width: u16,
    height: u16,
) -> ClockLayout {
    let max_width = lines
        .iter()
        .map(|line| line.chars().count())
        .max()
        .unwrap_or(0);
    let clock_height = lines.len() as u16;
    let row = center_above_start(max_width as u16, clock_height, width, height).row;

    let Some(anchor) = separator_anchor_offset(text) else {
        return ClockLayout {
            col: centered_layout(lines, width, height).col,
            row,
        };
    };

    let separator_col = width.saturating_sub(anchor.width as u16) as isize / 2;
    let desired_col = separator_col - anchor.offset as isize;
    let col = desired_col.max(0) as u16;

    ClockLayout { col, row }
}

pub fn render_clock(
    renderer: &mut TerminalRenderer,
    time: NaiveTime,
    format: ClockFormat,
    width: u16,
    height: u16,
    color: Color,
) -> io::Result<()> {
    let text = format_time(time, format);
    let lines = ascii_lines(&text);
    let layout = separator_anchored_layout(&text, &lines, width, height);

    for (idx, line) in lines.iter().enumerate() {
        renderer.render_line_colored(layout.col, layout.row + idx as u16, line, color)?;
    }

    Ok(())
}

fn clock_font() -> &'static ClockFont {
    static FONT: OnceLock<ClockFont> = OnceLock::new();
    FONT.get_or_init(|| {
        parse_clock_font(CLOCK_FONT_SOURCE).expect("bundled clock font must be valid")
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct SeparatorAnchor {
    offset: usize,
    width: usize,
}

fn separator_anchor_offset(text: &str) -> Option<SeparatorAnchor> {
    let font = clock_font();
    let chars: Vec<char> = text.chars().collect();
    let separator_target = chars
        .iter()
        .enumerate()
        .filter_map(|(idx, ch)| (*ch == ':').then_some(idx))
        .collect::<Vec<_>>();
    let separator_target = separator_target.get(separator_target.len() / 2).copied()?;

    let mut offset = 0;
    for (idx, ch) in chars.iter().copied().enumerate() {
        if idx > 0 {
            offset += if ch == ':' || chars[idx - 1] == ':' {
                font.separator_spacing
            } else {
                font.spacing
            };
        }

        let glyph_width = glyph_width(font, ch);
        if idx == separator_target {
            return Some(SeparatorAnchor {
                offset,
                width: glyph_width,
            });
        }

        offset += glyph_width;
    }

    None
}

fn glyph_width(font: &ClockFont, ch: char) -> usize {
    font.glyphs
        .get(&ch)
        .or_else(|| font.glyphs.get(&' '))
        .map(|glyph| {
            glyph
                .iter()
                .map(|line| line.chars().count())
                .max()
                .unwrap_or(0)
        })
        .unwrap_or(0)
}

fn parse_clock_font(source: &str) -> Result<ClockFont, String> {
    let file: ClockFontFile =
        toml::from_str(source).map_err(|e| format!("invalid clock font TOML: {e}"))?;

    if file.height == 0 {
        return Err("clock font height must be greater than zero".to_string());
    }

    let mut glyphs = HashMap::new();
    for (key, lines) in file.glyphs {
        let mut chars = key.chars();
        let Some(ch) = chars.next() else {
            return Err("clock font glyph key cannot be empty".to_string());
        };
        if chars.next().is_some() {
            return Err(format!(
                "clock font glyph key {key:?} must be one character"
            ));
        }
        if lines.len() != file.height {
            return Err(format!(
                "clock font glyph {key:?} has {} rows, expected {}",
                lines.len(),
                file.height
            ));
        }
        glyphs.insert(ch, pad_glyph_lines(lines));
    }

    for required in required_glyphs() {
        if !glyphs.contains_key(&required) {
            return Err(format!("clock font is missing required glyph {required:?}"));
        }
    }

    Ok(ClockFont {
        height: file.height,
        spacing: file.spacing,
        separator_spacing: file.separator_spacing,
        glyphs,
    })
}

fn required_glyphs() -> impl Iterator<Item = char> {
    "0123456789: APM".chars()
}

fn pad_glyph_lines(mut lines: Vec<String>) -> Vec<String> {
    let width = lines
        .iter()
        .map(|line| line.chars().count())
        .max()
        .unwrap_or(0);

    for line in &mut lines {
        let padding = width.saturating_sub(line.chars().count());
        line.extend(std::iter::repeat(' ').take(padding));
    }

    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveTime;

    #[test]
    fn formats_twenty_four_hour_time() {
        let time = NaiveTime::from_hms_opt(0, 5, 30).unwrap();
        let evening = NaiveTime::from_hms_opt(23, 59, 0).unwrap();

        assert_eq!(
            format_local_time(time, ClockFormat::TwentyFourHour),
            "00:05"
        );
        assert_eq!(
            format_local_time(evening, ClockFormat::TwentyFourHour),
            "23:59"
        );
    }

    #[test]
    fn formats_twelve_hour_time() {
        let midnight = NaiveTime::from_hms_opt(0, 5, 0).unwrap();
        let noon = NaiveTime::from_hms_opt(12, 30, 0).unwrap();
        let evening = NaiveTime::from_hms_opt(23, 59, 0).unwrap();

        assert_eq!(
            format_local_time(midnight, ClockFormat::TwelveHour),
            "12:05 AM"
        );
        assert_eq!(format_local_time(noon, ClockFormat::TwelveHour), "12:30 PM");
        assert_eq!(
            format_local_time(evening, ClockFormat::TwelveHour),
            "11:59 PM"
        );
    }

    #[test]
    fn parses_open_meteo_local_timestamp() {
        let parsed = parse_local_datetime("2026-07-08T15:45").unwrap();
        assert_eq!(parsed.time(), NaiveTime::from_hms_opt(15, 45, 0).unwrap());
    }

    #[test]
    fn ascii_output_has_stable_height() {
        let lines = ascii_clock_lines("12:34 PM");
        assert_eq!(lines.len(), clock_height());
        assert!(lines.iter().all(|line| !line.is_empty()));
    }

    #[test]
    fn renders_expected_ascii_glyphs() {
        let zero = ascii_clock_lines("0");
        assert_eq!(zero.len(), 7);
        assert!(zero[0].contains(".oooo."));
        assert!(zero[6].contains("Y8bd8P"));

        let colon = ascii_clock_lines(":");
        assert_eq!(colon.len(), clock_height());
        assert_eq!(colon[1].trim(), "##");

        let letters = ascii_clock_lines("APM");
        assert_eq!(letters.len(), clock_height());
        assert!(letters[0].contains("###"));
        assert!(letters[0].contains("######"));
    }

    #[test]
    fn clock_separator_has_wide_spacing_on_both_sides() {
        let lines = ascii_clock_lines("1:2");
        let row = &lines[1];
        let one = clock_font().glyphs.get(&'1').unwrap();
        let colon = clock_font().glyphs.get(&':').unwrap();

        let left_gap_start = one[1].len();
        let colon_start = left_gap_start + 5;
        let right_gap_start = colon_start + colon[1].len();

        assert_eq!(&row[left_gap_start..colon_start], "     ");
        assert_eq!(row[colon_start..right_gap_start].trim(), "##");
        assert_eq!(&row[right_gap_start..right_gap_start + 5], "     ");
    }

    #[test]
    fn layout_centers_above_middle() {
        let lines = ascii_lines("12:34");
        let layout = centered_layout(&lines, 100, 30);

        assert!(layout.col < 50);
        assert_eq!(layout.row, 7);
    }

    #[test]
    fn separator_anchored_layout_keeps_colon_fixed() {
        fn separator_col(text: &str) -> u16 {
            let lines = ascii_lines(text);
            let layout = separator_anchored_layout(text, &lines, 100, 30);
            let anchor = separator_anchor_offset(text).unwrap();
            layout.col + anchor.offset as u16
        }

        assert_eq!(separator_col("12:00"), separator_col("12:11"));
        assert_eq!(separator_col("09:59"), separator_col("10:00"));
        assert_eq!(separator_col("12:00"), 49);
    }

    #[test]
    fn separator_anchor_takes_priority_over_right_edge_fit() {
        let text = "12:05 AM";
        let lines = ascii_lines(text);
        let layout = separator_anchored_layout(text, &lines, 70, 30);
        let anchor = separator_anchor_offset(text).unwrap();

        assert_eq!(layout.col + anchor.offset as u16, 34);
    }

    #[test]
    fn layout_clamps_when_content_exceeds_area() {
        assert_eq!(
            center_above_start(120, clock_height() as u16, 80, 4),
            ClockLayout { col: 0, row: 0 }
        );
    }

    #[test]
    fn bundled_font_file_parses() {
        let font = parse_clock_font(CLOCK_FONT_SOURCE).expect("bundled font parses");

        assert_eq!(font.height, 7);
        assert_eq!(font.spacing, 1);
        assert_eq!(font.separator_spacing, 5);
        assert!(font.glyphs.contains_key(&'0'));
        assert!(font.glyphs.contains_key(&'9'));
    }

    #[test]
    fn font_parser_rejects_wrong_glyph_height() {
        let source = r#"
height = 2
spacing = 1

[glyphs]
"0" = ["only one row"]
"1" = ["a", "b"]
"2" = ["a", "b"]
"3" = ["a", "b"]
"4" = ["a", "b"]
"5" = ["a", "b"]
"6" = ["a", "b"]
"7" = ["a", "b"]
"8" = ["a", "b"]
"9" = ["a", "b"]
":" = ["a", "b"]
" " = [" ", " "]
"A" = ["a", "b"]
"P" = ["a", "b"]
"M" = ["a", "b"]
"#;

        let err = parse_clock_font(source).unwrap_err();
        assert!(err.contains("rows"));
    }
}
