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
