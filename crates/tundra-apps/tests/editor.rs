use std::path::PathBuf;

use tundra_apps::editor::{
    CursorMove, DocumentKind, EditorCommand, EditorDocument, EditorEffect, EditorMode,
    EditorPosition, EditorState, FormatCommand, LineEnding, SaveSnapshot, Selection,
    TableColumnEdge, TableColumnEdit,
};
use tundra_apps::explorer::{
    EditorAwareOpenRouteResolver, ExplorerOpenRouteResolver, ExplorerOpenTarget,
};
use tundra_apps::rich_document::{
    NodeId, ProjectedBlockKind, RichBlockKind, RichListKind, RichPosition,
};
use tundra_platform::FileAttributes;

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
    assert_eq!(snapshot.bytes, bytes);
}

#[test]
fn invalid_utf8_is_rejected_without_lossy_decoding() {
    let error = EditorDocument::from_bytes(None, DocumentKind::PlainText, &[b'a', 0xff])
        .expect_err("invalid UTF-8 must be rejected");
    assert_eq!(error.valid_up_to, 1);
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
    assert_eq!(editor.source_buffer(), Some("ab"));
    assert_eq!(editor.position(), Some(EditorPosition::Source(1)));
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
    assert_eq!(
        editor.apply(EditorCommand::RequestSave),
        vec![EditorEffect::SaveFile {
            path: PathBuf::from("note.md"),
            snapshot: SaveSnapshot {
                revision,
                bytes: b"original rich".to_vec(),
            },
        }]
    );
    assert_eq!(editor.document.source(), "original");

    editor.apply(EditorCommand::SetMode(EditorMode::Source));
    assert!(!editor.has_isolated_rich_buffer());
    assert_eq!(editor.source_buffer(), Some("original rich"));
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
    assert_eq!(source.source_buffer(), Some("plain source"));
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
    assert_eq!(editor.source_buffer(), Some(original));
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
    assert_eq!(
        editor.apply(EditorCommand::RequestSave),
        vec![EditorEffect::SaveFilePicker {
            suggested_name: "Untitled.md".to_owned(),
            snapshot: SaveSnapshot {
                revision,
                bytes: b"body".to_vec(),
            },
        }]
    );
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
    assert_eq!(snapshot.bytes, b"body");
    assert_eq!(editor.mode, EditorMode::Rich);
    assert_eq!(editor.rich_cursor(), cursor);

    editor.apply(EditorCommand::MarkSaved {
        path: Some(PathBuf::from("note.txt")),
        revision: snapshot.revision,
    });
    assert_eq!(editor.document.kind, DocumentKind::PlainText);
    assert_eq!(editor.document.path, Some(PathBuf::from("note.txt")));
    assert_eq!(editor.mode, EditorMode::Source);
    assert_eq!(editor.source_buffer(), Some("body"));
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
    ] {
        assert_eq!(
            resolver.route(PathBuf::from(name).as_path(), &attributes),
            ExplorerOpenTarget::Editor
        );
    }
    assert_eq!(
        resolver.route(PathBuf::from("photo.png").as_path(), &attributes),
        ExplorerOpenTarget::SystemDefault
    );
}
