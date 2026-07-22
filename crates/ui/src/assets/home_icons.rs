use std::fmt;
use std::sync::Arc;

pub use ascii_assets::{
    AsciiAssetStore, AssetDimensions, AssetError, ClockFontAsset, ExplorerIcon, HomeIcon,
    HomeIconCatalog,
};

#[derive(Clone)]
pub struct RuntimeAsciiAssets {
    store: Arc<AsciiAssetStore>,
}

impl RuntimeAsciiAssets {
    pub fn load_default() -> Result<Self, AssetError> {
        Ok(Self::from_store(AsciiAssetStore::load_default()?))
    }

    pub fn load_theme(theme_id: &str) -> Result<Self, AssetError> {
        Ok(Self::from_store(AsciiAssetStore::load_theme(theme_id)?))
    }

    pub fn from_store(store: AsciiAssetStore) -> Self {
        Self {
            store: Arc::new(store),
        }
    }

    pub fn from_shared_store(store: Arc<AsciiAssetStore>) -> Self {
        Self { store }
    }

    pub fn store(&self) -> &AsciiAssetStore {
        &self.store
    }

    pub fn banner_lines(&self, key: &str) -> Result<&[String], AssetError> {
        self.store.banner_lines(key)
    }

    pub fn home_icon_catalog(&self) -> &HomeIconCatalog {
        self.store.home_icon_catalog()
    }

    pub fn explorer_icon(&self, key: &str) -> Result<&ExplorerIcon, AssetError> {
        self.store.explorer_icon(key)
    }

    pub fn clock_font(&self) -> &ClockFontAsset {
        self.store.clock_font()
    }

    pub fn max_asset_dimensions(&self) -> AssetDimensions {
        self.store.max_asset_dimensions()
    }

    pub fn home_icon_for_label(&self, label: &str) -> Option<&HomeIcon> {
        let catalog = self.home_icon_catalog();
        catalog
            .icon_for_label(label)
            .or_else(|| catalog.icon_for_key(label))
            .or_else(|| catalog.icon_for_key("default"))
    }
}

impl fmt::Debug for RuntimeAsciiAssets {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RuntimeAsciiAssets")
            .finish_non_exhaustive()
    }
}

impl PartialEq for RuntimeAsciiAssets {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.store, &other.store)
    }
}

impl Eq for RuntimeAsciiAssets {}

pub fn try_home_icon_for_label(label: &str) -> Result<HomeIcon, AssetError> {
    let assets = RuntimeAsciiAssets::load_default()?;
    Ok(assets
        .home_icon_for_label(label)
        .cloned()
        .expect("home icon assets must define a default icon"))
}

pub fn home_icon_for_label(label: &str) -> HomeIcon {
    try_home_icon_for_label(label).expect("default ASCII home icon assets must load")
}
