use crate::error::WeatherAssetError;
use crate::render::clock::ClockFont;

#[derive(Clone, Debug)]
pub(crate) struct WeatherAnimationAssets {
    pub clouds: Vec<Vec<String>>,
    pub sun_frames: Vec<Vec<String>>,
    pub moon_phases: Vec<Vec<String>>,
    pub airplane: Vec<String>,
}

#[derive(Clone, Debug)]
pub(crate) struct WorldSceneAssets {
    pub house: Vec<String>,
    pub tree: Vec<String>,
    pub fence: Vec<String>,
    pub mailbox: Vec<String>,
    pub pine_tree: Vec<String>,
}

/// Display-ready assets compiled into Weathr.
///
/// Hosts may manage configurable ASCII assets for their own screens, but the
/// weather renderer consumes only this self-contained default scene. That
/// keeps its startup path free of filesystem, TOML and cache concerns.
pub(crate) struct WeatherAsciiAssets {
    animation: WeatherAnimationAssets,
    world: WorldSceneAssets,
    clock_font: ClockFont,
}

impl WeatherAsciiAssets {
    pub(crate) fn bundled() -> Result<Self, WeatherAssetError> {
        Ok(Self {
            animation: WeatherAnimationAssets {
                clouds: vec![
                    text_art(include_str!("animation/assets/cloud_0.txt")),
                    text_art(include_str!("animation/assets/cloud_1.txt")),
                    text_art(include_str!("animation/assets/cloud_2.txt")),
                    text_art(include_str!("animation/assets/cloud_3.txt")),
                ],
                sun_frames: vec![
                    text_art(include_str!("animation/assets/sun_0.txt")),
                    text_art(include_str!("animation/assets/sun_1.txt")),
                ],
                moon_phases: vec![
                    text_art(include_str!("animation/assets/moon/phase_0.txt")),
                    text_art(include_str!("animation/assets/moon/phase_1.txt")),
                    text_art(include_str!("animation/assets/moon/phase_2.txt")),
                    text_art(include_str!("animation/assets/moon/phase_3.txt")),
                    text_art(include_str!("animation/assets/moon/phase_4.txt")),
                    text_art(include_str!("animation/assets/moon/phase_5.txt")),
                    text_art(include_str!("animation/assets/moon/phase_6.txt")),
                    text_art(include_str!("animation/assets/moon/phase_7.txt")),
                ],
                airplane: text_art(include_str!("animation/assets/airplane.txt")),
            },
            world: WorldSceneAssets {
                house: text_art(include_str!("scene/world/assets/house.txt")),
                tree: text_art(include_str!("scene/world/assets/tree.txt")),
                fence: text_art(include_str!("scene/world/assets/fence.txt")),
                mailbox: text_art(include_str!("scene/world/assets/mailbox.txt")),
                pine_tree: text_art(include_str!("scene/world/assets/pine_tree.txt")),
            },
            clock_font: ClockFont::from_static(7, 1, 5, BUNDLED_CLOCK_GLYPHS)?,
        })
    }

    pub(crate) fn animation(&self) -> &WeatherAnimationAssets {
        &self.animation
    }

    pub(crate) fn world(&self) -> &WorldSceneAssets {
        &self.world
    }

    pub(crate) fn clock_font(&self) -> &ClockFont {
        &self.clock_font
    }

    pub(crate) fn max_dimensions(&self) -> (usize, usize) {
        let mut dimensions = (0, 0);
        for art in &self.animation.clouds {
            include_dimensions(&mut dimensions, art);
        }
        for art in &self.animation.sun_frames {
            include_dimensions(&mut dimensions, art);
        }
        for art in &self.animation.moon_phases {
            include_dimensions(&mut dimensions, art);
        }
        include_dimensions(&mut dimensions, &self.animation.airplane);
        include_dimensions(&mut dimensions, &self.world.house);
        include_dimensions(&mut dimensions, &self.world.tree);
        include_dimensions(&mut dimensions, &self.world.fence);
        include_dimensions(&mut dimensions, &self.world.mailbox);
        include_dimensions(&mut dimensions, &self.world.pine_tree);
        dimensions.0 = dimensions.0.max(self.clock_font.max_rendered_clock_width());
        dimensions.1 = dimensions.1.max(self.clock_font.height());
        dimensions
    }
}

fn text_art(source: &str) -> Vec<String> {
    source
        .trim_end_matches(['\r', '\n'])
        .lines()
        .map(|line| line.trim_end_matches('\r').to_string())
        .collect()
}

fn include_dimensions(dimensions: &mut (usize, usize), art: &[String]) {
    dimensions.0 = dimensions.0.max(
        art.iter()
            .map(|line| line.chars().count())
            .max()
            .unwrap_or(0),
    );
    dimensions.1 = dimensions.1.max(art.len());
}

const BUNDLED_CLOCK_GLYPHS: &[(char, &[&str])] = &[
    (
        '0',
        &[
            "  .oooo.",
            " d8P'`Y8b",
            "888    888",
            "888    888",
            "888    888",
            "`88b  d88'",
            " `Y8bd8P'",
        ],
    ),
    (
        '1',
        &[
            "   .o", " o888", "  888", "  888", "  888", "  888", " o888o",
        ],
    ),
    (
        '2',
        &[
            " .oooo.",
            ".dP\"\"Y88b",
            "      ]8P'",
            "    .d8P'",
            "  .dP'",
            ".oP     .o",
            "8888888888",
        ],
    ),
    (
        '3',
        &[
            " .oooo.",
            ".dP\"\"Y88b",
            "      ]8P'",
            "   <88b.",
            "    `88b.",
            "o.   .88P",
            "`8bd88P'",
        ],
    ),
    (
        '4',
        &[
            "      .o",
            "    .d88",
            "  .d'888",
            ".d'  888",
            "88ooo888oo",
            "     888",
            "    o888o",
        ],
    ),
    (
        '5',
        &[
            "oooooooo",
            "dP\"\"\"\"\"\"\"",
            "d88888b.",
            "    `Y88b",
            "      ]88",
            "o.   .88P",
            "`8bd88P'",
        ],
    ),
    (
        '6',
        &[
            " .ooo",
            ".88'",
            "d88'",
            "d888P\"Ybo.",
            "Y88[   ]88",
            "`Y88   88P",
            " `88bod8'",
        ],
    ),
    (
        '7',
        &[
            "ooooooooo",
            "d\"\"\"\"\"\"\"8'",
            "      .8'",
            "     .8'",
            "    .8'",
            "   .8'",
            "  .8'",
        ],
    ),
    (
        '8',
        &[
            " .ooooo.",
            "d88'   `8.",
            "Y88..  .8'",
            " `88888b.",
            ".8'  ``88b",
            "`8.   .88P",
            " `boood8'",
        ],
    ),
    (
        '9',
        &[
            " .ooooo.",
            "888' `Y88.",
            "888    888",
            "`Vbood888",
            "      888'",
            "    .88P'",
            "  .oP'",
        ],
    ),
    (':', &["  ", "##", "##", "  ", "##", "##", "  "]),
    (' ', &["   ", "   ", "   ", "   ", "   ", "   ", "   "]),
    (
        'A',
        &[
            "  ###  ", " #   # ", "#     #", "#######", "#     #", "#     #", "#     #",
        ],
    ),
    (
        'P',
        &[
            "###### ", "#     #", "#     #", "###### ", "#      ", "#      ", "#      ",
        ],
    ),
    (
        'M',
        &[
            "#     #", "##   ##", "# # # #", "#  #  #", "#     #", "#     #", "#     #",
        ],
    ),
];

#[cfg(test)]
impl WorldSceneAssets {
    pub(crate) fn placeholder() -> Self {
        Self {
            house: vec![String::new(); 10],
            tree: vec![String::new()],
            fence: vec![String::new()],
            mailbox: vec![String::new()],
            pine_tree: vec![String::new()],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_assets_are_complete_and_self_contained() {
        let assets = WeatherAsciiAssets::bundled().expect("bundled font is valid");

        assert_eq!(assets.animation().clouds.len(), 4);
        assert_eq!(assets.animation().sun_frames.len(), 2);
        assert_eq!(assets.animation().moon_phases.len(), 8);
        assert_eq!(assets.clock_font().height(), 7);
        assert!(assets.max_dimensions().0 >= 64);
        assert!(assets.max_dimensions().1 >= 10);
    }
}
