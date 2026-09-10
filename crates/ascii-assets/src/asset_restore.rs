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
    // A valid pre-Logs catalog needs only its new entry. Preserve user labels,
    // artwork, comments and extra items rather than resetting the entire catalog.
    let upgraded = if file_key == "home_icons" {
        upgraded_home_icons(&path, file.contents)
    } else {
        None
    };
    let contents = upgraded
        .as_deref()
        .map(str::as_bytes)
        .unwrap_or(file.contents);
    let changed = fs::read(&path)
        .map(|existing| existing != contents)
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
        fs::write(&path, contents).map_err(|source| AssetError::RestoreAsset {
            asset: file_key.to_string(),
            path: path.clone(),
            source,
        })?;
    }

    Ok(AssetRestoreReport { path, changed })
}

fn upgraded_home_icons(path: &Path, embedded: &[u8]) -> Option<String> {
    let existing = fs::read_to_string(path).ok()?;
    let parsed: toml::Value = toml::from_str(&existing).ok()?;
    let items = parsed.get("items")?.as_table()?;
    if items.contains_key("logs")
        || [
            "explorer",
            "launcher",
            "settings",
            "diagnostics",
            "system_status",
            "user_management",
            "user_profile",
            "default",
        ]
        .iter()
        .any(|key| !items.contains_key(*key))
    {
        return None;
    }
    // Only upgrade catalogs whose existing artwork parses correctly.
    let root = path.parent()?.parent()?.parent()?;
    let resolver = crate::AssetResolver::from_unchecked_root(root.to_path_buf());
    crate::artwork::load_art_set(&resolver, DEFAULT_THEME_ID, "home_icons", "home_icons.toml")
        .ok()?;
    let defaults = std::str::from_utf8(embedded).ok()?;
    let entry = defaults.split_once("[items.logs]")?.1;
    Some(format!("{existing}\n[items.logs]{entry}"))
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
