use super::*;

impl SourceBuffer {
    pub fn from_text(text: impl AsRef<str>) -> Self {
        let text = text.as_ref();
        let mut buffer = Self {
            text: Rope::from_str(text),
            word_count: text.unicode_words().count(),
            line_endings: LineEndingStats::from_text(text),
            non_ascii_bytes: non_ascii_byte_count(text),
            display_widths: SourceDisplayWidthCache::default(),
            c_highlights: c_syntax::Cache::default(),
        };
        buffer.rebuild_display_width_cache();
        buffer
    }

    pub(super) fn from_rope(text: &Rope, word_count: usize, line_endings: LineEndingStats) -> Self {
        let mut buffer = Self {
            text: text.clone(),
            word_count,
            line_endings,
            non_ascii_bytes: text.chunks().map(non_ascii_byte_count).sum(),
            display_widths: SourceDisplayWidthCache::default(),
            c_highlights: c_syntax::Cache::default(),
        };
        buffer.rebuild_display_width_cache();
        buffer
    }

    pub fn len_bytes(&self) -> usize {
        self.text.len_bytes()
    }

    pub fn is_empty(&self) -> bool {
        self.text.len_bytes() == 0
    }

    pub fn line_count(&self) -> usize {
        self.text.len_lines()
    }

    pub fn word_count(&self) -> usize {
        self.word_count
    }

    pub fn max_display_width(&self) -> usize {
        self.display_widths.max_width()
    }

    pub(super) fn line_display_widths(&self, line_range: Range<usize>) -> Vec<usize> {
        let start = line_range.start.min(self.line_count());
        let end = line_range.end.min(self.line_count()).max(start);
        self.text
            .lines_at(start)
            .take(end.saturating_sub(start))
            .map(|line| rope_slice_display_width(source_line_content(line)))
            .collect()
    }

    pub(super) fn rebuild_display_width_cache(&mut self) {
        let mut display_widths = SourceDisplayWidthCache::default();
        // `Rope::lines` advances sequentially through the tree. In
        // particular, an all-ASCII log uses only cached RopeSlice lengths and
        // never performs one `line_to_byte` lookup per short line.
        for line in self.text.lines() {
            display_widths.insert(rope_slice_display_width(source_line_content(line)));
        }
        self.display_widths = display_widths;
    }

    pub(super) fn metadata(&self, utf8_bom: bool, fallback: LineEnding) -> TextMetadata {
        let distinct = usize::from(self.line_endings.lf > 0)
            + usize::from(self.line_endings.crlf > 0)
            + usize::from(self.line_endings.cr > 0);
        let preferred_line_ending = [
            (self.line_endings.lf, LineEnding::Lf),
            (self.line_endings.crlf, LineEnding::CrLf),
            (self.line_endings.cr, LineEnding::Cr),
        ]
        .into_iter()
        .max_by_key(|(count, _)| *count)
        .filter(|(count, _)| *count > 0)
        .map(|(_, ending)| ending)
        .unwrap_or(fallback);
        TextMetadata {
            utf8_bom,
            preferred_line_ending,
            mixed_line_endings: distinct > 1,
            has_final_newline: self
                .text
                .get_byte(self.len_bytes().saturating_sub(1))
                .is_some_and(|byte| matches!(byte, b'\r' | b'\n')),
        }
    }

    /// Returns the complete source, borrowing only when Ropey stores it in a
    /// single contiguous leaf. Callers rendering a viewport should use
    /// [`Self::lines`] instead to avoid flattening large documents.
    pub fn text(&self) -> Cow<'_, str> {
        Cow::from(&self.text)
    }

    pub fn byte_slice(&self, range: SourceRange) -> Option<String> {
        self.validated_range(range.start..range.end)
            .map(|range| self.text.byte_slice(range).to_string())
    }

    /// UTF-8 byte range of a line's visible content, excluding LF, CRLF, or
    /// CR. The final empty line after a trailing newline is retained.
    pub fn line_range(&self, line_index: usize) -> Option<SourceRange> {
        if line_index >= self.line_count() {
            return None;
        }
        let start = self.text.line_to_byte(line_index);
        let mut end = if line_index + 1 < self.line_count() {
            self.text.line_to_byte(line_index + 1)
        } else {
            self.len_bytes()
        };
        if end > start && self.text.byte(end - 1) == b'\n' {
            end -= 1;
            if end > start && self.text.byte(end - 1) == b'\r' {
                end -= 1;
            }
        } else if end > start && self.text.byte(end - 1) == b'\r' {
            end -= 1;
        }
        Some(SourceRange::new(start, end))
    }

    /// Materializes only the requested half-open line range. Out-of-bounds
    /// ends are clamped, making it convenient for viewport + overscan reads.
    pub fn lines(&self, line_range: Range<usize>) -> Vec<SourceLine> {
        let start = line_range.start.min(self.line_count());
        let end = line_range.end.min(self.line_count()).max(start);
        (start..end)
            .filter_map(|line_index| {
                let byte_range = self.line_range(line_index)?;
                let text = self
                    .text
                    .byte_slice(byte_range.start..byte_range.end)
                    .to_string();
                Some(SourceLine {
                    line_index,
                    byte_range,
                    text,
                })
            })
            .collect()
    }

    /// Materializes a bounded vertical and horizontal window. Grapheme
    /// boundaries are resolved directly against Ropey chunks, so a multi-MiB
    /// single line is not flattened merely to display its first screenful.
    pub fn viewport_lines(
        &self,
        line_range: Range<usize>,
        left_column: usize,
        width: usize,
    ) -> Vec<SourceViewportLine> {
        let start = line_range.start.min(self.line_count());
        let end = line_range.end.min(self.line_count()).max(start);
        (start..end)
            .filter_map(|line_index| self.viewport_line(line_index, left_column, width))
            .collect()
    }

    pub(super) fn viewport_line(
        &self,
        line_index: usize,
        left_column: usize,
        width: usize,
    ) -> Option<SourceViewportLine> {
        let line = self.line_range(line_index)?;
        if self.non_ascii_bytes == 0 && width > 0 {
            return Some(self.ascii_viewport_line(line_index, line, left_column, width));
        }
        let right_column = left_column.saturating_add(width);
        let mut cursor = GraphemeCursor::new(line.start, self.len_bytes(), true);
        let mut byte = line.start;
        let mut column = 0usize;
        let mut visible_start = None;
        let mut visible_end = line.start;
        let mut visible_start_column = left_column;
        let mut visible_end_column = left_column;

        while byte < line.end && width > 0 {
            let Some(next) = next_rope_grapheme_boundary(&self.text, &mut cursor) else {
                break;
            };
            let next = next.min(line.end);
            if next <= byte {
                break;
            }
            let grapheme = self.text.byte_slice(byte..next).to_string();
            let grapheme_width = source_grapheme_display_width(&grapheme);
            let next_column = column.saturating_add(grapheme_width);
            let intersects = (next_column > left_column
                || grapheme_width == 0 && column >= left_column)
                && column < right_column;
            if intersects {
                if visible_start.is_none() {
                    visible_start = Some(byte);
                    visible_start_column = column;
                }
                visible_end = next;
                visible_end_column = next_column;
            } else if visible_start.is_some() && column >= right_column {
                break;
            }
            byte = next;
            column = next_column;
        }

        let visible_start = visible_start.unwrap_or_else(|| {
            // No grapheme intersects this viewport (for example, a short line
            // scrolled entirely off the left edge). Both ends must follow the
            // scan position; keeping visible_end at line.start inverts the range.
            visible_end = line.end.min(byte);
            visible_start_column = column;
            visible_end_column = column;
            visible_end
        });
        let visible_byte_range = SourceRange::new(visible_start, visible_end);
        let text = self
            .text
            .byte_slice(visible_byte_range.start..visible_byte_range.end)
            .to_string();
        Some(SourceViewportLine {
            line_index,
            line_byte_range: line,
            visible_byte_range,
            start_column: visible_start_column,
            end_column: visible_end_column,
            truncated_left: visible_start > line.start,
            truncated_right: visible_end < line.end,
            text,
        })
    }

    pub(super) fn ascii_viewport_line(
        &self,
        line_index: usize,
        line: SourceRange,
        left_column: usize,
        width: usize,
    ) -> SourceViewportLine {
        // Every ASCII byte is one extended grapheme and is rendered as one
        // terminal cell, including the control-picture substitution used for
        // otherwise unsafe bytes. Line terminators are excluded by `line`.
        let line_len = line.end.saturating_sub(line.start);
        let start_column = left_column.min(line_len);
        let end_column = left_column.saturating_add(width).min(line_len);
        let visible_byte_range = SourceRange::new(
            line.start.saturating_add(start_column),
            line.start.saturating_add(end_column),
        );
        let text = self
            .text
            .byte_slice(visible_byte_range.start..visible_byte_range.end)
            .to_string();
        SourceViewportLine {
            line_index,
            line_byte_range: line,
            visible_byte_range,
            start_column,
            end_column,
            truncated_left: start_column > 0,
            truncated_right: end_column < line_len,
            text,
        }
    }

    pub(super) fn validated_range(&self, range: Range<usize>) -> Option<Range<usize>> {
        (range.start <= range.end
            && range.end <= self.len_bytes()
            && self.is_cursor_boundary(range.start)
            && self.is_cursor_boundary(range.end))
        .then_some(range)
    }

    pub(super) fn is_cursor_boundary(&self, position: usize) -> bool {
        position <= self.len_bytes()
            && self.text.try_byte_to_char(position).is_ok()
            && !(position > 0
                && position < self.len_bytes()
                && self.text.byte(position - 1) == b'\r'
                && self.text.byte(position) == b'\n')
    }

    pub(super) fn normalize_position(&self, position: usize) -> usize {
        let mut position = position.min(self.len_bytes());
        while self.text.try_byte_to_char(position).is_err() {
            position = position.saturating_sub(1);
        }
        if position > 0
            && position < self.len_bytes()
            && self.text.byte(position - 1) == b'\r'
            && self.text.byte(position) == b'\n'
        {
            position -= 1;
        }
        position
    }

    pub(super) fn replace_range(
        &mut self,
        range: Range<usize>,
        replacement: &str,
    ) -> Option<String> {
        let range = self.validated_range(range)?;
        self.replace_validated_range(range, replacement)
    }

    /// History may need to undo an edit that created a CRLF pair. One edge of
    /// the inverse range can therefore sit between CR and LF even though a UI
    /// cursor is never allowed there.
    pub(super) fn replace_history_range(
        &mut self,
        range: Range<usize>,
        replacement: &str,
    ) -> Option<String> {
        if range.start > range.end
            || range.end > self.len_bytes()
            || self.text.try_byte_to_char(range.start).is_err()
            || self.text.try_byte_to_char(range.end).is_err()
        {
            return None;
        }
        self.replace_validated_range(range, replacement)
    }

    pub(super) fn replace_validated_range(
        &mut self,
        range: Range<usize>,
        replacement: &str,
    ) -> Option<String> {
        let start_char = self.text.byte_to_char(range.start);
        let end_char = self.text.byte_to_char(range.end);
        let context_start = self.text.char_to_byte(start_char.saturating_sub(1));
        let context_end = self
            .text
            .char_to_byte((end_char + 1).min(self.text.len_chars()));
        let suffix_context_len = context_end.saturating_sub(range.end);
        let old_line_endings = LineEndingStats::from_text(
            &self.text.byte_slice(context_start..context_end).to_string(),
        );
        let old_start_line = self.text.byte_to_line(range.start);
        let old_end_line = self.text.byte_to_line(range.end);
        // Keep stable byte anchors around the edit. History ranges are
        // allowed to start between CR and LF, where `byte_to_line` can change
        // after the edit even though the neighboring content lines do not.
        let old_display_start_line = old_start_line.saturating_sub(1);
        let old_display_end_line = old_end_line.saturating_add(2).min(self.line_count());
        let display_window_start_byte = self.text.line_to_byte(old_display_start_line);
        let display_window_end_byte = if old_display_end_line < self.line_count() {
            self.text.line_to_byte(old_display_end_line)
        } else {
            self.len_bytes()
        };
        let old_line_widths =
            self.line_display_widths(old_display_start_line..old_display_end_line);
        let old_window_start = self.line_range(old_start_line.saturating_sub(1))?.start;
        let old_window_end = self
            .line_range((old_end_line + 1).min(self.line_count().saturating_sub(1)))?
            .end;
        let word_suffix_len = old_window_end.saturating_sub(range.end);
        let old_word_count = self
            .text
            .byte_slice(old_window_start..old_window_end)
            .to_string()
            .unicode_words()
            .count();
        let removed = self.text.byte_slice(range.clone()).to_string();
        self.text.remove(start_char..end_char);
        self.text.insert(start_char, replacement);
        self.c_highlights.0.take();
        let new_end = range.start + replacement.len();
        let new_context_end = new_end
            .saturating_add(suffix_context_len)
            .min(self.len_bytes());
        let new_line_endings = LineEndingStats::from_text(
            &self
                .text
                .byte_slice(context_start..new_context_end)
                .to_string(),
        );
        self.line_endings
            .replace_window(old_line_endings, new_line_endings);
        let new_window_start = old_window_start;
        let new_window_end = new_end
            .saturating_add(word_suffix_len)
            .min(self.len_bytes());
        let new_word_count = self
            .text
            .byte_slice(new_window_start..new_window_end)
            .to_string()
            .unicode_words()
            .count();
        self.word_count = self
            .word_count
            .saturating_sub(old_word_count)
            .saturating_add(new_word_count);
        self.non_ascii_bytes = self
            .non_ascii_bytes
            .saturating_sub(non_ascii_byte_count(&removed))
            .saturating_add(non_ascii_byte_count(replacement));
        let new_display_start_line = self
            .text
            .byte_to_line(display_window_start_byte.min(self.len_bytes()));
        let new_display_end_byte = display_window_end_byte
            .saturating_sub(range.end.saturating_sub(range.start))
            .saturating_add(replacement.len())
            .min(self.len_bytes());
        let new_display_end_line = if new_display_end_byte == self.len_bytes() {
            self.line_count()
        } else {
            self.text.byte_to_line(new_display_end_byte)
        };
        let new_line_widths =
            self.line_display_widths(new_display_start_line..new_display_end_line);
        for width in old_line_widths {
            self.display_widths.remove(width);
        }
        for width in new_line_widths {
            self.display_widths.insert(width);
        }
        Some(removed)
    }

    pub(super) fn previous_grapheme_boundary(&self, byte_offset: usize) -> usize {
        let offset = self.normalize_position(byte_offset);
        if offset == 0 {
            return 0;
        }
        let mut cursor = GraphemeCursor::new(offset, self.len_bytes(), true);
        previous_rope_grapheme_boundary(&self.text, &mut cursor).unwrap_or(offset)
    }

    pub(super) fn next_grapheme_boundary(&self, byte_offset: usize) -> usize {
        let offset = self.normalize_position(byte_offset);
        if offset >= self.len_bytes() {
            return self.len_bytes();
        }
        let mut cursor = GraphemeCursor::new(offset, self.len_bytes(), true);
        next_rope_grapheme_boundary(&self.text, &mut cursor).unwrap_or(offset)
    }

    pub(super) fn previous_word_boundary(&self, byte_offset: usize) -> usize {
        let offset = self.normalize_position(byte_offset);
        let mut line_index = self.text.byte_to_line(offset);
        loop {
            let Some(line) = self.line_range(line_index) else {
                return 0;
            };
            let text = self.text.byte_slice(line.start..line.end).to_string();
            let local_offset = if line_index == self.text.byte_to_line(offset) {
                offset.saturating_sub(line.start).min(text.len())
            } else {
                text.len()
            };
            let mut previous = None;
            for (start, word) in text.unicode_word_indices() {
                let end = start + word.len();
                if start >= local_offset {
                    break;
                }
                if local_offset <= end {
                    return line.start + start;
                }
                previous = Some(start);
            }
            if let Some(previous) = previous {
                return line.start + previous;
            }
            let Some(previous_line) = line_index.checked_sub(1) else {
                return 0;
            };
            line_index = previous_line;
        }
    }

    pub(super) fn next_word_boundary(&self, byte_offset: usize) -> usize {
        let offset = self.normalize_position(byte_offset);
        let initial_line = self.text.byte_to_line(offset);
        for line_index in initial_line..self.line_count() {
            let Some(line) = self.line_range(line_index) else {
                break;
            };
            let text = self.text.byte_slice(line.start..line.end).to_string();
            let local_offset = if line_index == initial_line {
                offset.saturating_sub(line.start).min(text.len())
            } else {
                0
            };
            for (start, word) in text.unicode_word_indices() {
                let end = start + word.len();
                if local_offset < start {
                    return line.start + start;
                }
                if local_offset < end {
                    return line.start + end;
                }
            }
        }
        self.len_bytes()
    }

    pub(super) fn byte_at_grapheme_column(
        &self,
        line_index: usize,
        column: usize,
    ) -> Option<usize> {
        let line = self.line_range(line_index)?;
        let mut byte = line.start;
        let mut cursor = GraphemeCursor::new(byte, self.len_bytes(), true);
        for _ in 0..column {
            let Some(next) = next_rope_grapheme_boundary(&self.text, &mut cursor) else {
                return Some(byte.min(line.end));
            };
            if next > line.end || next <= byte {
                return Some(line.end);
            }
            byte = next;
        }
        Some(byte.min(line.end))
    }

    pub(super) fn line_grapheme_column(&self, line: SourceRange, byte_offset: usize) -> usize {
        let end = byte_offset.min(line.end);
        if end <= line.start {
            return 0;
        }
        let mut byte = line.start;
        let mut column = 0usize;
        let mut cursor = GraphemeCursor::new(byte, self.len_bytes(), true);
        while byte < end {
            let Some(next) = next_rope_grapheme_boundary(&self.text, &mut cursor) else {
                break;
            };
            if next <= byte {
                break;
            }
            column = column.saturating_add(1);
            if next >= end || next > line.end {
                break;
            }
            byte = next;
        }
        column
    }

    pub(super) fn line_display_column(&self, line: SourceRange, byte_offset: usize) -> usize {
        let end = byte_offset.min(line.end);
        if end <= line.start {
            return 0;
        }
        rope_slice_display_width(self.text.byte_slice(line.start..end))
    }
}
