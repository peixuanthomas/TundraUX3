//! Display rows are a projection of the Rope, never inserted newlines.
use super::{SourceRange, source_grapheme_display_width};
use ropey::Rope;
use std::borrow::Cow;
use std::sync::{Arc, Mutex};
use unicode_segmentation::UnicodeSegmentation;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WrapRow {
    pub range: SourceRange,
    pub logical_line: usize,
    pub continuation: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WrapLayout {
    pub width: usize,
    pub rows: Vec<WrapRow>,
    source: Rope,
}

impl WrapLayout {
    pub fn new(source: &Rope, width: usize) -> Self {
        let width = width.max(1);
        // Reserve the final cell for a caret at EOL, including exact-width lines.
        let content_width = width.saturating_sub(1).max(1);
        let mut rows = Vec::new();
        let mut offset = 0;
        for (logical_line, line) in source.lines().enumerate() {
            let text = Cow::from(line);
            let text = text.trim_end_matches(['\r', '\n']);
            let mut start = 0;
            let mut column = 0;
            for (byte, grapheme) in text.grapheme_indices(true) {
                let cells = source_grapheme_display_width(grapheme).max(1);
                if column > 0 && column + cells > content_width {
                    rows.push(WrapRow {
                        range: SourceRange::new(offset + start, offset + byte),
                        logical_line,
                        continuation: start > 0,
                    });
                    start = byte;
                    column = 0;
                }
                column += cells;
            }
            rows.push(WrapRow {
                range: SourceRange::new(offset + start, offset + text.len()),
                logical_line,
                continuation: start > 0,
            });
            offset += line.len_bytes();
        }
        Self {
            width,
            rows,
            source: source.clone(),
        }
    }

    pub fn text(&self, row: usize) -> String {
        let range = self.rows[row].range;
        self.source.byte_slice(range.start..range.end).to_string()
    }

    pub fn position(&self, byte: usize) -> (usize, usize) {
        // At a soft break the caret belongs to the following row.
        let row = self
            .rows
            .partition_point(|r| r.range.start <= byte)
            .saturating_sub(1);
        let range = self.rows[row].range;
        let text = self.text(row);
        let column = text
            .grapheme_indices(true)
            .take_while(|(i, _)| range.start + i < byte.min(range.end))
            .map(|(_, g)| source_grapheme_display_width(g).max(1))
            .sum();
        (row, column)
    }

    pub fn offset(&self, row: usize, column: usize) -> usize {
        let row = row.min(self.rows.len() - 1);
        let range = self.rows[row].range;
        let text = self.text(row);
        let mut cells = 0;
        for (byte, grapheme) in text.grapheme_indices(true) {
            let width = source_grapheme_display_width(grapheme).max(1);
            if cells + width > column {
                return range.start + byte;
            }
            cells += width;
        }
        // Do not jump to the following display row when clamping a vertical move.
        if self
            .rows
            .get(row + 1)
            .is_some_and(|next| next.range.start == range.end)
        {
            range.start + text.grapheme_indices(true).last().map_or(0, |(i, _)| i)
        } else {
            range.end
        }
    }
}

#[derive(Debug, Default)]
pub(super) struct Cache(Mutex<Option<Arc<WrapLayout>>>);
impl Clone for Cache {
    fn clone(&self) -> Self {
        Self(Mutex::new(self.0.lock().unwrap().clone()))
    }
}
impl PartialEq for Cache {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}
impl Eq for Cache {}
impl Cache {
    pub fn get(&self, source: &Rope, width: usize) -> Arc<WrapLayout> {
        let mut cache = self.0.lock().unwrap();
        if let Some(layout) = &*cache
            && layout.width == width.max(1)
        {
            return Arc::clone(layout);
        }
        let layout = Arc::new(WrapLayout::new(source, width));
        *cache = Some(Arc::clone(&layout));
        layout
    }
}
