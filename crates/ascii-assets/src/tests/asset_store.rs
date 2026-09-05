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
    let system_status = store
        .home_icon_catalog()
        .icon("system_status")
        .expect("System Status Home icon by key");
    assert_eq!(system_status.label(), Some("System Status"));
    assert_eq!(system_status.lines().len(), 4);
    assert_eq!(
        system_status.image_path(),
        Some("home_icons/system_status.png")
    );
    assert_eq!(
        store
            .home_icon_catalog()
            .icon_for_label("System Status")
            .map(ArtItem::key),
        Some("system_status")
    );
    assert!(
        store
            .home_icon_image_path("system_status")
            .is_some_and(|path| path.is_file())
    );
    assert!(store.home_icon_image_bytes("system_status").is_some());
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
    let editor = store
        .launcher_icon("builtin.editor")
        .expect("Editor Launcher icon");
    assert_eq!(editor.label(), Some("Editor"));
    assert_eq!(editor.image_path(), Some("launcher_icons/editor.png"));
    assert!(
        store
            .launcher_icon_image_path("builtin.editor")
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
fn launcher_icon_loading_rejects_a_missing_declared_image() {
    let root = TemporaryAssetRoot::copy_of(Path::new(CANONICAL_ASSETS_DIR));
    fs::remove_file(
        root.path
            .join("themes/default/launcher_icons/command_line.png"),
    )
    .expect("remove generated Launcher icon");

    let error = AsciiAssetStore::load_with_root(&root.path, DEFAULT_THEME_ID)
        .expect_err("a declared image is part of the theme and must load");
    assert!(matches!(error, AssetError::MissingAsset { .. }));
}

#[test]
fn home_icon_loading_rejects_a_missing_declared_image() {
    let root = TemporaryAssetRoot::copy_of(Path::new(CANONICAL_ASSETS_DIR));
    fs::remove_file(root.path.join("themes/default/home_icons/explorer.png"))
        .expect("remove generated Home icon");

    let error = AsciiAssetStore::load_with_root(&root.path, DEFAULT_THEME_ID)
        .expect_err("a declared image is part of the theme and must load");
    assert!(matches!(error, AssetError::MissingAsset { .. }));
}

#[test]
fn explorer_icon_loading_rejects_missing_icons_without_hardcoded_fallbacks() {
    let root = TemporaryAssetRoot::copy_of(Path::new(CANONICAL_ASSETS_DIR));
    let icon_path = root.path.join("themes/default/explorer_icons.toml");
    let source = fs::read_to_string(&icon_path).expect("read Explorer icons");
    // Exercise the Windows checkout form even when this test runs on Unix.
    let mut source = source.lines().collect::<Vec<_>>().join("\r\n");
    let cancel_section = source
        .find("[items.cancel]")
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
