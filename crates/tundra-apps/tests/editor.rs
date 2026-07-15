use std::path::PathBuf;

use tundra_apps::editor::{
    CursorMove, DocumentKind, EditorCommand, EditorDocument, EditorEffect, EditorMode, EditorState,
    FormatCommand, InlineStyle, LineEnding, RenderBlock, Selection,
};
use tundra_apps::explorer::{
    EditorAwareOpenRouteResolver, ExplorerOpenRouteResolver, ExplorerOpenTarget,
};
use tundra_platform::FileAttributes;

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
}

#[test]
fn invalid_utf8_is_rejected_without_lossy_decoding() {
    let error = EditorDocument::from_bytes(None, DocumentKind::PlainText, &[b'a', 0xff])
        .expect_err("invalid UTF-8 must be rejected");
    assert_eq!(error.valid_up_to, 1);
}

#[test]
fn unicode_graphemes_are_inserted_moved_and_deleted_atomically() {
    let mut editor = EditorState::new();
    editor.apply(EditorCommand::InsertText("A好e\u{301}🙂".to_owned()));
    assert_eq!(editor.document.source(), "A好e\u{301}🙂");

    editor.apply(EditorCommand::MoveCursor {
        movement: CursorMove::Left,
        extend_selection: false,
    });
    editor.apply(EditorCommand::Backspace);
    assert_eq!(editor.document.source(), "A好🙂");
    assert_eq!(editor.cursor_line_column(), (0, 2));

    editor.apply(EditorCommand::Backspace);
    assert_eq!(editor.document.source(), "A🙂");
    assert!(editor.is_dirty());
}

#[test]
fn crlf_is_one_cursor_step_and_one_delete_unit() {
    let document = EditorDocument::from_text(None, DocumentKind::PlainText, "a\r\nb");
    let mut editor = EditorState::from_document(document);
    editor.apply(EditorCommand::MoveTo {
        byte_offset: 3,
        extend_selection: false,
    });
    editor.apply(EditorCommand::Backspace);
    assert_eq!(editor.document.source(), "ab");
    assert_eq!(editor.cursor.byte_offset, 1);
}

#[test]
fn selection_copy_cut_paste_and_undo_share_one_source() {
    let mut editor = EditorState::new();
    editor.apply(EditorCommand::InsertText("alpha beta".to_owned()));
    editor.selection = Some(Selection::new(0, 5));
    editor.cursor.byte_offset = 5;

    assert_eq!(
        editor.apply(EditorCommand::Copy),
        vec![EditorEffect::WriteClipboard("alpha".to_owned())]
    );
    assert_eq!(
        editor.apply(EditorCommand::Cut),
        vec![EditorEffect::WriteClipboard("alpha".to_owned())]
    );
    assert_eq!(editor.document.source(), " beta");
    editor.apply(EditorCommand::Paste("ALPHA".to_owned()));
    assert_eq!(editor.document.source(), "ALPHA beta");

    editor.apply(EditorCommand::Undo);
    assert_eq!(editor.document.source(), " beta");
    editor.apply(EditorCommand::Undo);
    assert_eq!(editor.document.source(), "alpha beta");
    editor.apply(EditorCommand::Redo);
    assert_eq!(editor.document.source(), " beta");
}

#[test]
fn saved_checkpoint_drives_dirty_state_even_across_undo() {
    let mut editor = EditorState::new();
    editor.apply(EditorCommand::InsertText("saved".to_owned()));
    editor.apply(EditorCommand::MarkSaved {
        path: Some(PathBuf::from("note.md")),
    });
    assert!(!editor.is_dirty());

    editor.apply(EditorCommand::InsertText(" change".to_owned()));
    assert!(editor.is_dirty());
    editor.apply(EditorCommand::Undo);
    assert!(!editor.is_dirty());
    editor.apply(EditorCommand::Undo);
    assert!(editor.is_dirty());
}

#[test]
fn inline_format_wraps_only_the_selection_and_toggles_off() {
    let original = "before selected after";
    let mut editor = EditorState::from_document(EditorDocument::from_text(
        None,
        DocumentKind::Markdown,
        original,
    ));
    editor.selection = Some(Selection::new(7, 15));
    editor.cursor.byte_offset = 15;
    editor.apply(EditorCommand::ApplyFormat(FormatCommand::Bold));
    assert_eq!(editor.document.source(), "before **selected** after");
    assert_eq!(editor.selected_text(), Some("selected"));

    editor.apply(EditorCommand::ApplyFormat(FormatCommand::Bold));
    assert_eq!(editor.document.source(), original);
    assert_eq!(editor.selected_text(), Some("selected"));
    editor.apply(EditorCommand::Undo);
    assert_eq!(editor.document.source(), "before **selected** after");
}

#[test]
fn italic_toggle_does_not_mistake_bold_markers_for_emphasis() {
    let mut editor = EditorState::from_document(EditorDocument::from_text(
        None,
        DocumentKind::Markdown,
        "**text**",
    ));
    editor.selection = Some(Selection::new(2, 6));
    editor.cursor.byte_offset = 6;
    editor.apply(EditorCommand::ApplyFormat(FormatCommand::Italic));
    assert_eq!(editor.document.source(), "***text***");
    editor.apply(EditorCommand::ApplyFormat(FormatCommand::Italic));
    assert_eq!(editor.document.source(), "**text**");
}

#[test]
fn block_formats_change_only_line_prefixes() {
    let mut editor = EditorState::from_document(EditorDocument::from_text(
        None,
        DocumentKind::Markdown,
        "first\nsecond\ntail",
    ));
    editor.selection = Some(Selection::new(0, 12));
    editor.cursor.byte_offset = 12;
    editor.apply(EditorCommand::ApplyFormat(FormatCommand::BulletList));
    assert_eq!(editor.document.source(), "- first\n- second\ntail");
    assert!(editor.document.source().ends_with("tail"));

    editor.apply(EditorCommand::ApplyFormat(FormatCommand::BulletList));
    assert_eq!(editor.document.source(), "first\nsecond\ntail");
    editor.apply(EditorCommand::ApplyFormat(FormatCommand::Heading(2)));
    assert_eq!(editor.document.source(), "first\n## second\ntail");
}

#[test]
fn mode_switch_rebuilds_projection_without_touching_source() {
    let mut editor = EditorState::from_document(EditorDocument::from_text(
        None,
        DocumentKind::Markdown,
        "# Title\n\nBody",
    ));
    let original = editor.document.source().to_owned();
    assert!(matches!(
        editor.render_blocks()[0],
        RenderBlock::Heading { .. }
    ));

    editor.apply(EditorCommand::ToggleMode);
    assert_eq!(editor.mode, EditorMode::Source);
    assert!(matches!(
        editor.render_blocks()[0],
        RenderBlock::PlainText { .. }
    ));
    editor.apply(EditorCommand::ToggleMode);
    assert_eq!(editor.document.source(), original);
}

#[test]
fn markdown_projection_covers_terminal_block_types_and_valid_ranges() {
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
        "<aside>raw</aside>\n\n",
        "![alt](image.png \"Title\")\n\n",
        "note[^a]\n\n[^a]: footnote\n"
    );
    let editor = EditorState::from_document(EditorDocument::from_text(
        None,
        DocumentKind::Markdown,
        source,
    ));
    let blocks = editor.render_blocks();

    assert!(
        blocks
            .iter()
            .any(|block| matches!(block, RenderBlock::Heading { .. }))
    );
    assert!(
        blocks
            .iter()
            .any(|block| matches!(block, RenderBlock::Paragraph { .. }))
    );
    assert!(
        blocks
            .iter()
            .any(|block| matches!(block, RenderBlock::Quote { .. }))
    );
    assert!(
        blocks
            .iter()
            .any(|block| matches!(block, RenderBlock::CodeBlock { .. }))
    );
    assert!(
        blocks
            .iter()
            .any(|block| matches!(block, RenderBlock::BulletList { .. }))
    );
    assert!(
        blocks
            .iter()
            .any(|block| matches!(block, RenderBlock::OrderedList { start: 3, .. }))
    );
    assert!(
        blocks
            .iter()
            .any(|block| matches!(block, RenderBlock::TaskList { .. }))
    );
    assert!(
        blocks
            .iter()
            .any(|block| matches!(block, RenderBlock::Table { .. }))
    );
    assert!(
        blocks
            .iter()
            .any(|block| matches!(block, RenderBlock::Rule { .. }))
    );
    assert!(
        blocks
            .iter()
            .any(|block| matches!(block, RenderBlock::RawHtml { .. }))
    );
    assert!(
        blocks
            .iter()
            .any(|block| matches!(block, RenderBlock::Image { .. }))
    );
    assert!(
        blocks
            .iter()
            .any(|block| matches!(block, RenderBlock::FootnoteDefinition { .. }))
    );

    for block in &blocks {
        let range = block.source_range();
        assert!(range.start <= range.end, "invalid range for {block:?}");
        assert!(
            range.end <= source.len(),
            "out-of-bounds range for {block:?}"
        );
        assert!(source.is_char_boundary(range.start));
        assert!(source.is_char_boundary(range.end));
    }
}

#[test]
fn markdown_source_ranges_are_utf8_boundaries_for_cjk_and_emoji() {
    let source = "# 中文🙂 **粗体**\n\n段落 emoji 🧊 与 `代码`。";
    let editor = EditorState::from_document(EditorDocument::from_text(
        None,
        DocumentKind::Markdown,
        source,
    ));
    let blocks = editor.render_blocks();
    let RenderBlock::Heading {
        content,
        source_range,
        ..
    } = &blocks[0]
    else {
        panic!("expected heading");
    };
    assert_eq!(
        &source[source_range.start..source_range.end],
        "# 中文🙂 **粗体**"
    );
    for span in content {
        assert!(source.is_char_boundary(span.source_range.start));
        assert!(source.is_char_boundary(span.source_range.end));
    }
    let bold = content
        .iter()
        .find(|span| span.styles.contains(&InlineStyle::Bold))
        .expect("bold CJK span");
    assert_eq!(bold.text, "粗体");
    assert_eq!(
        &source[bold.source_range.start..bold.source_range.end],
        "粗体"
    );

    let paragraph_range = blocks[1].source_range();
    assert!(source.is_char_boundary(paragraph_range.start));
    assert!(source.is_char_boundary(paragraph_range.end));
    assert_eq!(
        &source[paragraph_range.start..paragraph_range.end],
        "段落 emoji 🧊 与 `代码`。"
    );
}

#[test]
fn inline_projection_retains_styles_links_and_image_fallback_data() {
    let editor = EditorState::from_document(EditorDocument::from_text(
        None,
        DocumentKind::Markdown,
        "Text **bold** *italic* `code` [site](https://example.com).",
    ));
    let RenderBlock::Paragraph { content, .. } = &editor.render_blocks()[0] else {
        panic!("expected paragraph");
    };
    assert!(
        content
            .iter()
            .any(|span| span.styles.contains(&InlineStyle::Bold) && span.text == "bold")
    );
    assert!(
        content
            .iter()
            .any(|span| span.styles.contains(&InlineStyle::Italic) && span.text == "italic")
    );
    assert!(
        content
            .iter()
            .any(|span| span.styles.contains(&InlineStyle::Code) && span.text == "code")
    );
    assert!(content.iter().any(|span| {
        span.styles.contains(&InlineStyle::Link)
            && span.target.as_deref() == Some("https://example.com")
    }));
}

#[test]
fn file_commands_are_effects_and_do_not_mark_saved_early() {
    let mut editor = EditorState::new();
    editor.apply(EditorCommand::InsertText("body".to_owned()));
    assert_eq!(
        editor.apply(EditorCommand::RequestSave),
        vec![EditorEffect::SaveFilePicker {
            suggested_name: "Untitled.md".to_owned(),
            contents: b"body".to_vec(),
        }]
    );
    assert!(editor.is_dirty());
    assert_eq!(
        editor.apply(EditorCommand::RequestClose),
        vec![EditorEffect::ConfirmClose]
    );
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
