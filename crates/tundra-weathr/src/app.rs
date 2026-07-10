use crate::animation_manager::AnimationManager;
use crate::app_state::{AppState, BottomHudPrompt};
use crate::assets::WeatherAsciiAssets;
use crate::config::{ClockFormat, Config, Provider};
use crate::error::{WeatherAssetError, WeatherError};
use crate::network_clock::{self, NetworkClock, TimeSyncResult};
use crate::render::{TerminalRenderer, clock};
use crate::scene::lockscreen::LockscreenScene;
use crate::scene::overlay::OverlayRegistry;
use crate::scene::world::WorldScene;
use crate::scene::{SceneContext, SceneRegistry};
use crate::theme::ThemeRegistry;

use crate::weather::provider::WeatherProvider;
use crate::weather::{OpenMeteoProvider, WeatherClient, WeatherData, WeatherLocation};
use crossterm::event::{self, Event, KeyCode, KeyModifiers};
use std::io;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;

const REFRESH_INTERVAL: Duration = Duration::from_secs(300);
const INPUT_POLL_FPS: u64 = 30;
const FRAME_DURATION: Duration = Duration::from_millis(1000 / INPUT_POLL_FPS);
const DEFAULT_THEME_ID: &str = "default";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AppRunOutcome {
    Space,
    Cancelled,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ThemeBindings {
    theme_id: &'static str,
    scene_id: &'static str,
    overlay_id: Option<&'static str>,
}

fn resolve_theme_bindings(
    themes: &ThemeRegistry,
    scenes: &SceneRegistry,
    overlays: &OverlayRegistry,
) -> ThemeBindings {
    let active_theme = themes.active();
    let mut theme_id = active_theme.id;
    let mut scene_id = active_theme.scene_id;
    let mut overlay_id = active_theme.overlay_id;

    let scene_missing = scenes.get(scene_id).is_none();
    if scene_missing {
        if theme_id != DEFAULT_THEME_ID {
            eprintln!(
                "Warning: theme '{}' references missing scene '{}'. Falling back to '{}'.",
                theme_id, scene_id, DEFAULT_THEME_ID
            );
            let fallback_theme = themes
                .get(DEFAULT_THEME_ID)
                .expect("default theme must be registered");
            theme_id = fallback_theme.id;
            scene_id = fallback_theme.scene_id;
            overlay_id = fallback_theme.overlay_id;
        } else {
            panic!("default theme references missing scene '{}'.", scene_id);
        }
    }

    if scenes.get(scene_id).is_none() {
        panic!(
            "theme '{}' references missing scene '{}', and no fallback scene is available",
            theme_id, scene_id
        );
    }

    let validated_overlay = overlay_id.and_then(|id| {
        if overlays.get(id).is_some() {
            Some(id)
        } else {
            eprintln!(
                "Warning: theme '{}' references missing overlay '{}'. Overlay disabled.",
                theme_id, id
            );
            None
        }
    });

    ThemeBindings {
        theme_id,
        scene_id,
        overlay_id: validated_overlay,
    }
}

fn build_visual_registries(
    term_width: u16,
    term_height: u16,
    assets: &WeatherAsciiAssets,
) -> (AnimationManager, SceneRegistry) {
    let animations = AnimationManager::new(term_width, term_height, false, assets.animation());

    let mut scenes = SceneRegistry::new();
    scenes.register(Box::new(LockscreenScene::new(term_width, term_height)));
    scenes.register(Box::new(WorldScene::new(
        term_width,
        term_height,
        assets.world().clone(),
    )));

    (animations, scenes)
}

pub struct App {
    state: AppState,
    animations: AnimationManager,
    scenes: SceneRegistry,
    overlays: OverlayRegistry,
    themes: ThemeRegistry,
    ascii_assets: WeatherAsciiAssets,
    active_scene_id: &'static str,
    active_overlay_id: Option<&'static str>,
    weather_receiver: mpsc::Receiver<Result<WeatherData, WeatherError>>,
    time_receiver: mpsc::Receiver<TimeSyncResult>,
    clock: NetworkClock,
    clock_format: ClockFormat,
}

fn render_centered_line(
    renderer: &mut TerminalRenderer,
    width: u16,
    row: u16,
    text: &str,
    color: crossterm::style::Color,
) -> io::Result<()> {
    let text_width = text.chars().count() as u16;
    let col = width.saturating_sub(text_width) / 2;
    renderer.render_line_colored(col, row, text, color)
}

impl App {
    pub fn new(
        config: &Config,
        term_width: u16,
        term_height: u16,
        themes: ThemeRegistry,
        timezone_id: Option<String>,
    ) -> Result<Self, WeatherAssetError> {
        Self::new_with_bottom_hud_prompt(
            config,
            term_width,
            term_height,
            themes,
            timezone_id,
            BottomHudPrompt::Quit,
        )
    }

    pub(crate) fn new_with_bottom_hud_prompt(
        config: &Config,
        term_width: u16,
        term_height: u16,
        themes: ThemeRegistry,
        timezone_id: Option<String>,
        bottom_hud_prompt: BottomHudPrompt,
    ) -> Result<Self, WeatherAssetError> {
        let location = WeatherLocation {
            latitude: config.location.latitude,
            longitude: config.location.longitude,
            elevation: None,
        };

        let state = AppState::new_with_bottom_hud_prompt(
            location,
            config.location.city.clone(),
            config.location.display,
            config.location.hide,
            config.units,
            bottom_hud_prompt,
        );

        let requested_theme_id = themes.active().id;
        let mut ascii_assets = WeatherAsciiAssets::load(requested_theme_id)?;
        let (mut animations, mut scenes) =
            build_visual_registries(term_width, term_height, &ascii_assets);
        let overlays = OverlayRegistry::new();
        let bindings = resolve_theme_bindings(&themes, &scenes, &overlays);

        if bindings.theme_id != requested_theme_id {
            ascii_assets = WeatherAsciiAssets::load(bindings.theme_id)?;
            (animations, scenes) = build_visual_registries(term_width, term_height, &ascii_assets);
        }

        let (tx, rx) = mpsc::channel(1);

        let wanted_provider = Provider::OpenMeteo;
        let provider: Arc<dyn WeatherProvider> = Arc::new(OpenMeteoProvider::new());
        let weather_client = WeatherClient::new(provider, REFRESH_INTERVAL);
        let units = config.units;

        tokio::spawn(async move {
            loop {
                let result = weather_client
                    .get_current_weather(&location, &units, wanted_provider)
                    .await;
                if tx.send(result).await.is_err() {
                    break;
                }
                tokio::time::sleep(REFRESH_INTERVAL).await;
            }
        });

        let (time_tx, time_rx) = mpsc::channel(1);
        tokio::spawn(async move {
            loop {
                let result = network_clock::fetch_standard_time().await;
                if time_tx.send(result).await.is_err() {
                    break;
                }
                tokio::time::sleep(network_clock::TIME_SYNC_INTERVAL).await;
            }
        });

        Ok(Self {
            state,
            animations,
            scenes,
            overlays,
            themes,
            ascii_assets,
            active_scene_id: bindings.scene_id,
            active_overlay_id: bindings.overlay_id,
            weather_receiver: rx,
            time_receiver: time_rx,
            clock: NetworkClock::new(timezone_id),
            clock_format: config.lockscreen.clock_format,
        })
    }

    pub async fn run(&mut self, renderer: &mut TerminalRenderer) -> io::Result<()> {
        self.run_with_outcome(renderer).await.map(|_| ())
    }

    pub(crate) async fn run_with_outcome(
        &mut self,
        renderer: &mut TerminalRenderer,
    ) -> io::Result<AppRunOutcome> {
        let mut rng = rand::rng();
        let mut attribution = "Awaiting weather data".to_string();

        loop {
            match self.weather_receiver.try_recv() {
                Ok(result) => match result {
                    Ok(weather) => {
                        let rain_intensity = weather.condition.rain_intensity();
                        let snow_intensity = weather.condition.snow_intensity();
                        let fog_intensity = weather.condition.fog_intensity();
                        let wind_speed = weather.wind_speed;
                        let wind_direction = weather.wind_direction;
                        attribution = weather.attribution.clone();

                        if let Some(moon_phase) = weather.moon_phase {
                            self.animations.update_moon_phase(moon_phase);
                        }

                        self.state.update_weather(weather);
                        self.animations.update_rain_intensity(rain_intensity);
                        self.animations.update_snow_intensity(snow_intensity);
                        self.animations.update_fog_intensity(fog_intensity);
                        self.animations
                            .update_wind(wind_speed as f32, wind_direction as f32);
                    }
                    Err(error) => {
                        let error_msg = match &error {
                            WeatherError::Network(net_err) => net_err.user_friendly_message(),
                            _ => format!("Failed to fetch weather: {}", error),
                        };

                        self.state.clear_weather_for_offline();
                        attribution = format!("Provider failed with {error_msg}");
                    }
                },
                Err(e) => {
                    if e == mpsc::error::TryRecvError::Disconnected {
                        attribution = "".to_string();
                    }
                }
            }

            loop {
                match self.time_receiver.try_recv() {
                    Ok(result) => self.clock.apply_sync(result),
                    Err(mpsc::error::TryRecvError::Empty) => break,
                    Err(mpsc::error::TryRecvError::Disconnected) => break,
                }
            }

            renderer.clear()?;

            let theme = self.themes.active();
            let palette = &theme.palette;

            let (term_width, term_height) = renderer.get_size();
            let scene = self
                .scenes
                .get_mut(self.active_scene_id)
                .expect("active scene must be registered");
            scene.update_size(term_width, term_height);

            let layout = scene.layout();
            let ctx = SceneContext {
                conditions: &self.state.weather_conditions,
                palette,
            };

            self.animations.render_background(
                renderer,
                &self.state.weather_conditions,
                &self.state,
                &layout,
                &mut rng,
            )?;

            scene.render(renderer, &ctx)?;

            if let Some(ov_id) = self.active_overlay_id
                && let Some(overlay) = self.overlays.get_mut(ov_id)
            {
                overlay.update_size(term_width, term_height);
                overlay.render(renderer, &ctx, &layout)?;
            }

            self.animations.render_chimney_smoke(
                renderer,
                &self.state.weather_conditions,
                &self.state,
                &layout,
                &mut rng,
            )?;

            self.animations.render_foreground(
                renderer,
                &self.state.weather_conditions,
                &self.state,
                &layout,
                &mut rng,
            )?;

            let current = self.clock.current();
            let time_text = clock::format_time(current.time, self.clock_format);
            let clock_font = self.ascii_assets.clock_font();
            let clock_lines = clock::ascii_lines(&time_text, clock_font);
            let clock_layout = clock::separator_anchored_layout(
                &time_text,
                &clock_lines,
                clock_font,
                term_width,
                term_height,
            );

            let date_text = current.date.format("%Y-%m-%d").to_string();
            render_centered_line(
                renderer,
                term_width,
                clock_layout.row.saturating_sub(2),
                &date_text,
                crossterm::style::Color::Grey,
            )?;

            for (idx, line) in clock_lines.iter().enumerate() {
                renderer.render_line_colored(
                    clock_layout.col,
                    clock_layout.row + idx as u16,
                    line,
                    crossterm::style::Color::White,
                )?;
            }

            let weather_row = clock_layout
                .row
                .saturating_add(clock_lines.len() as u16)
                .saturating_add(1);
            if weather_row < term_height.saturating_sub(1)
                && let Some(weather_summary) = self.state.weather_summary_text()
            {
                render_centered_line(
                    renderer,
                    term_width,
                    weather_row,
                    &weather_summary,
                    crossterm::style::Color::Cyan,
                )?;
            }

            self.state.update_loading_animation();
            self.state.update_cached_info();

            renderer.render_line_colored(
                2,
                term_height.saturating_sub(1),
                &self.state.cached_weather_info,
                crossterm::style::Color::Cyan,
            )?;

            let mut next_status_row = 0;
            if !attribution.is_empty() {
                renderer.render_line_colored(
                    2,
                    0,
                    &attribution,
                    crossterm::style::Color::DarkGrey,
                )?;
                next_status_row = 1;
            }

            if let Some(warning) = current.warning {
                renderer.render_line_colored(
                    2,
                    next_status_row,
                    &warning,
                    crossterm::style::Color::Yellow,
                )?;
            }

            renderer.flush()?;

            if event::poll(FRAME_DURATION)? {
                match event::read()? {
                    Event::Resize(width, height) => {
                        renderer.manual_resize(width, height)?;
                        let (new_width, new_height) = renderer.get_size();
                        self.animations.on_resize(new_width, new_height);
                    }
                    Event::Key(key_event) => match key_event.code {
                        KeyCode::Char(' ') => return Ok(AppRunOutcome::Space),
                        KeyCode::Char('c')
                            if key_event.modifiers.contains(KeyModifiers::CONTROL) =>
                        {
                            return Ok(AppRunOutcome::Cancelled);
                        }
                        _ => {}
                    },
                    _ => {}
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::TerminalRenderer;
    use crate::scene::overlay::SceneOverlay;
    use crate::scene::{Scene, SceneContext, SceneLayout};
    use crate::theme::catalogue::DEFAULT_PALETTE;
    use crate::theme::{Theme, ThemeRegistry};
    use std::io;

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

        fn render(
            &self,
            _renderer: &mut TerminalRenderer,
            _ctx: &SceneContext<'_>,
        ) -> io::Result<()> {
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
}
