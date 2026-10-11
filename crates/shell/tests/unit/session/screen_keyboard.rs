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

fn route(state: &mut ShellSession, input: InputEvent) -> Option<InputEvent> {
    state.route_screen_keyboard_input(input, platform::native_platform().as_ref(), Instant::now())
}

fn click(state: &mut ShellSession, action: Action) -> Option<InputEvent> {
    let layout = state.input_keyboard_layout();
    let area = layout
        .buttons
        .iter()
        .find(|button| button.action == action)
        .unwrap()
        .area;
    assert!(
        route(
            state,
            InputEvent::mouse_down(PointerButton::Left, (area.x, area.y))
        )
        .is_none()
    );
    route(
        state,
        InputEvent::mouse_up(PointerButton::Left, (area.x, area.y)),
    )
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
    assert!(!state.input_keyboard_open());
    assert_eq!(state.command_line_input_area().unwrap(), original);
    assert_eq!(
        state.screen_keyboard.modifiers,
        ScreenKeyboardModifiers::default()
    );
}

#[test]
fn keyboard_cancels_clicks_on_drag_resize_focus_loss_timeout_and_page_change() {
    for cancel in 0..5 {
        let mut state = session();
        state.toggle_input_keyboard();
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
            _ => {
                state.enter_screen(ShellScreen::Launcher);
                state.sync_input_keyboard();
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
    state.show_command_line_close_confirmation(state.active_page_id());
    state.sync_input_keyboard();
    assert!(!state.input_keyboard_open());
    while state.notification_dismiss_active_modal_without_response() {}
    state.sync_input_keyboard();
    assert!(!state.input_keyboard_open());
    state.toggle_input_keyboard();
    state.enter_new_page(ShellScreen::CommandLine);
    state.sync_input_keyboard();
    assert!(!state.input_keyboard_open());
}
