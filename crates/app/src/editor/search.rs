//! UTF-8 byte based search. Only whole extended grapheme clusters are editable.
use super::background::Latest;
use regex::{Regex, RegexBuilder};
use ropey::Rope;
use std::collections::VecDeque;
use std::ops::Range;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use unicode_segmentation::{GraphemeCursor, GraphemeIncomplete};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SearchOptions {
    pub regex: bool,
    pub case_sensitive: bool,
    pub whole_word: bool,
}
impl Default for SearchOptions {
    fn default() -> Self {
        Self {
            regex: false,
            case_sensitive: true,
            whole_word: false,
        }
    }
}

#[derive(Debug, Clone)]
pub struct SearchEngine {
    regex: Regex,
    literal: bool,
    empty: bool,
    literal_bytes: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Replacement {
    pub range: Range<usize>,
    pub original: String,
    pub replacement: String,
}

impl SearchEngine {
    pub fn new(query: &str, options: SearchOptions) -> Result<Self, regex::Error> {
        let pattern = if options.regex {
            query.to_owned()
        } else {
            regex::escape(query)
        };
        let pattern = if options.whole_word {
            format!(r"\b(?:{pattern})\b")
        } else {
            pattern
        };
        let regex = RegexBuilder::new(&pattern)
            .case_insensitive(!options.case_sensitive)
            .multi_line(true)
            .size_limit(4 * 1024 * 1024)
            .build()?;
        Ok(Self {
            regex,
            literal: !options.regex,
            empty: query.is_empty(),
            literal_bytes: query.chars().count().saturating_mul(4),
        })
    }

    pub fn find_all(&self, source: &str) -> Vec<Range<usize>> {
        if self.empty {
            return Vec::new();
        }
        let mut boundaries = Boundaries::new(source.len());
        self.regex
            .find_iter(source)
            .map(|m| m.range())
            .filter(|r| boundaries.string_range(source, r))
            .collect()
    }

    /// Regex replacement supports $0, $1, ${name} and $$; literal mode leaves $ intact.
    pub fn preview(&self, source: &str, replacement: &str) -> Vec<Replacement> {
        if self.empty {
            return Vec::new();
        }
        let mut boundaries = Boundaries::new(source.len());
        self.regex
            .captures_iter(source)
            .filter_map(|captures| {
                let found = captures.get(0)?;
                if !boundaries.string_range(source, &found.range()) {
                    return None;
                }
                let mut expanded = String::new();
                if self.literal {
                    expanded.push_str(replacement);
                } else {
                    captures.expand(replacement, &mut expanded);
                }
                Some(Replacement {
                    range: found.range(),
                    original: found.as_str().to_owned(),
                    replacement: expanded,
                })
            })
            .collect()
    }
}

/// Monotone match endpoints need only one cursor. Nearby endpoints advance it
/// so long runs of regional indicators do not repeatedly rescan their prefix.
/// Sparse matches can jump directly, without enumerating intervening graphemes.
pub(super) struct Boundaries {
    cursor: GraphemeCursor,
}
impl Boundaries {
    pub fn new(len: usize) -> Self {
        Self {
            cursor: GraphemeCursor::new(0, len, true),
        }
    }
    fn contains(
        &mut self,
        at: usize,
        len: usize,
        ascii_boundary: bool,
        mut boundary: impl FnMut(&mut GraphemeCursor) -> bool,
        mut next: impl FnMut(&mut GraphemeCursor) -> Option<usize>,
    ) -> bool {
        if at < self.cursor.cur_cursor() {
            return false;
        }
        if at == self.cursor.cur_cursor() {
            return true;
        }
        if ascii_boundary || at == len {
            self.cursor = GraphemeCursor::new(at, len, true);
            return true;
        }
        if at - self.cursor.cur_cursor() > 64 {
            self.cursor = GraphemeCursor::new(at, len, true);
            if boundary(&mut self.cursor) {
                return true;
            }
            next(&mut self.cursor);
            return false;
        }
        while self.cursor.cur_cursor() < at {
            if next(&mut self.cursor).is_none() {
                return false;
            }
        }
        self.cursor.cur_cursor() == at
    }
    fn string_range(&mut self, source: &str, range: &Range<usize>) -> bool {
        range.start <= range.end
            && [range.start, range.end].into_iter().all(|at| {
                if !source.is_char_boundary(at) {
                    return false;
                }
                let ascii = at > 0
                    && at < source.len()
                    && ascii_boundary(source.as_bytes()[at - 1], source.as_bytes()[at]);
                self.contains(
                    at,
                    source.len(),
                    ascii,
                    |cursor| cursor.is_boundary(source, 0).unwrap_or(false),
                    |cursor| cursor.next_boundary(source, 0).ok().flatten(),
                )
            })
    }
    pub fn rope_range(&mut self, source: &Rope, range: &Range<usize>) -> bool {
        range.start <= range.end
            && range.end <= source.len_bytes()
            && [range.start, range.end].into_iter().all(|at| {
                let (chunk, start, _, _) = source.chunk_at_byte(at);
                if !chunk.is_char_boundary(at - start) {
                    return false;
                }
                let ascii = at > 0
                    && at < source.len_bytes()
                    && ascii_boundary(source.byte(at - 1), source.byte(at));
                self.contains(
                    at,
                    source.len_bytes(),
                    ascii,
                    |cursor| rope_boundary(source, cursor),
                    |cursor| super::next_rope_grapheme_boundary(source, cursor),
                )
            })
    }
}
fn ascii_boundary(before: u8, after: u8) -> bool {
    before.is_ascii() && after.is_ascii() && !(before == b'\r' && after == b'\n')
}
fn rope_boundary(source: &Rope, cursor: &mut GraphemeCursor) -> bool {
    let (chunk, start, _, _) = source.chunk_at_byte(cursor.cur_cursor());
    loop {
        match cursor.is_boundary(chunk, start) {
            Ok(boundary) => return boundary,
            Err(GraphemeIncomplete::PreContext(end)) => {
                let (context, start, _, _) = source.chunk_at_byte(end - 1);
                cursor.provide_context(&context[..end - start], start);
            }
            Err(_) => return false,
        }
    }
}

const SCAN_BYTES: usize = 64 * 1024;
const PAGE_MATCHES: usize = 512;

impl SearchEngine {
    /// Literal queries (including Unicode case folding and whole words) scan
    /// bounded overlapping windows. Arbitrary regexes keep their full-haystack
    /// semantics; only that fallback flattens the Rope, in the worker.
    fn scan_rope(
        &self,
        source: &Rope,
        cancel: &AtomicBool,
        mut visit: impl FnMut(&regex::Captures<'_>, usize) -> bool,
    ) {
        if self.empty {
            return;
        }
        let mut boundaries = Boundaries::new(source.len_bytes());
        if !self.literal {
            let mut text = String::with_capacity(source.len_bytes());
            for chunk in source.chunks() {
                if cancel.load(Ordering::Relaxed) {
                    return;
                }
                text.push_str(chunk);
            }
            for found in self.regex.captures_iter(&text) {
                if cancel.load(Ordering::Relaxed) {
                    return;
                }
                if boundaries.rope_range(source, &found.get(0).unwrap().range())
                    && !visit(&found, 0)
                {
                    return;
                }
            }
            return;
        }
        let len = source.len_bytes();
        let mut next = 0;
        while next < len {
            if cancel.load(Ordering::Relaxed) {
                return;
            }
            // Include one complete scalar on either side for Unicode word boundaries.
            let start = source.char_to_byte(source.byte_to_char(next).saturating_sub(1));
            let limit =
                source.char_to_byte(source.byte_to_char(next.saturating_add(SCAN_BYTES).min(len)));
            let end = source.char_to_byte(
                source.byte_to_char(
                    limit
                        .saturating_add(self.literal_bytes)
                        .saturating_add(4)
                        .min(len),
                ),
            );
            let text = source.byte_slice(start..end).to_string();
            while let Some(found) = self.regex.captures_at(&text, next - start) {
                if cancel.load(Ordering::Relaxed) {
                    return;
                }
                let matched = found.get(0).unwrap();
                let range = start + matched.start()..start + matched.end();
                if range.start >= limit {
                    break;
                }
                next = range.end;
                if boundaries.rope_range(source, &range) && !visit(&found, start) {
                    return;
                }
            }
            next = next.max(limit);
        }
    }

    pub fn find_all_rope(&self, source: &Rope) -> Vec<Range<usize>> {
        let mut matches = Vec::new();
        self.scan_rope(source, &AtomicBool::new(false), |captures, base| {
            let found = captures.get(0).unwrap();
            matches.push(base + found.start()..base + found.end());
            true
        });
        matches
    }

    pub fn preview_rope(
        &self,
        source: &Rope,
        replacement: &str,
    ) -> Result<Vec<Replacement>, PreviewLimitExceeded> {
        self.preview_for(source, replacement, None)
    }
    pub fn preview_for(
        &self,
        source: &Rope,
        replacement: &str,
        target: Option<PreviewTarget>,
    ) -> Result<Vec<Replacement>, PreviewLimitExceeded> {
        self.preview_cancelled(source, replacement, target, &AtomicBool::new(false))
    }
    fn preview_cancelled(
        &self,
        source: &Rope,
        replacement: &str,
        target: Option<PreviewTarget>,
        cancel: &AtomicBool,
    ) -> Result<Vec<Replacement>, PreviewLimitExceeded> {
        let wanted = if let Some(target) = target {
            let selected = target.selection.and_then(|selection| {
                let page = self.page(source, Target::Forward(selection.start), cancel);
                page.current
                    .and_then(|index| (page.matches[index] == selection).then_some(selection))
            });
            let range = selected.or_else(|| {
                let page = self.page(
                    source,
                    if target.backwards {
                        Target::Backward(target.cursor)
                    } else {
                        Target::Forward(target.cursor)
                    },
                    cancel,
                );
                page.current.map(|index| page.matches[index].clone())
            });
            let Some(range) = range else {
                return Ok(Vec::new());
            };
            Some(range)
        } else {
            None
        };
        let mut edits = Vec::new();
        let mut bytes = 0usize;
        let mut exceeded = false;
        self.scan_rope(source, cancel, |captures, base| {
            let found = captures.get(0).unwrap();
            let range = base + found.start()..base + found.end();
            if wanted.as_ref().is_some_and(|wanted| *wanted != range) {
                return true;
            }
            if found.len() > 8 * 1024 * 1024 || replacement.len() > 8 * 1024 * 1024 {
                exceeded = true;
                return false;
            }
            let mut expanded = String::new();
            if self.literal {
                expanded.push_str(replacement);
            } else {
                captures.expand(replacement, &mut expanded);
            }
            bytes = bytes
                .saturating_add(found.len())
                .saturating_add(expanded.len())
                .saturating_add(std::mem::size_of::<Replacement>());
            if edits.len() >= 10_000 || bytes > 8 * 1024 * 1024 {
                exceeded = true;
                return false;
            }
            edits.push(Replacement {
                range,
                original: found.as_str().into(),
                replacement: expanded,
            });
            wanted.is_none()
        });
        if exceeded {
            Err(PreviewLimitExceeded)
        } else {
            Ok(edits)
        }
    }

    pub(super) fn page(&self, source: &Rope, target: Target, cancel: &AtomicBool) -> Page {
        let mut first = Vec::new();
        let mut selected = VecDeque::new();
        let mut tail = VecDeque::new();
        let mut current = None;
        let mut total = 0;
        self.scan_rope(source, cancel, |captures, base| {
            let found = captures.get(0).unwrap();
            let range = base + found.start()..base + found.end();
            if first.len() < PAGE_MATCHES {
                first.push(range.clone());
            }
            tail.push_back((total, range.clone()));
            if tail.len() > PAGE_MATCHES {
                tail.pop_front();
            }
            match target {
                Target::Index(index) => {
                    let start = index / PAGE_MATCHES * PAGE_MATCHES;
                    if total >= start && total < start + PAGE_MATCHES {
                        selected.push_back((total, range));
                    }
                    current = Some(index);
                }
                Target::Forward(byte) => {
                    if current.is_none() && range.start >= byte {
                        current = Some(total);
                    }
                    if current.is_some() && selected.len() < PAGE_MATCHES {
                        selected.push_back((total, range));
                    }
                }
                Target::Backward(byte) => {
                    if range.start < byte {
                        current = Some(total);
                        selected.push_back((total, range));
                        if selected.len() > PAGE_MATCHES {
                            selected.pop_front();
                        }
                    }
                }
            }
            total += 1;
            true
        });
        if total == 0 {
            return Page {
                matches: Arc::from([]),
                first: 0,
                current: None,
                total,
            };
        }
        if selected.is_empty() {
            if matches!(target, Target::Backward(_)) {
                selected = tail;
                current = Some(total - 1);
            } else {
                selected = first.iter().cloned().enumerate().collect();
                current = Some(0);
            }
        }
        if total <= PAGE_MATCHES {
            return Page {
                matches: first.into(),
                first: 0,
                current,
                total,
            };
        }
        let first = selected.front().unwrap().0;
        Page {
            matches: selected.into_iter().map(|(_, range)| range).collect(),
            first,
            current: current.map(|index| index - first),
            total,
        }
    }
}
#[derive(Clone, Copy)]
pub(super) enum Target {
    Index(usize),
    Forward(usize),
    Backward(usize),
}
pub(super) struct Page {
    pub matches: Arc<[Range<usize>]>,
    pub first: usize,
    pub current: Option<usize>,
    pub total: usize,
}
pub(super) struct Completion {
    pub revision: u64,
    pub cursor: usize,
    pub query: String,
    pub options: SearchOptions,
    pub page: Page,
}

/// A preview worker used by the shell. There is one active and one latest request.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct PreviewTask(Latest<Result<Vec<Replacement>, PreviewLimitExceeded>>);
impl PreviewTask {
    pub fn start(
        &self,
        engine: SearchEngine,
        source: Rope,
        replacement: String,
        target: Option<PreviewTarget>,
    ) {
        self.0
            .submit(move |cancel| engine.preview_cancelled(&source, &replacement, target, cancel));
    }
    pub fn take(&self) -> Option<Result<Vec<Replacement>, PreviewLimitExceeded>> {
        self.0.take()
    }
    pub fn cancel(&self) {
        self.0.cancel();
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PreviewLimitExceeded;

#[derive(Debug, Clone)]
pub struct PreviewTarget {
    pub selection: Option<Range<usize>>,
    pub cursor: usize,
    pub backwards: bool,
}
