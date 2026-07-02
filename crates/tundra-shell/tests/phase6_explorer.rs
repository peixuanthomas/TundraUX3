use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use ratatui::layout::Rect;
use tundra_platform::mock::{MockCall, MockPlatform};
use tundra_platform::{
    PlatformCapabilities, PlatformKind, UserDirs, build_windows_app_paths, cleanup_temp_path,
};
use tundra_shell::{
    HomeModeOverride, InputEvent, PointerButton, ShellComponent, ShellHomeMode, ShellLaunchConfig,
    ShellScreen, ShellState, ShellTerminalMode, prepare_shell_startup,
};

fn default_config() -> ShellLaunchConfig {
    ShellLaunchConfig {
        terminal_mode: ShellTerminalMode::Fullscreen,
        home_mode_override: HomeModeOverride::BuildDefault,
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

    assert_eq!(platform.calls(), vec![MockCall::OpenPath(target)]);
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

    assert_eq!(platform.calls(), vec![MockCall::OpenPath(alpha)]);
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
fn delete_key_moves_selection_to_tundra_trash() {
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

    assert!(!target.exists());
    let storage = prepare_shell_startup(&platform, default_config())
        .expect("startup")
        .storage_manager
        .expect("storage");
    assert_eq!(storage.load_trash().expect("trash").records.len(), 1);
}

fn bootstrap_with_shell(platform: &MockPlatform) {
    let startup = prepare_shell_startup(platform, default_config()).expect("startup");
    let mut state = ShellState::new_with_startup(default_config(), (120, 40), startup);
    type_text(&mut state, platform, "AdminUser");
    state.apply_input_with_platform(InputEvent::from_key_label("Tab"), platform);
    type_text(&mut state, platform, "StrongPass123");
    state.apply_input_with_platform(InputEvent::from_key_label("Enter"), platform);
    assert_eq!(state.active_screen(), ShellScreen::Home);
}

fn logged_in_state(platform: &MockPlatform) -> ShellState {
    let startup = prepare_shell_startup(platform, default_config()).expect("startup");
    let mut state = ShellState::new_with_startup(default_config(), (120, 40), startup);
    type_text(&mut state, platform, "AdminUser");
    state.apply_input_with_platform(InputEvent::from_key_label("Tab"), platform);
    type_text(&mut state, platform, "StrongPass123");
    state.apply_input_with_platform(InputEvent::from_key_label("Enter"), platform);
    assert_eq!(state.active_screen(), ShellScreen::Home);
    assert_eq!(state.home_mode(), ShellHomeMode::User);
    state
}

fn type_text(state: &mut ShellState, platform: &MockPlatform, text: &str) {
    for character in text.chars() {
        state
            .apply_input_with_platform(InputEvent::from_key_label(character.to_string()), platform);
    }
}

fn first_entry_coordinates(state: &ShellState) -> (u16, u16) {
    let area = Rect::new(0, 0, state.terminal_size().0, state.terminal_size().1);
    let tundra_ui::ShellLayout::Full { main, .. } = tundra_ui::compute_shell_layout(area) else {
        panic!("phase6 tests use a full shell layout");
    };
    let content_width = main.width.saturating_sub(2);
    let content_line = tundra_ui::explorer_first_entry_content_line(
        &state.to_explorer_view_model(),
        content_width,
    ) as u16;
    (
        main.x.saturating_add(3),
        main.y.saturating_add(1 + content_line),
    )
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
