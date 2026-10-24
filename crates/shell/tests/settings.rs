use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use identity::{SessionService, UserRole, UserService};
use platform::mock::{MockCall, MockPlatform};
use platform::{
    AppPaths, PlatformCapabilities, PlatformKind, UserDirs, build_windows_app_paths,
    cleanup_temp_path,
};
use ratatui::layout::Rect;
use shell::{
    HomeModeOverride, InputEvent, InputKey, InputModifiers, InputPhase, KeyInput,
    ShellLaunchConfig, ShellScreen, ShellSession, prepare_shell_startup,
};
use storage::{BorderShape, IconDisplayMode, MotionPreference, StorageManager, TimeSyncSource};
use ui::{SettingsApplication, SettingsCategory, SettingsField, SettingsPickerKind};

fn default_config() -> ShellLaunchConfig {
    ShellLaunchConfig {
        home_mode_override: HomeModeOverride::BuildDefault,
    }
}

#[test]
fn settings_action_repeat_is_ignored_and_shift_tab_goes_to_previous_section() {
    let fixture = FixtureRoot::new("settings-key-phases");
    let platform = mock_platform(fixture.path());
    initialize_users(&platform, false, false);
    let mut state = logged_in_state(&platform, "AdminUser", "StrongPass123");
    open_settings_from_home(&mut state, &platform);
    for phase in [InputPhase::Repeat, InputPhase::Release] {
        for key in [InputKey::Enter, InputKey::Char(' ')] {
            state.apply_input_with_platform(
                InputEvent::Key(KeyInput::with_phase(key, InputModifiers::none(), phase)),
                &platform,
            );
            assert!(state.to_settings_view_model().unwrap().picker.is_none());
        }
    }
    press(&mut state, &platform, "Shift+Tab");
    assert_eq!(
        state.to_settings_view_model().unwrap().selected_category,
        SettingsCategory::Update
    );
    press(&mut state, &platform, "Tab");
    assert_eq!(
        state.to_settings_view_model().unwrap().selected_category,
        SettingsCategory::Appearance
    );
    press(&mut state, &platform, "Enter");
    assert_eq!(
        state.to_settings_view_model().unwrap().picker.unwrap().kind,
        SettingsPickerKind::Theme
    );
    press(&mut state, &platform, "Esc");
    assert_eq!(state.active_screen(), ShellScreen::Settings);
}

#[test]
fn settings_arrows_do_not_change_saved_values() {
    let fixture = FixtureRoot::new("settings-navigation-only");
    let platform = mock_platform(fixture.path());
    let manager = initialize_users(&platform, false, false);
    let mut state = logged_in_state(&platform, "AdminUser", "StrongPass123");
    open_settings_from_home(&mut state, &platform);
    select_category(&mut state, &platform, SettingsCategory::System);
    let before = manager.load_config().unwrap();
    for key in ["Right", "Left", "Down", "Right", "Left", "Up"] {
        press(&mut state, &platform, key);
        assert_eq!(manager.load_config().unwrap(), before, "{key}");
    }
}

#[test]
fn numeric_settings_wait_for_confirmation_and_escape_discards_changes() {
    let fixture = FixtureRoot::new("settings-numeric-confirmation");
    let platform = mock_platform(fixture.path());
    let manager = initialize_users(&platform, false, false);
    let mut state = logged_in_state(&platform, "AdminUser", "StrongPass123");
    open_settings_from_home(&mut state, &platform);
    for _ in 0..5 {
        press(&mut state, &platform, "Down");
    }
    let saved_speed = || {
        manager.load_users().unwrap().users[0]
            .appearance
            .animation_speed_percent
    };
    press(&mut state, &platform, " ");
    assert!(
        state
            .to_settings_view_model()
            .unwrap()
            .number_editor
            .is_none()
    );
    assert_eq!(saved_speed(), 100);
    press(&mut state, &platform, "Enter");
    assert_eq!(saved_speed(), 100);
    press(&mut state, &platform, "Right");
    assert_eq!(saved_speed(), 100);
    press(&mut state, &platform, "Enter");
    assert_eq!(saved_speed(), 125);
    assert_eq!(
        state.to_settings_view_model().unwrap().selected_field,
        SettingsField::AnimationSpeed
    );
    for key in ["Enter", "Left", "Esc"] {
        press(&mut state, &platform, key);
    }
    assert_eq!(saved_speed(), 125);
    assert_eq!(state.active_screen(), ShellScreen::Settings);
}

#[test]
fn horizontal_navigation_moves_between_categories_applications_and_fields() {
    let fixture = FixtureRoot::new("settings-focus");
    let platform = mock_platform(fixture.path());
    let manager = initialize_users(&platform, false, false);
    let mut state = logged_in_state(&platform, "AdminUser", "StrongPass123");
    open_settings_from_home(&mut state, &platform);
    let before = manager.load_config().unwrap();
    for (key, category, field) in [
        (
            "Left",
            SettingsCategory::Appearance,
            SettingsField::CategoryList,
        ),
        (
            "Down",
            SettingsCategory::RegionTime,
            SettingsField::CategoryList,
        ),
        (
            "Down",
            SettingsCategory::System,
            SettingsField::CategoryList,
        ),
        ("Right", SettingsCategory::System, SettingsField::AutoAdmin),
        (
            "Down",
            SettingsCategory::System,
            SettingsField::AutomationEnabled,
        ),
        (
            "Left",
            SettingsCategory::System,
            SettingsField::CategoryList,
        ),
        (
            "Down",
            SettingsCategory::Applications,
            SettingsField::CategoryList,
        ),
        (
            "Right",
            SettingsCategory::Applications,
            SettingsField::ApplicationList,
        ),
        (
            "Down",
            SettingsCategory::Applications,
            SettingsField::ApplicationList,
        ),
        (
            "Right",
            SettingsCategory::Applications,
            SettingsField::ExplorerOpenExtensions,
        ),
        (
            "Down",
            SettingsCategory::Applications,
            SettingsField::CursorAcceleration,
        ),
        (
            "Left",
            SettingsCategory::Applications,
            SettingsField::ApplicationList,
        ),
        (
            "Left",
            SettingsCategory::Applications,
            SettingsField::CategoryList,
        ),
    ] {
        press(&mut state, &platform, key);
        let model = state.to_settings_view_model().unwrap();
        assert_eq!(
            (model.selected_category, model.selected_field),
            (category, field),
            "{key}"
        );
        assert_eq!(manager.load_config().unwrap(), before);
        assert!(model.picker.is_none());
        assert!(model.number_editor.is_none());
    }
}

#[test]
fn numeric_settings_validate_direct_input_before_saving() {
    let fixture = FixtureRoot::new("settings-number-input");
    let platform = mock_platform(fixture.path());
    let manager = initialize_users(&platform, false, false);
    let mut state = logged_in_state(&platform, "AdminUser", "StrongPass123");
    open_settings_from_home(&mut state, &platform);
    for (category, field, value, display) in [
        (
            SettingsCategory::System,
            SettingsField::SystemLowAvailable,
            "20",
            "20 GiB",
        ),
        (
            SettingsCategory::System,
            SettingsField::SystemLowPercentage,
            "25",
            "25%",
        ),
        (
            SettingsCategory::System,
            SettingsField::SystemCriticalAvailable,
            "3",
            "3 GiB",
        ),
        (
            SettingsCategory::System,
            SettingsField::SystemCriticalPercentage,
            "8",
            "8%",
        ),
        (
            SettingsCategory::Applications,
            SettingsField::CursorDelay,
            "1000",
            "1000 ms",
        ),
        (
            SettingsCategory::Applications,
            SettingsField::CursorRamp,
            "2000",
            "2000 ms",
        ),
        (
            SettingsCategory::Applications,
            SettingsField::CursorHorizontalStep,
            "12",
            "12 cells",
        ),
        (
            SettingsCategory::Applications,
            SettingsField::CursorVerticalStep,
            "5",
            "5 lines",
        ),
    ] {
        select_category(&mut state, &platform, category);
        if category == SettingsCategory::Applications
            && state.to_settings_view_model().unwrap().selected_application
                != SettingsApplication::Editor
        {
            press(&mut state, &platform, "Home");
            press(&mut state, &platform, "Down");
        }
        select_field(&mut state, &platform, field);
        press(&mut state, &platform, "Enter");
        let before = manager.load_config().unwrap();
        let model = state.to_settings_view_model().unwrap();
        let editor = model.number_editor.unwrap();
        for invalid in [String::new(), "0".into(), (editor.maximum + 1).to_string()] {
            press(&mut state, &platform, "Ctrl+A");
            press(&mut state, &platform, "Backspace");
            for digit in invalid.chars() {
                press(&mut state, &platform, &digit.to_string());
            }
            press(&mut state, &platform, "Enter");
            assert!(
                state
                    .to_settings_view_model()
                    .unwrap()
                    .number_editor
                    .unwrap()
                    .error
                    .is_some(),
                "{field:?}"
            );
            assert_eq!(manager.load_config().unwrap(), before);
        }
        press(&mut state, &platform, "Ctrl+A");
        for digit in value.chars() {
            press(&mut state, &platform, &digit.to_string());
        }
        assert_eq!(manager.load_config().unwrap(), before);
        press(&mut state, &platform, "Enter");
        let model = state.to_settings_view_model().unwrap();
        assert!(model.number_editor.is_none());
        assert_eq!(model.selected_field, field);
        assert_eq!(
            model
                .cards
                .iter()
                .flat_map(|card| &card.items)
                .find(|item| item.field == field)
                .unwrap()
                .value,
            display
        );
    }
}

#[test]
fn numeric_settings_use_the_same_dialog_for_mouse_and_touch() {
    for hover in [true, false] {
        let fixture = FixtureRoot::new("settings-number-pointer");
        let platform = mock_platform(fixture.path());
        let manager = initialize_users(&platform, false, false);
        let mut state = logged_in_state(&platform, "AdminUser", "StrongPass123");
        open_settings_from_home(&mut state, &platform);
        select_category(&mut state, &platform, SettingsCategory::System);
        let main = match ui::compute_shell_layout(Rect::new(0, 0, 120, 40)) {
            ui::ShellLayout::Compact(main) | ui::ShellLayout::Full { main, .. } => main,
        };
        for confirm in [true, false] {
            let before = manager.load_config().unwrap();
            let layout = ui::settings_layout(main, &state.to_settings_view_model().unwrap());
            let field = layout
                .fields
                .iter()
                .find(|field| field.field == SettingsField::SystemLowAvailable)
                .unwrap()
                .area;
            let point = (field.x, field.y);
            if hover {
                state.apply_input_with_platform(InputEvent::mouse_moved(point), &platform);
            }
            state.apply_input_with_platform(
                InputEvent::mouse_down(ui::MouseButton::Left, point),
                &platform,
            );
            state.apply_input_with_platform(
                InputEvent::mouse_up(ui::MouseButton::Left, point),
                &platform,
            );
            let model = state.to_settings_view_model().unwrap();
            assert_eq!(
                model.number_editor.as_ref().unwrap().field,
                SettingsField::SystemLowAvailable
            );
            let layout = ui::settings_layout(main, &model);
            let number = layout.number_editor.unwrap();
            let point = (number.increase.x, number.increase.y);
            state.apply_input_with_platform(
                InputEvent::mouse_down(ui::MouseButton::Left, point),
                &platform,
            );
            state.apply_input_with_platform(
                InputEvent::mouse_up(ui::MouseButton::Left, point),
                &platform,
            );
            assert_eq!(manager.load_config().unwrap(), before);
            assert_eq!(
                state
                    .to_settings_view_model()
                    .unwrap()
                    .number_editor
                    .unwrap()
                    .input
                    .value(),
                (before.system_status.low_available_gib + 1).to_string()
            );
            let background = layout.category_cards[0].area;
            state.apply_input_with_platform(
                InputEvent::mouse_down(ui::MouseButton::Left, (background.x, background.y)),
                &platform,
            );
            state.apply_input_with_platform(
                InputEvent::mouse_up(ui::MouseButton::Left, (background.x, background.y)),
                &platform,
            );
            assert_eq!(
                state.to_settings_view_model().unwrap().selected_category,
                SettingsCategory::System
            );
            let button = if confirm {
                layout.overlay_apply.unwrap()
            } else {
                layout.overlay_cancel.unwrap()
            };
            let point = (button.x, button.y);
            state.apply_input_with_platform(
                InputEvent::mouse_down(ui::MouseButton::Left, point),
                &platform,
            );
            state.apply_input_with_platform(
                InputEvent::mouse_up(ui::MouseButton::Left, point),
                &platform,
            );
            let model = state.to_settings_view_model().unwrap();
            assert!(model.number_editor.is_none());
            assert_eq!(model.selected_field, SettingsField::SystemLowAvailable);
            assert_eq!(
                manager
                    .load_config()
                    .unwrap()
                    .system_status
                    .low_available_gib,
                before.system_status.low_available_gib + u16::from(confirm)
            );
        }
    }
}

#[cfg(target_os = "linux")]
#[test]
fn linux_personal_settings_work_without_system_account_service() {
    const CHILD: &str = "TUNDRA_TEST_SETTINGS_WITHOUT_ACCOUNT_SERVICE";
    let fixture = FixtureRoot::new("linux-no-account-service");
    if std::env::var_os(CHILD).is_none() {
        // Change the bus address only in an isolated test process, never while
        // other tests may be using the environment or the real system bus.
        let result = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "linux_personal_settings_work_without_system_account_service",
                "--nocapture",
            ])
            .env(CHILD, "1")
            .env(
                "DBUS_SYSTEM_BUS_ADDRESS",
                format!("unix:path={}/missing-bus", fixture.path().display()),
            )
            .status()
            .unwrap();
        assert!(result.success());
        return;
    }
    let platform = mock_platform(fixture.path()).with_kind(PlatformKind::Linux);
    let manager = StorageManager::open_from_platform(&platform)
        .unwrap()
        .manager;
    let backend = identity::IdentityBackend::Linux;
    let actor = SessionService::new(manager.clone())
        .with_backend(backend)
        .attach_current_linux_user()
        .unwrap();
    let users = UserService::new(manager.clone()).with_backend(backend);
    let appearance = storage::AppearanceConfig {
        icon_display_mode: IconDisplayMode::Ascii,
        ..Default::default()
    };
    users
        .complete_personalization(&actor, appearance.clone())
        .unwrap();
    assert!(
        users.list_accessible_users(&actor).is_err(),
        "system account management must really be unavailable"
    );

    let mut startup = prepare_shell_startup(&platform).unwrap();
    startup.identity_backend = backend;
    startup.auth_bootstrap_required = false;
    let mut state = ShellSession::new_with_startup(default_config(), (120, 40), startup);
    assert_eq!(state.active_screen(), ShellScreen::Home);
    open_settings_from_home(&mut state, &platform);
    assert_eq!(state.active_screen(), ShellScreen::Settings);
    assert!(state.to_settings_view_model().is_some());

    // Exercise the normal settings save path, then reopen from disk.
    let before = manager.load_users().unwrap().users[0]
        .appearance
        .border_shape;
    press(&mut state, &platform, "Down");
    press(&mut state, &platform, "Enter");
    let saved = manager.load_users().unwrap().users[0].appearance.clone();
    assert_ne!(saved.border_shape, before);
    press(&mut state, &platform, "Esc");
    open_settings_from_home(&mut state, &platform);
    assert_eq!(state.active_screen(), ShellScreen::Settings);
    assert_eq!(manager.load_users().unwrap().users[0].appearance, saved);

    let before = manager.load_users().unwrap();
    let mut wrong_actor = actor.clone();
    wrong_actor.user_id.push_str("-another-user");
    assert!(users.current_user_appearance(&wrong_actor).is_err());
    assert!(
        users
            .update_user_appearance(&wrong_actor, &actor.username, appearance.clone())
            .is_err()
    );
    assert!(
        users
            .update_user_appearance(&actor, "another-user", appearance)
            .is_err()
    );
    assert_eq!(manager.load_users().unwrap(), before);
}

#[test]
fn tab_cycles_sections_while_arrows_select_right_hand_settings() {
    let fixture = FixtureRoot::new("tab-section-cycle");
    let platform = mock_platform(fixture.path());
    initialize_users(&platform, false, false);
    let mut state = logged_in_state(&platform, "AdminUser", "StrongPass123");

    open_settings_from_home(&mut state, &platform);
    let appearance = state.to_settings_view_model().unwrap();
    assert_eq!(appearance.selected_category, SettingsCategory::Appearance);
    assert_eq!(appearance.selected_field, SettingsField::Theme);

    press(&mut state, &platform, "Down");
    let appearance = state.to_settings_view_model().unwrap();
    assert_eq!(appearance.selected_category, SettingsCategory::Appearance);
    assert_eq!(appearance.selected_field, SettingsField::BorderShape);

    for expected in [
        SettingsCategory::RegionTime,
        SettingsCategory::System,
        SettingsCategory::Applications,
        SettingsCategory::Update,
        SettingsCategory::Appearance,
    ] {
        press(&mut state, &platform, "Tab");
        assert_eq!(
            state.to_settings_view_model().unwrap().selected_category,
            expected
        );
    }
    assert_eq!(
        state.to_settings_view_model().unwrap().selected_field,
        SettingsField::Theme
    );

    press(&mut state, &platform, "Shift+Tab");
    let update = state.to_settings_view_model().unwrap();
    assert_eq!(update.selected_category, SettingsCategory::Update);
    assert_eq!(
        update.selected_field,
        if cfg!(target_os = "linux") {
            SettingsField::UpdateMode
        } else {
            SettingsField::InstalledVersion
        }
    );

    press(&mut state, &platform, "Shift+Tab");
    let applications = state.to_settings_view_model().unwrap();
    assert_eq!(
        applications.selected_category,
        SettingsCategory::Applications
    );
    assert_eq!(applications.selected_field, SettingsField::ApplicationList);
    assert_eq!(
        applications.selected_application,
        SettingsApplication::FileExplorer
    );
    press(&mut state, &platform, "Down");
    press(&mut state, &platform, "Enter");
    let editor = state.to_settings_view_model().unwrap();
    assert_eq!(editor.selected_category, SettingsCategory::Applications);
    assert_eq!(editor.selected_application, SettingsApplication::Editor);
    assert_eq!(editor.selected_field, SettingsField::ExplorerOpenExtensions);

    press(&mut state, &platform, "Down");
    let editor = state.to_settings_view_model().unwrap();
    assert_eq!(editor.selected_category, SettingsCategory::Applications);
    assert_eq!(editor.selected_field, SettingsField::CursorAcceleration);
}

#[cfg(target_os = "linux")]
#[test]
fn linux_update_mode_saves_keyboard_and_mouse_choices_across_reopen() {
    let fixture = FixtureRoot::new("update-mode");
    let platform = mock_platform(fixture.path());
    let manager = initialize_users(&platform, false, false);
    let mut state = logged_in_state(&platform, "AdminUser", "StrongPass123");
    assert_eq!(
        manager.load_config().unwrap().linux_update_mode,
        storage::LinuxUpdateMode::Release
    );
    open_settings_from_home(&mut state, &platform);
    press(&mut state, &platform, "Shift+Tab");
    assert_eq!(
        state.to_settings_view_model().unwrap().selected_field,
        SettingsField::UpdateMode
    );
    press(&mut state, &platform, "Enter");
    assert_eq!(
        manager.load_config().unwrap().linux_update_mode,
        storage::LinuxUpdateMode::Beta
    );
    press(&mut state, &platform, "Esc");
    open_settings_from_home(&mut state, &platform);
    press(&mut state, &platform, "Shift+Tab");
    let model = state.to_settings_view_model().unwrap();
    assert!(
        model
            .cards
            .iter()
            .flat_map(|c| &c.items)
            .any(|i| i.field == SettingsField::UpdateMode && i.value.contains("Beta"))
    );
    let main = match ui::compute_shell_layout(Rect::new(0, 0, 120, 40)) {
        ui::ShellLayout::Compact(main) | ui::ShellLayout::Full { main, .. } => main,
    };
    let layout = ui::settings_layout(main, &model);
    let field = layout
        .fields
        .iter()
        .find(|f| f.field == SettingsField::UpdateMode)
        .unwrap();
    state.apply_input_with_platform(
        InputEvent::mouse_down(ui::MouseButton::Left, (field.area.x, field.area.y)),
        &platform,
    );
    assert_eq!(
        manager.load_config().unwrap().linux_update_mode,
        storage::LinuxUpdateMode::Release
    );
    press(&mut state, &platform, "Enter");
    assert_eq!(
        manager.load_config().unwrap().linux_update_mode,
        storage::LinuxUpdateMode::Beta
    );
    press(&mut state, &platform, "Enter");
    assert_eq!(
        manager.load_config().unwrap().linux_update_mode,
        storage::LinuxUpdateMode::Release
    );
}

#[test]
fn application_list_switches_with_pointer_without_changing_saved_settings() {
    let fixture = FixtureRoot::new("application-settings-pointer");
    let platform = mock_platform(fixture.path());
    let manager = initialize_users(&platform, false, false);
    let mut state = logged_in_state(&platform, "AdminUser", "StrongPass123");
    open_settings_from_home(&mut state, &platform);
    select_category(&mut state, &platform, SettingsCategory::Applications);
    let original = manager.load_config().unwrap();
    let ui::ShellLayout::Full { main, .. } = ui::compute_shell_layout(Rect::new(0, 0, 120, 40))
    else {
        panic!("full layout");
    };
    for application in [
        SettingsApplication::Editor,
        SettingsApplication::FileExplorer,
    ] {
        let model = state.to_settings_view_model().unwrap();
        let layout = ui::settings_layout(main, &model);
        let button = layout
            .applications
            .as_ref()
            .unwrap()
            .options
            .iter()
            .find(|(candidate, _)| *candidate == application)
            .unwrap()
            .1;
        let point = (button.x, button.y);
        state.apply_input_with_platform(
            InputEvent::mouse_down(ui::MouseButton::Left, point),
            &platform,
        );
        state.apply_input_with_platform(
            InputEvent::mouse_up(ui::MouseButton::Left, point),
            &platform,
        );
        let model = state.to_settings_view_model().unwrap();
        assert_eq!(model.selected_application, application);
        assert_eq!(model.selected_field, SettingsField::ApplicationList);
        let first = model.cards[0].items[0].field;
        assert_eq!(
            first,
            match application {
                SettingsApplication::FileExplorer => SettingsField::ShowHidden,
                SettingsApplication::Editor => SettingsField::ExplorerOpenExtensions,
            }
        );
        assert_eq!(manager.load_config().unwrap(), original);
    }
}

#[test]
fn restore_application_defaults_changes_only_the_selected_application() {
    let fixture = FixtureRoot::new("application-settings-reset");
    let platform = mock_platform(fixture.path());
    let manager = initialize_users(&platform, false, false);
    let mut config = manager.load_config().unwrap();
    config.explorer.show_hidden = true;
    config.editor.cursor_acceleration_delay_ms += 100;
    manager.save_config(&config).unwrap();
    let mut state = logged_in_state(&platform, "AdminUser", "StrongPass123");
    open_settings_from_home(&mut state, &platform);
    select_category(&mut state, &platform, SettingsCategory::Applications);
    for key in ["End", "Enter", "R"] {
        press(&mut state, &platform, key);
    }
    let saved = manager.load_config().unwrap();
    assert_eq!(saved.explorer, storage::ExplorerConfig::default());
    assert_eq!(saved.editor, config.editor);
    for key in ["Home", "Down", "Enter", "End", "Enter", "R"] {
        press(&mut state, &platform, key);
    }
    let saved = manager.load_config().unwrap();
    assert_eq!(saved.editor, storage::EditorConfig::default());
    assert_eq!(saved.explorer, storage::ExplorerConfig::default());
    assert_eq!(
        state.to_settings_view_model().unwrap().selected_application,
        SettingsApplication::Editor
    );
}

#[test]
fn mouse_wheel_scrolls_only_to_the_settings_content_boundary() {
    let fixture = FixtureRoot::new("bounded-settings-scroll");
    let platform = mock_platform(fixture.path());
    initialize_users(&platform, false, false);
    let mut state = logged_in_state(&platform, "AdminUser", "StrongPass123");
    open_settings_from_home(&mut state, &platform);

    state.apply_input_with_platform(
        InputEvent::Resize {
            width: 120,
            height: 60,
        },
        &platform,
    );
    for _ in 0..20 {
        state.apply_input_with_platform(
            InputEvent::mouse_scroll(shell::ScrollDirection::Down, (60, 10)),
            &platform,
        );
    }
    let fitting_model = state.to_settings_view_model().expect("settings model");
    let fitting_main = match ui::compute_shell_layout(Rect::new(0, 0, 120, 60)) {
        ui::ShellLayout::Compact(compact) => compact,
        ui::ShellLayout::Full { main, .. } => main,
    };
    let fitting_layout = ui::settings_layout(fitting_main, &fitting_model);
    assert_eq!(fitting_model.scroll_offset, 0);
    assert_eq!(fitting_layout.max_scroll_offset, 0);
    assert!(fitting_layout.scrollbar.is_none());

    state.apply_input_with_platform(
        InputEvent::Resize {
            width: 120,
            height: 12,
        },
        &platform,
    );
    let overflowing_model = state.to_settings_view_model().expect("settings model");
    let overflowing_main = match ui::compute_shell_layout(Rect::new(0, 0, 120, 12)) {
        ui::ShellLayout::Compact(compact) => compact,
        ui::ShellLayout::Full { main, .. } => main,
    };
    let overflowing_layout = ui::settings_layout(overflowing_main, &overflowing_model);
    assert!(overflowing_layout.scrollbar.is_some());
    assert!(overflowing_layout.max_scroll_offset > 0);

    for direction in [shell::ScrollDirection::Left, shell::ScrollDirection::Right] {
        state.apply_input_with_platform(InputEvent::mouse_scroll(direction, (60, 6)), &platform);
        assert_eq!(state.to_settings_view_model().unwrap().scroll_offset, 0);
    }

    for _ in 0..100 {
        state.apply_input_with_platform(
            InputEvent::mouse_scroll(shell::ScrollDirection::Down, (60, 6)),
            &platform,
        );
    }
    assert_eq!(
        state
            .to_settings_view_model()
            .expect("scrolled settings model")
            .scroll_offset,
        overflowing_layout.max_scroll_offset
    );

    state.apply_input_with_platform(
        InputEvent::Resize {
            width: 120,
            height: 60,
        },
        &platform,
    );
    let resized_model = state
        .to_settings_view_model()
        .expect("resized settings model");
    let resized_layout = ui::settings_layout(fitting_main, &resized_model);
    assert_eq!(resized_model.scroll_offset, 0);
    assert!(resized_layout.scrollbar.is_none());
}

#[test]
fn system_settings_show_defaults_and_persist_confirmed_values() {
    let fixture = FixtureRoot::new("system-settings");
    let platform = mock_platform(fixture.path());
    let manager = initialize_users(&platform, false, false);
    let mut state = logged_in_state(&platform, "AdminUser", "StrongPass123");
    open_settings_from_home(&mut state, &platform);
    select_category(&mut state, &platform, SettingsCategory::System);
    for (index, policy) in [
        (2, storage::AutoAdminPolicy::Automatic),
        (0, storage::AutoAdminPolicy::Deny),
        (1, storage::AutoAdminPolicy::Manual),
    ] {
        let before = manager.load_config().unwrap().auto_admin;
        press(&mut state, &platform, "Enter");
        press(&mut state, &platform, "Home");
        for _ in 0..index {
            press(&mut state, &platform, "Down");
        }
        assert_eq!(manager.load_config().unwrap().auto_admin, before);
        press(&mut state, &platform, "Enter");
        assert_eq!(manager.load_config().unwrap().auto_admin, policy);
    }
    press(&mut state, &platform, "Down");
    press(&mut state, &platform, "Enter");
    assert!(manager.load_config().unwrap().automation.enabled);
    press(&mut state, &platform, " ");
    assert!(!manager.load_config().unwrap().automation.enabled);
    for _ in 0..2 {
        press(&mut state, &platform, "Down");
    }
    let model = state.to_settings_view_model().unwrap();
    let items = &model.cards[3].items;
    assert_eq!(
        items
            .iter()
            .map(|item| item.value.as_str())
            .collect::<Vec<_>>(),
        ["5 GiB", "10%", "1 GiB", "5%"]
    );
    assert_eq!(model.selected_field, SettingsField::SystemLowAvailable);
    for key in ["Enter", "Left", "Enter"] {
        press(&mut state, &platform, key);
    }
    assert_eq!(
        manager
            .load_config()
            .unwrap()
            .system_status
            .low_available_gib,
        4
    );

    let mut config = manager.load_config().unwrap();
    config.system_status.low_available_gib = 5;
    config.system_status.critical_available_gib = 5;
    manager.save_config(&config).unwrap();
    for key in ["Enter", "Ctrl+A", "4", "Enter"] {
        press(&mut state, &platform, key);
    }
    let saved = manager.load_config().unwrap();
    assert_eq!(saved.system_status.low_available_gib, 4);
    assert_eq!(saved.system_status.critical_available_gib, 4);
    for key in ["Down", "Down", "Enter", "Right", "Enter"] {
        press(&mut state, &platform, key);
    }
    assert_eq!(
        manager
            .load_config()
            .unwrap()
            .system_status
            .critical_available_gib,
        4
    );
}

#[test]
fn auto_admin_picker_requires_confirmation_with_mouse_or_touch() {
    for hover in [true, false] {
        let fixture = FixtureRoot::new("aa-picker");
        let platform = mock_platform(fixture.path());
        let manager = initialize_users(&platform, true, false);
        for (username, password, enabled) in [
            ("AdminUser", "StrongPass123", true),
            ("NormalUser", "NormalPass123", false),
        ] {
            let mut state = logged_in_state(&platform, username, password);
            open_settings_from_home(&mut state, &platform);
            select_category(&mut state, &platform, SettingsCategory::System);
            let main = match ui::compute_shell_layout(Rect::new(0, 0, 120, 40)) {
                ui::ShellLayout::Compact(main) | ui::ShellLayout::Full { main, .. } => main,
            };
            for (index, policy) in [
                (2, storage::AutoAdminPolicy::Automatic),
                (0, storage::AutoAdminPolicy::Deny),
                (1, storage::AutoAdminPolicy::Manual),
            ] {
                let before = manager.load_config().unwrap().auto_admin;
                let layout = ui::settings_layout(main, &state.to_settings_view_model().unwrap());
                let area = layout
                    .fields
                    .iter()
                    .find(|field| field.field == SettingsField::AutoAdmin)
                    .unwrap()
                    .area;
                let point = (area.x, area.y);
                if hover {
                    state.apply_input_with_platform(InputEvent::mouse_moved(point), &platform);
                }
                state.apply_input_with_platform(
                    InputEvent::mouse_down(ui::MouseButton::Left, point),
                    &platform,
                );
                state.apply_input_with_platform(
                    InputEvent::mouse_up(ui::MouseButton::Left, point),
                    &platform,
                );
                let model = state.to_settings_view_model().unwrap();
                assert_eq!(model.picker.is_some(), enabled);
                if !enabled {
                    assert_eq!(manager.load_config().unwrap().auto_admin, before);
                    continue;
                }
                assert_eq!(
                    model.picker.as_ref().unwrap().kind,
                    SettingsPickerKind::AutoAdmin
                );
                let layout = ui::settings_layout(main, &model);
                let option = layout
                    .picker_options
                    .iter()
                    .find(|option| option.index == index)
                    .unwrap()
                    .area;
                let point = (option.x, option.y);
                if hover {
                    state.apply_input_with_platform(InputEvent::mouse_moved(point), &platform);
                }
                state.apply_input_with_platform(
                    InputEvent::mouse_down(ui::MouseButton::Left, point),
                    &platform,
                );
                state.apply_input_with_platform(
                    InputEvent::mouse_up(ui::MouseButton::Left, point),
                    &platform,
                );
                assert_eq!(manager.load_config().unwrap().auto_admin, before);
                assert_eq!(
                    state
                        .to_settings_view_model()
                        .unwrap()
                        .picker
                        .unwrap()
                        .selected_index,
                    index
                );
                let apply = layout.overlay_apply.unwrap();
                let point = (apply.x, apply.y);
                state.apply_input_with_platform(
                    InputEvent::mouse_down(ui::MouseButton::Left, point),
                    &platform,
                );
                state.apply_input_with_platform(
                    InputEvent::mouse_up(ui::MouseButton::Left, point),
                    &platform,
                );
                assert_eq!(manager.load_config().unwrap().auto_admin, policy);
                assert!(state.to_settings_view_model().unwrap().picker.is_none());
            }
        }
    }
}

#[test]
fn screen_timeout_saves_all_choices_and_wakes_without_logging_out() {
    let fixture = FixtureRoot::new("screen-timeout");
    let platform = mock_platform(fixture.path());
    let manager = initialize_users(&platform, false, false);
    let mut state = logged_in_state(&platform, "AdminUser", "StrongPass123");
    open_settings_from_home(&mut state, &platform);
    for key in ["Tab", "Tab", "Down", "Down"] {
        press(&mut state, &platform, key);
    }
    assert_eq!(
        state.to_settings_view_model().unwrap().selected_field,
        SettingsField::ScreenTimeout
    );
    for (index, seconds) in [0, 30, 60, 120, 300, 600, 1800].into_iter().enumerate() {
        let before = manager.load_config().unwrap().screen_timeout_seconds;
        press(&mut state, &platform, "Enter");
        press(&mut state, &platform, "Home");
        for _ in 0..index {
            press(&mut state, &platform, "Down");
        }
        assert_eq!(
            manager.load_config().unwrap().screen_timeout_seconds,
            before
        );
        press(&mut state, &platform, "Enter");
        assert_eq!(
            manager.load_config().unwrap().screen_timeout_seconds,
            seconds
        );
    }
    for key in ["Enter", "Home", "Down", "Down", "Down", "Enter"] {
        press(&mut state, &platform, key);
    }
    assert_eq!(manager.load_config().unwrap().screen_timeout_seconds, 120);
    let actor = state.auth_session().cloned();
    let now = Instant::now();
    state.apply_input_at(InputEvent::FocusGained, now);
    state.apply_input_at(InputEvent::Tick, now + Duration::from_secs(120));
    assert!(state.screen_saver_active());
    state.apply_input_at(
        InputEvent::from_key_label("Esc"),
        now + Duration::from_secs(121),
    );
    assert!(!state.screen_saver_active());
    assert_eq!(state.active_screen(), ShellScreen::Settings);
    assert_eq!(state.auth_session(), actor.as_ref());
    assert!(!state.shutdown_requested());
    assert_eq!(
        state.to_settings_view_model().unwrap().selected_field,
        SettingsField::ScreenTimeout
    );
}

#[test]
fn normal_user_system_settings_keep_their_controls_disabled() {
    let fixture = FixtureRoot::new("system-settings-user");
    let platform = mock_platform(fixture.path());
    let manager = initialize_users(&platform, true, false);
    let mut state = logged_in_state(&platform, "NormalUser", "NormalPass123");
    open_settings_from_home(&mut state, &platform);
    press(&mut state, &platform, "Tab");
    press(&mut state, &platform, "Tab");

    let before = manager.load_config().unwrap();
    let model = state.to_settings_view_model().unwrap();
    assert!(
        model
            .cards
            .iter()
            .flat_map(|card| &card.items)
            .all(|item| !item.enabled)
    );
    assert!(
        model
            .cards
            .iter()
            .flat_map(|card| &card.items)
            .all(|item| item.kind != ui::SettingsControlKind::ReadOnly)
    );
    press(&mut state, &platform, "Right");
    assert_eq!(manager.load_config().unwrap(), before);
}

#[test]
fn admin_settings_immediately_persist_global_changes_and_confirm_picker_selection() {
    let fixture = FixtureRoot::new("admin-persistence");
    let platform = mock_platform(fixture.path());
    let manager = initialize_users(&platform, false, false);
    let mut state = logged_in_state(&platform, "AdminUser", "StrongPass123");

    open_settings_from_home(&mut state, &platform);
    assert_eq!(state.active_screen(), ShellScreen::Settings);

    // Appearance -> Region & Time. Moving the timezone highlight must not save.
    press(&mut state, &platform, "Tab");
    press(&mut state, &platform, "Down");
    assert_eq!(
        state.to_settings_view_model().unwrap().selected_field,
        SettingsField::Timezone
    );
    let original_timezone = manager.load_config().unwrap().timezone;
    press(&mut state, &platform, "Enter");
    press(&mut state, &platform, "Down");
    let highlighted_timezone = state
        .to_settings_view_model()
        .unwrap()
        .picker
        .unwrap()
        .options
        .into_iter()
        .nth(1)
        .and_then(|option| option.timezone_id)
        .expect("second timezone option");
    assert_eq!(manager.load_config().unwrap().timezone, original_timezone);
    press(&mut state, &platform, "Enter");
    assert_eq!(
        manager.load_config().unwrap().timezone,
        highlighted_timezone
    );

    select_category(&mut state, &platform, SettingsCategory::Applications);
    assert_eq!(
        state.to_settings_view_model().unwrap().selected_category,
        SettingsCategory::Applications
    );
    assert!(!manager.load_config().unwrap().explorer.show_hidden);
    press(&mut state, &platform, "Right");
    press(&mut state, &platform, "Enter");
    assert!(manager.load_config().unwrap().explorer.show_hidden);
    assert!(
        state
            .to_settings_view_model()
            .unwrap()
            .status
            .contains("Saved")
    );
    select_field(&mut state, &platform, SettingsField::SortField);
    for expected in [
        storage::ExplorerSortField::Type,
        storage::ExplorerSortField::Size,
        storage::ExplorerSortField::Modified,
        storage::ExplorerSortField::Name,
    ] {
        press(&mut state, &platform, "Enter");
        assert_eq!(manager.load_config().unwrap().explorer.sort_field, expected);
    }
}

#[test]
fn weather_location_rejects_non_english_input_and_saves_only_after_warning() {
    let fixture = FixtureRoot::new("weather-location");
    let platform = mock_platform(fixture.path());
    let manager = initialize_users(&platform, false, false);
    let mut state = logged_in_state(&platform, "AdminUser", "StrongPass123");

    open_settings_from_home(&mut state, &platform);
    press(&mut state, &platform, "Tab");
    for _ in 0..2 {
        press(&mut state, &platform, "Down");
    }
    assert_eq!(
        state.to_settings_view_model().unwrap().selected_field,
        SettingsField::WeatherLocation
    );
    press(&mut state, &platform, "Enter");

    press(&mut state, &platform, "杭");
    let editor = state
        .to_settings_view_model()
        .unwrap()
        .weather_location_editor
        .expect("weather location editor");
    assert!(editor.value.is_empty());
    assert!(editor.error.unwrap().contains("Only English"));

    let location = "Cambridge, Massachusetts, USA";
    for character in location.chars() {
        press(&mut state, &platform, &character.to_string());
    }
    press(&mut state, &platform, "Enter");
    assert_eq!(manager.load_config().unwrap().weather_location, None);
    let warning = state
        .to_notification_view_model()
        .expect("weather search warning");
    assert!(warning.message.contains("may be inaccurate"));
    assert!(warning.message.contains("no results"));

    press(&mut state, &platform, "Enter");
    assert_eq!(
        manager.load_config().unwrap().weather_location.as_deref(),
        Some(location)
    );
    assert!(
        state
            .to_settings_view_model()
            .unwrap()
            .weather_location_editor
            .is_none()
    );
}

#[test]
fn operating_system_time_source_uses_platform_boundary_and_persists() {
    let fixture = FixtureRoot::new("system-time-source");
    let platform = mock_platform(fixture.path());
    let manager = initialize_users(&platform, false, false);
    let system_time = UNIX_EPOCH + Duration::from_secs(1_700_000_000);
    platform.set_system_time_result(Ok(system_time));
    let mut state = logged_in_state(&platform, "AdminUser", "StrongPass123");

    open_settings_from_home(&mut state, &platform);
    press(&mut state, &platform, "Tab");
    press(&mut state, &platform, "Down");
    press(&mut state, &platform, "Down");
    press(&mut state, &platform, "Down");
    assert_eq!(
        state.to_settings_view_model().unwrap().selected_field,
        SettingsField::TimeSyncSource
    );
    press(&mut state, &platform, "Enter");

    assert_eq!(
        manager.load_config().unwrap().time_sync.source,
        TimeSyncSource::OperatingSystem
    );
    assert!(
        platform
            .calls()
            .iter()
            .any(|call| matches!(call, MockCall::SystemTime))
    );
    assert!(!state.time_sync_failure_dialog_visible());
}

#[test]
fn malformed_time_server_shows_error_and_does_not_save() {
    let fixture = FixtureRoot::new("malformed-time-server");
    let platform = mock_platform(fixture.path());
    let manager = initialize_users(&platform, false, false);
    let mut state = logged_in_state(&platform, "AdminUser", "StrongPass123");

    open_time_server_editor(&mut state, &platform);
    for character in "not a URL".chars() {
        press(&mut state, &platform, &character.to_string());
    }
    press(&mut state, &platform, "Enter");

    assert_eq!(manager.load_config().unwrap().time_sync.server_url, None);
    assert!(state.time_sync_failure_dialog_visible());
    assert!(
        state
            .time_sync_failure_message()
            .is_some_and(|message| message.contains("setting was not saved"))
    );
}

#[test]
fn editor_settings_save_normalized_explorer_open_suffixes() {
    let fixture = FixtureRoot::new("editor-open-suffixes");
    let platform = mock_platform(fixture.path());
    let manager = initialize_users(&platform, false, false);
    let mut state = logged_in_state(&platform, "AdminUser", "StrongPass123");

    open_settings_from_home(&mut state, &platform);
    select_category(&mut state, &platform, SettingsCategory::Applications);
    press(&mut state, &platform, "Down");
    press(&mut state, &platform, "Enter");
    assert_eq!(
        state.to_settings_view_model().unwrap().selected_field,
        SettingsField::ExplorerOpenExtensions
    );

    press(&mut state, &platform, "Enter");
    let existing_len = state
        .to_settings_view_model()
        .unwrap()
        .file_extensions_editor
        .expect("file suffix editor")
        .value
        .chars()
        .count();
    for _ in 0..existing_len {
        press(&mut state, &platform, "Backspace");
    }
    for character in ".RS, rs; .d.ts".chars() {
        press(&mut state, &platform, &character.to_string());
    }
    press(&mut state, &platform, "Enter");

    assert_eq!(
        manager
            .load_config()
            .unwrap()
            .editor
            .explorer_open_extensions,
        vec!["rs".to_string(), "d.ts".to_string()]
    );
    assert!(
        state
            .to_settings_view_model()
            .unwrap()
            .file_extensions_editor
            .is_none()
    );
}

#[test]
fn normal_user_can_change_only_their_appearance() {
    let fixture = FixtureRoot::new("user-permissions");
    let platform = mock_platform(fixture.path());
    let manager = initialize_users(&platform, true, false);
    let mut state = logged_in_state(&platform, "NormalUser", "NormalPass123");

    open_settings_from_home(&mut state, &platform);
    let appearance = state.to_settings_view_model().unwrap();
    assert!(appearance.locked_message.is_none());
    assert!(
        appearance
            .cards
            .iter()
            .flat_map(|card| &card.items)
            .all(|item| item.enabled)
    );

    press(&mut state, &platform, "Down");
    press(&mut state, &platform, "Enter");
    let users = manager.load_users().unwrap();
    assert_eq!(
        users
            .users
            .iter()
            .find(|user| user.username == "NormalUser")
            .unwrap()
            .appearance
            .border_shape,
        BorderShape::Square
    );
    assert_eq!(
        users
            .users
            .iter()
            .find(|user| user.username == "AdminUser")
            .unwrap()
            .appearance
            .border_shape,
        BorderShape::Rounded
    );

    press(&mut state, &platform, "Tab");
    let region = state.to_settings_view_model().unwrap();
    assert_eq!(region.selected_category, SettingsCategory::RegionTime);
    assert!(
        region
            .locked_message
            .as_deref()
            .unwrap()
            .contains("administrator")
    );
    assert!(
        region
            .cards
            .iter()
            .flat_map(|card| &card.items)
            .all(|item| !item.enabled)
    );

    let config_before = manager.load_config().unwrap();
    press(&mut state, &platform, "Enter");
    press(&mut state, &platform, "Enter");
    assert_eq!(manager.load_config().unwrap(), config_before);
    assert!(
        state
            .to_settings_view_model()
            .unwrap()
            .status
            .starts_with("Error:")
    );
}

#[test]
fn advanced_animation_phases_save_per_user_and_restore_with_speed_defaults() {
    let fixture = FixtureRoot::new("animation-phases");
    let platform = mock_platform(fixture.path());
    let manager = initialize_users(&platform, true, false);
    let mut state = logged_in_state(&platform, "NormalUser", "NormalPass123");
    open_settings_from_home(&mut state, &platform);
    for _ in 0..5 {
        press(&mut state, &platform, "Down");
    }
    for key in ["Enter", "Right", "Enter", "Down"] {
        press(&mut state, &platform, key);
    }
    let phases = [
        SettingsField::PageAcceleration,
        SettingsField::PageCruise,
        SettingsField::PageDeceleration,
        SettingsField::DialogAcceleration,
        SettingsField::DialogCruise,
        SettingsField::DialogDeceleration,
        SettingsField::MenuAcceleration,
        SettingsField::MenuCruise,
        SettingsField::MenuDeceleration,
    ];
    let model = state.to_settings_view_model().unwrap();
    assert_eq!(model.selected_field, SettingsField::AnimationAdvanced);
    assert!(
        !model
            .cards
            .iter()
            .flat_map(|card| &card.items)
            .any(|item| phases.contains(&item.field))
    );
    press(&mut state, &platform, "Enter");
    for (index, field) in phases.into_iter().enumerate() {
        let model = state.to_settings_view_model().unwrap();
        let editor = model.animation_editor.as_ref().expect("advanced modal");
        assert_eq!(model.selected_field, SettingsField::AnimationAdvanced);
        assert_eq!(editor.items[editor.selected_index].field, field);
        for key in ["Enter", "Right", "Enter"] {
            press(&mut state, &platform, key);
        }
        if index < phases.len() - 1 {
            press(&mut state, &platform, "Down");
        }
    }
    press(&mut state, &platform, "Esc");
    let model = state.to_settings_view_model().unwrap();
    assert!(model.animation_editor.is_none());
    assert_eq!(model.selected_field, SettingsField::AnimationAdvanced);
    assert_eq!(state.active_screen(), ShellScreen::Settings);
    let users = manager.load_users().unwrap();
    let saved = &users
        .users
        .iter()
        .find(|user| user.username == "NormalUser")
        .unwrap()
        .appearance;
    assert_eq!(saved.animation_speed_percent, 125);
    assert_eq!(saved.animation_timings.page, [56, 111, 56]);
    assert_eq!(saved.animation_timings.dialog, [46, 91, 46]);
    assert_eq!(saved.animation_timings.menu, [41, 81, 41]);
    assert_eq!(
        users
            .users
            .iter()
            .find(|user| user.username == "AdminUser")
            .unwrap()
            .appearance
            .animation_timings,
        storage::AnimationTimingsConfig::default()
    );

    let mut reopened = logged_in_state(&platform, "NormalUser", "NormalPass123");
    open_settings_from_home(&mut reopened, &platform);
    press(&mut reopened, &platform, "Home");
    for _ in 0..4 {
        press(&mut reopened, &platform, "Down");
    }
    press(&mut reopened, &platform, "Enter");
    press(&mut reopened, &platform, "End");
    press(&mut reopened, &platform, "Up");
    assert_eq!(
        reopened.to_settings_view_model().unwrap().selected_field,
        SettingsField::ResetAnimationSpeed
    );
    press(&mut reopened, &platform, "Enter");
    let users = manager.load_users().unwrap();
    let restored = &users
        .users
        .iter()
        .find(|user| user.username == "NormalUser")
        .unwrap()
        .appearance;
    assert_eq!(restored.animation_speed_percent, 100);
    assert_eq!(
        restored.animation_timings,
        storage::AnimationTimingsConfig::default()
    );
    assert_eq!(restored.motion_preference, MotionPreference::Reduced);
}

#[test]
fn animation_modal_blocks_background_supports_pointer_reset_and_restores_focus() {
    let fixture = FixtureRoot::new("animation-modal");
    let platform = mock_platform(fixture.path());
    initialize_users(&platform, true, false);
    let mut state = logged_in_state(&platform, "NormalUser", "NormalPass123");
    open_settings_from_home(&mut state, &platform);
    let main = match ui::compute_shell_layout(Rect::new(0, 0, 120, 40)) {
        ui::ShellLayout::Full { main, .. } | ui::ShellLayout::Compact(main) => main,
    };
    let layout = ui::settings_layout(main, &state.to_settings_view_model().unwrap());
    let advanced = layout
        .fields
        .iter()
        .find(|item| item.field == SettingsField::AnimationAdvanced)
        .unwrap()
        .area;
    state.apply_input_with_platform(
        InputEvent::mouse_down(ui::MouseButton::Left, (advanced.x, advanced.y)),
        &platform,
    );
    let model = state.to_settings_view_model().unwrap();
    assert!(model.animation_editor.is_some());
    let editor = ui::settings_layout(main, &model).animation_editor.unwrap();
    let background = layout
        .category_cards
        .iter()
        .find(|item| item.category == SettingsCategory::RegionTime)
        .unwrap()
        .area;
    state.apply_input_with_platform(
        InputEvent::mouse_down(ui::MouseButton::Left, (background.x, background.y)),
        &platform,
    );
    assert_eq!(
        state.to_settings_view_model().unwrap().selected_category,
        SettingsCategory::Appearance
    );
    let value = editor
        .fields
        .iter()
        .find(|field| field.field == SettingsField::PageAcceleration)
        .unwrap()
        .area;
    state.apply_input_with_platform(
        InputEvent::mouse_down(ui::MouseButton::Left, (value.x, value.y)),
        &platform,
    );
    assert!(
        state
            .to_settings_view_model()
            .unwrap()
            .number_editor
            .is_some()
    );
    for key in ["Right", "Enter"] {
        press(&mut state, &platform, key);
    }
    assert_eq!(
        state
            .to_settings_view_model()
            .unwrap()
            .animation_editor
            .unwrap()
            .items[0]
            .value,
        "56 ms"
    );
    press(&mut state, &platform, "End");
    press(&mut state, &platform, "Up");
    press(&mut state, &platform, "Enter");
    assert_eq!(
        state
            .to_settings_view_model()
            .unwrap()
            .animation_editor
            .unwrap()
            .items[0]
            .value,
        "55 ms"
    );
    state.apply_input_with_platform(
        InputEvent::Resize {
            width: 120,
            height: 18,
        },
        &platform,
    );
    let short_main = match ui::compute_shell_layout(Rect::new(0, 0, 120, 18)) {
        ui::ShellLayout::Full { main, .. } | ui::ShellLayout::Compact(main) => main,
    };
    let model = state.to_settings_view_model().unwrap();
    let editor = ui::settings_layout(short_main, &model)
        .animation_editor
        .unwrap();
    assert!(editor.scrollbar.is_some());
    for _ in 0..10 {
        state.apply_input_with_platform(
            InputEvent::mouse_scroll(
                shell::ScrollDirection::Down,
                (editor.viewport.x, editor.viewport.y),
            ),
            &platform,
        );
    }
    let scrolled = state.to_settings_view_model().unwrap();
    assert_eq!(scrolled.scroll_offset, model.scroll_offset);
    assert_eq!(
        scrolled.animation_editor.unwrap().scroll_offset,
        editor.max_scroll_offset
    );
    press(&mut state, &platform, "Home");
    assert_eq!(
        state
            .to_settings_view_model()
            .unwrap()
            .animation_editor
            .unwrap()
            .scroll_offset,
        0
    );
    let track = editor.scrollbar.unwrap();
    let bottom = (track.x, track.bottom() - 1);
    state.apply_input_with_platform(
        InputEvent::mouse_down(ui::MouseButton::Left, bottom),
        &platform,
    );
    state.apply_input_with_platform(
        InputEvent::mouse_up(ui::MouseButton::Left, bottom),
        &platform,
    );
    assert!(
        state
            .to_settings_view_model()
            .unwrap()
            .animation_editor
            .unwrap()
            .scroll_offset
            > 0
    );
    state.apply_input_with_platform(
        InputEvent::mouse_down(ui::MouseButton::Left, (editor.close.x, editor.close.y)),
        &platform,
    );
    let model = state.to_settings_view_model().unwrap();
    assert!(model.animation_editor.is_none());
    assert_eq!(model.selected_field, SettingsField::AnimationAdvanced);
    assert_eq!(state.active_screen(), ShellScreen::Settings);
}

#[test]
fn animation_speed_is_per_user_adjustable_and_has_a_default_button() {
    let fixture = FixtureRoot::new("animation-speed");
    let platform = mock_platform(fixture.path());
    let manager = initialize_users(&platform, true, false);
    let mut state = logged_in_state(&platform, "NormalUser", "NormalPass123");

    open_settings_from_home(&mut state, &platform);
    for _ in 0..5 {
        press(&mut state, &platform, "Down");
    }
    let model = state.to_settings_view_model().unwrap();
    assert_eq!(model.selected_field, SettingsField::AnimationSpeed);
    let speed = model
        .cards
        .iter()
        .flat_map(|card| &card.items)
        .find(|item| item.field == SettingsField::AnimationSpeed)
        .expect("animation speed setting");
    assert_eq!(speed.value, "100%");
    assert_eq!(speed.kind, ui::SettingsControlKind::Stepper);
    assert!(speed.enabled);

    for key in ["Enter", "Right", "Enter"] {
        press(&mut state, &platform, key);
    }
    let normal_appearance = || {
        manager
            .load_users()
            .unwrap()
            .users
            .into_iter()
            .find(|user| user.username == "NormalUser")
            .unwrap()
            .appearance
    };
    assert_eq!(normal_appearance().animation_speed_percent, 125);

    let terminal_area = Rect::new(0, 0, 120, 40);
    let ui::ShellLayout::Full { main, .. } = ui::compute_shell_layout(terminal_area) else {
        panic!("expected full settings layout");
    };
    let speed_area = ui::settings_layout(main, &state.to_settings_view_model().unwrap())
        .fields
        .into_iter()
        .find(|field| field.field == SettingsField::AnimationSpeed)
        .expect("visible animation speed field")
        .area;
    state.apply_input_with_platform(
        InputEvent::mouse_down(ui::MouseButton::Left, (speed_area.x, speed_area.y)),
        &platform,
    );
    let number_model = state.to_settings_view_model().unwrap();
    let editor = number_model
        .number_editor
        .as_ref()
        .expect("animation speed number dialog");
    assert_eq!(editor.input.value(), "125");
    assert_eq!(normal_appearance().animation_speed_percent, 125);
    for key in ["Ctrl+A", "2", "0", "0", "Enter"] {
        press(&mut state, &platform, key);
    }
    assert_eq!(normal_appearance().animation_speed_percent, 200);
    assert!(
        state
            .to_settings_view_model()
            .unwrap()
            .number_editor
            .is_none()
    );
    for key in ["Enter", "Left", "Enter"] {
        press(&mut state, &platform, key);
    }
    assert_eq!(normal_appearance().animation_speed_percent, 175);

    press(&mut state, &platform, "Down");
    assert_eq!(
        state.to_settings_view_model().unwrap().selected_field,
        SettingsField::AnimationAdvanced
    );
    press(&mut state, &platform, "Down");
    assert_eq!(
        state.to_settings_view_model().unwrap().selected_field,
        SettingsField::ResetAnimationSpeed
    );
    press(&mut state, &platform, "Enter");
    assert_eq!(normal_appearance().animation_speed_percent, 100);

    press(&mut state, &platform, "Up");
    press(&mut state, &platform, "Up");
    press(&mut state, &platform, "Up");
    assert_eq!(
        state.to_settings_view_model().unwrap().selected_field,
        SettingsField::MotionPreference
    );
    press(&mut state, &platform, "Enter");
    assert_eq!(
        normal_appearance().motion_preference,
        MotionPreference::Reduced
    );
    let reduced = state.to_settings_view_model().unwrap();
    for field in [SettingsField::AnimationSpeed] {
        assert!(
            !reduced
                .cards
                .iter()
                .flat_map(|card| &card.items)
                .find(|item| item.field == field)
                .expect("animation setting")
                .enabled
        );
    }
    assert!(
        reduced
            .cards
            .iter()
            .flat_map(|card| &card.items)
            .find(|item| item.field == SettingsField::ResetAnimationSpeed)
            .unwrap()
            .enabled
    );
}

#[test]
fn default_theme_icon_picker_persists_ascii_and_image_modes_when_supported() {
    let fixture = FixtureRoot::new("theme-icon-mode-supported");
    let platform = mock_platform(fixture.path());
    let manager = initialize_users(&platform, true, false);
    let mut state = logged_in_state(&platform, "NormalUser", "NormalPass123");
    state.set_terminal_image_support(true);

    open_settings_from_home(&mut state, &platform);
    assert!(state.graphical_icons_enabled());
    assert_eq!(
        state.to_settings_view_model().unwrap().selected_field,
        SettingsField::Theme
    );

    press(&mut state, &platform, "Enter");
    assert_eq!(
        state
            .to_settings_view_model()
            .unwrap()
            .picker
            .as_ref()
            .map(|picker| picker.kind),
        Some(SettingsPickerKind::Theme)
    );
    press(&mut state, &platform, "Enter");
    let picker = state.to_settings_view_model().unwrap().picker.unwrap();
    assert_eq!(picker.kind, SettingsPickerKind::DefaultThemeIcons);
    assert_eq!(picker.options.len(), 2);
    assert!(picker.options.iter().all(|option| option.enabled));
    assert_eq!(picker.selected_index, 1);

    press(&mut state, &platform, "Home");
    press(&mut state, &platform, "Enter");
    let normal = manager
        .load_users()
        .unwrap()
        .users
        .into_iter()
        .find(|user| user.username == "NormalUser")
        .expect("normal user");
    assert_eq!(normal.appearance.icon_display_mode, IconDisplayMode::Ascii);
    assert!(!state.graphical_icons_enabled());

    press(&mut state, &platform, "Enter");
    press(&mut state, &platform, "Enter");
    press(&mut state, &platform, "Down");
    press(&mut state, &platform, "Enter");
    let normal = manager
        .load_users()
        .unwrap()
        .users
        .into_iter()
        .find(|user| user.username == "NormalUser")
        .expect("normal user");
    assert_eq!(normal.appearance.icon_display_mode, IconDisplayMode::Image);
    assert!(state.graphical_icons_enabled());
}

#[test]
fn unsupported_terminal_locks_default_theme_to_ascii_and_disables_image_option() {
    let fixture = FixtureRoot::new("theme-icon-mode-unsupported");
    let platform = mock_platform(fixture.path());
    initialize_users(&platform, true, false);
    let mut state = logged_in_state(&platform, "NormalUser", "NormalPass123");
    state.set_terminal_image_support(false);

    open_settings_from_home(&mut state, &platform);
    let appearance = state.to_settings_view_model().unwrap();
    let theme = appearance
        .cards
        .iter()
        .flat_map(|card| &card.items)
        .find(|item| item.field == SettingsField::Theme)
        .expect("Theme setting");
    assert_eq!(theme.value, "Default theme / ASCII icons");
    assert!(!state.graphical_icons_enabled());

    press(&mut state, &platform, "Enter");
    press(&mut state, &platform, "Enter");
    let picker = state.to_settings_view_model().unwrap().picker.unwrap();
    assert_eq!(picker.kind, SettingsPickerKind::DefaultThemeIcons);
    assert_eq!(picker.selected_index, 0);
    assert!(picker.options[0].enabled);
    assert!(!picker.options[1].enabled);

    press(&mut state, &platform, "Down");
    press(&mut state, &platform, "Enter");
    let model = state.to_settings_view_model().unwrap();
    assert!(model.picker.is_some());
    assert!(model.status.contains("unavailable"));
    assert!(!state.graphical_icons_enabled());
}

#[test]
fn guest_home_does_not_expose_settings() {
    let fixture = FixtureRoot::new("guest-hidden");
    let platform = mock_platform(fixture.path());
    initialize_users(&platform, false, true);
    let state = logged_in_state(&platform, "GuestUser", "GuestPass123");

    assert_eq!(state.active_screen(), ShellScreen::Home);
    assert!(
        state
            .to_home_view_model()
            .entries()
            .iter()
            .all(|entry| entry.label != "Settings")
    );
}

fn initialize_users(
    platform: &MockPlatform,
    create_user: bool,
    create_guest: bool,
) -> StorageManager {
    let manager = StorageManager::open_from_platform(platform)
        .expect("storage open")
        .manager;
    let users = UserService::new(manager.clone());
    users
        .bootstrap_admin("AdminUser", "StrongPass123")
        .expect("bootstrap admin");
    let admin = SessionService::new(manager.clone())
        .login("AdminUser", "StrongPass123")
        .expect("admin login");
    if create_user {
        users
            .create_user(
                &admin,
                "NormalUser",
                "Normal User",
                UserRole::User,
                "NormalPass123",
            )
            .expect("create normal user");
    }
    if create_guest {
        users
            .create_user(
                &admin,
                "GuestUser",
                "Guest User",
                UserRole::Guest,
                "GuestPass123",
            )
            .expect("create guest user");
    }
    manager
}

fn logged_in_state(platform: &MockPlatform, username: &str, password: &str) -> ShellSession {
    let startup = prepare_shell_startup(platform).expect("startup");
    let mut state = ShellSession::new_with_startup(default_config(), (120, 40), startup);
    assert_eq!(state.active_screen(), ShellScreen::Login);
    let target = state
        .to_login_view_model()
        .users
        .iter()
        .position(|user| user.username == username)
        .expect("login user");
    while state.to_login_view_model().selected_index < target {
        press(&mut state, platform, "Down");
    }
    press(&mut state, platform, "Tab");
    for character in password.chars() {
        state
            .apply_input_with_platform(InputEvent::from_key_label(character.to_string()), platform);
    }
    press(&mut state, platform, "Enter");
    assert_eq!(state.active_screen(), ShellScreen::Home);
    state
}

fn open_settings_from_home(state: &mut ShellSession, platform: &MockPlatform) {
    let target = state
        .to_home_view_model()
        .entries()
        .iter()
        .position(|entry| entry.label == "Settings")
        .expect("Settings home entry");
    while state.selected_home_entry_index() < target {
        press(state, platform, "Right");
    }
    press(state, platform, "Enter");
}

fn open_time_server_editor(state: &mut ShellSession, platform: &MockPlatform) {
    open_settings_from_home(state, platform);
    press(state, platform, "Tab");
    for _ in 0..4 {
        press(state, platform, "Down");
    }
    assert_eq!(
        state.to_settings_view_model().unwrap().selected_field,
        SettingsField::TimeSyncServer
    );
    press(state, platform, "Enter");
    assert!(
        state
            .to_settings_view_model()
            .unwrap()
            .time_sync_server_editor
            .is_some()
    );
}

fn press(state: &mut ShellSession, platform: &MockPlatform, key: &str) {
    let input = if key == "Ctrl+A" {
        InputEvent::Key(KeyInput::with_modifiers(
            InputKey::Char('a'),
            InputModifiers::ctrl(),
        ))
    } else {
        InputEvent::from_key_label(key)
    };
    state.apply_input_with_platform(input, platform);
}

fn select_field(state: &mut ShellSession, platform: &MockPlatform, field: SettingsField) {
    press(state, platform, "Home");
    let model = state.to_settings_view_model().unwrap();
    if model.selected_category == SettingsCategory::Applications {
        press(state, platform, "Right");
    }
    let index = model
        .cards
        .iter()
        .flat_map(|card| &card.items)
        .position(|item| item.field == field)
        .unwrap();
    for _ in 0..index {
        press(state, platform, "Down");
    }
    assert_eq!(
        state.to_settings_view_model().unwrap().selected_field,
        field
    );
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
            "tundra-shell-settings-{name}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        fs::create_dir_all(&path).expect("create fixture root");
        Self {
            path: fs::canonicalize(path).expect("canonicalize fixture root"),
        }
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

fn select_category(state: &mut ShellSession, platform: &MockPlatform, category: SettingsCategory) {
    for _ in 0..SettingsCategory::ALL.len() {
        if state.to_settings_view_model().unwrap().selected_category == category {
            return;
        }
        press(state, platform, "Tab");
    }
    panic!("settings category not reachable: {category:?}");
}

#[test]
fn unavailable_device_pages_are_removed_from_settings_on_every_platform() {
    for kind in [
        PlatformKind::Macos,
        PlatformKind::Windows,
        PlatformKind::Linux,
        PlatformKind::Unsupported,
    ] {
        let fixture = FixtureRoot::new(&format!("device-settings-{kind:?}"));
        let platform = mock_platform(fixture.path()).with_kind(kind);
        let manager = initialize_users(&platform, true, false);
        for (username, password) in [
            ("AdminUser", "StrongPass123"),
            ("NormalUser", "NormalPass123"),
        ] {
            let mut state = logged_in_state(&platform, username, password);
            open_settings_from_home(&mut state, &platform);
            let before = manager.load_config().unwrap();
            let users_before = manager.load_users().unwrap();
            for _ in 0..SettingsCategory::ALL.len() {
                let category = state.to_settings_view_model().unwrap().selected_category;
                assert!(!matches!(
                    category,
                    SettingsCategory::Sound
                        | SettingsCategory::Display
                        | SettingsCategory::Wifi
                        | SettingsCategory::Bluetooth
                ));
                press(&mut state, &platform, "Tab");
            }
            assert_eq!(manager.load_config().unwrap(), before);
            assert_eq!(manager.load_users().unwrap(), users_before);
        }
    }
}
