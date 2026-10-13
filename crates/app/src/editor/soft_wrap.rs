//! Exact row counts, with display rows materialized only when requested.
//! The index uses one usize per logical line, not one object per display row.
use super::{
    SourceRange,
    background::{Latest, SYNC_BYTES},
    source_grapheme_display_width, source_line_content,
};
use ropey::Rope;
use std::{
    borrow::Cow,
    collections::BTreeMap,
    ops::Range,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};
use unicode_segmentation::{GraphemeCursor, GraphemeIncomplete, UnicodeSegmentation};

const CHECKPOINT_ROWS: usize = 256;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WrapRow {
    pub range: SourceRange,
    pub logical_line: usize,
    pub continuation: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WrapLayout {
    pub width: usize,
    row_ends: Vec<usize>,
    // Only long Unicode lines need sparse byte checkpoints. ASCII rows use arithmetic.
    checkpoints: BTreeMap<usize, Arc<[usize]>>,
    source: Rope,
}

impl WrapLayout {
    pub fn new(source: &Rope, width: usize) -> Self {
        Self::build(source, width, None, &[], &AtomicBool::new(false)).unwrap()
    }
    fn build(
        source: &Rope,
        width: usize,
        previous: Option<&Self>,
        edits: &[(Range<usize>, Range<usize>)],
        cancel: &AtomicBool,
    ) -> Option<Self> {
        let width = width.max(1);
        let previous = previous.filter(|old| old.width == width);
        let mut counts = Vec::new();
        let mut checkpoints = BTreeMap::new();
        if let Some(old) = previous {
            let mut end = 0;
            counts = old
                .row_ends
                .iter()
                .map(|&next| {
                    let count = next - end;
                    end = next;
                    count
                })
                .collect();
            checkpoints = old.checkpoints.clone();
            for (old, new) in edits {
                counts.splice(old.clone(), std::iter::repeat_n(0, new.len()));
                checkpoints = checkpoints
                    .into_iter()
                    .filter_map(|(line, value)| {
                        if line < old.start {
                            Some((line, value))
                        } else if line >= old.end {
                            Some((line - old.len() + new.len(), value))
                        } else {
                            None
                        }
                    })
                    .collect();
            }
        } else {
            counts.resize(source.len_lines(), 0);
        }
        let mut layout = Self {
            width,
            row_ends: Vec::with_capacity(counts.len()),
            checkpoints,
            source: source.clone(),
        };
        let mut total = 0;
        for (line, mut count) in counts.into_iter().enumerate() {
            if cancel.load(Ordering::Relaxed) {
                return None;
            }
            if count == 0 {
                let content = source_line_content(source.line(line));
                if content.len_bytes() == content.len_chars() {
                    count = content.len_bytes().div_ceil(layout.content_width()).max(1);
                } else {
                    let mut offsets = Vec::new();
                    for (row, range) in layout.line_rows(line, 0).enumerate() {
                        if row % CHECKPOINT_ROWS == 0 {
                            if cancel.load(Ordering::Relaxed) {
                                return None;
                            }
                            offsets.push(range.start - source.line_to_byte(line));
                        }
                        count += 1;
                    }
                    if offsets.len() > 1 {
                        layout.checkpoints.insert(line, offsets.into());
                    }
                }
            }
            total += count;
            layout.row_ends.push(total);
        }
        Some(layout)
    }
    fn content_width(&self) -> usize {
        self.width.saturating_sub(1).max(1)
    }
    pub fn row_count(&self) -> usize {
        self.row_ends.last().copied().unwrap_or(1)
    }
    pub fn logical_line_count(&self) -> usize {
        self.source.len_lines()
    }
    fn first_row(&self, line: usize) -> usize {
        if line == 0 {
            0
        } else {
            self.row_ends[line - 1]
        }
    }
    fn line_rows(&self, line: usize, relative: usize) -> Rows<'_> {
        let start = self.source.line_to_byte(line);
        let end = start + source_line_content(self.source.line(line)).len_bytes();
        Rows::new(&self.source, start + relative, end, self.content_width())
    }
    pub fn row(&self, row: usize) -> Option<WrapRow> {
        if row >= self.row_count() {
            return None;
        }
        let line = self.row_ends.partition_point(|&end| end <= row);
        let local = row - self.first_row(line);
        let start = self.source.line_to_byte(line);
        let content = source_line_content(self.source.line(line));
        let range = if content.len_bytes() == content.len_chars() {
            SourceRange::new(
                start + local * self.content_width(),
                start + ((local + 1) * self.content_width()).min(content.len_bytes()),
            )
        } else {
            let checkpoint = local / CHECKPOINT_ROWS;
            let offset = self
                .checkpoints
                .get(&line)
                .map_or(0, |points| points[checkpoint]);
            self.line_rows(line, offset)
                .nth(local - checkpoint * CHECKPOINT_ROWS)?
        };
        Some(WrapRow {
            range,
            logical_line: line,
            continuation: local > 0,
        })
    }
    pub fn text(&self, row: usize) -> String {
        let range = self.row(row).unwrap().range;
        self.source.byte_slice(range.start..range.end).to_string()
    }
    pub fn position(&self, byte: usize) -> (usize, usize) {
        let byte = byte.min(self.source.len_bytes());
        let line = self.source.byte_to_line(byte);
        let start = self.source.line_to_byte(line);
        let content = source_line_content(self.source.line(line));
        let relative = byte.saturating_sub(start).min(content.len_bytes());
        let (local, range_start) = if content.len_bytes() == content.len_chars() {
            let count = self.row_ends[line] - self.first_row(line);
            let row = (relative / self.content_width()).min(count - 1);
            return (
                self.first_row(line) + row,
                relative - row * self.content_width(),
            );
        } else {
            let checkpoint = self.checkpoints.get(&line).map_or(0, |points| {
                points
                    .partition_point(|&at| at <= relative)
                    .saturating_sub(1)
            });
            let offset = self
                .checkpoints
                .get(&line)
                .map_or(0, |points| points[checkpoint]);
            let mut found = (0, start + offset);
            for (row, range) in self.line_rows(line, offset).enumerate() {
                if range.start > start + relative {
                    break;
                }
                found = (checkpoint * CHECKPOINT_ROWS + row, range.start);
                if range.end > start + relative {
                    break;
                }
            }
            found
        };
        let text = self.source.byte_slice(range_start..start + relative);
        let column = Cow::from(text)
            .graphemes(true)
            .map(source_grapheme_display_width)
            .sum();
        (self.first_row(line) + local, column)
    }
    pub fn offset(&self, row: usize, column: usize) -> usize {
        let row = row.min(self.row_count() - 1);
        let range = self.row(row).unwrap().range;
        let text = self.source.byte_slice(range.start..range.end);
        let text = Cow::from(text);
        let mut cells = 0;
        let mut last = 0;
        for (byte, grapheme) in text.grapheme_indices(true) {
            last = byte;
            let width = source_grapheme_display_width(grapheme);
            if cells + width > column {
                return range.start + byte;
            }
            cells += width;
        }
        if self
            .row(row + 1)
            .is_some_and(|next| next.range.start == range.end)
        {
            range.start + last
        } else {
            range.end
        }
    }
}

// Stream graphemes across Rope chunks, borrowing all clusters within a chunk.
struct Rows<'a> {
    rope: &'a Rope,
    cursor: GraphemeCursor,
    chunk: &'a str,
    chunk_start: usize,
    start: usize,
    grapheme_start: usize,
    end: usize,
    width: usize,
    column: usize,
    done: bool,
}
impl<'a> Rows<'a> {
    fn new(rope: &'a Rope, start: usize, end: usize, width: usize) -> Self {
        let (chunk, chunk_start, _, _) = rope.chunk_at_byte(start);
        Self {
            rope,
            cursor: GraphemeCursor::new(start, rope.len_bytes(), true),
            chunk,
            chunk_start,
            start,
            grapheme_start: start,
            end,
            width,
            column: 0,
            done: false,
        }
    }
    fn next_boundary(&mut self) -> Option<usize> {
        loop {
            match self.cursor.next_boundary(self.chunk, self.chunk_start) {
                Ok(value) => return value,
                Err(GraphemeIncomplete::NextChunk) => {
                    let at = self.cursor.cur_cursor();
                    (self.chunk, self.chunk_start, _, _) = self.rope.chunk_at_byte(at);
                    // At a chunk edge unicode-segmentation requests RI/emoji
                    // pre-context even when a forward cursor already counted it.
                    // Resolve the edge with a fresh cursor to avoid counting a
                    // regional indicator twice (and splitting a flag).
                    self.cursor = GraphemeCursor::new(at, self.rope.len_bytes(), true);
                    if at > self.grapheme_start {
                        loop {
                            match self.cursor.is_boundary(self.chunk, self.chunk_start) {
                                Ok(true) => return Some(at),
                                Ok(false) => break,
                                Err(GraphemeIncomplete::PreContext(end)) => {
                                    let (chunk, start, _, _) = self.rope.chunk_at_byte(end - 1);
                                    self.cursor.provide_context(&chunk[..end - start], start);
                                }
                                Err(_) => return None,
                            }
                        }
                    }
                }
                Err(GraphemeIncomplete::PreContext(end)) => {
                    let (chunk, start, _, _) = self.rope.chunk_at_byte(end - 1);
                    self.cursor.provide_context(&chunk[..end - start], start);
                }
                Err(_) => return None,
            }
        }
    }
}
impl Iterator for Rows<'_> {
    type Item = SourceRange;
    fn next(&mut self) -> Option<Self::Item> {
        if self.done {
            return None;
        }
        while self.grapheme_start < self.end {
            let byte = self.grapheme_start;
            let end = self.next_boundary()?.min(self.end);
            let cells = if byte >= self.chunk_start && end <= self.chunk_start + self.chunk.len() {
                source_grapheme_display_width(
                    &self.chunk[byte - self.chunk_start..end - self.chunk_start],
                )
            } else {
                source_grapheme_display_width(&Cow::from(self.rope.byte_slice(byte..end)))
            };
            self.grapheme_start = end;
            if self.column > 0 && self.column + cells > self.width {
                let row = SourceRange::new(self.start, byte);
                self.start = byte;
                self.column = cells;
                return Some(row);
            }
            self.column += cells;
        }
        self.done = true;
        Some(SourceRange::new(self.start, self.end))
    }
}

#[derive(Debug, Clone, Default)]
struct State {
    layout: Option<Arc<WrapLayout>>,
    generation: u64,
    requested: Option<(usize, u64)>,
    edits: Vec<(Range<usize>, Range<usize>)>,
}
#[derive(Debug, Default)]
pub(super) struct Cache {
    state: Mutex<State>,
    job: Latest<(u64, Option<WrapLayout>)>,
}
impl Clone for Cache {
    fn clone(&self) -> Self {
        let mut state = self.state.lock().unwrap().clone();
        state.requested = None;
        Self {
            state: Mutex::new(state),
            job: Latest::default(),
        }
    }
}
impl PartialEq for Cache {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}
impl Eq for Cache {}
impl Cache {
    pub fn busy(&self) -> bool {
        self.job.busy()
    }
    pub fn poll(&self) -> bool {
        let mut state = self.state.lock().unwrap();
        let ready = self.job.ready();
        self.receive(&mut state);
        ready
    }
    fn receive(&self, state: &mut State) {
        if let Some((generation, Some(layout))) = self.job.take()
            && generation == state.generation
        {
            state.layout = Some(Arc::new(layout));
            state.edits.clear();
        }
    }
    pub fn invalidate(&mut self, old: Range<usize>, new: Range<usize>) {
        let mut state = self.state.lock().unwrap();
        self.receive(&mut state);
        self.job.cancel();
        state.generation += 1;
        state.requested = None;
        if state.layout.is_some() {
            state.edits.push((old, new));
        }
    }
    pub fn get(&self, source: &Rope, width: usize) -> Option<Arc<WrapLayout>> {
        let width = width.max(1);
        let mut state = self.state.lock().unwrap();
        if state.edits.is_empty()
            && let Some(layout) = &state.layout
            && layout.width == width
        {
            return Some(layout.clone());
        }
        if state.requested == Some((width, state.generation)) {
            return None;
        }
        let previous = state.layout.clone();
        let edits = state.edits.clone();
        let generation = state.generation;
        state.requested = Some((width, generation));
        if source.len_bytes() <= SYNC_BYTES {
            let layout = Arc::new(WrapLayout::build(
                source,
                width,
                previous.as_deref(),
                &edits,
                &AtomicBool::new(false),
            )?);
            state.layout = Some(layout.clone());
            state.edits.clear();
            Some(layout)
        } else {
            let source = source.clone();
            self.job.submit(move |cancel| {
                (
                    generation,
                    WrapLayout::build(&source, width, previous.as_deref(), &edits, cancel),
                )
            });
            None
        }
    }
}
