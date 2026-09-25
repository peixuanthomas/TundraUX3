pub(crate) mod home_icons;
pub(crate) mod setup_catalog;

/// Text icon embedded so shell navigation also works without terminal image support.
pub(crate) const BACK_ICON: &str = include_str!("../../assets/icons/back.txt");

pub use home_icons::{
    AsciiAssetStore, AssetDimensions, AssetError, ClockFontAsset, DEFAULT_THEME_ID,
    DefaultThemeCheckReport, DefaultThemeFile, ExplorerIcon, HomeIcon, HomeIconCatalog,
    RuntimeAsciiAssets, asset_root_for_recovery_from_env_or_current_exe, check_default_theme,
    default_theme_files, home_icon_for_label, restore_default_theme, try_home_icon_for_label,
};
pub use setup_catalog::{
    SetupColorOption, setup_language_options, setup_standard_color_options, setup_timezone_options,
};
