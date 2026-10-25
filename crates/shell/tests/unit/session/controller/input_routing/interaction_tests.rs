use super::*;

#[test]
fn script_buttons_keep_shared_feedback_and_native_release_events() {
    let mut session = ShellSession::new(ShellLaunchConfig::default(), (140, 40));
    let point = (20, 10);
    session.button_regions.push(ui::components::ButtonRegion {
        id: "script.action.Debug".into(),
        area: Rect::new(point.0, point.1, 14, 1),
        disabled: false,
    });
    let now = Instant::now();
    let down = InputEvent::mouse_down(PointerButton::Left, point);
    let up = InputEvent::mouse_up(PointerButton::Left, point);
    assert_eq!(session.prepare_button_input(down.clone(), now), Some(down));
    let capture = session.interaction.button_capture.as_ref().unwrap();
    assert!(capture.native_release);
    assert_eq!(capture.region.id.as_str(), "script.action.Debug");
    assert_eq!(session.prepare_button_input(up.clone(), now), Some(up));
    assert!(session.interaction.button_capture.is_none());
    assert_eq!(session.interaction.hover_position(), None);
}

#[test]
fn ordinary_editor_buttons_keep_release_adapted_to_original_press() {
    let mut session = ShellSession::new(ShellLaunchConfig::default(), (140, 40));
    let point = (20, 10);
    session.button_regions.push(ui::components::ButtonRegion {
        id: "editor.menu.File".into(),
        area: Rect::new(point.0, point.1, 14, 1),
        disabled: false,
    });
    let now = Instant::now();
    let down = InputEvent::mouse_down(PointerButton::Left, point);
    assert_eq!(session.prepare_button_input(down.clone(), now), None);
    assert!(
        !session
            .interaction
            .button_capture
            .as_ref()
            .unwrap()
            .native_release
    );
    assert_eq!(
        session.prepare_button_input(InputEvent::mouse_up(PointerButton::Left, point), now),
        Some(down)
    );
}

#[test]
fn stationary_pointer_and_key_releases_cannot_steal_keyboard_emphasis() {
    let mut state = InteractionState::default();
    let point = (10, 5);
    state.observe(&InputEvent::mouse_moved(point), false);
    assert_eq!(state.hover_position(), Some(point));
    state.observe(&InputEvent::key(InputKey::Tab), true);
    state.observe(&InputEvent::mouse_moved(point), false);
    assert!(state.keyboard_focus_visible());
    assert_eq!(state.hover_position(), None);

    state.observe(&InputEvent::mouse_down(PointerButton::Left, point), false);
    state.observe(&InputEvent::mouse_up(PointerButton::Left, point), false);
    state.observe(&InputEvent::mouse_moved(point), false);
    assert!(!state.keyboard_focus_visible());
    assert_eq!(state.hover_position(), None);
    state.observe(&InputEvent::mouse_moved((11, 5)), false);
    assert_eq!(state.hover_position(), Some((11, 5)));

    state.observe(&InputEvent::FocusLost, false);
    state.observe(&InputEvent::FocusGained, false);
    state.observe(
        &InputEvent::Key(KeyInput::with_phase(
            InputKey::Tab,
            InputModifiers::default(),
            InputPhase::Release,
        )),
        true,
    );
    assert!(!state.keyboard_focus_visible());
    assert_eq!(state.hover_position(), None);
    state.observe(&InputEvent::key(InputKey::Tab), true);
    assert!(state.keyboard_focus_visible());
}

#[test]
fn releasing_a_modifier_does_not_replay_a_held_search_shortcut_into_text() {
    let mut state = InteractionState {
        opening_key: Some(InputKey::Char('f')),
        ..Default::default()
    };
    let key =
        |key, phase| InputEvent::Key(KeyInput::with_phase(key, InputModifiers::default(), phase));
    assert!(
        !state
            .consumes_opening_repeat(&key(InputKey::Other("Control".into()), InputPhase::Release))
    );
    assert!(state.consumes_opening_repeat(&key(InputKey::Char('f'), InputPhase::Repeat)));
    assert!(!state.consumes_opening_repeat(&key(InputKey::Char('f'), InputPhase::Release)));
    assert!(!state.consumes_opening_repeat(&key(InputKey::Char('f'), InputPhase::Press)));
    assert!(!state.consumes_opening_repeat(&key(InputKey::Char('f'), InputPhase::Repeat)));
}
