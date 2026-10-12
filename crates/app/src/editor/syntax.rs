//! LSH tokens over the existing Rope. No terminal colors or rendering live here.
use std::borrow::Cow;
use std::sync::{Arc, Mutex};

use super::SourceRange;
use ropey::Rope;

mod definitions {
    include!(concat!(env!("OUT_DIR"), "/lsh_definitions.rs"));
}
pub use definitions::HighlightKind;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyntaxToken {
    pub range: SourceRange,
    pub kind: HighlightKind,
}

#[derive(Debug, Default)]
pub(super) struct Cache(Mutex<Option<(String, Arc<[SyntaxToken]>)>>);
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
    pub fn get(&self, path: &str, rope: &Rope) -> Arc<[SyntaxToken]> {
        let mut cache = self.0.lock().unwrap();
        if let Some((name, tokens)) = &*cache
            && name == path
        {
            return Arc::clone(tokens);
        }
        let tokens = highlight(path, rope);
        *cache = Some((path.to_owned(), Arc::clone(&tokens)));
        tokens
    }
}

pub fn highlight_text(path: &str, source: &str) -> Arc<[SyntaxToken]> {
    highlight(path, &Rope::from_str(source))
}

pub fn highlight(path: &str, rope: &Rope) -> Arc<[SyntaxToken]> {
    // A leading slash makes **/*.ext associations work for bare file names too.
    let path = format!("/{}", path.replace('\\', "/"));
    let Some((_, language)) = definitions::FILE_ASSOCIATIONS
        .iter()
        .find(|(pattern, _)| lsh::glob::glob_match(pattern.as_bytes(), path.as_bytes()))
    else {
        return Arc::from([]);
    };
    let mut runtime = lsh::runtime::Runtime::new(
        &definitions::ASSEMBLY,
        &definitions::STRINGS,
        &definitions::CHARSETS,
        language.entrypoint,
    );
    let mut tokens = Vec::new();
    let mut spans = Vec::<lsh::runtime::Highlight<HighlightKind>>::new();
    let mut offset = 0;
    for line in rope.lines() {
        let text = Cow::from(line);
        let content = text.trim_end_matches(['\r', '\n']);
        spans.clear();
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
        offset += line.len_bytes();
    }
    tokens.into()
}
