use app::editor::search::{Replacement, SearchEngine, SearchOptions};
use app::editor::soft_wrap::WrapLayout;
use app::editor::syntax::{self, HighlightKind};
use app::editor::{EditorCommand, EditorEffect, EditorPosition, EditorState, SourceBuffer};
use unicode_segmentation::UnicodeSegmentation;

fn regex() -> SearchOptions {
    SearchOptions {
        regex: true,
        ..Default::default()
    }
}

#[test]
fn search_uses_utf8_bytes_and_never_splits_graphemes() {
    let source = "中e\u{301}👩‍💻中文\r\n中";
    assert_eq!(
        SearchEngine::new("中", Default::default())
            .unwrap()
            .find_all(source),
        vec![0..3, 17..20, 25..28]
    );
    for fragment in ["e", "\u{301}", "👩", "💻", "\r", "\n"] {
        assert!(
            SearchEngine::new(fragment, Default::default())
                .unwrap()
                .find_all(source)
                .is_empty(),
            "{fragment}"
        );
    }
    let matches = SearchEngine::new(r".", regex()).unwrap().find_all(source);
    let boundaries: Vec<_> = source
        .grapheme_indices(true)
        .map(|(i, _)| i)
        .chain([source.len()])
        .collect();
    assert!(
        matches
            .iter()
            .all(|r| boundaries.contains(&r.start) && boundaries.contains(&r.end))
    );
}

#[test]
fn regex_captures_literal_dollars_multiline_and_options() {
    let engine = SearchEngine::new(r"^(?<key>\w+)=(\d+)$", regex()).unwrap();
    let edits = engine.preview("a=12\n中文=34", "${key}: $2 $$");
    assert_eq!(edits.len(), 2);
    assert_eq!(edits[1].replacement, "中文: 34 $");
    assert_eq!(edits[1].range, 5..14);
    let options = SearchOptions {
        case_sensitive: false,
        whole_word: true,
        ..Default::default()
    };
    assert_eq!(
        SearchEngine::new("cat", options)
            .unwrap()
            .find_all("Cat catfish CAT"),
        vec![0..3, 12..15]
    );
    assert_eq!(
        SearchEngine::new("a.b", Default::default())
            .unwrap()
            .preview("a.b axb", "$1")[0]
            .replacement,
        "$1"
    );
    assert!(SearchEngine::new("[", regex()).is_err());
    assert!(
        SearchEngine::new("", regex())
            .unwrap()
            .find_all("abc")
            .is_empty()
    );
}

#[test]
fn batch_replace_is_one_undo_preserves_metadata_and_rejects_stale_preview() {
    let mut state = EditorState::open("test.txt", "\u{feff}中=1\r\n中=2\r\n".as_bytes()).unwrap();
    let original = state.to_bytes();
    let engine = SearchEngine::new(r"中=(\d)", regex()).unwrap();
    let edits = engine.preview(&state.source(), "字段=$1");
    let revision = state.revision();
    state.apply(EditorCommand::ReplaceMatches {
        revision,
        replacements: edits.clone(),
    });
    assert_eq!(state.to_bytes(), "\u{feff}字段=1\r\n字段=2\r\n".as_bytes());
    let after = state.to_bytes();
    assert!(matches!(
        state
            .apply(EditorCommand::ReplaceMatches {
                revision,
                replacements: edits
            })
            .as_slice(),
        [EditorEffect::Message(_)]
    ));
    state.apply(EditorCommand::Undo);
    assert_eq!(state.to_bytes(), original);
    assert!(!state.can_undo());
    state.apply(EditorCommand::Redo);
    assert_eq!(state.to_bytes(), after);
}

#[test]
fn replacement_rejects_partial_clusters_and_overlap_atomically() {
    let mut state = EditorState::open("test.txt", "e\u{301}abc".as_bytes()).unwrap();
    for replacements in [
        vec![Replacement {
            range: 0..1,
            original: "e".into(),
            replacement: "X".into(),
        }],
        vec![
            Replacement {
                range: 3..5,
                original: "ab".into(),
                replacement: "X".into(),
            },
            Replacement {
                range: 4..6,
                original: "bc".into(),
                replacement: "Y".into(),
            },
        ],
    ] {
        assert!(
            !state
                .apply(EditorCommand::ReplaceMatches {
                    revision: 0,
                    replacements
                })
                .is_empty()
        );
        assert_eq!(state.source(), "e\u{301}abc");
        assert!(!state.can_undo());
    }
}

#[test]
fn zero_length_regex_moves_and_replaces_without_looping() {
    let mut state = EditorState::open("test.txt", b"a\nb").unwrap();
    state.apply(EditorCommand::Find {
        query: "^".into(),
        options: regex(),
        backwards: false,
    });
    assert_eq!(state.cursor.byte_offset, 0);
    let matches = state.search.as_ref().unwrap().matches.clone();
    state.apply(EditorCommand::FindNext { backwards: false });
    assert!(std::sync::Arc::ptr_eq(
        &matches,
        &state.search.as_ref().unwrap().matches
    ));
    assert_eq!(state.cursor.byte_offset, 2);
    state.apply(EditorCommand::FindNext { backwards: false });
    assert_eq!(state.cursor.byte_offset, 0);
    state.apply(EditorCommand::FindNext { backwards: true });
    assert_eq!(state.cursor.byte_offset, 2);
    let edits = SearchEngine::new("^", regex())
        .unwrap()
        .preview(&state.source(), "> ");
    state.apply(EditorCommand::ReplaceMatches {
        revision: state.revision(),
        replacements: edits,
    });
    assert_eq!(state.source(), "> a\n> b");
    state.apply(EditorCommand::Undo);
    assert_eq!(state.source(), "a\nb");
}

#[test]
fn json_formatter_preserves_tokens_order_duplicates_bom_and_newlines() {
    let original = b"\xef\xbb\xbf{\"z\":1e+02,\"a\":[{},true],\"z\":123456789012345678901234567890,\"escaped\":\"a,\\\"b\"}\r\n";
    let mut state = EditorState::open("a.json", original).unwrap();
    assert!(state.apply(EditorCommand::FormatJson).is_empty());
    assert_eq!(
        state.source(),
        "{\r\n  \"z\": 1e+02,\r\n  \"a\": [\r\n    {},\r\n    true\r\n  ],\r\n  \"z\": 123456789012345678901234567890,\r\n  \"escaped\": \"a,\\\"b\"\r\n}\r\n"
    );
    let formatted = state.to_bytes();
    state.apply(EditorCommand::FormatJson);
    state.apply(EditorCommand::Undo);
    assert_eq!(state.to_bytes(), original);
    state.apply(EditorCommand::Redo);
    assert_eq!(state.to_bytes(), formatted);
    let mut invalid = EditorState::open("a.json", b"{\"a\":1,}").unwrap();
    assert!(matches!(
        invalid.apply(EditorCommand::FormatJson).as_slice(),
        [EditorEffect::Message(_)]
    ));
    assert_eq!(invalid.source(), "{\"a\":1,}");
    assert!(!invalid.can_undo());
}

#[test]
fn read_only_search_works_but_replace_and_format_do_not_mutate() {
    let mut state = EditorState::open_read_only("a.json", b"{\"x\":1}").unwrap();
    state.apply(EditorCommand::Find {
        query: "x".into(),
        options: Default::default(),
        backwards: false,
    });
    assert_eq!(state.selected_text().as_deref(), Some("x"));
    state.apply(EditorCommand::FormatJson);
    let edits = SearchEngine::new("x", Default::default())
        .unwrap()
        .preview(&state.source(), "y");
    state.apply(EditorCommand::ReplaceMatches {
        revision: state.revision(),
        replacements: edits,
    });
    assert_eq!(state.source(), "{\"x\":1}");
    assert!(!state.is_dirty());
}

#[test]
fn lsh_languages_multiline_comments_and_cache_invalidation() {
    for (path, text) in [
        ("a.rs", "fn main() { let x = 42; }"),
        ("a.json", "{\"中\":true}"),
        ("a.toml", "key = \"value\""),
        ("a.md", "# Heading\n**bold**"),
        ("a.py", "def foo():\n    return 42"),
    ] {
        let tokens = syntax::highlight_text(path, text);
        assert!(!tokens.is_empty(), "{path}");
        assert!(
            tokens
                .iter()
                .all(|t| text.get(t.range.start..t.range.end).is_some())
        );
    }
    assert!(syntax::highlight_text("unknown.txt", "let x = 42").is_empty());
    let mut state = EditorState::open("a.c", b"/* start\ncomment\n*/ int x;").unwrap();
    let old = state.source_syntax_highlights();
    assert!(
        old.iter()
            .any(|t| t.kind == HighlightKind::Comment && t.range.start == 9)
    );
    assert!(std::sync::Arc::ptr_eq(
        &old,
        &state.source_syntax_highlights()
    ));
    state.apply(EditorCommand::InsertText("// ".into()));
    assert!(!std::sync::Arc::ptr_eq(
        &old,
        &state.source_syntax_highlights()
    ));
    state.apply(EditorCommand::Undo);
    assert_eq!(old, state.source_syntax_highlights());
}

#[test]
fn wrapping_round_trips_graphemes_and_preserves_original_bytes() {
    let source = "a中e\u{301}👩‍💻bcdef\r\n\r\n尾\n";
    let rope = SourceBuffer::from_text(source).rope_snapshot();
    for width in [1, 2, 4, 8, 80] {
        let layout = WrapLayout::new(&rope, width);
        for (offset, _) in source
            .grapheme_indices(true)
            .filter(|(_, g)| !g.contains(['\r', '\n']))
        {
            let (row, column) = layout.position(offset);
            assert_eq!(layout.offset(row, column), offset, "{width}: {offset}");
        }
        assert_eq!(layout.rows.last().unwrap().range.end, source.len());
    }
    assert_eq!(rope.to_string(), source);
}

#[test]
fn visual_navigation_keeps_column_across_short_rows_and_reflows_after_edit() {
    let mut state = EditorState::open("a.txt", b"abcdefghijk\nx\nabcdefghij").unwrap();
    let map = state.source_wrap_layout(6).unwrap();
    assert_eq!(map.rows.len(), 6);
    state.apply(EditorCommand::MoveTo {
        position: EditorPosition::Source(3),
        extend_selection: false,
    });
    for expected in [8, 11, 13, 17] {
        state.apply(EditorCommand::MoveVisual {
            width: 6,
            down: true,
            extend_selection: true,
        });
        assert_eq!(state.cursor.byte_offset, expected);
    }
    assert!(state.has_selection());
    assert!(!state.is_dirty());
    assert!(std::sync::Arc::ptr_eq(
        &map,
        &state.source_wrap_layout(6).unwrap()
    ));
    state.apply(EditorCommand::InsertText("中".into()));
    assert!(!std::sync::Arc::ptr_eq(
        &map,
        &state.source_wrap_layout(6).unwrap()
    ));
}
