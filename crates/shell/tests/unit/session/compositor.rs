use super::*;
use ratatui::{Terminal, backend::TestBackend, buffer::Buffer};

fn session() -> ShellSession {
    let mut state = ShellSession::new_for_home_mode(
        ShellLaunchConfig::default(),
        (120, 40),
        ShellHomeMode::User,
    );
    while state.notification_dismiss_active_modal_without_response() {}
    state.screen_stack = vec![ShellScreen::Home];
    state.refresh_hit_map();
    state
}

fn home_frame(state: &ShellSession, millis: u64, reduced: bool) -> PreparedFrame {
    PreparedFrame {
        page: ScreenViewModel::Home(Box::new(state.to_home_view_model())),
        chrome: state.to_shell_chrome_view_model(),
        context: ui::RenderContext::from_theme(
            &ui::TundraTheme::default_dark(),
            ui::MotionFrame {
                now: Duration::from_millis(millis),
                delta: Duration::from_millis(50),
                reduced_motion: reduced,
                animation_speed_percent: 100,
            },
            Default::default(),
        ),
        notification: None,
        time_sync: None,
        progress_running: false,
    }
}

fn draw(
    compositor: &mut ScreenCompositor,
    terminal: &mut Terminal<TestBackend>,
    state: &mut ShellSession,
    frame: &PreparedFrame,
) -> bool {
    let mut running = false;
    terminal
        .draw(|target| running = compositor.render(target, state, frame, None))
        .unwrap();
    running
}

fn text(buffer: &Buffer, area: Rect) -> String {
    (area.y..area.bottom())
        .map(|y| {
            (area.x..area.right())
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn auto_admin_keeps_shell_title_and_status_visible_after_resize() {
    let mut state = session();
    let (responses, _inputs) = mpsc::channel();
    state
        .begin_auto_admin("Remove neofetch".into(), false, responses)
        .unwrap();
    for (width, height) in [(180, 55), (80, 24), (120, 40)] {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        let mut compositor = ScreenCompositor::default();
        state.close_auto_admin();
        let prepared = home_frame(&state, 0, true);
        draw(&mut compositor, &mut terminal, &mut state, &prepared);
        let ui::ShellLayout::Full { top, main, status } =
            ui::compute_shell_layout(Rect::new(0, 0, width, height))
        else {
            panic!("full shell layout expected");
        };
        let title_before = text(terminal.backend().buffer(), top);
        let status_before = text(terminal.backend().buffer(), status);
        state.apply_input(InputEvent::from_key_label("F12"));
        state.apply_input(InputEvent::Key(KeyInput::with_phase(
            InputKey::F(12),
            InputModifiers::NONE,
            InputPhase::Release,
        )));
        draw(&mut compositor, &mut terminal, &mut state, &prepared);
        assert_eq!(text(terminal.backend().buffer(), top), title_before);
        assert_eq!(text(terminal.backend().buffer(), status), status_before);
        assert!(text(terminal.backend().buffer(), main).contains("AutoAdmin (AA)"));
    }
}

#[test]
fn keyboard_selection_hides_for_pointer_input_and_returns_on_navigation() {
    for mode in 0..3 {
        let launcher = mode != 0;
        let mut state = session();
        if launcher {
            state.screen_stack.push(ShellScreen::Launcher);
            if mode == 2 {
                state.launcher_view_mode = ui::LauncherViewMode::Details;
            }
            state.refresh_hit_map();
        }
        let mut compositor = ScreenCompositor::default();
        let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
        let render = |state: &mut ShellSession,
                      compositor: &mut ScreenCompositor,
                      terminal: &mut Terminal<TestBackend>| {
            let mut prepared = home_frame(state, 0, true);
            if launcher {
                prepared.page = ScreenViewModel::Launcher(Box::new(state.to_launcher_view_model()));
            }
            draw(compositor, terminal, state, &prepared);
        };
        render(&mut state, &mut compositor, &mut terminal);
        assert!(state.keyboard_focus_visible);
        state.apply_input(InputEvent::key(InputKey::Home));
        assert!(state.keyboard_focus_visible);
        render(&mut state, &mut compositor, &mut terminal);
        let id = if launcher {
            format!("launcher.item.{}", app::EDITOR_APPLICATION.id)
        } else {
            "home.entry.0".to_string()
        };
        let area = if mode == 2 {
            let ui::ShellLayout::Full { main, .. } =
                state.shell_layout_for(Rect::new(0, 0, 120, 40))
            else {
                panic!("full layout required");
            };
            ui::launcher_layout(main, &state.to_launcher_view_model()).items[0].area
        } else {
            state
                .button_regions
                .iter()
                .find(|region| region.id.as_str() == id)
                .unwrap()
                .area
        };
        let border = (area.x, area.y);
        let focused_color = terminal.backend().buffer()[border].fg;
        let selection = if launcher {
            state.launcher_selected_index
        } else {
            state.selected_home_entry_index
        };
        for pointer in [
            InputEvent::mouse_moved((0, 0)),
            InputEvent::mouse_down(PointerButton::Left, (0, 0)),
        ] {
            state.apply_input(pointer);
            assert!(!state.keyboard_focus_visible);
            render(&mut state, &mut compositor, &mut terminal);
            assert_ne!(terminal.backend().buffer()[border].fg, focused_color);
            assert_eq!(
                if launcher {
                    state.launcher_selected_index
                } else {
                    state.selected_home_entry_index
                },
                selection
            );
            for ignored in [
                InputEvent::Key(KeyInput::new(InputKey::Home).released()),
                InputEvent::key(InputKey::Char('z')),
                InputEvent::Tick,
                InputEvent::FocusGained,
            ] {
                state.apply_input(ignored);
                assert!(!state.keyboard_focus_visible);
            }
            state.apply_input(InputEvent::key(InputKey::Home));
            render(&mut state, &mut compositor, &mut terminal);
            assert_eq!(terminal.backend().buffer()[border].fg, focused_color);
        }
        state.apply_input(InputEvent::FocusLost);
        assert!(!state.keyboard_focus_visible);
    }
}

#[test]
fn pointer_back_button_does_not_restore_keyboard_focus() {
    let mut state = session();
    state.apply_input(InputEvent::key(InputKey::Home));
    let mut compositor = ScreenCompositor::default();
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    let prepared = home_frame(&state, 0, true);
    draw(&mut compositor, &mut terminal, &mut state, &prepared);
    let area = state
        .button_regions
        .iter()
        .find(|region| region.id.as_str() == "shell.back")
        .unwrap()
        .area;
    let point = (area.x, area.y);
    state.apply_input(InputEvent::mouse_down(PointerButton::Left, point));
    assert!(!state.keyboard_focus_visible);
    state.apply_input(InputEvent::mouse_up(PointerButton::Left, point));
    assert_eq!(state.active_screen(), ShellScreen::ExitConfirm);
    assert!(!state.keyboard_focus_visible);
}

#[test]
fn compact_escape_button_closes_pages_and_modals_only_on_matching_release() {
    for (width, height) in [(49, 11), (30, 8)] {
        let mut state = session();
        state.apply_input(InputEvent::Resize { width, height });
        state.screen_stack.push(ShellScreen::Clock);
        state.refresh_hit_map();
        let mut compositor = ScreenCompositor::default();
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        let mut prepared = home_frame(&state, 0, true);
        prepared.page = ScreenViewModel::Clock(Box::new(
            state.to_clock_view_model_at(&state.app.snapshot().clock, Instant::now()),
        ));
        draw(&mut compositor, &mut terminal, &mut state, &prepared);
        let back = state.frame_layout.unwrap().back_button.unwrap();
        assert!(text(terminal.backend().buffer(), back).contains("[◀]"));
        assert_eq!(
            back.intersection(state.frame_layout.unwrap().main).area(),
            0
        );
        let point = (back.x + back.width / 2, back.y);
        let now = Instant::now();
        state.apply_input_at(InputEvent::mouse_down(PointerButton::Left, point), now);
        assert_eq!(state.active_screen(), ShellScreen::Clock);
        state.apply_input_at(
            InputEvent::mouse_up(PointerButton::Left, point),
            now + Duration::from_millis(10),
        );
        assert_eq!(state.active_screen(), ShellScreen::Home);

        // Home still reaches the exit dialog through the same global button.
        let prepared = home_frame(&state, 0, true);
        draw(&mut compositor, &mut terminal, &mut state, &prepared);
        state.apply_input_at(InputEvent::mouse_down(PointerButton::Left, point), now);
        assert_eq!(state.active_screen(), ShellScreen::Home);
        state.apply_input_at(
            InputEvent::mouse_up(PointerButton::Left, point),
            now + Duration::from_millis(10),
        );
        assert_eq!(state.active_screen(), ShellScreen::ExitConfirm);
        let mut prepared = home_frame(&state, 0, true);
        prepared.notification = state.to_notification_view_model();
        draw(&mut compositor, &mut terminal, &mut state, &prepared);
        assert!(text(terminal.backend().buffer(), back).contains("[◀]"));
        assert_eq!(
            state.hit_map().target_at(point),
            Some(ShellComponent::BackButton)
        );
        if let ui::NotificationLayout::Dialog(dialog) = ui::notification_layout(
            state.frame_layout.unwrap().modal_area(),
            prepared.notification.as_ref().unwrap(),
        ) {
            assert_eq!(dialog.dialog.intersection(back).area(), 0);
            for action in &dialog.actions {
                assert_eq!(
                    state.notification_action_index_at((action.area.x, action.area.y)),
                    Some(action.index)
                );
            }
        } else {
            assert_eq!((width, height), (30, 8));
        }
        state.apply_input_at(InputEvent::mouse_down(PointerButton::Left, point), now);
        assert_eq!(state.active_screen(), ShellScreen::ExitConfirm);
        state.apply_input_at(
            InputEvent::mouse_up(PointerButton::Left, point),
            now + Duration::from_millis(10),
        );
        assert_eq!(state.active_screen(), ShellScreen::Home);
        assert!(!state.shutdown_requested());
    }
}

#[test]
fn animated_toast_leaves_clock_pixels_and_mouse_target_intact() {
    let mut state = session();
    let mut compositor = ScreenCompositor::default();
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    let mut initial = home_frame(&state, 0, false);
    initial.chrome.status.time_button_label = Some("2026-09-25 12:59".into());
    draw(&mut compositor, &mut terminal, &mut state, &initial);
    let clock = state.frame_layout.unwrap().time_button.unwrap();
    let baseline: Vec<_> = clock
        .positions()
        .map(|p| terminal.backend().buffer()[p].clone())
        .collect();
    for millis in [0, 50, 150, 300] {
        let mut prepared = home_frame(&state, millis, false);
        prepared.chrome.status.time_button_label = initial.chrome.status.time_button_label.clone();
        prepared.chrome.status.toast = Some("Saved changes".into());
        compositor.synchronize_toast(&prepared.chrome, prepared.context.motion);
        draw(&mut compositor, &mut terminal, &mut state, &prepared);
        assert_eq!(
            clock
                .positions()
                .map(|p| terminal.backend().buffer()[p].clone())
                .collect::<Vec<_>>(),
            baseline
        );
        assert!(
            state
                .hit_map()
                .regions()
                .iter()
                .any(|region| region.component == ShellComponent::ClockButton
                    && region.area == clock)
        );
    }
    let mut next = home_frame(&state, 350, false);
    next.chrome.status.time_button_label = Some("2026-09-25 13:00".into());
    next.chrome.status.toast = Some("Saved changes".into());
    compositor.synchronize_toast(&next.chrome, next.context.motion);
    draw(&mut compositor, &mut terminal, &mut state, &next);
    assert!(text(terminal.backend().buffer(), clock).contains("13:00"));
}

#[test]
fn alert_preempts_an_exiting_toast_and_reduced_motion_has_no_animation_demand() {
    let mut state = session();
    let mut compositor = ScreenCompositor::default();
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    let mut prepared = home_frame(&state, 0, false);
    prepared.chrome.status.toast = Some("Saved changes".into());
    compositor.synchronize_toast(&prepared.chrome, prepared.context.motion);
    draw(&mut compositor, &mut terminal, &mut state, &prepared);
    prepared.context.motion.now = Duration::from_millis(100);
    prepared.chrome.status.toast = None;
    compositor.synchronize_toast(&prepared.chrome, prepared.context.motion);
    assert!(compositor.toast.is_some());
    prepared.chrome.status.error = Some("Disk is full".into());
    prepared.context.motion = ui::MotionFrame::reduced(Duration::from_millis(150));
    compositor.synchronize_toast(&prepared.chrome, prepared.context.motion);
    assert!(compositor.toast.is_none());
    assert!(!draw(&mut compositor, &mut terminal, &mut state, &prepared));
    let output = text(
        terminal.backend().buffer(),
        state.frame_layout.unwrap().status_message.unwrap(),
    );
    assert!(output.contains("Disk is full"));
    assert!(!output.contains("Saved changes"));
}

#[test]
fn compositor_resize_uses_actual_frame_geometry_for_chrome_and_hit_map() {
    let mut state = session();
    let mut compositor = ScreenCompositor::default();
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    let prepared = home_frame(&state, 0, true);
    draw(&mut compositor, &mut terminal, &mut state, &prepared);
    terminal.backend_mut().resize(90, 25);
    draw(&mut compositor, &mut terminal, &mut state, &prepared);
    let layout = state.frame_layout.unwrap();
    assert_eq!(layout.bounds, Rect::new(0, 0, 90, 25));
    assert_eq!(state.hit_map().terminal_size(), (90, 25));
    assert!(text(terminal.backend().buffer(), Rect::new(0, 0, 90, 3)).contains("90x25"));
    for region in state.hit_map().regions() {
        assert_eq!(region.area.intersection(layout.bounds), region.area);
    }
    terminal.backend_mut().resize(49, 11);
    draw(&mut compositor, &mut terminal, &mut state, &prepared);
    assert!(state.frame_layout.unwrap().is_compact());
    assert!(state.hit_map().regions().iter().all(|region| !matches!(
        region.component,
        ShellComponent::TopBar | ShellComponent::StatusBar | ShellComponent::ClockButton
    )));
}

#[test]
fn global_notification_is_the_only_shell_modal_visual() {
    let mut state = session();
    let mut compositor = ScreenCompositor::default();
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    let mut prepared = home_frame(&state, 0, true);
    prepared.notification = Some(ui::NotificationViewModel::new(
        "test.modal",
        ui::NotificationLevel::Modal,
        ui::NotificationTone::Warning,
        "Confirm operation",
        "Keep this dialog visible",
        vec![],
    ));
    prepared.time_sync = Some(ui::TimeSyncDialogViewModel::new());
    prepared.chrome.status.toast = Some("Background toast".into());
    compositor.synchronize_toast(&prepared.chrome, prepared.context.motion);
    draw(&mut compositor, &mut terminal, &mut state, &prepared);
    let output = text(terminal.backend().buffer(), Rect::new(0, 0, 120, 40));
    assert!(output.contains("Keep this dialog visible"));
    assert!(!output.contains(&ui::TimeSyncDialogViewModel::new().message()));
}

#[test]
fn rendered_chrome_buttons_hover_press_and_activate_only_on_release() {
    let mut state = session();
    let mut compositor = ScreenCompositor::default();
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    let mut prepared = home_frame(&state, 0, true);
    prepared.chrome.status.time_button_label = Some("12:00".into());
    draw(&mut compositor, &mut terminal, &mut state, &prepared);
    let back = state
        .button_regions
        .iter()
        .find(|r| r.id.as_str() == "shell.back")
        .unwrap()
        .clone();
    let point = (back.area.x + 3, back.area.y + 1);
    let theme = prepared.context.compatibility_theme();
    state.apply_input(InputEvent::mouse_moved(point));
    draw(&mut compositor, &mut terminal, &mut state, &prepared);
    assert_eq!(
        terminal.backend().buffer()[point].fg,
        theme.button_hover_color()
    );
    state.apply_input(InputEvent::mouse_down(PointerButton::Left, point));
    assert_eq!(state.active_screen(), ShellScreen::Home);
    draw(&mut compositor, &mut terminal, &mut state, &prepared);
    assert_eq!(
        terminal.backend().buffer()[point].fg,
        theme.button_pressed_color()
    );
    state.apply_input(InputEvent::mouse_up(PointerButton::Left, point));
    assert_eq!(state.active_screen(), ShellScreen::ExitConfirm);
    assert!(state.button_pointer_capture.is_none());
}

#[test]
fn button_capture_cancels_outside_on_focus_loss_resize_and_page_change() {
    for cancel in 0..5 {
        let mut state = session();
        let mut compositor = ScreenCompositor::default();
        let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
        let prepared = home_frame(&state, 0, true);
        draw(&mut compositor, &mut terminal, &mut state, &prepared);
        let back = state
            .button_regions
            .iter()
            .find(|r| r.id.as_str() == "shell.back")
            .unwrap()
            .area;
        let point = (back.x + 3, back.y + 1);
        state.apply_input(InputEvent::mouse_down(PointerButton::Left, point));
        match cancel {
            0 => {
                state.apply_input(InputEvent::mouse_up(PointerButton::Left, (0, 0)));
            }
            1 => {
                state.apply_input(InputEvent::FocusLost);
            }
            2 => {
                state.apply_input(InputEvent::Resize {
                    width: 130,
                    height: 42,
                });
            }
            3 => {
                state.screen_stack.push(ShellScreen::Clock);
            }
            _ => {
                state.apply_input(InputEvent::mouse_drag(PointerButton::Left, point));
            }
        }
        state.apply_input(InputEvent::mouse_up(PointerButton::Left, point));
        assert_ne!(state.active_screen(), ShellScreen::ExitConfirm);
        assert!(state.button_pointer_capture.is_none());
    }
}

#[test]
fn button_click_allows_short_presses_but_rejects_holds_over_500_ms() {
    for kind in 0..4 {
        for millis in [0, 1, 499, 500, 501, 2_000] {
            let mut state = session();
            if kind == 1 || kind == 2 {
                state.screen_stack.push(ShellScreen::Launcher);
                if kind == 2 {
                    state.launcher_view_mode = ui::LauncherViewMode::Details;
                }
                state.refresh_hit_map();
            } else if kind == 3 {
                state.apply_input(InputEvent::key(InputKey::Escape));
            }
            let mut compositor = ScreenCompositor::default();
            let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
            let mut prepared = home_frame(&state, 0, true);
            let point = if kind == 1 || kind == 2 {
                let model = state.to_launcher_view_model();
                let ui::ShellLayout::Full { main, .. } =
                    state.shell_layout_for(Rect::new(0, 0, 120, 40))
                else {
                    panic!("full layout required");
                };
                let layout = ui::launcher_layout(main, &model);
                let editor = model
                    .items
                    .iter()
                    .position(|item| item.id == app::EDITOR_APPLICATION.id)
                    .unwrap();
                let area = layout
                    .items
                    .iter()
                    .find(|item| item.index == editor)
                    .unwrap()
                    .area;
                prepared.page = ScreenViewModel::Launcher(Box::new(model));
                draw(&mut compositor, &mut terminal, &mut state, &prepared);
                (area.x + 1, area.y)
            } else {
                prepared.notification = state.to_notification_view_model();
                draw(&mut compositor, &mut terminal, &mut state, &prepared);
                let area = state
                    .button_regions
                    .iter()
                    .find(|region| {
                        if kind == 0 {
                            region.id.as_str() == "shell.back"
                        } else {
                            region.id.as_str().starts_with("notification.")
                        }
                    })
                    .unwrap()
                    .area;
                (area.x, area.y)
            };
            let now = Instant::now();
            let before = state.active_screen();
            let modal_before = state.notification_active_modal_id();
            state.apply_input_at(InputEvent::mouse_down(PointerButton::Left, point), now);
            assert_eq!(state.active_screen(), before);
            state.apply_input_at(
                InputEvent::mouse_up(PointerButton::Left, point),
                now + Duration::from_millis(millis),
            );
            let activated = match kind {
                0 => state.active_screen() == ShellScreen::ExitConfirm,
                1 | 2 => state.active_screen() == ShellScreen::Editor,
                _ => state.notification_active_modal_id() != modal_before,
            };
            assert_eq!(activated, millis <= 500, "kind={kind}, millis={millis}");
            assert!(state.button_pointer_capture.is_none());
            if millis > 500 {
                // A late extra release must not resurrect a cancelled click.
                state.apply_input_at(
                    InputEvent::mouse_up(PointerButton::Left, point),
                    now + Duration::from_millis(millis + 1),
                );
                assert_eq!(state.active_screen(), before);
                assert!(!state.shutdown_requested());
            }
        }
    }
}

#[test]
fn page_buttons_use_pointer_hover_instead_of_keyboard_focus_and_release_opens_dialog() {
    let mut state = session();
    state.screen_stack.push(ShellScreen::Clock);
    state.restore_clock_profile(Default::default());
    state.refresh_hit_map();
    let mut compositor = ScreenCompositor::default();
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    let mut prepared = home_frame(&state, 0, true);
    prepared.page = ScreenViewModel::Clock(Box::new(
        state.to_clock_view_model_at(&state.app.snapshot().clock, Instant::now()),
    ));
    draw(&mut compositor, &mut terminal, &mut state, &prepared);
    let region = state
        .button_regions
        .iter()
        .find(|r| r.id.as_str() == "clock.new")
        .unwrap()
        .clone();
    let point = (region.area.x + region.area.width / 2, region.area.y);
    let theme = prepared.context.compatibility_theme();
    assert_eq!(terminal.backend().buffer()[point].fg, theme.accent_color);
    state.apply_input(InputEvent::mouse_moved(point));
    draw(&mut compositor, &mut terminal, &mut state, &prepared);
    assert_eq!(
        terminal.backend().buffer()[point].fg,
        theme.button_hover_color()
    );
    state.apply_input(InputEvent::mouse_down(PointerButton::Left, point));
    assert!(state.clock_create_state.is_none());
    draw(&mut compositor, &mut terminal, &mut state, &prepared);
    assert_eq!(
        terminal.backend().buffer()[point].fg,
        theme.button_pressed_color()
    );
    state.apply_input(InputEvent::mouse_up(PointerButton::Left, point));
    assert!(state.clock_create_state.is_some());
}

#[test]
fn launcher_card_single_click_waits_for_release() {
    let mut state = session();
    state.screen_stack.push(ShellScreen::Launcher);
    state.refresh_hit_map();
    let mut compositor = ScreenCompositor::default();
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    let mut prepared = home_frame(&state, 0, true);
    prepared.page = ScreenViewModel::Launcher(Box::new(state.to_launcher_view_model()));
    draw(&mut compositor, &mut terminal, &mut state, &prepared);
    let id = format!("launcher.item.{}", app::EDITOR_APPLICATION.id);
    let area = state
        .button_regions
        .iter()
        .find(|r| r.id.as_str() == id)
        .unwrap()
        .area;
    let point = (area.x + 1, area.y + 1);
    let now = Instant::now();
    state.apply_input_at(InputEvent::mouse_down(PointerButton::Left, point), now);
    assert_eq!(state.active_screen(), ShellScreen::Launcher);
    state.apply_input_at(
        InputEvent::mouse_up(PointerButton::Left, point),
        now + Duration::from_millis(30),
    );
    assert_eq!(state.active_screen(), ShellScreen::Editor);
}

#[test]
fn launcher_click_launches_only_after_matching_release_in_both_views() {
    for view in [
        ui::LauncherViewMode::LargeIcons,
        ui::LauncherViewMode::Details,
    ] {
        for cancel in 0..5 {
            let mut state = session();
            state.screen_stack.push(ShellScreen::Launcher);
            state.launcher_view_mode = view;
            state.refresh_hit_map();
            let mut compositor = ScreenCompositor::default();
            let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
            let mut prepared = home_frame(&state, 0, true);
            let model = state.to_launcher_view_model();
            let ui::ShellLayout::Full { main, .. } =
                state.shell_layout_for(Rect::new(0, 0, 120, 40))
            else {
                panic!("full layout required");
            };
            let layout = ui::launcher_layout(main, &model);
            let editor = model
                .items
                .iter()
                .position(|item| item.id == app::EDITOR_APPLICATION.id)
                .unwrap();
            let area = layout
                .items
                .iter()
                .find(|item| item.index == editor)
                .unwrap()
                .area;
            let point = (area.x + 1, area.y);
            prepared.page = ScreenViewModel::Launcher(Box::new(model));
            draw(&mut compositor, &mut terminal, &mut state, &prepared);
            state.apply_input(InputEvent::mouse_down(PointerButton::Left, point));
            assert_eq!(state.active_screen(), ShellScreen::Launcher);
            match cancel {
                1 => {
                    state.apply_input(InputEvent::mouse_up(PointerButton::Left, (0, 0)));
                }
                2 => {
                    state.apply_input(InputEvent::FocusLost);
                }
                3 => {
                    state.apply_input(InputEvent::mouse_drag(PointerButton::Left, point));
                }
                4 => {
                    state.apply_input(InputEvent::Resize {
                        width: 130,
                        height: 42,
                    });
                }
                _ => {}
            }
            state.apply_input(InputEvent::mouse_up(PointerButton::Left, point));
            assert_eq!(
                state.active_screen(),
                if cancel == 0 {
                    ShellScreen::Editor
                } else {
                    ShellScreen::Launcher
                },
                "view={view:?}, cancel={cancel}"
            );
        }
    }
}

#[test]
fn home_entry_single_click_opens_on_first_release() {
    struct StorageFixtureGuard(PathBuf);
    impl Drop for StorageFixtureGuard {
        fn drop(&mut self) {
            let _ = platform::cleanup_temp_path(&self.0);
        }
    }

    let root = platform::create_temp_dir(
        &std::env::temp_dir().join("TundraUX3-shell-tests"),
        "home-launcher-click",
    )
    .expect("create private Launcher storage fixture");
    let _storage_guard = StorageFixtureGuard(root.clone());
    let paths = platform::build_linux_app_paths(
        root.join("Config"),
        root.join("Data"),
        root.join("Cache"),
        root.join("State"),
        root.join("Temp"),
    )
    .unwrap();
    let manager = StorageManager::open(paths.clone()).unwrap().manager;
    let user_dirs = platform::UserDirs::new(
        root.join("Desktop"),
        root.join("Documents"),
        root.join("Downloads"),
        root.join("Pictures"),
        root.join("Videos"),
        root.join("Music"),
        root.join("UserData"),
    )
    .unwrap();
    let platform = platform::mock::MockPlatform::new(user_dirs, paths);
    let launcher_session = || {
        let mut state = session();
        state.storage_manager = Some(manager.clone());
        state.app.dispatch_at(
            app::AppCommand::SetAuthSession(Some(AuthSession {
                source: identity::IdentitySource::LocalAccount,
                session_id: "click-session".into(),
                user_id: "click-user".into(),
                username: "click-user".into(),
                role: UserRole::User,
                started_at_epoch_ms: 1,
            })),
            Instant::now(),
        );
        state.refresh_hit_map();
        state
    };
    // Check the storage and authorization prerequisites through the real
    // keyboard path before testing pointer activation on an independent session.
    let mut keyboard_state = launcher_session();
    let launcher_index = keyboard_state
        .user_home_entries()
        .iter()
        .position(|entry| entry.icon_identity() == "launcher")
        .unwrap();
    keyboard_state.select_home_entry(launcher_index);
    keyboard_state.apply_input_with_platform(InputEvent::key(InputKey::Enter), &platform);
    assert_eq!(keyboard_state.active_screen(), ShellScreen::Launcher);
    assert!(keyboard_state.error_message.is_none());

    let mut state = launcher_session();
    let mut compositor = ScreenCompositor::default();
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    let prepared = home_frame(&state, 0, true);
    draw(&mut compositor, &mut terminal, &mut state, &prepared);
    let launcher_id = format!("home.entry.{launcher_index}");
    let area = state
        .button_regions
        .iter()
        .find(|r| r.id.as_str() == launcher_id)
        .unwrap()
        .area;
    let point = (area.x + 1, area.y + 1);
    assert_eq!(state.home_entry_index_at(point), Some(launcher_index));
    let pressed_at = Instant::now();
    state.apply_input_with_platform_at(
        InputEvent::mouse_down(PointerButton::Left, point),
        &platform,
        pressed_at,
    );
    assert_eq!(state.active_screen(), ShellScreen::Home);
    assert_eq!(
        state
            .button_pointer_capture
            .as_ref()
            .map(|capture| capture.region.id.as_str()),
        Some(launcher_id.as_str()),
    );
    state.apply_input_with_platform_at(
        InputEvent::mouse_up(PointerButton::Left, point),
        &platform,
        pressed_at + Duration::from_millis(10),
    );
    assert_eq!(state.active_screen(), ShellScreen::Launcher);
    assert!(state.error_message.is_none());
}

#[test]
fn exit_buttons_activate_on_first_release_across_ticks_and_redraws() {
    let root = std::env::temp_dir().join("tundra-exit-click-test");
    let user_dirs = platform::UserDirs::new(
        root.join("Desktop"),
        root.join("Documents"),
        root.join("Downloads"),
        root.join("Pictures"),
        root.join("Videos"),
        root.join("Music"),
        root.join("AppData"),
    )
    .unwrap();
    let paths = platform::build_windows_app_paths(
        root.join("Roaming"),
        root.join("Local"),
        root.join("Temp"),
    )
    .unwrap();
    let platform = platform::mock::MockPlatform::new(user_dirs, paths);
    for (id, expected, restart) in [
        ("restore-terminal", ShellAction::Exit, false),
        ("restart", ShellAction::Exit, true),
        ("reboot", ShellAction::Reboot, false),
        ("poweroff", ShellAction::PowerOff, false),
        ("cancel", ShellAction::Redraw, false),
    ] {
        let mut state = session();
        state.apply_input_with_platform(InputEvent::key(InputKey::Escape), &platform);
        let mut compositor = ScreenCompositor::default();
        let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
        let mut prepared = home_frame(&state, 0, true);
        prepared.notification = state.to_notification_view_model();
        draw(&mut compositor, &mut terminal, &mut state, &prepared);
        let area = state
            .button_regions
            .iter()
            .find(|region| region.id.as_str().ends_with(&format!(".action.{id}")))
            .unwrap()
            .area;
        let point = (area.x, area.y);
        let pressed_at = Instant::now();
        state.apply_input_with_platform_at(
            InputEvent::mouse_down(PointerButton::Left, point),
            &platform,
            pressed_at,
        );
        assert!(!state.shutdown_requested());
        for millis in [50, 100] {
            state.apply_input_with_platform_at(
                InputEvent::Tick,
                &platform,
                pressed_at + Duration::from_millis(millis),
            );
            prepared.notification = state.to_notification_view_model();
            draw(&mut compositor, &mut terminal, &mut state, &prepared);
        }
        assert_eq!(
            state.apply_input_with_platform_at(
                InputEvent::mouse_up(PointerButton::Left, point),
                &platform,
                pressed_at + Duration::from_millis(150),
            ),
            expected,
            "first click on {id}",
        );
        assert_eq!(state.restart_requested, restart, "{id}");
        assert!(!state.notification_has_active_modal(), "{id}");
    }
}

#[test]
fn notification_button_release_after_focus_loss_does_not_activate() {
    let mut state = session();
    state.apply_input(InputEvent::key(InputKey::Escape));
    assert_eq!(state.active_screen(), ShellScreen::ExitConfirm);
    let mut compositor = ScreenCompositor::default();
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    let mut prepared = home_frame(&state, 0, true);
    prepared.notification = state.to_notification_view_model();
    draw(&mut compositor, &mut terminal, &mut state, &prepared);
    let area = state
        .button_regions
        .iter()
        .find(|region| region.id.as_str().starts_with("notification."))
        .unwrap()
        .area;
    let point = (area.x, area.y);
    state.apply_input(InputEvent::mouse_down(PointerButton::Left, point));
    assert!(state.notification_pointer_capture.is_some());
    state.apply_input(InputEvent::FocusLost);
    state.apply_input(InputEvent::mouse_up(PointerButton::Left, point));
    assert!(!state.shutdown_requested());
    assert_eq!(state.active_screen(), ShellScreen::ExitConfirm);
}

#[test]
fn aa_keyboard_focus_hover_and_press_render_the_requested_accent_colors() {
    let mut state = session();
    let (tx, _rx) = mpsc::channel();
    state
        .begin_auto_admin("Enable TestUser".into(), true, tx)
        .unwrap();
    let mut compositor = ScreenCompositor::default();
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    let prepared = home_frame(&state, 0, true);
    let theme = prepared.context.compatibility_theme();
    draw(&mut compositor, &mut terminal, &mut state, &prepared);
    let button = |state: &ShellSession, id: &str| {
        let area = state
            .button_regions
            .iter()
            .find(|r| r.id.as_str() == id)
            .unwrap()
            .area;
        (area.x + area.width / 2, area.y)
    };
    let approve = button(&state, "aa.approve");
    let deny = button(&state, "aa.deny");
    assert_eq!(terminal.backend().buffer()[approve].fg, theme.accent_color);
    assert_eq!(terminal.backend().buffer()[deny].fg, theme.foreground);
    state.apply_input(InputEvent::key(InputKey::Right));
    draw(&mut compositor, &mut terminal, &mut state, &prepared);
    assert_eq!(terminal.backend().buffer()[approve].fg, theme.foreground);
    assert_eq!(terminal.backend().buffer()[deny].fg, theme.accent_color);

    state.apply_input(InputEvent::mouse_moved(approve));
    draw(&mut compositor, &mut terminal, &mut state, &prepared);
    assert_eq!(terminal.backend().buffer()[approve].fg, theme.accent_color);
    assert_eq!(terminal.backend().buffer()[deny].fg, theme.foreground);
    state.apply_input(InputEvent::mouse_down(PointerButton::Left, approve));
    draw(&mut compositor, &mut terminal, &mut state, &prepared);
    assert_eq!(
        terminal.backend().buffer()[approve].fg,
        theme.button_pressed_color()
    );
    assert!(state.auto_admin_view().unwrap().confirming);

    // Keyboard navigation cancels the press and takes over from the stationary mouse.
    state.apply_input(InputEvent::key(InputKey::Tab));
    state.apply_input(InputEvent::mouse_moved(approve));
    draw(&mut compositor, &mut terminal, &mut state, &prepared);
    assert!(state.auto_admin_pressed_button().is_none());
    assert_eq!(terminal.backend().buffer()[approve].fg, theme.foreground);
    assert_eq!(terminal.backend().buffer()[deny].fg, theme.accent_color);
    let background = state
        .button_regions
        .iter()
        .find(|r| r.id.as_str() == "shell.back")
        .unwrap()
        .area;
    assert!(state.button_at((background.x, background.y)).is_none());
    state.apply_input(InputEvent::FocusLost);
    draw(&mut compositor, &mut terminal, &mut state, &prepared);
    assert_eq!(terminal.backend().buffer()[deny].fg, theme.foreground);

    // Reopening from a keyboard shortcut must also restore visible focus.
    state.close_auto_admin();
    state.apply_input(InputEvent::key(InputKey::F(12)));
    draw(&mut compositor, &mut terminal, &mut state, &prepared);
    let close = button(&state, "aa.close");
    assert_eq!(terminal.backend().buffer()[close].fg, theme.accent_color);
}

#[test]
fn aa_button_release_clears_emphasis_until_the_pointer_moves_again() {
    let mut state = session();
    let (tx, rx) = mpsc::channel();
    state
        .begin_auto_admin("Read fixture output".into(), false, tx)
        .unwrap();
    let mut compositor = ScreenCompositor::default();
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    let prepared = home_frame(&state, 0, true);
    let theme = prepared.context.compatibility_theme();
    draw(&mut compositor, &mut terminal, &mut state, &prepared);
    let area = state
        .button_regions
        .iter()
        .find(|r| r.id.as_str() == "aa.enter")
        .unwrap()
        .area;
    let point = (area.x + area.width / 2, area.y);
    let now = Instant::now();
    state.apply_input_at(InputEvent::mouse_moved(point), now);
    state.apply_input_at(InputEvent::mouse_down(PointerButton::Left, point), now);
    draw(&mut compositor, &mut terminal, &mut state, &prepared);
    assert_eq!(
        terminal.backend().buffer()[point].fg,
        theme.button_pressed_color()
    );
    state.apply_input_at(
        InputEvent::mouse_up(PointerButton::Left, point),
        now + Duration::from_millis(50),
    );
    draw(&mut compositor, &mut terminal, &mut state, &prepared);
    assert_eq!(terminal.backend().buffer()[point].fg, theme.foreground);
    assert!(rx.try_iter().any(|input| matches!(input, platform::management::OperationInput::Terminal { bytes } if bytes == b"\r")));
    state.apply_input(InputEvent::mouse_moved(point));
    draw(&mut compositor, &mut terminal, &mut state, &prepared);
    assert_eq!(terminal.backend().buffer()[point].fg, theme.foreground);
    state.apply_input(InputEvent::mouse_moved((point.0 + 1, point.1)));
    draw(&mut compositor, &mut terminal, &mut state, &prepared);
    assert_eq!(terminal.backend().buffer()[point].fg, theme.accent_color);
    state.apply_input(InputEvent::key(InputKey::F(6)));
    draw(&mut compositor, &mut terminal, &mut state, &prepared);
    assert_eq!(terminal.backend().buffer()[point].fg, theme.foreground);
    let first = state
        .button_regions
        .iter()
        .find(|r| r.id.as_str() == "aa.y")
        .unwrap()
        .area;
    assert_eq!(
        terminal.backend().buffer()[(first.x, first.y)].fg,
        theme.accent_color
    );
}
