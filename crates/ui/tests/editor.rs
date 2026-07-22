use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::layout::{Position, Rect};
use ratatui::style::{Color, Modifier};
use ui::{
    EditorBlockArea, EditorBlockSourceMap, EditorDocumentPosition, EditorFocus, EditorHitTarget,
    EditorMenu, EditorMenuAction, EditorMode, EditorQuickAction, EditorQuickMenuViewModel,
    EditorRenderBlock, EditorRenderSpan, EditorSelection, EditorSettingsControl,
    EditorSettingsField, EditorSettingsViewModel, EditorSourceRange, EditorSourceSelection,
    EditorSourceWindowLine, EditorTableAlignment, EditorTableCell, EditorTableEdge,
    EditorTextPosition, EditorToolbarAction, EditorViewModel, RichPosition, RichRange, TundraTheme,
    editor_layout, render_editor,
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
    assert_eq!(heading.fg, theme.accent_color);
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
    assert_eq!(link.fg, theme.accent_color);
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
    assert_eq!(first.bg, TundraTheme::default_dark().accent_color);
    let unselected = &terminal.backend().buffer()[(layout.canvas.x + 4, layout.canvas.y)];
    assert_ne!(unselected.bg, TundraTheme::default_dark().accent_color);

    let source = layout
        .modes
        .iter()
        .find(|item| item.mode == EditorMode::Source)
        .expect("source mode");
    let source_cell = &terminal.backend().buffer()[(source.area.x, source.area.y)];
    assert_eq!(source_cell.bg, TundraTheme::default_dark().accent_color);
}

#[test]
fn source_mode_disables_markdown_toolbar_actions_and_keeps_plain_actions_available() {
    let model = EditorViewModel::source("README.md", "plain **source**");
    let layout = editor_layout(Rect::new(0, 0, 140, 14), &model);
    let formatting = [
        EditorToolbarAction::ParagraphStyle,
        EditorToolbarAction::Bold,
        EditorToolbarAction::Italic,
        EditorToolbarAction::Strikethrough,
        EditorToolbarAction::InlineCode,
        EditorToolbarAction::BulletList,
        EditorToolbarAction::OrderedList,
        EditorToolbarAction::Quote,
        EditorToolbarAction::Link,
        EditorToolbarAction::Image,
        EditorToolbarAction::Table,
    ];

    for action in formatting {
        let item = toolbar_item(&layout, action);
        assert!(!item.enabled, "{action:?} must be disabled in Source mode");
        assert_eq!(layout.hit_test(item.area.x, item.area.y), None);
    }
    assert!(toolbar_item(&layout, EditorToolbarAction::New).enabled);
    assert!(toolbar_item(&layout, EditorToolbarAction::Open).enabled);
    assert!(toolbar_item(&layout, EditorToolbarAction::Save).enabled);

    let terminal = render(&model, 140, 14);
    let bold = toolbar_item(&layout, EditorToolbarAction::Bold);
    assert_eq!(
        terminal.backend().buffer()[(bold.area.x + 1, bold.area.y)].fg,
        TundraTheme::default_dark().muted
    );
}

#[test]
fn open_menu_renders_a_clickable_overlay_above_the_toolbar_and_canvas() {
    let mut model = EditorViewModel::new(
        "menu.md",
        vec![EditorRenderBlock::paragraph("canvas content")],
    );
    model.open_menu = Some(EditorMenu::Format);
    let layout = editor_layout(Rect::new(0, 0, 72, 16), &model);
    let popup = layout.menu_popup.expect("Format popup");
    let bold = layout
        .menu_items
        .iter()
        .find(|item| item.action == EditorMenuAction::Toolbar(EditorToolbarAction::Bold))
        .expect("Bold menu item");

    assert!(popup.height > 2);
    assert_eq!(
        layout.hit_test(bold.area.x, bold.area.y),
        Some(EditorHitTarget::MenuAction(EditorMenuAction::Toolbar(
            EditorToolbarAction::Bold
        )))
    );
    let terminal = render(&model, 72, 16);
    assert!(terminal_output(&terminal).contains("Strikethrough"));
    let (bold_x, bold_y) = find_text(&terminal, "Bold");
    assert!(bold_y >= popup.y && bold_y < popup.bottom());
    assert!(bold_x >= popup.x && bold_x < popup.right());
}

#[test]
fn settings_button_opens_a_modal_acceleration_panel_with_clickable_controls() {
    let mut model = EditorViewModel::new(
        "settings.md",
        vec![EditorRenderBlock::paragraph("canvas content")],
    );
    model.settings = Some(EditorSettingsViewModel {
        editable: true,
        enabled: true,
        activation_delay_ms: 2_000,
        ramp_duration_ms: 3_000,
        horizontal_max_step: 8,
        vertical_max_step: 3,
        selected: EditorSettingsField::ActivationDelay,
    });
    let layout = editor_layout(Rect::new(0, 0, 100, 24), &model);
    assert!(
        layout
            .menus
            .iter()
            .any(|menu| menu.menu == EditorMenu::Settings)
    );
    let settings = layout.settings.as_ref().expect("settings modal");
    let increase_delay = settings
        .controls
        .iter()
        .find(|control| {
            control.control == EditorSettingsControl::Increase(EditorSettingsField::ActivationDelay)
        })
        .expect("increase delay control");
    assert_eq!(
        layout.hit_test(increase_delay.area.x, increase_delay.area.y),
        Some(EditorHitTarget::SettingsControl(
            EditorSettingsControl::Increase(EditorSettingsField::ActivationDelay)
        ))
    );
    assert_eq!(
        layout.hit_test(layout.area.x, layout.area.y),
        Some(EditorHitTarget::SettingsDialog)
    );

    let output = terminal_output(&render(&model, 100, 24));
    assert!(output.contains("Editor Settings"));
    assert!(output.contains("Cursor acceleration"));
    assert!(output.contains("2000 ms"));
    assert!(output.contains("Horizontal maximum"));
    assert!(output.contains("Restore defaults"));
}

#[test]
fn quick_menu_orders_actions_renders_last_and_has_highest_hit_priority() {
    let mut model = EditorViewModel::new(
        "quick-menu.md",
        vec![EditorRenderBlock::paragraph("canvas content")],
    );
    model.open_menu = Some(EditorMenu::Format);
    model.quick_menu = Some(EditorQuickMenuViewModel {
        anchor: (10, 5),
        block_actions_enabled: true,
    });
    let layout = editor_layout(Rect::new(0, 0, 40, 14), &model);
    let popup = layout.quick_menu_popup.expect("quick menu popup");
    assert_eq!(popup, Rect::new(10, 6, 28, 3));
    assert_eq!(
        layout
            .quick_menu_items
            .iter()
            .map(|item| item.action)
            .collect::<Vec<_>>(),
        vec![
            EditorQuickAction::Bold,
            EditorQuickAction::Italic,
            EditorQuickAction::Paragraph,
            EditorQuickAction::Heading(1),
            EditorQuickAction::Heading(2),
            EditorQuickAction::Heading(3),
        ]
    );
    assert!(layout.quick_menu_items.iter().all(|item| item.enabled));

    let heading = layout
        .quick_menu_items
        .iter()
        .find(|item| item.action == EditorQuickAction::Heading(1))
        .expect("H1 quick action");
    assert_eq!(
        layout.hit_test(heading.area.x + 1, heading.area.y),
        Some(EditorHitTarget::QuickMenuAction(
            EditorQuickAction::Heading(1)
        ))
    );
    assert_eq!(
        layout.hit_test(popup.x, popup.y),
        Some(EditorHitTarget::QuickMenuPopup)
    );

    let terminal = render(&model, 40, 14);
    let heading_cell = &terminal.backend().buffer()[(heading.area.x + 1, heading.area.y)];
    assert_eq!(heading_cell.symbol(), "H");
    assert_eq!(heading_cell.fg, TundraTheme::default_dark().accent_color);
    assert!(heading_cell.modifier.contains(Modifier::BOLD));
    assert!(heading_cell.modifier.contains(Modifier::UNDERLINED));

    model.quick_menu = Some(EditorQuickMenuViewModel {
        anchor: (10, 5),
        block_actions_enabled: false,
    });
    let disabled_layout = editor_layout(Rect::new(0, 0, 40, 14), &model);
    let paragraph = disabled_layout
        .quick_menu_items
        .iter()
        .find(|item| item.action == EditorQuickAction::Paragraph)
        .expect("Normal quick action");
    assert!(!paragraph.enabled);
    assert_eq!(
        disabled_layout.hit_test(paragraph.area.x + 1, paragraph.area.y),
        Some(EditorHitTarget::QuickMenuPopup)
    );
    assert!(
        disabled_layout
            .quick_menu_items
            .iter()
            .filter(|item| matches!(
                item.action,
                EditorQuickAction::Bold | EditorQuickAction::Italic
            ))
            .all(|item| item.enabled)
    );
}

#[test]
fn quick_menu_clamps_flips_wraps_and_hides_when_the_border_cannot_fit() {
    let mut model = EditorViewModel::new(
        "quick-menu.md",
        vec![EditorRenderBlock::paragraph("canvas content")],
    );
    model.quick_menu = Some(EditorQuickMenuViewModel {
        anchor: (16, 12),
        block_actions_enabled: true,
    });
    let layout = editor_layout(Rect::new(5, 3, 12, 10), &model);
    let popup = layout.quick_menu_popup.expect("wrapped popup above anchor");
    assert_eq!(popup, Rect::new(5, 6, 12, 6));
    assert_eq!(popup.bottom(), 12);
    assert_eq!(
        layout
            .quick_menu_items
            .iter()
            .map(|item| item.area.y)
            .collect::<Vec<_>>(),
        vec![7, 7, 8, 9, 9, 10]
    );
    assert!(
        layout
            .quick_menu_items
            .iter()
            .all(|item| item.area.x >= popup.x + 1 && item.area.right() <= popup.right() - 1)
    );

    model.quick_menu = Some(EditorQuickMenuViewModel {
        anchor: (8, 5),
        block_actions_enabled: true,
    });
    let too_narrow = editor_layout(Rect::new(5, 3, 9, 10), &model);
    assert_eq!(too_narrow.quick_menu_popup, None);
    assert!(too_narrow.quick_menu_items.is_empty());

    let too_short = editor_layout(Rect::new(5, 3, 10, 5), &model);
    assert_eq!(too_short.quick_menu_popup, None);
    assert!(too_short.quick_menu_items.is_empty());
}

#[test]
fn rich_heading_levels_use_terminal_safe_accent_styles() {
    let model = EditorViewModel::new(
        "headings.md",
        vec![
            EditorRenderBlock::heading(1, "Heading One"),
            EditorRenderBlock::heading(2, "Heading Two"),
            EditorRenderBlock::heading(3, "Heading Three"),
            EditorRenderBlock::heading(6, "Heading Six"),
        ],
    );
    let terminal = render(&model, 60, 12);
    let theme = TundraTheme::default_dark();

    for text in ["Heading One", "Heading Two", "Heading Three", "Heading Six"] {
        let (x, y) = find_text(&terminal, text);
        let cell = &terminal.backend().buffer()[(x, y)];
        assert_eq!(cell.fg, theme.accent_color, "{text}");
        assert!(cell.modifier.contains(Modifier::BOLD), "{text}");
    }

    let (h1_x, h1_y) = find_text(&terminal, "Heading One");
    let h1 = &terminal.backend().buffer()[(h1_x, h1_y)];
    assert!(h1.modifier.contains(Modifier::UNDERLINED));
    assert!(!h1.modifier.contains(Modifier::ITALIC));

    let (h2_x, h2_y) = find_text(&terminal, "Heading Two");
    let h2 = &terminal.backend().buffer()[(h2_x, h2_y)];
    assert!(!h2.modifier.contains(Modifier::UNDERLINED));
    assert!(!h2.modifier.contains(Modifier::ITALIC));

    for text in ["Heading Three", "Heading Six"] {
        let (x, y) = find_text(&terminal, text);
        let cell = &terminal.backend().buffer()[(x, y)];
        assert!(cell.modifier.contains(Modifier::ITALIC), "{text}");
        assert!(!cell.modifier.contains(Modifier::UNDERLINED), "{text}");
    }
}

#[test]
fn source_horizontal_scroll_keeps_rendered_cells_and_mouse_positions_in_sync() {
    let mut model = EditorViewModel::source("wide.md", "0123456789".repeat(6));
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
fn wide_source_line_exposes_and_renders_a_proportional_horizontal_scrollbar() {
    let source = (0..100)
        .map(|column| char::from(b'0' + (column % 10) as u8))
        .collect::<String>();
    let mut model = EditorViewModel::source("wide.log", source);
    model.horizontal_scroll = 20;
    let layout = editor_layout(Rect::new(0, 0, 50, 8), &model);
    let scrollbar = layout
        .horizontal_scrollbar
        .expect("wide Source line has a horizontal scrollbar");
    let terminal = render(&model, 50, 8);

    assert_eq!(scrollbar.track.y, layout.canvas.bottom());
    assert_eq!(scrollbar.track.width, layout.canvas.width);
    assert!(scrollbar.thumb.width > 0);
    assert!(scrollbar.thumb.width < scrollbar.track.width);
    assert!(scrollbar.thumb.x > scrollbar.track.x);
    assert_eq!(
        layout.hit_test(scrollbar.track.x, scrollbar.track.y),
        Some(EditorHitTarget::HorizontalScrollbar)
    );
    assert_eq!(
        terminal.backend().buffer()[(layout.canvas.x, layout.canvas.y)].symbol(),
        "0"
    );
    assert_eq!(
        terminal.backend().buffer()[(scrollbar.track.x, scrollbar.track.y)].symbol(),
        "-"
    );
    assert_eq!(
        terminal.backend().buffer()[(scrollbar.thumb.x, scrollbar.thumb.y)].symbol(),
        "#"
    );
}

#[test]
fn source_scrollbars_reach_a_fixed_point_without_overlapping() {
    // Four lines require a vertical bar. That column reduces an exactly
    // fitting 47-cell line plus its caret cell to 47 cells, which then also
    // requires a horizontal bar; the new bottom row must keep the vertical
    // bar necessary.
    let source = format!("{}\na\nb\nc", "x".repeat(47));
    let model = EditorViewModel::source("both.log", source);
    let layout = editor_layout(Rect::new(0, 0, 50, 8), &model);
    let vertical = layout.vertical_scrollbar.expect("vertical scrollbar");
    let horizontal = layout.horizontal_scrollbar.expect("horizontal scrollbar");

    assert_eq!(layout.canvas.width, 47);
    assert_eq!(layout.canvas.height, 2);
    assert_eq!(vertical.track.height, layout.canvas.height);
    assert_eq!(horizontal.track.width, layout.canvas.width);
    assert_eq!(vertical.track.bottom(), horizontal.track.y);
    assert_eq!(horizontal.track.right(), vertical.track.x);
    assert_eq!(
        layout.hit_test(vertical.track.x, vertical.track.y),
        Some(EditorHitTarget::VerticalScrollbar)
    );
    assert_eq!(
        layout.hit_test(horizontal.track.x, horizontal.track.y),
        Some(EditorHitTarget::HorizontalScrollbar)
    );
}

#[test]
fn viewport_only_source_uses_document_width_for_its_horizontal_scrollbar() {
    let mut model = EditorViewModel::source_viewport(
        "window.log",
        0,
        1,
        vec![EditorSourceWindowLine::new(
            EditorSourceRange::new(16, 20),
            16,
            "qrst",
        )],
    );
    model.horizontal_scroll = 16;
    model.horizontal_content_width = 100;
    let layout = editor_layout(Rect::new(0, 0, 50, 8), &model);
    let terminal = render(&model, 50, 8);

    assert!(layout.horizontal_scrollbar.is_some());
    assert_eq!(layout.horizontal_scroll, 16);
    assert_eq!(
        terminal.backend().buffer()[(layout.canvas.x, layout.canvas.y)].symbol(),
        "q"
    );
    assert_eq!(
        layout.hit_test(layout.canvas.x + 2, layout.canvas.y),
        Some(EditorHitTarget::Canvas(EditorTextPosition::new(0, 18)))
    );
}

#[test]
fn source_horizontal_layout_handles_unicode_and_tiny_areas() {
    let model = EditorViewModel::source("unicode.log", "好e\u{301}");
    assert_eq!(model.horizontal_content_width, 4);

    let wide = EditorViewModel::source("tiny.log", "x".repeat(100));
    for width in 0..=2 {
        for height in 0..=5 {
            let layout = editor_layout(Rect::new(0, 0, width, height), &wide);
            if let Some(scrollbar) = layout.horizontal_scrollbar {
                assert!(scrollbar.track.width > 0);
                assert_eq!(scrollbar.track.height, 1);
                assert!(scrollbar.thumb.width > 0);
                assert!(scrollbar.thumb.width <= scrollbar.track.width);
            }
        }
    }
}

#[test]
fn end_caret_on_an_exactly_fitting_source_line_remains_visible() {
    let mut model = EditorViewModel::source("exact.log", "x".repeat(48));
    model.horizontal_scroll = 1;
    model.cursor = Some(EditorTextPosition::new(0, 48));
    let layout = editor_layout(Rect::new(0, 0, 50, 8), &model);
    let mut terminal = render(&model, 50, 8);

    assert_eq!(model.horizontal_content_width, 49);
    assert!(layout.horizontal_scrollbar.is_some());
    assert_eq!(layout.horizontal_scroll, 1);
    assert_eq!(
        terminal.get_cursor_position().expect("End cursor position"),
        Position::new(layout.canvas.right().saturating_sub(1), layout.canvas.y)
    );
}

#[test]
fn cursor_left_of_horizontal_viewport_is_not_drawn_at_canvas_origin() {
    let mut model = EditorViewModel::source("wide.log", "x".repeat(100));
    model.horizontal_scroll = 20;
    model.cursor = Some(EditorTextPosition::new(0, 5));
    let layout = editor_layout(Rect::new(0, 0, 50, 8), &model);
    let mut terminal = Terminal::new(TestBackend::new(50, 8)).expect("terminal");
    let sentinel = Position::new(49, 7);
    terminal
        .set_cursor_position(sentinel)
        .expect("seed cursor position");
    terminal
        .draw(|frame| {
            render_editor(frame, frame.area(), &model, &TundraTheme::default_dark());
        })
        .expect("render editor");

    assert_eq!(
        terminal.get_cursor_position().expect("cursor position"),
        sentinel
    );
    assert_ne!(sentinel.x, layout.canvas.x);
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
fn rich_table_cells_map_to_source_and_expose_draggable_column_edges() {
    let source = "| Name | Value |\n| --- | --- |\n| old | 1 |";
    let name_start = source.find("Name").expect("Name cell");
    let value_start = source.find("Value").expect("Value cell");
    let old_start = source.find("old").expect("old cell");
    let one_start = source.rfind('1').expect("numeric cell");
    let mut model = EditorViewModel::new(
        "table.md",
        vec![EditorRenderBlock::Table {
            header: vec![
                EditorTableCell {
                    spans: vec![
                        EditorRenderSpan::plain("Name")
                            .with_source_range(EditorSourceRange::new(name_start, name_start + 4)),
                    ],
                },
                EditorTableCell {
                    spans: vec![
                        EditorRenderSpan::plain("Value").with_source_range(EditorSourceRange::new(
                            value_start,
                            value_start + 5,
                        )),
                    ],
                },
            ],
            rows: vec![vec![
                EditorTableCell {
                    spans: vec![
                        EditorRenderSpan::plain("old")
                            .with_source_range(EditorSourceRange::new(old_start, old_start + 3)),
                    ],
                },
                EditorTableCell {
                    spans: vec![
                        EditorRenderSpan::plain("1")
                            .with_source_range(EditorSourceRange::new(one_start, one_start + 1)),
                    ],
                },
            ]],
            alignments: vec![EditorTableAlignment::Left, EditorTableAlignment::Right],
        }],
    );
    model.source = Some(source.to_string());
    model.block_sources = vec![EditorBlockSourceMap::new(EditorSourceRange::new(
        0,
        source.len(),
    ))];
    model.table_column_widths = vec![vec![8, 5]];

    let layout = editor_layout(Rect::new(0, 0, 80, 14), &model);
    let left_edge = layout
        .table_edge_handles
        .iter()
        .find(|handle| handle.edge == EditorTableEdge::Left)
        .expect("left table edge");
    let right_edge = layout
        .table_edge_handles
        .iter()
        .find(|handle| handle.edge == EditorTableEdge::Right)
        .expect("right table edge");
    for edge in [left_edge, right_edge] {
        assert_eq!(
            layout.hit_test(edge.area.x, edge.area.y + 1),
            Some(EditorHitTarget::TableEdge {
                block_index: 0,
                edge: edge.edge,
                source_range: EditorSourceRange::new(0, source.len()),
            })
        );
    }
    let old_end = layout
        .visual_position_for_source(old_start + 3)
        .expect("old cell end maps to the rendered table");
    let old_end_hit = layout
        .hit_test_source(
            layout.canvas.x + old_end.column as u16,
            layout.canvas.y + old_end.line as u16,
        )
        .expect("table content is editable");
    assert!(old_end_hit.editable);
    assert_eq!(old_end_hit.byte_offset, old_start + 3);

    let first_edge = layout
        .table_resize_handles
        .first()
        .expect("first column resize edge");
    assert_eq!(first_edge.width, 8);
    assert_eq!(first_edge.column_index, 0);
    assert_eq!(
        layout.hit_test(first_edge.area.x, first_edge.area.y + 1),
        Some(EditorHitTarget::TableResize {
            block_index: 0,
            column_index: 0,
            width: 8,
        })
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
fn rich_hits_use_stable_container_ids_and_grapheme_offsets_without_markdown_source() {
    let text = "A好e\u{301}🙂";
    let mut model = EditorViewModel::new(
        "logical.md",
        vec![EditorRenderBlock::Paragraph(vec![
            EditorRenderSpan::plain(text).with_rich_range(RichRange::from_raw(42, 10, 14)),
        ])],
    );
    model.rich_cursor = Some(RichPosition::from_raw(42, 12));
    // Deliberately conflicting legacy offsets must not affect a logical Rich
    // view model.
    model.cursor_offset = Some(0);
    let layout = editor_layout(Rect::new(0, 0, 60, 10), &model);
    let line = line_area(&layout, 0);

    assert_eq!(
        layout
            .hit_test_document(line.area.x, line.area.y)
            .expect("logical start")
            .position,
        EditorDocumentPosition::Rich(RichPosition::from_raw(42, 10))
    );
    assert_eq!(
        layout
            .hit_test_document(line.area.x + 2, line.area.y)
            .expect("inside wide grapheme")
            .position,
        EditorDocumentPosition::Rich(RichPosition::from_raw(42, 12))
    );
    assert_eq!(
        layout.visual_position_for_rich(RichPosition::from_raw(42, 13)),
        Some(EditorTextPosition::new(0, 4))
    );
    assert_eq!(
        layout.visual_position_for_document(EditorDocumentPosition::Source(0)),
        None
    );
}

#[test]
fn rich_virtual_list_marker_maps_to_nearest_logical_boundary_not_source() {
    let model = EditorViewModel::new(
        "list.md",
        vec![EditorRenderBlock::BulletListItem {
            depth: 0,
            checked: None,
            spans: vec![
                EditorRenderSpan::plain("item").with_rich_range(RichRange::from_raw(7, 0, 4)),
            ],
        }],
    );
    let layout = editor_layout(Rect::new(0, 0, 60, 10), &model);
    let line = line_area(&layout, 0);
    let marker_hit = layout
        .hit_test_document(line.area.x, line.area.y)
        .expect("virtual marker resolves to item start");

    assert_eq!(
        marker_hit.position,
        EditorDocumentPosition::Rich(RichPosition::from_raw(7, 0))
    );
    assert!(marker_hit.editable);
    assert!(layout.source_line_maps.is_empty());
    assert!(layout.hit_test_source(line.area.x, line.area.y).is_none());
    assert_eq!(
        layout.visual_position_for_rich(RichPosition::from_raw(7, 0)),
        Some(EditorTextPosition::new(0, 2))
    );
}

#[test]
fn rich_table_cells_and_virtual_borders_use_cell_logical_ranges() {
    let mut model = EditorViewModel::new(
        "table.md",
        vec![EditorRenderBlock::Table {
            header: vec![
                EditorTableCell {
                    spans: vec![
                        EditorRenderSpan::plain("Name")
                            .with_rich_range(RichRange::from_raw(100, 0, 4)),
                    ],
                },
                EditorTableCell {
                    spans: vec![
                        EditorRenderSpan::plain("值")
                            .with_rich_range(RichRange::from_raw(101, 0, 1)),
                    ],
                },
            ],
            rows: vec![vec![
                EditorTableCell {
                    spans: vec![
                        EditorRenderSpan::plain("e\u{301}")
                            .with_rich_range(RichRange::from_raw(200, 0, 1)),
                    ],
                },
                EditorTableCell {
                    spans: vec![
                        EditorRenderSpan::plain("🙂")
                            .with_rich_range(RichRange::from_raw(201, 0, 1)),
                    ],
                },
            ]],
            alignments: vec![EditorTableAlignment::Left, EditorTableAlignment::Right],
        }],
    );
    model.table_column_widths = vec![vec![6, 4]];
    let layout = editor_layout(Rect::new(0, 0, 80, 14), &model);

    let second_header = layout
        .visual_position_for_rich(RichPosition::from_raw(101, 0))
        .expect("second header cell");
    assert_eq!(second_header.line, 1);
    let content_hit = layout
        .hit_test_document(
            layout.canvas.x + second_header.column as u16,
            layout.canvas.y + second_header.line as u16,
        )
        .expect("header content hit");
    assert_eq!(
        content_hit.position,
        EditorDocumentPosition::Rich(RichPosition::from_raw(101, 0))
    );
    assert!(content_hit.editable);

    // The top border is synthetic, but each column segment carries its cell's
    // logical anchor and resolves to the nearest editable cell boundary.
    let border_hit = layout
        .hit_test_document(
            layout.canvas.x + second_header.column as u16,
            layout.canvas.y,
        )
        .expect("virtual border hit");
    assert_eq!(
        border_hit.position,
        EditorDocumentPosition::Rich(RichPosition::from_raw(101, 0))
    );
    assert!(border_hit.editable);

    let emoji_end = layout
        .visual_position_for_rich(RichPosition::from_raw(201, 1))
        .expect("emoji cell end");
    assert_eq!(emoji_end.line, 3);
    assert!(layout.source_line_maps.is_empty());
    assert!(
        layout
            .hit_test_source(
                layout.canvas.x + second_header.column as u16,
                layout.canvas.y
            )
            .is_none()
    );
}

#[test]
fn source_document_hits_remain_utf8_byte_offsets() {
    let model = EditorViewModel::source("source.md", "好x");
    let layout = editor_layout(Rect::new(0, 0, 60, 10), &model);
    let line = line_area(&layout, 0);

    assert_eq!(
        layout
            .hit_test_document(line.area.x + 2, line.area.y)
            .expect("source boundary")
            .position,
        EditorDocumentPosition::Source("好".len())
    );
    assert_eq!(
        layout.visual_position_for_document(EditorDocumentPosition::Rich(RichPosition::from_raw(
            1, 0
        ))),
        None
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
    assert_eq!(first_selected.bg, TundraTheme::default_dark().accent_color);
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
    model.image_protocol = ui::EditorImageProtocolStatus::Available;
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
    let terminal = render(&model, 72, 14);
    assert_eq!(
        terminal.backend().buffer()[(scrollbar.track.x, scrollbar.track.y)].symbol(),
        "|"
    );
    assert_eq!(
        terminal.backend().buffer()[(scrollbar.thumb.x, scrollbar.thumb.y)].symbol(),
        "#"
    );
}

#[test]
fn read_only_editor_disables_mutating_toolbar_controls() {
    let mut model = EditorViewModel::source("diagnostics.log", "diagnostic output");
    model.read_only = true;
    let layout = editor_layout(Rect::new(0, 0, 100, 24), &model);

    assert!(!toolbar_item(&layout, EditorToolbarAction::Save).enabled);
    assert!(!toolbar_item(&layout, EditorToolbarAction::Undo).enabled);
    assert!(!toolbar_item(&layout, EditorToolbarAction::Open).enabled);

    let mut rich = sample_model();
    rich.read_only = true;
    rich.quick_menu = Some(ui::EditorQuickMenuViewModel {
        anchor: (10, 10),
        block_actions_enabled: true,
    });
    let rich_layout = editor_layout(Rect::new(0, 0, 100, 30), &rich);
    assert!(rich_layout.table_edge_handles.is_empty());
    assert!(rich_layout.table_resize_handles.is_empty());
    assert!(rich_layout.quick_menu_items.is_empty());
}

#[test]
fn read_only_tail_editor_renders_access_window_and_reload_metadata() {
    let mut model = EditorViewModel::source("diagnostics.log", "tail content");
    model.read_only = true;
    model.reload_available = true;
    model.read_window = Some(ui::EditorReadWindowViewModel {
        start_byte: 42,
        total_bytes: 100,
    });

    let terminal = render(&model, 120, 24);
    let output = terminal_output(&terminal);

    assert!(output.contains("diagnostics.log [read-only] [tail]"));
    assert!(output.contains("R Reload"));
    assert!(output.contains("Bytes 43-100 of 100"));

    model.read_window = Some(ui::EditorReadWindowViewModel {
        start_byte: 0,
        total_bytes: 12,
    });
    let output = terminal_output(&render(&model, 120, 24));
    assert!(output.contains("diagnostics.log [read-only]"));
    assert!(!output.contains("[tail]"));
}

fn sample_model() -> EditorViewModel {
    let mut strong = EditorRenderSpan::strong("strong");
    strong.color = ui::EditorSpanColor::Normal;
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
    layout: &ui::EditorLayout,
    action: EditorToolbarAction,
) -> ui::EditorToolbarItemLayout {
    layout
        .toolbar_items
        .iter()
        .copied()
        .find(|item| item.action == action)
        .expect("toolbar item")
}

fn line_area(layout: &ui::EditorLayout, document_line: usize) -> ui::EditorLineLayout {
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
