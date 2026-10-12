//! UTF-8 byte based search. Only whole extended grapheme clusters are editable.
use regex::{Regex, RegexBuilder};
use std::ops::Range;
use unicode_segmentation::UnicodeSegmentation;

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
        })
    }

    pub fn find_all(&self, source: &str) -> Vec<Range<usize>> {
        if self.empty {
            return Vec::new();
        }
        let boundaries = grapheme_boundaries(source);
        self.regex
            .find_iter(source)
            .map(|m| m.range())
            .filter(|r| valid_range(&boundaries, r))
            .collect()
    }

    /// Regex replacement supports $0, $1, ${name} and $$; literal mode leaves $ intact.
    pub fn preview(&self, source: &str, replacement: &str) -> Vec<Replacement> {
        if self.empty {
            return Vec::new();
        }
        let boundaries = grapheme_boundaries(source);
        self.regex
            .captures_iter(source)
            .filter_map(|captures| {
                let found = captures.get(0)?;
                if !valid_range(&boundaries, &found.range()) {
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

pub(super) fn grapheme_boundaries(source: &str) -> Vec<usize> {
    source
        .grapheme_indices(true)
        .map(|(i, _)| i)
        .chain(std::iter::once(source.len()))
        .collect()
}
pub(super) fn valid_range(boundaries: &[usize], range: &Range<usize>) -> bool {
    range.start <= range.end
        && boundaries.binary_search(&range.start).is_ok()
        && boundaries.binary_search(&range.end).is_ok()
}
