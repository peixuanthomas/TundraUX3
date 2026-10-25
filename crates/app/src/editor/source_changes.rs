/// A source edit described using one-based logical lines before the edit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SourceLineChange {
    pub start_line: usize,
    pub end_line: usize,
    pub inserted_lines: usize,
    pub start_at_line_start: bool,
    pub end_at_line_start: bool,
}

impl SourceLineChange {
    pub(super) fn for_edit(
        source: &ropey::Rope,
        range: std::ops::Range<usize>,
        replacement: &str,
    ) -> Self {
        let context = crlf_context(source, &range, replacement);
        let start = source.byte_to_line(context.start);
        let end = source.byte_to_line(context.end);
        let mut inserted_lines = 0;
        let mut previous_cr = false;
        let prefix = (context.start < range.start).then_some(b'\r');
        let suffix = (context.end > range.end).then_some(b'\n');
        for byte in prefix.into_iter().chain(replacement.bytes()).chain(suffix) {
            if byte == b'\r' || byte == b'\n' && !previous_cr {
                inserted_lines += 1;
            }
            previous_cr = byte == b'\r';
        }
        Self {
            start_line: start + 1,
            end_line: end + 1,
            inserted_lines,
            start_at_line_start: source.line_to_byte(start) == context.start,
            end_at_line_start: source.line_to_byte(end) == context.end,
        }
    }

    pub(super) fn inverse(
        self,
        source: &ropey::Rope,
        range: std::ops::Range<usize>,
        replacement: &str,
    ) -> Self {
        let context = crlf_context(source, &range, replacement);
        Self {
            start_line: self.start_line,
            end_line: self.start_line + self.inserted_lines,
            inserted_lines: self.end_line.saturating_sub(self.start_line),
            start_at_line_start: self.start_at_line_start,
            end_at_line_start: if context.end > range.end {
                true
            } else if replacement.is_empty() {
                context.start < range.start || self.start_at_line_start
            } else {
                replacement.ends_with('\r') || replacement.ends_with('\n')
            },
        }
    }

    pub fn remap_line(self, line: usize) -> Option<usize> {
        if line < self.start_line {
            return Some(line);
        }
        let removed = self.end_line.saturating_sub(self.start_line);
        if removed == self.inserted_lines {
            return Some(line);
        }
        if line == self.start_line {
            if removed > 0 && self.start_at_line_start {
                return None;
            }
            if removed == 0 && self.start_at_line_start {
                return Some(line.saturating_add(self.inserted_lines));
            }
            return Some(line);
        }
        if line < self.end_line || (line == self.end_line && !self.end_at_line_start) {
            return None;
        }
        Some(
            line.saturating_sub(removed)
                .saturating_add(self.inserted_lines),
        )
    }

    /// Translate Vim's listener_add() line edit without comparing buffer snapshots.
    pub fn from_vim(start: usize, end: usize, added: i64, column: usize) -> Self {
        if added < 0 {
            return Self {
                start_line: start,
                end_line: start.saturating_add(added.unsigned_abs() as usize),
                inserted_lines: 0,
                start_at_line_start: true,
                end_at_line_start: true,
            };
        }
        if added > 0 && end <= start.saturating_add(1) {
            return Self {
                start_line: start,
                end_line: start,
                inserted_lines: added as usize,
                start_at_line_start: column <= 1,
                end_at_line_start: true,
            };
        }
        let replaced = end.saturating_sub(start);
        let inserted = replaced.saturating_add_signed(added as isize);
        Self {
            start_line: start,
            end_line: end,
            inserted_lines: inserted,
            start_at_line_start: inserted == 0,
            end_at_line_start: true,
        }
    }
}

fn crlf_context(
    source: &ropey::Rope,
    range: &std::ops::Range<usize>,
    replacement: &str,
) -> std::ops::Range<usize> {
    let before_cr = range.start > 0 && source.byte(range.start - 1) == b'\r';
    let after_lf = source.get_byte(range.end) == Some(b'\n');
    let include_before = before_cr
        && (source.get_byte(range.start) == Some(b'\n')
            || replacement.starts_with('\n')
            || replacement.is_empty() && after_lf);
    let include_after = after_lf
        && (range.end > range.start && source.byte(range.end - 1) == b'\r'
            || replacement.ends_with('\r')
            || replacement.is_empty() && before_cr);
    // Undo may cut a CRLF created by the edit; both old and new pairs need context.
    range.start.saturating_sub(usize::from(include_before))..range.end + usize::from(include_after)
}
