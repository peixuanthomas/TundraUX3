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
    assert_eq!(layout.items.len(), model.entries().len());
    assert!(layout.scrollbar.is_none());
    assert_eq!(
        layout.items.last().expect("last Home card").area.bottom(),
        main_area(&state).bottom() - 1,
        "the last card uses the space formerly reserved for Exit"
    );
    state.apply_input(InputEvent::key(InputKey::Escape));
    assert_eq!(state.active_screen(), ShellScreen::ExitConfirm);
    assert!(!state.shutdown_requested());
}

#[test]
fn home_scrollbar_reaches_tail_without_opening_a_card() {
    let mut state = state((72, 16));
    let main = main_area(&state);
    let layout = ui::home_layout(main, &state.to_home_view_model());
    let track = layout.scrollbar.expect("narrow Home overflows");
    let grab = (track.x, track.y);
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

#[test]
fn launcher_scrollbars_reach_tail_in_both_views_without_launch_or_reorder() {
    for mode in [
        ui::LauncherViewMode::LargeIcons,
        ui::LauncherViewMode::Details,
    ] {
        let mut state = state((72, 20));
        state.app.dispatch_at(
            app::AppCommand::SetLauncherState(Some(LauncherState {
                items: (0..30)
                    .map(|index| app::launcher::LauncherItem {
                        record: storage::LauncherEntryRecord {
                            id: format!("app-{index}"),
                            path: format!("/touch-test/app-{index}"),
                            executable_kind: Some(storage::LauncherExecutableKind::NativeBinary),
                            fingerprint: None,
                            added_by_user_id: "admin".into(),
                            added_at_epoch_ms: 0,
                        },
                        status: LauncherItemStatus::Ready,
                    })
                    .collect(),
                ..Default::default()
            })),
            Instant::now(),
        );
        state.screen_stack.push(ShellScreen::Launcher);
        state.launcher_view_mode = mode;
        state.refresh_hit_map();
        let original = state.app.launcher_state().unwrap().items.clone();
        let main = main_area(&state);
        let layout = ui::launcher_layout(main, &state.to_launcher_view_model());
        let track = layout.scrollbar.expect("overflowing Launcher");
        let bottom = (track.x, track.bottom().saturating_add(10));
        for event in [
            InputEvent::mouse_down(PointerButton::Left, (track.x, track.y)),
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
        assert!(state.launcher_drag.is_none());
        assert!(state.scrollbar_drag.is_none());
    }
}

#[test]
fn launcher_open_button_waits_for_release_and_drag_cancels_activation() {
    let mut state = state((72, 20));
    state.screen_stack.push(ShellScreen::Launcher);
    state.launcher_selected_index = state
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
