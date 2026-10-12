use super::*;

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
