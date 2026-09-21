use std::env;
use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

use super::*;
use crate::asset_manifest::{CANONICAL_ASSETS_DIR, DEFAULT_THEME_ID};

#[test]
fn check_required_assets_warns_for_missing_root_contents() {
    let temp = TempDir::new("missing-assets");
    fs::create_dir_all(temp.path().join("themes/default")).expect("temp root");

    let report = check_required_assets(temp.path(), DEFAULT_THEME_ID);

    assert!(report.has_warnings());
    assert!(
        report
            .warning_messages()
            .iter()
            .any(|message| message.contains("missing ASCII asset"))
    );
}

#[test]
fn check_required_assets_applies_explorer_icon_dimension_validation() {
    let temp = TempDir::new("invalid-explorer-icons");
    let theme = temp.path().join("themes/default");
    fs::create_dir_all(&theme).expect("temp theme root");
    let canonical = Path::new(CANONICAL_ASSETS_DIR).join("themes/default/explorer_icons.toml");
    let source = fs::read_to_string(canonical).expect("canonical Explorer icons");
    let invalid = source.replacen("lines = [\"[+]\"]", "lines = [\"[]\"]", 1);
    fs::write(theme.join("explorer_icons.toml"), invalid).expect("invalid icon fixture");

    let report = check_required_assets(temp.path(), DEFAULT_THEME_ID);
    let check = report
        .checks
        .iter()
        .find(|check| check.key == "explorer_icons")
        .expect("Explorer icon check");

    assert!(check.is_invalid());
    assert!(check.message.contains("folder must be exactly 3x1"));
}

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(name: &str) -> Self {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time")
            .as_nanos();
        let path = env::temp_dir().join(format!(
            "tundra-ascii-assets-{}-{nanos}-{name}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).expect("temp dir");
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
