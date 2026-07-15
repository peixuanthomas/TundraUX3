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
    ShellAction, ShellComponent, ShellHomeMode, ShellLaunchConfig, ShellScreen, ShellState,
    ShellTerminalMode, prepare_shell_startup,
};
use tundra_ui::{EditorMenu, EditorMode, ShellLayout};

fn default_config() -> ShellLaunchConfig {
    ShellLaunchConfig {
        terminal_mode: ShellTerminalMode::Fullscreen,
        home_mode_override: HomeModeOverride::BuildDefault,
    }
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
    assert_eq!(editor.cursor.map(|cursor| cursor.column), Some(7));
}

#[test]
fn rich_source_toggle_preserves_markdown_and_rendered_content() {
    let fixture = FixtureRoot::new("mode-toggle");
    let platform = mock_platform(fixture.path());
    let mut state = new_user_home_state();
    open_editor_from_home(&mut state, &platform);
    type_text(&mut state, &platform, "# 标题");

    let rich = state.to_editor_view_model();
    assert_eq!(rich.mode, EditorMode::Rich);
    assert!(!rich.blocks.is_empty());

    state.apply_input_with_platform(ctrl_shift('m'), &platform);
    let source = state.to_editor_view_model();
    assert_eq!(source.mode, EditorMode::Source);
    assert_eq!(source.source_lines.join("\n"), "# 标题");

    state.apply_input_with_platform(ctrl_shift('m'), &platform);
    let rich_again = state.to_editor_view_model();
    assert_eq!(rich_again.mode, EditorMode::Rich);
    assert_eq!(rich_again.word_count, rich.word_count);
    assert!(!rich_again.blocks.is_empty());
}

#[test]
fn rich_canvas_click_edits_the_mapped_markdown_source_offset() {
    let fixture = FixtureRoot::new("rich-source-hit");
    let platform = mock_platform(fixture.path());
    let mut state = new_user_home_state();
    open_editor_from_home(&mut state, &platform);
    type_text(&mut state, &platform, "# Title");

    let layout = current_editor_layout(&state);
    let visual = layout
        .visual_position_for_source(2)
        .expect("heading content source offset is visible");
    let coordinates = (
        layout.canvas.x + u16::try_from(visual.column).expect("visual column"),
        layout.canvas.y
            + u16::try_from(visual.line.saturating_sub(layout.visible_start)).expect("visual row"),
    );
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
fn word_style_heading_and_link_shortcuts_edit_markdown() {
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

    state.apply_input_with_platform(ctrl('z'), &platform);
    state.apply_input_with_platform(ctrl('a'), &platform);
    state.apply_input_with_platform(ctrl('k'), &platform);
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

    let editor = state.to_editor_view_model();
    assert_eq!(state.active_screen(), ShellScreen::Editor);
    assert_eq!(
        editor.path_hint.as_deref(),
        Some(replacement.to_string_lossy().as_ref())
    );
    assert_eq!(editor.source.as_deref(), Some("replacement"));
    assert!(!editor.dirty);
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

    let editor = state.to_editor_view_model();
    assert_eq!(state.active_screen(), ShellScreen::Editor);
    assert_eq!(
        editor.path_hint.as_deref(),
        Some(replacement.to_string_lossy().as_ref())
    );
    assert_eq!(editor.source.as_deref(), Some("replacement"));
}

#[test]
fn non_exact_rich_blocks_switch_to_source_before_editing() {
    let fixture = FixtureRoot::new("rich-safe-fallback");
    let platform = mock_platform(fixture.path());
    let mut state = new_user_home_state();
    open_editor_from_home(&mut state, &platform);
    let markdown = "| A |\n| - |\n| B |";
    state.apply_input_with_platform(InputEvent::Paste(markdown.to_string()), &platform);

    let layout = current_editor_layout(&state);
    let coordinates = (layout.canvas.y..layout.canvas.bottom())
        .flat_map(|y| (layout.canvas.x..layout.canvas.right()).map(move |x| (x, y)))
        .find(|(x, y)| {
            layout
                .hit_test_source(*x, *y)
                .is_some_and(|hit| !hit.editable)
        })
        .expect("table exposes a non-editable rendered cell");
    state.apply_input_with_platform(
        InputEvent::mouse_down(PointerButton::Left, coordinates),
        &platform,
    );

    let editor = state.to_editor_view_model();
    assert_eq!(editor.mode, EditorMode::Source);
    assert_eq!(editor.source_lines.join("\n"), markdown);
    assert!(editor.status_message.as_deref().is_some_and(|message| {
        message.contains("Source mode") || message.contains("source position")
    }));
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

    assert!(!path.exists());
    let editor = state.to_editor_view_model();
    assert!(editor.dirty);
    assert!(editor.status_message.as_deref().is_some_and(|message| {
        message.starts_with("Could not save") || message.contains("changed outside")
    }));
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
    assert_eq!(state.active_screen(), ShellScreen::Editor);
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
