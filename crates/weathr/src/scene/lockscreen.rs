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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assets::WorldSceneAssets;
    use crate::scene::world::WorldScene;

    #[test]
    fn layout_matches_world_weather_bounds_without_chimney() {
        let lockscreen = LockscreenScene::new(120, 40);
        let world = WorldScene::new(120, 40, WorldSceneAssets::placeholder());

        let lockscreen_layout = lockscreen.layout();
        let world_layout = world.layout();

        assert_eq!(lockscreen.id(), "lockscreen");
        assert_eq!(lockscreen_layout.ground_y, world_layout.ground_y);
        assert_eq!(lockscreen_layout.width, world_layout.width);
        assert_eq!(lockscreen_layout.height, world_layout.height);
        assert!(lockscreen_layout.chimney_pos.is_none());
        assert!(world_layout.chimney_pos.is_some());
    }

    #[test]
    fn layout_updates_with_terminal_size() {
        let mut lockscreen = LockscreenScene::new(120, 40);

        lockscreen.update_size(80, 24);
        let layout = lockscreen.layout();

        assert_eq!(layout.width, 80);
        assert_eq!(layout.height, 24);
        assert_eq!(
            layout.ground_y,
            24_u16.saturating_sub(WEATHER_GROUND_HEIGHT)
        );
        assert!(layout.chimney_pos.is_none());
    }
}
