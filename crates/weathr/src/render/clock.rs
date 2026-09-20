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
#[path = "../../tests/unit/render/clock/tests.rs"]
mod tests;
