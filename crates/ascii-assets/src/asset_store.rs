use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use crate::artwork::{
    ArtItem, ArtSet, ExplorerIcon, HomeIconCatalog, LauncherIcon, TextArt, load_art_set,
    load_explorer_icons, load_home_icon_catalog, load_launcher_icons, load_text_art,
};
use crate::asset_error::AssetError;
use crate::asset_manifest::{DEFAULT_THEME_ID, REQUIRED_TEXT_ARTS};
use crate::asset_resolver::AssetResolver;
use crate::clock_font::{ClockFontAsset, load_clock_font};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AssetDimensions {
    pub width: usize,
    pub height: usize,
}

impl AssetDimensions {
    fn include(&mut self, width: usize, height: usize) {
        self.width = self.width.max(width);
        self.height = self.height.max(height);
    }
}

#[derive(Debug, Clone)]
pub struct AsciiAssetStore {
    resolver: AssetResolver,
    theme_id: String,
    banners: ArtSet,
    explorer_icons: ArtSet,
    home_icons: HomeIconCatalog,
    launcher_icons: ArtSet,
    image_assets: BTreeMap<String, Vec<u8>>,
    clock_font: ClockFontAsset,
    text_arts: BTreeMap<String, TextArt>,
}

impl AsciiAssetStore {
    pub fn load_default() -> Result<Self, AssetError> {
        Self::load_theme(DEFAULT_THEME_ID)
    }

    pub fn load_theme(theme_id: &str) -> Result<Self, AssetError> {
        Self::load_with_resolver(AssetResolver::from_env_or_current_exe()?, theme_id)
    }

    pub fn load_with_root(root: impl Into<PathBuf>, theme_id: &str) -> Result<Self, AssetError> {
        Self::load_with_resolver(AssetResolver::new(root.into())?, theme_id)
    }

    pub fn load_with_resolver(resolver: AssetResolver, theme_id: &str) -> Result<Self, AssetError> {
        let banners = load_art_set(&resolver, theme_id, "banner", "banner.toml")?;
        let explorer_icons = load_explorer_icons(&resolver, theme_id)?;
        let home_icons = load_home_icon_catalog(&resolver, theme_id)?;
        let launcher_icons = load_launcher_icons(&resolver, theme_id)?;
        let image_assets = load_image_assets(&resolver, theme_id, &home_icons, &launcher_icons)?;
        let clock_font = load_clock_font(&resolver, theme_id)?;
        let mut text_arts = BTreeMap::new();
        for (key, relative_path) in REQUIRED_TEXT_ARTS {
            let art = load_text_art(&resolver, theme_id, key, relative_path)?;
            text_arts.insert((*key).to_string(), art);
        }

        Ok(Self {
            resolver,
            theme_id: theme_id.to_string(),
            banners,
            explorer_icons,
            home_icons,
            launcher_icons,
            image_assets,
            clock_font,
            text_arts,
        })
    }

    pub fn reload(&mut self) -> Result<(), AssetError> {
        *self = Self::load_with_resolver(self.resolver.clone(), &self.theme_id)?;
        Ok(())
    }

    pub fn root(&self) -> &Path {
        self.resolver.root()
    }

    pub fn theme_id(&self) -> &str {
        &self.theme_id
    }

    pub fn banner_lines(&self, key: &str) -> Result<&[String], AssetError> {
        self.banners
            .get(key)
            .map(ArtItem::lines)
            .ok_or_else(|| AssetError::UnknownAsset {
                asset: format!("banner/{key}"),
            })
    }

    pub fn home_icon_catalog(&self) -> &HomeIconCatalog {
        &self.home_icons
    }

    pub fn home_icon_image_path(&self, key: &str) -> Option<PathBuf> {
        let relative_path = self.home_icons.icon_for_key(key)?.image_path()?;
        self.image_assets
            .contains_key(relative_path)
            .then(|| self.resolver.asset_path(&self.theme_id, relative_path))
    }

    pub fn home_icon_image_bytes(&self, key: &str) -> Option<&[u8]> {
        let relative_path = self.home_icons.icon_for_key(key)?.image_path()?;
        self.image_assets.get(relative_path).map(Vec::as_slice)
    }

    pub fn explorer_icon(&self, key: &str) -> Result<&ExplorerIcon, AssetError> {
        self.explorer_icons
            .get(key)
            .ok_or_else(|| AssetError::UnknownAsset {
                asset: format!("explorer_icons/{key}"),
            })
    }

    pub fn explorer_icons(&self) -> impl Iterator<Item = &ExplorerIcon> {
        self.explorer_icons.items()
    }

    pub fn launcher_icon(&self, key: &str) -> Option<&LauncherIcon> {
        self.launcher_icons.get(key)
    }

    pub fn launcher_icon_image_path(&self, key: &str) -> Option<PathBuf> {
        let relative_path = self.launcher_icon(key)?.image_path()?;
        self.image_assets
            .contains_key(relative_path)
            .then(|| self.resolver.asset_path(&self.theme_id, relative_path))
    }

    pub fn launcher_icon_image_bytes(&self, key: &str) -> Option<&[u8]> {
        let relative_path = self.launcher_icon(key)?.image_path()?;
        self.image_assets.get(relative_path).map(Vec::as_slice)
    }

    pub fn clock_font(&self) -> &ClockFontAsset {
        &self.clock_font
    }

    pub fn text_art(&self, key: &str) -> Result<&TextArt, AssetError> {
        self.text_arts
            .get(key)
            .ok_or_else(|| AssetError::UnknownAsset {
                asset: key.to_string(),
            })
    }

    pub fn max_asset_dimensions(&self) -> AssetDimensions {
        let mut dimensions = AssetDimensions::default();

        for item in self
            .banners
            .items()
            .chain(self.home_icons.icons())
            .chain(self.launcher_icons.items())
            .chain(self.explorer_icons.items())
        {
            dimensions.include(item.width(), item.height());
        }
        for art in self.text_arts.values() {
            dimensions.include(art.width(), art.height());
        }
        dimensions.include(
            self.clock_font.max_rendered_clock_width(),
            self.clock_font.height,
        );

        dimensions
    }
}

fn load_image_assets(
    resolver: &AssetResolver,
    theme_id: &str,
    home_icons: &HomeIconCatalog,
    launcher_icons: &ArtSet,
) -> Result<BTreeMap<String, Vec<u8>>, AssetError> {
    let relative_paths = home_icons
        .icons()
        .chain(launcher_icons.items())
        .filter_map(ArtItem::image_path)
        .map(str::to_string)
        .collect::<BTreeSet<_>>();
    let mut images = BTreeMap::new();

    for relative_path in relative_paths {
        let path = resolver.asset_path(theme_id, &relative_path);
        let bytes = match fs::read(&path) {
            Ok(bytes) => bytes,
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => {
                return Err(AssetError::MissingAsset {
                    asset: relative_path,
                    path,
                });
            }
            Err(source) => {
                return Err(AssetError::ReadAsset {
                    asset: relative_path,
                    path,
                    source,
                });
            }
        };
        images.insert(relative_path, bytes);
    }

    Ok(images)
}

#[cfg(test)]
#[path = "tests/asset_store.rs"]
mod tests;
