use super::*;

fn session(now: Instant, seconds: u64) -> ShellSession {
    let mut state = ShellSession::new_for_home_mode(
        ShellLaunchConfig::default(),
        (120, 40),
        ShellHomeMode::User,
    );
    while state.notification_dismiss_active_modal_without_response() {}
    let mut config = state.app.storage_config().clone();
    config.screen_timeout_seconds = seconds;
    state.app.dispatch_at(
        app::AppCommand::SetStorageConfig {
            config,
            synchronized_utc: None,
        },
        now,
    );
    state.screen_saver.last_activity = now;
    state
}

#[test]
fn every_page_enters_weather_at_the_configured_boundary_and_keeps_its_page() {
    let now = Instant::now();
    for screen in [
        ShellScreen::Home,
        ShellScreen::FirstRunSetup,
        ShellScreen::Login,
        ShellScreen::BootstrapAdmin,
        ShellScreen::UserManagement,
        ShellScreen::Explorer,
        ShellScreen::Launcher,
        ShellScreen::CommandLine,
        ShellScreen::Editor,
        ShellScreen::Settings,
        ShellScreen::Logs,
        ShellScreen::Management,
        ShellScreen::Diagnostics,
        ShellScreen::SystemStatus,
        ShellScreen::Clock,
        ShellScreen::ExitConfirm,
    ] {
        let mut state = session(now, 30);
        state.set_navigation_path(vec![screen]);
        let page = state.active_page_id();
        let focus = state.focused_component();
        let auth = state.auth_session().cloned();
        let deadline = now + Duration::from_secs(30);
        assert_eq!(
            state.screen_saver_poll_timeout(now, Duration::MAX),
            Duration::from_secs(30)
        );
        state.apply_input_at(InputEvent::Tick, deadline - Duration::from_nanos(1));
        assert!(!state.screen_saver_active(), "{screen:?}");
        state.apply_input_at(InputEvent::Tick, deadline);
        assert!(state.screen_saver_active(), "{screen:?}");
        assert!(!state.return_to_lockscreen_requested());
        state.apply_input_at(InputEvent::from_key_label("Enter"), deadline);
        assert!(!state.screen_saver_active());
        assert_eq!(state.active_page_id(), page);
        assert_eq!(state.active_screen(), screen);
        assert_eq!(state.focused_component(), focus);
        assert_eq!(state.auth_session(), auth.as_ref());
        assert!(!state.shutdown_requested());
    }
}

#[test]
fn never_disables_weather_and_only_actual_activity_restarts_the_timer() {
    let now = Instant::now();
    let mut disabled = session(now, 0);
    disabled.apply_input_at(InputEvent::Tick, now + Duration::from_secs(86_400));
    assert!(!disabled.screen_saver_active());
    assert_eq!(
        disabled.screen_saver_poll_timeout(now, Duration::MAX),
        Duration::MAX
    );

    for input in [
        InputEvent::from_key_label("a"),
        InputEvent::Paste("text".into()),
        InputEvent::mouse_moved((4, 5)),
        InputEvent::mouse_down(PointerButton::Left, (0, 0)),
        InputEvent::Resize {
            width: 121,
            height: 40,
        },
        InputEvent::FocusGained,
    ] {
        let mut state = session(now, 30);
        let at = now + Duration::from_secs(29);
        state.prepare_input(input, &platform::mock::UnsupportedPlatform, at);
        assert_eq!(state.screen_saver.last_activity, at);
        state.advance_screen_saver_at(now + Duration::from_secs(30));
        assert!(!state.screen_saver_active());
    }
    let mut state = session(now, 30);
    state.interaction.pointer_position = Some((4, 5));
    for input in [
        InputEvent::mouse_moved((4, 5)),
        InputEvent::FocusLost,
        InputEvent::Tick,
    ] {
        state.consume_screen_saver_input(&input, now + Duration::from_secs(29));
        assert_eq!(state.screen_saver.last_activity, now);
    }
}

#[test]
fn modal_and_child_terminal_input_restart_the_timer_before_they_consume_input() {
    let now = Instant::now();
    let platform = platform::mock::UnsupportedPlatform;
    let mut state = session(now, 30);
    let (tx, _rx) = mpsc::channel();
    state.begin_auto_admin("test".into(), false, tx).unwrap();
    let at = now + Duration::from_secs(29);
    assert!(
        state
            .prepare_input(InputEvent::from_key_label("a"), &platform, at)
            .is_none()
    );
    assert_eq!(state.screen_saver.last_activity, at);
    state.advance_screen_saver_at(now + Duration::from_secs(30));
    assert!(!state.screen_saver_active());

    let mut state = session(now, 30);
    state.set_navigation_path(vec![ShellScreen::CommandLine]);
    let input = InputEvent::Paste("echo text".into());
    assert!(state.prepare_input(input, &platform, at).is_some());
    assert_eq!(state.screen_saver.last_activity, at);
}

#[test]
fn waking_consumes_the_whole_key_or_pointer_gesture_and_ticks_keep_running() {
    let now = Instant::now();
    let platform = platform::mock::UnsupportedPlatform;
    let mut state = session(now, 30);
    state.advance_screen_saver_at(now + Duration::from_secs(30));
    let tick = state.tick_count;
    state.apply_input_at(InputEvent::Tick, now + Duration::from_secs(31));
    assert!(state.tick_count > tick);
    assert!(state.screen_saver_active());
    state.apply_input_at(
        InputEvent::mouse_moved((2, 3)),
        now + Duration::from_secs(31),
    );
    assert!(state.screen_saver_active());
    for phase in [InputPhase::Press, InputPhase::Repeat, InputPhase::Release] {
        assert!(
            state
                .prepare_input(
                    InputEvent::Key(KeyInput::with_phase(
                        InputKey::Enter,
                        InputModifiers::none(),
                        phase,
                    )),
                    &platform,
                    now + Duration::from_secs(32)
                )
                .is_none()
        );
        assert!(!state.screen_saver_active());
    }
    assert!(
        state
            .prepare_input(
                InputEvent::from_key_label("a"),
                &platform,
                now + Duration::from_secs(33)
            )
            .is_some()
    );
    state.advance_screen_saver_at(now + Duration::from_secs(63));
    for input in [
        InputEvent::mouse_down(PointerButton::Left, (0, 0)),
        InputEvent::mouse_moved((1, 1)),
        InputEvent::mouse_up(PointerButton::Left, (1, 1)),
    ] {
        assert!(
            state
                .prepare_input(input, &platform, now + Duration::from_secs(64))
                .is_none()
        );
    }
    assert!(!state.screen_saver_active());
    assert!(!state.interaction.keyboard_focus_visible());
    assert!(
        state
            .prepare_input(
                InputEvent::from_key_label("a"),
                &platform,
                now + Duration::from_secs(65)
            )
            .is_some()
    );
}
