use super::*;
use crate::render::TerminalRenderer;
use crate::scene::overlay::SceneOverlay;
use crate::scene::{Scene, SceneContext, SceneLayout};
use crate::theme::catalogue::DEFAULT_PALETTE;
use crate::theme::{Theme, ThemeRegistry};
use std::io;
use std::sync::atomic::{AtomicUsize, Ordering};

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
