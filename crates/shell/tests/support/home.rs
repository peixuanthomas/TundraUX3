use platform::Platform;
use shell::{InputEvent, InputKey, ShellScreen, ShellSession};

pub fn open_home_entry(state: &mut ShellSession, platform: &dyn Platform, key: &str) {
    assert_eq!(state.active_screen(), ShellScreen::Home);
    let index = state
        .to_home_view_model()
        .entries()
        .iter()
        .position(|entry| entry.icon_identity() == key)
        .expect("Home entry");
    state.apply_input_with_platform(InputEvent::key(InputKey::Home), platform);
    for _ in 0..index {
        state.apply_input_with_platform(InputEvent::key(InputKey::Right), platform);
    }
    state.apply_input_with_platform(InputEvent::key(InputKey::Enter), platform);
}
