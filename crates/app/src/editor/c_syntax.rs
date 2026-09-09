//! C-only lexical colouring. Offsets refer to the original UTF-8 source,
//! including escaped newlines; no source text or cursor positions are changed.

use std::collections::VecDeque;
use std::path::Path;
use std::sync::{Arc, OnceLock};

use super::SourceRange;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CTokenKind {
    Keyword,
    String,
    Number,
    Comment,
    Preprocessor,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CToken {
    pub range: SourceRange,
    pub kind: CTokenKind,
}

pub fn is_c_file(path: impl AsRef<Path>) -> bool {
    path.as_ref()
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            extension.eq_ignore_ascii_case("c") || extension.eq_ignore_ascii_case("h")
        })
}

pub fn highlight(source: &str) -> Arc<[CToken]> {
    highlight_chars(source.chars())
}

/// Shared derived data is populated only when a C document is displayed.
/// Scrolling and moving the cursor reuse it; editing invalidates it.
#[derive(Debug, Clone, Default)]
pub(super) struct Cache(pub OnceLock<Arc<[CToken]>>);

impl PartialEq for Cache {
    fn eq(&self, _other: &Self) -> bool {
        true
    }
}

impl Eq for Cache {}

// Keep a few characters of lookahead rather than flattening the source rope.
// C removes backslash-newline pairs before recognizing comments and tokens.
struct Input<I: Iterator<Item = char>> {
    chars: std::iter::Peekable<I>,
    ahead: VecDeque<(usize, char, usize)>,
    offset: usize,
    end: usize,
}

impl<I: Iterator<Item = char>> Input<I> {
    fn new(chars: I) -> Self {
        Self {
            chars: chars.peekable(),
            ahead: VecDeque::new(),
            offset: 0,
            end: 0,
        }
    }

    fn peek(&mut self, index: usize) -> Option<char> {
        while self.ahead.len() <= index {
            let start = self.offset;
            let ch = self.chars.next()?;
            self.offset += ch.len_utf8();
            if ch == '\\' && matches!(self.chars.peek(), Some('\r' | '\n')) {
                let newline = self.chars.next().unwrap();
                self.offset += 1;
                if newline == '\r' && self.chars.peek() == Some(&'\n') {
                    self.chars.next();
                    self.offset += 1;
                }
                continue;
            }
            self.ahead.push_back((start, ch, self.offset));
        }
        self.ahead.get(index).map(|(_, ch, _)| *ch)
    }

    fn take(&mut self) -> Option<char> {
        self.peek(0)?;
        let (_, ch, end) = self.ahead.pop_front()?;
        self.end = end;
        Some(ch)
    }

    fn quoted(&mut self, close: char) {
        self.take();
        while let Some(ch) = self.peek(0) {
            // An unfinished literal must not colour the following code line.
            if matches!(ch, '\r' | '\n') {
                break;
            }
            self.take();
            if ch == close {
                break;
            }
            if ch == '\\' {
                self.take();
            }
        }
    }
}

pub(super) fn highlight_chars(chars: impl Iterator<Item = char>) -> Arc<[CToken]> {
    let mut input = Input::new(chars);
    let mut tokens = Vec::new();
    let mut line_start = true;
    let mut directive = false;
    let mut directive_name = false;
    let mut header = false;
    while let Some(ch) = input.peek(0) {
        let start = input.ahead.front().unwrap().0;
        let kind = if ch.is_whitespace() {
            input.take();
            if matches!(ch, '\r' | '\n') {
                line_start = true;
                directive = false;
                directive_name = false;
                header = false;
            }
            continue;
        } else if ch == '/' && input.peek(1) == Some('/') {
            while input.peek(0).is_some_and(|ch| !matches!(ch, '\r' | '\n')) {
                input.take();
            }
            Some(CTokenKind::Comment)
        } else if ch == '/' && input.peek(1) == Some('*') {
            input.take();
            input.take();
            while let Some(ch) = input.take() {
                if matches!(ch, '\r' | '\n') {
                    line_start = true;
                    directive = false;
                    directive_name = false;
                    header = false;
                }
                if ch == '*' && input.peek(0) == Some('/') {
                    input.take();
                    break;
                }
            }
            Some(CTokenKind::Comment)
        } else if line_start && (ch == '#' || ch == '%' && input.peek(1) == Some(':')) {
            input.take();
            if ch == '%' {
                input.take();
            }
            line_start = false;
            directive = true;
            directive_name = true;
            Some(CTokenKind::Preprocessor)
        } else {
            line_start = false;
            let prefix = match ch {
                'u' if input.peek(1) == Some('8') && matches!(input.peek(2), Some('\'' | '"')) => 2,
                'u' | 'U' | 'L' if matches!(input.peek(1), Some('\'' | '"')) => 1,
                _ => 0,
            };
            if prefix > 0 || matches!(ch, '\'' | '"') || header && ch == '<' {
                for _ in 0..prefix {
                    input.take();
                }
                let quote = input.peek(0).unwrap();
                input.quoted(if quote == '<' { '>' } else { quote });
                header = false;
                Some(CTokenKind::String)
            } else if ch.is_ascii_digit()
                || ch == '.' && input.peek(1).is_some_and(|ch| ch.is_ascii_digit())
            {
                let mut previous = input.take().unwrap();
                while let Some(next) = input.peek(0) {
                    if next.is_alphanumeric()
                        || matches!(next, '_' | '.' | '\'')
                        || matches!(next, '+' | '-') && matches!(previous, 'e' | 'E' | 'p' | 'P')
                    {
                        previous = input.take().unwrap();
                    } else {
                        break;
                    }
                }
                Some(CTokenKind::Number)
            } else if ch == '_' || ch.is_alphabetic() || !ch.is_ascii() {
                let mut word = String::new();
                while input
                    .peek(0)
                    .is_some_and(|ch| ch == '_' || ch.is_alphanumeric() || !ch.is_ascii())
                {
                    let ch = input.take().unwrap();
                    // No C keyword is this long. Bound temporary storage for
                    // very long identifiers while still consuming the token.
                    if word.len() < 32 {
                        word.push(ch);
                    }
                }
                if directive_name {
                    directive_name = false;
                    header = word == "include";
                    Some(CTokenKind::Preprocessor)
                } else if is_keyword(&word) {
                    Some(CTokenKind::Keyword)
                } else {
                    header = false;
                    directive.then_some(CTokenKind::Preprocessor)
                }
            } else {
                input.take();
                header = false;
                directive.then_some(CTokenKind::Preprocessor)
            }
        };
        if let Some(kind) = kind {
            tokens.push(CToken {
                range: SourceRange::new(start, input.end),
                kind,
            });
        }
    }
    tokens.into()
}

fn is_keyword(word: &str) -> bool {
    matches!(
        word,
        "auto"
            | "break"
            | "case"
            | "char"
            | "const"
            | "continue"
            | "default"
            | "do"
            | "double"
            | "else"
            | "enum"
            | "extern"
            | "float"
            | "for"
            | "goto"
            | "if"
            | "inline"
            | "int"
            | "long"
            | "register"
            | "restrict"
            | "return"
            | "short"
            | "signed"
            | "sizeof"
            | "static"
            | "struct"
            | "switch"
            | "typedef"
            | "union"
            | "unsigned"
            | "void"
            | "volatile"
            | "while"
            | "_Alignas"
            | "_Alignof"
            | "_Atomic"
            | "_BitInt"
            | "_Bool"
            | "_Complex"
            | "_Decimal32"
            | "_Decimal64"
            | "_Decimal128"
            | "_Generic"
            | "_Imaginary"
            | "_Noreturn"
            | "_Static_assert"
            | "_Thread_local"
            | "alignas"
            | "alignof"
            | "bool"
            | "constexpr"
            | "false"
            | "nullptr"
            | "static_assert"
            | "thread_local"
            | "true"
            | "typeof"
            | "typeof_unqual"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parts(source: &str) -> Vec<(&str, CTokenKind)> {
        highlight(source)
            .iter()
            .map(|token| (&source[token.range.start..token.range.end], token.kind))
            .collect()
    }

    #[test]
    fn c_tokens_do_not_confuse_identifiers_literals_and_comments() {
        use CTokenKind::*;
        assert_eq!(
            parts("int integer = 0x1.fp+2; char *s = u8\"中文 // text\"; '\\''; // return 42"),
            vec![
                ("int", Keyword),
                ("0x1.fp+2", Number),
                ("char", Keyword),
                ("u8\"中文 // text\"", String),
                ("'\\''", String),
                ("// return 42", Comment),
            ]
        );
        assert_eq!(
            parts("class namespace template auto _Atomic _BitInt constexpr"),
            vec![
                ("auto", Keyword),
                ("_Atomic", Keyword),
                ("_BitInt", Keyword),
                ("constexpr", Keyword),
            ]
        );
    }

    #[test]
    fn directives_headers_and_continuations_keep_original_offsets() {
        use CTokenKind::*;
        assert_eq!(
            parts(" /* leading */ #include <stdio.h>\r\n#define VALUE 1 + \\\r\n2\nreturn VALUE;"),
            vec![
                ("/* leading */", Comment),
                ("#", Preprocessor),
                ("include", Preprocessor),
                ("<stdio.h>", String),
                ("#", Preprocessor),
                ("define", Preprocessor),
                ("VALUE", Preprocessor),
                ("1", Number),
                ("+", Preprocessor),
                ("2", Number),
                ("return", Keyword),
            ]
        );
        assert_eq!(
            parts("ret\\\nurn /\\\n/ comment\\\nmore\nint"),
            vec![
                ("ret\\\nurn", Keyword),
                ("/\\\n/ comment\\\nmore", Comment),
                ("int", Keyword),
            ]
        );
    }

    #[test]
    fn multiline_comments_and_unfinished_literals_recover() {
        use CTokenKind::*;
        assert_eq!(
            parts("/* 注释\r\n int */\rreturn \"unfinished\nint x; /* open"),
            vec![
                ("/* 注释\r\n int */", Comment),
                ("return", Keyword),
                ("\"unfinished", String),
                ("int", Keyword),
                ("/* open", Comment),
            ]
        );
        assert_eq!(
            parts("1e-3 .25f 0b1010u 1'000ULL"),
            vec![
                ("1e-3", Number),
                (".25f", Number),
                ("0b1010u", Number),
                ("1'000ULL", Number),
            ]
        );
    }

    #[test]
    fn only_c_source_and_header_suffixes_enable_highlighting() {
        for name in ["main.c", "include/api.h", "API.H"] {
            assert!(is_c_file(name));
        }
        for name in [
            "main.cpp",
            "api.hpp",
            "main.cc",
            "main.rs",
            "config.json",
            "config.toml",
            "main.c.txt",
            "Untitled",
        ] {
            assert!(!is_c_file(name));
        }
    }
}
