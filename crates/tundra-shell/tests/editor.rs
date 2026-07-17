use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use ratatui::layout::Rect;
use tundra_platform::mock::{MockCall, MockPlatform};
use tundra_platform::{
    AppPaths, PlatformCapabilities, PlatformKind, UserDirs, build_windows_app_paths,
    cleanup_temp_path,
};
use tundra_shell::{
    HomeModeOverride, InputEvent, InputKey, InputModifiers, InputPhase, KeyInput, PointerButton,
    ScrollDirection, ShellAction, ShellComponent, ShellHomeMode, ShellLaunchConfig,
    ShellLaunchTarget, ShellScreen, ShellState, ShellTerminalMode, prepare_shell_startup,
};
use tundra_ui::{
    EditorDocumentPosition, EditorFocus, EditorHitTarget, EditorMenu, EditorMenuAction, EditorMode,
    EditorQuickAction, EditorRenderBlock, EditorTextPosition, EditorToolbarAction, ShellLayout,
};

fn default_config() -> ShellLaunchConfig {
    ShellLaunchConfig {
        terminal_mode: ShellTerminalMode::Fullscreen,
        home_mode_override: HomeModeOverride::BuildDefault,
        launch_target: ShellLaunchTarget::Home,
    }
}

fn editor_config() -> ShellLaunchConfig {
    ShellLaunchConfig::editor()
}

#[test]
fn editor_launch_target_starts_directly_without_an_auth_gate() {
    let state = ShellState::new(editor_config(), (120, 40));

    assert_eq!(state.active_screen(), ShellScreen::Editor);
    assert_eq!(state.focused_component(), ShellComponent::Editor);
    assert_eq!(state.screen_stack(), &[ShellScreen::Editor]);
    assert_eq!(state.to_editor_view_model().file_name, "Untitled.md");
}

#[test]
fn editor_launch_target_waits_for_login_then_opens_editor() {
    let fixture = FixtureRoot::new("direct-editor-login");
    let platform = mock_platform(fixture.path());
    bootstrap_with_shell(&platform);
    let startup = prepare_shell_startup(&platform, editor_config()).expect("editor startup");
    let mut state = ShellState::new_with_startup(editor_config(), (120, 40), startup);

    assert_eq!(state.active_screen(), ShellScreen::Login);
    assert_eq!(state.to_editor_view_model().file_name, "Untitled.md");

    select_login_user(&mut state, &platform, "AdminUser");
    state.apply_input_with_platform(InputEvent::from_key_label("Tab"), &platform);
    type_text(&mut state, &platform, "StrongPass123");
    state.apply_input_with_platform(InputEvent::from_key_label("Enter"), &platform);

    assert_eq!(state.active_screen(), ShellScreen::Editor);
    assert_eq!(state.focused_component(), ShellComponent::Editor);
    assert_eq!(
        state.screen_stack(),
        &[ShellScreen::Home, ShellScreen::Editor]
    );
    assert_eq!(state.to_editor_view_model().file_name, "Untitled.md");
}

#[test]
fn home_editor_entry_opens_a_rich_markdown_document() {
    let fixture = FixtureRoot::new("home-open");
    let platform = mock_platform(fixture.path());
    let mut state = new_user_home_state();

    open_editor_from_home(&mut state, &platform);

    assert_eq!(state.active_screen(), ShellScreen::Editor);
    assert_eq!(state.focused_component(), ShellComponent::Editor);
    assert_eq!(
        state.screen_stack(),
        &[ShellScreen::Home, ShellScreen::Editor]
    );
    let editor = state.to_editor_view_model();
    assert_eq!(editor.file_name, "Untitled.md");
    assert_eq!(editor.mode, EditorMode::Rich);
    assert!(!editor.dirty);
}

#[test]
fn editor_accepts_unicode_and_inserts_spaces_for_tab() {
    let fixture = FixtureRoot::new("unicode-tab");
    let platform = mock_platform(fixture.path());
    let mut state = new_user_home_state();
    open_editor_from_home(&mut state, &platform);

    type_text(&mut state, &platform, "你好🙂");
    state.apply_input_with_platform(InputEvent::from_key_label("Tab"), &platform);
    state.apply_input_with_platform(ctrl_shift('m'), &platform);

    let editor = state.to_editor_view_model();
    assert_eq!(editor.mode, EditorMode::Source);
    assert_eq!(editor.source_lines.join("\n"), "你好🙂    ");
    assert!(editor.dirty);
    assert_eq!(editor.cursor.map(|cursor| cursor.column), Some(10));
}

#[test]
fn source_caret_reveals_a_long_line_and_the_horizontal_scrollbar_moves_the_viewport() {
    let fixture = FixtureRoot::new("source-horizontal-caret");
    let platform = mock_platform(fixture.path());
    bootstrap_with_shell(&platform);
    fs::write(
        fixture.path().join("Documents").join("wide.log"),
        "0123456789".repeat(30),
    )
    .expect("seed wide log");
    let mut state = logged_in_state(&platform);
    open_only_document_in_editor(&mut state, &platform);
    assert!(state.to_editor_view_model().read_only);

    state.apply_input_with_platform(InputEvent::from_key_label("Home"), &platform);
    let home = state.to_editor_view_model();
    let home_layout = current_editor_layout(&state);
    assert_eq!(home.horizontal_scroll, 0);
    assert!(home_layout.horizontal_scrollbar.is_some());

    state.apply_input_with_platform(InputEvent::from_key_label("End"), &platform);
    let end = state.to_editor_view_model();
    let end_layout = current_editor_layout(&state);
    let cursor = end.cursor.expect("Source caret");
    assert!(end.horizontal_scroll > 0);
    assert!(cursor.column >= end.horizontal_scroll);
    assert!(
        cursor.column
            < end
                .horizontal_scroll
                .saturating_add(usize::from(end_layout.canvas.width))
    );

    state.apply_input_with_platform(InputEvent::from_key_label("Home"), &platform);
    let scrollbar = current_editor_layout(&state)
        .horizontal_scrollbar
        .expect("horizontal scrollbar");
    state.apply_input_with_platform(
        InputEvent::mouse_down(
            PointerButton::Left,
            (scrollbar.track.right().saturating_sub(1), scrollbar.track.y),
        ),
        &platform,
    );
    assert!(state.to_editor_view_model().horizontal_scroll > 0);
}

#[test]
fn rich_source_toggle_preserves_markdown_and_rendered_content() {
    let fixture = FixtureRoot::new("mode-toggle");
    let platform = mock_platform(fixture.path());
    let mut state = new_user_home_state();
    open_editor_from_home(&mut state, &platform);
    type_text(&mut state, &platform, "标题");
    state.apply_input_with_platform(ctrl_alt('2'), &platform);

    let rich = state.to_editor_view_model();
    assert_eq!(rich.mode, EditorMode::Rich);
    assert_eq!(rich.source, None);
    assert!(rich.rich_cursor.is_some());
    assert!(matches!(
        rich.render_blocks(),
        [EditorRenderBlock::Heading { level: 2, .. }]
    ));
    assert!(!rich.render_blocks().is_empty());

    state.apply_input_with_platform(ctrl_shift('m'), &platform);
    let source = state.to_editor_view_model();
    assert_eq!(source.mode, EditorMode::Source);
    assert_eq!(source.source_lines.join("\n"), "## 标题");

    state.apply_input_with_platform(ctrl_shift('m'), &platform);
    let rich_again = state.to_editor_view_model();
    assert_eq!(rich_again.mode, EditorMode::Rich);
    assert_eq!(rich_again.source, None);
    assert_eq!(rich_again.word_count, rich.word_count);
    assert!(matches!(
        rich_again.render_blocks(),
        [EditorRenderBlock::Heading { level: 2, .. }]
    ));
}

#[test]
fn rich_space_and_enter_update_the_projection_immediately() {
    let fixture = FixtureRoot::new("rich-whitespace-projection");
    let platform = mock_platform(fixture.path());
    let mut state = new_user_home_state();
    open_editor_from_home(&mut state, &platform);

    type_text(&mut state, &platform, "a ");
    let spaced = state.to_editor_view_model();
    assert_eq!(spaced.source, None);
    let spaced_cursor = spaced.rich_cursor.expect("Rich cursor");
    assert_eq!(spaced_cursor.grapheme_offset, 2);
    let spaced_layout = current_editor_layout(&state);
    assert_eq!(
        spaced_layout.visual_position_for_document(EditorDocumentPosition::Rich(spaced_cursor)),
        Some(EditorTextPosition::new(0, 2))
    );

    state.apply_input_with_platform(InputEvent::from_key_label("Enter"), &platform);
    let newline = state.to_editor_view_model();
    assert_eq!(newline.source, None);
    let newline_cursor = newline.rich_cursor.expect("Rich cursor after newline");
    assert_eq!(newline_cursor.container_id, spaced_cursor.container_id);
    assert_eq!(newline_cursor.grapheme_offset, 3);
    let newline_layout = current_editor_layout(&state);
    assert_eq!(newline_layout.document_line_count, 2);
    assert_eq!(
        newline_layout.visual_position_for_document(EditorDocumentPosition::Rich(newline_cursor)),
        Some(EditorTextPosition::new(1, 0))
    );
    let blank_hit = newline_layout
        .hit_test_document(newline_layout.canvas.x, newline_layout.canvas.y + 1)
        .expect("trailing blank line is mapped");
    assert_eq!(
        blank_hit.position,
        EditorDocumentPosition::Rich(newline_cursor)
    );
    assert!(blank_hit.editable);

    state.apply_input_with_platform(InputEvent::from_key_label("Enter"), &platform);
    type_text(&mut state, &platform, "b");
    let third_line = state.to_editor_view_model();
    assert_eq!(third_line.source, None);
    let third_cursor = third_line.rich_cursor.expect("third-line Rich cursor");
    assert_eq!(third_cursor.grapheme_offset, 5);
    let third_layout = current_editor_layout(&state);
    assert_eq!(third_layout.document_line_count, 3);
    assert_eq!(
        third_layout.visual_position_for_document(EditorDocumentPosition::Rich(third_cursor)),
        Some(EditorTextPosition::new(2, 1))
    );
}

#[test]
fn enter_after_heading_starts_an_independent_paragraph() {
    let fixture = FixtureRoot::new("heading-enter-paragraph");
    let platform = mock_platform(fixture.path());
    let mut state = new_user_home_state();
    open_editor_from_home(&mut state, &platform);

    type_text(&mut state, &platform, "Heading");
    state.apply_input_with_platform(ctrl_alt('1'), &platform);
    state.apply_input_with_platform(InputEvent::from_key_label("Enter"), &platform);
    type_text(&mut state, &platform, "Body");

    let rich = state.to_editor_view_model();
    assert!(matches!(
        rich.render_blocks(),
        [
            EditorRenderBlock::Heading { level: 1, .. },
            EditorRenderBlock::Paragraph(_)
        ]
    ));

    state.apply_input_with_platform(ctrl_alt('2'), &platform);
    assert!(matches!(
        state.to_editor_view_model().render_blocks(),
        [
            EditorRenderBlock::Heading { level: 1, .. },
            EditorRenderBlock::Heading { level: 2, .. }
        ]
    ));

    let layout = current_editor_layout(&state);
    let paragraph = layout
        .toolbar_items
        .iter()
        .find(|item| item.action == EditorToolbarAction::ParagraphStyle)
        .expect("paragraph toolbar item");
    state.apply_input_with_platform(
        InputEvent::mouse_down(PointerButton::Left, (paragraph.area.x, paragraph.area.y)),
        &platform,
    );
    assert!(matches!(
        state.to_editor_view_model().render_blocks(),
        [
            EditorRenderBlock::Heading { level: 1, .. },
            EditorRenderBlock::Paragraph(_)
        ]
    ));

    state.apply_input_with_platform(ctrl_shift('m'), &platform);
    let source_lines = state.to_editor_view_model().source_lines;
    assert_eq!(source_lines.first().map(String::as_str), Some("# Heading"));
    assert_eq!(source_lines.last().map(String::as_str), Some("Body"));
    assert_eq!(
        source_lines
            .iter()
            .filter(|line| line.starts_with("# "))
            .count(),
        1
    );
}

#[test]
fn rich_trailing_space_uses_terminal_cell_width_for_the_caret() {
    let fixture = FixtureRoot::new("rich-wide-space-projection");
    let platform = mock_platform(fixture.path());
    let mut state = new_user_home_state();
    open_editor_from_home(&mut state, &platform);

    type_text(&mut state, &platform, "好 ");

    let cursor = state
        .to_editor_view_model()
        .rich_cursor
        .expect("Rich cursor");
    assert_eq!(
        current_editor_layout(&state)
            .visual_position_for_document(EditorDocumentPosition::Rich(cursor)),
        Some(EditorTextPosition::new(0, 3))
    );
}

#[test]
fn collapsed_inline_toolbar_action_keeps_the_document_stable_and_restores_the_caret() {
    let fixture = FixtureRoot::new("collapsed-toolbar");
    let platform = mock_platform(fixture.path());
    let mut state = new_user_home_state();
    open_editor_from_home(&mut state, &platform);
    let layout = current_editor_layout(&state);
    let bold = layout
        .toolbar_items
        .iter()
        .find(|item| item.action == EditorToolbarAction::Bold)
        .expect("Bold toolbar item");

    state.apply_input_with_platform(
        InputEvent::mouse_down(PointerButton::Left, (bold.area.x, bold.area.y)),
        &platform,
    );

    let after_click = state.to_editor_view_model();
    assert_eq!(after_click.source, None);
    assert_eq!(after_click.focus, EditorFocus::Canvas);
    assert!(rendered_text(&after_click).is_empty());
    assert!(
        after_click
            .status_message
            .as_deref()
            .is_some_and(|message| { message.contains("Select text") })
    );

    type_text(&mut state, &platform, "x");
    let typed = state.to_editor_view_model();
    assert_eq!(typed.source, None);
    assert_eq!(rendered_text(&typed), "x");
}

#[test]
fn inline_code_toolbar_formats_only_the_caret_line_without_a_selection() {
    let fixture = FixtureRoot::new("caret-line-inline-code");
    let platform = mock_platform(fixture.path());
    let mut state = new_user_home_state();
    open_editor_from_home(&mut state, &platform);
    type_text(&mut state, &platform, "one");
    state.apply_input_with_platform(InputEvent::from_key_label("Enter"), &platform);
    type_text(&mut state, &platform, "two");
    state.apply_input_with_platform(InputEvent::from_key_label("Enter"), &platform);
    type_text(&mut state, &platform, "three");

    let layout = current_editor_layout(&state);
    let code = layout
        .toolbar_items
        .iter()
        .find(|item| item.action == EditorToolbarAction::InlineCode)
        .expect("Code toolbar item");
    state.apply_input_with_platform(
        InputEvent::mouse_down(PointerButton::Left, (code.area.x, code.area.y)),
        &platform,
    );

    let rich = state.to_editor_view_model();
    let spans = rich
        .render_blocks()
        .iter()
        .flat_map(|block| match block {
            EditorRenderBlock::Paragraph(spans) => spans.as_slice(),
            _ => &[],
        })
        .collect::<Vec<_>>();
    assert!(
        spans
            .iter()
            .any(|span| span.text == "three" && span.inline_code)
    );
    assert!(
        spans
            .iter()
            .filter(|span| span.text == "one" || span.text == "two")
            .all(|span| !span.inline_code)
    );
    assert!(
        !rich
            .status_message
            .as_deref()
            .is_some_and(|message| message.contains("Select text"))
    );

    state.apply_input_with_platform(ctrl_shift('m'), &platform);
    assert_eq!(
        state.to_editor_view_model().source_lines.join("\n"),
        "one\ntwo\n`three`"
    );
}

#[test]
fn selected_text_formats_from_the_toolbar_without_exposing_markers_in_rich_view() {
    let fixture = FixtureRoot::new("selected-toolbar-format");
    let platform = mock_platform(fixture.path());
    let mut state = new_user_home_state();
    open_editor_from_home(&mut state, &platform);
    type_text(&mut state, &platform, "text");
    state.apply_input_with_platform(ctrl('a'), &platform);
    let layout = current_editor_layout(&state);
    let bold = layout
        .toolbar_items
        .iter()
        .find(|item| item.action == EditorToolbarAction::Bold)
        .expect("Bold toolbar item");

    state.apply_input_with_platform(
        InputEvent::mouse_down(PointerButton::Left, (bold.area.x, bold.area.y)),
        &platform,
    );

    let editor = state.to_editor_view_model();
    assert_eq!(editor.source, None);
    assert_eq!(editor.selection_offsets, None);
    assert_eq!(editor.rich_selection, None);
    assert_eq!(editor.focus, EditorFocus::Canvas);
    let rich_text = editor
        .render_blocks()
        .iter()
        .flat_map(|block| match block {
            tundra_ui::EditorRenderBlock::Paragraph(spans) => spans.as_slice(),
            _ => &[],
        })
        .find(|span| span.text == "text")
        .expect("rendered text span");
    assert!(rich_text.bold);

    state.apply_input_with_platform(ctrl_shift('m'), &platform);
    assert_eq!(
        state.to_editor_view_model().source_lines.join("\n"),
        "**text**"
    );
}

#[test]
fn navigation_delete_space_and_newline_after_bold_never_expose_markdown_or_leave_rich_mode() {
    let fixture = FixtureRoot::new("bold-then-whitespace");
    let platform = mock_platform(fixture.path());
    let mut state = new_user_home_state();
    open_editor_from_home(&mut state, &platform);
    type_text(&mut state, &platform, "Codex");
    state.apply_input_with_platform(ctrl('a'), &platform);
    state.apply_input_with_platform(ctrl('b'), &platform);

    // Exercise edits at a position inside a formatted run. The caret is a
    // grapheme position in the Rich model, never an offset into `**...**`.
    state.apply_input_with_platform(InputEvent::from_key_label("Left"), &platform);
    state.apply_input_with_platform(InputEvent::from_key_label("Backspace"), &platform);
    type_text(&mut state, &platform, "e");
    state.apply_input_with_platform(InputEvent::from_key_label("End"), &platform);
    type_text(&mut state, &platform, " ");
    state.apply_input_with_platform(InputEvent::from_key_label("Enter"), &platform);
    type_text(&mut state, &platform, "next");

    let editor = state.to_editor_view_model();
    assert_eq!(editor.mode, EditorMode::Rich);
    assert_eq!(editor.source, None);
    assert_eq!(editor.selection_offsets, None);
    let rendered = rendered_text(&editor);
    assert!(rendered.contains("Codex"));
    assert!(rendered.contains("next"));
    assert!(!rendered.contains("**"));
    let cursor = editor.rich_cursor.expect("Rich cursor after editing");
    assert_eq!(
        current_editor_layout(&state)
            .visual_position_for_document(EditorDocumentPosition::Rich(cursor)),
        Some(EditorTextPosition::new(1, 4))
    );

    // Markdown is only materialized at the explicit Rich -> Source boundary.
    state.apply_input_with_platform(ctrl_shift('m'), &platform);
    let markdown = state.to_editor_view_model().source_lines.join("\n");
    assert_eq!(markdown.replace("**", ""), "Codex \nnext");
}

#[test]
fn source_mode_rejects_markdown_toolbar_and_shortcut_actions() {
    let fixture = FixtureRoot::new("source-format-gate");
    let platform = mock_platform(fixture.path());
    let mut state = new_user_home_state();
    open_editor_from_home(&mut state, &platform);
    type_text(&mut state, &platform, "source text");
    state.apply_input_with_platform(ctrl_shift('m'), &platform);
    state.apply_input_with_platform(ctrl('a'), &platform);
    let original = state.to_editor_view_model().source_lines.join("\n");

    for shortcut in [
        ctrl('b'),
        ctrl('i'),
        ctrl('k'),
        ctrl_shift('x'),
        ctrl_alt('2'),
    ] {
        state.apply_input_with_platform(shortcut, &platform);
    }
    assert_eq!(
        state.to_editor_view_model().source_lines.join("\n"),
        original
    );

    let layout = current_editor_layout(&state);
    for action in [
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
    ] {
        let item = layout
            .toolbar_items
            .iter()
            .find(|item| item.action == action)
            .expect("formatting toolbar item");
        assert!(!item.enabled, "{action:?} must be disabled");
        state.apply_input_with_platform(
            InputEvent::mouse_down(PointerButton::Left, (item.area.x, item.area.y)),
            &platform,
        );
    }
    assert_eq!(
        state.to_editor_view_model().source_lines.join("\n"),
        original
    );
}

#[test]
fn menu_items_are_visible_and_dispatch_commands() {
    let fixture = FixtureRoot::new("menu-dispatch");
    let platform = mock_platform(fixture.path());
    let mut state = new_user_home_state();
    open_editor_from_home(&mut state, &platform);
    let view_menu = current_editor_layout(&state)
        .menus
        .into_iter()
        .find(|menu| menu.menu == EditorMenu::View)
        .expect("View menu");
    state.apply_input_with_platform(
        InputEvent::mouse_down(PointerButton::Left, (view_menu.area.x, view_menu.area.y)),
        &platform,
    );
    let open_layout = current_editor_layout(&state);
    assert!(open_layout.menu_popup.is_some());
    let source = open_layout
        .menu_items
        .iter()
        .find(|item| item.action == EditorMenuAction::Mode(EditorMode::Source))
        .expect("Source menu action");

    state.apply_input_with_platform(
        InputEvent::mouse_down(PointerButton::Left, (source.area.x, source.area.y)),
        &platform,
    );

    let editor = state.to_editor_view_model();
    assert_eq!(editor.mode, EditorMode::Source);
    assert_eq!(editor.open_menu, None);
    assert_eq!(editor.focus, EditorFocus::Canvas);
}

#[test]
fn rich_canvas_click_edits_a_logical_grapheme_position() {
    let fixture = FixtureRoot::new("rich-source-hit");
    let platform = mock_platform(fixture.path());
    let mut state = new_user_home_state();
    open_editor_from_home(&mut state, &platform);
    type_text(&mut state, &platform, "Title");
    state.apply_input_with_platform(ctrl_alt('1'), &platform);

    let editor = state.to_editor_view_model();
    assert_eq!(editor.source, None);
    let heading_range = editor
        .render_blocks()
        .iter()
        .find_map(|block| match block {
            EditorRenderBlock::Heading { spans, .. } => {
                spans.first().and_then(|span| span.rich_range)
            }
            _ => None,
        })
        .expect("heading logical range");
    let layout = current_editor_layout(&state);
    let visual = layout
        .visual_position_for_document(EditorDocumentPosition::Rich(heading_range.start))
        .expect("heading content Rich position is visible");
    let coordinates = (
        layout.canvas.x + u16::try_from(visual.column).expect("visual column"),
        layout.canvas.y
            + u16::try_from(visual.line.saturating_sub(layout.visible_start)).expect("visual row"),
    );
    let hit = layout
        .hit_test_document(coordinates.0, coordinates.1)
        .expect("logical Rich hit");
    assert_eq!(
        hit.position,
        EditorDocumentPosition::Rich(heading_range.start)
    );
    assert!(hit.editable);
    state.apply_input_with_platform(
        InputEvent::mouse_down(PointerButton::Left, coordinates),
        &platform,
    );
    type_text(&mut state, &platform, "X");
    state.apply_input_with_platform(ctrl_shift('m'), &platform);

    assert_eq!(
        state.to_editor_view_model().source_lines.join("\n"),
        "# XTitle"
    );
}

#[test]
fn escape_closes_an_open_menu_before_closing_the_document() {
    let fixture = FixtureRoot::new("escape-menu");
    let platform = mock_platform(fixture.path());
    let mut state = new_user_home_state();
    open_editor_from_home(&mut state, &platform);

    let file_menu = current_editor_layout(&state)
        .menus
        .into_iter()
        .find(|menu| menu.menu == EditorMenu::File)
        .expect("File menu");
    state.apply_input_with_platform(
        InputEvent::mouse_down(PointerButton::Left, (file_menu.area.x, file_menu.area.y)),
        &platform,
    );
    assert_eq!(
        state.to_editor_view_model().open_menu,
        Some(EditorMenu::File)
    );

    state.apply_input_with_platform(InputEvent::from_key_label("Esc"), &platform);
    assert_eq!(state.active_screen(), ShellScreen::Editor);
    assert_eq!(state.to_editor_view_model().open_menu, None);

    state.apply_input_with_platform(InputEvent::from_key_label("Esc"), &platform);
    assert_eq!(state.active_screen(), ShellScreen::Home);
}

#[test]
fn quick_menu_requires_a_rich_selection_and_preserves_it_when_opened() {
    let fixture = FixtureRoot::new("quick-menu-gate");
    let platform = mock_platform(fixture.path());
    let mut state = new_user_home_state();
    open_editor_from_home(&mut state, &platform);

    let coordinates = editor_canvas_point(&state);
    state.apply_input_with_platform(
        InputEvent::mouse_down(PointerButton::Right, coordinates),
        &platform,
    );
    assert_eq!(state.to_editor_view_model().quick_menu, None);

    type_text(&mut state, &platform, "selected text");
    state.apply_input_with_platform(ctrl('a'), &platform);
    let selection = state
        .to_editor_view_model()
        .rich_selection
        .expect("Rich selection before opening the quick menu");
    let coordinates = open_editor_quick_menu(&mut state, &platform);
    let editor = state.to_editor_view_model();
    let menu = editor.quick_menu.expect("quick menu view model");
    assert_eq!(menu.anchor, coordinates);
    assert!(menu.block_actions_enabled);
    assert_eq!(editor.rich_selection, Some(selection));

    state.apply_input_with_platform(ctrl_shift('m'), &platform);
    state.apply_input_with_platform(ctrl('a'), &platform);
    let coordinates = editor_canvas_point(&state);
    state.apply_input_with_platform(
        InputEvent::mouse_down(PointerButton::Right, coordinates),
        &platform,
    );
    assert_eq!(state.to_editor_view_model().mode, EditorMode::Source);
    assert_eq!(state.to_editor_view_model().quick_menu, None);
}

#[test]
fn quick_menu_dispatches_inline_and_heading_formats() {
    let fixture = FixtureRoot::new("quick-menu-format");
    let platform = mock_platform(fixture.path());
    let mut bold = new_user_home_state();
    open_editor_from_home(&mut bold, &platform);
    type_text(&mut bold, &platform, "Bold");
    bold.apply_input_with_platform(ctrl('a'), &platform);
    open_editor_quick_menu(&mut bold, &platform);
    click_editor_quick_action(&mut bold, &platform, EditorQuickAction::Bold);
    assert_eq!(bold.to_editor_view_model().quick_menu, None);
    bold.apply_input_with_platform(ctrl_shift('m'), &platform);
    assert_eq!(
        bold.to_editor_view_model().source_lines.join("\n"),
        "**Bold**"
    );

    let mut heading = new_user_home_state();
    open_editor_from_home(&mut heading, &platform);
    type_text(&mut heading, &platform, "Heading");
    heading.apply_input_with_platform(ctrl('a'), &platform);
    open_editor_quick_menu(&mut heading, &platform);
    click_editor_quick_action(&mut heading, &platform, EditorQuickAction::Heading(2));
    assert_eq!(heading.to_editor_view_model().quick_menu, None);
    heading.apply_input_with_platform(ctrl_shift('m'), &platform);
    assert_eq!(
        heading.to_editor_view_model().source_lines.join("\n"),
        "## Heading"
    );
}

#[test]
fn quick_menu_popup_swallows_clicks_and_escape_scroll_and_external_click_close_it() {
    let fixture = FixtureRoot::new("quick-menu-dismiss");
    let platform = mock_platform(fixture.path());
    let mut state = new_user_home_state();
    open_editor_from_home(&mut state, &platform);
    type_text(&mut state, &platform, "keep selected");
    state.apply_input_with_platform(ctrl('a'), &platform);
    let selection = state.to_editor_view_model().rich_selection;
    let anchor = open_editor_quick_menu(&mut state, &platform);
    let popup = current_editor_layout(&state)
        .quick_menu_popup
        .expect("quick menu popup");
    assert_eq!(
        current_editor_layout(&state).hit_test(popup.x, popup.y),
        Some(EditorHitTarget::QuickMenuPopup)
    );
    state.apply_input_with_platform(
        InputEvent::mouse_down(PointerButton::Left, (popup.x, popup.y)),
        &platform,
    );
    assert!(state.to_editor_view_model().quick_menu.is_some());
    assert_eq!(state.to_editor_view_model().rich_selection, selection);

    state.apply_input_with_platform(InputEvent::from_key_label("Esc"), &platform);
    assert_eq!(state.active_screen(), ShellScreen::Editor);
    assert_eq!(state.to_editor_view_model().quick_menu, None);
    assert_eq!(state.to_editor_view_model().rich_selection, selection);
    assert!(state.to_notification_view_model().is_none());

    open_editor_quick_menu(&mut state, &platform);
    state.apply_input_with_platform(
        InputEvent::mouse_scroll(ScrollDirection::Down, anchor),
        &platform,
    );
    assert_eq!(state.to_editor_view_model().quick_menu, None);
    assert_eq!(state.to_editor_view_model().rich_selection, selection);

    open_editor_quick_menu(&mut state, &platform);
    let layout = current_editor_layout(&state);
    let outside = (layout.canvas.x, layout.canvas.y);
    assert!(matches!(
        layout.hit_test(outside.0, outside.1),
        Some(EditorHitTarget::Canvas(_))
    ));
    state.apply_input_with_platform(
        InputEvent::mouse_down(PointerButton::Left, outside),
        &platform,
    );
    assert_eq!(state.to_editor_view_model().quick_menu, None);
}

#[test]
fn repeated_command_shortcut_does_not_trigger_a_one_shot_action() {
    let fixture = FixtureRoot::new("repeat-shortcut");
    let platform = mock_platform(fixture.path());
    let mut state = new_user_home_state();
    open_editor_from_home(&mut state, &platform);

    state.apply_input_with_platform(
        InputEvent::Key(KeyInput::new(
            InputKey::Character('w'),
            InputModifiers {
                control: true,
                ..InputModifiers::none()
            },
            InputPhase::Repeat,
        )),
        &platform,
    );

    assert_eq!(state.active_screen(), ShellScreen::Editor);
}

#[test]
fn word_style_heading_and_link_shortcuts_edit_the_native_rich_model() {
    let fixture = FixtureRoot::new("format-shortcuts");
    let platform = mock_platform(fixture.path());
    let mut state = new_user_home_state();
    open_editor_from_home(&mut state, &platform);
    type_text(&mut state, &platform, "Heading");

    state.apply_input_with_platform(ctrl_alt('2'), &platform);
    state.apply_input_with_platform(ctrl_shift('m'), &platform);
    assert_eq!(
        state.to_editor_view_model().source_lines.join("\n"),
        "## Heading"
    );
    state.apply_input_with_platform(ctrl_shift('m'), &platform);

    state.apply_input_with_platform(ctrl('z'), &platform);
    state.apply_input_with_platform(ctrl('a'), &platform);
    state.apply_input_with_platform(ctrl('k'), &platform);
    let rich = state.to_editor_view_model();
    assert_eq!(rich.mode, EditorMode::Rich);
    assert_eq!(rich.source, None);
    let link = rich
        .render_blocks()
        .iter()
        .flat_map(block_spans)
        .find(|span| span.text == "Heading")
        .expect("linked Rich span");
    assert!(link.link);

    state.apply_input_with_platform(ctrl_shift('m'), &platform);
    assert_eq!(
        state.to_editor_view_model().source_lines.join("\n"),
        "[Heading](https://)"
    );
}

#[test]
fn ctrl_c_copies_selection_in_editor_instead_of_shutting_down() {
    let fixture = FixtureRoot::new("copy");
    let platform = mock_platform(fixture.path());
    let mut state = new_user_home_state();
    open_editor_from_home(&mut state, &platform);
    type_text(&mut state, &platform, "copy me");
    state.apply_input_with_platform(ctrl('a'), &platform);

    let action = state.apply_input_with_platform(InputEvent::from_key_label("Ctrl+C"), &platform);

    assert_eq!(action, ShellAction::Redraw);
    assert_eq!(state.active_screen(), ShellScreen::Editor);
    assert!(!state.shutdown_requested());
    assert!(
        platform.calls().iter().any(|call| {
            matches!(call, MockCall::WriteClipboardText(text) if text == "copy me")
        })
    );
    assert_eq!(
        state.to_editor_view_model().status_message.as_deref(),
        Some("Copied")
    );
}

#[test]
fn dirty_editor_close_can_be_cancelled_or_discarded() {
    let fixture = FixtureRoot::new("dirty-close");
    let platform = mock_platform(fixture.path());
    let mut state = new_user_home_state();
    open_editor_from_home(&mut state, &platform);
    type_text(&mut state, &platform, "unsaved");

    state.apply_input_with_platform(InputEvent::from_key_label("Esc"), &platform);
    let modal = state
        .to_notification_view_model()
        .expect("dirty-close modal");
    assert_eq!(modal.title, "Unsaved document");
    assert_eq!(
        modal
            .actions
            .iter()
            .map(|action| action.label.as_str())
            .collect::<Vec<_>>(),
        vec!["Save", "Discard", "Cancel"]
    );

    state.apply_input_with_platform(InputEvent::from_key_label("Esc"), &platform);
    assert_eq!(state.active_screen(), ShellScreen::Editor);
    assert!(state.to_notification_view_model().is_none());
    assert!(state.to_editor_view_model().dirty);

    state.apply_input_with_platform(InputEvent::from_key_label("Esc"), &platform);
    state.apply_input_with_platform(InputEvent::from_key_label("Tab"), &platform);
    state.apply_input_with_platform(InputEvent::from_key_label("Enter"), &platform);

    assert_eq!(state.active_screen(), ShellScreen::Home);
    assert_eq!(state.screen_stack(), &[ShellScreen::Home]);
    assert!(state.to_notification_view_model().is_none());
}

#[test]
fn dirty_editor_open_requires_a_decision_and_cancel_preserves_the_buffer() {
    let fixture = FixtureRoot::new("dirty-open-cancel");
    let platform = mock_platform(fixture.path());
    let mut state = new_user_home_state();
    open_editor_from_home(&mut state, &platform);
    type_text(&mut state, &platform, "keep this unsaved text");

    state.apply_input_with_platform(ctrl('o'), &platform);

    let modal = state
        .to_notification_view_model()
        .expect("dirty-open modal");
    assert_eq!(modal.title, "Unsaved document");
    assert_eq!(
        modal
            .actions
            .iter()
            .map(|action| action.label.as_str())
            .collect::<Vec<_>>(),
        vec!["Save", "Discard", "Cancel"]
    );
    assert_eq!(state.active_screen(), ShellScreen::Editor);

    state.apply_input_with_platform(InputEvent::from_key_label("Esc"), &platform);
    state.apply_input_with_platform(ctrl_shift('m'), &platform);
    let editor = state.to_editor_view_model();
    assert_eq!(editor.source_lines.join("\n"), "keep this unsaved text");
    assert!(editor.dirty);
}

#[test]
fn discard_then_open_replaces_the_buffer_only_after_a_file_is_selected() {
    let fixture = FixtureRoot::new("dirty-open-discard");
    let platform = mock_platform(fixture.path());
    bootstrap_with_shell(&platform);
    let documents = fixture.path().join("Documents");
    let current = documents.join("note.md");
    let replacement = documents.join("other.md");
    fs::write(&current, "original").expect("seed current document");
    let mut state = logged_in_state(&platform);
    open_only_document_in_editor(&mut state, &platform);
    fs::write(&replacement, "replacement").expect("seed replacement document");
    type_text(&mut state, &platform, "local ");

    state.apply_input_with_platform(ctrl('o'), &platform);
    state.apply_input_with_platform(InputEvent::from_key_label("Tab"), &platform);
    state.apply_input_with_platform(InputEvent::from_key_label("Enter"), &platform);

    assert_eq!(state.active_screen(), ShellScreen::Explorer);
    assert_eq!(
        fs::read_to_string(&current).expect("current document remains untouched"),
        "original"
    );
    let entries = state.to_explorer_view_model().entries;
    let replacement_index = entries
        .iter()
        .position(|entry| entry.name == "other.md")
        .expect("replacement entry");
    for _ in 0..replacement_index {
        state.apply_input_with_platform(InputEvent::from_key_label("Down"), &platform);
    }
    state.apply_input_with_platform(InputEvent::from_key_label("Enter"), &platform);
    wait_for_editor_background_tasks(&mut state, &platform);

    let editor = state.to_editor_view_model();
    assert_eq!(state.active_screen(), ShellScreen::Editor);
    assert_eq!(
        editor.path_hint.as_deref(),
        Some(replacement.to_string_lossy().as_ref())
    );
    assert_eq!(editor.mode, EditorMode::Rich);
    assert_eq!(editor.source, None);
    assert_eq!(rendered_text(&editor), "replacement");
    assert!(!editor.dirty);

    state.apply_input_with_platform(ctrl_shift('m'), &platform);
    assert_eq!(
        state.to_editor_view_model().source_lines.join("\n"),
        "replacement"
    );
}

#[test]
fn save_then_open_continues_to_the_picker_after_the_save_succeeds() {
    let fixture = FixtureRoot::new("dirty-open-save");
    let platform = mock_platform(fixture.path());
    bootstrap_with_shell(&platform);
    let documents = fixture.path().join("Documents");
    let current = documents.join("note.md");
    let replacement = documents.join("other.md");
    fs::write(&current, "original").expect("seed current document");
    let mut state = logged_in_state(&platform);
    open_only_document_in_editor(&mut state, &platform);
    fs::write(&replacement, "replacement").expect("seed replacement document");
    type_text(&mut state, &platform, "saved ");

    state.apply_input_with_platform(ctrl('o'), &platform);
    state.apply_input_with_platform(InputEvent::from_key_label("Enter"), &platform);
    wait_for_editor_background_tasks(&mut state, &platform);

    assert_eq!(state.active_screen(), ShellScreen::Explorer);
    assert_eq!(
        fs::read_to_string(&current).expect("saved current document"),
        "saved original"
    );
    let entries = state.to_explorer_view_model().entries;
    let replacement_index = entries
        .iter()
        .position(|entry| entry.name == "other.md")
        .expect("replacement entry");
    for _ in 0..replacement_index {
        state.apply_input_with_platform(InputEvent::from_key_label("Down"), &platform);
    }
    state.apply_input_with_platform(InputEvent::from_key_label("Enter"), &platform);
    wait_for_editor_background_tasks(&mut state, &platform);

    let editor = state.to_editor_view_model();
    assert_eq!(state.active_screen(), ShellScreen::Editor);
    assert_eq!(
        editor.path_hint.as_deref(),
        Some(replacement.to_string_lossy().as_ref())
    );
    assert_eq!(editor.mode, EditorMode::Rich);
    assert_eq!(editor.source, None);
    assert_eq!(rendered_text(&editor), "replacement");

    state.apply_input_with_platform(ctrl_shift('m'), &platform);
    assert_eq!(
        state.to_editor_view_model().source_lines.join("\n"),
        "replacement"
    );
}

#[test]
fn pasted_markdown_syntax_remains_plain_rich_text() {
    let fixture = FixtureRoot::new("rich-paste-is-text");
    let platform = mock_platform(fixture.path());
    let mut state = new_user_home_state();
    open_editor_from_home(&mut state, &platform);
    let markdown = "| A |\n| - |\n| B |";
    state.apply_input_with_platform(InputEvent::Paste(markdown.to_string()), &platform);

    let editor = state.to_editor_view_model();
    assert_eq!(editor.mode, EditorMode::Rich);
    assert_eq!(editor.source, None);
    assert!(editor.render_blocks().iter().all(|block| {
        !matches!(
            block,
            EditorRenderBlock::Table { .. } | EditorRenderBlock::RichTable { .. }
        )
    }));
    assert_eq!(rendered_text(&editor), markdown);
    let cursor = editor.rich_cursor.expect("Rich cursor after paste");
    let layout = current_editor_layout(&state);
    assert_eq!(
        layout.visual_position_for_document(EditorDocumentPosition::Rich(cursor)),
        Some(EditorTextPosition::new(2, 5))
    );

    // The Markdown representation is created only at this explicit boundary
    // and escapes the pasted punctuation so it cannot become a GFM table.
    state.apply_input_with_platform(ctrl_shift('m'), &platform);
    let source = state.to_editor_view_model().source_lines.join("\n");
    assert!(source.contains("\\|"));
    state.apply_input_with_platform(ctrl_shift('m'), &platform);
    assert!(
        state
            .to_editor_view_model()
            .render_blocks()
            .iter()
            .all(|block| {
                !matches!(
                    block,
                    EditorRenderBlock::Table { .. } | EditorRenderBlock::RichTable { .. }
                )
            })
    );
}

#[test]
fn typed_markdown_punctuation_never_changes_rich_structure() {
    let fixture = FixtureRoot::new("rich-syntax-is-text");
    let platform = mock_platform(fixture.path());
    let mut state = new_user_home_state();
    open_editor_from_home(&mut state, &platform);
    let literal = "* **** `\n# \n---\n| A |";

    type_text(&mut state, &platform, literal);

    let rich = state.to_editor_view_model();
    assert_eq!(rich.mode, EditorMode::Rich);
    assert_eq!(rich.source, None);
    assert_eq!(rendered_text(&rich), literal);
    assert!(matches!(
        rich.render_blocks(),
        [EditorRenderBlock::Paragraph(_)]
    ));
    assert!(rich.render_blocks().iter().all(|block| {
        !matches!(
            block,
            EditorRenderBlock::Heading { .. }
                | EditorRenderBlock::HorizontalRule
                | EditorRenderBlock::CodeBlock { .. }
                | EditorRenderBlock::Table { .. }
                | EditorRenderBlock::RichTable { .. }
        )
    }));

    state.apply_input_with_platform(ctrl_shift('m'), &platform);
    let markdown = state.to_editor_view_model().source_lines.join("\n");
    for escaped in ["\\*", "\\`", "\\#", "\\-", "\\|"] {
        assert!(
            markdown.contains(escaped),
            "missing escaped literal {escaped:?}"
        );
    }
}

#[test]
fn rich_table_cells_accept_direct_input_and_column_edges_resize() {
    let fixture = FixtureRoot::new("rich-table-edit");
    let platform = mock_platform(fixture.path());
    let mut state = new_user_home_state();
    open_editor_from_home(&mut state, &platform);
    click_toolbar_action(&mut state, &platform, EditorToolbarAction::Table);

    let editor = state.to_editor_view_model();
    assert_eq!(editor.source, None);
    let (table_id, empty_cell) = editor
        .render_blocks()
        .iter()
        .find_map(|block| match block {
            EditorRenderBlock::RichTable { table_id, rows, .. } => rows
                .first()
                .and_then(|row| row.first())
                .and_then(|cell| cell.spans.first())
                .map(|span| (*table_id, span)),
            _ => None,
        })
        .expect("empty table cell");
    assert!(empty_cell.text.is_empty());
    assert_eq!(empty_cell.source_range, None);
    let empty_position = empty_cell
        .rich_range
        .expect("empty table cell insertion point")
        .start;
    let layout = current_editor_layout(&state);
    let empty_position = layout
        .visual_position_for_document(EditorDocumentPosition::Rich(empty_position))
        .expect("empty table cell maps to Rich coordinates");
    assert!(empty_position.line >= layout.visible_start);
    let empty_coordinates = (
        layout.canvas.x
            + empty_position
                .column
                .saturating_sub(layout.horizontal_scroll) as u16,
        layout.canvas.y + empty_position.line.saturating_sub(layout.visible_start) as u16,
    );
    let empty_hit = layout
        .hit_test_document(empty_coordinates.0, empty_coordinates.1)
        .expect("table cell is directly editable");
    assert!(empty_hit.editable);
    assert!(matches!(
        empty_hit.position,
        EditorDocumentPosition::Rich(_)
    ));

    state.apply_input_with_platform(
        InputEvent::mouse_down(PointerButton::Left, empty_coordinates),
        &platform,
    );
    type_text(&mut state, &platform, "old");
    let edited = state.to_editor_view_model();
    assert_eq!(edited.source, None);
    assert!(rich_table_cell_text(&edited, table_id, 0, 0).is_some_and(|text| text == "old"));

    let layout = current_editor_layout(&state);
    let first_edge = *layout
        .table_resize_handles
        .first()
        .expect("table column resize edge");
    assert_eq!(first_edge.table_id, Some(table_id));
    let drag_y = first_edge.area.y + u16::from(first_edge.area.height > 1);
    state.apply_input_with_platform(
        InputEvent::mouse_down(PointerButton::Left, (first_edge.area.x, drag_y)),
        &platform,
    );
    state.apply_input_with_platform(
        InputEvent::mouse_drag(PointerButton::Left, (first_edge.area.x + 4, drag_y)),
        &platform,
    );
    state.apply_input_with_platform(
        InputEvent::mouse_up(PointerButton::Left, (first_edge.area.x + 4, drag_y)),
        &platform,
    );

    let resized = current_editor_layout(&state)
        .table_resize_handles
        .first()
        .copied()
        .expect("resized first column edge");
    assert_eq!(resized.width, first_edge.width + 4);
    assert_eq!(resized.table_id, Some(table_id));

    let shrink_y = resized.area.y + u16::from(resized.area.height > 1);
    state.apply_input_with_platform(
        InputEvent::mouse_down(PointerButton::Left, (resized.area.x, shrink_y)),
        &platform,
    );
    state.apply_input_with_platform(
        InputEvent::mouse_drag(
            PointerButton::Left,
            (resized.area.x.saturating_sub(3), shrink_y),
        ),
        &platform,
    );
    state.apply_input_with_platform(
        InputEvent::mouse_up(
            PointerButton::Left,
            (resized.area.x.saturating_sub(3), shrink_y),
        ),
        &platform,
    );
    let shrunk = current_editor_layout(&state)
        .table_resize_handles
        .first()
        .copied()
        .expect("shrunk first column edge");
    assert_eq!(shrunk.width, first_edge.width + 1);
    assert_eq!(shrunk.table_id, Some(table_id));
    assert!(
        rich_table_cell_text(&state.to_editor_view_model(), table_id, 0, 0)
            .is_some_and(|text| text == "old")
    );

    state.apply_input_with_platform(ctrl_shift('m'), &platform);
    assert!(
        state
            .to_editor_view_model()
            .source_lines
            .join("\n")
            .contains("old")
    );
}

#[test]
fn rich_table_outer_edges_add_and_remove_columns_with_mouse_buttons() {
    let fixture = FixtureRoot::new("rich-table-edges");
    let platform = mock_platform(fixture.path());
    let mut state = new_user_home_state();
    open_editor_from_home(&mut state, &platform);
    click_toolbar_action(&mut state, &platform, EditorToolbarAction::Table);

    let table_id = rich_table_id(&state.to_editor_view_model()).expect("native Rich table ID");
    assert_eq!(
        rich_table_column_count(&state.to_editor_view_model(), table_id),
        Some(3)
    );

    let layout = current_editor_layout(&state);
    let left = layout
        .table_edge_handles
        .iter()
        .find(|handle| {
            handle.table_id == Some(table_id) && handle.edge == tundra_ui::EditorTableEdge::Left
        })
        .expect("left outer table edge");
    state.apply_input_with_platform(
        InputEvent::mouse_down(PointerButton::Left, (left.area.x, left.area.y + 1)),
        &platform,
    );
    assert_eq!(rich_table_id(&state.to_editor_view_model()), Some(table_id));
    assert_eq!(
        rich_table_column_count(&state.to_editor_view_model(), table_id),
        Some(4)
    );

    state.apply_input_with_platform(ctrl('a'), &platform);
    open_editor_quick_menu(&mut state, &platform);
    assert!(state.to_editor_view_model().quick_menu.is_some());

    let layout = current_editor_layout(&state);
    let left = layout
        .table_edge_handles
        .iter()
        .find(|handle| {
            handle.table_id == Some(table_id) && handle.edge == tundra_ui::EditorTableEdge::Left
        })
        .expect("updated left outer table edge");
    state.apply_input_with_platform(
        InputEvent::mouse_down(PointerButton::Right, (left.area.x, left.area.y + 1)),
        &platform,
    );
    assert_eq!(rich_table_id(&state.to_editor_view_model()), Some(table_id));
    assert_eq!(
        rich_table_column_count(&state.to_editor_view_model(), table_id),
        Some(3)
    );
    assert_eq!(state.to_editor_view_model().quick_menu, None);

    let layout = current_editor_layout(&state);
    let right = layout
        .table_edge_handles
        .iter()
        .find(|handle| {
            handle.table_id == Some(table_id) && handle.edge == tundra_ui::EditorTableEdge::Right
        })
        .expect("right outer table edge");
    state.apply_input_with_platform(
        InputEvent::mouse_down(PointerButton::Left, (right.area.x, right.area.y + 1)),
        &platform,
    );
    assert_eq!(rich_table_id(&state.to_editor_view_model()), Some(table_id));
    assert_eq!(
        rich_table_column_count(&state.to_editor_view_model(), table_id),
        Some(4)
    );
}

#[test]
fn new_table_exposes_an_editable_paragraph_below_it() {
    let fixture = FixtureRoot::new("rich-table-following-paragraph");
    let platform = mock_platform(fixture.path());
    let mut state = new_user_home_state();
    open_editor_from_home(&mut state, &platform);
    click_toolbar_action(&mut state, &platform, EditorToolbarAction::Table);

    let editor = state.to_editor_view_model();
    assert_eq!(editor.source, None);
    let paragraph_index = editor
        .render_blocks()
        .iter()
        .position(|block| matches!(block, EditorRenderBlock::Paragraph(_)))
        .expect("paragraph below table");
    let layout = current_editor_layout(&state);
    let paragraph_area = layout
        .block_areas
        .iter()
        .find(|area| area.block_index == paragraph_index)
        .expect("paragraph layout area");
    let coordinates = (paragraph_area.area.x, paragraph_area.area.y);
    let below = layout
        .hit_test_document(coordinates.0, coordinates.1)
        .expect("paragraph below table has a logical Rich insertion point");
    assert!(below.editable);
    assert!(matches!(below.position, EditorDocumentPosition::Rich(_)));
    state.apply_input_with_platform(
        InputEvent::mouse_down(PointerButton::Left, coordinates),
        &platform,
    );
    type_text(&mut state, &platform, "below");
    let edited = state.to_editor_view_model();
    assert_eq!(edited.source, None);
    assert!(
        matches!(edited.render_blocks().last(), Some(EditorRenderBlock::Paragraph(spans)) if spans.iter().map(|span| span.text.as_str()).collect::<String>() == "below")
    );
}

#[test]
fn explorer_opens_markdown_and_ctrl_s_saves_the_edited_document() {
    let fixture = FixtureRoot::new("open-save");
    let platform = mock_platform(fixture.path());
    bootstrap_with_shell(&platform);
    let path = fixture.path().join("Documents").join("note.md");
    fs::write(&path, "original").expect("seed markdown");
    let mut state = logged_in_state(&platform);

    open_only_document_in_editor(&mut state, &platform);
    type_text(&mut state, &platform, "edited ");
    state.apply_input_with_platform(ctrl('s'), &platform);
    wait_for_editor_background_tasks(&mut state, &platform);

    assert_eq!(state.active_screen(), ShellScreen::Editor);
    assert_eq!(
        fs::read_to_string(&path).expect("saved markdown"),
        "edited original"
    );
    let editor = state.to_editor_view_model();
    assert_eq!(
        editor.path_hint.as_deref(),
        Some(path.to_string_lossy().as_ref())
    );
    assert!(!editor.dirty);
    assert!(
        editor
            .status_message
            .as_deref()
            .is_some_and(|message| message.starts_with("Saved "))
    );
}

#[test]
fn save_locks_editor_input_until_the_background_write_finishes() {
    let fixture = FixtureRoot::new("save-locks-input");
    let platform = mock_platform(fixture.path());
    bootstrap_with_shell(&platform);
    let path = fixture.path().join("Documents").join("notes.txt");
    fs::write(&path, "original").expect("seed text document");
    let mut state = logged_in_state(&platform);
    open_only_document_in_editor(&mut state, &platform);
    type_text(&mut state, &platform, "saved ");

    state.apply_input_with_platform(ctrl('s'), &platform);
    assert!(
        state
            .to_editor_view_model()
            .status_message
            .as_deref()
            .is_some_and(|message| message.starts_with("Saving"))
    );
    type_text(&mut state, &platform, "ignored ");
    wait_for_editor_background_tasks(&mut state, &platform);

    assert_eq!(
        fs::read_to_string(&path).expect("saved text document"),
        "saved original"
    );
    let editor = state.to_editor_view_model();
    assert_eq!(editor.source_lines.join("\n"), "saved original");
    assert!(!editor.dirty);
}

#[test]
fn log_name_variants_open_read_only_at_the_document_bottom() {
    for (case, file_name) in [
        ("readonly-log", "service.log"),
        ("readonly-uppercase-log", "service.LOG"),
        ("readonly-rotated-log", "service.log.1"),
    ] {
        let fixture = FixtureRoot::new(case);
        let platform = mock_platform(fixture.path());
        bootstrap_with_shell(&platform);
        let contents = (0..100)
            .map(|line| format!("line {line}"))
            .collect::<Vec<_>>()
            .join("\n");
        fs::write(fixture.path().join("Documents").join(file_name), contents).expect("seed log");
        let mut state = logged_in_state(&platform);

        open_only_document_in_editor(&mut state, &platform);

        let editor = state.to_editor_view_model();
        assert_eq!(editor.mode, EditorMode::Source);
        assert!(editor.read_only, "{file_name} must be read-only");
        assert!(editor.reload_available, "{file_name} must support reload");
        assert_eq!(editor.cursor.map(|cursor| cursor.line), Some(99));
        assert_eq!(
            editor.source_lines.last().map(String::as_str),
            Some("line 99")
        );
        assert!(editor.source_lines.iter().any(|line| line == "line 90"));
        let layout = current_editor_layout(&state);
        assert_eq!(
            layout.visible_start,
            layout
                .document_line_count
                .saturating_sub(layout.visible_capacity)
        );

        type_text(&mut state, &platform, "x");
        let unchanged = state.to_editor_view_model();
        assert_eq!(
            unchanged.source_lines.last().map(String::as_str),
            Some("line 99")
        );
        assert!(!unchanged.dirty);
    }
}

#[test]
fn log_snapshot_changes_only_after_r_reload() {
    let fixture = FixtureRoot::new("log-reload-snapshot");
    let platform = mock_platform(fixture.path());
    bootstrap_with_shell(&platform);
    let path = fixture.path().join("Documents").join("service.log");
    fs::write(&path, "first\nsecond").expect("seed log");
    let mut state = logged_in_state(&platform);
    open_only_document_in_editor(&mut state, &platform);

    assert_eq!(
        state.to_editor_view_model().source_lines.join("\n"),
        "first\nsecond"
    );
    fs::write(&path, "first\nsecond\nthird").expect("append log snapshot");
    for _ in 0..3 {
        state.apply_input_with_platform(InputEvent::Tick, &platform);
        std::thread::yield_now();
    }
    assert_eq!(
        state.to_editor_view_model().source_lines.join("\n"),
        "first\nsecond",
        "an open log must not follow file changes automatically"
    );

    state.apply_input_with_platform(InputEvent::from_key_label("R"), &platform);
    assert!(
        state
            .to_editor_view_model()
            .status_message
            .as_deref()
            .is_some_and(|message| message.starts_with("Reloading"))
    );
    wait_for_editor_background_tasks(&mut state, &platform);

    let reloaded = state.to_editor_view_model();
    assert!(reloaded.read_only);
    assert!(reloaded.reload_available);
    assert!(reloaded.source_lines.join("\n").ends_with("third"));
    assert_eq!(reloaded.cursor.map(|cursor| cursor.line), Some(2));
}

#[test]
fn non_log_text_document_remains_editable() {
    let fixture = FixtureRoot::new("editable-text");
    let platform = mock_platform(fixture.path());
    bootstrap_with_shell(&platform);
    fs::write(
        fixture.path().join("Documents").join("notes.txt"),
        "editable",
    )
    .expect("seed text document");
    let mut state = logged_in_state(&platform);
    open_only_document_in_editor(&mut state, &platform);

    let opened = state.to_editor_view_model();
    assert!(!opened.read_only);
    assert!(!opened.reload_available);
    type_text(&mut state, &platform, "x");

    let edited = state.to_editor_view_model();
    assert_eq!(edited.source_lines.join("\n"), "xeditable");
    assert!(edited.dirty);
}

#[test]
fn escape_cancels_an_in_flight_large_file_open() {
    let fixture = FixtureRoot::new("cancel-large-open");
    let platform = mock_platform(fixture.path());
    bootstrap_with_shell(&platform);
    fs::write(
        fixture.path().join("Documents").join("large.txt"),
        vec![b'x'; 8 * 1024 * 1024],
    )
    .expect("seed large log");
    let mut state = logged_in_state(&platform);
    state.apply_input_with_platform(InputEvent::from_key_label("e"), &platform);
    assert_eq!(state.active_screen(), ShellScreen::Explorer);

    state.apply_input_with_platform(InputEvent::from_key_label("Enter"), &platform);
    assert!(
        state
            .to_editor_view_model()
            .status_message
            .as_deref()
            .is_some_and(|message| message.starts_with("Loading"))
    );
    state.apply_input_with_platform(InputEvent::from_key_label("Esc"), &platform);
    assert_eq!(state.active_screen(), ShellScreen::Explorer);
    wait_for_editor_background_tasks(&mut state, &platform);
    assert_eq!(state.active_screen(), ShellScreen::Explorer);
}

#[test]
fn editor_rejects_documents_above_the_one_gibibyte_limit_without_allocating_them() {
    let fixture = FixtureRoot::new("open-size-limit");
    let platform = mock_platform(fixture.path());
    bootstrap_with_shell(&platform);
    let path = fixture.path().join("Documents").join("oversized.txt");
    let file = fs::OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(&path)
        .expect("create sparse oversized document");
    file.set_len(tundra_platform::MAX_DOCUMENT_BYTES + 1)
        .expect("size sparse oversized document");
    let mut state = logged_in_state(&platform);

    state.apply_input_with_platform(InputEvent::from_key_label("e"), &platform);
    state.apply_input_with_platform(InputEvent::from_key_label("Enter"), &platform);
    wait_for_editor_background_tasks(&mut state, &platform);

    assert_eq!(state.active_screen(), ShellScreen::Explorer);
    assert!(
        state
            .to_shell_chrome_view_model()
            .status
            .error
            .as_deref()
            .is_some_and(|message| {
                message.contains("too large") && message.contains("1073741824")
            })
    );
}

#[test]
fn closing_editor_returns_to_the_original_explorer_for_markdown_and_text_files() {
    for (case, file_name, contents, expected_mode) in [
        (
            "close-to-explorer-markdown",
            "note.md",
            "# Title",
            EditorMode::Rich,
        ),
        (
            "close-to-explorer-text",
            "note.txt",
            "plain text",
            EditorMode::Source,
        ),
    ] {
        let fixture = FixtureRoot::new(case);
        let platform = mock_platform(fixture.path());
        bootstrap_with_shell(&platform);
        fs::write(fixture.path().join("Documents").join(file_name), contents)
            .expect("seed editor document");
        let mut state = logged_in_state(&platform);

        state.apply_input_with_platform(InputEvent::from_key_label("e"), &platform);
        assert_eq!(state.active_screen(), ShellScreen::Explorer);
        let explorer_before = state.to_explorer_view_model();
        let entry_names_before = explorer_before
            .entries
            .iter()
            .map(|entry| entry.name.clone())
            .collect::<Vec<_>>();

        state.apply_input_with_platform(InputEvent::from_key_label("Enter"), &platform);
        wait_for_editor_background_tasks(&mut state, &platform);
        assert_eq!(state.active_screen(), ShellScreen::Editor);
        assert_eq!(state.to_editor_view_model().mode, expected_mode);

        state.apply_input_with_platform(ctrl('w'), &platform);

        assert_eq!(state.active_screen(), ShellScreen::Explorer);
        assert_eq!(state.focused_component(), ShellComponent::Explorer);
        assert_eq!(
            state.screen_stack(),
            &[ShellScreen::Home, ShellScreen::Explorer]
        );
        let explorer_after = state.to_explorer_view_model();
        assert_eq!(explorer_after.current_path, explorer_before.current_path);
        assert_eq!(
            explorer_after.selected_index,
            explorer_before.selected_index
        );
        assert_eq!(
            explorer_after
                .entries
                .iter()
                .map(|entry| entry.name.clone())
                .collect::<Vec<_>>(),
            entry_names_before
        );
    }
}

#[test]
fn save_refuses_to_overwrite_a_file_changed_outside_the_editor() {
    let fixture = FixtureRoot::new("external-change");
    let platform = mock_platform(fixture.path());
    bootstrap_with_shell(&platform);
    let path = fixture.path().join("Documents").join("note.md");
    fs::write(&path, "original").expect("seed markdown");
    let mut state = logged_in_state(&platform);
    open_only_document_in_editor(&mut state, &platform);
    type_text(&mut state, &platform, "local ");
    fs::write(&path, "external").expect("external update");

    state.apply_input_with_platform(ctrl('s'), &platform);
    wait_for_editor_background_tasks(&mut state, &platform);

    assert_eq!(
        fs::read_to_string(&path).expect("external contents"),
        "external"
    );
    let editor = state.to_editor_view_model();
    assert!(editor.dirty);
    assert!(
        editor
            .status_message
            .as_deref()
            .is_some_and(|message| message.contains("changed outside"))
    );
}

#[test]
fn save_as_does_not_clobber_an_existing_document() {
    let fixture = FixtureRoot::new("save-as-no-clobber");
    let platform = mock_platform(fixture.path());
    bootstrap_with_shell(&platform);
    let existing = fixture.path().join("Documents").join("taken.md");
    fs::write(&existing, "keep me").expect("seed existing file");
    let mut state = logged_in_state(&platform);
    open_editor_from_home(&mut state, &platform);
    type_text(&mut state, &platform, "new contents");

    state.apply_input_with_platform(ctrl_shift('s'), &platform);
    assert_eq!(state.active_screen(), ShellScreen::Explorer);
    type_text(&mut state, &platform, "taken.md");
    state.apply_input_with_platform(InputEvent::from_key_label("Enter"), &platform);
    wait_for_editor_background_tasks(&mut state, &platform);

    assert_eq!(
        fs::read_to_string(&existing).expect("existing file"),
        "keep me"
    );
    let editor = state.to_editor_view_model();
    assert!(editor.dirty);
    assert!(
        editor
            .status_message
            .as_deref()
            .is_some_and(|message| message.contains("changed outside"))
    );
}

#[test]
fn failed_save_keeps_the_document_dirty() {
    let fixture = FixtureRoot::new("save-failure-dirty");
    let platform = mock_platform(fixture.path());
    bootstrap_with_shell(&platform);
    let documents = fixture.path().join("Documents");
    let path = documents.join("note.md");
    fs::write(&path, "original").expect("seed markdown");
    let mut state = logged_in_state(&platform);
    open_only_document_in_editor(&mut state, &platform);
    type_text(&mut state, &platform, "local ");
    fs::remove_file(&path).expect("remove opened document");
    fs::remove_dir(&documents).expect("remove document parent");
    fs::write(&documents, "not a directory").expect("replace parent with a file");

    state.apply_input_with_platform(ctrl('s'), &platform);
    wait_for_editor_background_tasks(&mut state, &platform);

    assert!(!path.exists());
    let editor = state.to_editor_view_model();
    assert!(editor.dirty);
    assert!(editor.status_message.as_deref().is_some_and(|message| {
        message.starts_with("Could not save") || message.contains("changed outside")
    }));
}

#[test]
fn recovery_tick_never_writes_or_touches_the_open_markdown_file() {
    let fixture = FixtureRoot::new("recovery-does-not-save");
    let platform = mock_platform(fixture.path());
    bootstrap_with_shell(&platform);
    let path = fixture.path().join("Documents").join("note.md");
    let original = b"original markdown\n";
    fs::write(&path, original).expect("seed markdown");
    let before = fs::metadata(&path).expect("metadata before recovery tick");
    let mut state = logged_in_state(&platform);
    open_only_document_in_editor(&mut state, &platform);
    type_text(&mut state, &platform, "unsaved ");

    state.apply_input_at_for_test(InputEvent::Tick, Instant::now() + Duration::from_secs(3));

    assert_eq!(
        fs::read(&path).expect("markdown after recovery tick"),
        original
    );
    let after = fs::metadata(&path).expect("metadata after recovery tick");
    assert_eq!(after.len(), before.len());
    assert_eq!(after.modified().ok(), before.modified().ok());
    assert_eq!(
        fs::read_dir(path.parent().expect("document parent"))
            .expect("document directory")
            .count(),
        1,
        "recovery must not create a Markdown sidecar"
    );
    assert!(state.to_editor_view_model().dirty);
}

#[test]
fn unsaved_document_is_recovered_dirty_after_a_new_login_session() {
    let fixture = FixtureRoot::new("recovery");
    let platform = mock_platform(fixture.path());
    bootstrap_with_shell(&platform);
    let mut state = logged_in_state(&platform);
    open_editor_from_home(&mut state, &platform);
    type_text(&mut state, &platform, "需要恢复的内容");

    state.apply_input_at_for_test(InputEvent::Tick, Instant::now() + Duration::from_secs(3));
    drop(state);

    let mut restored = logged_in_state(&platform);
    open_editor_from_home(&mut restored, &platform);
    restored.apply_input_with_platform(ctrl_shift('m'), &platform);
    let editor = restored.to_editor_view_model();
    assert_eq!(editor.source_lines.join("\n"), "需要恢复的内容");
    assert!(editor.dirty);
    assert!(
        editor
            .status_message
            .as_deref()
            .is_some_and(|message| message.contains("Recovered"))
    );
}

#[test]
fn shutdown_flushes_recovery_without_waiting_for_the_autosave_tick() {
    let fixture = FixtureRoot::new("shutdown-recovery");
    let platform = mock_platform(fixture.path());
    bootstrap_with_shell(&platform);
    let mut state = logged_in_state(&platform);
    open_editor_from_home(&mut state, &platform);
    type_text(&mut state, &platform, "immediate recovery");

    assert_eq!(
        state.apply_input_with_platform(InputEvent::Shutdown, &platform),
        ShellAction::Exit
    );
    drop(state);

    let mut restored = logged_in_state(&platform);
    open_editor_from_home(&mut restored, &platform);
    restored.apply_input_with_platform(ctrl_shift('m'), &platform);
    let editor = restored.to_editor_view_model();
    assert_eq!(editor.source_lines.join("\n"), "immediate recovery");
    assert!(editor.dirty);
}

fn block_spans(block: &EditorRenderBlock) -> &[tundra_ui::EditorRenderSpan] {
    match block {
        EditorRenderBlock::Paragraph(spans)
        | EditorRenderBlock::Heading { spans, .. }
        | EditorRenderBlock::BulletListItem { spans, .. }
        | EditorRenderBlock::OrderedListItem { spans, .. }
        | EditorRenderBlock::Quote { spans, .. }
        | EditorRenderBlock::Footnote { spans, .. } => spans,
        _ => &[],
    }
}

fn rendered_text(model: &tundra_ui::EditorViewModel) -> String {
    let mut output = String::new();
    for block in model.render_blocks() {
        match block {
            EditorRenderBlock::Paragraph(_)
            | EditorRenderBlock::Heading { .. }
            | EditorRenderBlock::BulletListItem { .. }
            | EditorRenderBlock::OrderedListItem { .. }
            | EditorRenderBlock::Quote { .. }
            | EditorRenderBlock::Footnote { .. } => {
                for span in block_spans(block) {
                    output.push_str(&span.text);
                }
            }
            EditorRenderBlock::CodeBlock { lines, .. } => output.push_str(&lines.join("\n")),
            EditorRenderBlock::RawHtml(raw) => output.push_str(raw),
            EditorRenderBlock::Image { markdown } => output.push_str(markdown),
            EditorRenderBlock::HorizontalRule
            | EditorRenderBlock::Table { .. }
            | EditorRenderBlock::RichTable { .. }
            | EditorRenderBlock::Blank => {}
        }
    }
    output
}

fn click_toolbar_action(
    state: &mut ShellState,
    platform: &MockPlatform,
    action: EditorToolbarAction,
) {
    let item = current_editor_layout(state)
        .toolbar_items
        .iter()
        .find(|item| item.action == action)
        .copied()
        .unwrap_or_else(|| panic!("missing toolbar action: {action:?}"));
    assert!(item.enabled, "toolbar action is disabled: {action:?}");
    state.apply_input_with_platform(
        InputEvent::mouse_down(PointerButton::Left, (item.area.x, item.area.y)),
        platform,
    );
}

fn editor_canvas_point(state: &ShellState) -> (u16, u16) {
    let layout = current_editor_layout(state);
    let coordinates = (
        layout.canvas.x + layout.canvas.width / 2,
        layout.canvas.y + layout.canvas.height.saturating_sub(1),
    );
    assert!(matches!(
        layout.hit_test(coordinates.0, coordinates.1),
        Some(EditorHitTarget::Canvas(_))
    ));
    coordinates
}

fn open_editor_quick_menu(state: &mut ShellState, platform: &MockPlatform) -> (u16, u16) {
    let coordinates = editor_canvas_point(state);
    state.apply_input_with_platform(
        InputEvent::mouse_down(PointerButton::Right, coordinates),
        platform,
    );
    assert!(state.to_editor_view_model().quick_menu.is_some());
    coordinates
}

fn click_editor_quick_action(
    state: &mut ShellState,
    platform: &MockPlatform,
    action: EditorQuickAction,
) {
    let item = current_editor_layout(state)
        .quick_menu_items
        .iter()
        .find(|item| item.action == action)
        .copied()
        .unwrap_or_else(|| panic!("missing quick action: {action:?}"));
    assert!(item.enabled, "quick action is disabled: {action:?}");
    state.apply_input_with_platform(
        InputEvent::mouse_down(PointerButton::Left, (item.area.x, item.area.y)),
        platform,
    );
}

fn rich_table_id(model: &tundra_ui::EditorViewModel) -> Option<tundra_ui::NodeId> {
    model
        .render_blocks()
        .iter()
        .find_map(EditorRenderBlock::table_id)
}

fn rich_table_column_count(
    model: &tundra_ui::EditorViewModel,
    table_id: tundra_ui::NodeId,
) -> Option<usize> {
    model.render_blocks().iter().find_map(|block| match block {
        EditorRenderBlock::RichTable {
            table_id: candidate,
            header,
            ..
        } if *candidate == table_id => Some(header.len()),
        _ => None,
    })
}

fn rich_table_cell_text(
    model: &tundra_ui::EditorViewModel,
    table_id: tundra_ui::NodeId,
    row: usize,
    column: usize,
) -> Option<String> {
    model.render_blocks().iter().find_map(|block| match block {
        EditorRenderBlock::RichTable {
            table_id: candidate,
            rows,
            ..
        } if *candidate == table_id => rows.get(row).and_then(|row| row.get(column)).map(|cell| {
            cell.spans
                .iter()
                .map(|span| span.text.as_str())
                .collect::<String>()
        }),
        _ => None,
    })
}

fn new_user_home_state() -> ShellState {
    ShellState::new_for_home_mode(default_config(), (120, 40), ShellHomeMode::User)
}

fn current_editor_layout(state: &ShellState) -> tundra_ui::EditorLayout {
    let editor_area = match tundra_ui::compute_shell_layout(Rect::new(0, 0, 120, 40)) {
        ShellLayout::Compact(compact) => compact,
        ShellLayout::Full { main, .. } => main,
    };
    tundra_ui::editor_layout(editor_area, &state.to_editor_view_model())
}

fn open_editor_from_home(state: &mut ShellState, platform: &MockPlatform) {
    state.apply_input_with_platform(InputEvent::from_key_label("Right"), platform);
    state.apply_input_with_platform(InputEvent::from_key_label("Right"), platform);
    state.apply_input_with_platform(InputEvent::from_key_label("Enter"), platform);
}

fn open_only_document_in_editor(state: &mut ShellState, platform: &MockPlatform) {
    state.apply_input_with_platform(InputEvent::from_key_label("e"), platform);
    assert_eq!(state.active_screen(), ShellScreen::Explorer);
    assert_eq!(state.to_explorer_view_model().entries.len(), 1);
    state.apply_input_with_platform(InputEvent::from_key_label("Enter"), platform);
    wait_for_editor_background_tasks(state, platform);
    assert_eq!(state.active_screen(), ShellScreen::Editor);
}

fn wait_for_editor_background_tasks(state: &mut ShellState, platform: &MockPlatform) {
    for _ in 0..400 {
        state.apply_input_with_platform(InputEvent::Tick, platform);
        let status = state.to_editor_view_model().status_message;
        let busy = status.as_deref().is_some_and(|message| {
            ["Loading", "Reloading", "Saving"]
                .iter()
                .any(|prefix| message.starts_with(prefix))
        });
        if !busy {
            return;
        }
        std::thread::yield_now();
        std::thread::sleep(Duration::from_millis(5));
    }
    panic!(
        "Editor background task did not finish in time: screen={:?}, status={:?}",
        state.active_screen(),
        state.to_editor_view_model().status_message
    );
}

fn type_text(state: &mut ShellState, platform: &MockPlatform, text: &str) {
    for character in text.chars() {
        state
            .apply_input_with_platform(InputEvent::from_key_label(character.to_string()), platform);
    }
}

fn ctrl(character: char) -> InputEvent {
    modified_key(character, true, false)
}

fn ctrl_shift(character: char) -> InputEvent {
    modified_key(character, true, true)
}

fn ctrl_alt(character: char) -> InputEvent {
    InputEvent::Key(KeyInput::new(
        InputKey::Character(character),
        InputModifiers {
            control: true,
            alt: true,
            ..InputModifiers::none()
        },
        InputPhase::Press,
    ))
}

fn modified_key(character: char, control: bool, shift: bool) -> InputEvent {
    InputEvent::Key(KeyInput::new(
        InputKey::Character(character),
        InputModifiers {
            control,
            shift,
            ..InputModifiers::none()
        },
        InputPhase::Press,
    ))
}

fn mock_platform(base: &Path) -> MockPlatform {
    for directory in [
        "Desktop",
        "Documents",
        "Downloads",
        "Pictures",
        "Videos",
        "Music",
        "Roaming",
        "Local",
        "Temp",
    ] {
        fs::create_dir_all(base.join(directory)).expect("fixture directory");
    }
    MockPlatform::new(user_dirs(base), app_paths(base))
        .with_kind(PlatformKind::Windows)
        .with_capabilities(PlatformCapabilities::native_supported())
}

fn bootstrap_with_shell(platform: &MockPlatform) {
    let startup = prepare_shell_startup(platform, default_config()).expect("startup");
    let mut state = ShellState::new_with_startup(default_config(), (120, 40), startup);
    complete_first_run_setup(
        &mut state,
        platform,
        "AdminUser",
        "StrongPass123",
        "Recovery hint",
    );
    assert_eq!(state.active_screen(), ShellScreen::Home);
}

fn logged_in_state(platform: &MockPlatform) -> ShellState {
    let startup = prepare_shell_startup(platform, default_config()).expect("startup");
    let mut state = ShellState::new_with_startup(default_config(), (120, 40), startup);
    select_login_user(&mut state, platform, "AdminUser");
    state.apply_input_with_platform(InputEvent::from_key_label("Tab"), platform);
    type_text(&mut state, platform, "StrongPass123");
    state.apply_input_with_platform(InputEvent::from_key_label("Enter"), platform);
    assert_eq!(state.active_screen(), ShellScreen::Home);
    state
}

fn select_login_user(state: &mut ShellState, platform: &MockPlatform, username: &str) {
    assert_eq!(state.active_screen(), ShellScreen::Login);
    if state.focused_component() != ShellComponent::LoginUserList {
        state.apply_input_with_platform(InputEvent::from_key_label("Shift+Tab"), platform);
    }
    let target = state
        .to_login_view_model()
        .users
        .iter()
        .position(|user| user.username.eq_ignore_ascii_case(username))
        .unwrap_or_else(|| panic!("missing login user: {username}"));
    while state.to_login_view_model().selected_index < target {
        state.apply_input_with_platform(InputEvent::from_key_label("Down"), platform);
    }
    while state.to_login_view_model().selected_index > target {
        state.apply_input_with_platform(InputEvent::from_key_label("Up"), platform);
    }
}

fn complete_first_run_setup(
    state: &mut ShellState,
    platform: &MockPlatform,
    username: &str,
    password: &str,
    hint: &str,
) {
    assert_eq!(state.active_screen(), ShellScreen::FirstRunSetup);
    state.apply_input_with_platform(InputEvent::from_key_label("Enter"), platform);
    state.apply_input_with_platform(InputEvent::from_key_label("Enter"), platform);
    type_text(state, platform, username);
    state.apply_input_with_platform(InputEvent::from_key_label("Enter"), platform);
    type_text(state, platform, password);
    state.apply_input_with_platform(InputEvent::from_key_label("Enter"), platform);
    type_text(state, platform, password);
    state.apply_input_with_platform(InputEvent::from_key_label("Enter"), platform);
    type_text(state, platform, hint);
    state.apply_input_with_platform(InputEvent::from_key_label("Enter"), platform);
    state.apply_input_with_platform(InputEvent::from_key_label("Enter"), platform);
}

fn user_dirs(base: &Path) -> UserDirs {
    UserDirs::new(
        base.join("Desktop"),
        base.join("Documents"),
        base.join("Downloads"),
        base.join("Pictures"),
        base.join("Videos"),
        base.join("Music"),
        base.join("Roaming"),
    )
    .expect("fixture user directories")
}

fn app_paths(base: &Path) -> AppPaths {
    build_windows_app_paths(base.join("Roaming"), base.join("Local"), base.join("Temp"))
        .expect("fixture app paths")
}

struct FixtureRoot {
    path: PathBuf,
}

impl FixtureRoot {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "tundra-shell-editor-{name}-{}-{}",
            unix_millis(),
            std::process::id()
        ));
        fs::create_dir_all(&path).expect("create fixture root");
        let path = fs::canonicalize(&path).expect("canonicalize fixture root");
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for FixtureRoot {
    fn drop(&mut self) {
        let _ = cleanup_temp_path(&self.path);
    }
}

fn unix_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock after epoch")
        .as_millis()
}
