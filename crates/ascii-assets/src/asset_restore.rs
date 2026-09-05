use std::fs;
use std::path::{Path, PathBuf};

use crate::asset_error::AssetError;
use crate::asset_manifest::DEFAULT_THEME_ID;
use crate::asset_validation::{AssetCheckStatus, check_default_theme};
use crate::embedded_defaults::embedded_default_theme_file;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssetRestoreReport {
    pub path: PathBuf,
    pub changed: bool,
}

/// Restores one default-theme file from the contents embedded in the binary.
pub fn restore_default_theme_file(
    root: &Path,
    file_key: &str,
) -> Result<AssetRestoreReport, AssetError> {
    let file = embedded_default_theme_file(file_key).ok_or_else(|| AssetError::UnknownAsset {
        asset: file_key.to_string(),
    })?;
    let path = root
        .join("themes")
        .join(DEFAULT_THEME_ID)
        .join(file.relative_path);
    let changed = fs::read(&path)
        .map(|contents| contents != file.contents)
        .unwrap_or(true);

    if changed {
        let parent = path
            .parent()
            .expect("embedded default theme file paths always have a parent");
        fs::create_dir_all(parent).map_err(|source| AssetError::RestoreAsset {
            asset: file_key.to_string(),
            path: path.clone(),
            source,
        })?;
        fs::write(&path, file.contents).map_err(|source| AssetError::RestoreAsset {
            asset: file_key.to_string(),
            path: path.clone(),
            source,
        })?;
    }

    Ok(AssetRestoreReport { path, changed })
}

/// Restores every missing, unreadable, or invalid file in the default theme,
/// including raster images. Healthy files are preserved.
pub fn restore_default_theme(root: &Path) -> Result<Vec<AssetRestoreReport>, AssetError> {
    check_default_theme(root)
        .checks
        .into_iter()
        .filter(|check| check.status == AssetCheckStatus::Warning)
        .map(|check| restore_default_theme_file(root, &check.key))
        .collect()
}

#[cfg(test)]
#[path = "tests/asset_restore.rs"]
mod tests;
