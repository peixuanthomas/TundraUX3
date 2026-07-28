mod artwork;
mod asset_distribution;
mod asset_error;
mod asset_manifest;
mod asset_resolver;
mod asset_restore;
mod asset_store;
mod asset_validation;
mod clock_font;
mod embedded_defaults;

pub use artwork::{
    ArtItem, ArtSet, ExplorerIcon, HomeIcon, HomeIconCatalog, LauncherIcon, TextArt,
};
pub use asset_distribution::{
    cargo_profile_dir_from_out_dir, copy_canonical_assets_to_profile_dir,
};
pub use asset_error::AssetError;
pub use asset_manifest::{
    AssetKind, CANONICAL_ASSETS_DIR, DEFAULT_THEME_ID, ENV_ASSETS_DIR, RequiredAsset,
    required_assets,
};
pub use asset_resolver::{
    AssetResolver, asset_root_for_recovery_from_env_or_current_exe,
    asset_root_from_env_or_current_exe,
};
pub use asset_restore::{AssetRestoreReport, restore_default_theme, restore_default_theme_file};
pub use asset_store::{AsciiAssetStore, AssetDimensions};
pub use asset_validation::{
    AssetCheck, AssetCheckReport, AssetCheckStatus, DefaultThemeCheck, DefaultThemeCheckReport,
    check_default_theme, check_required_assets,
};
pub use clock_font::ClockFontAsset;
pub use embedded_defaults::{DefaultThemeFile, default_theme_files};
