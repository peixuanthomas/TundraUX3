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
