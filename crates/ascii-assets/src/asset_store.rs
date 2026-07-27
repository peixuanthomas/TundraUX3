use std::collections::BTreeMap;
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
        let path = self.resolver.asset_path(&self.theme_id, relative_path);
        path.is_file().then_some(path)
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
        let path = self.resolver.asset_path(&self.theme_id, relative_path);
        path.is_file().then_some(path)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::artwork::{EXPLORER_ACTION_ICON_KEYS, EXPLORER_ENTRY_AND_LOCATION_ICON_KEYS};
    use crate::asset_manifest::CANONICAL_ASSETS_DIR;
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_TEMP_ROOT_ID: AtomicU64 = AtomicU64::new(0);

    #[test]
    fn default_store_loads_canonical_assets() {
        let store = AsciiAssetStore::load_with_root(CANONICAL_ASSETS_DIR, DEFAULT_THEME_ID)
            .expect("canonical assets should load");

        assert_eq!(store.banner_lines("tundraux3").unwrap().len(), 10);
        let explorer = store
            .home_icon_catalog()
            .icon("explorer")
            .expect("Explorer Home icon");
        assert_eq!(explorer.image_path(), Some("home_icons/explorer.png"));
        assert!(
            store
                .home_icon_image_path("explorer")
                .is_some_and(|path| path.is_file())
        );
        let command_line = store
            .launcher_icon("builtin.command-line")
            .expect("Command Line Launcher icon");
        assert_eq!(command_line.label(), Some("Command Line"));
        assert_eq!(
            command_line.image_path(),
            Some("launcher_icons/command_line.png")
        );
        assert!(
            store
                .launcher_icon_image_path("builtin.command-line")
                .is_some_and(|path| path.is_file())
        );
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
    fn default_explorer_icons_are_complete_and_dimensioned_by_role() {
        let store = AsciiAssetStore::load_with_root(CANONICAL_ASSETS_DIR, DEFAULT_THEME_ID)
            .expect("canonical assets should load");

        for key in EXPLORER_ENTRY_AND_LOCATION_ICON_KEYS {
            let icon = store
                .explorer_icon(key)
                .unwrap_or_else(|error| panic!("missing Explorer icon {key}: {error}"));
            assert_eq!((icon.width(), icon.height()), (3, 1), "icon {key}");
        }
        for key in EXPLORER_ACTION_ICON_KEYS {
            let icon = store
                .explorer_icon(key)
                .unwrap_or_else(|error| panic!("missing Explorer icon {key}: {error}"));
            assert_eq!((icon.width(), icon.height()), (1, 1), "icon {key}");
        }
        assert_eq!(
            store.explorer_icons().count(),
            EXPLORER_ENTRY_AND_LOCATION_ICON_KEYS.len() + EXPLORER_ACTION_ICON_KEYS.len()
        );
        assert!(matches!(
            store.explorer_icon("not-defined"),
            Err(AssetError::UnknownAsset { .. })
        ));
    }

    #[test]
    fn launcher_icon_loading_keeps_ascii_fallback_when_declared_image_is_missing() {
        let root = TemporaryAssetRoot::copy_of(Path::new(CANONICAL_ASSETS_DIR));
        fs::remove_file(
            root.path
                .join("themes/default/launcher_icons/command_line.png"),
        )
        .expect("remove generated Launcher icon");

        let store = AsciiAssetStore::load_with_root(&root.path, DEFAULT_THEME_ID)
            .expect("missing optional image must preserve ASCII asset loading");
        let icon = store
            .launcher_icon("builtin.command-line")
            .expect("Command Line ASCII icon");
        assert!(!icon.lines().is_empty());
        assert_eq!(icon.image_path(), Some("launcher_icons/command_line.png"));
        assert_eq!(store.launcher_icon_image_path("builtin.command-line"), None);
    }

    #[test]
    fn home_icon_loading_keeps_ascii_fallback_when_declared_image_is_missing() {
        let root = TemporaryAssetRoot::copy_of(Path::new(CANONICAL_ASSETS_DIR));
        fs::remove_file(root.path.join("themes/default/home_icons/explorer.png"))
            .expect("remove generated Home icon");

        let store = AsciiAssetStore::load_with_root(&root.path, DEFAULT_THEME_ID)
            .expect("missing optional image must preserve ASCII asset loading");
        let icon = store
            .home_icon_catalog()
            .icon_for_key("explorer")
            .expect("Explorer ASCII icon");
        assert!(!icon.lines().is_empty());
        assert_eq!(icon.image_path(), Some("home_icons/explorer.png"));
        assert_eq!(store.home_icon_image_path("explorer"), None);
    }

    #[test]
    fn explorer_icon_loading_rejects_missing_icons_without_hardcoded_fallbacks() {
        let root = TemporaryAssetRoot::copy_of(Path::new(CANONICAL_ASSETS_DIR));
        let icon_path = root.path.join("themes/default/explorer_icons.toml");
        let mut source = fs::read_to_string(&icon_path).expect("read Explorer icons");
        let cancel_section = source
            .find("\n[items.cancel]")
            .expect("canonical cancel icon section");
        source.truncate(cancel_section);
        fs::write(&icon_path, source).expect("remove required icon from fixture");

        let error = AsciiAssetStore::load_with_root(&root.path, DEFAULT_THEME_ID)
            .expect_err("missing icon must fail startup asset loading");
        assert!(
            error
                .to_string()
                .contains("missing required Explorer action icon cancel"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn explorer_icon_loading_rejects_invalid_role_dimensions() {
        let root = TemporaryAssetRoot::copy_of(Path::new(CANONICAL_ASSETS_DIR));
        let icon_path = root.path.join("themes/default/explorer_icons.toml");
        let source = fs::read_to_string(&icon_path).expect("read Explorer icons");
        let invalid = source.replacen("lines = [\"[+]\"]", "lines = [\"[]\"]", 1);
        assert_ne!(invalid, source, "folder fixture should be replaced");
        fs::write(&icon_path, invalid).expect("write invalid icon fixture");

        let error = AsciiAssetStore::load_with_root(&root.path, DEFAULT_THEME_ID)
            .expect_err("wrong-sized icon must fail startup asset loading");
        assert!(error.to_string().contains("folder must be exactly 3x1"));
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
