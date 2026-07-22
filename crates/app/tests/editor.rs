use std::io::{self, Write};
use std::path::PathBuf;

use unicode_segmentation::UnicodeSegmentation;

use app::editor::{
    CursorMove, DocumentKind, EditorAccess, EditorCommand, EditorDocument, EditorEffect,
    EditorMode, EditorPosition, EditorState, FormatCommand, LineEnding, SaveSnapshot, Selection,
    SourceRange, TableColumnEdge, TableColumnEdit, is_log_document_path,
};
use app::explorer::{EditorAwareOpenRouteResolver, ExplorerOpenRouteResolver, ExplorerOpenTarget};
use app::rich_document::{
    InlineContent, InlineMarks, InlineNode, LinkAttributes, NodeId, ProjectedBlockKind, RichBlock,
    RichBlockKind, RichDocument, RichLineEnding, RichListKind, RichPosition, RichText,
};
use app::rich_edit::RichSelection;
use platform::FileAttributes;

fn rich_container(editor: &EditorState) -> NodeId {
    editor
        .rich_cursor()
        .expect("Rich document should expose an editable cursor")
        .container_id
}

fn move_rich(editor: &mut EditorState, container_id: NodeId, grapheme_offset: usize) {
    editor.apply(EditorCommand::MoveTo {
        position: EditorPosition::Rich(RichPosition::new(container_id, grapheme_offset)),
        extend_selection: false,
    });
}

fn select_rich(editor: &mut EditorState, container_id: NodeId, anchor: usize, focus: usize) {
    move_rich(editor, container_id, anchor);
    editor.apply(EditorCommand::MoveTo {
        position: EditorPosition::Rich(RichPosition::new(container_id, focus)),
        extend_selection: true,
    });
}

fn picker_snapshot(effects: Vec<EditorEffect>) -> SaveSnapshot {
    let [EditorEffect::SaveFilePicker { snapshot, .. }] = effects.as_slice() else {
        panic!("expected one SaveFilePicker effect, got {effects:?}");
    };
    snapshot.clone()
}

#[test]
fn new_documents_choose_the_expected_mode_and_name() {
    let markdown = EditorState::new();
    assert_eq!(markdown.document.kind, DocumentKind::Markdown);
    assert_eq!(markdown.mode, EditorMode::Rich);
    assert_eq!(markdown.document.display_name(), "Untitled.md");
    assert!(!markdown.is_dirty());

    let text = EditorState::untitled(DocumentKind::PlainText);
    assert_eq!(text.mode, EditorMode::Source);
    assert_eq!(text.document.display_name(), "Untitled.txt");
}

#[test]
fn utf8_bom_and_mixed_newlines_round_trip_without_normalization() {
    let bytes = b"\xEF\xBB\xBFfirst\r\nsecond\nthird\r";
    let document = EditorDocument::from_bytes(
        Some(PathBuf::from("mixed.md")),
        DocumentKind::Markdown,
        bytes,
    )
    .expect("valid UTF-8 document");

    assert_eq!(document.source(), "first\r\nsecond\nthird\r");
    assert!(document.metadata.utf8_bom);
    assert!(document.metadata.mixed_line_endings);
    assert_eq!(document.metadata.preferred_line_ending, LineEnding::Cr);
    assert!(document.metadata.has_final_newline);
    assert_eq!(document.to_bytes(), bytes);

    let mut editor = EditorState::from_document(document);
    let snapshot = match editor.apply(EditorCommand::RequestSave).as_slice() {
        [EditorEffect::SaveFile { snapshot, .. }] => snapshot.clone(),
        effects => panic!("expected save effect, got {effects:?}"),
    };
    assert_eq!(snapshot.revision, 0);
    assert_eq!(snapshot.to_bytes().unwrap(), bytes);
}

#[test]
fn invalid_utf8_is_rejected_without_lossy_decoding() {
    let error = EditorDocument::from_bytes(None, DocumentKind::PlainText, &[b'a', 0xff])
        .expect_err("invalid UTF-8 must be rejected");
    assert_eq!(error.valid_up_to, 1);
}

#[test]
fn owned_bytes_open_preserves_utf8_and_bom_metadata() {
    let document = EditorDocument::open_owned(
        PathBuf::from("owned.log"),
        b"\xEF\xBB\xBFalpha\r\nbeta".to_vec(),
    )
    .expect("valid owned UTF-8 document");
    assert_eq!(document.source(), "alpha\r\nbeta");
    assert!(document.metadata.utf8_bom);
    assert_eq!(document.metadata.preferred_line_ending, LineEnding::CrLf);

    let error = EditorDocument::from_owned_bytes(None, DocumentKind::PlainText, vec![b'a', 0xff])
        .expect_err("invalid owned UTF-8 must be rejected");
    assert_eq!(error.valid_up_to, 1);
}

#[test]
fn source_viewport_access_materializes_only_requested_lines() {
    let source = "zero\r\none\ntwo\rthree\n";
    let editor =
        EditorState::open_read_only_owned(PathBuf::from("app.log"), source.as_bytes().to_vec())
            .expect("valid log document");

    assert_eq!(editor.source_len_bytes(), Some(source.len()));
    assert_eq!(editor.source_line_count(), Some(5));
    assert_eq!(editor.source_line_range(1), Some(SourceRange::new(6, 9)));
    assert_eq!(
        editor.source_byte_slice(SourceRange::new(6, 9)).as_deref(),
        Some("one")
    );
    assert_eq!(editor.source_position(8), Some((1, 2)));
    assert_eq!(editor.source_offset(1, 2), Some(8));
    assert_eq!(editor.source_offset(1, usize::MAX), Some(9));

    let lines = editor.source_lines(1..4);
    assert_eq!(lines.len(), 3);
    assert_eq!(lines[0].line_index, 1);
    assert_eq!(lines[0].text, "one");
    assert_eq!(lines[1].text, "two");
    assert_eq!(lines[2].text, "three");
    assert_eq!(editor.source_lines(99..120), Vec::new());
}

#[test]
fn source_viewport_clips_long_lines_without_flattening_them_into_the_result() {
    let source = format!("{}\nshort", "x".repeat(2 * 1024 * 1024));
    let long_line_len = 2 * 1024 * 1024;
    let editor = EditorState::open_read_only_owned(PathBuf::from("huge.log"), source.into_bytes())
        .expect("valid log document");
    assert_eq!(
        editor.source_position(long_line_len),
        Some((0, long_line_len))
    );
    assert_eq!(editor.source_max_display_width(), Some(long_line_len));
    assert_eq!(
        editor.source_display_position(long_line_len - 96),
        Some((0, long_line_len - 96))
    );
    assert_eq!(editor.source_offset(0, long_line_len), Some(long_line_len));
    let viewport = editor.source_viewport_lines(0..1, 0, 80);
    assert_eq!(viewport.len(), 1);
    assert_eq!(viewport[0].text, "x".repeat(80));
    assert_eq!(viewport[0].visible_byte_range, SourceRange::new(0, 80));
    assert!(!viewport[0].truncated_left);
    assert!(viewport[0].truncated_right);

    let deep_start = long_line_len - 96;
    let viewport = editor.source_viewport_lines(0..1, deep_start, 80);
    assert_eq!(viewport.len(), 1);
    assert_eq!(viewport[0].text, "x".repeat(80));
    assert_eq!(
        viewport[0].visible_byte_range,
        SourceRange::new(deep_start, deep_start + 80)
    );
    assert_eq!(viewport[0].start_column, deep_start);
    assert_eq!(viewport[0].end_column, deep_start + 80);
    assert!(viewport[0].truncated_left);
    assert!(viewport[0].truncated_right);

    let viewport = editor.source_viewport_lines(0..1, long_line_len + 500, 80);
    assert_eq!(viewport[0].text, "");
    assert_eq!(
        viewport[0].visible_byte_range,
        SourceRange::new(long_line_len, long_line_len)
    );
    assert_eq!(viewport[0].start_column, long_line_len);
    assert_eq!(viewport[0].end_column, long_line_len);

    let unicode = EditorState::open_read_only_owned(
        PathBuf::from("unicode.log"),
        "a好🙂b".as_bytes().to_vec(),
    )
    .expect("valid Unicode log");
    let viewport = unicode.source_viewport_lines(0..1, 1, 2);
    assert_eq!(viewport[0].text, "好");
    assert_eq!(viewport[0].start_column, 1);
    assert_eq!(viewport[0].end_column, 3);
    assert!(viewport[0].truncated_left);
    assert!(viewport[0].truncated_right);
    assert_eq!(unicode.source_position(8), Some((0, 3)));
    assert_eq!(unicode.source_offset(0, 3), Some(8));
}

#[test]
fn source_display_positions_and_max_width_use_terminal_cells() {
    let ascii = "1234567";
    let unicode = "a好🙂e\u{301}\t\u{0001}";
    let source = format!("{ascii}\n{unicode}");
    let editor = EditorState::from_document(EditorDocument::from_text(
        None,
        DocumentKind::PlainText,
        source,
    ));
    let line_start = ascii.len() + 1;

    assert_eq!(editor.source_max_display_width(), Some(8));
    assert_eq!(editor.source_display_position(line_start), Some((1, 0)));
    assert_eq!(
        editor.source_display_position(line_start + "a".len()),
        Some((1, 1))
    );
    assert_eq!(
        editor.source_display_position(line_start + "a好".len()),
        Some((1, 3))
    );
    assert_eq!(
        editor.source_display_position(line_start + "a好🙂".len()),
        Some((1, 5))
    );
    assert_eq!(
        editor.source_display_position(line_start + "a好🙂e\u{301}".len()),
        Some((1, 6))
    );
    assert_eq!(
        editor.source_display_position(line_start + unicode.len()),
        Some((1, 8))
    );
    assert_eq!(
        editor.source_position(line_start + unicode.len()),
        Some((1, 7)),
        "Unicode-scalar and terminal-cell columns must remain distinct"
    );
}

#[test]
fn source_max_display_width_initializes_many_short_ascii_lines_sequentially() {
    let mut source = "short log line\n".repeat(100_000);
    source.push_str(&"x".repeat(257));
    let editor = EditorState::open_read_only_owned(PathBuf::from("many.log"), source.into_bytes())
        .expect("valid ASCII log");

    assert_eq!(editor.source_line_count(), Some(100_001));
    assert_eq!(editor.source_max_display_width(), Some(257));
}

#[test]
fn source_max_display_width_tracks_edits_and_history_incrementally() {
    let mut editor = EditorState::from_document(EditorDocument::from_text(
        None,
        DocumentKind::PlainText,
        "longest\nwide\nmid",
    ));
    assert_eq!(editor.source_max_display_width(), Some(7));

    assert!(editor.replace_source_range(SourceRange::new(0, 7), "x"));
    assert_eq!(editor.source_buffer().as_deref(), Some("x\nwide\nmid"));
    assert_eq!(editor.source_max_display_width(), Some(4));

    editor.apply(EditorCommand::Undo);
    assert_eq!(editor.source_max_display_width(), Some(7));
    editor.apply(EditorCommand::Redo);
    assert_eq!(editor.source_max_display_width(), Some(4));

    assert!(editor.replace_source_range(SourceRange::new(1, 2), ""));
    assert_eq!(editor.source_buffer().as_deref(), Some("xwide\nmid"));
    assert_eq!(editor.source_max_display_width(), Some(5));
    editor.apply(EditorCommand::Undo);
    assert_eq!(editor.source_max_display_width(), Some(4));
    editor.apply(EditorCommand::Redo);
    assert_eq!(editor.source_max_display_width(), Some(5));
}

#[test]
fn source_max_display_width_handles_ties_and_unicode_replacements() {
    let mut tied = EditorState::from_document(EditorDocument::from_text(
        None,
        DocumentKind::PlainText,
        "abcd\nwxyz\nq",
    ));
    assert_eq!(tied.source_max_display_width(), Some(4));
    assert!(tied.replace_source_range(SourceRange::new(0, 4), "a"));
    assert_eq!(tied.source_max_display_width(), Some(4));
    assert!(tied.replace_source_range(SourceRange::new(2, 6), "b"));
    assert_eq!(tied.source_buffer().as_deref(), Some("a\nb\nq"));
    assert_eq!(tied.source_max_display_width(), Some(1));

    let mut unicode = EditorState::from_document(EditorDocument::from_text(
        None,
        DocumentKind::PlainText,
        "a好🙂\n12345",
    ));
    assert_eq!(unicode.source_max_display_width(), Some(5));
    assert!(unicode.replace_source_range(SourceRange::new(1, 4), "e\u{301}"));
    assert_eq!(unicode.source_max_display_width(), Some(5));
    assert!(unicode.replace_source_range(SourceRange::new(9, 14), "x"));
    assert_eq!(unicode.source_buffer().as_deref(), Some("ae\u{301}🙂\nx"));
    assert_eq!(unicode.source_max_display_width(), Some(4));
    unicode.apply(EditorCommand::Undo);
    assert_eq!(unicode.source_max_display_width(), Some(5));
    unicode.apply(EditorCommand::Undo);
    assert_eq!(unicode.source_buffer().as_deref(), Some("a好🙂\n12345"));
    assert_eq!(unicode.source_max_display_width(), Some(5));
}

#[test]
fn source_selection_query_and_delta_history_preserve_byte_offsets() {
    let mut editor = EditorState::from_document(EditorDocument::from_text(
        None,
        DocumentKind::PlainText,
        "a🙂b",
    ));
    editor.apply(EditorCommand::MoveTo {
        position: EditorPosition::Source(1),
        extend_selection: false,
    });
    editor.apply(EditorCommand::MoveTo {
        position: EditorPosition::Source(5),
        extend_selection: true,
    });
    assert!(editor.has_selection());
    editor.apply(EditorCommand::InsertText("好".to_owned()));
    assert!(!editor.has_selection());
    assert_eq!(editor.source_buffer().as_deref(), Some("a好b"));
    assert_eq!(editor.position(), Some(EditorPosition::Source(4)));

    editor.apply(EditorCommand::Undo);
    assert_eq!(editor.source_buffer().as_deref(), Some("a🙂b"));
    assert_eq!(editor.selection, Some(Selection::new(1, 5)));
    editor.apply(EditorCommand::Redo);
    assert_eq!(editor.source_buffer().as_deref(), Some("a好b"));
    assert_eq!(editor.position(), Some(EditorPosition::Source(4)));
}

#[test]
fn source_delta_history_retains_the_latest_256_edits() {
    let mut editor = EditorState::untitled(DocumentKind::PlainText);
    for _ in 0..300 {
        editor.apply(EditorCommand::InsertText("x".to_owned()));
    }
    assert_eq!(editor.history_depth(), (256, 0));
    for _ in 0..256 {
        editor.apply(EditorCommand::Undo);
    }
    assert_eq!(editor.source_len_bytes(), Some(44));
    assert_eq!(editor.history_depth(), (0, 256));
}

#[test]
fn source_edits_increment_cached_words_and_line_endings() {
    let mut editor = EditorState::from_document(EditorDocument::from_text(
        None,
        DocumentKind::PlainText,
        "one\rtwo",
    ));
    assert_eq!(editor.word_count(), 2);
    assert_eq!(editor.source_max_display_width(), Some(3));
    assert_eq!(
        editor.document.metadata.preferred_line_ending,
        LineEnding::Cr
    );

    assert!(editor.replace_source_range(SourceRange::new(4, 4), "\n"));
    assert_eq!(editor.source_buffer().as_deref(), Some("one\r\ntwo"));
    assert_eq!(editor.word_count(), 2);
    assert_eq!(editor.source_max_display_width(), Some(3));
    assert_eq!(
        editor.document.metadata.preferred_line_ending,
        LineEnding::CrLf
    );
    assert!(!editor.document.metadata.mixed_line_endings);

    assert!(editor.replace_source_range(SourceRange::new(3, 5), ""));
    assert_eq!(editor.source_buffer().as_deref(), Some("onetwo"));
    assert_eq!(editor.word_count(), 1);
    assert_eq!(editor.source_max_display_width(), Some(6));
    assert!(!editor.document.metadata.has_final_newline);

    editor.apply(EditorCommand::Undo);
    assert_eq!(editor.source_buffer().as_deref(), Some("one\r\ntwo"));
    assert_eq!(editor.word_count(), 2);
    assert_eq!(editor.source_max_display_width(), Some(3));
    assert_eq!(
        editor.document.metadata.preferred_line_ending,
        LineEnding::CrLf
    );
    editor.apply(EditorCommand::Undo);
    assert_eq!(editor.source_buffer().as_deref(), Some("one\rtwo"));
    assert_eq!(
        editor.document.metadata.preferred_line_ending,
        LineEnding::Cr
    );
    editor.apply(EditorCommand::Redo);
    editor.apply(EditorCommand::Redo);
    assert_eq!(editor.source_buffer().as_deref(), Some("onetwo"));
    assert_eq!(editor.word_count(), 1);
}

#[test]
fn source_save_snapshot_streams_rope_chunks() {
    #[derive(Default)]
    struct RecordingWriter {
        bytes: Vec<u8>,
        writes: usize,
        largest_write: usize,
    }

    impl Write for RecordingWriter {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.writes += 1;
            self.largest_write = self.largest_write.max(bytes.len());
            self.bytes.extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    let source = "source line\n".repeat(200_000);
    let mut editor = EditorState::from_document(EditorDocument::from_text(
        Some(PathBuf::from("large.txt")),
        DocumentKind::PlainText,
        source.clone(),
    ));
    editor.apply(EditorCommand::MoveCursor {
        movement: CursorMove::DocumentEnd,
        extend_selection: false,
    });
    editor.apply(EditorCommand::InsertText("tail".to_owned()));
    let effects = editor.apply(EditorCommand::RequestSave);
    let [EditorEffect::SaveFile { snapshot, .. }] = effects.as_slice() else {
        panic!("expected SaveFile effect");
    };
    let mut writer = RecordingWriter::default();
    snapshot.write_to(&mut writer).expect("streamed save");
    assert_eq!(writer.bytes.len(), source.len() + 4);
    assert!(writer.writes > 1);
    assert!(writer.largest_write < writer.bytes.len());

    editor.apply(EditorCommand::MarkSaved {
        path: Some(PathBuf::from("large.txt")),
        revision: snapshot.revision,
    });
    assert!(!editor.is_dirty());
}

#[test]
fn read_only_editor_blocks_mutating_commands_and_direct_source_replacement() {
    let mut editor = EditorState::open_read_only(PathBuf::from("app.log"), b"initial")
        .expect("valid log document");
    let original = editor.export_text();
    let revision = editor.revision();

    for command in [
        EditorCommand::InsertText(" text".to_owned()),
        EditorCommand::InsertNewline,
        EditorCommand::Paste(" paste".to_owned()),
        EditorCommand::Backspace,
        EditorCommand::DeleteForward,
        EditorCommand::DeleteSelection,
        EditorCommand::Undo,
        EditorCommand::Redo,
        EditorCommand::RequestPaste,
        EditorCommand::RequestOpen,
        EditorCommand::RequestSave,
        EditorCommand::RequestSaveAs,
        EditorCommand::ReplaceDocument(EditorDocument::from_text(
            None,
            DocumentKind::PlainText,
            "replacement",
        )),
        EditorCommand::MarkSaved {
            path: Some(PathBuf::from("other.log")),
            revision: 0,
        },
    ] {
        assert!(editor.apply(command).is_empty());
    }

    assert_eq!(editor.access(), EditorAccess::ReadOnly);
    assert!(editor.is_read_only());
    assert_eq!(editor.export_text(), original);
    assert_eq!(editor.revision(), revision);
    assert_eq!(editor.history_depth(), (0, 0));
    assert!(!editor.replace_source_range(SourceRange::new(0, 1), "X"));
    assert_eq!(editor.export_text(), original);
}

#[test]
fn read_only_editor_keeps_navigation_selection_copy_and_close_available() {
    let mut editor = EditorState::open_read_only(PathBuf::from("app.log"), b"alpha beta")
        .expect("valid log document");

    editor.apply(EditorCommand::MoveTo {
        position: EditorPosition::Source(6),
        extend_selection: false,
    });
    editor.apply(EditorCommand::MoveTo {
        position: EditorPosition::Source(10),
        extend_selection: true,
    });

    assert_eq!(editor.selected_text().as_deref(), Some("beta"));
    assert_eq!(
        editor.apply(EditorCommand::Copy),
        vec![EditorEffect::WriteClipboard("beta".to_owned())]
    );
    assert_eq!(
        editor.apply(EditorCommand::RequestClose),
        vec![EditorEffect::Close]
    );
}

#[test]
fn read_only_editor_allows_view_mode_changes() {
    let document = EditorDocument::from_text(
        Some(PathBuf::from("report.md")),
        DocumentKind::Markdown,
        "# report",
    );
    let mut editor = EditorState::from_read_only_document(document);

    assert!(EditorCommand::SetMode(EditorMode::Source).is_allowed_in_read_only());
    assert!(EditorCommand::ToggleMode.is_allowed_in_read_only());
    assert!(!EditorCommand::RequestSave.is_allowed_in_read_only());
    editor.apply(EditorCommand::SetMode(EditorMode::Source));
    assert_eq!(editor.mode, EditorMode::Source);
    editor.apply(EditorCommand::ToggleMode);
    assert_eq!(editor.mode, EditorMode::Rich);
    assert!(!editor.is_dirty());
}

#[test]
fn rich_unicode_graphemes_use_logical_positions_and_delete_atomically() {
    let mut editor = EditorState::new();
    editor.apply(EditorCommand::InsertText("A好e\u{301}🙂".to_owned()));
    assert_eq!(editor.export_text(), "A好e\u{301}🙂");
    assert_eq!(editor.rich_cursor().unwrap().grapheme_offset, 4);

    editor.apply(EditorCommand::MoveCursor {
        movement: CursorMove::Left,
        extend_selection: false,
    });
    editor.apply(EditorCommand::Backspace);
    assert_eq!(editor.export_text(), "A好🙂");
    assert_eq!(editor.rich_cursor().unwrap().grapheme_offset, 2);
    assert_eq!(editor.cursor_line_column(), (0, 2));

    editor.apply(EditorCommand::Backspace);
    assert_eq!(editor.export_text(), "A🙂");
    assert!(editor.is_dirty());
}

#[test]
fn source_crlf_is_one_cursor_step_and_one_delete_unit() {
    let document = EditorDocument::from_text(None, DocumentKind::PlainText, "a\r\nb");
    let mut editor = EditorState::from_document(document);
    editor.apply(EditorCommand::MoveTo {
        position: EditorPosition::Source(3),
        extend_selection: false,
    });
    editor.apply(EditorCommand::Backspace);
    assert_eq!(editor.source_buffer().as_deref(), Some("ab"));
    assert_eq!(editor.position(), Some(EditorPosition::Source(1)));
}

#[test]
fn source_navigation_steps_unicode_graphemes_in_a_multi_megabyte_line() {
    let prefix_len = 2 * 1024 * 1024;
    let mut source = "x".repeat(prefix_len);
    source.push_str("e\u{301}🙂\r\nz");
    let combining_start = prefix_len;
    let emoji_start = combining_start + "e\u{301}".len();
    let first_line_end = emoji_start + "🙂".len();
    let second_line_start = first_line_end + "\r\n".len();
    let mut editor = EditorState::from_document(EditorDocument::from_text(
        None,
        DocumentKind::PlainText,
        source,
    ));
    editor.apply(EditorCommand::MoveTo {
        position: EditorPosition::Source(first_line_end),
        extend_selection: false,
    });

    editor.apply(EditorCommand::MoveCursor {
        movement: CursorMove::Left,
        extend_selection: false,
    });
    assert_eq!(editor.position(), Some(EditorPosition::Source(emoji_start)));
    editor.apply(EditorCommand::MoveCursor {
        movement: CursorMove::Left,
        extend_selection: false,
    });
    assert_eq!(
        editor.position(),
        Some(EditorPosition::Source(combining_start))
    );

    editor.apply(EditorCommand::MoveCursor {
        movement: CursorMove::Right,
        extend_selection: false,
    });
    assert_eq!(editor.position(), Some(EditorPosition::Source(emoji_start)));
    editor.apply(EditorCommand::MoveCursor {
        movement: CursorMove::Right,
        extend_selection: false,
    });
    assert_eq!(
        editor.position(),
        Some(EditorPosition::Source(first_line_end))
    );
    editor.apply(EditorCommand::MoveCursor {
        movement: CursorMove::Right,
        extend_selection: false,
    });
    assert_eq!(
        editor.position(),
        Some(EditorPosition::Source(second_line_start))
    );
    editor.apply(EditorCommand::MoveCursor {
        movement: CursorMove::Left,
        extend_selection: false,
    });
    assert_eq!(
        editor.position(),
        Some(EditorPosition::Source(first_line_end))
    );
}

#[test]
fn source_navigation_matches_grapheme_boundaries_across_rope_chunks() {
    let source = "Ae\u{301}👨‍👩‍👧‍👦👍🏽🇨🇳क्‍ष\r\n".repeat(128);
    let mut boundaries = source
        .grapheme_indices(true)
        .map(|(offset, _)| offset)
        .collect::<Vec<_>>();
    boundaries.push(source.len());
    let mut editor = EditorState::from_document(EditorDocument::from_text(
        None,
        DocumentKind::PlainText,
        source,
    ));

    for &boundary in &boundaries[1..] {
        editor.apply(EditorCommand::MoveCursor {
            movement: CursorMove::Right,
            extend_selection: false,
        });
        assert_eq!(editor.position(), Some(EditorPosition::Source(boundary)));
    }
    for &boundary in boundaries[..boundaries.len() - 1].iter().rev() {
        editor.apply(EditorCommand::MoveCursor {
            movement: CursorMove::Left,
            extend_selection: false,
        });
        assert_eq!(editor.position(), Some(EditorPosition::Source(boundary)));
    }
}

#[test]
fn source_vertical_navigation_preserves_grapheme_columns_without_line_strings() {
    // The first line's third grapheme starts at byte 8 even though its prior
    // graphemes contain a combining sequence and a four-byte emoji.
    let source = "Ae\u{301}🙂Z\n01234";
    let second_line_column_three = "Ae\u{301}🙂Z\n".len() + 3;
    let mut editor = EditorState::from_document(EditorDocument::from_text(
        None,
        DocumentKind::PlainText,
        source,
    ));
    editor.apply(EditorCommand::MoveTo {
        position: EditorPosition::Source(second_line_column_three),
        extend_selection: false,
    });

    editor.apply(EditorCommand::MoveCursor {
        movement: CursorMove::Up,
        extend_selection: false,
    });
    assert_eq!(editor.position(), Some(EditorPosition::Source(8)));
    editor.apply(EditorCommand::MoveCursor {
        movement: CursorMove::Down,
        extend_selection: false,
    });
    assert_eq!(
        editor.position(),
        Some(EditorPosition::Source(second_line_column_three))
    );
}

#[test]
fn rich_selection_copy_cut_paste_and_undo_share_the_native_model() {
    let mut editor = EditorState::new();
    editor.apply(EditorCommand::InsertText("alpha beta".to_owned()));
    let id = rich_container(&editor);
    select_rich(&mut editor, id, 0, 5);

    assert_eq!(
        editor.apply(EditorCommand::Copy),
        vec![EditorEffect::WriteClipboard("alpha".to_owned())]
    );
    assert_eq!(
        editor.apply(EditorCommand::Cut),
        vec![EditorEffect::WriteClipboard("alpha".to_owned())]
    );
    assert_eq!(editor.export_text(), " beta");
    editor.apply(EditorCommand::Paste("ALPHA".to_owned()));
    assert_eq!(editor.export_text(), "ALPHA beta");

    editor.apply(EditorCommand::Undo);
    assert_eq!(editor.export_text(), " beta");
    editor.apply(EditorCommand::Undo);
    assert_eq!(editor.export_text(), "alpha beta");
    editor.apply(EditorCommand::Redo);
    assert_eq!(editor.export_text(), " beta");
}

#[test]
fn saved_revision_checkpoint_drives_dirty_state_even_across_undo() {
    let mut editor = EditorState::new();
    editor.apply(EditorCommand::InsertText("saved".to_owned()));
    let snapshot = picker_snapshot(editor.apply(EditorCommand::RequestSave));
    assert_eq!(snapshot.revision, editor.revision());
    editor.apply(EditorCommand::MarkSaved {
        path: Some(PathBuf::from("note.md")),
        revision: snapshot.revision,
    });
    assert_eq!(editor.saved_revision(), snapshot.revision);
    assert!(!editor.is_dirty());

    editor.apply(EditorCommand::InsertText(" change".to_owned()));
    assert!(editor.is_dirty());
    editor.apply(EditorCommand::Undo);
    assert!(!editor.is_dirty());
    editor.apply(EditorCommand::Undo);
    assert!(editor.is_dirty());
}

#[test]
fn inline_format_changes_logical_selection_and_toggles_off() {
    let original = "before selected after";
    let mut editor = EditorState::from_document(EditorDocument::from_text(
        None,
        DocumentKind::Markdown,
        original,
    ));
    let id = rich_container(&editor);
    select_rich(&mut editor, id, 7, 15);
    editor.apply(EditorCommand::ApplyFormat(FormatCommand::Bold));
    assert_eq!(editor.export_text(), "before **selected** after");
    assert_eq!(editor.selected_text(), None);
    assert_eq!(editor.rich_cursor(), Some(RichPosition::new(id, 15)));

    select_rich(&mut editor, id, 7, 15);
    editor.apply(EditorCommand::ApplyFormat(FormatCommand::Bold));
    assert_eq!(editor.export_text(), original);
    assert_eq!(editor.selected_text(), None);
    editor.apply(EditorCommand::Undo);
    assert_eq!(editor.export_text(), "before **selected** after");
}

#[test]
fn rich_edits_stay_in_memory_until_an_explicit_boundary() {
    let document = EditorDocument::from_text(
        Some(PathBuf::from("note.md")),
        DocumentKind::Markdown,
        "original",
    );
    let mut editor = EditorState::from_document(document);
    assert!(editor.has_isolated_rich_buffer());
    let id = rich_container(&editor);
    move_rich(&mut editor, id, 8);
    editor.apply(EditorCommand::InsertText(" rich".to_owned()));

    assert_eq!(editor.export_text(), "original rich");
    assert_eq!(editor.source(), "original");
    assert_eq!(editor.document.source(), "original");
    let revision = editor.revision();
    let effects = editor.apply(EditorCommand::RequestSave);
    let [EditorEffect::SaveFile { path, snapshot }] = effects.as_slice() else {
        panic!("expected one SaveFile effect, got {effects:?}");
    };
    assert_eq!(path, &PathBuf::from("note.md"));
    assert_eq!(snapshot.revision, revision);
    assert_eq!(snapshot.to_bytes().unwrap(), b"original rich");
    assert_eq!(editor.document.source(), "original");

    editor.apply(EditorCommand::SetMode(EditorMode::Source));
    assert!(!editor.has_isolated_rich_buffer());
    assert_eq!(editor.source_buffer().as_deref(), Some("original rich"));
    assert_eq!(editor.document.source(), "original rich");
}

#[test]
fn typing_after_rich_formatting_never_exposes_markdown_markers() {
    let mut editor = EditorState::from_document(EditorDocument::from_text(
        None,
        DocumentKind::Markdown,
        "text",
    ));
    let id = rich_container(&editor);
    select_rich(&mut editor, id, 0, 4);

    editor.apply(EditorCommand::ApplyFormat(FormatCommand::Bold));
    editor.apply(EditorCommand::InsertText(" ".to_owned()));
    editor.apply(EditorCommand::InsertNewline);
    editor.apply(EditorCommand::InsertText("next".to_owned()));

    assert_eq!(editor.mode, EditorMode::Rich);
    assert_eq!(editor.export_text(), "**text** \nnext");
    assert_eq!(editor.rich_cursor(), Some(RichPosition::new(id, 10)));
    assert_eq!(editor.rich_selection(), None);
    let projection = editor.rich_projection().unwrap();
    let ProjectedBlockKind::Paragraph { content } = &projection.blocks[0].kind else {
        panic!("expected a rich paragraph");
    };
    assert!(
        content
            .iter()
            .any(|span| span.text == "text" && span.marks.bold)
    );
    assert!(content.iter().all(|span| !span.text.contains("**")));
}

#[test]
fn rich_heading_enter_handles_start_middle_end_and_empty_heading() {
    let mut at_start = EditorState::from_document(EditorDocument::from_text(
        None,
        DocumentKind::Markdown,
        "# title",
    ));
    let heading_id = rich_container(&at_start);
    move_rich(&mut at_start, heading_id, 0);
    at_start.apply(EditorCommand::InsertNewline);
    let projection = at_start.rich_projection().unwrap();
    assert_eq!(projection.blocks.len(), 2);
    assert!(matches!(
        projection.blocks[0].kind,
        ProjectedBlockKind::Paragraph { .. }
    ));
    assert_eq!(projection.blocks[1].id, heading_id);
    assert!(matches!(
        projection.blocks[1].kind,
        ProjectedBlockKind::Heading { level: 1, .. }
    ));
    assert_eq!(
        at_start.rich_cursor(),
        Some(RichPosition::new(projection.blocks[0].id, 0))
    );
    assert_eq!(at_start.export_text(), "\n\n# title");

    at_start.apply(EditorCommand::Backspace);
    let projection = at_start.rich_projection().unwrap();
    assert_eq!(projection.blocks.len(), 1);
    assert_eq!(projection.blocks[0].id, heading_id);
    assert_eq!(
        at_start.rich_cursor(),
        Some(RichPosition::new(heading_id, 0))
    );
    assert_eq!(at_start.export_text(), "# title");

    for (offset, expected_heading, expected_paragraph, expected_markdown) in [
        (2, "ti", "tle", "# ti\n\ntle"),
        (5, "title", "", "# title\n\n"),
    ] {
        let mut editor = EditorState::from_document(EditorDocument::from_text(
            None,
            DocumentKind::Markdown,
            "# title",
        ));
        let heading_id = rich_container(&editor);
        move_rich(&mut editor, heading_id, offset);

        editor.apply(EditorCommand::InsertNewline);

        let projection = editor.rich_projection().unwrap();
        assert_eq!(projection.blocks.len(), 2);
        let ProjectedBlockKind::Heading { level, content } = &projection.blocks[0].kind else {
            panic!("expected the left block to remain a heading");
        };
        assert_eq!(*level, 1);
        assert_eq!(
            content
                .iter()
                .map(|span| span.text.as_str())
                .collect::<String>(),
            expected_heading
        );
        let ProjectedBlockKind::Paragraph { content } = &projection.blocks[1].kind else {
            panic!("expected the right block to become a paragraph");
        };
        assert_eq!(
            content
                .iter()
                .map(|span| span.text.as_str())
                .collect::<String>(),
            expected_paragraph
        );
        assert_eq!(
            editor.rich_cursor(),
            Some(RichPosition::new(projection.blocks[1].id, 0))
        );
        assert_eq!(editor.rich_selection(), None);
        assert_eq!(editor.export_text(), expected_markdown);
        assert_eq!(editor.history_depth(), (1, 0));

        editor.apply(EditorCommand::Undo);
        assert_eq!(editor.export_text(), "# title");
        assert_eq!(editor.rich_projection().unwrap().blocks.len(), 1);
        assert_eq!(editor.history_depth(), (0, 1));
    }

    let mut empty =
        EditorState::from_document(EditorDocument::from_text(None, DocumentKind::Markdown, "#"));
    let empty_heading_id = rich_container(&empty);
    assert!(matches!(
        empty.rich_projection().unwrap().blocks[0].kind,
        ProjectedBlockKind::Heading { .. }
    ));
    empty.apply(EditorCommand::InsertNewline);
    let projection = empty.rich_projection().unwrap();
    assert_eq!(projection.blocks.len(), 1);
    assert_eq!(projection.blocks[0].id, empty_heading_id);
    assert!(matches!(
        projection.blocks[0].kind,
        ProjectedBlockKind::Paragraph { .. }
    ));
    assert_eq!(
        empty.rich_cursor(),
        Some(RichPosition::new(empty_heading_id, 0))
    );
    assert_eq!(empty.export_text(), "");
    empty.apply(EditorCommand::Undo);
    assert_eq!(empty.export_text(), "#");
}

#[test]
fn rich_heading_enter_uses_crlf_and_reopens_as_heading_plus_paragraph() {
    let mut editor = EditorState::from_document(EditorDocument::from_text(
        None,
        DocumentKind::Markdown,
        "# title\r\n",
    ));
    let heading_id = rich_container(&editor);
    move_rich(&mut editor, heading_id, 2);
    editor.apply(EditorCommand::InsertNewline);

    let exported = editor.export_text();
    assert_eq!(exported, "# ti\r\n\r\ntle\r\n");
    let reopened = EditorState::from_document(EditorDocument::from_text(
        None,
        DocumentKind::Markdown,
        &exported,
    ));
    let projection = reopened.rich_projection().unwrap();
    assert_eq!(projection.blocks.len(), 2);
    assert!(matches!(
        projection.blocks[0].kind,
        ProjectedBlockKind::Heading { .. }
    ));
    assert!(matches!(
        projection.blocks[1].kind,
        ProjectedBlockKind::Paragraph { .. }
    ));
}

#[test]
fn rich_heading_enter_preserves_graphemes_marks_and_links_on_both_sides() {
    let mut editor = EditorState::from_document(EditorDocument::from_text(
        None,
        DocumentKind::Markdown,
        "# **A🙂[BC](https://example.com)D**",
    ));
    let heading_id = rich_container(&editor);
    move_rich(&mut editor, heading_id, 3);

    editor.apply(EditorCommand::InsertNewline);

    let projection = editor.rich_projection().unwrap();
    let ProjectedBlockKind::Heading { content: left, .. } = &projection.blocks[0].kind else {
        panic!("expected heading");
    };
    let ProjectedBlockKind::Paragraph { content: right } = &projection.blocks[1].kind else {
        panic!("expected paragraph");
    };
    assert_eq!(
        left.iter()
            .map(|span| span.text.as_str())
            .collect::<String>(),
        "A🙂B"
    );
    assert_eq!(
        right
            .iter()
            .map(|span| span.text.as_str())
            .collect::<String>(),
        "CD"
    );
    assert!(left.iter().chain(right).all(|span| span.marks.bold));
    assert!(left.iter().any(|span| {
        span.text == "B"
            && span
                .link
                .as_ref()
                .is_some_and(|link| link.url == "https://example.com")
    }));
    assert!(right.iter().any(|span| {
        span.text == "C"
            && span
                .link
                .as_ref()
                .is_some_and(|link| link.url == "https://example.com")
    }));
    assert_eq!(
        editor.export_text(),
        "# **A🙂**[**B**](https://example.com)\n\n[**C**](https://example.com)**D**"
    );

    editor.apply(EditorCommand::Backspace);
    let projection = editor.rich_projection().unwrap();
    assert_eq!(projection.blocks.len(), 1);
    assert_eq!(projection.blocks[0].id, heading_id);
    let ProjectedBlockKind::Heading { content, .. } = &projection.blocks[0].kind else {
        panic!("expected merged heading");
    };
    assert_eq!(
        content
            .iter()
            .map(|span| span.text.as_str())
            .collect::<String>(),
        "A🙂BCD"
    );
    assert!(content.iter().all(|span| span.marks.bold));
    assert!(content.iter().any(|span| {
        span.text == "BC"
            && span
                .link
                .as_ref()
                .is_some_and(|link| link.url == "https://example.com")
    }));
    assert_eq!(editor.rich_cursor(), Some(RichPosition::new(heading_id, 3)));
    assert_eq!(editor.history_depth(), (2, 0));
}

#[test]
fn rich_heading_enter_replaces_selection_and_undo_restores_it_once() {
    let mut editor = EditorState::from_document(EditorDocument::from_text(
        None,
        DocumentKind::Markdown,
        "# alphabet",
    ));
    let heading_id = rich_container(&editor);
    select_rich(&mut editor, heading_id, 2, 5);

    editor.apply(EditorCommand::InsertNewline);

    let projection = editor.rich_projection().unwrap();
    assert_eq!(editor.export_text(), "# al\n\nbet");
    assert_eq!(
        editor.rich_cursor(),
        Some(RichPosition::new(projection.blocks[1].id, 0))
    );
    assert_eq!(editor.rich_selection(), None);
    assert_eq!(editor.history_depth(), (1, 0));

    editor.apply(EditorCommand::Undo);
    assert_eq!(editor.export_text(), "# alphabet");
    assert_eq!(editor.history_depth(), (0, 1));
}

#[test]
fn multiline_paste_into_heading_ends_heading_and_round_trips_as_two_blocks() {
    let mut editor = EditorState::from_document(EditorDocument::from_text(
        None,
        DocumentKind::Markdown,
        "# title",
    ));
    let heading_id = rich_container(&editor);
    move_rich(&mut editor, heading_id, 2);

    editor.apply(EditorCommand::Paste("x\ny".to_owned()));

    let projection = editor.rich_projection().unwrap();
    assert_eq!(projection.blocks.len(), 2);
    assert_eq!(projection.blocks[0].id, heading_id);
    let ProjectedBlockKind::Heading { content: left, .. } = &projection.blocks[0].kind else {
        panic!("expected heading");
    };
    let ProjectedBlockKind::Paragraph { content: right } = &projection.blocks[1].kind else {
        panic!("expected paragraph");
    };
    assert_eq!(
        left.iter()
            .map(|span| span.text.as_str())
            .collect::<String>(),
        "tix"
    );
    assert_eq!(
        right
            .iter()
            .map(|span| span.text.as_str())
            .collect::<String>(),
        "ytle"
    );
    assert_eq!(
        editor.rich_cursor(),
        Some(RichPosition::new(projection.blocks[1].id, 1))
    );
    let exported = editor.export_text();
    assert_eq!(exported, "# tix\n\nytle");

    let reopened = EditorState::from_document(EditorDocument::from_text(
        None,
        DocumentKind::Markdown,
        &exported,
    ));
    let reopened_projection = reopened.rich_projection().unwrap();
    assert_eq!(reopened_projection.blocks.len(), 2);
    assert!(matches!(
        reopened_projection.blocks[0].kind,
        ProjectedBlockKind::Heading { .. }
    ));
    assert!(matches!(
        reopened_projection.blocks[1].kind,
        ProjectedBlockKind::Paragraph { .. }
    ));
}

#[test]
fn legacy_rich_draft_heading_breaks_migrate_in_place_and_remap_positions() {
    let mut document = RichDocument::new();
    document.preferred_line_ending = RichLineEnding::CrLf;
    let heading_id = document.allocate_node_id();
    let sibling_id = document.allocate_node_id();
    document.blocks.push(RichBlock::new(
        heading_id,
        RichBlockKind::Heading {
            level: 1,
            content: InlineContent(vec![
                InlineNode::Text(RichText {
                    text: "A🙂".to_owned(),
                    marks: InlineMarks {
                        bold: true,
                        ..InlineMarks::default()
                    },
                    link: None,
                }),
                InlineNode::SoftBreak,
                InlineNode::Text(RichText::plain("    ")),
                InlineNode::Text(RichText {
                    text: "right".to_owned(),
                    marks: InlineMarks {
                        italic: true,
                        ..InlineMarks::default()
                    },
                    link: Some(LinkAttributes {
                        url: "https://example.com".to_owned(),
                        title: Some("kept".to_owned()),
                    }),
                }),
            ]),
        },
    ));
    document.blocks.push(RichBlock::new(
        sibling_id,
        RichBlockKind::Paragraph {
            content: InlineContent::plain("unchanged"),
        },
    ));

    let mut editor = EditorState::new();
    editor.install_rich_draft(
        document,
        Some(RichPosition::new(heading_id, 3)),
        Some(RichSelection::new(
            RichPosition::new(heading_id, 1),
            RichPosition::new(heading_id, 4),
        )),
    );

    let projection = editor.rich_projection().unwrap();
    assert_eq!(projection.blocks.len(), 3);
    assert_eq!(projection.blocks[0].id, heading_id);
    assert_eq!(projection.blocks[2].id, sibling_id);
    let paragraph_id = projection.blocks[1].id;
    let ProjectedBlockKind::Heading { content: left, .. } = &projection.blocks[0].kind else {
        panic!("expected migrated heading");
    };
    let ProjectedBlockKind::Paragraph { content: right } = &projection.blocks[1].kind else {
        panic!("expected migrated paragraph");
    };
    assert_eq!(
        left.iter()
            .map(|span| span.text.as_str())
            .collect::<String>(),
        "A🙂"
    );
    assert!(left.iter().all(|span| span.marks.bold));
    assert_eq!(
        right
            .iter()
            .map(|span| span.text.as_str())
            .collect::<String>(),
        "    right"
    );
    assert!(right.iter().any(|span| {
        span.text == "right"
            && span.marks.italic
            && span
                .link
                .as_ref()
                .is_some_and(|link| link.url == "https://example.com")
    }));
    assert_eq!(
        editor.rich_cursor(),
        Some(RichPosition::new(paragraph_id, 0))
    );
    assert_eq!(
        editor.rich_selection(),
        Some(RichSelection::new(
            RichPosition::new(heading_id, 1),
            RichPosition::new(paragraph_id, 1),
        ))
    );
    assert!(editor.export_text().contains("\r\n\r\n"));
}

#[test]
fn legacy_nested_and_empty_heading_breaks_are_fully_normalized() {
    let mut document = RichDocument::new();
    let quote_id = document.allocate_node_id();
    let nested_heading_id = document.allocate_node_id();
    let empty_heading_id = document.allocate_node_id();
    document.blocks.push(RichBlock::new(
        quote_id,
        RichBlockKind::Quote {
            blocks: vec![RichBlock::new(
                nested_heading_id,
                RichBlockKind::Heading {
                    level: 2,
                    content: InlineContent(vec![
                        InlineNode::SoftBreak,
                        InlineNode::Text(RichText::plain("Nested")),
                        InlineNode::HardBreak,
                        InlineNode::Text(RichText::plain("tail")),
                    ]),
                },
            )],
        },
    ));
    document.blocks.push(RichBlock::new(
        empty_heading_id,
        RichBlockKind::Heading {
            level: 3,
            content: InlineContent(vec![InlineNode::SoftBreak]),
        },
    ));

    let mut editor = EditorState::new();
    editor.install_rich_draft(document, Some(RichPosition::new(empty_heading_id, 1)), None);

    let projection = editor.rich_projection().unwrap();
    assert_eq!(projection.blocks[0].id, quote_id);
    let ProjectedBlockKind::Quote { blocks } = &projection.blocks[0].kind else {
        panic!("expected quote");
    };
    assert_eq!(blocks.len(), 3);
    assert!(matches!(
        blocks.as_slice(),
        [
            app::rich_document::ProjectedBlock {
                kind: ProjectedBlockKind::Paragraph { .. },
                ..
            },
            app::rich_document::ProjectedBlock {
                id,
                kind: ProjectedBlockKind::Heading { level: 2, .. },
            },
            app::rich_document::ProjectedBlock {
                kind: ProjectedBlockKind::Paragraph { .. },
                ..
            }
        ] if *id == nested_heading_id
    ));
    assert_eq!(projection.blocks[1].id, empty_heading_id);
    assert!(matches!(
        projection.blocks[1].kind,
        ProjectedBlockKind::Paragraph { .. }
    ));
    assert_eq!(
        editor.rich_cursor(),
        Some(RichPosition::new(empty_heading_id, 0))
    );
    assert!(projection.blocks.iter().all(|block| match &block.kind {
        ProjectedBlockKind::Heading { content, .. } => {
            content.iter().all(|span| !span.text.contains('\n'))
        }
        ProjectedBlockKind::Quote { blocks } => blocks.iter().all(|block| match &block.kind {
            ProjectedBlockKind::Heading { content, .. } => {
                content.iter().all(|span| !span.text.contains('\n'))
            }
            _ => true,
        }),
        _ => true,
    }));
}

#[test]
fn rich_heading_enter_splits_nested_quote_and_list_blocks() {
    let mut quote = EditorState::from_document(EditorDocument::from_text(
        None,
        DocumentKind::Markdown,
        "> # title",
    ));
    let quote_heading_id = match &quote.rich_projection().unwrap().blocks[0].kind {
        ProjectedBlockKind::Quote { blocks } => blocks[0].id,
        other => panic!("expected quote, got {other:?}"),
    };
    move_rich(&mut quote, quote_heading_id, 2);
    quote.apply(EditorCommand::InsertNewline);
    let quote_projection = quote.rich_projection().unwrap();
    assert!(matches!(
        &quote_projection.blocks[0].kind,
        ProjectedBlockKind::Quote { blocks }
            if blocks.len() == 2
                && matches!(blocks[0].kind, ProjectedBlockKind::Heading { .. })
                && matches!(blocks[1].kind, ProjectedBlockKind::Paragraph { .. })
    ));
    assert_eq!(quote.export_text(), "> # ti\n>\n> tle");

    let mut list = EditorState::from_document(EditorDocument::from_text(
        None,
        DocumentKind::Markdown,
        "- # title",
    ));
    let list_heading_id = match &list.rich_projection().unwrap().blocks[0].kind {
        ProjectedBlockKind::List { items, .. } => items[0].blocks[0].id,
        other => panic!("expected list, got {other:?}"),
    };
    move_rich(&mut list, list_heading_id, 2);
    list.apply(EditorCommand::InsertNewline);
    let list_projection = list.rich_projection().unwrap();
    assert!(matches!(
        &list_projection.blocks[0].kind,
        ProjectedBlockKind::List { items, .. }
            if items[0].blocks.len() == 2
                && matches!(items[0].blocks[0].kind, ProjectedBlockKind::Heading { .. })
                && matches!(items[0].blocks[1].kind, ProjectedBlockKind::Paragraph { .. })
    ));
    assert_eq!(list.export_text(), "- # ti\n  \n  tle");
}

#[test]
fn collapsed_inline_format_does_not_create_empty_markers_or_history() {
    let formats = [
        FormatCommand::Bold,
        FormatCommand::Italic,
        FormatCommand::Strikethrough,
        FormatCommand::InlineCode,
    ];

    for format in formats {
        let mut editor = EditorState::new();
        editor.apply(EditorCommand::ApplyFormat(format));
        assert_eq!(editor.export_text(), "");
        assert_eq!(editor.rich_cursor(), None);
        assert_eq!(editor.history_depth(), (0, 0));
        assert_eq!(editor.revision(), 0);
    }
}

#[test]
fn source_and_plain_text_modes_reject_markdown_format_commands() {
    let formats = vec![
        FormatCommand::Bold,
        FormatCommand::Italic,
        FormatCommand::Strikethrough,
        FormatCommand::InlineCode,
        FormatCommand::Heading(2),
        FormatCommand::Paragraph,
        FormatCommand::Quote,
        FormatCommand::BulletList,
        FormatCommand::OrderedList,
        FormatCommand::TaskList,
        FormatCommand::Link {
            url: "https://example.com".to_owned(),
            title: None,
        },
        FormatCommand::Image {
            url: "image.png".to_owned(),
            alt: "image".to_owned(),
            title: None,
        },
        FormatCommand::Table {
            columns: 2,
            rows: 1,
        },
    ];

    let mut source = EditorState::from_document(EditorDocument::from_text(
        None,
        DocumentKind::Markdown,
        "plain source",
    ));
    source.apply(EditorCommand::SetMode(EditorMode::Source));
    source.apply(EditorCommand::MoveTo {
        position: EditorPosition::Source(0),
        extend_selection: false,
    });
    source.apply(EditorCommand::MoveTo {
        position: EditorPosition::Source(5),
        extend_selection: true,
    });
    for format in formats {
        source.apply(EditorCommand::ApplyFormat(format));
    }
    assert_eq!(source.source_buffer().as_deref(), Some("plain source"));
    assert_eq!(source.selection, Some(Selection::new(0, 5)));
    assert_eq!(source.position(), Some(EditorPosition::Source(5)));
    assert_eq!(source.history_depth(), (0, 0));

    let mut text = EditorState::untitled(DocumentKind::PlainText);
    text.apply(EditorCommand::SetMode(EditorMode::Rich));
    assert_eq!(text.mode, EditorMode::Source);
    text.apply(EditorCommand::ToggleMode);
    assert_eq!(text.mode, EditorMode::Source);
}

#[test]
fn italic_toggle_operates_on_marks_not_bold_delimiters() {
    let mut editor = EditorState::from_document(EditorDocument::from_text(
        None,
        DocumentKind::Markdown,
        "**text**",
    ));
    let id = rich_container(&editor);
    select_rich(&mut editor, id, 0, 4);
    editor.apply(EditorCommand::ApplyFormat(FormatCommand::Italic));
    assert_eq!(editor.export_text(), "**_text_**");
    assert_eq!(editor.rich_cursor(), Some(RichPosition::new(id, 4)));
    editor.apply(EditorCommand::InsertText("!".to_owned()));
    assert_eq!(editor.export_text(), "**_text_**\\!");

    editor.apply(EditorCommand::Undo);
    select_rich(&mut editor, id, 0, 4);
    editor.apply(EditorCommand::ApplyFormat(FormatCommand::Italic));
    assert_eq!(editor.export_text(), "**text**");
    editor.apply(EditorCommand::InsertText("!".to_owned()));
    assert_eq!(editor.export_text(), "**text**\\!");
}

#[test]
fn rich_delete_removes_content_without_ever_editing_inline_markers() {
    let mut bold = EditorState::from_document(EditorDocument::from_text(
        None,
        DocumentKind::Markdown,
        "**bold**",
    ));
    let id = rich_container(&bold);
    select_rich(&mut bold, id, 0, 4);
    bold.apply(EditorCommand::DeleteSelection);
    assert_eq!(bold.export_text(), "");

    let mut nested = EditorState::from_document(EditorDocument::from_text(
        None,
        DocumentKind::Markdown,
        "***ab***",
    ));
    let id = rich_container(&nested);
    move_rich(&mut nested, id, 2);
    nested.apply(EditorCommand::Backspace);
    assert_eq!(nested.export_text(), "**_a_**");
    nested.apply(EditorCommand::InsertText("c".to_owned()));
    assert_eq!(nested.export_text(), "**_a_**c");
    let projection = nested.rich_projection().unwrap();
    let ProjectedBlockKind::Paragraph { content } = &projection.blocks[0].kind else {
        panic!("nested inline markup should remain a paragraph");
    };
    assert_eq!(
        content
            .iter()
            .map(|span| span.text.as_str())
            .collect::<String>(),
        "ac"
    );
    assert!(
        content
            .iter()
            .any(|span| { span.text == "a" && span.marks.bold && span.marks.italic })
    );
    assert!(content.iter().all(|span| !span.text.contains('*')));

    select_rich(&mut nested, id, 0, 2);
    nested.apply(EditorCommand::DeleteSelection);
    assert_eq!(nested.export_text(), "");
}

#[test]
fn rich_punctuation_is_plain_text_and_does_not_rebuild_structure() {
    let mut editor =
        EditorState::from_document(EditorDocument::from_text(None, DocumentKind::Markdown, "x"));
    let id = rich_container(&editor);
    move_rich(&mut editor, id, 1);

    let typed = "*****`# ---|";
    for character in typed.chars() {
        editor.apply(EditorCommand::InsertText(character.to_string()));
        let document = editor.rich_document().unwrap();
        assert_eq!(document.blocks.len(), 1);
        assert_eq!(document.blocks[0].id, id);
        assert!(matches!(
            document.blocks[0].kind,
            RichBlockKind::Paragraph { .. }
        ));
        assert_eq!(editor.mode, EditorMode::Rich);
    }

    let projection = editor.rich_projection().unwrap();
    let ProjectedBlockKind::Paragraph { content } = &projection.blocks[0].kind else {
        panic!("punctuation must not create Markdown structure");
    };
    assert_eq!(
        content
            .iter()
            .map(|span| span.text.as_str())
            .collect::<String>(),
        format!("x{typed}")
    );
}

#[test]
fn table_commands_use_stable_table_ids_and_leave_an_editable_paragraph() {
    let mut editor = EditorState::new();
    editor.apply(EditorCommand::ApplyFormat(FormatCommand::Table {
        columns: 2,
        rows: 1,
    }));
    let projection = editor.rich_projection().unwrap();
    let table_id = projection.blocks[0].id;
    let paragraph_id = projection.blocks[1].id;
    let ProjectedBlockKind::Table { header, rows, .. } = &projection.blocks[0].kind else {
        panic!("expected table");
    };
    assert_eq!((header.len(), rows.len()), (2, 1));
    assert!(matches!(
        projection.blocks[1].kind,
        ProjectedBlockKind::Paragraph { .. }
    ));

    move_rich(&mut editor, paragraph_id, 0);
    editor.apply(EditorCommand::InsertText("below".to_owned()));
    assert!(editor.export_text().ends_with("\n\nbelow"));

    editor.apply(EditorCommand::EditTableColumn {
        table_id,
        edge: TableColumnEdge::Left,
        edit: TableColumnEdit::Insert,
    });
    let projection = editor.rich_projection().unwrap();
    assert_eq!(projection.blocks[0].id, table_id);
    let ProjectedBlockKind::Table { header, rows, .. } = &projection.blocks[0].kind else {
        panic!("table id should still address the table");
    };
    assert_eq!(header.len(), 3);
    assert!(rows.iter().all(|row| row.len() == 3));

    editor.apply(EditorCommand::EditTableColumn {
        table_id,
        edge: TableColumnEdge::Left,
        edit: TableColumnEdit::Remove,
    });
    editor.apply(EditorCommand::EditTableColumn {
        table_id,
        edge: TableColumnEdge::Right,
        edit: TableColumnEdit::Insert,
    });
    editor.apply(EditorCommand::EditTableColumn {
        table_id,
        edge: TableColumnEdge::Right,
        edit: TableColumnEdit::Remove,
    });
    let projection = editor.rich_projection().unwrap();
    let ProjectedBlockKind::Table { header, rows, .. } = &projection.blocks[0].kind else {
        panic!("expected table");
    };
    assert_eq!(header.len(), 2);
    assert!(rows.iter().all(|row| row.len() == 2));
}

#[test]
fn block_formats_change_semantic_block_kinds() {
    let mut heading = EditorState::from_document(EditorDocument::from_text(
        None,
        DocumentKind::Markdown,
        "line",
    ));
    heading.apply(EditorCommand::ApplyFormat(FormatCommand::Heading(2)));
    assert!(matches!(
        heading.rich_projection().unwrap().blocks[0].kind,
        ProjectedBlockKind::Heading { level: 2, .. }
    ));
    assert_eq!(heading.export_text(), "## line");
    heading.apply(EditorCommand::ApplyFormat(FormatCommand::Paragraph));
    assert!(matches!(
        heading.rich_projection().unwrap().blocks[0].kind,
        ProjectedBlockKind::Paragraph { .. }
    ));

    let mut list = EditorState::from_document(EditorDocument::from_text(
        None,
        DocumentKind::Markdown,
        "first\nsecond",
    ));
    list.apply(EditorCommand::ApplyFormat(FormatCommand::BulletList));
    let projection = list.rich_projection().unwrap();
    assert!(matches!(
        projection.blocks[0].kind,
        ProjectedBlockKind::List {
            kind: RichListKind::Bullet,
            ..
        }
    ));
}

#[test]
fn quote_and_list_actions_toggle_only_the_cursor_logical_line() {
    let mut quote = EditorState::from_document(EditorDocument::from_text(
        None,
        DocumentKind::Markdown,
        "one\ntwo\nthree",
    ));
    let line_id = quote.rich_projection().unwrap().blocks[0].id;
    move_rich(&mut quote, line_id, 5);
    quote.apply(EditorCommand::ApplyFormat(FormatCommand::Quote));
    let projection = quote.rich_projection().unwrap();
    assert!(matches!(
        projection.blocks.as_slice(),
        [
            app::rich_document::ProjectedBlock {
                kind: ProjectedBlockKind::Paragraph { .. },
                ..
            },
            app::rich_document::ProjectedBlock {
                kind: ProjectedBlockKind::Quote { .. },
                ..
            },
            app::rich_document::ProjectedBlock {
                kind: ProjectedBlockKind::Paragraph { .. },
                ..
            }
        ]
    ));
    assert_eq!(quote.export_text(), "one\n\n> two\n\nthree");
    quote.apply(EditorCommand::ApplyFormat(FormatCommand::Quote));
    assert!(
        quote
            .rich_projection()
            .unwrap()
            .blocks
            .iter()
            .all(|block| matches!(block.kind, ProjectedBlockKind::Paragraph { .. }))
    );

    for (format, expected_kind, expected_markdown) in [
        (
            FormatCommand::BulletList,
            RichListKind::Bullet,
            "one\n\n- two\n\nthree",
        ),
        (
            FormatCommand::OrderedList,
            RichListKind::Ordered,
            "one\n\n1. two\n\nthree",
        ),
    ] {
        let mut list = EditorState::from_document(EditorDocument::from_text(
            None,
            DocumentKind::Markdown,
            "one\ntwo\nthree",
        ));
        let line_id = list.rich_projection().unwrap().blocks[0].id;
        move_rich(&mut list, line_id, 5);
        list.apply(EditorCommand::ApplyFormat(format.clone()));
        let projection = list.rich_projection().unwrap();
        assert!(matches!(
            &projection.blocks[1].kind,
            ProjectedBlockKind::List { kind, items, .. }
                if *kind == expected_kind && items.len() == 1
        ));
        assert!(matches!(
            projection.blocks[0].kind,
            ProjectedBlockKind::Paragraph { .. }
        ));
        assert!(matches!(
            projection.blocks[2].kind,
            ProjectedBlockKind::Paragraph { .. }
        ));
        assert_eq!(list.export_text(), expected_markdown);
        list.apply(EditorCommand::ApplyFormat(format));
        assert!(
            list.rich_projection()
                .unwrap()
                .blocks
                .iter()
                .all(|block| matches!(block.kind, ProjectedBlockKind::Paragraph { .. }))
        );
    }
}

#[test]
fn inline_code_is_clipped_to_the_cursor_logical_line() {
    let mut editor = EditorState::from_document(EditorDocument::from_text(
        None,
        DocumentKind::Markdown,
        "one\ntwo\nthree",
    ));
    let paragraph = editor.rich_projection().unwrap().blocks[0].id;
    move_rich(&mut editor, paragraph, 5);
    editor.apply(EditorCommand::ApplyFormat(FormatCommand::InlineCode));
    assert_eq!(editor.export_text(), "one\n`two`\nthree");

    let mut line_with_break = EditorState::from_document(EditorDocument::from_text(
        None,
        DocumentKind::Markdown,
        "one\ntwo\nthree",
    ));
    let paragraph = line_with_break.rich_projection().unwrap().blocks[0].id;
    select_rich(&mut line_with_break, paragraph, 4, 8);
    line_with_break.apply(EditorCommand::ApplyFormat(FormatCommand::InlineCode));
    assert_eq!(line_with_break.export_text(), "one\n`two`\nthree");

    let mut cross_line = EditorState::from_document(EditorDocument::from_text(
        None,
        DocumentKind::Markdown,
        "one\ntwo\nthree",
    ));
    let paragraph = cross_line.rich_projection().unwrap().blocks[0].id;
    select_rich(&mut cross_line, paragraph, 0, 13);
    cross_line.apply(EditorCommand::ApplyFormat(FormatCommand::InlineCode));
    assert_eq!(cross_line.export_text(), "one\ntwo\n`three`");
}

#[test]
fn block_formats_apply_only_to_the_cursor_logical_line() {
    let mut editor = EditorState::from_document(EditorDocument::from_text(
        None,
        DocumentKind::Markdown,
        "one\ntwo\nthree",
    ));
    let paragraph = editor.rich_projection().unwrap().blocks[0].id;
    move_rich(&mut editor, paragraph, 5);
    editor.apply(EditorCommand::ApplyFormat(FormatCommand::Heading(2)));

    let projection = editor.rich_projection().unwrap();
    assert!(matches!(
        projection.blocks[0].kind,
        ProjectedBlockKind::Paragraph { .. }
    ));
    assert!(matches!(
        projection.blocks[1].kind,
        ProjectedBlockKind::Heading { level: 2, .. }
    ));
    assert!(matches!(
        projection.blocks[2].kind,
        ProjectedBlockKind::Paragraph { .. }
    ));
    assert_eq!(editor.export_text(), "one\n\n## two\n\nthree");
    assert_eq!(editor.history_depth(), (1, 0));

    editor.apply(EditorCommand::ApplyFormat(FormatCommand::Paragraph));
    assert!(
        editor
            .rich_projection()
            .unwrap()
            .blocks
            .iter()
            .all(|block| matches!(block.kind, ProjectedBlockKind::Paragraph { .. }))
    );
    assert_eq!(editor.export_text(), "one\n\ntwo\n\nthree");
}

#[test]
fn block_format_selection_query_requires_a_selected_paragraph_or_heading() {
    let mut paragraph = EditorState::from_document(EditorDocument::from_text(
        None,
        DocumentKind::Markdown,
        "paragraph",
    ));
    assert!(!paragraph.can_apply_block_format_to_selection());
    let paragraph_id = paragraph.rich_projection().unwrap().blocks[0].id;
    select_rich(&mut paragraph, paragraph_id, 0, 1);
    assert!(paragraph.can_apply_block_format_to_selection());
    paragraph.apply(EditorCommand::ApplyFormat(FormatCommand::Heading(1)));
    select_rich(&mut paragraph, paragraph_id, 0, 1);
    assert!(paragraph.can_apply_block_format_to_selection());

    let mut code = EditorState::from_document(EditorDocument::from_text(
        None,
        DocumentKind::Markdown,
        "```\ncode\n```",
    ));
    let code_id = code.rich_projection().unwrap().blocks[0].id;
    select_rich(&mut code, code_id, 0, 1);
    assert!(!code.can_apply_block_format_to_selection());
    code.apply(EditorCommand::ApplyFormat(FormatCommand::Heading(1)));
    assert_eq!(code.history_depth(), (0, 0));
    assert!(matches!(
        code.rich_projection().unwrap().blocks[0].kind,
        ProjectedBlockKind::CodeBlock { .. }
    ));

    let mut table = EditorState::from_document(EditorDocument::from_text(
        None,
        DocumentKind::Markdown,
        "| head |\n| --- |\n| body |",
    ));
    let cell_id = match &table.rich_projection().unwrap().blocks[0].kind {
        ProjectedBlockKind::Table { header, .. } => header[0].id,
        other => panic!("expected table, got {other:?}"),
    };
    select_rich(&mut table, cell_id, 0, 1);
    assert!(!table.can_apply_block_format_to_selection());
    table.apply(EditorCommand::ApplyFormat(FormatCommand::Heading(1)));
    assert_eq!(table.history_depth(), (0, 0));

    paragraph.apply(EditorCommand::ToggleMode);
    assert!(!paragraph.can_apply_block_format_to_selection());
}

#[test]
fn normal_extracts_only_the_cursor_line_from_quotes_and_lists() {
    let mut quote = EditorState::from_document(EditorDocument::from_text(
        None,
        DocumentKind::Markdown,
        "> one\n> two\n> three",
    ));
    let quote_id = match &quote.rich_projection().unwrap().blocks[0].kind {
        ProjectedBlockKind::Quote { blocks } => blocks[0].id,
        other => panic!("expected quote, got {other:?}"),
    };
    move_rich(&mut quote, quote_id, 5);
    quote.apply(EditorCommand::ApplyFormat(FormatCommand::Paragraph));
    let projection = quote.rich_projection().unwrap();
    assert!(matches!(
        &projection.blocks[0].kind,
        ProjectedBlockKind::Quote { .. }
    ));
    assert!(matches!(
        projection.blocks[1].kind,
        ProjectedBlockKind::Paragraph { .. }
    ));
    assert!(matches!(
        projection.blocks[2].kind,
        ProjectedBlockKind::Quote { .. }
    ));
    assert_eq!(quote.export_text(), "> one\n\ntwo\n\n> three");

    let mut list = EditorState::from_document(EditorDocument::from_text(
        None,
        DocumentKind::Markdown,
        "- one\n- two\n- three",
    ));
    let second_id = match &list.rich_projection().unwrap().blocks[0].kind {
        ProjectedBlockKind::List { items, .. } => items[1].blocks[0].id,
        other => panic!("expected list, got {other:?}"),
    };
    move_rich(&mut list, second_id, 1);
    list.apply(EditorCommand::ApplyFormat(FormatCommand::Paragraph));
    let projection = list.rich_projection().unwrap();
    assert!(matches!(
        &projection.blocks[0].kind,
        ProjectedBlockKind::List { items, .. } if items.len() == 1
    ));
    assert!(matches!(
        projection.blocks[1].kind,
        ProjectedBlockKind::Paragraph { .. }
    ));
    assert!(matches!(
        &projection.blocks[2].kind,
        ProjectedBlockKind::List { items, .. } if items.len() == 1
    ));
    assert_eq!(list.export_text(), "- one\n\ntwo\n\n- three");
}

#[test]
fn source_edit_session_collapses_to_one_rich_history_transaction() {
    let original = "# Title\n\nBody";
    let mut editor = EditorState::from_document(EditorDocument::from_text(
        None,
        DocumentKind::Markdown,
        original,
    ));
    let initial_revision = editor.revision();
    assert!(matches!(
        editor.rich_projection().unwrap().blocks[0].kind,
        ProjectedBlockKind::Heading { .. }
    ));

    editor.apply(EditorCommand::ToggleMode);
    assert_eq!(editor.mode, EditorMode::Source);
    assert_eq!(editor.source_buffer().as_deref(), Some(original));
    editor.apply(EditorCommand::MoveTo {
        position: EditorPosition::Source(original.len()),
        extend_selection: false,
    });
    editor.apply(EditorCommand::InsertText(" one".to_owned()));
    editor.apply(EditorCommand::InsertText(" two".to_owned()));
    assert_eq!(editor.history_depth(), (2, 0));

    editor.apply(EditorCommand::ToggleMode);
    assert_eq!(editor.mode, EditorMode::Rich);
    assert_eq!(editor.export_text(), "# Title\n\nBody one two");
    assert_eq!(editor.history_depth(), (1, 0));

    editor.apply(EditorCommand::Undo);
    assert_eq!(editor.mode, EditorMode::Rich);
    assert_eq!(editor.export_text(), original);
    assert_eq!(editor.revision(), initial_revision);
    editor.apply(EditorCommand::Redo);
    assert_eq!(editor.export_text(), "# Title\n\nBody one two");
}

#[test]
fn switching_modes_without_editing_does_not_create_content_history() {
    let mut editor = EditorState::from_document(EditorDocument::from_text(
        None,
        DocumentKind::Markdown,
        "# Title\n\nBody",
    ));
    let original = editor.export_text();
    let revision = editor.revision();
    editor.apply(EditorCommand::ToggleMode);
    editor.apply(EditorCommand::ToggleMode);
    assert_eq!(editor.export_text(), original);
    assert_eq!(editor.revision(), revision);
    assert_eq!(editor.history_depth(), (0, 0));
}

#[test]
fn rich_projection_covers_supported_blocks_and_keeps_unsupported_markdown_opaque() {
    let source = concat!(
        "# Heading **bold**\n\n",
        "Paragraph with *em* and ~~gone~~ and [link](https://example.com).\n\n",
        "> quoted\n\n",
        "```rust\nfn main() {}\n```\n\n",
        "- bullet\n- item\n\n",
        "3. third\n4. fourth\n\n",
        "- [x] done\n- [ ] todo\n\n",
        "| left | right |\n| :--- | ---: |\n| a | b |\n\n",
        "---\n\n",
        "<aside>raw</aside>\n"
    );
    let editor = EditorState::from_document(EditorDocument::from_text(
        None,
        DocumentKind::Markdown,
        source,
    ));
    let projection = editor.rich_projection().unwrap();

    assert!(
        projection
            .blocks
            .iter()
            .any(|block| matches!(block.kind, ProjectedBlockKind::Heading { .. }))
    );
    assert!(
        projection
            .blocks
            .iter()
            .any(|block| matches!(block.kind, ProjectedBlockKind::Paragraph { .. }))
    );
    assert!(
        projection
            .blocks
            .iter()
            .any(|block| matches!(block.kind, ProjectedBlockKind::Quote { .. }))
    );
    assert!(
        projection
            .blocks
            .iter()
            .any(|block| matches!(block.kind, ProjectedBlockKind::CodeBlock { .. }))
    );
    assert!(projection.blocks.iter().any(|block| matches!(
        block.kind,
        ProjectedBlockKind::List {
            kind: RichListKind::Bullet,
            ..
        }
    )));
    assert!(projection.blocks.iter().any(|block| matches!(
        block.kind,
        ProjectedBlockKind::List {
            kind: RichListKind::Ordered,
            start: 3,
            ..
        }
    )));
    assert!(projection.blocks.iter().any(|block| matches!(
        block.kind,
        ProjectedBlockKind::List {
            kind: RichListKind::Task,
            ..
        }
    )));
    assert!(
        projection
            .blocks
            .iter()
            .any(|block| matches!(block.kind, ProjectedBlockKind::Table { .. }))
    );
    assert!(
        projection
            .blocks
            .iter()
            .any(|block| matches!(block.kind, ProjectedBlockKind::Rule))
    );
    assert!(
        projection
            .blocks
            .iter()
            .any(|block| matches!(block.kind, ProjectedBlockKind::OpaqueMarkdown { .. }))
    );
    assert_eq!(editor.export_text(), source);
}

#[test]
fn rich_projection_ranges_are_grapheme_offsets_for_cjk_and_emoji() {
    let editor = EditorState::from_document(EditorDocument::from_text(
        None,
        DocumentKind::Markdown,
        "# 中文🙂 **粗体**\n\n段落 emoji 🧊 与 `代码`。",
    ));
    let projection = editor.rich_projection().unwrap();
    let ProjectedBlockKind::Heading { content, .. } = &projection.blocks[0].kind else {
        panic!("expected heading");
    };
    let bold = content
        .iter()
        .find(|span| span.marks.bold)
        .expect("bold CJK span");
    assert_eq!(bold.text, "粗体");
    assert_eq!(bold.range.start.container_id, bold.range.end.container_id);
    assert_eq!(
        bold.range.end.grapheme_offset - bold.range.start.grapheme_offset,
        2
    );

    let ProjectedBlockKind::Paragraph { content } = &projection.blocks[1].kind else {
        panic!("expected paragraph");
    };
    assert!(content.iter().all(|span| {
        span.range.start.container_id == span.range.end.container_id
            && span.range.start.grapheme_offset <= span.range.end.grapheme_offset
    }));
}

#[test]
fn inline_projection_retains_marks_links_and_image_data() {
    let editor = EditorState::from_document(EditorDocument::from_text(
        None,
        DocumentKind::Markdown,
        "Text **bold** *italic* `code` [site](https://example.com) ![alt](image.png \"Title\").",
    ));
    let projection = editor.rich_projection().unwrap();
    let ProjectedBlockKind::Paragraph { content } = &projection.blocks[0].kind else {
        panic!("expected paragraph");
    };
    assert!(
        content
            .iter()
            .any(|span| span.marks.bold && span.text == "bold")
    );
    assert!(
        content
            .iter()
            .any(|span| span.marks.italic && span.text == "italic")
    );
    assert!(
        content
            .iter()
            .any(|span| span.marks.code && span.text == "code")
    );
    assert!(content.iter().any(|span| {
        span.link
            .as_ref()
            .is_some_and(|link| link.url == "https://example.com")
    }));
    assert!(content.iter().any(|span| {
        span.text == "alt"
            && span.image.as_ref().is_some_and(|image| {
                image.url == "image.png" && image.title.as_deref() == Some("Title")
            })
    }));
}

#[test]
fn cross_block_rich_selection_uses_logical_endpoints() {
    let mut editor = EditorState::from_document(EditorDocument::from_text(
        None,
        DocumentKind::Markdown,
        "one\n\n**two**",
    ));
    let projection = editor.rich_projection().unwrap();
    let first = projection.blocks[0].id;
    let second = projection.blocks[1].id;
    move_rich(&mut editor, first, 1);
    editor.apply(EditorCommand::MoveTo {
        position: EditorPosition::Rich(RichPosition::new(second, 2)),
        extend_selection: true,
    });
    assert_eq!(editor.selected_text().as_deref(), Some("ne\ntw"));
    assert_eq!(
        editor.apply(EditorCommand::Copy),
        vec![EditorEffect::WriteClipboard("ne\ntw".to_owned())]
    );
    editor.apply(EditorCommand::DeleteSelection);
    assert_eq!(editor.export_text(), "o\n\n**o**");
    assert_eq!(editor.mode, EditorMode::Rich);
}

#[test]
fn save_effect_is_revisioned_and_does_not_mark_saved_early() {
    let mut editor = EditorState::new();
    editor.apply(EditorCommand::InsertText("body".to_owned()));
    let revision = editor.revision();
    let effects = editor.apply(EditorCommand::RequestSave);
    let [
        EditorEffect::SaveFilePicker {
            suggested_name,
            snapshot,
        },
    ] = effects.as_slice()
    else {
        panic!("expected one SaveFilePicker effect, got {effects:?}");
    };
    assert_eq!(suggested_name, "Untitled.md");
    assert_eq!(snapshot.revision, revision);
    assert_eq!(snapshot.to_bytes().unwrap(), b"body");
    assert!(editor.is_dirty());
    assert_eq!(editor.saved_revision(), 0);
    assert_eq!(
        editor.apply(EditorCommand::RequestClose),
        vec![EditorEffect::ConfirmClose]
    );
}

#[test]
fn successful_save_of_an_old_revision_leaves_current_revision_dirty() {
    let mut editor = EditorState::from_document(EditorDocument::from_text(
        Some(PathBuf::from("note.md")),
        DocumentKind::Markdown,
        "base",
    ));
    let id = rich_container(&editor);
    move_rich(&mut editor, id, 4);
    editor.apply(EditorCommand::InsertText(" saved".to_owned()));
    let effects = editor.apply(EditorCommand::RequestSave);
    let [EditorEffect::SaveFile { snapshot, .. }] = effects.as_slice() else {
        panic!("expected SaveFile effect");
    };
    let saved = snapshot.clone();
    editor.apply(EditorCommand::InsertText(" newer".to_owned()));
    let current_revision = editor.revision();
    assert_ne!(current_revision, saved.revision);

    editor.apply(EditorCommand::MarkSaved {
        path: Some(PathBuf::from("note.md")),
        revision: saved.revision,
    });
    assert_eq!(editor.saved_revision(), saved.revision);
    assert_eq!(editor.revision(), current_revision);
    assert!(editor.is_dirty());
    assert_eq!(editor.document.source(), "base saved");
    assert_eq!(editor.export_text(), "base saved newer");
}

#[test]
fn save_as_non_markdown_converts_the_session_to_source_after_success() {
    let mut editor = EditorState::new();
    editor.apply(EditorCommand::InsertText("body".to_owned()));
    let cursor = editor.rich_cursor();
    let snapshot = picker_snapshot(editor.apply(EditorCommand::RequestSaveAs));
    assert_eq!(snapshot.to_bytes().unwrap(), b"body");
    assert_eq!(editor.mode, EditorMode::Rich);
    assert_eq!(editor.rich_cursor(), cursor);

    editor.apply(EditorCommand::MarkSaved {
        path: Some(PathBuf::from("note.txt")),
        revision: snapshot.revision,
    });
    assert_eq!(editor.document.kind, DocumentKind::PlainText);
    assert_eq!(editor.document.path, Some(PathBuf::from("note.txt")));
    assert_eq!(editor.mode, EditorMode::Source);
    assert_eq!(editor.source_buffer().as_deref(), Some("body"));
    assert_eq!(editor.position(), Some(EditorPosition::Source(4)));
    assert!(!editor.is_dirty());
}

#[test]
fn explorer_editor_resolver_routes_supported_text_extensions() {
    let resolver = EditorAwareOpenRouteResolver;
    let attributes = FileAttributes {
        path: PathBuf::from("README.MD"),
        is_file: true,
        is_dir: false,
        len: 0,
        readonly: false,
        modified: None,
        hidden: false,
        system: false,
        archive: false,
        symlink: false,
        junction: false,
        reparse_point: false,
        shortcut: false,
    };
    for name in [
        "README.MD",
        "note.markdown",
        "draft.mdown",
        "x.mkd",
        "plain.txt",
        "app.log",
        "APP.LOG",
        "app.log.1",
    ] {
        assert_eq!(
            resolver.route(PathBuf::from(name).as_path(), &attributes),
            ExplorerOpenTarget::Editor
        );
    }
    for name in ["app.log.old", "app.log.gz", "catalog", "photo.png"] {
        assert!(!is_log_document_path(PathBuf::from(name).as_path()));
    }
    for name in ["app.log", "APP.LOG", "app.LoG.1", "app.log.001"] {
        assert!(is_log_document_path(PathBuf::from(name).as_path()));
    }
    for name in ["photo.png", "app.log.old", "app.log.gz", "catalog"] {
        assert_eq!(
            resolver.route(PathBuf::from(name).as_path(), &attributes),
            ExplorerOpenTarget::SystemDefault
        );
    }
}
