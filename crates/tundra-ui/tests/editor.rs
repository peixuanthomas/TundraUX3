use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier};
use tundra_ui::{
    EditorBlockArea, EditorBlockSourceMap, EditorFocus, EditorHitTarget, EditorMode,
    EditorRenderBlock, EditorRenderSpan, EditorSelection, EditorSourceRange, EditorSourceSelection,
    EditorTableAlignment, EditorTableCell, EditorTextPosition, EditorToolbarAction,
    EditorViewModel, TundraTheme, editor_layout, render_editor,
};

#[test]
fn minimum_editor_layout_keeps_save_mode_canvas_and_status_available() {
    let model = sample_model();
    let layout = editor_layout(Rect::new(0, 0, 50, 12), &model);

    assert_eq!(layout.menu_bar, Rect::new(0, 0, 50, 1));
    assert_eq!(layout.toolbar, Rect::new(0, 1, 50, 1));
    assert_eq!(layout.status_bar, Rect::new(0, 11, 50, 1));
    assert!(layout.canvas.height >= 7);
    assert!(layout.toolbar_overflow);

    let save = toolbar_item(&layout, EditorToolbarAction::Save);
    assert_eq!(
        layout.hit_test(save.area.x, save.area.y),
        Some(EditorHitTarget::Toolbar(EditorToolbarAction::Save))
    );
    let source = layout
        .modes
        .iter()
        .find(|item| item.mode == EditorMode::Source)
        .expect("source mode");
    assert_eq!(
        layout.hit_test(source.area.x, source.area.y),
        Some(EditorHitTarget::Mode(EditorMode::Source))
    );
    assert!(
        layout
            .toolbar_items
            .iter()
            .any(|item| item.action == EditorToolbarAction::More)
    );
}

#[test]
fn shell_minimum_main_height_degrades_without_losing_the_editing_canvas() {
    let model = sample_model();
    let layout = editor_layout(Rect::new(0, 0, 50, 6), &model);

    assert!(!layout.canvas_framed);
    assert_eq!(layout.menu_bar.height, 1);
    assert_eq!(layout.toolbar.height, 1);
    assert_eq!(layout.canvas.height, 3);
    assert_eq!(layout.status_bar.height, 1);
    assert!(
        layout
            .toolbar_items
            .iter()
            .any(|item| item.action == EditorToolbarAction::Save)
    );
    assert_eq!(layout.modes.len(), 2);
}

#[test]
fn rich_renderer_covers_markdown_blocks_and_terminal_fallbacks() {
    let model = sample_model();
    let terminal = render(&model, 120, 40);
    let output = terminal_output(&terminal);

    assert!(output.contains("Terminal Editor"));
    assert!(output.contains("strong"));
    assert!(output.contains("emphasis"));
    assert!(output.contains("inline_code"));
    assert!(output.contains("linked text"));
    assert!(output.contains("☒ completed task"));
    assert!(output.contains("2. ordered item"));
    assert!(output.contains("│ quoted text"));
    assert!(output.contains("┌─ rust"));
    assert!(output.contains("let answer = 42;"));
    assert!(output.contains("Name"));
    assert!(output.contains("Value"));
    assert!(output.contains("Tundra"));
    assert!(output.contains("HTML <details>raw</details>"));
    assert!(output.contains("![Tundra logo](images/tundra.png)"));
    assert!(output.contains("[^note] footnote text"));

    let theme = TundraTheme::default_dark();
    let (heading_x, heading_y) = find_text(&terminal, "Terminal Editor");
    let heading = &terminal.backend().buffer()[(heading_x, heading_y)];
    assert_eq!(heading.fg, theme.accent);
    assert!(heading.modifier.contains(Modifier::BOLD));
    assert!(heading.modifier.contains(Modifier::UNDERLINED));

    let (strong_x, strong_y) = find_text(&terminal, "strong");
    assert!(
        terminal.backend().buffer()[(strong_x, strong_y)]
            .modifier
            .contains(Modifier::BOLD)
    );
    let (emphasis_x, emphasis_y) = find_text(&terminal, "emphasis");
    assert!(
        terminal.backend().buffer()[(emphasis_x, emphasis_y)]
            .modifier
            .contains(Modifier::ITALIC)
    );
    let (code_x, code_y) = find_text(&terminal, "inline_code");
    assert_eq!(
        terminal.backend().buffer()[(code_x, code_y)].bg,
        Color::DarkGray
    );
    let (link_x, link_y) = find_text(&terminal, "linked text");
    let link = &terminal.backend().buffer()[(link_x, link_y)];
    assert_eq!(link.fg, theme.accent);
    assert!(link.modifier.contains(Modifier::UNDERLINED));
}

#[test]
fn source_mode_preserves_markdown_and_highlights_the_selection() {
    let mut model =
        EditorViewModel::source("README.md", "# raw **markdown**\n\n- [x] remains source");
    model.focus = EditorFocus::Canvas;
    model.selection = Some(EditorSelection::new(
        EditorTextPosition::new(0, 0),
        EditorTextPosition::new(0, 3),
    ));
    model.cursor = Some(EditorTextPosition::new(0, 3));
    let layout = editor_layout(Rect::new(0, 0, 72, 14), &model);
    let terminal = render(&model, 72, 14);
    let output = terminal_output(&terminal);

    assert!(output.contains("# raw **markdown**"));
    assert!(output.contains("- [x] remains source"));
    let first = &terminal.backend().buffer()[(layout.canvas.x, layout.canvas.y)];
    assert_eq!(first.symbol(), "#");
    assert_eq!(first.bg, TundraTheme::default_dark().accent);
    let unselected = &terminal.backend().buffer()[(layout.canvas.x + 4, layout.canvas.y)];
    assert_ne!(unselected.bg, TundraTheme::default_dark().accent);

    let source = layout
        .modes
        .iter()
        .find(|item| item.mode == EditorMode::Source)
        .expect("source mode");
    let source_cell = &terminal.backend().buffer()[(source.area.x, source.area.y)];
    assert_eq!(source_cell.bg, TundraTheme::default_dark().accent);
}

#[test]
fn source_horizontal_scroll_keeps_rendered_cells_and_mouse_positions_in_sync() {
    let mut model = EditorViewModel::source("wide.md", "0123456789");
    model.horizontal_scroll = 4;
    model.cursor = Some(EditorTextPosition::new(0, 6));
    let layout = editor_layout(Rect::new(0, 0, 50, 8), &model);
    let terminal = render(&model, 50, 8);

    assert_eq!(
        terminal.backend().buffer()[(layout.canvas.x, layout.canvas.y)].symbol(),
        "4"
    );
    assert_eq!(
        layout.hit_test(layout.canvas.x + 2, layout.canvas.y),
        Some(EditorHitTarget::Canvas(EditorTextPosition::new(0, 6)))
    );
}

#[test]
fn editor_never_emits_terminal_control_sequences_from_untrusted_text() {
    let payload = "\u{1b}]52;c;dGVzdA==\u{7}\u{9b}31m\u{202e}";
    let mut source = EditorViewModel::source(format!("evil{payload}.md"), payload);
    source.status_message = Some(payload.to_string());
    let source_terminal = render(&source, 96, 12);
    assert_terminal_output_is_inert(&source_terminal);
    let source_output = terminal_output(&source_terminal);
    assert!(source_output.contains('\u{241b}'));
    assert!(source_output.contains('\u{2407}'));
    assert!(source_output.contains('\u{fffd}'));

    let mut rich = EditorViewModel::new(
        format!("rich{payload}.md"),
        vec![EditorRenderBlock::RawHtml(payload.to_string())],
    );
    rich.status_message = Some(payload.to_string());
    let rich_terminal = render(&rich, 96, 12);
    assert_terminal_output_is_inert(&rich_terminal);
}

#[test]
fn source_mode_splits_all_supported_line_endings_without_rendering_carriage_returns() {
    let model = EditorViewModel::source("mixed.txt", "one\r\ntwo\rthree\nfour");
    let terminal = render(&model, 60, 12);
    let output = terminal_output(&terminal);

    assert!(output.contains("one"));
    assert!(output.contains("two"));
    assert!(output.contains("three"));
    assert!(output.contains("four"));
    assert!(!output.contains('\u{240d}'));
}

#[test]
fn rich_hits_resolve_hidden_heading_markers_and_source_blank_lines() {
    let source = "# Title\n\nParagraph";
    let mut model = EditorViewModel::new(
        "mapped.md",
        vec![
            EditorRenderBlock::Heading {
                level: 1,
                spans: vec![
                    EditorRenderSpan::plain("Title")
                        .with_source_range(EditorSourceRange::new(2, 7)),
                ],
            },
            EditorRenderBlock::Paragraph(vec![
                EditorRenderSpan::plain("Paragraph")
                    .with_source_range(EditorSourceRange::new(9, 18)),
            ]),
        ],
    );
    model.source = Some(source.to_string());
    model.block_sources = vec![
        EditorBlockSourceMap::new(EditorSourceRange::new(0, 7))
            .with_content_range(EditorSourceRange::new(2, 7)),
        EditorBlockSourceMap::new(EditorSourceRange::new(9, 18))
            .with_content_range(EditorSourceRange::new(9, 18)),
    ];
    let layout = editor_layout(Rect::new(0, 0, 60, 12), &model);
    let heading = line_area(&layout, 0);
    let paragraph = line_area(&layout, 1);

    assert_eq!(
        layout
            .hit_test_source(heading.area.x, heading.area.y)
            .expect("mapped heading")
            .byte_offset,
        2
    );
    assert_eq!(
        layout
            .hit_test_source(paragraph.area.x, paragraph.area.y)
            .expect("mapped paragraph")
            .byte_offset,
        9
    );
}

#[test]
fn wrapped_rich_lines_keep_exact_source_offsets() {
    let source = "abcdefghijklmnopqrstuvwx";
    let mut model = EditorViewModel::new(
        "wrapped.md",
        vec![EditorRenderBlock::Paragraph(vec![
            EditorRenderSpan::plain(source)
                .with_source_range(EditorSourceRange::new(0, source.len())),
        ])],
    );
    model.source = Some(source.to_string());
    model.block_sources = vec![
        EditorBlockSourceMap::new(EditorSourceRange::new(0, source.len()))
            .with_content_range(EditorSourceRange::new(0, source.len())),
    ];
    let layout = editor_layout(Rect::new(0, 0, 20, 10), &model);
    let continuation = line_area(&layout, 1);

    assert_eq!(layout.canvas.width, 18);
    assert_eq!(
        layout
            .hit_test_source(continuation.area.x, continuation.area.y)
            .expect("mapped continuation")
            .byte_offset,
        18
    );
}

#[test]
fn list_and_code_virtual_cells_anchor_to_editable_source_content() {
    let mut list = EditorViewModel::new(
        "list.md",
        vec![EditorRenderBlock::BulletListItem {
            depth: 0,
            checked: None,
            spans: vec![
                EditorRenderSpan::plain("item").with_source_range(EditorSourceRange::new(2, 6)),
            ],
        }],
    );
    list.source = Some("- item".to_string());
    list.block_sources = vec![
        EditorBlockSourceMap::new(EditorSourceRange::new(0, 6))
            .with_content_range(EditorSourceRange::new(2, 6)),
    ];
    let list_layout = editor_layout(Rect::new(0, 0, 60, 10), &list);
    let list_line = line_area(&list_layout, 0);
    assert_eq!(
        list_layout
            .hit_test_source(list_line.area.x, list_line.area.y)
            .expect("mapped bullet")
            .byte_offset,
        2
    );
    assert_eq!(
        list_layout.visual_position_for_source(2),
        Some(EditorTextPosition::new(0, 2))
    );
    assert_eq!(
        list_layout
            .hit_test_source(list_line.area.x + 2, list_line.area.y)
            .expect("mapped item")
            .byte_offset,
        2
    );

    let code_source = "```txt\ncode\n```";
    let mut code = EditorViewModel::new(
        "code.md",
        vec![EditorRenderBlock::CodeBlock {
            language: Some("txt".to_string()),
            lines: vec!["code".to_string()],
        }],
    );
    code.source = Some(code_source.to_string());
    code.block_sources = vec![
        EditorBlockSourceMap::new(EditorSourceRange::new(0, code_source.len()))
            .with_content_range(EditorSourceRange::new(7, 11)),
    ];
    let code_layout = editor_layout(Rect::new(0, 0, 60, 12), &code);
    let header = line_area(&code_layout, 0);
    let content = line_area(&code_layout, 1);
    let footer = line_area(&code_layout, 2);
    assert_eq!(
        code_layout
            .hit_test_source(header.area.x, header.area.y)
            .expect("mapped code header")
            .byte_offset,
        7
    );
    assert_eq!(
        code_layout
            .hit_test_source(content.area.x + 2, content.area.y)
            .expect("mapped code text")
            .byte_offset,
        7
    );
    assert_eq!(
        code_layout
            .hit_test_source(footer.area.x, footer.area.y)
            .expect("mapped code footer")
            .byte_offset,
        11
    );
    assert_eq!(
        code_layout.visual_position_for_source(7),
        Some(EditorTextPosition::new(1, 2))
    );
}

#[test]
fn source_hits_follow_terminal_cells_but_return_utf8_grapheme_boundaries() {
    let source = "A好e\u{301}🙂";
    let model = EditorViewModel::source("unicode.txt", source);
    let layout = editor_layout(Rect::new(0, 0, 60, 10), &model);
    let line = line_area(&layout, 0);

    assert_eq!(
        layout
            .hit_test_source(line.area.x + 1, line.area.y)
            .expect("before CJK")
            .byte_offset,
        1
    );
    assert_eq!(
        layout
            .hit_test_source(line.area.x + 2, line.area.y)
            .expect("inside CJK")
            .byte_offset,
        4
    );
    assert_eq!(
        layout
            .hit_test_source(line.area.x + 4, line.area.y)
            .expect("after combining grapheme")
            .byte_offset,
        7
    );
    assert_eq!(
        layout
            .hit_test_source(line.area.x + 5, line.area.y)
            .expect("inside emoji")
            .byte_offset,
        source.len()
    );
}

#[test]
fn rich_and_source_views_preserve_the_same_cursor_and_selection_offsets() {
    let source = "**bold**";
    let mut rich = EditorViewModel::new(
        "mode.md",
        vec![EditorRenderBlock::Paragraph(vec![
            EditorRenderSpan::strong("bold").with_source_range(EditorSourceRange::new(2, 6)),
        ])],
    );
    rich.source = Some(source.to_string());
    rich.block_sources = vec![
        EditorBlockSourceMap::new(EditorSourceRange::new(0, source.len()))
            .with_content_range(EditorSourceRange::new(2, 6)),
    ];
    rich.cursor_offset = Some(4);
    rich.selection_offsets = Some(EditorSourceSelection::new(2, 6));
    let rich_layout = editor_layout(Rect::new(0, 0, 60, 10), &rich);
    let rich_position = rich_layout
        .visual_position_for_source(4)
        .expect("rich cursor mapping");

    let mut source_model = EditorViewModel::source("mode.md", source);
    source_model.cursor_offset = Some(4);
    source_model.selection_offsets = Some(EditorSourceSelection::new(2, 6));
    let source_layout = editor_layout(Rect::new(0, 0, 60, 10), &source_model);
    let source_position = source_layout
        .visual_position_for_source(4)
        .expect("source cursor mapping");

    assert_eq!(rich_position, EditorTextPosition::new(0, 2));
    assert_eq!(source_position, EditorTextPosition::new(0, 4));
    assert_eq!(
        rich_layout
            .hit_test_source(
                rich_layout.canvas.x + rich_position.column as u16,
                rich_layout.canvas.y,
            )
            .expect("rich round trip")
            .byte_offset,
        4
    );
    assert_eq!(
        source_layout
            .hit_test_source(
                source_layout.canvas.x + source_position.column as u16,
                source_layout.canvas.y,
            )
            .expect("source round trip")
            .byte_offset,
        4
    );

    let terminal = render(&rich, 60, 10);
    let first_selected = &terminal.backend().buffer()[(rich_layout.canvas.x, rich_layout.canvas.y)];
    assert_eq!(first_selected.bg, TundraTheme::default_dark().accent);
}

#[test]
fn block_and_image_areas_track_visible_markdown_geometry() {
    let mut model = EditorViewModel::new(
        "images.md",
        vec![
            EditorRenderBlock::paragraph("before"),
            EditorRenderBlock::Image {
                markdown: "![preview](preview.png)".to_string(),
            },
            EditorRenderBlock::CodeBlock {
                language: Some("text".to_string()),
                lines: vec!["one".to_string(), "two".to_string()],
            },
        ],
    );
    model.image_protocol = tundra_ui::EditorImageProtocolStatus::Available;
    let layout = editor_layout(Rect::new(0, 0, 80, 18), &model);

    assert!(layout.block_areas.iter().any(|area| area.block_index == 0));
    assert_eq!(
        layout.image_areas,
        vec![EditorBlockArea {
            block_index: 1,
            area: layout
                .block_areas
                .iter()
                .find(|area| area.block_index == 1)
                .expect("image block")
                .area,
        }]
    );
    let image = layout.image_areas[0];
    assert_eq!(
        layout.hit_test(image.area.x, image.area.y),
        Some(EditorHitTarget::Canvas(EditorTextPosition::new(
            layout
                .line_areas
                .iter()
                .find(|line| line.block_index == Some(1))
                .expect("image line")
                .document_line,
            0,
        )))
    );
}

#[test]
fn overflowing_document_exposes_proportional_scrollbar_and_scrolled_hits() {
    let mut model = EditorViewModel::new(
        "long.md",
        (0..40)
            .map(|index| EditorRenderBlock::paragraph(format!("line {index}")))
            .collect(),
    );
    model.scroll_line = 20;
    let layout = editor_layout(Rect::new(0, 0, 72, 14), &model);
    let scrollbar = layout
        .vertical_scrollbar
        .expect("overflowing document scrollbar");

    assert_eq!(layout.visible_start, 20);
    assert!(scrollbar.thumb.height > 0);
    assert!(scrollbar.thumb.height < scrollbar.track.height);
    assert!(scrollbar.thumb.y > scrollbar.track.y);
    assert_eq!(
        layout.hit_test(layout.canvas.x, layout.canvas.y),
        Some(EditorHitTarget::Canvas(EditorTextPosition::new(20, 0)))
    );
    assert_eq!(
        layout.hit_test(scrollbar.track.x, scrollbar.track.y),
        Some(EditorHitTarget::VerticalScrollbar)
    );
}

fn sample_model() -> EditorViewModel {
    let mut strong = EditorRenderSpan::strong("strong");
    strong.color = tundra_ui::EditorSpanColor::Normal;
    let mut struck = EditorRenderSpan::plain("struck");
    struck.strikethrough = true;
    let blocks = vec![
        EditorRenderBlock::heading(1, "Terminal Editor"),
        EditorRenderBlock::Paragraph(vec![
            EditorRenderSpan::plain("Paragraph with "),
            strong,
            EditorRenderSpan::plain(", "),
            EditorRenderSpan::emphasis("emphasis"),
            EditorRenderSpan::plain(", "),
            struck,
            EditorRenderSpan::plain(", "),
            EditorRenderSpan::code("inline_code"),
            EditorRenderSpan::plain(" and "),
            EditorRenderSpan::plain("linked text").with_link(),
        ]),
        EditorRenderBlock::BulletListItem {
            depth: 0,
            checked: Some(true),
            spans: vec![EditorRenderSpan::plain("completed task")],
        },
        EditorRenderBlock::OrderedListItem {
            depth: 0,
            number: 2,
            spans: vec![EditorRenderSpan::plain("ordered item")],
        },
        EditorRenderBlock::Quote {
            depth: 1,
            spans: vec![EditorRenderSpan::plain("quoted text")],
        },
        EditorRenderBlock::CodeBlock {
            language: Some("rust".to_string()),
            lines: vec!["let answer = 42;".to_string()],
        },
        EditorRenderBlock::Table {
            header: vec![
                EditorTableCell::text("Name"),
                EditorTableCell::text("Value"),
            ],
            rows: vec![vec![
                EditorTableCell::text("Tundra"),
                EditorTableCell::text("3"),
            ]],
            alignments: vec![EditorTableAlignment::Left, EditorTableAlignment::Right],
        },
        EditorRenderBlock::HorizontalRule,
        EditorRenderBlock::RawHtml("<details>raw</details>".to_string()),
        EditorRenderBlock::Image {
            markdown: "![Tundra logo](images/tundra.png)".to_string(),
        },
        EditorRenderBlock::Footnote {
            label: "note".to_string(),
            spans: vec![EditorRenderSpan::plain("footnote text")],
        },
    ];
    let mut model = EditorViewModel::new("README.md", blocks);
    model.dirty = true;
    model.word_count = 18;
    model.cursor = Some(EditorTextPosition::new(0, 0));
    model
}

fn render(model: &EditorViewModel, width: u16, height: u16) -> Terminal<TestBackend> {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("terminal");
    terminal
        .draw(|frame| {
            render_editor(frame, frame.area(), model, &TundraTheme::default_dark());
        })
        .expect("render editor");
    terminal
}

fn toolbar_item(
    layout: &tundra_ui::EditorLayout,
    action: EditorToolbarAction,
) -> tundra_ui::EditorToolbarItemLayout {
    layout
        .toolbar_items
        .iter()
        .copied()
        .find(|item| item.action == action)
        .expect("toolbar item")
}

fn line_area(
    layout: &tundra_ui::EditorLayout,
    document_line: usize,
) -> tundra_ui::EditorLineLayout {
    layout
        .line_areas
        .iter()
        .copied()
        .find(|line| line.document_line == document_line)
        .expect("visible editor line")
}

fn terminal_output(terminal: &Terminal<TestBackend>) -> String {
    terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect()
}

fn assert_terminal_output_is_inert(terminal: &Terminal<TestBackend>) {
    for cell in terminal.backend().buffer().content() {
        assert!(
            !cell.symbol().chars().any(char::is_control),
            "terminal control character leaked through rendered cell: {:?}",
            cell.symbol()
        );
    }
}

fn find_text(terminal: &Terminal<TestBackend>, needle: &str) -> (u16, u16) {
    let buffer = terminal.backend().buffer();
    for y in 0..buffer.area.height {
        let row = (0..buffer.area.width)
            .map(|x| buffer[(x, y)].symbol())
            .collect::<String>();
        if let Some(byte_index) = row.find(needle) {
            let x = row[..byte_index].chars().count() as u16;
            return (x, y);
        }
    }
    panic!("text not found: {needle}");
}
