use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::artwork::{
    ArtItem, ArtSet, HomeIconCatalog, TextArt, load_art_set, load_home_icon_catalog, load_text_art,
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
    home_icons: HomeIconCatalog,
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
        let home_icons = load_home_icon_catalog(&resolver, theme_id)?;
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
            home_icons,
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

        for item in self.banners.items().chain(self.home_icons.icons()) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::asset_manifest::CANONICAL_ASSETS_DIR;
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_TEMP_ROOT_ID: AtomicU64 = AtomicU64::new(0);

    #[test]
    fn default_store_loads_canonical_assets() {
        let store = AsciiAssetStore::load_with_root(CANONICAL_ASSETS_DIR, DEFAULT_THEME_ID)
            .expect("canonical assets should load");

        assert_eq!(store.banner_lines("tundraux3").unwrap().len(), 10);
        assert!(store.home_icon_catalog().icon("explorer").is_some());
        assert_eq!(store.clock_font().height, 7);
        assert!(store.text_art("weathr/world/house").unwrap().height() >= 10);
        assert_eq!(
            store.max_asset_dimensions(),
            AssetDimensions {
                width: 108,
                height: 10,
            }
        );
    }

    #[test]
    fn default_digit_glyphs_follow_the_clock_font_shape() {
        let store = AsciiAssetStore::load_with_root(CANONICAL_ASSETS_DIR, DEFAULT_THEME_ID)
            .expect("canonical assets should load");
        let font = store.clock_font();

        for digit in '0'..='9' {
            let rows = font
                .glyphs
                .get(&digit)
                .unwrap_or_else(|| panic!("default clock font should contain {digit}"));
            let width = rows
                .first()
                .map(|row| row.chars().count())
                .unwrap_or_default();

            assert_eq!(
                rows.len(),
                font.height,
                "digit {digit} should use the font's declared height"
            );
            assert!(width > 0, "digit {digit} should not be empty");
            assert!(
                rows.iter().all(|row| row.chars().count() == width),
                "digit {digit} should be rectangular"
            );
        }
    }

    #[test]
    fn max_asset_dimensions_follow_larger_runtime_theme_assets() {
        let root = TemporaryAssetRoot::copy_of(Path::new(CANONICAL_ASSETS_DIR));
        let width = 137;
        let height = 23;
        let body = (0..height)
            .map(|_| "X".repeat(width))
            .collect::<Vec<_>>()
            .join("\n");
        let banner = format!(
            "schema_version = 1\nname = \"test-banners\"\n\n\
             [items.tundraux3]\nlabel = \"TundraUX3\"\nbody = '''\n{body}\n'''\n"
        );
        fs::write(
            root.path.join("themes/default/banner.toml"),
            banner.as_bytes(),
        )
        .expect("write oversized test banner");

        let store = AsciiAssetStore::load_with_root(&root.path, DEFAULT_THEME_ID)
            .expect("custom theme should load");

        assert_eq!(
            store.max_asset_dimensions(),
            AssetDimensions { width, height }
        );
    }

    struct TemporaryAssetRoot {
        path: PathBuf,
    }

    impl TemporaryAssetRoot {
        fn copy_of(source: &Path) -> Self {
            let id = NEXT_TEMP_ROOT_ID.fetch_add(1, Ordering::Relaxed);
            let timestamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system clock should follow Unix epoch")
                .as_nanos();
            let path = std::env::temp_dir().join(format!(
                "tundra-ascii-assets-{}-{timestamp}-{id}",
                std::process::id()
            ));
            copy_directory(source, &path);
            Self { path }
        }
    }

    impl Drop for TemporaryAssetRoot {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    fn copy_directory(source: &Path, destination: &Path) {
        fs::create_dir_all(destination).expect("create temporary asset directory");
        for entry in fs::read_dir(source).expect("read canonical asset directory") {
            let entry = entry.expect("read canonical asset entry");
            let target = destination.join(entry.file_name());
            if entry.file_type().expect("read asset entry type").is_dir() {
                copy_directory(&entry.path(), &target);
            } else {
                fs::copy(entry.path(), target).expect("copy canonical asset file");
            }
        }
    }
}
