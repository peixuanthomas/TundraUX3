use super::*;

fn state() -> ShellSession {
    ShellSession::new_for_home_mode(ShellLaunchConfig::default(), (120, 40), ShellHomeMode::User)
}

fn back(state: &mut ShellSession, pointer: bool) {
    if pointer {
        state.refresh_hit_map();
        let area = state
            .hit_map()
            .regions()
            .iter()
            .find(|region| region.component == ShellComponent::BackButton)
            .unwrap()
            .area;
        let point = (area.x + area.width / 2, area.y + area.height / 2);
        state.apply_input(InputEvent::mouse_down(PointerButton::Left, point));
        state.apply_input(InputEvent::mouse_up(PointerButton::Left, point));
    } else {
        state.apply_input(InputEvent::from_key_label("Esc"));
    }
}

fn prepare_page(state: &mut ShellSession, screen: ShellScreen) {
    state.enter_screen(screen);
    if screen == ShellScreen::Editor {
        state.app.dispatch_at(
            app::AppCommand::SetEditorState(Some(EditorState::new())),
            Instant::now(),
        );
    }
    if screen == ShellScreen::Settings {
        state.settings_state = Some(SettingsState {
            category: ui::SettingsCategory::Appearance,
            selected_field: ui::SettingsField::Theme,
            status: i18n::msg!("settings-ready").into(),
            scroll_offset: 0,
            picker: None,
            color_editor: None,
            weather_location_editor: None,
            file_extensions_editor: None,
            time_sync_server_editor: None,
            time_sync_validation_request_id: None,
        });
    }
    state.refresh_hit_map();
}

#[test]
fn every_page_returns_to_actual_caller_for_escape_and_back_button() {
    for parent in [
        ShellScreen::Launcher,
        ShellScreen::Explorer,
        ShellScreen::Logs,
    ] {
        for page in [
            ShellScreen::Clock,
            ShellScreen::Diagnostics,
            ShellScreen::Logs,
            ShellScreen::Management,
            ShellScreen::SystemStatus,
            ShellScreen::Explorer,
            ShellScreen::Launcher,
            ShellScreen::Editor,
            ShellScreen::Settings,
            ShellScreen::UserManagement,
        ] {
            if parent == page {
                continue;
            }
            for pointer in [false, true] {
                let mut state = state();
                state.enter_screen(parent);
                prepare_page(&mut state, page);
                back(&mut state, pointer);
                assert_eq!(
                    state.screen_stack(),
                    &[ShellScreen::Home, parent],
                    "{parent:?} -> {page:?}, pointer={pointer}"
                );
                assert_eq!(state.focused_component, screen_focus(parent));
            }
        }
    }
}

#[test]
fn reset_discards_old_paths_and_wrong_page_close_is_ignored() {
    let mut state = state();
    state.enter_screen(ShellScreen::Launcher);
    state.enter_screen(ShellScreen::Editor);
    state.return_from_screen(ShellScreen::Launcher);
    assert_eq!(state.active_screen(), ShellScreen::Editor);
    state.enter_screen(ShellScreen::Editor);
    assert_eq!(state.screen_stack().len(), 3);
    state.reset_navigation(ShellScreen::Login);
    state.enter_screen(ShellScreen::ExitConfirm);
    state.cancel_exit_confirmation();
    assert_eq!(state.screen_stack(), &[ShellScreen::Login]);
    state.reset_navigation(ShellScreen::Home);
    state.enter_screen(ShellScreen::Launcher);
    state.close_launcher();
    assert_eq!(state.screen_stack(), &[ShellScreen::Home]);
}

#[test]
fn nested_visits_restore_each_callers_focus_and_ignore_held_escape() {
    let mut state = state();
    state.focused_component = ShellComponent::StatusBar;
    state.open_clock();
    state.focused_component = ShellComponent::ClockNewButton;
    prepare_page(&mut state, ShellScreen::UserManagement);
    back(&mut state, false);
    assert_eq!(state.active_screen(), ShellScreen::Clock);
    assert_eq!(state.focused_component, ShellComponent::ClockNewButton);
    for phase in [InputPhase::Repeat, InputPhase::Release] {
        state.apply_input(InputEvent::Key(KeyInput::with_phase(
            InputKey::Escape,
            InputModifiers::NONE,
            phase,
        )));
        assert_eq!(state.active_screen(), ShellScreen::Clock);
    }
    back(&mut state, true);
    assert_eq!(state.active_screen(), ShellScreen::Home);
    assert_eq!(state.focused_component, ShellComponent::StatusBar);
}

#[test]
fn notification_and_form_close_before_leaving_the_page() {
    for pointer in [false, true] {
        let mut state = state();
        state.enter_screen(ShellScreen::Launcher);
        prepare_page(&mut state, ShellScreen::UserManagement);
        state.user_management_mode = UserManagementMode::Create(UserManagementCreateForm {
            username: "draft".into(),
            display_name: String::new(),
            password: String::new(),
            role: UserRole::User,
            focused_field: UserManagementFormField::Username,
        });
        state.notify_modal(
            "Notice",
            "Close this first",
            ui::NotificationTone::Info,
            vec![ShellNotificationAction::new("cancel", "Cancel").cancel()],
        );
        back(&mut state, pointer);
        assert!(!state.notification_has_active_modal());
        assert!(matches!(
            state.user_management_mode,
            UserManagementMode::Create(_)
        ));
        back(&mut state, pointer);
        assert_eq!(state.user_management_mode, UserManagementMode::Browse);
        assert_eq!(state.active_screen(), ShellScreen::UserManagement);
        back(&mut state, pointer);
        assert_eq!(state.active_screen(), ShellScreen::Launcher);
    }
}

#[test]
fn dirty_editor_back_requests_confirmation_and_cancel_keeps_path() {
    for pointer in [false, true] {
        let mut state = state();
        state.enter_screen(ShellScreen::Logs);
        prepare_page(&mut state, ShellScreen::Editor);
        state.apply_input(InputEvent::from_key_label("x"));
        assert!(state.app.editor_state().unwrap().is_dirty());
        back(&mut state, pointer);
        assert!(state.notification_has_active_modal());
        assert_eq!(state.active_screen(), ShellScreen::Editor);
        back(&mut state, pointer);
        assert!(!state.notification_has_active_modal());
        assert!(state.app.editor_state().unwrap().is_dirty());
        assert_eq!(
            state.screen_stack(),
            &[ShellScreen::Home, ShellScreen::Logs, ShellScreen::Editor]
        );
    }
}

#[test]
fn editor_rollback_restores_picker_beneath_overlay_and_rejects_stale_results() {
    let mut state = state();
    state.enter_screen(ShellScreen::Launcher);
    state.enter_screen(ShellScreen::Editor);
    state.enter_screen(ShellScreen::Explorer);
    let rollback = state.begin_editor_navigation(true);
    state.open_clock();
    let operation = EditorLoadOperation::Open {
        navigation: EditorLoadNavigation::EditorPicker,
        rollback,
        reload: None,
        replacing_dirty: false,
    };
    state.restore_editor_load_navigation(&operation);
    state.restore_editor_load_navigation(&operation);
    assert_eq!(
        state.screen_stack(),
        &[
            ShellScreen::Home,
            ShellScreen::Launcher,
            ShellScreen::Editor,
            ShellScreen::Explorer,
            ShellScreen::Clock
        ]
    );
    state.close_clock();
    assert_eq!(state.active_screen(), ShellScreen::Explorer);
    assert_eq!(state.focused_component, ShellComponent::Explorer);

    state.return_from_screen(ShellScreen::Explorer);
    state.return_from_screen(ShellScreen::Editor);
    let rollback = state.begin_editor_navigation(false);
    state.return_from_screen(ShellScreen::Editor);
    state.enter_screen(ShellScreen::Editor);
    state.restore_editor_load_navigation(&EditorLoadOperation::Open {
        navigation: EditorLoadNavigation::Editor,
        rollback,
        reload: None,
        replacing_dirty: false,
    });
    assert_eq!(state.active_screen(), ShellScreen::Editor);
}

#[test]
fn command_line_escape_is_forwarded_and_back_requests_emergency_exit() {
    let mut state = state();
    state.enter_screen(ShellScreen::Explorer);
    state.enter_screen(ShellScreen::CommandLine);
    state.refresh_hit_map();
    let escape = InputEvent::from_key_label("Esc");
    assert_eq!(
        state.normalize_shell_navigation_input(escape.clone()),
        escape
    );
    let (_, command) = state.route_key_input(&KeyInput::from_label("Esc"));
    assert_eq!(
        command,
        ShellCommand::CommandLineKey(KeyInput::from_label("Esc"))
    );
    let area = state
        .hit_map()
        .regions()
        .iter()
        .find(|region| region.component == ShellComponent::BackButton)
        .unwrap()
        .area;
    let input = InputEvent::mouse_down(PointerButton::Left, (area.x, area.y));
    assert_eq!(
        state.normalize_shell_navigation_input(input),
        InputEvent::Key(KeyInput::with_modifiers(
            InputKey::Char('x'),
            InputModifiers::CTRL_SHIFT
        ))
    );
    state.close_command_line();
    assert_eq!(state.active_screen(), ShellScreen::Explorer);
}
