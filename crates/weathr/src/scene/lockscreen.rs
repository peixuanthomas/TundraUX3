use crate::render::TerminalRenderer;
use crate::scene::world::ground::Ground;
use crate::scene::world::style::WorldSceneStyle;
use crate::scene::{Scene, SceneContext, SceneLayout, WEATHER_GROUND_HEIGHT};
use std::io;

pub struct LockscreenScene {
    ground: Ground,
    width: u16,
    height: u16,
}

impl LockscreenScene {
    pub fn new(width: u16, height: u16) -> Self {
        Self {
            ground: Ground,
            width,
            height,
        }
    }
}

impl Scene for LockscreenScene {
    fn id(&self) -> &'static str {
        "lockscreen"
    }

    fn update_size(&mut self, width: u16, height: u16) {
        self.width = width;
        self.height = height;
    }

    fn layout(&self) -> SceneLayout {
        SceneLayout {
            ground_y: self.height.saturating_sub(WEATHER_GROUND_HEIGHT),
            chimney_pos: None,
            width: self.width,
            height: self.height,
        }
    }

    fn render(&self, renderer: &mut TerminalRenderer, ctx: &SceneContext<'_>) -> io::Result<()> {
        let layout = self.layout();
        let style = WorldSceneStyle::resolve(ctx);

        self.ground.render(
            renderer,
            self.width,
            WEATHER_GROUND_HEIGHT,
            layout.ground_y,
            &style,
        )
    }
}
