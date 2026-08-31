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
mod tests {
    use super::*;
    use crate::{AssetCheckStatus, check_default_theme, check_required_assets};
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn restores_missing_and_invalid_assets_from_embedded_contents() {
        let root = TempDir::new("restore-default");

        let restored = restore_default_theme_file(root.path(), "weathr/animation/cloud_0")
            .expect("missing asset should be restored");
        assert!(restored.changed);
        assert!(restored.path.is_file());

        let check = check_required_assets(root.path(), DEFAULT_THEME_ID)
            .checks
            .into_iter()
            .find(|check| check.key == "weathr/animation/cloud_0")
            .expect("restored asset check");
        assert_eq!(check.status, AssetCheckStatus::Pass);

        fs::write(&restored.path, b"not the default cloud")
            .expect("invalid asset fixture should be writable");
        let repaired = restore_default_theme_file(root.path(), "weathr/animation/cloud_0")
            .expect("invalid asset should be restored");
        assert!(repaired.changed);

        let unchanged = restore_default_theme_file(root.path(), "weathr/animation/cloud_0")
            .expect("healthy default asset should be accepted");
        assert!(!unchanged.changed);
    }

    #[test]
    fn restores_the_editor_launcher_image_and_marks_it_valid() {
        let root = TempDir::new("image-restore");
        assert!(
            check_default_theme(root.path())
                .checks
                .iter()
                .find(|check| check.key == "launcher_icons/editor.png")
                .is_some_and(|check| check.status == AssetCheckStatus::Warning)
        );

        let restored = restore_default_theme_file(root.path(), "launcher_icons/editor.png")
            .expect("embedded image should be restored");

        assert!(restored.changed);
        assert!(restored.path.is_file());
        assert!(
            check_default_theme(root.path())
                .checks
                .iter()
                .find(|check| check.key == "launcher_icons/editor.png")
                .is_some_and(|check| check.status == AssetCheckStatus::Pass)
        );
    }

    #[test]
    fn restores_the_complete_default_theme_including_images() {
        let root = TempDir::new("restore-all-defaults");

        let restored =
            restore_default_theme(root.path()).expect("default theme should be restored");

        assert_eq!(restored.len(), crate::default_theme_files().len());
        assert!(check_default_theme(root.path()).is_ok());
        assert!(
            root.path()
                .join("themes/default/home_icons/explorer.png")
                .is_file()
        );
        assert!(
            root.path()
                .join("themes/default/home_icons/system_status.png")
                .is_file()
        );
        assert!(
            root.path()
                .join("themes/default/launcher_icons/command_line.png")
                .is_file()
        );
        assert!(
            root.path()
                .join("themes/default/launcher_icons/editor.png")
                .is_file()
        );
    }

    struct TempDir {
        path: PathBuf,
    }

    impl TempDir {
        fn new(case: &str) -> Self {
            let nanos = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system time after Unix epoch")
                .as_nanos();
            let path = std::env::temp_dir().join(format!(
                "tundra-ascii-assets-restore-{}-{nanos}-{case}",
                std::process::id()
            ));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).expect("create temporary asset root");
            Self { path }
        }

        fn path(&self) -> &Path {
            &self.path
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}
