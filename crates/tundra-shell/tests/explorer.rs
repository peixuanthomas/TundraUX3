use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use ratatui::layout::Rect;
use tundra_platform::mock::{MockCall, MockPlatform};
use tundra_platform::{
    Platform, PlatformCapabilities, PlatformKind, UserDirs, build_windows_app_paths,
    cleanup_temp_path,
};
use tundra_shell::{
    HomeModeOverride, InputEvent, InputKey, InputModifiers, InputPhase, KeyInput, PointerButton,
    ShellCommand, ShellComponent, ShellHomeMode, ShellLaunchConfig, ShellLaunchTarget, ShellScreen,
    ShellState, ShellTerminalMode, prepare_shell_startup,
};
use tundra_storage::StorageManager;
use tundra_ui::NotificationTone;

fn default_config() -> ShellLaunchConfig {
    ShellLaunchConfig {
        terminal_mode: ShellTerminalMode::Fullscreen,
        home_mode_override: HomeModeOverride::BuildDefault,
        launch_target: ShellLaunchTarget::Home,
    }
}

#[test]
fn login_can_open_explorer_and_search_current_directory() {
    let fixture = FixtureRoot::new("open-search");
    let platform = mock_platform(fixture.path());
    bootstrap_with_shell(&platform);
    fs::write(fixture.path().join("Documents").join("alpha.txt"), "alpha").expect("alpha");
    fs::write(fixture.path().join("Documents").join("beta.txt"), "beta").expect("beta");
    let mut state = logged_in_state(&platform);

    state.apply_input_with_platform(InputEvent::from_key_label("e"), &platform);

    assert_eq!(state.active_screen(), ShellScreen::Explorer);
    assert_eq!(state.focused_component(), ShellComponent::Explorer);
    assert_eq!(state.to_explorer_view_model().entries.len(), 2);

    state.apply_input_with_platform(InputEvent::from_key_label("/"), &platform);
    type_text(&mut state, &platform, "alp");
    state.apply_input_with_platform(InputEvent::from_key_label("Enter"), &platform);

    let explorer = state.to_explorer_view_model();
    assert_eq!(explorer.entries.len(), 1);
    assert_eq!(explorer.entries[0].name, "alpha.txt");
    assert_eq!(
        explorer.search.as_ref().map(|search| search.query.as_str()),
        Some("alp")
    );
}

#[test]
fn mouse_single_click_selects_and_double_click_opens_file() {
    let fixture = FixtureRoot::new("mouse-open");
    let platform = mock_platform(fixture.path());
    bootstrap_with_shell(&platform);
    let target = fixture.path().join("Documents").join("alpha.txt");
    fs::write(&target, "alpha").expect("alpha");
    let mut state = logged_in_state(&platform);
    state.apply_input_with_platform(InputEvent::from_key_label("e"), &platform);
    let first_entry = first_entry_coordinates(&state);

    state.apply_input_with_platform(
        InputEvent::mouse_down(PointerButton::Left, first_entry),
        &platform,
    );
    assert_eq!(
        state
            .to_explorer_view_model()
            .selected_entry()
            .map(|entry| entry.name.as_str()),
        Some("alpha.txt")
    );
    state.apply_input_with_platform(
        InputEvent::mouse_down(PointerButton::Left, first_entry),
        &platform,
    );
    drive_editor_tasks_until_idle(&mut state, &platform);

    assert_eq!(state.active_screen(), ShellScreen::Editor);
    let editor = state.to_editor_view_model();
    assert_eq!(editor.file_name, "alpha.txt");
    assert_eq!(
        editor.path_hint.as_deref(),
        Some(target.to_string_lossy().as_ref())
    );
    assert_eq!(editor.source_lines.join("\n"), "alpha");
}

#[test]
fn mouse_double_click_on_first_rendered_row_does_not_open_second_entry() {
    let fixture = FixtureRoot::new("mouse-row-offset");
    let platform = mock_platform(fixture.path());
    bootstrap_with_shell(&platform);
    let alpha = fixture.path().join("Documents").join("alpha.txt");
    let beta = fixture.path().join("Documents").join("beta.txt");
    fs::write(&alpha, "alpha").expect("alpha");
    fs::write(&beta, "beta").expect("beta");
    let mut state = logged_in_state(&platform);
    state.apply_input_with_platform(InputEvent::from_key_label("e"), &platform);
    let first_entry = first_entry_coordinates(&state);

    state.apply_input_with_platform(
        InputEvent::mouse_down(PointerButton::Left, first_entry),
        &platform,
    );
    state.apply_input_with_platform(
        InputEvent::mouse_down(PointerButton::Left, first_entry),
        &platform,
    );
    drive_editor_tasks_until_idle(&mut state, &platform);

    assert_eq!(state.active_screen(), ShellScreen::Editor);
    let editor = state.to_editor_view_model();
    assert_eq!(editor.file_name, "alpha.txt");
    assert_eq!(
        editor.path_hint.as_deref(),
        Some(alpha.to_string_lossy().as_ref())
    );
    assert_eq!(editor.source_lines.join("\n"), "alpha");
}

#[test]
fn right_click_selects_explorer_entry_and_opens_context_menu() {
    let fixture = FixtureRoot::new("right-click");
    let platform = mock_platform(fixture.path());
    bootstrap_with_shell(&platform);
    fs::write(fixture.path().join("Documents").join("alpha.txt"), "alpha").expect("alpha");
    let mut state = logged_in_state(&platform);
    state.apply_input_with_platform(InputEvent::from_key_label("e"), &platform);
    let first_entry = first_entry_coordinates(&state);

    state.apply_input_with_platform(
        InputEvent::mouse_down(PointerButton::Right, first_entry),
        &platform,
    );

    assert_eq!(
        state.active_popup().map(|popup| popup.owner),
        Some(Some(ShellComponent::Explorer))
    );
    assert_eq!(
        state
            .to_explorer_view_model()
            .selected_entry()
            .map(|entry| entry.name.as_str()),
        Some("alpha.txt")
    );
}

#[test]
fn normal_directory_click_release_does_not_start_a_drag_move() {
    let fixture = FixtureRoot::new("click-release");
    let platform = mock_platform(fixture.path());
    bootstrap_with_shell(&platform);
    let folder = fixture.path().join("Documents").join("folder");
    fs::create_dir(&folder).expect("folder");
    let mut state = logged_in_state(&platform);
    state.apply_input_with_platform(InputEvent::from_key_label("e"), &platform);
    let first_entry = first_entry_coordinates(&state);

    state.apply_input_with_platform(
        InputEvent::mouse_down(PointerButton::Left, first_entry),
        &platform,
    );
    state.apply_input_with_platform(
        InputEvent::mouse_drag(PointerButton::Left, first_entry),
        &platform,
    );
    state.apply_input_with_platform(
        InputEvent::mouse_up(PointerButton::Left, first_entry),
        &platform,
    );

    let explorer = state.to_explorer_view_model();
    assert!(folder.is_dir());
    assert!(explorer.operation.is_none());
    assert!(explorer.error.is_none());
}

#[test]
fn explorer_scrollbar_thumb_drags_to_the_end_without_starting_a_file_drag() {
    let fixture = FixtureRoot::new("scrollbar-drag");
    let platform = mock_platform(fixture.path());
    bootstrap_with_shell(&platform);
    let documents = fixture.path().join("Documents");
    for index in 0..80 {
        fs::write(
            documents.join(format!("file-{index:03}.txt")),
            index.to_string(),
        )
        .expect("overflowing Explorer fixture");
    }
    let mut state = logged_in_state(&platform);
    state.apply_input_with_platform(InputEvent::from_key_label("e"), &platform);

    let area = Rect::new(0, 0, state.terminal_size().0, state.terminal_size().1);
    let tundra_ui::ShellLayout::Full { main, .. } = tundra_ui::compute_shell_layout(area) else {
        panic!("Explorer scrollbar test requires a full layout");
    };
    let layout = tundra_ui::explorer_layout(main, &state.to_explorer_view_model());
    let scrollbar = layout.scrollbar.expect("overflowing Explorer scrollbar");
    let grab = (
        scrollbar.thumb.x,
        scrollbar.thumb.y.saturating_add(scrollbar.thumb.height / 2),
    );
    let bottom = (
        scrollbar.track.x,
        scrollbar.track.bottom().saturating_sub(1),
    );

    state.apply_input_with_platform(InputEvent::mouse_down(PointerButton::Left, grab), &platform);
    state.apply_input_with_platform(
        InputEvent::mouse_drag(PointerButton::Left, bottom),
        &platform,
    );
    state.apply_input_with_platform(InputEvent::mouse_up(PointerButton::Left, bottom), &platform);

    let model = state.to_explorer_view_model();
    let final_layout = tundra_ui::explorer_layout(main, &model);
    assert!(!model.viewport_follows_focus);
    assert_eq!(
        final_layout.visible_start,
        model
            .entries
            .len()
            .saturating_sub(final_layout.visible_capacity)
    );

    let scrollbar = final_layout
        .scrollbar
        .expect("Explorer scrollbar after dragging down");
    let grab = (
        scrollbar.thumb.x,
        scrollbar.thumb.y.saturating_add(scrollbar.thumb.height / 2),
    );
    let top = (scrollbar.track.x, scrollbar.track.y);
    state.apply_input_with_platform(InputEvent::mouse_down(PointerButton::Left, grab), &platform);
    state.apply_input_with_platform(InputEvent::mouse_drag(PointerButton::Left, top), &platform);
    state.apply_input_with_platform(InputEvent::mouse_up(PointerButton::Left, top), &platform);

    let model = state.to_explorer_view_model();
    assert_eq!(tundra_ui::explorer_layout(main, &model).visible_start, 0);
    assert!(model.operation.is_none());
    assert!(model.error.is_none());
}

#[test]
fn context_menu_supports_arrow_and_enter_keyboard_activation() {
    let fixture = FixtureRoot::new("context-keyboard");
    let platform = mock_platform(fixture.path());
    bootstrap_with_shell(&platform);
    fs::write(fixture.path().join("Documents").join("alpha.txt"), "alpha").expect("alpha");
    let mut state = logged_in_state(&platform);
    state.apply_input_with_platform(InputEvent::from_key_label("e"), &platform);
    let first_entry = first_entry_coordinates(&state);
    state.apply_input_with_platform(
        InputEvent::mouse_down(PointerButton::Right, first_entry),
        &platform,
    );

    state.apply_input_with_platform(InputEvent::from_key_label("Down"), &platform);
    state.apply_input_with_platform(InputEvent::from_key_label("Enter"), &platform);

    assert!(state.active_popup().is_none());
    assert!(state.to_explorer_view_model().entry_presentations[0].cut);
}

#[test]
fn admin_batch_adds_launcher_targets_and_high_risk_launch_requires_confirmation() {
    let fixture = FixtureRoot::new("launcher-context");
    let platform = mock_platform(fixture.path());
    bootstrap_with_shell(&platform);
    let executable = fixture.path().join("Documents").join("program.exe");
    let script = fixture.path().join("Documents").join("script.cmd");
    fs::write(&executable, "program").expect("executable");
    fs::write(&script, "echo launcher").expect("script");
    let mut state = logged_in_state(&platform);

    state.apply_input_with_platform(InputEvent::from_key_label("e"), &platform);
    state.apply_input_with_platform(ctrl_key('a'), &platform);
    let first_entry = first_entry_coordinates(&state);
    state.apply_input_with_platform(
        InputEvent::mouse_down(PointerButton::Right, first_entry),
        &platform,
    );

    let explorer = state.to_explorer_view_model();
    let menu = match explorer.overlay.as_ref() {
        Some(tundra_ui::ExplorerOverlayViewModel::ContextMenu(menu)) => menu,
        other => panic!("expected Explorer context menu, got {other:?}"),
    };
    assert!(menu.items.iter().any(|item| {
        item.id == "add-to-launcher"
            && item.label == "Add to Launcher"
            && item.shortcut.as_deref() == Some("A")
            && item.enabled
    }));

    state.apply_input_with_platform(InputEvent::from_key_label("a"), &platform);
    assert!(state.active_popup().is_none());
    assert_eq!(
        state.last_command(),
        Some(&ShellCommand::ExplorerAddToLauncher)
    );
    assert_eq!(state.active_screen(), ShellScreen::Explorer);

    state.apply_input_with_platform(InputEvent::from_key_label("a"), &platform);
    assert_eq!(
        state.last_command(),
        Some(&ShellCommand::ExplorerAddToLauncher)
    );
    assert_eq!(state.active_screen(), ShellScreen::Explorer);

    state.apply_input_with_platform(InputEvent::from_key_label("Esc"), &platform);
    state.apply_input_with_platform(InputEvent::from_key_label("Right"), &platform);
    state.apply_input_with_platform(InputEvent::from_key_label("Enter"), &platform);
    assert_eq!(state.active_screen(), ShellScreen::Launcher);
    assert_eq!(state.focused_component(), ShellComponent::Launcher);
    assert_eq!(state.to_launcher_view_model().items.len(), 2);

    let launcher = state.to_launcher_view_model();
    assert_eq!(launcher.items.len(), 2);
    assert!(
        launcher
            .items
            .iter()
            .all(|item| { item.status == tundra_ui::LauncherItemStatus::Ready })
    );
    assert_eq!(
        launcher
            .toolbar
            .iter()
            .map(|button| button.action)
            .collect::<Vec<_>>(),
        vec![
            tundra_ui::LauncherToolbarAction::Remove,
            tundra_ui::LauncherToolbarAction::Reapprove,
            tundra_ui::LauncherToolbarAction::Refresh,
            tundra_ui::LauncherToolbarAction::ToggleView,
        ]
    );

    let area = Rect::new(0, 0, state.terminal_size().0, state.terminal_size().1);
    let tundra_ui::ShellLayout::Full { main, .. } = tundra_ui::compute_shell_layout(area) else {
        panic!("Launcher drag test requires the full shell layout");
    };
    let layout = tundra_ui::launcher_layout(main, &launcher);
    let source = layout.items[0].area;
    let destination = layout.items[1].area;
    let source_point = (
        source.x.saturating_add(source.width / 2),
        source.y.saturating_add(1),
    );
    let after_destination = (
        destination.right().saturating_sub(1),
        destination.y.saturating_add(1),
    );
    state.apply_input_with_platform(
        InputEvent::mouse_down(PointerButton::Left, source_point),
        &platform,
    );
    state.apply_input_with_platform(
        InputEvent::mouse_drag(PointerButton::Left, after_destination),
        &platform,
    );
    let dragging = state.to_launcher_view_model();
    assert_eq!(
        dragging
            .drop_target
            .map(tundra_ui::LauncherDropTarget::insertion_index),
        Some(2)
    );
    assert!(
        tundra_ui::launcher_layout(main, &dragging)
            .drop_indicator
            .is_some()
    );
    state.apply_input_with_platform(
        InputEvent::mouse_up(PointerButton::Left, after_destination),
        &platform,
    );
    assert_eq!(
        state
            .to_launcher_view_model()
            .items
            .iter()
            .map(|item| item.name.as_str())
            .collect::<Vec<_>>(),
        vec!["script.cmd", "program.exe"]
    );

    let launcher = state.to_launcher_view_model();
    let layout = tundra_ui::launcher_layout(main, &launcher);
    let source = layout.items[1].area;
    let destination = layout.items[0].area;
    let source_point = (
        source.x.saturating_add(source.width / 2),
        source.y.saturating_add(1),
    );
    let before_destination = (destination.x, destination.y.saturating_add(1));
    state.apply_input_with_platform(
        InputEvent::mouse_down(PointerButton::Left, source_point),
        &platform,
    );
    state.apply_input_with_platform(
        InputEvent::mouse_drag(PointerButton::Left, before_destination),
        &platform,
    );
    state.apply_input_with_platform(
        InputEvent::mouse_up(PointerButton::Left, before_destination),
        &platform,
    );
    let launcher = state.to_launcher_view_model();
    assert_eq!(
        launcher
            .items
            .iter()
            .map(|item| item.name.as_str())
            .collect::<Vec<_>>(),
        vec!["program.exe", "script.cmd"]
    );
    let storage = StorageManager::open(platform.app_paths().expect("app paths"))
        .expect("storage")
        .manager;
    assert_eq!(
        storage
            .load_config()
            .expect("Launcher order")
            .launcher
            .entries
            .iter()
            .map(|entry| entry.id.as_str())
            .collect::<Vec<_>>(),
        launcher
            .items
            .iter()
            .map(|item| item.id.as_str())
            .collect::<Vec<_>>()
    );

    state.apply_input_with_platform(InputEvent::from_key_label("v"), &platform);
    assert_eq!(
        state.to_launcher_view_model().view_mode,
        tundra_ui::LauncherViewMode::Details
    );
    state.apply_input_with_platform(InputEvent::from_key_label("End"), &platform);
    assert_eq!(
        state
            .to_launcher_view_model()
            .selected_item()
            .map(|item| item.name.as_str()),
        Some("script.cmd")
    );
    state.apply_input_with_platform(InputEvent::from_key_label("Enter"), &platform);
    assert!(state.to_launcher_view_model().confirmation.is_some());
    assert!(!platform.calls().iter().any(|call| {
        matches!(call, MockCall::OpenPath(path) if path.file_name().is_some_and(|name| name == "script.cmd"))
    }));

    state.apply_input_with_platform(InputEvent::from_key_label("Enter"), &platform);
    assert!(state.to_launcher_view_model().confirmation.is_none());
    assert!(platform.calls().iter().any(|call| {
        matches!(call, MockCall::OpenPath(path) if path.file_name().is_some_and(|name| name == "script.cmd"))
    }));

    let storage = StorageManager::open(platform.app_paths().expect("app paths"))
        .expect("storage")
        .manager;
    let persisted = storage.load_state().expect("Launcher view state");
    assert!(
        persisted
            .values
            .iter()
            .any(|(key, value)| { key.starts_with("launcher.view.") && value == "details" })
    );
}

#[test]
fn delete_key_moves_selection_to_system_trash() {
    let fixture = FixtureRoot::new("delete-trash");
    let platform = mock_platform(fixture.path());
    bootstrap_with_shell(&platform);
    let target = fixture.path().join("Documents").join("alpha.txt");
    fs::write(&target, "alpha").expect("alpha");
    let mut state = logged_in_state(&platform);
    state.apply_input_with_platform(InputEvent::from_key_label("e"), &platform);

    state.apply_input_with_platform(InputEvent::from_key_label("Delete"), &platform);
    assert!(
        state
            .to_explorer_view_model()
            .pending_dialog
            .as_ref()
            .map(|dialog| dialog.title.as_str())
            .unwrap_or_default()
            .contains("Delete")
    );
    state.apply_input_with_platform(InputEvent::from_key_label("y"), &platform);

    assert!(platform.calls().iter().any(|call| matches!(
        call,
        tundra_platform::mock::MockCall::MoveToTrash(paths)
            if paths == &vec![target.clone()]
    )));
    let explorer = state.to_explorer_view_model();
    assert!(explorer.pending_dialog.is_none());
    assert!(explorer.operation.is_none());
    assert!(
        target.exists(),
        "the mock platform must not mutate the filesystem"
    );
    let storage = prepare_shell_startup(&platform, default_config())
        .expect("startup")
        .storage_manager
        .expect("storage");
    assert!(storage.load_trash().expect("trash").records.is_empty());
}

#[test]
fn failed_system_trash_delete_reports_a_stable_operation_error() {
    let fixture = FixtureRoot::new("delete-confirm-failure");
    let platform = mock_platform(fixture.path());
    bootstrap_with_shell(&platform);
    let target = fixture.path().join("Documents").join("alpha.txt");
    fs::write(&target, "alpha").expect("alpha");
    let mut state = logged_in_state(&platform);
    state.apply_input_with_platform(InputEvent::from_key_label("e"), &platform);
    state.apply_input_with_platform(InputEvent::from_key_label("Delete"), &platform);
    platform.set_move_to_trash_result(Err(tundra_platform::PlatformError::Native {
        operation: "move to system Trash",
        message: "injected system Trash failure".to_string(),
    }));

    state.apply_input_with_platform(InputEvent::from_key_label("y"), &platform);
    drive_explorer_tasks_until(&mut state, &platform, |state| {
        let explorer = state.to_explorer_view_model();
        explorer.pending_dialog.is_none()
            && explorer.operation.is_none()
            && explorer.error.as_deref().is_some_and(|error| {
                error.contains("failed") || error.contains("error") || error.contains("missing")
            })
    });
    let reported_error = state
        .to_explorer_view_model()
        .error
        .expect("failed background delete should report an Explorer error");
    assert!(reported_error.contains("failed") || reported_error.contains("error"));
    assert!(state.to_notification_view_model().is_none());
    while state.take_notification_response().is_some() {}

    for input in [
        InputEvent::Key(KeyInput::new(
            InputKey::Character('y'),
            InputModifiers {
                control: true,
                ..InputModifiers::none()
            },
            InputPhase::Press,
        )),
        InputEvent::Key(KeyInput::new(
            InputKey::Enter,
            InputModifiers::none(),
            InputPhase::Repeat,
        )),
    ] {
        state.apply_input_with_platform(input, &platform);
        assert_eq!(
            state.to_explorer_view_model().error.as_deref(),
            Some(reported_error.as_str())
        );
        assert_eq!(state.take_notification_response(), None);
    }
}

#[test]
fn explorer_alert_resolves_after_success_and_close_without_clearing_unrelated_alert() {
    let fixture = FixtureRoot::new("alert-lifecycle");
    let platform = mock_platform(fixture.path());
    bootstrap_with_shell(&platform);
    let mut state = logged_in_state(&platform);
    state.notify_alert_with_key(
        "test.unrelated",
        "Unrelated warning",
        NotificationTone::Warning,
    );

    state.apply_input_with_platform(InputEvent::from_key_label("e"), &platform);
    assert_eq!(state.active_screen(), ShellScreen::Explorer);
    assert_eq!(
        state.to_shell_chrome_view_model().status.error.as_deref(),
        Some("Unrelated warning")
    );

    state.apply_input_with_platform(InputEvent::from_key_label("v"), &platform);
    let failed = state.to_shell_chrome_view_model();
    assert!(
        failed
            .status
            .error
            .as_deref()
            .is_some_and(|message| message.contains("clipboard is empty"))
    );
    assert_eq!(failed.status.alert_tone, NotificationTone::Error);

    state.apply_input_with_platform(InputEvent::from_key_label("h"), &platform);
    let recovered = state.to_shell_chrome_view_model();
    assert_eq!(recovered.status.error.as_deref(), Some("Unrelated warning"));
    assert_eq!(recovered.status.alert_tone, NotificationTone::Warning);

    state.apply_input_with_platform(InputEvent::from_key_label("v"), &platform);
    assert!(
        state
            .to_shell_chrome_view_model()
            .status
            .error
            .as_deref()
            .is_some_and(|message| message.contains("clipboard is empty"))
    );

    state.apply_input_with_platform(InputEvent::from_key_label("Esc"), &platform);
    let closed = state.to_shell_chrome_view_model();
    assert_eq!(state.active_screen(), ShellScreen::Home);
    assert_eq!(closed.status.error.as_deref(), Some("Unrelated warning"));
    assert_eq!(closed.status.alert_tone, NotificationTone::Warning);
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
    assert_eq!(state.home_mode(), ShellHomeMode::User);
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

fn type_text(state: &mut ShellState, platform: &MockPlatform, text: &str) {
    for character in text.chars() {
        state
            .apply_input_with_platform(InputEvent::from_key_label(character.to_string()), platform);
    }
}

fn ctrl_key(character: char) -> InputEvent {
    InputEvent::Key(KeyInput::new(
        InputKey::Character(character),
        InputModifiers {
            control: true,
            ..InputModifiers::none()
        },
        InputPhase::Press,
    ))
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
    for _ in 0..5 {
        state.apply_input_with_platform(InputEvent::from_key_label("Tab"), platform);
    }
    assert_eq!(
        state.focused_component(),
        ShellComponent::SetupAppearanceSubmit
    );
    state.apply_input_with_platform(InputEvent::from_key_label("Enter"), platform);
}

fn first_entry_coordinates(state: &ShellState) -> (u16, u16) {
    let area = Rect::new(0, 0, state.terminal_size().0, state.terminal_size().1);
    let tundra_ui::ShellLayout::Full { main, .. } = tundra_ui::compute_shell_layout(area) else {
        panic!("phase6 tests use a full shell layout");
    };
    let model = state.to_explorer_view_model();
    let row = tundra_ui::explorer_layout(main, &model)
        .rows
        .into_iter()
        .next()
        .expect("Explorer should render its first entry row");
    (row.area.x.saturating_add(2), row.area.y)
}

fn drive_explorer_tasks_until(
    state: &mut ShellState,
    platform: &MockPlatform,
    done: impl Fn(&ShellState) -> bool,
) {
    for _ in 0..200 {
        state.apply_input_with_platform(InputEvent::Tick, platform);
        if done(state) {
            return;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    let explorer = state.to_explorer_view_model();
    panic!(
        "Explorer background task did not finish in time: operation={:?}, dialog={:?}, error={:?}, message={:?}",
        explorer.operation, explorer.pending_dialog, explorer.error, explorer.message
    );
}

fn drive_editor_tasks_until_idle(state: &mut ShellState, platform: &MockPlatform) {
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

fn mock_platform(base: &Path) -> MockPlatform {
    let documents = base.join("Documents");
    fs::create_dir_all(&documents).expect("documents");
    let app_paths =
        build_windows_app_paths(base.join("Roaming"), base.join("Local"), base.join("Temp"))
            .expect("fixture app paths should resolve");
    MockPlatform::new(user_dirs(base), app_paths)
        .with_kind(PlatformKind::Windows)
        .with_capabilities(PlatformCapabilities::native_supported())
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
    .expect("fixture user directories should resolve")
}

struct FixtureRoot {
    path: PathBuf,
}

impl FixtureRoot {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "tundra-shell-phase6-{name}-{}-{}",
            unix_millis(),
            std::process::id()
        ));
        fs::create_dir_all(&path).expect("fixture root");
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

fn unix_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}
