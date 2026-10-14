use super::*;

fn session() -> ShellSession {
    let mut state = ShellSession::new_for_home_mode(
        ShellLaunchConfig::default(),
        (120, 40),
        ShellHomeMode::User,
    );
    while state.notification_dismiss_active_modal_without_response() {}
    state.enter_screen(ShellScreen::CommandLine);
    state
}

#[test]
fn keyboard_animation_reverses_without_jumping_and_honors_speed_and_reduced_motion() {
    let frame = |millis, speed| ui::MotionFrame {
        now: Duration::from_millis(millis),
        animation_speed_percent: speed,
        ..Default::default()
    };
    for speed in [50, 100, 200] {
        let mut state = session();
        let original = state.command_line_input_area();
        state.toggle_input_keyboard();
        assert!(state.advance_input_keyboard(frame(0, speed)));
        assert_eq!(state.screen_keyboard.visibility, 0);
        let halfway = 110 * 100 / u64::from(speed);
        assert!(state.advance_input_keyboard(frame(halfway, speed)));
        let opening = state.screen_keyboard.visibility;
        assert!(opening > 0 && opening < 1_000);
        let area = state.input_keyboard_layout().buttons[0].region();
        state.toggle_input_keyboard();
        state.advance_input_keyboard(frame(halfway, speed));
        assert_eq!(state.screen_keyboard.visibility, opening);
        state.screen_keyboard.pressed = Some((area, Instant::now()));
        assert!(state.advance_input_keyboard(frame(halfway * 2, speed)));
        let closing = state.screen_keyboard.visibility;
        assert!(closing > 0 && closing < opening);
        assert!(state.screen_keyboard.pressed.is_none());
        state.toggle_input_keyboard();
        state.advance_input_keyboard(frame(halfway * 2, speed));
        assert_eq!(state.screen_keyboard.visibility, closing);
        assert!(!state.advance_input_keyboard(frame(halfway * 4, speed)));
        assert_eq!(state.screen_keyboard.visibility, 1_000);
        state.toggle_input_keyboard();
        assert!(!state.advance_input_keyboard(ui::MotionFrame::reduced(Duration::from_secs(2))));
        assert_eq!(state.screen_keyboard.visibility, 0);
        assert_eq!(state.command_line_input_area(), original);
        assert!(!state.advance_input_keyboard(frame(2_001, speed)));
    }
}

fn route(state: &mut ShellSession, input: InputEvent) -> Option<InputEvent> {
    state.route_screen_keyboard_input(input, platform::native_platform().as_ref(), Instant::now())
}

#[test]
fn rapid_keyboard_clicks_keep_small_movements_inside_each_key() {
    for auto_admin in [false, true] {
        for hover in [false, true] {
            let mut state = session();
            let (responses, inputs) = std::sync::mpsc::channel();
            if auto_admin {
                state
                    .begin_auto_admin("Keyboard input".into(), false, responses)
                    .unwrap();
            }
            state.toggle_input_keyboard();
            state.advance_input_keyboard(ui::MotionFrame::reduced(Duration::ZERO));
            let platform = platform::native_platform();
            let start = Instant::now();
            let mut keys = Vec::new();
            // No redraw or delay between clicks; touch also works without hover.
            for (index, letter) in ['a', 's', 's', 'd'].into_iter().enumerate() {
                let area = state
                    .input_keyboard_layout()
                    .buttons
                    .into_iter()
                    .find(|button| button.action == Action::Letter(letter))
                    .unwrap()
                    .area;
                assert!(area.width > 1);
                let point = (area.x, area.y);
                let moved = (area.x + 1, area.y);
                let at = start + Duration::from_millis(index as u64 * 30);
                let mut events = Vec::new();
                if hover {
                    events.push(InputEvent::mouse_moved(point));
                }
                events.push(InputEvent::mouse_down(PointerButton::Left, point));
                if index > 0 {
                    events.push(InputEvent::mouse_drag(PointerButton::Left, point));
                    events.push(InputEvent::mouse_drag(PointerButton::Left, moved));
                }
                events.push(InputEvent::mouse_up(PointerButton::Left, moved));
                for event in events {
                    if let Some(InputEvent::Key(key)) =
                        state.prepare_input(event, platform.as_ref(), at)
                    {
                        keys.push(key);
                    }
                }
                assert!(state.screen_keyboard.pressed.is_none());
                assert!(state.interaction.button_capture.is_none());
            }
            if auto_admin {
                let bytes: Vec<_> = inputs
                    .try_iter()
                    .flat_map(|input| match input {
                        platform::management::OperationInput::Terminal { bytes } => bytes,
                        _ => Vec::new(),
                    })
                    .collect();
                assert_eq!(bytes, b"assd", "hover={hover}");
                assert!(keys.is_empty());
            } else {
                assert_eq!(
                    keys,
                    ['a', 's', 's', 'd'].map(|letter| KeyInput::new(InputKey::Char(letter))),
                    "hover={hover}"
                );
            }
        }
    }
}

fn click(state: &mut ShellSession, action: Action) -> Option<InputEvent> {
    click_with_platform(state, action, platform::native_platform().as_ref())
}

fn click_with_platform(
    state: &mut ShellSession,
    action: Action,
    platform: &dyn platform::Platform,
) -> Option<InputEvent> {
    let layout = state.input_keyboard_layout();
    let area = layout
        .buttons
        .iter()
        .find(|button| button.action == action)
        .unwrap()
        .area;
    assert!(
        state
            .route_screen_keyboard_input(
                InputEvent::mouse_down(PointerButton::Left, (area.x, area.y)),
                platform,
                Instant::now(),
            )
            .is_none()
    );
    state.route_screen_keyboard_input(
        InputEvent::mouse_up(PointerButton::Left, (area.x, area.y)),
        platform,
        Instant::now(),
    )
}

#[test]
fn keyboard_paste_handles_empty_unavailable_and_nonempty_clipboards() {
    let root = std::env::temp_dir().join("tundra-keyboard-paste-test");
    let dirs = platform::UserDirs::new(
        root.join("Desktop"),
        root.join("Documents"),
        root.join("Downloads"),
        root.join("Pictures"),
        root.join("Videos"),
        root.join("Music"),
        root.join("Data"),
    )
    .unwrap();
    let platform = platform::mock::MockPlatform::new(
        dirs,
        platform::build_binary_dir_app_paths(root).unwrap(),
    );
    let mut state = session();
    state.toggle_input_keyboard();
    state.advance_input_keyboard(ui::MotionFrame::reduced(Duration::ZERO));
    for _ in 0..3 {
        assert_eq!(
            click_with_platform(&mut state, Action::Paste, &platform),
            None
        );
        assert!(state.status().contains("No text to paste"));
        assert!(state.input_keyboard_open());
    }
    assert_eq!(
        click_with_platform(
            &mut state,
            Action::Paste,
            &platform::mock::UnsupportedPlatform
        ),
        None,
    );
    assert!(state.status().contains("Clipboard unavailable"));
    assert!(state.input_keyboard_open());
    platform.set_clipboard_text("中文\nnormal text");
    assert_eq!(
        click_with_platform(&mut state, Action::Paste, &platform),
        Some(InputEvent::Paste("中文\nnormal text".into())),
    );
    assert!(matches!(
        click(&mut state, Action::Letter('a')),
        Some(InputEvent::Key(_))
    ));
}

#[test]
fn command_line_launcher_resizes_content_and_generates_real_modified_keys() {
    let mut state = session();
    let physical = InputEvent::Key(KeyInput::with_modifiers(
        InputKey::Char('K'),
        InputModifiers::CTRL_SHIFT,
    ));
    assert_eq!(route(&mut state, physical.clone()), Some(physical));
    assert!(!state.input_keyboard_open());
    let original = state.command_line_input_area().unwrap();
    let area = state.keyboard_launcher_area().unwrap();
    route(
        &mut state,
        InputEvent::mouse_down(PointerButton::Left, (area.x, area.y)),
    );
    assert!(!state.input_keyboard_open());
    route(
        &mut state,
        InputEvent::mouse_up(PointerButton::Left, (area.x, area.y)),
    );
    state.advance_input_keyboard(ui::MotionFrame::reduced(Duration::ZERO));
    let smaller = state.command_line_input_area().unwrap();
    assert!(smaller.height < original.height);
    assert!(smaller.bottom() <= state.input_keyboard_layout().keyboard.y);
    click(&mut state, Action::Shift);
    assert_eq!(
        click(&mut state, Action::Letter('a')),
        Some(InputEvent::Key(KeyInput::with_modifiers(
            InputKey::Char('A'),
            ui::KeyModifiers::SHIFT
        )))
    );
    assert_eq!(
        click(&mut state, Action::Tab),
        Some(InputEvent::Key(KeyInput::with_modifiers(
            InputKey::BackTab,
            ui::KeyModifiers::SHIFT
        )))
    );
    click(&mut state, Action::Shift);
    click(&mut state, Action::LeftCtrl);
    assert_eq!(
        click(&mut state, Action::Letter('c')),
        Some(InputEvent::Key(KeyInput::with_modifiers(
            InputKey::Char('c'),
            ui::KeyModifiers::CTRL
        )))
    );
    click(&mut state, Action::ToggleKeyboard);
    state.advance_input_keyboard(ui::MotionFrame::reduced(Duration::ZERO));
    assert!(!state.input_keyboard_open());
    assert_eq!(state.command_line_input_area().unwrap(), original);
    assert_eq!(
        state.screen_keyboard.modifiers,
        ScreenKeyboardModifiers::default()
    );
}

#[test]
fn keyboard_cancels_clicks_on_drag_resize_focus_loss_timeout_and_page_change() {
    for cancel in 0..9 {
        let mut state = session();
        state.toggle_input_keyboard();
        state.advance_input_keyboard(ui::MotionFrame::reduced(std::time::Duration::ZERO));
        let layout = state.input_keyboard_layout();
        let area = layout
            .buttons
            .iter()
            .find(|button| button.action == Action::Letter('a'))
            .unwrap()
            .area;
        route(
            &mut state,
            InputEvent::mouse_down(PointerButton::Left, (area.x, area.y)),
        );
        match cancel {
            0 => {
                route(&mut state, InputEvent::mouse_moved((0, 0)));
            }
            1 => {
                route(
                    &mut state,
                    InputEvent::Resize {
                        width: 100,
                        height: 30,
                    },
                );
            }
            2 => {
                route(&mut state, InputEvent::FocusLost);
            }
            3 => {
                state.poll_input_keyboard(Instant::now() + Duration::from_secs(1));
            }
            4 => {
                state.enter_screen(ShellScreen::Launcher);
                state.sync_input_keyboard();
            }
            5 => {
                route(
                    &mut state,
                    InputEvent::mouse_drag(PointerButton::Left, (0, 0)),
                );
                route(
                    &mut state,
                    InputEvent::mouse_drag(PointerButton::Left, (area.x, area.y)),
                );
            }
            6 => {
                route(
                    &mut state,
                    InputEvent::mouse_scroll(ScrollDirection::Down, (area.x, area.y)),
                );
            }
            7 => {
                route(
                    &mut state,
                    InputEvent::mouse_down(PointerButton::Right, (area.x, area.y)),
                );
            }
            _ => {
                route(
                    &mut state,
                    InputEvent::mouse_drag(PointerButton::Right, (area.x, area.y)),
                );
            }
        }
        let input = route(
            &mut state,
            InputEvent::mouse_up(PointerButton::Left, (area.x, area.y)),
        );
        assert!(!matches!(input, Some(InputEvent::Key(_))));
        assert!(state.screen_keyboard.pressed.is_none());
    }
}

#[test]
fn keyboard_closes_for_an_overlay_and_never_reappears_for_a_different_page() {
    let mut state = session();
    state.toggle_input_keyboard();
    state.advance_input_keyboard(ui::MotionFrame::reduced(std::time::Duration::ZERO));
    state.show_command_line_close_confirmation(state.active_page_id());
    state.sync_input_keyboard();
    assert!(!state.input_keyboard_open());
    while state.notification_dismiss_active_modal_without_response() {}
    state.sync_input_keyboard();
    assert!(!state.input_keyboard_open());
    state.toggle_input_keyboard();
    state.advance_input_keyboard(ui::MotionFrame::reduced(std::time::Duration::ZERO));
    state.enter_new_page(ShellScreen::CommandLine);
    state.sync_input_keyboard();
    assert!(!state.input_keyboard_open());
}
