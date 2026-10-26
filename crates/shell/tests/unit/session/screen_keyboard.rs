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
fn native_forms_accept_virtual_keys_spaces_and_safe_multilingual_paste() {
    for screen in [
        ShellScreen::Login,
        ShellScreen::BootstrapAdmin,
        ShellScreen::FirstRunSetup,
    ] {
        let mut state = session();
        state.enter_screen(screen);
        if screen == ShellScreen::FirstRunSetup {
            state.pages.setup.setup_step = ui::SetupStep::Admin;
        }
        state.toggle_input_keyboard();
        state.advance_input_keyboard(ui::MotionFrame::reduced(Duration::ZERO));
        assert!(state.input_keyboard_open());
        for action in [Action::Letter('a'), Action::Space] {
            let event = click(&mut state, action).unwrap();
            state.apply_input(event);
        }
        state.apply_input(InputEvent::Paste("中文\n文档".into()));
        let text = match screen {
            ShellScreen::Login => &state.pages.login.login_password,
            ShellScreen::BootstrapAdmin => &state.pages.bootstrap.bootstrap_username,
            _ => &state.pages.setup.setup_admin_username,
        };
        assert!(text.ends_with("a 中文文档"), "{screen:?}: {text:?}");
        assert_eq!(state.active_screen(), screen);
    }
}

#[test]
fn explorer_and_editor_keyboard_follow_page_and_field_changes() {
    let mut state = session();
    state.enter_screen(ShellScreen::Explorer);
    state.toggle_input_keyboard();
    state.advance_input_keyboard(ui::MotionFrame::reduced(Duration::ZERO));
    assert_eq!(
        state.pages.explorer.explorer_input_mode,
        ExplorerInputMode::Search
    );
    assert!(state.input_keyboard_open());
    state.pages.explorer.explorer_input_mode = ExplorerInputMode::Rename;
    state.sync_input_keyboard();
    assert!(!state.input_keyboard_open());
    state.enter_screen(ShellScreen::Editor);
    assert!(state.keyboard_launcher_area().is_some());
    state.toggle_input_keyboard();
    state.advance_input_keyboard(ui::MotionFrame::reduced(Duration::ZERO));
    let region = state.input_keyboard_layout().buttons[0].region();
    state.screen_keyboard.pressed = Some((region, Instant::now()));
    state.pages.editor.editor_search_bar = Some(ui::EditorSearchBar::default());
    state.sync_input_keyboard();
    assert!(state.screen_keyboard.pressed.is_none());
    assert!(state.input_keyboard_open());
    state.help_open = true;
    state.sync_input_keyboard();
    assert!(state.keyboard_launcher_area().is_none());
}

#[test]
fn account_and_clock_dialogs_focus_text_when_opening_the_keyboard() {
    let mut state = session();
    state.enter_screen(ShellScreen::UserManagement);
    state.pages.user_management.user_management_mode =
        UserManagementMode::Create(UserManagementCreateForm {
            username: String::new(),
            display_name: String::new(),
            password: String::new(),
            role: UserRole::User,
            focused_field: UserManagementFormField::Submit,
        });
    state.toggle_input_keyboard();
    state.advance_input_keyboard(ui::MotionFrame::reduced(Duration::ZERO));
    let event = click(&mut state, Action::Letter('a')).unwrap();
    state.apply_input(event);
    let UserManagementMode::Create(form) = &state.pages.user_management.user_management_mode else {
        panic!("form was submitted");
    };
    assert_eq!(form.username, "a");
    state.enter_screen(ShellScreen::Clock);
    state.pages.clock.clock_create_state = Some(ClockCreateState {
        focus: ui::ClockCreateDialogFocus::Cancel,
        ..Default::default()
    });
    state.sync_input_keyboard();
    state.toggle_input_keyboard();
    state.advance_input_keyboard(ui::MotionFrame::reduced(Duration::ZERO));
    let event = click(&mut state, Action::Character('1')).unwrap();
    state.apply_input(event);
    assert_eq!(
        state
            .pages
            .clock
            .clock_create_state
            .as_ref()
            .unwrap()
            .values[0],
        1
    );
}

#[test]
fn keyboard_reads_custom_page_phase_times() {
    let mut state = session();
    let mut appearance = storage::AppearanceConfig::default();
    appearance.animation_timings.page = [200, 100, 100];
    state.app.dispatch_at(
        app::AppCommand::SetActiveAppearance(Some(appearance)),
        Instant::now(),
    );
    state.interaction.pointer_motion = true;
    state.toggle_input_keyboard();
    state.advance_input_keyboard(ui::MotionFrame::default());
    state.advance_input_keyboard(ui::MotionFrame {
        now: Duration::from_millis(100),
        ..Default::default()
    });
    assert_eq!(state.screen_keyboard.visibility, 100);
    assert!(!state.advance_input_keyboard(ui::MotionFrame {
        now: Duration::from_millis(400),
        ..Default::default()
    }));
    assert_eq!(state.screen_keyboard.visibility, 1_000);
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
        state.interaction.pointer_motion = true;
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
fn shared_keyboard_launcher_opens_and_closes_with_mouse_or_touch_in_each_input_context() {
    for context in ["cli", "aa", "search", "management_form", "editor_form"] {
        for hover in [false, true] {
            let mut state = session();
            let (tx, _rx) = std::sync::mpsc::channel();
            match context {
                "aa" => {
                    state
                        .begin_auto_admin("Keyboard input".into(), false, tx)
                        .unwrap();
                }
                "search" | "management_form" => state.enter_screen(ShellScreen::Management),
                "editor_form" => state.enter_screen(ShellScreen::Editor),
                _ => {}
            }
            if context.ends_with("form") {
                state.config_form(
                    "open",
                    String::new(),
                    vec![platform::management::ManagementField {
                        id: "path".into(),
                        label: "Path".into(),
                        ..Default::default()
                    }],
                );
            }
            for open in [true, false] {
                let area = state.keyboard_launcher_area().unwrap();
                let point = (area.right() - 1, area.bottom() - 1);
                if !open {
                    let keyboard = state.input_keyboard_layout();
                    let hide = keyboard
                        .buttons
                        .iter()
                        .find(|button| button.action == ui::ScreenKeyboardAction::ToggleKeyboard)
                        .unwrap();
                    assert_eq!(area, hide.area);
                    assert_eq!(state.button_at(point), Some(hide.region()));
                    assert_eq!(
                        area.height,
                        keyboard
                            .buttons
                            .iter()
                            .find(|button| button.action == ui::ScreenKeyboardAction::Letter('q'))
                            .unwrap()
                            .area
                            .height
                    );
                }
                if hover {
                    route(&mut state, InputEvent::mouse_moved(point));
                }
                route(
                    &mut state,
                    InputEvent::mouse_down(PointerButton::Left, point),
                );
                assert_ne!(state.input_keyboard_open(), open);
                route(&mut state, InputEvent::mouse_up(PointerButton::Left, point));
                assert_eq!(
                    state.input_keyboard_open(),
                    open,
                    "{context}, hover={hover}"
                );
                state.advance_input_keyboard(ui::MotionFrame::reduced(Duration::ZERO));
            }
        }
    }
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
    let area = state.keyboard_launcher_area().unwrap();
    route(
        &mut state,
        InputEvent::mouse_down(PointerButton::Left, (area.x, area.y)),
    );
    route(
        &mut state,
        InputEvent::mouse_up(PointerButton::Left, (area.x, area.y)),
    );
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

#[test]
fn physical_f1_does_not_open_help_over_the_screen_keyboard() {
    let mut state = session();
    state.toggle_input_keyboard();
    state.apply_input(InputEvent::key(InputKey::F(1)));
    assert!(state.input_keyboard_open());
    assert!(!state.help_open);
}
