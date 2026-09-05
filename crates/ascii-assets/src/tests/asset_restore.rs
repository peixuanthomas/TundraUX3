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

    let restored = restore_default_theme(root.path()).expect("default theme should be restored");

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
