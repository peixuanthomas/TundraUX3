use super::*;
use crate::{AsciiAssetStore, DEFAULT_THEME_ID, check_required_assets, required_assets};
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn derives_profile_dir_from_build_out_dir() {
    let out_dir = Path::new("/repo/target/debug/build/tundra-cli-abc/out");

    let profile_dir = cargo_profile_dir_from_out_dir(out_dir).expect("profile dir");

    assert_eq!(profile_dir, PathBuf::from("/repo/target/debug"));
}

#[test]
fn runtime_asset_copy_includes_valid_icon_assets() {
    let root = TempDir::new("runtime-icon-assets");
    let out_dir = root
        .path
        .join("target/debug/build/tundra-shell-fixture/out");
    fs::create_dir_all(&out_dir).expect("create OUT_DIR fixture");

    let copied_root =
        copy_canonical_assets_to_profile_dir(&out_dir).expect("copy canonical runtime assets");
    assert!(
        copied_root
            .join("themes/default/explorer_icons.toml")
            .is_file()
    );
    assert!(
        copied_root
            .join("themes/default/launcher_icons.toml")
            .is_file()
    );
    assert!(
        copied_root
            .join("themes/default/launcher_icons/command_line.png")
            .is_file()
    );
    assert!(
        copied_root
            .join("themes/default/launcher_icons/editor.png")
            .is_file()
    );
    for icon in [
        "explorer",
        "launcher",
        "settings",
        "diagnostics",
        "system_status",
        "user_management",
        "user_profile",
        "default",
    ] {
        assert!(
            copied_root
                .join(format!("themes/default/home_icons/{icon}.png"))
                .is_file(),
            "copied runtime assets should include Home icon {icon}"
        );
    }
    let store = AsciiAssetStore::load_with_root(&copied_root, DEFAULT_THEME_ID)
        .expect("copied runtime assets should be self-contained");
    let report = check_required_assets(&copied_root, DEFAULT_THEME_ID);
    assert_eq!(report.checks.len(), required_assets().len());
    assert!(report.is_ok(), "{:?}", report.warning_messages());
    assert_eq!(
        store.explorer_icon("folder").expect("folder icon").width(),
        3
    );
    assert_eq!(
        store
            .explorer_icon("refresh")
            .expect("refresh icon")
            .width(),
        1
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
            "tundra-ascii-assets-copy-{}-{nanos}-{case}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).expect("create temp root");
        Self { path }
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}
