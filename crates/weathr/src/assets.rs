use crate::error::WeatherAssetError;
use crate::render::clock::ClockFont;
use ascii_assets::{AsciiAssetStore, AssetError};

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

pub(crate) struct WeatherAsciiAssets {
    animation: WeatherAnimationAssets,
    world: WorldSceneAssets,
    clock_font: ClockFont,
}

impl WeatherAsciiAssets {
    pub(crate) fn load(theme_id: &str) -> Result<Self, WeatherAssetError> {
        let store = AsciiAssetStore::load_theme(theme_id)?;
        Self::from_store(&store)
    }

    fn from_store(store: &AsciiAssetStore) -> Result<Self, WeatherAssetError> {
        Ok(Self {
            animation: WeatherAnimationAssets {
                clouds: load_many(
                    store,
                    &[
                        "weathr/animation/cloud_0",
                        "weathr/animation/cloud_1",
                        "weathr/animation/cloud_2",
                        "weathr/animation/cloud_3",
                    ],
                )?,
                sun_frames: load_many(
                    store,
                    &["weathr/animation/sun_0", "weathr/animation/sun_1"],
                )?,
                moon_phases: load_many(
                    store,
                    &[
                        "weathr/animation/moon/phase_0",
                        "weathr/animation/moon/phase_1",
                        "weathr/animation/moon/phase_2",
                        "weathr/animation/moon/phase_3",
                        "weathr/animation/moon/phase_4",
                        "weathr/animation/moon/phase_5",
                        "weathr/animation/moon/phase_6",
                        "weathr/animation/moon/phase_7",
                    ],
                )?,
                airplane: load_text_art(store, "weathr/animation/airplane")?,
            },
            world: WorldSceneAssets {
                house: load_text_art(store, "weathr/world/house")?,
                tree: load_text_art(store, "weathr/world/tree")?,
                fence: load_text_art(store, "weathr/world/fence")?,
                mailbox: load_text_art(store, "weathr/world/mailbox")?,
                pine_tree: load_text_art(store, "weathr/world/pine_tree")?,
            },
            clock_font: ClockFont::from_asset(store.clock_font())?,
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
}

fn load_many(store: &AsciiAssetStore, ids: &[&str]) -> Result<Vec<Vec<String>>, AssetError> {
    ids.iter().map(|id| load_text_art(store, id)).collect()
}

fn load_text_art(store: &AsciiAssetStore, id: &str) -> Result<Vec<String>, AssetError> {
    Ok(store.text_art(id)?.lines().to_vec())
}

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
