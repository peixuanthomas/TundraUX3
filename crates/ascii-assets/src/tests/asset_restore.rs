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

#[test]
fn restores_missing_or_damaged_logs_image_and_keeps_healthy_files() {
    let root = TempDir::new("logs-image");
    restore_default_theme(root.path()).unwrap();
    let image = root.path().join("themes/default/home_icons/logs.png");
    fs::remove_file(&image).unwrap();
    let restored = restore_default_theme(root.path()).unwrap();
    assert_eq!(restored.len(), 1);
    assert_eq!(restored[0].path, image);
    fs::write(&image, b"broken png").unwrap();
    assert_eq!(restore_default_theme(root.path()).unwrap().len(), 1);
    assert!(restore_default_theme(root.path()).unwrap().is_empty());
    assert!(check_default_theme(root.path()).is_ok());
    let bytes = fs::read(image).unwrap();
    assert_eq!(&bytes[0..8], b"\x89PNG\r\n\x1a\n");
    assert_eq!(u32::from_be_bytes(bytes[16..20].try_into().unwrap()), 256);
    assert_eq!(u32::from_be_bytes(bytes[20..24].try_into().unwrap()), 256);
}

#[test]
fn upgrades_old_home_catalog_without_replacing_customized_items() {
    let root = TempDir::new("logs-upgrade");
    restore_default_theme(root.path()).unwrap();
    let catalog = root.path().join("themes/default/home_icons.toml");
    let original = fs::read_to_string(&catalog).unwrap();
    let old = original
        .split("[items.logs]")
        .next()
        .unwrap()
        .replace("label = \"Explorer\"", "label = \"My Explorer\"");
    fs::write(&catalog, &old).unwrap();
    assert!(
        check_default_theme(root.path())
            .warning_checks()
            .iter()
            .any(|check| check.key == "home_icons")
    );
    let restored = restore_default_theme(root.path()).unwrap();
    assert_eq!(restored.len(), 1);
    let updated = fs::read_to_string(&catalog).unwrap();
    assert!(updated.starts_with(&old));
    assert!(updated.contains("[items.logs]"));
    assert!(updated.contains("label = \"My Explorer\""));
    assert!(check_default_theme(root.path()).is_ok());
    assert!(restore_default_theme(root.path()).unwrap().is_empty());
}

#[test]
fn logs_restore_preserves_unrelated_custom_theme_files() {
    let root = TempDir::new("logs-custom-theme");
    let custom = root.path().join("themes/custom");
    fs::create_dir_all(&custom).unwrap();
    fs::write(custom.join("home_icons.toml"), b"custom theme").unwrap();
    restore_default_theme(root.path()).unwrap();
    assert_eq!(
        fs::read(custom.join("home_icons.toml")).unwrap(),
        b"custom theme"
    );
}

#[test]
fn automatic_recovery_creates_a_missing_root_and_is_idempotent() {
    let parent = TempDir::new("automatic-missing-root");
    let root = parent.path().join("missing/assets");
    let (store, report) = AsciiAssetStore::load_default_with_root_and_recovery(&root).unwrap();
    assert_eq!(store.root(), root);
    assert_eq!(report.root, root);
    assert_eq!(report.repaired.len(), crate::default_theme_files().len());
    assert!(report.fallback.is_empty());
    assert!(
        report
            .repaired
            .iter()
            .all(|file| !file.issue.is_empty() && file.repair_error.is_none())
    );
    assert!(check_default_theme(&root).is_ok());
    let (_, second) = AsciiAssetStore::load_default_with_root_and_recovery(&root).unwrap();
    assert!(second.repaired.is_empty());
    assert!(second.fallback.is_empty());
}

#[test]
fn automatic_recovery_repairs_corrupt_defaults_and_preserves_custom_content() {
    let root = TempDir::new("automatic-custom-content");
    restore_default_theme(root.path()).unwrap();
    let theme = root.path().join("themes/default");
    let home_path = theme.join("home_icons.toml");
    let custom_home = fs::read_to_string(&home_path)
        .unwrap()
        .replace("label = \"Explorer\"", "label = \"My Explorer\"")
        .replace("home_icons/explorer.png", "home_icons/custom.png");
    fs::write(&home_path, &custom_home).unwrap();
    let custom_image = fs::read(theme.join("home_icons/settings.png")).unwrap();
    fs::write(theme.join("home_icons/custom.png"), &custom_image).unwrap();
    let custom_banner = "schema_version = 1\n[items.tundraux3]\nlines = [\"CUSTOM\"]\n";
    fs::write(theme.join("banner.toml"), custom_banner).unwrap();
    fs::write(theme.join("weathr/world/house.txt"), "CUSTOM HOUSE\n").unwrap();

    let corrupt = [
        ("explorer_icons", "explorer_icons.toml"),
        ("launcher_icons", "launcher_icons.toml"),
        ("weathr/render/clock_font", "weathr/render/clock_font.toml"),
        ("weathr/animation/cloud_0", "weathr/animation/cloud_0.txt"),
        ("home_icons/logs.png", "home_icons/logs.png"),
    ];
    for (_, relative_path) in corrupt {
        fs::write(theme.join(relative_path), b"\xff\x00broken").unwrap();
    }
    let (store, report) =
        AsciiAssetStore::load_default_with_root_and_recovery(root.path()).unwrap();
    assert_eq!(report.repaired.len(), corrupt.len());
    assert!(report.fallback.is_empty());
    for (key, _) in corrupt {
        assert!(report.repaired.iter().any(|file| file.key == key));
    }
    assert_eq!(fs::read_to_string(home_path).unwrap(), custom_home);
    assert_eq!(
        fs::read_to_string(theme.join("banner.toml")).unwrap(),
        custom_banner
    );
    assert_eq!(store.banner_lines("tundraux3").unwrap(), &["CUSTOM"]);
    assert_eq!(
        store.text_art("weathr/world/house").unwrap().lines(),
        &["CUSTOM HOUSE"]
    );
    assert_eq!(
        store.home_icon_image_bytes("explorer"),
        Some(custom_image.as_slice())
    );
    assert_eq!(
        store.home_icon_catalog().icon("explorer").unwrap().label(),
        Some("My Explorer")
    );
}

#[test]
fn automatic_recovery_uses_memory_when_root_is_a_file_without_creating_asset_directories() {
    let parent = TempDir::new("automatic-blocked-root");
    let root = parent.path().join("assets");
    fs::write(&root, b"keep root obstruction").unwrap();
    let (mut store, report) = AsciiAssetStore::load_default_with_root_and_recovery(&root).unwrap();
    assert!(report.repaired.is_empty());
    assert_eq!(report.fallback.len(), crate::default_theme_files().len());
    assert!(
        report
            .fallback
            .iter()
            .all(|file| !file.issue.is_empty() && file.repair_error.is_some())
    );
    assert_eq!(store.root(), root);
    assert_eq!(store.theme_id(), DEFAULT_THEME_ID);
    assert_eq!(fs::read(&root).unwrap(), b"keep root obstruction");
    assert_eq!(fs::read_dir(parent.path()).unwrap().count(), 1);

    let expected =
        AsciiAssetStore::load_with_root(crate::CANONICAL_ASSETS_DIR, DEFAULT_THEME_ID).unwrap();
    assert_eq!(
        store.banner_lines("tundraux3").unwrap(),
        expected.banner_lines("tundraux3").unwrap()
    );
    assert_eq!(store.home_icon_catalog(), expected.home_icon_catalog());
    assert_eq!(store.clock_font(), expected.clock_font());
    assert_eq!(
        store.max_asset_dimensions(),
        expected.max_asset_dimensions()
    );
    for (key, _) in crate::asset_manifest::REQUIRED_TEXT_ARTS {
        assert_eq!(
            store.text_art(key).unwrap(),
            expected.text_art(key).unwrap()
        );
    }
    for icon in expected.explorer_icons() {
        assert_eq!(store.explorer_icon(icon.key()).unwrap(), icon);
    }
    for icon in expected.home_icon_catalog().icons() {
        assert_eq!(
            store.home_icon_image_bytes(icon.key()),
            expected.home_icon_image_bytes(icon.key())
        );
        assert_eq!(
            store.home_icon_image_path(icon.key()),
            icon.image_path()
                .map(|path| root.join("themes/default").join(path))
        );
    }
    for key in ["builtin.command-line", "builtin.editor"] {
        assert_eq!(store.launcher_icon(key), expected.launcher_icon(key));
        assert_eq!(
            store.launcher_icon_image_bytes(key),
            expected.launcher_icon_image_bytes(key)
        );
        assert_eq!(
            store.launcher_icon_image_path(key),
            expected
                .launcher_icon(key)
                .unwrap()
                .image_path()
                .map(|path| root.join("themes/default").join(path))
        );
    }
    store
        .reload()
        .expect("reload retains embedded fallback bytes");
    assert_eq!(store.clock_font(), expected.clock_font());
}

#[test]
fn automatic_recovery_continues_after_individual_failures_and_cleans_staging_files() {
    let root = TempDir::new("automatic-mixed-recovery");
    restore_default_theme(root.path()).unwrap();
    let theme = root.path().join("themes/default");
    for relative in ["banner.toml", "home_icons/explorer.png"] {
        let path = theme.join(relative);
        fs::remove_file(&path).unwrap();
        fs::create_dir(&path).unwrap();
        fs::write(path.join("keep"), b"leave obstruction intact").unwrap();
    }
    fs::remove_file(theme.join("weathr/world/tree.txt")).unwrap();
    let (store, report) =
        AsciiAssetStore::load_default_with_root_and_recovery(root.path()).unwrap();
    assert_eq!(report.fallback.len(), 2);
    assert_eq!(report.repaired.len(), 1);
    assert_eq!(report.repaired[0].key, "weathr/world/tree");
    assert_eq!(
        store.home_icon_image_bytes("explorer"),
        Some(
            embedded_default_theme_file("home_icons/explorer.png")
                .unwrap()
                .contents
        )
    );
    assert!(store.banner_lines("tundraux3").is_ok());
    for directory in [&theme, &theme.join("home_icons")] {
        for entry in fs::read_dir(directory).unwrap() {
            assert!(
                !entry
                    .unwrap()
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".ascii-assets-restore-")
            );
        }
    }
    for relative in ["banner.toml", "home_icons/explorer.png"] {
        assert_eq!(
            fs::read(theme.join(relative).join("keep")).unwrap(),
            b"leave obstruction intact"
        );
    }
}

#[test]
fn automatic_recovery_validates_required_launcher_entries() {
    let root = TempDir::new("automatic-invalid-launcher");
    restore_default_theme(root.path()).unwrap();
    fs::write(
        root.path().join("themes/default/launcher_icons.toml"),
        "schema_version = 1\n[items]\n",
    )
    .unwrap();
    let (store, report) =
        AsciiAssetStore::load_default_with_root_and_recovery(root.path()).unwrap();
    assert_eq!(report.repaired.len(), 1);
    assert_eq!(report.repaired[0].key, "launcher_icons");
    assert!(
        report.repaired[0]
            .issue
            .contains("missing required built-in application icon")
    );
    assert!(store.launcher_icon("builtin.editor").is_some());
}

#[test]
fn restore_atomically_replaces_the_destination_instead_of_truncating_it() {
    let root = TempDir::new("atomic-replacement");
    let theme = root.path().join("themes/default");
    fs::create_dir_all(&theme).unwrap();
    let path = theme.join("banner.toml");
    fs::write(&path, b"corrupt original").unwrap();
    let original = root.path().join("original-hard-link");
    fs::hard_link(&path, &original).unwrap();

    restore_default_theme_file(root.path(), "banner").unwrap();

    assert_eq!(fs::read(&original).unwrap(), b"corrupt original");
    assert_eq!(
        fs::read(&path).unwrap(),
        embedded_default_theme_file("banner").unwrap().contents
    );
    assert_eq!(fs::read_dir(&theme).unwrap().count(), 1);
}

#[test]
fn automatic_recovery_completes_custom_image_dependencies_without_overwriting_catalogs() {
    let root = TempDir::new("custom-image-dependencies");
    restore_default_theme(root.path()).unwrap();
    let theme = root.path().join("themes/default");
    let mut custom_catalogs = Vec::new();
    for (catalog, original, custom) in [
        (
            "home_icons.toml",
            "home_icons/explorer.png",
            "custom/missing.png",
        ),
        (
            "launcher_icons.toml",
            "launcher_icons/editor.png",
            "custom/unreadable.png",
        ),
    ] {
        let path = theme.join(catalog);
        let contents = fs::read_to_string(&path).unwrap().replace(original, custom);
        fs::write(&path, &contents).unwrap();
        custom_catalogs.push((path, contents));
    }
    // A directory is a deterministic unreadable image on every supported platform.
    let unreadable = theme.join("custom/unreadable.png");
    fs::create_dir_all(&unreadable).unwrap();
    fs::write(unreadable.join("keep"), b"custom data").unwrap();
    let custom_banner = "schema_version = 1\n[items.tundraux3]\nlines = [\"MY BANNER\"]\n";
    fs::write(theme.join("banner.toml"), custom_banner).unwrap();
    // Combine a missing custom image dependency with an unwritable default image.
    let blocked_default = theme.join("home_icons/explorer.png");
    fs::remove_file(&blocked_default).unwrap();
    fs::create_dir(&blocked_default).unwrap();

    let (mut store, report) =
        AsciiAssetStore::load_default_with_root_and_recovery(root.path()).unwrap();
    assert!(report.repaired.is_empty());
    assert_eq!(report.fallback.len(), 3);
    for key in ["home_icons", "launcher_icons", "home_icons/explorer.png"] {
        assert!(
            report
                .fallback
                .iter()
                .any(|file| file.key == key && file.repair_error.is_some())
        );
    }
    for (path, contents) in custom_catalogs {
        assert_eq!(fs::read_to_string(path).unwrap(), contents);
    }
    assert!(!theme.join("custom/missing.png").exists());
    assert_eq!(fs::read(unreadable.join("keep")).unwrap(), b"custom data");
    assert_eq!(store.root(), root.path());
    assert_eq!(store.banner_lines("tundraux3").unwrap(), &["MY BANNER"]);
    assert_eq!(
        store.home_icon_image_path("explorer"),
        Some(blocked_default)
    );
    assert_eq!(
        store.home_icon_image_bytes("explorer"),
        Some(
            embedded_default_theme_file("home_icons/explorer.png")
                .unwrap()
                .contents
        )
    );
    assert_eq!(
        store.launcher_icon_image_bytes("builtin.editor"),
        Some(
            embedded_default_theme_file("launcher_icons/editor.png")
                .unwrap()
                .contents
        )
    );
    store
        .reload()
        .expect("reloading retains the complete fallback dependency graph");
    assert!(store.home_icon_image_bytes("explorer").is_some());
}
