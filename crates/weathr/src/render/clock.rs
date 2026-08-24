use crate::ClockFormat;
use crate::render::TerminalRenderer;
use chrono::{DateTime, NaiveDateTime, NaiveTime, Timelike};
use crossterm::style::Color;
use std::collections::HashMap;
use std::io;
use std::time::Duration;
use thiserror::Error as ThisError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClockLayout {
    pub col: u16,
    pub row: u16,
}

#[derive(Debug)]
pub struct ClockFont {
    height: usize,
    spacing: usize,
    separator_spacing: usize,
    glyphs: HashMap<char, Vec<String>>,
}

#[derive(Debug, ThisError, PartialEq, Eq)]
pub enum ClockFontError {
    #[error("clock font height must be greater than zero")]
    EmptyHeight,

    #[error("clock font glyph {glyph:?} has {actual} rows, expected {expected}")]
    GlyphHeight {
        glyph: char,
        actual: usize,
        expected: usize,
    },

    #[error("clock font is missing required glyph {0:?}")]
    MissingGlyph(char),
}

impl ClockFont {
    pub(crate) fn from_static(
        height: usize,
        spacing: usize,
        separator_spacing: usize,
        glyphs: &[(char, &[&str])],
    ) -> Result<Self, ClockFontError> {
        if height == 0 {
            return Err(ClockFontError::EmptyHeight);
        }

        let mut loaded_glyphs = HashMap::new();
        for &(glyph, lines) in glyphs {
            if lines.len() != height {
                return Err(ClockFontError::GlyphHeight {
                    glyph,
                    actual: lines.len(),
                    expected: height,
                });
            }
            loaded_glyphs.insert(
                glyph,
                pad_glyph_lines(lines.iter().map(|line| (*line).to_string()).collect()),
            );
        }

        for required in required_glyphs() {
            if !loaded_glyphs.contains_key(&required) {
                return Err(ClockFontError::MissingGlyph(required));
            }
        }

        Ok(Self {
            height,
            spacing,
            separator_spacing,
            glyphs: loaded_glyphs,
        })
    }

    pub fn height(&self) -> usize {
        self.height
    }

    pub(crate) fn max_rendered_clock_width(&self) -> usize {
        let glyph_width = |glyph: char| {
            self.glyphs
                .get(&glyph)
                .and_then(|lines| lines.iter().map(|line| line.chars().count()).max())
                .unwrap_or(0)
        };
        let digit_width = "0123456789".chars().map(glyph_width).max().unwrap_or(0);
        let suffix_width = glyph_width('A').max(glyph_width('P'));

        digit_width
            .saturating_mul(4)
            .saturating_add(glyph_width(':'))
            .saturating_add(glyph_width(' '))
            .saturating_add(suffix_width)
            .saturating_add(glyph_width('M'))
            .saturating_add(self.separator_spacing.saturating_mul(2))
            .saturating_add(self.spacing.saturating_mul(5))
    }
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

pub fn ascii_lines(text: &str, font: &ClockFont) -> Vec<String> {
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

pub fn ascii_clock_lines(text: &str, font: &ClockFont) -> Vec<String> {
    ascii_lines(text, font)
}

pub fn clock_height(font: &ClockFont) -> usize {
    font.height()
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
    font: &ClockFont,
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

    let Some(anchor) = separator_anchor_offset(text, font) else {
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
    font: &ClockFont,
    width: u16,
    height: u16,
    color: Color,
) -> io::Result<()> {
    let text = format_time(time, format);
    let lines = ascii_lines(&text, font);
    let layout = separator_anchored_layout(&text, &lines, font, width, height);

    for (idx, line) in lines.iter().enumerate() {
        renderer.render_line_colored(layout.col, layout.row + idx as u16, line, color)?;
    }

    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct SeparatorAnchor {
    offset: usize,
    width: usize,
}

fn separator_anchor_offset(text: &str, font: &ClockFont) -> Option<SeparatorAnchor> {
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
        line.push_str(&" ".repeat(padding));
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

    fn test_glyphs() -> Vec<(char, Vec<String>)> {
        let mut glyphs = Vec::new();
        for ch in "0123456789".chars() {
            glyphs.push((ch, vec![format!("{ch}{ch}"), format!("{ch}{ch}")]));
        }
        glyphs.retain(|(glyph, _)| *glyph != '1');
        glyphs.push(('1', vec!["1".to_string(), "1".to_string()]));
        glyphs.push((':', vec!["##".to_string(), "##".to_string()]));
        glyphs.push((' ', vec![" ".to_string(), " ".to_string()]));
        glyphs.push(('A', vec!["AA".to_string(), "AA".to_string()]));
        glyphs.push(('P', vec!["PP".to_string(), "PP".to_string()]));
        glyphs.push(('M', vec!["MM".to_string(), "MM".to_string()]));
        glyphs
    }

    fn font_from_glyphs(glyphs: &[(char, Vec<String>)]) -> Result<ClockFont, ClockFontError> {
        let borrowed = glyphs
            .iter()
            .map(|(glyph, lines)| (*glyph, lines.iter().map(String::as_str).collect::<Vec<_>>()))
            .collect::<Vec<_>>();
        let definition = borrowed
            .iter()
            .map(|(glyph, lines)| (*glyph, lines.as_slice()))
            .collect::<Vec<_>>();
        ClockFont::from_static(2, 1, 5, &definition)
    }

    fn test_font() -> ClockFont {
        font_from_glyphs(&test_glyphs()).expect("test font definition is valid")
    }

    #[test]
    fn ascii_output_has_stable_height() {
        let font = test_font();
        let lines = ascii_clock_lines("12:34 PM", &font);
        assert_eq!(lines.len(), clock_height(&font));
        assert!(lines.iter().all(|line| !line.is_empty()));
    }

    #[test]
    fn adapts_clock_font_definition_and_pads_glyph_rows() {
        let mut glyphs = test_glyphs();
        glyphs
            .iter_mut()
            .find(|(glyph, _)| *glyph == '0')
            .expect("zero glyph exists")
            .1 = vec!["0".to_string(), "00".to_string()];

        let font = font_from_glyphs(&glyphs).expect("definition adapts");
        let zero = font.glyphs.get(&'0').unwrap();
        assert_eq!(zero, &vec!["0 ".to_string(), "00".to_string()]);
    }

    #[test]
    fn clock_separator_has_wide_spacing_on_both_sides() {
        let font = test_font();
        let lines = ascii_clock_lines("1:2", &font);
        let row = &lines[1];
        let one = font.glyphs.get(&'1').unwrap();
        let colon = font.glyphs.get(&':').unwrap();

        let left_gap_start = one[1].len();
        let colon_start = left_gap_start + 5;
        let right_gap_start = colon_start + colon[1].len();

        assert_eq!(&row[left_gap_start..colon_start], "     ");
        assert_eq!(&row[colon_start..right_gap_start], colon[1].as_str());
        assert_eq!(&row[right_gap_start..right_gap_start + 5], "     ");
    }

    #[test]
    fn layout_centers_above_middle() {
        let font = test_font();
        let lines = ascii_lines("12:34", &font);
        let layout = centered_layout(&lines, 100, 30);

        assert!(layout.col < 50);
        assert_eq!(
            layout.row,
            center_above_start(0, lines.len() as u16, 0, 30).row
        );
    }

    #[test]
    fn separator_anchored_layout_keeps_colon_fixed() {
        let font = test_font();

        fn separator_col(text: &str, font: &ClockFont) -> u16 {
            let lines = ascii_lines(text, font);
            let layout = separator_anchored_layout(text, &lines, font, 100, 30);
            let anchor = separator_anchor_offset(text, font).unwrap();
            layout.col + anchor.offset as u16
        }

        let expected = {
            let anchor = separator_anchor_offset("12:00", &font).unwrap();
            100_u16.saturating_sub(anchor.width as u16) / 2
        };

        assert_eq!(separator_col("12:00", &font), separator_col("12:11", &font));
        assert_eq!(separator_col("09:59", &font), separator_col("10:00", &font));
        assert_eq!(separator_col("12:00", &font), expected);
    }

    #[test]
    fn separator_anchor_takes_priority_over_right_edge_fit() {
        let font = test_font();
        let text = "12:05 AM";
        let lines = ascii_lines(text, &font);
        let layout = separator_anchored_layout(text, &lines, &font, 70, 30);
        let anchor = separator_anchor_offset(text, &font).unwrap();

        assert_eq!(
            layout.col + anchor.offset as u16,
            70_u16.saturating_sub(anchor.width as u16) / 2
        );
    }

    #[test]
    fn layout_clamps_when_content_exceeds_area() {
        let font = test_font();
        assert_eq!(
            center_above_start(120, clock_height(&font) as u16, 80, 4),
            ClockLayout { col: 0, row: 0 }
        );
    }

    #[test]
    fn clock_font_definition_requires_required_glyphs() {
        let mut glyphs = test_glyphs();
        glyphs.retain(|(glyph, _)| *glyph != 'A');

        let err = font_from_glyphs(&glyphs).unwrap_err();
        assert_eq!(err, ClockFontError::MissingGlyph('A'));
    }

    #[test]
    fn clock_font_definition_rejects_wrong_glyph_height() {
        let mut glyphs = test_glyphs();
        glyphs
            .iter_mut()
            .find(|(glyph, _)| *glyph == '0')
            .expect("zero glyph exists")
            .1 = vec!["only one row".to_string()];

        let err = font_from_glyphs(&glyphs).unwrap_err();
        assert_eq!(
            err,
            ClockFontError::GlyphHeight {
                glyph: '0',
                actual: 1,
                expected: 2,
            }
        );
    }
}
