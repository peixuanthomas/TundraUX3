use crate::app::{App, AppInput};
use crate::app_state::BottomHudPrompt;
use crate::assets::WeatherAsciiAssets;
use crate::render::TerminalRenderer;
use crate::theme::ThemeRegistry;
use crate::{ClockFormat, LocalizationProvider, WeathrRunError};
use ratatui_core::buffer::Buffer;
use std::io;
use system_services::SystemSnapshot;
use tokio::sync::watch;

/// Draws weather into memory while the host owns terminal input and output.
pub struct EmbeddedDisplay {
    app: App,
    renderer: TerminalRenderer,
}

impl EmbeddedDisplay {
    pub fn new(
        snapshots: watch::Receiver<SystemSnapshot>,
        localize: LocalizationProvider,
        size: (u16, u16),
    ) -> Result<Self, WeathrRunError> {
        let assets = WeatherAsciiAssets::bundled()?;
        let dimensions = assets.max_dimensions();
        let minimum = (
            u16::try_from(dimensions.0).unwrap_or(u16::MAX),
            u16::try_from(dimensions.1).unwrap_or(u16::MAX),
        );
        let renderer = TerminalRenderer::buffered(size, minimum)?;
        let (term_width, term_height) = renderer.get_size();
        let app = App::new_with_bottom_hud_prompt_and_assets(AppInput {
            localize,
            term_width,
            term_height,
            themes: ThemeRegistry::new(),
            bottom_hud_prompt: BottomHudPrompt::Resume,
            snapshots,
            clock_format: ClockFormat::TwentyFourHour,
            hide_hud: false,
            assets,
        })?;
        Ok(Self { app, renderer })
    }

    pub fn render(&mut self, size: (u16, u16)) -> io::Result<&Buffer> {
        if self.renderer.resize_buffer(size.0, size.1)? {
            let (width, height) = self.renderer.get_size();
            self.app.resize_animations(width, height);
        }
        self.app.render_frame(&mut self.renderer)?;
        Ok(self.renderer.buffer())
    }
}
