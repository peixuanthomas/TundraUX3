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
    assert_eq!(terminal.backend().buffer()[point].fg, theme.accent_color);
    state.apply_input(InputEvent::mouse_up(PointerButton::Left, point));
    assert_eq!(state.active_screen(), ShellScreen::ExitConfirm);
    assert!(state.button_pointer_capture.is_none());
}

#[test]
fn button_capture_cancels_outside_on_focus_loss_resize_and_page_change() {
    for cancel in 0..4 {
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
            _ => {
                state.screen_stack.push(ShellScreen::Clock);
            }
        }
        state.apply_input(InputEvent::mouse_up(PointerButton::Left, point));
        assert_ne!(state.active_screen(), ShellScreen::ExitConfirm);
        assert!(state.button_pointer_capture.is_none());
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
    assert_eq!(terminal.backend().buffer()[point].fg, theme.accent_color);
    state.apply_input(InputEvent::mouse_up(PointerButton::Left, point));
    assert!(state.clock_create_state.is_some());
}

#[test]
fn launcher_card_double_click_waits_for_second_release() {
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
    state.apply_input_at(
        InputEvent::mouse_up(PointerButton::Left, point),
        now + Duration::from_millis(30),
    );
    state.apply_input_at(
        InputEvent::mouse_down(PointerButton::Left, point),
        now + Duration::from_millis(80),
    );
    assert_eq!(state.active_screen(), ShellScreen::Launcher);
    state.apply_input_at(
        InputEvent::mouse_up(PointerButton::Left, point),
        now + Duration::from_millis(110),
    );
    assert_eq!(state.active_screen(), ShellScreen::Editor);
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
