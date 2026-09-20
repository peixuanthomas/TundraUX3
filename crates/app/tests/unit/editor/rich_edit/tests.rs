use super::*;
use crate::editor::markdown_codec::{
    MarkdownCodec, parse_count_for_tests, reset_parse_count_for_tests,
};

fn assert_single_paragraph(editor: &RichEditor) {
    let projection = editor.projection();
    assert_eq!(projection.blocks.len(), 1);
    assert!(matches!(
        projection.blocks[0].kind,
        ProjectedBlockKind::Paragraph { .. }
    ));
}

#[test]
fn formatting_and_navigation_never_create_markdown_delimiters() {
    let imported = MarkdownCodec::import("text").unwrap();
    let mut editor = RichEditor::new(imported.document);
    let id = editor.cursor.unwrap().container_id;
    editor.selection = Some(RichSelection::new(
        RichPosition::new(id, 0),
        RichPosition::new(id, 4),
    ));
    assert!(editor.apply_format(&FormatCommand::Bold));
    editor.move_cursor(CursorMove::Left, false);
    editor.backspace();

    assert_eq!(editor.plain_text(), "tet");
    let projection = editor.projection();
    let ProjectedBlockKind::Paragraph { content } = &projection.blocks[0].kind else {
        panic!("paragraph")
    };
    assert!(content.iter().all(|span| !span.text.contains("**")));
    assert!(content.iter().any(|span| span.marks.bold));
}

#[test]
fn markdown_punctuation_is_plain_text_until_export() {
    let mut editor = RichEditor::new(RichDocument::new());
    editor.insert_text("**** # | ` literal");
    assert_eq!(editor.plain_text(), "**** # | ` literal");
    assert!(matches!(
        editor.projection().blocks[0].kind,
        ProjectedBlockKind::Paragraph { .. }
    ));
    let markdown = MarkdownCodec::export(&editor.document).unwrap().markdown;
    assert!(markdown.contains("\\*\\*\\*\\*"));
}

#[test]
fn unicode_positions_count_graphemes() {
    let mut editor = RichEditor::new(RichDocument::new());
    editor.insert_text("A👨‍👩‍👧‍👦e\u{301}🇨🇳好");
    assert_eq!(editor.cursor.unwrap().grapheme_offset, 5);
    editor.backspace();
    assert_eq!(editor.plain_text(), "A👨‍👩‍👧‍👦e\u{301}🇨🇳");
    editor.backspace();
    assert_eq!(editor.plain_text(), "A👨‍👩‍👧‍👦e\u{301}");
    editor.backspace();
    assert_eq!(editor.plain_text(), "A👨‍👩‍👧‍👦");
}

#[test]
fn nested_marks_and_deleting_the_last_marked_grapheme_stay_semantic() {
    let imported = MarkdownCodec::import("abcdef").unwrap();
    let mut editor = RichEditor::new(imported.document);
    let id = editor.cursor.unwrap().container_id;

    editor.selection = Some(RichSelection::new(
        RichPosition::new(id, 0),
        RichPosition::new(id, 6),
    ));
    assert!(editor.apply_format(&FormatCommand::Bold));
    editor.selection = Some(RichSelection::new(
        RichPosition::new(id, 1),
        RichPosition::new(id, 5),
    ));
    assert!(editor.apply_format(&FormatCommand::Italic));
    editor.selection = Some(RichSelection::new(
        RichPosition::new(id, 2),
        RichPosition::new(id, 4),
    ));
    assert!(editor.apply_format(&FormatCommand::Strikethrough));

    let projection = editor.projection();
    let ProjectedBlockKind::Paragraph { content } = &projection.blocks[0].kind else {
        panic!("paragraph")
    };
    assert_eq!(
        content
            .iter()
            .map(|span| span.text.as_str())
            .collect::<String>(),
        "abcdef"
    );
    assert!(content.iter().all(|span| span.marks.bold));
    assert!(
        content
            .iter()
            .any(|span| span.marks.bold && span.marks.italic)
    );
    assert!(
        content
            .iter()
            .any(|span| { span.marks.bold && span.marks.italic && span.marks.strikethrough })
    );
    assert!(
        content
            .iter()
            .all(|span| { !span.text.contains("**") && !span.text.contains("~~") })
    );

    // Remove the final grapheme of the innermost formatted range. No
    // delimiter can be exposed because delimiters do not exist in memory.
    editor.selection = Some(RichSelection::new(
        RichPosition::new(id, 3),
        RichPosition::new(id, 4),
    ));
    assert!(editor.delete_selection());
    assert_eq!(editor.plain_text(), "abcef");
    assert_single_paragraph(&editor);
    assert!(editor.projection().blocks.iter().all(|block| {
        match &block.kind {
            ProjectedBlockKind::Paragraph { content } => content
                .iter()
                .all(|span| !span.text.contains("**") && !span.text.contains("~~")),
            _ => false,
        }
    }));
}

#[test]
fn cross_container_edits_preserve_opaque_nodes_and_reject_opaque_positions() {
    let source = "alpha\n\n<section data-x=\"keep\">raw</section>\n\nomega";
    let imported = MarkdownCodec::import(source).unwrap();
    let mut editor = RichEditor::new(imported.document);
    assert_eq!(editor.document.blocks.len(), 3);
    let first_id = editor.document.blocks[0].id;
    let opaque_id = editor.document.blocks[1].id;
    let second_id = editor.document.blocks[2].id;
    assert!(matches!(
        &editor.document.blocks[1].kind,
        RichBlockKind::OpaqueMarkdown { .. }
    ));

    let original_cursor = editor.cursor;
    assert!(!editor.move_to(RichPosition::new(opaque_id, 0), false));
    assert_eq!(editor.cursor, original_cursor);

    editor.selection = Some(RichSelection::new(
        RichPosition::new(first_id, 2),
        RichPosition::new(second_id, 3),
    ));
    assert!(editor.apply_format(&FormatCommand::Bold));
    assert!(matches!(
        &editor.document.blocks[1].kind,
        RichBlockKind::OpaqueMarkdown { raw, .. }
            if raw == "<section data-x=\"keep\">raw</section>"
    ));

    editor.selection = Some(RichSelection::new(
        RichPosition::new(first_id, 4),
        RichPosition::new(second_id, 1),
    ));
    assert!(editor.delete_selection());
    assert_eq!(container_text(&editor.document, first_id), "alph");
    assert_eq!(container_text(&editor.document, second_id), "mega");
    assert!(matches!(
        &editor.document.blocks[1].kind,
        RichBlockKind::OpaqueMarkdown { raw, .. }
            if raw == "<section data-x=\"keep\">raw</section>"
    ));
}

#[test]
fn ordinary_rich_edits_and_render_projection_parse_markdown_zero_times() {
    let imported = MarkdownCodec::import("editable text").unwrap();
    let mut editor = RichEditor::new(imported.document);
    reset_parse_count_for_tests();

    let id = editor.cursor.unwrap().container_id;
    assert!(editor.move_to(RichPosition::new(id, 13), false));
    for key in ["*", "#", "`", "|", " ", "👨‍👩‍👧‍👦"] {
        assert!(editor.insert_text(key));
        let _ = editor.projection();
        let _ = editor.plain_text();
    }
    editor.selection = Some(RichSelection::new(
        RichPosition::new(id, 0),
        RichPosition::new(id, 8),
    ));
    assert!(editor.apply_format(&FormatCommand::Bold));
    editor.move_cursor(CursorMove::Left, false);
    assert!(editor.backspace());
    let _ = MarkdownCodec::export(&editor.document).unwrap();

    assert_eq!(parse_count_for_tests(), 0);
    let _ = MarkdownCodec::import("explicit boundary").unwrap();
    assert_eq!(parse_count_for_tests(), 1);
}
