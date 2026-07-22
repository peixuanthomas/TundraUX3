use super::document::*;
use super::*;

#[cfg(test)]
mod rich_layout_measurement_tests {
    use super::*;

    fn mapped(text: &str) -> EditorRenderSpan {
        EditorRenderSpan::plain(text).with_rich_range(RichRange::from_raw(
            42,
            0,
            Span::raw(text).styled_graphemes(Style::default()).count(),
        ))
    }

    fn representative_blocks() -> Vec<EditorRenderBlock> {
        vec![
            EditorRenderBlock::Paragraph(Vec::new()),
            EditorRenderBlock::Paragraph(vec![EditorRenderSpan::plain("")]),
            EditorRenderBlock::Paragraph(vec![EditorRenderSpan::plain(
                "plain 好 e\u{301} 🙂 text that wraps",
            )]),
            EditorRenderBlock::Paragraph(vec![mapped("first\r\nsecond\rthird\n")]),
            EditorRenderBlock::Heading {
                level: 2,
                spans: vec![mapped("a heading that wraps")],
            },
            EditorRenderBlock::BulletListItem {
                depth: 2,
                checked: Some(false),
                spans: vec![mapped("task item with a continuation")],
            },
            EditorRenderBlock::OrderedListItem {
                depth: 1,
                number: 123,
                spans: vec![mapped("ordered item")],
            },
            EditorRenderBlock::Quote {
                depth: 2,
                spans: vec![mapped("quoted text\nwith a break")],
            },
            EditorRenderBlock::Footnote {
                label: "note".to_string(),
                spans: vec![mapped("footnote body")],
            },
            EditorRenderBlock::CodeBlock {
                language: Some("rust".to_string()),
                lines: vec!["fn main() {}".to_string(), "// line 2".to_string()],
            },
            EditorRenderBlock::Table {
                header: vec![EditorTableCell::text("A"), EditorTableCell::text("B")],
                rows: vec![vec![
                    EditorTableCell::text("one"),
                    EditorTableCell::text("two"),
                ]],
                alignments: vec![EditorTableAlignment::Left, EditorTableAlignment::Right],
            },
            EditorRenderBlock::Table {
                header: Vec::new(),
                rows: Vec::new(),
                alignments: Vec::new(),
            },
            EditorRenderBlock::HorizontalRule,
            EditorRenderBlock::RawHtml("<div>raw\nhtml</div>".to_string()),
            EditorRenderBlock::Image {
                markdown: "![preview](image.png)".to_string(),
            },
            EditorRenderBlock::Blank,
        ]
    }

    #[test]
    fn bounded_measurement_matches_materialized_block_line_counts() {
        for block in representative_blocks() {
            for width in 1..=24 {
                let expected = block_lines(&block, 0, width, None, None, None).len();
                for limit in 0..=expected.saturating_add(1) {
                    assert_eq!(
                        rich_block_line_count_up_to(&block, width, limit),
                        (expected <= limit).then_some(expected),
                        "block={block:?}, width={width}, limit={limit}"
                    );
                }
            }
        }
    }

    #[test]
    fn rich_scrollbar_layout_flattens_the_document_only_once() {
        let blocks: Arc<[EditorRenderBlock]> = (0..10_000)
            .map(|index| EditorRenderBlock::paragraph(format!("line {index}")))
            .collect::<Vec<_>>()
            .into();
        let mut model = EditorViewModel::new_shared("large.md", blocks);
        model.scroll_line = 5_000;

        RICH_FLATTEN_CALL_COUNT.with(|count| count.set(0));
        let layout = editor_layout(Rect::new(0, 0, 80, 12), &model);
        let flatten_calls = RICH_FLATTEN_CALL_COUNT.with(std::cell::Cell::get);

        assert_eq!(flatten_calls, 1);
        assert_eq!(layout.document_line_count, 10_000);
        assert_eq!(layout.visible_start, 5_000);
        assert!(layout.vertical_scrollbar.is_some());
        assert_eq!(layout.prepared_lines.len(), layout.visible_capacity);
    }

    #[test]
    fn scrollbar_measurement_preserves_the_wide_layout_fixed_point() {
        // At full width this is exactly one line. Reserving a scrollbar first
        // would wrap it and incorrectly make the scrollbar self-fulfilling.
        let fitting =
            EditorViewModel::new("fit.md", vec![EditorRenderBlock::paragraph("x".repeat(20))]);
        let fitting_layout = editor_layout(Rect::new(0, 0, 20, 4), &fitting);
        assert_eq!(fitting_layout.canvas.width, 20);
        assert_eq!(fitting_layout.document_line_count, 1);
        assert!(fitting_layout.vertical_scrollbar.is_none());

        let overflowing = EditorViewModel::new(
            "overflow.md",
            vec![EditorRenderBlock::paragraph("x".repeat(21))],
        );
        let overflowing_layout = editor_layout(Rect::new(0, 0, 20, 4), &overflowing);
        assert_eq!(overflowing_layout.canvas.width, 19);
        assert_eq!(overflowing_layout.document_line_count, 2);
        assert!(overflowing_layout.vertical_scrollbar.is_some());
    }
}

#[cfg(test)]
mod rich_table_identity_tests {
    use super::*;

    fn rich_table(table_id: NodeId) -> EditorRenderBlock {
        EditorRenderBlock::RichTable {
            table_id,
            header: vec![
                EditorTableCell {
                    spans: vec![
                        EditorRenderSpan::plain("Name")
                            .with_rich_range(RichRange::from_raw(101, 0, 4)),
                    ],
                },
                EditorTableCell {
                    spans: vec![
                        EditorRenderSpan::plain("Value")
                            .with_rich_range(RichRange::from_raw(102, 0, 5)),
                    ],
                },
            ],
            rows: vec![vec![
                EditorTableCell {
                    spans: vec![
                        EditorRenderSpan::plain("Tundra")
                            .with_rich_range(RichRange::from_raw(201, 0, 6)),
                    ],
                },
                EditorTableCell {
                    spans: vec![
                        EditorRenderSpan::plain("3")
                            .with_rich_range(RichRange::from_raw(202, 0, 1)),
                    ],
                },
            ]],
            alignments: vec![EditorTableAlignment::Left, EditorTableAlignment::Right],
        }
    }

    #[test]
    fn rich_table_handles_and_widths_follow_stable_table_id() {
        let table_id = NodeId::new(9001);
        let mut model = EditorViewModel::new(
            "table.md",
            vec![EditorRenderBlock::paragraph("before"), rich_table(table_id)],
        );
        model.rich_table_column_widths.insert(table_id, vec![9, 4]);

        let first_layout = editor_layout(Rect::new(0, 0, 80, 16), &model);
        let first_resize = first_layout
            .table_resize_handles
            .iter()
            .find(|handle| handle.table_id == Some(table_id) && handle.column_index == 0)
            .expect("Rich table resize handle");
        assert_eq!(first_resize.block_index, 1);
        assert_eq!(first_resize.width, 9);
        assert_eq!(
            first_layout.hit_test(first_resize.area.x, first_resize.area.y + 1),
            Some(EditorHitTarget::RichTableResize {
                table_id,
                column_index: 0,
                width: 9,
            })
        );

        let left_edge = first_layout
            .table_edge_handles
            .iter()
            .find(|handle| {
                handle.table_id == Some(table_id) && handle.edge == EditorTableEdge::Left
            })
            .expect("Rich table edge handle");
        assert_eq!(left_edge.source_range, None);
        assert_eq!(
            first_layout.hit_test(left_edge.area.x, left_edge.area.y + 1),
            Some(EditorHitTarget::RichTableEdge {
                table_id,
                edge: EditorTableEdge::Left,
            })
        );

        model
            .blocks
            .insert(0, EditorRenderBlock::paragraph("new leading block"));
        let shifted_layout = editor_layout(Rect::new(0, 0, 80, 18), &model);
        let shifted_resize = shifted_layout
            .table_resize_handles
            .iter()
            .find(|handle| handle.table_id == Some(table_id) && handle.column_index == 0)
            .expect("shifted Rich table resize handle");
        assert_eq!(shifted_resize.block_index, 2);
        assert_eq!(shifted_resize.width, 9);
    }
}

#[cfg(test)]
mod rich_newline_position_tests {
    use super::*;

    #[test]
    fn soft_break_second_line_hit_is_the_boundary_after_the_break() {
        let container = NodeId::new(77);
        let model = EditorViewModel::new(
            "soft-break.md",
            vec![EditorRenderBlock::Paragraph(vec![
                EditorRenderSpan::plain("a ").with_rich_range(RichRange::from_raw(
                    container.get(),
                    0,
                    2,
                )),
                EditorRenderSpan::plain("\n").with_rich_range(RichRange::from_raw(
                    container.get(),
                    2,
                    3,
                )),
            ])],
        );
        let layout = editor_layout(Rect::new(0, 0, 40, 10), &model);
        let after_break = RichPosition::in_node(container, 3);

        let visual = layout
            .visual_position_for_rich(after_break)
            .expect("soft-break boundary has a visual position");
        assert_eq!(visual, EditorTextPosition::new(1, 0));

        let hit = layout
            .hit_test_document(
                layout.canvas.x.saturating_add(to_u16(visual.column)),
                layout
                    .canvas
                    .y
                    .saturating_add(to_u16(visual.line.saturating_sub(layout.visible_start))),
            )
            .expect("second visual line is hittable");
        assert_eq!(hit.position, EditorDocumentPosition::Rich(after_break));
        assert!(hit.editable);
        assert_eq!(
            layout.visual_position_for_document(hit.position),
            Some(visual)
        );
    }

    #[test]
    fn source_newline_mapping_remains_byte_based() {
        let model = EditorViewModel::source("source.md", "a \nb");
        let layout = editor_layout(Rect::new(0, 0, 40, 10), &model);
        let after_newline = 3;
        let visual = layout
            .visual_position_for_source(after_newline)
            .expect("source byte boundary has a visual position");
        assert_eq!(visual, EditorTextPosition::new(1, 0));

        let hit = layout
            .hit_test_document(
                layout.canvas.x.saturating_add(to_u16(visual.column)),
                layout
                    .canvas
                    .y
                    .saturating_add(to_u16(visual.line.saturating_sub(layout.visible_start))),
            )
            .expect("second source line is hittable");
        assert_eq!(hit.position, EditorDocumentPosition::Source(after_newline));
        assert!(hit.editable);
    }
}

#[cfg(test)]
mod shared_rich_blocks_tests {
    use super::*;

    #[test]
    fn shared_projection_is_authoritative_and_reused_by_layout() {
        let blocks: Arc<[EditorRenderBlock]> = vec![
            EditorRenderBlock::paragraph("shared paragraph"),
            EditorRenderBlock::Image {
                markdown: "![shared](preview.png)".to_string(),
            },
        ]
        .into();
        let retained = Arc::clone(&blocks);
        let model = EditorViewModel::new_shared("shared.md", blocks);

        assert!(model.blocks.is_empty());
        assert!(Arc::ptr_eq(
            model.shared_blocks.as_ref().expect("shared projection"),
            &retained
        ));
        assert_eq!(model.render_blocks().len(), 2);
        let layout = editor_layout(Rect::new(0, 0, 80, 14), &model);
        assert_eq!(layout.document_line_count, 2);
        assert_eq!(layout.image_areas.len(), 1);
        assert_eq!(layout.image_areas[0].block_index, 1);
    }
}

#[cfg(test)]
mod source_virtualization_tests {
    use super::*;

    #[test]
    fn coalesced_source_run_preserves_unicode_byte_boundaries() {
        let source = "A好e\u{301}🙂";
        let model = EditorViewModel::source("unicode.log", source);
        let layout = editor_layout(Rect::new(0, 0, 60, 10), &model);

        assert_eq!(layout.prepared_lines.len(), 1);
        assert_eq!(layout.prepared_lines[0].runs.len(), 1);
        assert_eq!(
            layout.prepared_lines[0].runs[0].text,
            DisplayText::SourceRange(EditorSourceRange::new(0, source.len()))
        );
        let boundaries = &layout.source_line_maps[0].boundaries;
        assert!(
            boundaries
                .iter()
                .any(|boundary| { boundary.column == 3 && boundary.byte_offset == "A好".len() })
        );
        assert!(boundaries.iter().any(|boundary| {
            boundary.column == 4 && boundary.byte_offset == "A好e\u{301}".len()
        }));
        assert!(layout.rich_line_maps.is_empty());
    }

    #[test]
    fn long_source_line_prepares_one_run_and_bounded_hit_boundaries() {
        let source = "x".repeat(1_000_000);
        let model = EditorViewModel::source("single-line.log", &source);
        let layout = editor_layout(Rect::new(0, 0, 80, 12), &model);

        assert_eq!(layout.document_line_count, 1);
        assert_eq!(layout.prepared_lines.len(), 1);
        assert_eq!(layout.prepared_lines[0].runs.len(), 1);
        assert!(
            layout.source_line_maps[0].boundaries.len()
                <= usize::from(layout.canvas.width).saturating_add(2)
        );
    }

    #[test]
    fn many_source_lines_prepare_only_the_scrolled_viewport() {
        let source = (0..100_000)
            .map(|line| format!("line {line}"))
            .collect::<Vec<_>>()
            .join("\n");
        let mut model = EditorViewModel::source("many-lines.log", source);
        model.scroll_line = 50_000;
        let layout = editor_layout(Rect::new(0, 0, 80, 12), &model);

        assert_eq!(layout.document_line_count, 100_000);
        assert_eq!(layout.visible_start, 50_000);
        assert_eq!(layout.prepared_lines.len(), layout.visible_capacity);
        assert!(layout.prepared_lines.len() < 100_000);
        assert_eq!(layout.source_line_maps.len(), layout.visible_capacity);
        assert!(
            layout
                .prepared_lines
                .iter()
                .all(|line| line.runs.len() == 1)
        );
        assert_eq!(
            layout
                .source_line_maps
                .first()
                .map(|line| line.document_line),
            Some(50_000)
        );
        assert!(layout.rich_line_maps.is_empty());
    }

    #[test]
    fn viewport_only_source_keeps_global_lines_columns_and_byte_offsets() {
        let first_line = 50_000;
        let first_byte = 900_000;
        let lines = (0..32)
            .map(|relative| {
                let text = if relative == 0 { "好x" } else { "row" };
                let start = first_byte + relative * 16;
                EditorSourceWindowLine::new(
                    EditorSourceRange::new(start, start + text.len()),
                    200,
                    text,
                )
            })
            .collect();
        let mut model = EditorViewModel::source_viewport("window.log", first_line, 100_000, lines);
        model.horizontal_scroll = 200;
        model.horizontal_content_width = 400;
        let layout = editor_layout(Rect::new(0, 0, 80, 12), &model);

        assert_eq!(layout.document_line_count, 100_000);
        assert_eq!(layout.visible_start, first_line);
        assert_eq!(layout.prepared_lines.len(), layout.visible_capacity);
        assert_eq!(layout.prepared_lines[0].column_start, 200);
        assert!(matches!(
            layout.prepared_lines[0].runs[0].text,
            DisplayText::Shared(_)
        ));
        let hit = layout
            .hit_test_source(layout.canvas.x + 2, layout.canvas.y)
            .expect("window line remains globally addressable");
        assert_eq!(hit.visual, EditorTextPosition::new(first_line, 202));
        assert_eq!(hit.byte_offset, first_byte + "好".len());
    }
}
