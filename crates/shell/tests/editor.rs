use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use platform::mock::{MockCall, MockPlatform};
use platform::{
    AppPaths, PlatformCapabilities, PlatformKind, UserDirs, build_windows_app_paths,
    cleanup_temp_path,
};
use ratatui::layout::Rect;
use shell::{
    HomeModeOverride, InputEvent, InputKey, InputModifiers, InputPhase, KeyInput, PointerButton,
    ShellAction, ShellComponent, ShellLaunchConfig, ShellScreen, ShellSession,
    prepare_shell_startup,
};
use ui::{
    EditorHitTarget, EditorMenu, EditorMode, EditorSettingsControl, EditorSettingsField,
    EditorToolbarAction, ShellLayout,
};

fn default_config() -> ShellLaunchConfig {
    ShellLaunchConfig {
        home_mode_override: HomeModeOverride::BuildDefault,
    }
}

#[test]
fn launcher_editor_entry_opens_a_plain_text_document() {
    let fixture = FixtureRoot::new("home-open");
    let platform = mock_platform(fixture.path());
    let mut state = new_user_home_state(&platform);

    open_editor_from_home(&mut state, &platform);

    assert_eq!(state.active_screen(), ShellScreen::Editor);
    assert_eq!(state.focused_component(), ShellComponent::Editor);
    assert_eq!(
        state.screen_stack(),
        &[
            ShellScreen::Home,
            ShellScreen::Launcher,
            ShellScreen::Editor
        ]
    );
    let editor = state.to_editor_view_model();
    assert_eq!(editor.file_name, "Untitled.txt");
    assert_eq!(editor.mode, EditorMode::Source);
    assert!(!editor.dirty);
    assert!(editor.blocks.is_empty());
    assert!(current_editor_layout(&state).modes.is_empty());
}

#[test]
fn terminal_text_sizing_capability_reaches_the_editor_view_model() {
    let fixture = FixtureRoot::new("text-sizing");
    let platform = mock_platform(fixture.path());
    let mut state = new_user_home_state(&platform);
    state.set_terminal_text_sizing_support(true);

    open_editor_from_home(&mut state, &platform);

    assert!(state.to_editor_view_model().text_sizing_protocol);
}

#[test]
fn editor_accepts_unicode_and_inserts_spaces_for_tab() {
    let fixture = FixtureRoot::new("unicode-tab");
    let platform = mock_platform(fixture.path());
    let mut state = new_user_home_state(&platform);
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
    let track_end = (scrollbar.track.right().saturating_sub(1), scrollbar.track.y);

    // Match Explorer: the track is not itself a draggable grab target.
    state.apply_input_with_platform(
        InputEvent::mouse_down(PointerButton::Left, track_end),
        &platform,
    );
    assert_eq!(state.to_editor_view_model().horizontal_scroll, 0);

    let grab = (
        scrollbar.thumb.x.saturating_add(scrollbar.thumb.width / 2),
        scrollbar.thumb.y,
    );
    let captured_track_end = (track_end.0, 0);
    state.apply_input_with_platform(InputEvent::mouse_down(PointerButton::Left, grab), &platform);
    state.apply_input_with_platform(
        InputEvent::mouse_drag(PointerButton::Left, captured_track_end),
        &platform,
    );
    state.apply_input_with_platform(
        InputEvent::mouse_up(PointerButton::Left, captured_track_end),
        &platform,
    );

    let model = state.to_editor_view_model();
    let layout = current_editor_layout(&state);
    assert_eq!(
        model.horizontal_scroll,
        model
            .horizontal_content_width
            .saturating_sub(usize::from(layout.canvas.width))
    );
}

#[test]
fn editor_vertical_scrollbar_thumb_drags_to_both_ends() {
    let fixture = FixtureRoot::new("vertical-scrollbar-drag");
    let platform = mock_platform(fixture.path());
    bootstrap_with_shell(&platform);
    let contents = (0..160)
        .map(|index| format!("line-{index:03}"))
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(fixture.path().join("Documents").join("long.log"), contents).expect("seed long log");
    let mut state = logged_in_state(&platform);
    open_only_document_in_editor(&mut state, &platform);

    let scrollbar = current_editor_layout(&state)
        .vertical_scrollbar
        .expect("vertical scrollbar");
    let initial_start = current_editor_layout(&state).visible_start;
    let track_start = (scrollbar.track.x, scrollbar.track.y);

    state.apply_input_with_platform(
        InputEvent::mouse_down(PointerButton::Left, track_start),
        &platform,
    );
    assert_eq!(current_editor_layout(&state).visible_start, initial_start);

    let grab = (
        scrollbar.thumb.x,
        scrollbar.thumb.y.saturating_add(scrollbar.thumb.height / 2),
    );
    let captured_track_start = (0, track_start.1);
    state.apply_input_with_platform(InputEvent::mouse_down(PointerButton::Left, grab), &platform);
    state.apply_input_with_platform(
        InputEvent::mouse_drag(PointerButton::Left, captured_track_start),
        &platform,
    );
    state.apply_input_with_platform(
        InputEvent::mouse_up(PointerButton::Left, captured_track_start),
        &platform,
    );
    assert_eq!(current_editor_layout(&state).visible_start, 0);

    let layout = current_editor_layout(&state);
    let scrollbar = layout
        .vertical_scrollbar
        .expect("vertical scrollbar after dragging up");
    let grab = (
        scrollbar.thumb.x,
        scrollbar.thumb.y.saturating_add(scrollbar.thumb.height / 2),
    );
    let track_end = (
        scrollbar.track.x,
        scrollbar.track.bottom().saturating_sub(1),
    );
    let captured_track_end = (0, track_end.1);
    state.apply_input_with_platform(InputEvent::mouse_down(PointerButton::Left, grab), &platform);
    state.apply_input_with_platform(
        InputEvent::mouse_drag(PointerButton::Left, captured_track_end),
        &platform,
    );
    state.apply_input_with_platform(
        InputEvent::mouse_up(PointerButton::Left, captured_track_end),
        &platform,
    );
    let layout = current_editor_layout(&state);
    assert_eq!(
        layout.visible_start,
        layout
            .document_line_count
            .saturating_sub(layout.visible_capacity)
    );
}

#[test]
fn plain_text_editor_omits_markdown_controls_and_preserves_markdown_syntax() {
    let fixture = FixtureRoot::new("plain-text-controls");
    let platform = mock_platform(fixture.path());
    let mut state = new_user_home_state(&platform);
    open_editor_from_home(&mut state, &platform);

    type_text(&mut state, &platform, "# **text**");
    state.apply_input_with_platform(ctrl('a'), &platform);
    for shortcut in [
        ctrl('b'),
        ctrl('i'),
        ctrl('k'),
        ctrl_shift('m'),
        ctrl_shift('x'),
        InputEvent::from_key_label("Ctrl+Alt+2"),
    ] {
        state.apply_input_with_platform(shortcut, &platform);
    }

    let editor = state.to_editor_view_model();
    assert_eq!(editor.mode, EditorMode::Source);
    assert_eq!(editor.source_lines.join("\n"), "# **text**");
    assert!(editor.render_blocks().is_empty());

    let layout = current_editor_layout(&state);
    assert!(layout.modes.is_empty());
    assert_eq!(
        layout
            .menus
            .iter()
            .map(|item| item.menu)
            .collect::<Vec<_>>(),
        vec![EditorMenu::File, EditorMenu::Edit, EditorMenu::Settings]
    );
    assert_eq!(
        layout
            .toolbar_items
            .iter()
            .map(|item| item.action)
            .collect::<Vec<_>>(),
        vec![
            EditorToolbarAction::New,
            EditorToolbarAction::Open,
            EditorToolbarAction::Save,
            EditorToolbarAction::Undo,
            EditorToolbarAction::Redo,
            EditorToolbarAction::Find,
        ]
    );

    let coordinates = editor_canvas_point(&state);
    state.apply_input_with_platform(
        InputEvent::mouse_down(PointerButton::Right, coordinates),
        &platform,
    );
    assert_eq!(state.to_editor_view_model().quick_menu, None);
}

#[test]
fn escape_closes_an_open_menu_before_closing_the_document() {
    let fixture = FixtureRoot::new("escape-menu");
    let platform = mock_platform(fixture.path());
    let mut state = new_user_home_state(&platform);
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
    assert_eq!(state.active_screen(), ShellScreen::Launcher);
}
#[test]
fn repeated_command_shortcut_does_not_trigger_a_one_shot_action() {
    let fixture = FixtureRoot::new("repeat-shortcut");
    let platform = mock_platform(fixture.path());
    let mut state = new_user_home_state(&platform);
    open_editor_from_home(&mut state, &platform);

    state.apply_input_with_platform(
        InputEvent::Key(KeyInput::with_phase(
            InputKey::Char('w'),
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
fn held_direction_keys_accelerate_non_linearly_with_slower_vertical_shift_selection() {
    let fixture = FixtureRoot::new("cursor-acceleration");
    let platform = mock_platform(fixture.path());
    let mut state = new_user_home_state(&platform);
    open_editor_from_home(&mut state, &platform);
    assert_eq!(state.active_screen(), ShellScreen::Editor);
    for character in "01234567890123456789\n".repeat(20).chars() {
        state.apply_input(InputEvent::from_key_label(character.to_string()));
    }
    state.apply_input(ctrl_shift('m'));
    let started_at = Instant::now();
    let shift = InputModifiers {
        shift: true,
        ..InputModifiers::none()
    };

    state.apply_input_at(
        InputEvent::Key(KeyInput::with_phase(InputKey::Up, shift, InputPhase::Press)),
        started_at,
    );
    state.apply_input_at(
        InputEvent::Key(KeyInput::with_phase(
            InputKey::Up,
            shift,
            InputPhase::Repeat,
        )),
        started_at + Duration::from_millis(5_100),
    );
    let vertical = state.to_editor_view_model();
    assert_eq!(vertical.cursor.map(|cursor| cursor.line), Some(16));
    let selection = vertical.selection.expect("accelerated Shift selection");
    assert_eq!(selection.anchor.line, 20);
    assert_eq!(selection.active.line, 16);

    state.apply_input_at(
        InputEvent::Key(KeyInput::with_phase(
            InputKey::Up,
            shift,
            InputPhase::Release,
        )),
        started_at + Duration::from_millis(5_200),
    );
    state.apply_input_at(
        InputEvent::from_key_label("Home"),
        started_at + Duration::from_millis(5_300),
    );
    state.apply_input_at(
        InputEvent::Key(KeyInput::with_phase(
            InputKey::Right,
            InputModifiers::none(),
            InputPhase::Press,
        )),
        started_at + Duration::from_secs(6),
    );
    state.apply_input_at(
        InputEvent::Key(KeyInput::with_phase(
            InputKey::Right,
            InputModifiers::none(),
            InputPhase::Repeat,
        )),
        started_at + Duration::from_millis(7_900),
    );
    assert_eq!(
        state
            .to_editor_view_model()
            .cursor
            .map(|cursor| cursor.column),
        Some(2),
        "the first two seconds should retain one-cell movement"
    );
    state.apply_input_at(
        InputEvent::Key(KeyInput::with_phase(
            InputKey::Right,
            InputModifiers::none(),
            InputPhase::Repeat,
        )),
        started_at + Duration::from_millis(8_100),
    );
    assert_eq!(
        state
            .to_editor_view_model()
            .cursor
            .map(|cursor| cursor.column),
        Some(4),
        "the quadratic curve should begin after two seconds"
    );
    state.apply_input_at(
        InputEvent::Key(KeyInput::with_phase(
            InputKey::Right,
            InputModifiers::none(),
            InputPhase::Repeat,
        )),
        started_at + Duration::from_millis(11_100),
    );
    assert_eq!(
        state
            .to_editor_view_model()
            .cursor
            .map(|cursor| cursor.column),
        Some(12),
        "horizontal movement should reach its higher eight-cell maximum"
    );
}

#[test]
fn editor_settings_restore_defaults_and_persist_saved_acceleration_values() {
    let fixture = FixtureRoot::new("cursor-acceleration-settings");
    let platform = mock_platform(fixture.path());
    bootstrap_with_shell(&platform);
    let manager = storage::StorageManager::open(app_paths(fixture.path()))
        .expect("open storage")
        .manager;
    let mut config = manager.load_config().expect("load config");
    config.editor.explorer_open_extensions = vec!["rs".to_string()];
    manager.save_config(&config).expect("save custom suffix");
    let mut state = logged_in_state(&platform);
    open_editor_from_home(&mut state, &platform);

    let settings_menu = current_editor_layout(&state)
        .menus
        .into_iter()
        .find(|menu| menu.menu == EditorMenu::Settings)
        .expect("Settings menu button");
    state.apply_input_with_platform(
        InputEvent::mouse_down(
            PointerButton::Left,
            (settings_menu.area.x, settings_menu.area.y),
        ),
        &platform,
    );
    let defaults = state
        .to_editor_view_model()
        .settings
        .expect("settings window");
    assert!(defaults.enabled);
    assert_eq!(defaults.activation_delay_ms, 2_000);
    assert_eq!(defaults.horizontal_max_step, 8);
    assert_eq!(defaults.vertical_max_step, 3);

    click_editor_setting(&mut state, &platform, EditorSettingsControl::ToggleEnabled);
    click_editor_setting(
        &mut state,
        &platform,
        EditorSettingsControl::Increase(EditorSettingsField::ActivationDelay),
    );
    click_editor_setting(
        &mut state,
        &platform,
        EditorSettingsControl::RestoreDefaults,
    );
    let restored = state
        .to_editor_view_model()
        .settings
        .expect("restored settings draft");
    assert!(restored.enabled);
    assert_eq!(restored.activation_delay_ms, 2_000);

    click_editor_setting(&mut state, &platform, EditorSettingsControl::ToggleEnabled);
    click_editor_setting(
        &mut state,
        &platform,
        EditorSettingsControl::Increase(EditorSettingsField::ActivationDelay),
    );
    click_editor_setting(&mut state, &platform, EditorSettingsControl::Save);
    assert!(state.to_editor_view_model().settings.is_none());

    let stored = storage::StorageManager::open(app_paths(fixture.path()))
        .expect("reopen storage")
        .manager
        .load_config()
        .expect("load persisted config")
        .editor;
    assert!(!stored.cursor_acceleration_enabled);
    assert_eq!(stored.cursor_acceleration_delay_ms, 2_250);
    assert_eq!(stored.cursor_acceleration_ramp_ms, 3_000);
    assert!(stored.cursor_vertical_max_step < stored.cursor_horizontal_max_step);
    assert_eq!(stored.explorer_open_extensions, vec!["rs".to_string()]);

    let mut reloaded = logged_in_state(&platform);
    open_editor_from_home(&mut reloaded, &platform);
    let settings_menu = current_editor_layout(&reloaded)
        .menus
        .into_iter()
        .find(|menu| menu.menu == EditorMenu::Settings)
        .expect("Settings menu after restart");
    reloaded.apply_input_with_platform(
        InputEvent::mouse_down(
            PointerButton::Left,
            (settings_menu.area.x, settings_menu.area.y),
        ),
        &platform,
    );
    let loaded = reloaded
        .to_editor_view_model()
        .settings
        .expect("persisted settings window");
    assert!(!loaded.enabled);
    assert_eq!(loaded.activation_delay_ms, 2_250);
    click_editor_setting(&mut reloaded, &platform, EditorSettingsControl::Cancel);

    type_text(&mut reloaded, &platform, "abcdefghijklmnopqrst");
    reloaded.apply_input_with_platform(ctrl_shift('m'), &platform);
    reloaded.apply_input_with_platform(InputEvent::from_key_label("Home"), &platform);
    let started_at = Instant::now();
    reloaded.apply_input_at(
        InputEvent::Key(KeyInput::with_phase(
            InputKey::Right,
            InputModifiers::none(),
            InputPhase::Press,
        )),
        started_at,
    );
    reloaded.apply_input_at(
        InputEvent::Key(KeyInput::with_phase(
            InputKey::Right,
            InputModifiers::none(),
            InputPhase::Repeat,
        )),
        started_at + Duration::from_secs(6),
    );
    assert_eq!(
        reloaded
            .to_editor_view_model()
            .cursor
            .map(|cursor| cursor.column),
        Some(2),
        "the persisted off switch must keep held movement at one cell per event"
    );
}

#[test]
fn ctrl_c_copies_selection_in_editor_instead_of_shutting_down() {
    let fixture = FixtureRoot::new("copy");
    let platform = mock_platform(fixture.path());
    let mut state = new_user_home_state(&platform);
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
    let mut state = new_user_home_state(&platform);
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

    assert_eq!(state.active_screen(), ShellScreen::Launcher);
    assert_eq!(
        state.screen_stack(),
        &[ShellScreen::Home, ShellScreen::Launcher]
    );
    assert!(state.to_notification_view_model().is_none());
}

#[test]
fn dirty_editor_open_requires_a_decision_and_cancel_preserves_the_buffer() {
    let fixture = FixtureRoot::new("dirty-open-cancel");
    let platform = mock_platform(fixture.path());
    let mut state = new_user_home_state(&platform);
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
    assert_eq!(editor.mode, EditorMode::Source);
    assert_eq!(editor.source_lines.join("\n"), "replacement");
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
    assert_eq!(editor.mode, EditorMode::Source);
    assert_eq!(editor.source_lines.join("\n"), "replacement");
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
    file.set_len(platform::MAX_DOCUMENT_BYTES + 1)
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
            EditorMode::Source,
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

    state.apply_input_at(InputEvent::Tick, Instant::now() + Duration::from_secs(3));

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

    state.apply_input_at(InputEvent::Tick, Instant::now() + Duration::from_secs(3));
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

fn editor_canvas_point(state: &ShellSession) -> (u16, u16) {
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

fn new_user_home_state(platform: &MockPlatform) -> ShellSession {
    bootstrap_with_shell(platform);
    logged_in_state(platform)
}

fn current_editor_layout(state: &ShellSession) -> ui::EditorLayout {
    let editor_area = match ui::compute_shell_layout(Rect::new(0, 0, 120, 40)) {
        ShellLayout::Compact(compact) => compact,
        ShellLayout::Full { main, .. } => main,
    };
    ui::editor_layout(editor_area, &state.to_editor_view_model())
}

fn click_editor_setting(
    state: &mut ShellSession,
    platform: &MockPlatform,
    expected: EditorSettingsControl,
) {
    let control = current_editor_layout(state)
        .settings
        .expect("settings layout")
        .controls
        .into_iter()
        .find(|control| control.control == expected)
        .unwrap_or_else(|| panic!("missing settings control: {expected:?}"));
    state.apply_input_with_platform(
        InputEvent::mouse_down(PointerButton::Left, (control.area.x, control.area.y)),
        platform,
    );
}

fn open_editor_from_home(state: &mut ShellSession, platform: &MockPlatform) {
    state.apply_input_with_platform(InputEvent::from_key_label("Right"), platform);
    state.apply_input_with_platform(InputEvent::from_key_label("Enter"), platform);
    assert_eq!(state.active_screen(), ShellScreen::Launcher);
    let editor_index = state
        .to_launcher_view_model()
        .items
        .iter()
        .position(|item| item.id == app::EDITOR_APPLICATION.id)
        .expect("built-in Editor application");
    state.apply_input_with_platform(InputEvent::from_key_label("Home"), platform);
    for _ in 0..editor_index {
        state.apply_input_with_platform(InputEvent::from_key_label("Right"), platform);
    }
    state.apply_input_with_platform(InputEvent::from_key_label("Enter"), platform);
}

fn open_only_document_in_editor(state: &mut ShellSession, platform: &MockPlatform) {
    state.apply_input_with_platform(InputEvent::from_key_label("e"), platform);
    assert_eq!(state.active_screen(), ShellScreen::Explorer);
    assert_eq!(state.to_explorer_view_model().entries.len(), 1);
    state.apply_input_with_platform(InputEvent::from_key_label("Enter"), platform);
    wait_for_editor_background_tasks(state, platform);
    assert_eq!(state.active_screen(), ShellScreen::Editor);
}

fn wait_for_editor_background_tasks(state: &mut ShellSession, platform: &MockPlatform) {
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

fn type_text(state: &mut ShellSession, platform: &MockPlatform, text: &str) {
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

fn modified_key(character: char, control: bool, shift: bool) -> InputEvent {
    InputEvent::Key(KeyInput::with_phase(
        InputKey::Char(character),
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
    let startup = prepare_shell_startup(platform).expect("startup");
    let mut state = ShellSession::new_with_startup(default_config(), (120, 40), startup);
    complete_first_run_setup(
        &mut state,
        platform,
        "AdminUser",
        "StrongPass123",
        "Recovery hint",
    );
    assert_eq!(state.active_screen(), ShellScreen::Home);
}

fn logged_in_state(platform: &MockPlatform) -> ShellSession {
    let startup = prepare_shell_startup(platform).expect("startup");
    let mut state = ShellSession::new_with_startup(default_config(), (120, 40), startup);
    select_login_user(&mut state, platform, "AdminUser");
    state.apply_input_with_platform(InputEvent::from_key_label("Tab"), platform);
    type_text(&mut state, platform, "StrongPass123");
    state.apply_input_with_platform(InputEvent::from_key_label("Enter"), platform);
    assert_eq!(state.active_screen(), ShellScreen::Home);
    state
}

fn select_login_user(state: &mut ShellSession, platform: &MockPlatform, username: &str) {
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
    state: &mut ShellSession,
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
    for _ in 0..5 {
        state.apply_input_with_platform(InputEvent::from_key_label("Tab"), platform);
    }
    assert_eq!(
        state.focused_component(),
        ShellComponent::SetupAppearanceSubmit
    );
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
