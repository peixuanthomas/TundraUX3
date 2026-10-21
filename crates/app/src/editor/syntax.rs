//! Viewport LSH tokens with sparse parser checkpoints. Parsing never holds the cache lock.
use std::borrow::Cow;
use std::collections::BTreeMap;
use std::ops::Range;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

use super::{
    SourceRange,
    background::{Latest, SYNC_BYTES},
};
use ropey::Rope;

mod definitions {
    include!(concat!(env!("OUT_DIR"), "/lsh_definitions.rs"));
}
pub use definitions::HighlightKind;

const CHECKPOINT_LINES: usize = 128;
const MAX_CHECKPOINTS: usize = 2048;
// LSH accepts contiguous logical lines. Do not flatten pathological log lines.
// Once such a line is reached, leave the remaining text uncoloured: its parser
// state is unknown, and restarting at the viewport would miscolour comments.
const MAX_LINE_BYTES: usize = 256 * 1024;
type Checkpoints = BTreeMap<usize, lsh::runtime::RuntimeState>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyntaxToken {
    pub range: SourceRange,
    pub kind: HighlightKind,
}

#[derive(Clone, Default)]
struct State {
    path: String,
    generation: u64,
    checkpoints: Arc<Checkpoints>,
    requested: Option<Range<usize>>,
    tokens: Arc<[SyntaxToken]>,
}
struct Computed {
    generation: u64,
    lines: Range<usize>,
    tokens: Arc<[SyntaxToken]>,
    checkpoints: Checkpoints,
}
#[derive(Default)]
pub(super) struct Cache {
    state: Mutex<State>,
    job: Latest<Computed>,
}
impl std::fmt::Debug for Cache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SyntaxCache(..)")
    }
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
        if let Some(result) = self.job.take()
            && result.generation == state.generation
            && state.requested.as_ref() == Some(&result.lines)
        {
            state.tokens = result.tokens;
            state.checkpoints = Arc::new(result.checkpoints);
            true
        } else {
            false
        }
    }
    pub fn invalidate(&mut self, line: usize) {
        self.job.cancel();
        let state = self.state.get_mut().unwrap();
        state.generation += 1;
        Arc::make_mut(&mut state.checkpoints).retain(|&at, _| at <= line);
        state.requested = None;
        state.tokens = Arc::from([]);
    }
    pub fn get(&self, path: &str, rope: &Rope, lines: Range<usize>) -> Arc<[SyntaxToken]> {
        let mut state = self.state.lock().unwrap();
        if state.path != path {
            self.job.cancel();
            *state = State {
                path: path.to_owned(),
                generation: state.generation + 1,
                ..State::default()
            };
        }
        if let Some(result) = self.job.take()
            && result.generation == state.generation
            && state.requested.as_ref() == Some(&result.lines)
        {
            state.tokens = result.tokens;
            state.checkpoints = Arc::new(result.checkpoints);
        }
        let lines = lines.start.min(rope.len_lines())..lines.end.min(rope.len_lines());
        if state.requested.as_ref() == Some(&lines) {
            return state.tokens.clone();
        }
        state.requested = Some(lines.clone());
        state.tokens = Arc::from([]);
        let checkpoints = (*state.checkpoints).clone();
        let generation = state.generation;
        if rope.len_bytes() <= SYNC_BYTES {
            let (tokens, checkpoints) =
                compute(path, rope, lines, checkpoints, &AtomicBool::new(false));
            state.tokens = tokens;
            state.checkpoints = Arc::new(checkpoints);
        } else {
            let rope = rope.clone();
            let path = path.to_owned();
            self.job.submit(move |cancel| {
                let (tokens, checkpoints) =
                    compute(&path, &rope, lines.clone(), checkpoints, cancel);
                Computed {
                    generation,
                    lines,
                    tokens,
                    checkpoints,
                }
            });
        }
        state.tokens.clone()
    }
}

pub fn highlight_text(path: &str, source: &str) -> Arc<[SyntaxToken]> {
    highlight(path, &Rope::from_str(source))
}
/// Synchronous utility for exports/tests. Interactive callers use the viewport cache.
pub fn highlight(path: &str, rope: &Rope) -> Arc<[SyntaxToken]> {
    compute(
        path,
        rope,
        0..rope.len_lines(),
        Checkpoints::new(),
        &AtomicBool::new(false),
    )
    .0
}

fn compute(
    path: &str,
    rope: &Rope,
    lines: Range<usize>,
    mut checkpoints: Checkpoints,
    cancel: &AtomicBool,
) -> (Arc<[SyntaxToken]>, Checkpoints) {
    let path = format!("/{}", path.replace('\\', "/"));
    let Some((_, language)) = definitions::FILE_ASSOCIATIONS
        .iter()
        .find(|(pattern, _)| lsh::glob::glob_match(pattern.as_bytes(), path.as_bytes()))
    else {
        return (Arc::from([]), checkpoints);
    };
    let mut runtime = lsh::runtime::Runtime::new(
        &definitions::ASSEMBLY,
        &definitions::STRINGS,
        &definitions::CHARSETS,
        language.entrypoint,
    );
    let start = if let Some((&line, snapshot)) = checkpoints.range(..=lines.start).next_back() {
        runtime.restore(snapshot);
        line
    } else {
        0
    };
    let mut tokens = Vec::new();
    let mut spans = Vec::<lsh::runtime::Highlight<HighlightKind>>::new();
    let mut offset = rope.line_to_byte(start);
    for (index, line) in rope
        .lines_at(start)
        .take(lines.end.saturating_sub(start))
        .enumerate()
    {
        if cancel.load(Ordering::Relaxed) || line.len_bytes() > MAX_LINE_BYTES {
            break;
        }
        let index = start + index;
        if index % CHECKPOINT_LINES == 0 {
            if checkpoints.len() >= MAX_CHECKPOINTS {
                checkpoints.pop_first();
            }
            checkpoints.insert(index, runtime.snapshot());
        }
        let text = Cow::from(line);
        let content = text.trim_end_matches(['\r', '\n']);
        spans.clear();
        // Outside the viewport only the parser state is needed.
        if index < lines.start {
            runtime.parse_next_line::<HighlightKind, _>(content.as_bytes(), Discard);
        } else {
            runtime.parse_next_line(content.as_bytes(), &mut spans);
            for pair in spans.windows(2) {
                let start = pair[0].start.min(content.len());
                let end = pair[1].start.min(content.len());
                if start < end
                    && content.is_char_boundary(start)
                    && content.is_char_boundary(end)
                    && pair[0].kind != HighlightKind::Other
                {
                    tokens.push(SyntaxToken {
                        range: SourceRange::new(offset + start, offset + end),
                        kind: pair[0].kind,
                    });
                }
            }
        }
        offset += line.len_bytes();
    }
    (tokens.into(), checkpoints)
}
struct Discard;
impl lsh::runtime::ExtendOne<lsh::runtime::Highlight<HighlightKind>> for Discard {
    fn extend_one(&mut self, _: lsh::runtime::Highlight<HighlightKind>) {}
}

#[cfg(test)]
#[path = "../../tests/unit/editor/syntax/tests.rs"]
mod tests;
