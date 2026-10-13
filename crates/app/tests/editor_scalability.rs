use app::editor::{
    EditorCommand, EditorPosition, EditorState,
    search::{SearchEngine, SearchOptions},
    soft_wrap::WrapLayout,
    syntax,
};
use ropey::Rope;
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use unicode_segmentation::UnicodeSegmentation;

fn settle(state: &mut EditorState) {
    let deadline = Instant::now() + Duration::from_secs(20);
    while state.has_background_work() {
        state.poll_background();
        assert!(Instant::now() < deadline, "worker did not finish");
        std::thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn chunked_search_matches_contiguous_unicode_and_word_semantics() {
    for padding in [980, 984, 65_520, 65_535, 65_536, 65_539] {
        let text = format!(
            "{} Cat catfish CAT 中e\u{301}👩‍💻🇨🇳\r\n尾\r\n{} END",
            "x".repeat(padding),
            "y".repeat(70_000)
        );
        let rope = Rope::from_str(&text);
        for query in [
            "Cat",
            "cat",
            "中",
            "e",
            "e\u{301}",
            "👩",
            "👩‍💻",
            "🇨",
            "🇨🇳",
            "\r",
            "\n",
            "\r\n",
            "尾\r\n",
            "END",
            "absent",
        ] {
            for case_sensitive in [false, true] {
                for whole_word in [false, true] {
                    let engine = SearchEngine::new(
                        query,
                        SearchOptions {
                            regex: false,
                            case_sensitive,
                            whole_word,
                        },
                    )
                    .unwrap();
                    assert_eq!(
                        engine.find_all_rope(&rope),
                        engine.find_all(&text),
                        "{padding}, {query:?}, {case_sensitive}, {whole_word}"
                    );
                    assert_eq!(
                        engine.preview_rope(&rope, "$0中").unwrap(),
                        engine.preview(&text, "$0中")
                    );
                }
            }
        }
        for query in [
            r"\A.*",
            r"END\z",
            r"(?s)Cat.*END",
            r"^",
            r"\b",
            r"(?m)^(.*)$",
        ] {
            let engine = SearchEngine::new(
                query,
                SearchOptions {
                    regex: true,
                    ..Default::default()
                },
            )
            .unwrap();
            assert_eq!(
                engine.find_all_rope(&rope),
                engine.find_all(&text),
                "{query}"
            );
        }
    }
}

#[test]
fn search_checks_regional_indicator_context_across_rope_chunks() {
    let text = format!("{}{} end", "x".repeat(980), "🇨🇳".repeat(9000));
    let rope = Rope::from_str(&text);
    for query in ["🇨", "🇳", "🇨🇳", "🇳🇨", "end"] {
        let engine = SearchEngine::new(query, Default::default()).unwrap();
        assert_eq!(
            engine.find_all_rope(&rope),
            engine.find_all(&text),
            "{query}"
        );
    }
}

#[test]
fn dense_search_pages_keep_exact_counts_and_navigate_both_directions() {
    let text = "hit ".repeat(20_000);
    let mut state = EditorState::open("large.txt", text.as_bytes()).unwrap();
    state.apply(EditorCommand::Find {
        query: "hit".into(),
        options: Default::default(),
        backwards: false,
    });
    assert!(state.search.as_ref().unwrap().pending);
    settle(&mut state);
    assert_eq!(state.search.as_ref().unwrap().total_matches, 20_000);
    assert!(state.search.as_ref().unwrap().matches.len() <= 512);
    let page = state.search.as_ref().unwrap().matches.clone();
    state.apply(EditorCommand::FindNext { backwards: false });
    assert!(Arc::ptr_eq(&page, &state.search.as_ref().unwrap().matches));
    for index in 2..515 {
        state.apply(EditorCommand::FindNext { backwards: false });
        settle(&mut state);
        assert_eq!(state.cursor.byte_offset, index * 4 + 3);
    }
    for index in (0..514).rev() {
        state.apply(EditorCommand::FindNext { backwards: true });
        settle(&mut state);
        assert_eq!(state.cursor.byte_offset, index * 4 + 3);
    }
    state.apply(EditorCommand::FindNext { backwards: true });
    settle(&mut state);
    assert_eq!(state.cursor.byte_offset, text.len() - 1);
    state.apply(EditorCommand::FindNext { backwards: false });
    settle(&mut state);
    assert_eq!(state.cursor.byte_offset, 3);
    assert!(
        SearchEngine::new("hit", Default::default())
            .unwrap()
            .preview_rope(&Rope::from_str(&text), "x")
            .is_err()
    );
}

#[test]
fn stale_search_cannot_move_caret_or_overwrite_a_new_request() {
    let mut state = EditorState::open(
        "large.txt",
        format!("{} needle", "row\n".repeat(100_000)).as_bytes(),
    )
    .unwrap();
    state.apply(EditorCommand::Find {
        query: "needle".into(),
        options: Default::default(),
        backwards: false,
    });
    state.apply(EditorCommand::InsertText("changed".into()));
    settle(&mut state);
    assert_eq!(state.cursor.byte_offset, 7);
    state.apply(EditorCommand::Find {
        query: "needle".into(),
        options: Default::default(),
        backwards: false,
    });
    state.apply(EditorCommand::Find {
        query: "changed".into(),
        options: Default::default(),
        backwards: false,
    });
    settle(&mut state);
    assert_eq!(state.selected_text().as_deref(), Some("changed"));
    state.apply(EditorCommand::Find {
        query: "needle".into(),
        options: Default::default(),
        backwards: false,
    });
    state.apply(EditorCommand::MoveTo {
        position: EditorPosition::Source(0),
        extend_selection: false,
    });
    settle(&mut state);
    assert_eq!(state.cursor.byte_offset, 0);
}

#[test]
fn sparse_wrap_rows_match_reference_even_on_long_unicode_lines() {
    let text = format!(
        "{}\r\n\r\n{}\n",
        "ab中e\u{301}👩‍💻🇨🇳".repeat(12_000),
        "x".repeat(80_000)
    );
    for width in [1, 2, 9, 80] {
        let layout = WrapLayout::new(&Rope::from_str(&text), width);
        let capacity = width.saturating_sub(1).max(1);
        let mut expected = Vec::new();
        let mut base = 0;
        for line in Rope::from_str(&text).lines() {
            let owned = line.to_string();
            let content = owned.trim_end_matches(['\r', '\n']);
            let (mut start, mut column) = (0, 0);
            for (byte, grapheme) in content.grapheme_indices(true) {
                let cells = unicode_width::UnicodeWidthStr::width(grapheme).max(1);
                if column > 0 && column + cells > capacity {
                    expected.push(base + start..base + byte);
                    start = byte;
                    column = 0;
                }
                column += cells;
            }
            expected.push(base + start..base + content.len());
            base += line.len_bytes();
        }
        if layout.row_count() != expected.len() {
            for row in 0..expected.len().min(4000) {
                let actual = layout.row(row).unwrap().range;
                assert_eq!(
                    actual.start..actual.end,
                    expected[row],
                    "first difference width {width}, row {row}, actual {:?}, expected {:?}",
                    &text[actual.start..actual.end],
                    &text[expected[row].clone()]
                );
            }
        }
        assert_eq!(layout.row_count(), expected.len());
        for row in (0..expected.len()).step_by(127).chain([expected.len() - 1]) {
            let range = layout.row(row).unwrap().range;
            assert_eq!(
                range.start..range.end,
                expected[row],
                "width {width} row {row}"
            );
            let (actual, column) = layout.position(range.start);
            assert_eq!((actual, column), (row, 0));
            assert_eq!(layout.offset(actual, column), range.start);
        }
    }
}

#[test]
fn wrap_reuses_unchanged_lines_after_edits_undo_and_resize() {
    let text = "a中e\u{301}👩‍💻 text\r\n".repeat(5000);
    let mut state = EditorState::open("large.txt", text.as_bytes()).unwrap();
    assert!(state.source_wrap_layout(12).is_none());
    settle(&mut state);
    let before = state.source_wrap_layout(12).unwrap();
    for inserted in ["\r", "\n", "x\n中\n", "👩‍💻"] {
        state.apply(EditorCommand::InsertText(inserted.into()));
    }
    for width in [12, 23] {
        state.source_wrap_layout(width);
        settle(&mut state);
        let layout = state.source_wrap_layout(width).unwrap();
        let reference = WrapLayout::new(&state.source_rope_snapshot().unwrap(), width);
        assert_eq!(layout.row_count(), reference.row_count());
        for row in [0, 1, 10, layout.row_count() / 2, layout.row_count() - 1] {
            assert_eq!(layout.row(row), reference.row(row));
        }
    }
    for _ in 0..4 {
        state.apply(EditorCommand::Undo);
    }
    state.source_wrap_layout(12);
    settle(&mut state);
    assert_eq!(state.source_wrap_layout(12).unwrap(), before);
}

#[test]
fn viewport_syntax_preserves_multiline_state_and_rejects_old_revisions() {
    let text = format!("/*\n{}*/\nint tail;\n", "comment text\n".repeat(10_000));
    let mut state = EditorState::open("large.c", text.as_bytes()).unwrap();
    let lines = 9000..9020;
    state.source_syntax_highlights_in(lines.clone());
    settle(&mut state);
    let tokens = state.source_syntax_highlights_in(lines.clone());
    assert!(
        tokens
            .iter()
            .all(|t| t.kind == syntax::HighlightKind::Comment)
    );
    assert_eq!(tokens.len(), 20);
    state.apply(EditorCommand::InsertText("// ".into()));
    state.source_syntax_highlights_in(lines.clone());
    state.apply(EditorCommand::InsertText("*/\n".into()));
    state.source_syntax_highlights_in(lines.clone());
    settle(&mut state);
    let rope = state.source_rope_snapshot().unwrap();
    let start = rope.line_to_byte(lines.start);
    let end = rope.line_to_byte(lines.end);
    let reference: Vec<_> = syntax::highlight("large.c", &rope)
        .iter()
        .filter(|t| t.range.start >= start && t.range.end <= end)
        .cloned()
        .collect();
    assert_eq!(
        &*state.source_syntax_highlights_in(lines),
        reference.as_slice()
    );
}

#[test]
fn ascii_edit_word_counts_match_full_unicode_count_with_long_words_and_punctuation() {
    let mut state = EditorState::open(
        "words.txt",
        format!(
            "{} . a:b 1,2 can't\r\n{}",
            "a".repeat(80_000),
            "123".repeat(30_000)
        )
        .as_bytes(),
    )
    .unwrap();
    let mut seed = 31u64;
    for step in 0..200 {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        let len = state.source_rope_snapshot().unwrap().len_bytes();
        let at = if step % 3 == 0 {
            79_985 + step % 30
        } else {
            seed as usize % len
        };
        state.apply(EditorCommand::MoveTo {
            position: EditorPosition::Source(at),
            extend_selection: false,
        });
        state.apply(EditorCommand::InsertText(
            [" x ", "'", ":", ",", "1", "\r\n", " ", "_", "\t"][step % 9].into(),
        ));
        assert_eq!(state.word_count(), state.source().unicode_words().count());
        state.apply(EditorCommand::DeleteForward);
        assert_eq!(state.word_count(), state.source().unicode_words().count());
        if step % 7 == 0 {
            state.apply(EditorCommand::Undo);
            assert_eq!(state.word_count(), state.source().unicode_words().count());
        }
    }
}

#[test]
fn current_preview_remains_available_when_replace_all_exceeds_budget() {
    use app::editor::search::PreviewTarget;
    let text = "hit ".repeat(30_000);
    let rope = Rope::from_str(&text);
    let engine = SearchEngine::new("hit", Default::default()).unwrap();
    assert!(engine.preview_rope(&rope, "done").is_err());
    for (selection, cursor, backwards, expected) in [
        (None, 8, false, 8..11),
        (None, 8, true, 4..7),
        (Some(8..11), 11, false, 8..11),
        (None, 0, true, text.len() - 4..text.len() - 1),
    ] {
        let edits = engine
            .preview_for(
                &rope,
                "done",
                Some(PreviewTarget {
                    selection,
                    cursor,
                    backwards,
                }),
            )
            .unwrap();
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].range, expected);
    }
}
