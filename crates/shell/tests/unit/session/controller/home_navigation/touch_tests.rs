use super::*;
use ratatui::{Terminal, backend::TestBackend};

fn state(size: (u16, u16)) -> ShellSession {
    let mut state =
        ShellSession::new_for_home_mode(ShellLaunchConfig::default(), size, ShellHomeMode::User);
    state.app.dispatch_at(
        app::AppCommand::SetAuthSession(Some(AuthSession {
            source: identity::IdentitySource::LocalAccount,
            session_id: "touch-test".into(),
            user_id: "admin".into(),
            username: "admin".into(),
            role: UserRole::Admin,
            started_at_epoch_ms: 1,
        })),
        Instant::now(),
    );
    while state.notification_dismiss_active_modal_without_response() {}
    state.refresh_hit_map();
    state
}

fn main_area(state: &ShellSession) -> Rect {
    let ui::ShellLayout::Full { main, .. } = state.shell_layout_for(Rect::new(
        0,
        0,
        state.terminal_size.0,
        state.terminal_size.1,
    )) else {
        panic!("full test layout");
    };
    main
}

fn draw_buttons(state: &mut ShellSession) {
    let theme = ui::TundraTheme::default_dark();
    let buttons = ui::components::ButtonFrame::new(None, None, &theme);
    let mut context = ui::RenderContext::from_theme(&theme, Default::default(), Default::default());
    context.buttons = Some(buttons.clone());
    let main = main_area(state);
    let mut terminal = Terminal::new(TestBackend::new(
        state.terminal_size.0,
        state.terminal_size.1,
    ))
    .unwrap();
    terminal
        .draw(|frame| match state.active_screen() {
            ShellScreen::Home => {
                ui::render_home_content(frame, main, &state.to_home_view_model(), &context, None)
            }
            ShellScreen::Launcher => ui::render_launcher_content(
                frame,
                main,
                &state.to_launcher_view_model(),
                &context,
                None,
            ),
            _ => panic!("unexpected touch test page"),
        })
        .unwrap();
    state.button_regions = buttons.regions();
}

#[test]
fn core_home_tasks_are_visible_and_open_without_launcher_at_minimum_size() {
    for size in [(108, 20), (140, 40)] {
        let mut state = state(size);
        draw_buttons(&mut state);
        let model = state.to_home_view_model();
        let layout = ui::home_layout(main_area(&state), &model);
        assert_eq!(layout.items.len(), model.entries().len());
        assert!(layout.scrollbar.is_none());
        for key in ["packages", "tasks", "logs", "users"] {
            let index = model
                .entries()
                .iter()
                .position(|entry| entry.icon_identity() == key)
                .unwrap();
            let item = layout
                .items
                .iter()
                .find(|item| item.index == index)
                .unwrap();
            assert_eq!(
                state.home_entry_index_at((item.area.x, item.area.y)),
                Some(index)
            );
        }
        let index = model
            .entries()
            .iter()
            .position(|entry| entry.icon_identity() == "tasks")
            .unwrap();
        state.select_home_entry(index);
        state.apply_input(InputEvent::key(InputKey::Enter));
        assert_eq!(state.active_screen(), ShellScreen::Management);
        assert!(
            state
                .to_management_view_model()
                .title
                .starts_with(&i18n::tr!("tasks-title"))
        );
    }
}

#[test]
fn launcher_excludes_applications_already_available_on_home() {
    let state = state((108, 20));
    let home = state.user_home_entries();
    let launcher = state.to_launcher_view_model();
    for key in ["packages", "users", "tasks", "logs"] {
        assert!(home.iter().any(|entry| entry.icon_identity() == key));
    }
    for entry in &home {
        let id = format!("builtin.{}", entry.icon_identity());
        assert!(
            launcher.items.iter().all(|item| item.id != id),
            "{id} must only be shown on Home"
        );
    }
    assert!(
        launcher
            .items
            .iter()
            .any(|item| item.id == "builtin.editor")
    );
    assert!(
        launcher
            .items
            .iter()
            .any(|item| item.id == "builtin.command-line")
    );
}

#[test]
fn home_has_no_page_exit_button_and_reuses_its_space_for_cards() {
    // Three card rows need 13 cells: three 3-cell cards and two 2-cell gaps.
    // At this height they fit only after the old 2-cell Exit footer is removed.
    let mut state = state((72, 22));
    draw_buttons(&mut state);
    assert!(
        state
            .button_regions
            .iter()
            .all(|button| !button.id.as_str().starts_with("home.toolbar."))
    );
    let model = state.to_home_view_model();
    let layout = ui::home_layout(main_area(&state), &model);
    assert!(!layout.items.is_empty());
    assert!(model.entries().iter().all(|entry| !matches!(
        entry.icon_key.as_deref(),
        Some("user_management" | "user_profile")
    )));
    // Keep the three-row geometry check independent of the application count.
    let three_rows = ui::home_entry_tile_areas(main_area(&state), layout.columns * 3);
    assert_eq!(
        three_rows
            .last()
            .expect("last card in a full three-row grid")
            .bottom(),
        main_area(&state).bottom() - 1,
        "the last card uses the space formerly reserved for Exit"
    );
    state.apply_input(InputEvent::key(InputKey::Escape));
    assert_eq!(state.active_screen(), ShellScreen::ExitConfirm);
    assert!(!state.shutdown_requested());
}

#[test]
fn home_letters_are_inactive_and_launcher_shortcuts_require_fresh_unmodified_keys() {
    let mut state = state((120, 40));
    for (key, modifiers) in [
        (InputKey::Char('l'), InputModifiers::ALT),
        (InputKey::Char('l'), InputModifiers::CTRL_SHIFT),
        (InputKey::Char('q'), InputModifiers::ALT),
        (InputKey::Enter, InputModifiers::ALT),
    ] {
        assert_eq!(
            state
                .route_key_input(&KeyInput::with_modifiers(key, modifiers))
                .1,
            ShellCommand::RecordInput,
        );
    }
    let home = state.active_page_id();
    for label in ["E", "e", "A", "a", "S", "s", "M", "m", "U", "u", "T", "t"] {
        for phase in [InputPhase::Press, InputPhase::Repeat, InputPhase::Release] {
            let mut key = KeyInput::from_label(label);
            key.phase = phase;
            state.apply_input(InputEvent::Key(key));
            assert_eq!(state.active_page_id(), home, "Home key {label} {phase:?}");
            assert!(matches!(
                state.last_command(),
                Some(ShellCommand::RecordInput | ShellCommand::Noop)
            ));
        }
    }
    state.enter_screen(ShellScreen::Launcher);
    for (label, command) in [
        ("R", ShellCommand::LauncherRefresh),
        ("F5", ShellCommand::LauncherRefresh),
        ("V", ShellCommand::LauncherToggleView),
        ("Enter", ShellCommand::LauncherActivate),
        ("Delete", ShellCommand::LauncherRemove),
    ] {
        assert_eq!(
            state.route_key_input(&KeyInput::from_label(label)).1,
            command
        );
        assert_ne!(
            state
                .route_key_input(&KeyInput::with_modifiers(
                    KeyInput::from_label(label).key,
                    InputModifiers::ALT
                ))
                .1,
            command
        );
        let mut repeated = KeyInput::from_label(label);
        repeated.phase = InputPhase::Repeat;
        assert_ne!(state.route_key_input(&repeated).1, command);
    }
}

#[test]
fn home_scrollbar_reaches_tail_without_opening_a_card() {
    for offset in [-1, 0, 1] {
        let mut state = state((72, 16));
        let main = main_area(&state);
        let layout = ui::home_layout(main, &state.to_home_view_model());
        let track = layout.scrollbar.expect("narrow Home overflows");
        let grab = (track.x.saturating_add_signed(offset), track.y);
        let bottom = (track.x, track.bottom().saturating_add(10));
        for event in [
            InputEvent::mouse_down(PointerButton::Left, grab),
            InputEvent::mouse_drag(PointerButton::Left, bottom),
            InputEvent::mouse_up(PointerButton::Left, bottom),
        ] {
            state.apply_input(event);
        }
        let model = state.to_home_view_model();
        let layout = ui::home_layout(main, &model);
        assert!(layout.visible_start > 0);
        assert!(
            layout
                .items
                .iter()
                .any(|item| item.index == model.entries().len() - 1)
        );
        assert!(matches!(state.scrollbar_drag, None));
        assert_eq!(state.active_screen(), ShellScreen::Home);
        state.apply_input(InputEvent::from_key_label("Home"));
        assert_eq!(
            ui::home_layout(main, &state.to_home_view_model()).visible_start,
            0
        );
    }
}

#[test]
fn launcher_scrollbars_reach_tail_in_both_views_without_launch_or_reorder() {
    for mode in [
        ui::LauncherViewMode::LargeIcons,
        ui::LauncherViewMode::Details,
    ] {
        for offset in [-1, 0, 1] {
            let mut state = state((72, 20));
            state.enter_screen(ShellScreen::Launcher);
            state.app.dispatch_at(
                app::AppCommand::SetLauncherState(Some(LauncherState {
                    items: (0..30)
                        .map(|index| app::launcher::LauncherItem {
                            record: storage::LauncherEntryRecord {
                                id: format!("app-{index}"),
                                path: format!("/touch-test/app-{index}"),
                                executable_kind: Some(
                                    storage::LauncherExecutableKind::NativeBinary,
                                ),
                                fingerprint: None,
                                added_by_user_id: "admin".into(),
                                added_at_epoch_ms: 0,
                                terminal: None,
                                custom: None,
                                name: None,
                                icon: None,
                            },
                            status: LauncherItemStatus::Ready,
                        })
                        .collect(),
                    ..Default::default()
                })),
                Instant::now(),
            );
            state.pages.launcher.launcher_view_mode = mode;
            state.refresh_hit_map();
            let original = state.app.launcher_state().unwrap().items.clone();
            let main = main_area(&state);
            let layout = ui::launcher_layout(main, &state.to_launcher_view_model());
            let track = layout.scrollbar.expect("overflowing Launcher");
            let bottom = (track.x, track.bottom().saturating_add(10));
            for event in [
                InputEvent::mouse_down(
                    PointerButton::Left,
                    (track.x.saturating_add_signed(offset), track.y),
                ),
                InputEvent::mouse_drag(PointerButton::Left, bottom),
                InputEvent::mouse_up(PointerButton::Left, bottom),
            ] {
                state.apply_input(event);
            }
            let model = state.to_launcher_view_model();
            let layout = ui::launcher_layout(main, &model);
            assert!(
                layout
                    .items
                    .iter()
                    .any(|item| item.index == model.items.len() - 1)
            );
            assert_eq!(state.active_screen(), ShellScreen::Launcher);
            assert_eq!(state.app.launcher_state().unwrap().items, original);
            assert!(state.pages.launcher.launcher_drag.is_none());
            assert!(state.scrollbar_drag.is_none());
        }
    }
}

#[test]
fn launcher_open_button_waits_for_release_and_drag_cancels_activation() {
    let mut state = state((72, 20));
    state.enter_screen(ShellScreen::Launcher);
    state.pages.launcher.launcher_selected_index = state
        .built_in_launcher_applications()
        .iter()
        .position(|app| app.id == app::EDITOR_APPLICATION.id)
        .unwrap();
    state.refresh_hit_map();
    draw_buttons(&mut state);
    let layout = ui::launcher_layout(main_area(&state), &state.to_launcher_view_model());
    let area = layout
        .toolbar_buttons
        .iter()
        .find(|button| button.action == ui::LauncherToolbarAction::Open)
        .unwrap()
        .area;
    let point = (area.x + 1, area.y);
    state.apply_input(InputEvent::mouse_down(PointerButton::Left, point));
    assert_eq!(state.active_screen(), ShellScreen::Launcher);
    state.apply_input(InputEvent::mouse_drag(PointerButton::Left, (0, 0)));
    state.apply_input(InputEvent::mouse_up(PointerButton::Left, point));
    assert_eq!(state.active_screen(), ShellScreen::Launcher);
    state.apply_input(InputEvent::mouse_down(PointerButton::Left, point));
    state.apply_input(InputEvent::mouse_up(PointerButton::Left, point));
    assert_eq!(state.active_screen(), ShellScreen::Editor);
}

#[test]
fn global_tasks_entry_toggles_tasks_and_restores_the_original_page() {
    for size in [(108, 20), (140, 40)] {
        let mut state = state(size);
        state.open_settings();
        let original_page = state.active_page_id();
        let original_stack = state.screen_stack().to_vec();
        for _ in 0..3 {
            state.refresh_hit_map();
            let area = state
                .frame_layout
                .unwrap()
                .tasks_button
                .expect("visible task entry");
            let point = (area.x + 1, area.y + 1);
            state.apply_input(InputEvent::mouse_down(PointerButton::Left, point));
            state.apply_input(InputEvent::mouse_up(PointerButton::Left, point));
            assert_eq!(state.active_screen(), ShellScreen::Management);
            assert!(
                state
                    .to_management_view_model()
                    .title
                    .starts_with(&i18n::tr!("tasks-title"))
            );
            assert_eq!(state.screen_stack().len(), original_stack.len() + 1);
            let tasks_page = state.active_page_id();
            state.apply_input(InputEvent::mouse_down(PointerButton::Left, point));
            state.apply_input(InputEvent::mouse_up(PointerButton::Left, point));
            assert_eq!(state.active_page_id(), original_page);
            assert_eq!(state.screen_stack(), original_stack);
            assert!(state.page_screen(tasks_page).is_none());
        }
    }
}

#[test]
fn global_tasks_entry_closes_tasks_opened_from_home() {
    let mut state = state((108, 20));
    let home = state.active_page_id();
    let index = state
        .user_home_entries()
        .iter()
        .position(|entry| entry.icon_identity() == "tasks")
        .unwrap();
    state.select_home_entry(index);
    state.apply_input(InputEvent::key(InputKey::Enter));
    assert_eq!(state.active_screen(), ShellScreen::Management);
    let area = state.frame_layout.unwrap().tasks_button.unwrap();
    let point = (area.x + 1, area.y + 1);
    state.apply_input(InputEvent::mouse_down(PointerButton::Left, point));
    state.apply_input(InputEvent::mouse_up(PointerButton::Left, point));
    assert_eq!(state.active_page_id(), home);
    assert_eq!(state.screen_stack(), &[ShellScreen::Home]);
    assert_eq!(state.selected_home_entry_index(), index);
}

#[test]
fn global_task_status_distinguishes_operations_queries_and_pending_approval() {
    let mut state = state((108, 20));
    let initial = state.tasks_button_label().unwrap();
    state.tasks.register(
        TaskDomain::Logs,
        1,
        TaskOwner::Session,
        TaskClass::Query,
        TaskControl::new(|| {}, || false),
    );
    assert_eq!(state.tasks_button_label().unwrap(), initial);
    state.tasks.register(
        TaskDomain::Explorer,
        2,
        TaskOwner::Session,
        TaskClass::Operation,
        TaskControl::new(|| {}, || false),
    );
    assert_eq!(
        state.tasks_button_label().unwrap(),
        i18n::tr!("ui-shell-tasks-active")
    );
    let (tx, _rx) = mpsc::channel();
    state
        .begin_auto_admin("test approval".into(), true, tx)
        .unwrap();
    assert_eq!(
        state.tasks_button_label().unwrap(),
        i18n::tr!("ui-shell-tasks-waiting")
    );
}
