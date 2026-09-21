use super::*;

fn assert_dense_positions(
    map: &MarkdownPositionMap,
    container_id: NodeId,
    grapheme_len: usize,
    source: &str,
) {
    for grapheme_offset in 0..=grapheme_len {
        let rich = RichPosition::new(container_id, grapheme_offset);
        let entry = map
            .entries
            .iter()
            .find(|entry| entry.rich == rich)
            .unwrap_or_else(|| panic!("missing mapping for grapheme {grapheme_offset}"));
        assert!(
            source.is_char_boundary(entry.source_offset),
            "source offset {} is not a UTF-8 boundary",
            entry.source_offset
        );
    }
}

#[test]
fn unedited_import_exports_byte_for_byte() {
    let source = "# Title\r\n\r\nText with **bold**.\n\n<aside>keep</aside>\n";
    let imported = MarkdownCodec::import(source).unwrap();
    let exported = MarkdownCodec::export(&imported.document).unwrap();
    assert_eq!(exported.markdown, source);
    assert!(matches!(
        imported.document.blocks.last().map(|block| &block.kind),
        Some(RichBlockKind::OpaqueMarkdown { .. })
    ));
}

#[test]
fn dirty_block_is_serialized_from_semantics_while_other_blocks_stay_raw() {
    let source = "__old spelling__\n\n<custom untouched>\n";
    let mut imported = MarkdownCodec::import(source).unwrap();
    let first = &mut imported.document.blocks[0];
    first.kind = RichBlockKind::Paragraph {
        content: InlineContent(vec![InlineNode::Text(RichText {
            text: "changed * literally".to_owned(),
            marks: InlineMarks {
                bold: true,
                ..InlineMarks::default()
            },
            link: None,
        })]),
    };
    first.rewrite = RewriteState::Dirty;
    let exported = MarkdownCodec::export(&imported.document).unwrap();
    assert!(exported.markdown.starts_with("**changed \\* literally**"));
    assert!(exported.markdown.ends_with("<custom untouched>\n"));
}

#[test]
fn supported_export_reimports_with_the_same_semantics() {
    let mut document = RichDocument::new();
    let id = document.allocate_node_id();
    document.blocks.push(RichBlock::new(
        id,
        RichBlockKind::Heading {
            level: 2,
            content: InlineContent(vec![InlineNode::Text(RichText {
                text: "Bold and italic".to_owned(),
                marks: InlineMarks {
                    bold: true,
                    italic: true,
                    ..InlineMarks::default()
                },
                link: None,
            })]),
        },
    ));
    let markdown = MarkdownCodec::export(&document).unwrap().markdown;
    let reparsed = MarkdownCodec::import(&markdown).unwrap();
    let RichBlockKind::Heading { content, .. } = &reparsed.document.blocks[0].kind else {
        panic!("expected heading")
    };
    let InlineNode::Text(text) = &content.0[0] else {
        panic!("expected text")
    };
    assert!(text.marks.bold);
    assert!(text.marks.italic);
}

#[test]
fn unedited_mixed_line_endings_and_bom_are_byte_identical() {
    let source = "# Title\r\n\r\nfirst\n\nsecond\r";
    let imported = MarkdownCodec::import_with_metadata(source, true, RichLineEnding::CrLf).unwrap();
    let exported = MarkdownCodec::export(&imported.document).unwrap();
    let mut expected = b"\xEF\xBB\xBF".to_vec();
    expected.extend_from_slice(source.as_bytes());

    assert_eq!(exported.to_bytes(imported.document.utf8_bom), expected);
}

#[test]
fn changing_one_block_preserves_front_matter_html_footnotes_and_definitions() {
    let source = concat!(
        "---\n",
        "title: \"Tundra\"\n",
        "tags: [editor, markdown]\n",
        "---\n\n",
        "<section data-note=\"keep | exactly\">\n",
        "  <b>raw html</b>\n",
        "</section>\n\n",
        "Paragraph _using its original spelling_.\n\n",
        "[^note]: Footnote with **raw markers**.\n\n",
        "[reference]: https://example.com/a_(b) \"Raw title\"\n",
    );
    let mut imported = MarkdownCodec::import(source).unwrap();
    let target = imported
        .document
        .blocks
        .iter_mut()
        .find(|block| match &block.kind {
            RichBlockKind::Paragraph { content } => content.plain_text().starts_with("Paragraph "),
            _ => false,
        })
        .expect("editable paragraph in preservation corpus");
    target.kind = RichBlockKind::Paragraph {
        content: InlineContent::plain("Only this paragraph changed."),
    };
    target.rewrite = RewriteState::Dirty;

    let exported = MarkdownCodec::export(&imported.document).unwrap();
    let expected = source.replace(
        "Paragraph _using its original spelling_.",
        "Only this paragraph changed\\.",
    );
    assert_eq!(exported.markdown, expected);
}

#[test]
fn table_pipes_code_backticks_and_link_image_escapes_reimport_semantically() {
    let mut document = RichDocument::new();
    let table_id = document.allocate_node_id();
    let header_id = document.allocate_node_id();
    let body_id = document.allocate_node_id();
    document.blocks.push(RichBlock::new(
        table_id,
        RichBlockKind::Table {
            alignments: vec![RichTableAlignment::None],
            header: vec![RichTableCell {
                id: header_id,
                content: InlineContent::plain("head|pipe"),
            }],
            rows: vec![vec![RichTableCell {
                id: body_id,
                content: InlineContent::plain("body|pipe"),
            }]],
        },
    ));
    let paragraph_id = document.allocate_node_id();
    document.blocks.push(RichBlock::new(
        paragraph_id,
        RichBlockKind::Paragraph {
            content: InlineContent(vec![
                InlineNode::Text(RichText {
                    text: "a `tick` and ``pair``".to_owned(),
                    marks: InlineMarks {
                        code: true,
                        ..InlineMarks::default()
                    },
                    link: None,
                }),
                InlineNode::Text(RichText {
                    text: " linked]label".to_owned(),
                    marks: InlineMarks::default(),
                    link: Some(LinkAttributes {
                        url: "https://example.com/a_(b)".to_owned(),
                        title: Some("a \"quoted\" title".to_owned()),
                    }),
                }),
                InlineNode::Image {
                    alt: "image]alt|pipe".to_owned(),
                    url: "https://example.com/image_(1).png".to_owned(),
                    title: Some("image \"title\"".to_owned()),
                },
            ]),
        },
    ));

    let exported = MarkdownCodec::export(&document).unwrap();
    assert!(exported.markdown.contains("head\\|pipe"));
    assert!(!exported.markdown.contains("head\\\\|pipe"));
    let reparsed = MarkdownCodec::import(&exported.markdown).unwrap();

    let RichBlockKind::Table { header, rows, .. } = &reparsed.document.blocks[0].kind else {
        panic!("expected table")
    };
    assert_eq!(header[0].content.plain_text(), "head|pipe");
    assert_eq!(rows[0][0].content.plain_text(), "body|pipe");
    let RichBlockKind::Paragraph { content } = &reparsed.document.blocks[1].kind else {
        panic!("expected paragraph")
    };
    assert_eq!(
        content.plain_text(),
        "a `tick` and ``pair`` linked]labelimage]alt|pipe"
    );
    let code = content
        .0
        .iter()
        .find_map(|node| match node {
            InlineNode::Text(text) if text.marks.code => Some(text),
            _ => None,
        })
        .expect("code span");
    assert_eq!(code.text, "a `tick` and ``pair``");
    let linked = content
        .0
        .iter()
        .find_map(|node| match node {
            InlineNode::Text(text) if text.link.is_some() => Some(text),
            _ => None,
        })
        .expect("linked text");
    assert_eq!(
        linked.link.as_ref().unwrap().url,
        "https://example.com/a_(b)"
    );
    assert!(content.0.iter().any(|node| matches!(
        node,
        InlineNode::Image { alt, url, .. }
            if alt == "image]alt|pipe" && url == "https://example.com/image_(1).png"
    )));
}

#[test]
fn formatted_unicode_maps_every_grapheme_boundary_on_import_and_export() {
    let source = "# A **好👨‍👩‍👧‍👦e\u{301}** Z\n";
    let imported = MarkdownCodec::import(source).unwrap();
    let block = &imported.document.blocks[0];
    let RichBlockKind::Heading { content, .. } = &block.kind else {
        panic!("heading")
    };
    assert_dense_positions(
        &imported.positions,
        block.id,
        content.grapheme_len(),
        source,
    );

    let logical_before_unicode = "A ".graphemes(true).count();
    let good_start = source.find('好').unwrap();
    assert_eq!(
        imported
            .positions
            .source_offset_for(RichPosition::new(block.id, logical_before_unicode)),
        Some(good_start)
    );
    assert_eq!(
        imported
            .positions
            .source_offset_for(RichPosition::new(block.id, logical_before_unicode + 1)),
        Some(good_start + '好'.len_utf8())
    );

    let exported = MarkdownCodec::export(&imported.document).unwrap();
    assert_eq!(exported.markdown, source);
    assert_dense_positions(
        &exported.positions,
        block.id,
        content.grapheme_len(),
        &exported.markdown,
    );
    assert_eq!(
        exported
            .positions
            .rich_position_for(good_start + '好'.len_utf8()),
        Some(RichPosition::new(block.id, logical_before_unicode + 1))
    );
}

#[test]
fn quote_list_and_code_descendants_have_dense_position_maps() {
    let source = concat!(
        "> 引用 **好**\n",
        "\n",
        "- item 👩‍👩‍👧‍👦\n",
        "\n",
        "```rs\n",
        "let 名 = 1;\n",
        "```\n",
    );
    let imported = MarkdownCodec::import(source).unwrap();

    let RichBlockKind::Quote { blocks } = &imported.document.blocks[0].kind else {
        panic!("quote")
    };
    let quote_paragraph = &blocks[0];
    let RichBlockKind::Paragraph {
        content: quote_content,
    } = &quote_paragraph.kind
    else {
        panic!("quote paragraph")
    };

    let RichBlockKind::List { items, .. } = &imported.document.blocks[1].kind else {
        panic!("list")
    };
    let list_paragraph = &items[0].blocks[0];
    let RichBlockKind::Paragraph {
        content: list_content,
    } = &list_paragraph.kind
    else {
        panic!("list paragraph")
    };

    let code_block = &imported.document.blocks[2];
    let RichBlockKind::CodeBlock { code, .. } = &code_block.kind else {
        panic!("code block")
    };

    let containers = [
        (quote_paragraph.id, quote_content.grapheme_len()),
        (list_paragraph.id, list_content.grapheme_len()),
        (code_block.id, code.graphemes(true).count()),
    ];
    for (id, length) in containers {
        assert_dense_positions(&imported.positions, id, length, source);
    }

    let exported = MarkdownCodec::export(&imported.document).unwrap();
    assert_eq!(exported.markdown, source);
    for (id, length) in containers {
        assert_dense_positions(&exported.positions, id, length, &exported.markdown);
    }

    let mut dirty_document = imported.document.clone();
    for block in &mut dirty_document.blocks {
        block.rewrite = RewriteState::Dirty;
    }
    let normalized = MarkdownCodec::export(&dirty_document).unwrap();
    for (id, length) in containers {
        assert_dense_positions(&normalized.positions, id, length, &normalized.markdown);
    }
}

#[test]
fn table_cells_map_unicode_and_escaped_pipe_boundaries() {
    let source = concat!(
        "| 名字 | 值\\|pipe |\n",
        "| --- | --- |\n",
        "| e\u{301} | 👨‍👩‍👧‍👦 |\n",
    );
    let imported = MarkdownCodec::import(source).unwrap();
    let RichBlockKind::Table { header, rows, .. } = &imported.document.blocks[0].kind else {
        panic!("table")
    };
    let cells = header
        .iter()
        .chain(rows.iter().flatten())
        .collect::<Vec<_>>();
    for cell in &cells {
        assert_dense_positions(
            &imported.positions,
            cell.id,
            cell.content.grapheme_len(),
            source,
        );
    }

    let escaped_pipe_cell = &header[1];
    let pipe_logical = "值".graphemes(true).count();
    // The caret before a literal pipe belongs before the complete escape
    // sequence, not between `\\` and `|`.
    let pipe_source = source.find("\\|").unwrap();
    assert_eq!(
        imported
            .positions
            .source_offset_for(RichPosition::new(escaped_pipe_cell.id, pipe_logical)),
        Some(pipe_source)
    );

    let exported = MarkdownCodec::export(&imported.document).unwrap();
    assert_eq!(exported.markdown, source);
    for cell in cells {
        assert_dense_positions(
            &exported.positions,
            cell.id,
            cell.content.grapheme_len(),
            &exported.markdown,
        );
    }
}

#[test]
fn unsupported_inline_in_nested_container_promotes_the_top_level_block() {
    let source = "> quote with a footnote[^nested]\n\n[^nested]: definition\n";
    let imported = MarkdownCodec::import(source).unwrap();
    assert!(matches!(
        imported.document.blocks[0].kind,
        RichBlockKind::OpaqueMarkdown { .. }
    ));
    assert!(
        imported
            .positions
            .entries
            .iter()
            .all(|entry| entry.rich.container_id != imported.document.blocks[0].id)
    );
    assert_eq!(
        MarkdownCodec::export(&imported.document).unwrap().markdown,
        source
    );
}

#[test]
fn footnote_definition_before_later_blocks_imports_in_source_order() {
    let source = concat!(
        "# Before\n\n",
        "[^editor]: A definition stored before its reference.\n\n",
        "Paragraph after the definition uses [^editor].\n\n",
        "# After\n",
    );

    let imported = MarkdownCodec::import(source).unwrap();

    assert!(imported.document.blocks.len() >= 4);
    assert!(matches!(
        imported.document.blocks[1].kind,
        RichBlockKind::OpaqueMarkdown { .. }
    ));
    assert_eq!(
        MarkdownCodec::export(&imported.document).unwrap().markdown,
        source
    );
}
