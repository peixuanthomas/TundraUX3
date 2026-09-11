use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::artwork::load_art_set;
use crate::asset_error::AssetError;
use crate::asset_manifest::DEFAULT_THEME_ID;
use crate::asset_validation::{AssetCheckStatus, check_default_theme, validate_default_theme_file};
use crate::embedded_defaults::{EMBEDDED_DEFAULT_THEME_FILES, embedded_default_theme_file};
use crate::{AsciiAssetStore, AssetResolver};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssetRestoreReport {
    pub path: PathBuf,
    pub changed: bool,
}

/// A damaged file encountered during automatic default-theme loading.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssetRecoveryFile {
    pub key: String,
    pub path: PathBuf,
    /// The original validation/read failure that triggered recovery.
    pub issue: String,
    /// Restore, post-write validation, or dependency failure; present only for memory fallbacks.
    pub repair_error: Option<String>,
}

/// Only files requiring recovery are listed; healthy custom catalogs/art remain intact.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DefaultThemeRecoveryReport {
    pub root: PathBuf,
    pub repaired: Vec<AssetRecoveryFile>,
    pub fallback: Vec<AssetRecoveryFile>,
}

pub(crate) fn recover_default_theme(
    root: &Path,
) -> Result<(AsciiAssetStore, DefaultThemeRecoveryReport), AssetError> {
    let mut resolver = AssetResolver::from_unchecked_root(root.to_path_buf());
    let mut report = DefaultThemeRecoveryReport {
        root: root.to_path_buf(),
        repaired: Vec::new(),
        fallback: Vec::new(),
    };
    // Continue after individual restore failures so writable siblings still heal.
    for file in EMBEDDED_DEFAULT_THEME_FILES {
        let Err(issue) = validate_default_theme_file(&resolver, file) else {
            continue;
        };
        let mut recovery = AssetRecoveryFile {
            key: file.key.to_string(),
            path: resolver.asset_path(DEFAULT_THEME_ID, file.relative_path),
            issue: issue.to_string(),
            repair_error: None,
        };
        match restore_default_theme_file(root, file.key) {
            Ok(_) => report.repaired.push(recovery),
            Err(error) => {
                recovery.repair_error = Some(error.to_string());
                resolver.use_embedded_default(file.relative_path, file.contents);
                // Use the very same parser and semantic checks as disk-backed files.
                validate_default_theme_file(&resolver, file)?;
                report.fallback.push(recovery);
            }
        }
    }
    // A syntactically valid custom catalog can still refer to an unavailable image.
    // Use its complete embedded catalog in memory so every image dependency resolves
    // to a default file already repaired above. Never rewrite healthy custom data.
    for key in ["home_icons", "launcher_icons"] {
        let file = embedded_default_theme_file(key).expect("embedded icon catalog");
        let catalog = load_art_set(&resolver, DEFAULT_THEME_ID, key, file.relative_path)?;
        let failures = catalog
            .items()
            .filter_map(|item| item.image_path())
            .filter_map(|path| resolver.read_asset(DEFAULT_THEME_ID, path, path).err())
            .map(|error| error.to_string())
            .collect::<Vec<_>>();
        if failures.is_empty() {
            continue;
        }
        resolver.use_embedded_default(file.relative_path, file.contents);
        validate_default_theme_file(&resolver, file)?;
        report.repaired.retain(|recovery| recovery.key != key);
        report.fallback.push(AssetRecoveryFile {
            key: key.to_string(),
            path: resolver.asset_path(DEFAULT_THEME_ID, file.relative_path),
            issue: failures.join("; "),
            repair_error: Some(
                "custom image dependencies are unavailable; using the embedded catalog without replacing the disk catalog".to_string(),
            ),
        });
    }
    let store = AsciiAssetStore::load_with_resolver(resolver, DEFAULT_THEME_ID)?;
    Ok((store, report))
}

/// Restores one default-theme file from the contents embedded in the binary.
/// Writes atomically within the destination directory, then revalidates the file.
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
        atomic_write(&path, contents).map_err(|source| AssetError::RestoreAsset {
            asset: file_key.to_string(),
            path: path.clone(),
            source,
        })?;
    }

    validate_default_theme_file(
        &AssetResolver::from_unchecked_root(root.to_path_buf()),
        file,
    )?;
    Ok(AssetRestoreReport { path, changed })
}

static NEXT_RESTORE_ID: AtomicU64 = AtomicU64::new(0);

fn atomic_write(path: &Path, contents: &[u8]) -> io::Result<()> {
    let parent = path.parent().expect("asset path has a parent");
    // create_new prevents clobbering another concurrent restore's staging file.
    for _ in 0..64 {
        let id = NEXT_RESTORE_ID.fetch_add(1, Ordering::Relaxed);
        let temporary = parent.join(format!(
            ".ascii-assets-restore-{}-{id}.tmp",
            std::process::id()
        ));
        let mut output = match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
        {
            Ok(output) => output,
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        };
        let staged = StagedAsset(temporary);
        let written = output.write_all(contents).and_then(|()| output.sync_all());
        drop(output);
        written?;
        // Never remove the destination first: a failed rename leaves it intact.
        fs::rename(&staged.0, path)?;
        return Ok(());
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "could not allocate an asset staging file",
    ))
}

struct StagedAsset(PathBuf);

impl Drop for StagedAsset {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
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
