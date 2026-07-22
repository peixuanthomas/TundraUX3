use super::document::{DisplayLine, DisplayRun, empty_display_line};
use super::*;

/// Replaces bytes that a terminal could interpret as control traffic with
/// visible, inert Unicode glyphs. This must be applied at the final rendering
/// boundary as documents, file names, and status messages are all untrusted.
///
/// C0 controls use the Unicode Control Pictures block, DEL uses its dedicated
/// symbol, and C1/bidirectional formatting controls use the replacement glyph.
/// Newlines are laid out structurally before this boundary; if one reaches this
/// function it is shown visibly instead of being emitted to the terminal.
pub fn terminal_safe_text(value: &str) -> Cow<'_, str> {
    if !value.chars().any(is_terminal_unsafe_character) {
        return Cow::Borrowed(value);
    }

    Cow::Owned(value.chars().map(terminal_safe_character).collect())
}

pub(super) fn terminal_safe_character(character: char) -> char {
    match character {
        '\u{0000}'..='\u{001f}' => {
            char::from_u32(0x2400 + u32::from(character)).unwrap_or('\u{fffd}')
        }
        '\u{007f}' => '\u{2421}',
        '\u{0080}'..='\u{009f}' => '\u{fffd}',
        character if is_unsafe_format_character(character) => '\u{fffd}',
        character => character,
    }
}

pub(super) fn is_terminal_unsafe_character(character: char) -> bool {
    character.is_control() || is_unsafe_format_character(character)
}

pub(super) fn is_unsafe_format_character(character: char) -> bool {
    matches!(
        character,
        '\u{061c}'
            | '\u{200e}'
            | '\u{200f}'
            | '\u{202a}'..='\u{202e}'
            | '\u{2060}'..='\u{206f}'
            | '\u{feff}'
    )
}

pub(super) fn source_document_line_count(model: &EditorViewModel) -> usize {
    if let Some(window) = &model.source_window {
        return window.total_line_count.max(1);
    }
    if let Some(ranges) = cached_source_line_ranges(model) {
        return ranges.len().max(1);
    }
    model
        .source
        .as_deref()
        .map(source_line_count)
        .unwrap_or_else(|| model.source_lines.len().max(1))
}

pub(super) fn source_display_lines_for_viewport(
    model: &EditorViewModel,
    start: usize,
    end: usize,
) -> Vec<DisplayLine> {
    if let Some(window) = &model.source_window {
        return (start..end)
            .map(|document_line| {
                document_line
                    .checked_sub(window.first_line)
                    .and_then(|relative| window.lines.get(relative))
                    .map_or_else(
                        || empty_display_line(None),
                        |line| DisplayLine {
                            runs: vec![DisplayRun::shared_source(
                                Arc::clone(&line.text),
                                line.visible_byte_range,
                            )],
                            block_index: None,
                            no_wrap: true,
                            column_start: line.start_column,
                        },
                    )
            })
            .collect();
    }
    if let Some(source) = model.source.as_deref() {
        let fallback;
        let ranges = if let Some(ranges) = cached_source_line_ranges(model) {
            ranges
        } else {
            fallback = source_display_line_ranges(source);
            &fallback
        };
        return ranges
            .get(start.min(ranges.len())..end.min(ranges.len()))
            .unwrap_or_default()
            .iter()
            .copied()
            .map(|range| DisplayLine {
                runs: vec![DisplayRun::source_range(range)],
                block_index: None,
                no_wrap: true,
                column_start: 0,
            })
            .collect();
    }

    model
        .source_lines
        .get(start.min(model.source_lines.len())..end.min(model.source_lines.len()))
        .unwrap_or_default()
        .iter()
        .map(|line| DisplayLine {
            runs: vec![DisplayRun::unmapped(
                line.clone(),
                EditorRenderSpan::plain(""),
            )],
            block_index: None,
            no_wrap: true,
            column_start: 0,
        })
        .collect()
}

pub(super) fn cached_source_line_ranges(model: &EditorViewModel) -> Option<&[EditorSourceRange]> {
    let source = model.source.as_deref()?;
    let ranges = model.source_line_ranges.as_slice();
    let valid = !ranges.is_empty()
        && ranges.first().is_some_and(|range| range.start == 0)
        && ranges.last().is_some_and(|range| range.end == source.len());
    valid.then_some(ranges)
}

pub(super) fn source_line_count(source: &str) -> usize {
    let bytes = source.as_bytes();
    let mut lines = 1usize;
    let mut index = 0usize;
    while index < bytes.len() {
        match bytes[index] {
            b'\r' if bytes.get(index + 1) == Some(&b'\n') => {
                lines = lines.saturating_add(1);
                index += 2;
            }
            b'\r' | b'\n' => {
                lines = lines.saturating_add(1);
                index += 1;
            }
            _ => index += 1,
        }
    }
    lines
}

pub(super) fn source_display_line_ranges(source: &str) -> Vec<EditorSourceRange> {
    let bytes = source.as_bytes();
    let mut lines = Vec::new();
    let mut start = 0usize;
    let mut index = 0usize;
    while index < bytes.len() {
        match bytes[index] {
            b'\r' if bytes.get(index + 1) == Some(&b'\n') => {
                lines.push(EditorSourceRange::new(start, index));
                index += 2;
                start = index;
            }
            b'\r' | b'\n' => {
                lines.push(EditorSourceRange::new(start, index));
                index += 1;
                start = index;
            }
            _ => index += 1,
        }
    }
    lines.push(EditorSourceRange::new(start, source.len()));
    lines
}

pub(super) fn source_horizontal_content_width(source: &str, ranges: &[EditorSourceRange]) -> usize {
    ranges
        .iter()
        .filter_map(|range| source.get(range.start..range.end))
        .map(|line| Span::raw(terminal_safe_text(line)).width())
        .max()
        .unwrap_or(0)
        .saturating_add(1)
        .max(1)
}
