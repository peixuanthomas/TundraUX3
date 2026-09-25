use super::*;
use crate::render::TerminalRenderer;
use crate::scene::overlay::SceneOverlay;
use crate::scene::{Scene, SceneContext, SceneLayout};
use crate::theme::catalogue::DEFAULT_PALETTE;
use crate::theme::{Theme, ThemeRegistry};
use std::io;
use std::sync::atomic::{AtomicUsize, Ordering};

#[test]
fn start_accepts_character_navigation_function_and_control_keys() {
    use crossterm::event::KeyEvent;
    for (code, modifiers) in [
        (KeyCode::Char(' '), KeyModifiers::NONE),
        (KeyCode::Char('a'), KeyModifiers::NONE),
        (KeyCode::Char('中'), KeyModifiers::NONE),
        (KeyCode::Enter, KeyModifiers::NONE),
        (KeyCode::Esc, KeyModifiers::NONE),
        (KeyCode::Tab, KeyModifiers::NONE),
        (KeyCode::Left, KeyModifiers::NONE),
        (KeyCode::F(1), KeyModifiers::NONE),
        (KeyCode::Char('c'), KeyModifiers::CONTROL),
    ] {
        assert_eq!(
            input_outcome(
                Event::Key(KeyEvent::new(code, modifiers)),
                BottomHudPrompt::Start
            ),
            Some(AppRunOutcome::Continue),
            "{code:?} with {modifiers:?}"
        );
    }
    for kind in [KeyEventKind::Release, KeyEventKind::Repeat] {
        assert_eq!(
            input_outcome(
                Event::Key(KeyEvent::new_with_kind(
                    KeyCode::Char('l'),
                    KeyModifiers::NONE,
                    kind
                )),
                BottomHudPrompt::Start
            ),
            None,
            "held/released keys must not immediately re-enter after locking"
        );
    }
}

#[test]
fn start_accepts_clicks_anywhere_but_ignores_other_mouse_events() {
    use crossterm::event::{MouseButton, MouseEvent};
    for button in [MouseButton::Left, MouseButton::Right, MouseButton::Middle] {
        for (column, row) in [(0, 0), (40, 12), (119, 39)] {
            let event = Event::Mouse(MouseEvent {
                kind: MouseEventKind::Down(button),
                column,
                row,
                modifiers: KeyModifiers::NONE,
            });
            assert_eq!(
                input_outcome(event.clone(), BottomHudPrompt::Start),
                Some(AppRunOutcome::Continue)
            );
            assert_eq!(input_outcome(event, BottomHudPrompt::Quit), None);
        }
    }
    for kind in [
        MouseEventKind::Moved,
        MouseEventKind::Drag(MouseButton::Left),
        MouseEventKind::Up(MouseButton::Left),
        MouseEventKind::ScrollUp,
        MouseEventKind::ScrollDown,
    ] {
        assert_eq!(
            input_outcome(
                Event::Mouse(MouseEvent {
                    kind,
                    column: 40,
                    row: 12,
                    modifiers: KeyModifiers::NONE,
                }),
                BottomHudPrompt::Start
            ),
            None
        );
    }
    for event in [
        Event::FocusGained,
        Event::FocusLost,
        Event::Resize(120, 40),
        Event::Paste("text".into()),
    ] {
        assert_eq!(input_outcome(event, BottomHudPrompt::Start), None);
    }
}

#[test]
fn quit_mode_keeps_space_and_control_c_actions() {
    use crossterm::event::KeyEvent;
    for (code, modifiers, expected) in [
        (
            KeyCode::Char(' '),
            KeyModifiers::NONE,
            Some(AppRunOutcome::Continue),
        ),
        (
            KeyCode::Char('c'),
            KeyModifiers::CONTROL,
            Some(AppRunOutcome::Cancelled),
        ),
        (KeyCode::Char('a'), KeyModifiers::NONE, None),
    ] {
        assert_eq!(
            input_outcome(
                Event::Key(KeyEvent::new(code, modifiers)),
                BottomHudPrompt::Quit
            ),
            expected
        );
    }
}

#[test]
fn first_frame_callback_runs_once_after_successful_flush() {
    let calls = Arc::new(AtomicUsize::new(0));
    let callback_calls = calls.clone();
    let mut callback: Option<Arc<dyn Fn() -> io::Result<()> + Send + Sync>> =
        Some(Arc::new(move || {
            callback_calls.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }));

    flush_and_notify_first_frame(&mut callback, || Ok(())).unwrap();
    flush_and_notify_first_frame(&mut callback, || Ok(())).unwrap();

    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[test]
fn first_frame_callback_error_is_returned() {
    let mut callback: Option<Arc<dyn Fn() -> io::Result<()> + Send + Sync>> =
        Some(Arc::new(|| Err(io::Error::other("ready failed"))));
    let error = flush_and_notify_first_frame(&mut callback, || Ok(())).unwrap_err();
    assert_eq!(error.to_string(), "ready failed");
}

#[test]
fn first_frame_callback_waits_for_successful_flush() {
    let calls = Arc::new(AtomicUsize::new(0));
    let callback_calls = calls.clone();
    let mut callback: Option<Arc<dyn Fn() -> io::Result<()> + Send + Sync>> =
        Some(Arc::new(move || {
            callback_calls.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }));
    assert!(
        flush_and_notify_first_frame(&mut callback, || Err(io::Error::other("flush failed")))
            .is_err()
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    flush_and_notify_first_frame(&mut callback, || Ok(())).unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

struct TestScene {
    id: &'static str,
}

impl TestScene {
    fn new(id: &'static str) -> Self {
        Self { id }
    }
}

impl Scene for TestScene {
    fn id(&self) -> &'static str {
        self.id
    }

    fn update_size(&mut self, _width: u16, _height: u16) {}

    fn render(&self, _renderer: &mut TerminalRenderer, _ctx: &SceneContext<'_>) -> io::Result<()> {
        Ok(())
    }

    fn layout(&self) -> SceneLayout {
        SceneLayout {
            ground_y: 0,
            chimney_pos: None,
            width: 0,
            height: 0,
        }
    }
}

struct TestOverlay {
    id: &'static str,
}

impl TestOverlay {
    fn new(id: &'static str) -> Self {
        Self { id }
    }
}

impl SceneOverlay for TestOverlay {
    fn id(&self) -> &'static str {
        self.id
    }

    fn update_size(&mut self, _width: u16, _height: u16) {}

    fn render(
        &self,
        _renderer: &mut TerminalRenderer,
        _ctx: &SceneContext<'_>,
        _layout: &SceneLayout,
    ) -> io::Result<()> {
        Ok(())
    }
}

fn scene_registry_with_lockscreen_and_world() -> SceneRegistry {
    let mut scenes = SceneRegistry::new();
    scenes.register(Box::new(TestScene::new("lockscreen")));
    scenes.register(Box::new(TestScene::new("world")));
    scenes
}

#[test]
fn bindings_fall_back_to_default_when_scene_missing() {
    let scenes = scene_registry_with_lockscreen_and_world();
    let overlays = OverlayRegistry::new();
    let mut themes = ThemeRegistry::new();
    themes.register(Theme {
        id: "custom",
        display_name: "Custom",
        scene_id: "unknown",
        overlay_id: None,
        palette: DEFAULT_PALETTE,
    });
    themes.set_active("custom").unwrap();

    let bindings = resolve_theme_bindings(&themes, &scenes, &overlays);

    assert_eq!(bindings.theme_id, DEFAULT_THEME_ID);
    assert_eq!(bindings.scene_id, "lockscreen");
    assert_eq!(bindings.overlay_id, None);
}

#[test]
fn bindings_disable_unregistered_overlay() {
    let scenes = scene_registry_with_lockscreen_and_world();
    let overlays = OverlayRegistry::new();
    let mut themes = ThemeRegistry::new();
    themes.register(Theme {
        id: "overlay-theme",
        display_name: "Overlay Theme",
        scene_id: "world",
        overlay_id: Some("hud"),
        palette: DEFAULT_PALETTE,
    });
    themes.set_active("overlay-theme").unwrap();

    let bindings = resolve_theme_bindings(&themes, &scenes, &overlays);

    assert_eq!(bindings.theme_id, "overlay-theme");
    assert_eq!(bindings.scene_id, "world");
    assert_eq!(bindings.overlay_id, None);
}

#[test]
fn bindings_keep_registered_overlay() {
    let scenes = scene_registry_with_lockscreen_and_world();
    let mut overlays = OverlayRegistry::new();
    overlays.register(Box::new(TestOverlay::new("hud")));
    let mut themes = ThemeRegistry::new();
    themes.register(Theme {
        id: "overlay",
        display_name: "Overlay",
        scene_id: "world",
        overlay_id: Some("hud"),
        palette: DEFAULT_PALETTE,
    });
    themes.set_active("overlay").unwrap();

    let bindings = resolve_theme_bindings(&themes, &scenes, &overlays);

    assert_eq!(bindings.theme_id, "overlay");
    assert_eq!(bindings.overlay_id, Some("hud"));
}
