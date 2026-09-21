use std::path::{Path, PathBuf};

use crate::artwork::{
    load_art_set, load_explorer_icons, load_home_icon_catalog, load_launcher_icons, load_text_art,
};
use crate::asset_error::AssetError;
use crate::asset_manifest::{AssetKind, RequiredAsset, required_assets};
use crate::asset_resolver::AssetResolver;
use crate::clock_font::load_clock_font;
use crate::embedded_defaults::{EMBEDDED_DEFAULT_THEME_FILES, EmbeddedDefaultThemeFile};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssetCheck {
    pub key: String,
    pub path: PathBuf,
    pub kind: AssetKind,
    pub status: AssetCheckStatus,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssetCheckStatus {
    Pass,
    Warning,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssetCheckReport {
    pub root: PathBuf,
    pub theme_id: String,
    pub checks: Vec<AssetCheck>,
}

impl AssetCheckReport {
    pub fn is_ok(&self) -> bool {
        !self.has_warnings()
    }

    pub fn has_warnings(&self) -> bool {
        self.checks
            .iter()
            .any(|check| check.status == AssetCheckStatus::Warning)
    }

    pub fn missing_assets(&self) -> Vec<&AssetCheck> {
        self.checks
            .iter()
            .filter(|check| check.is_missing())
            .collect()
    }

    pub fn unreadable_assets(&self) -> Vec<&AssetCheck> {
        self.checks
            .iter()
            .filter(|check| check.is_unreadable())
            .collect()
    }

    pub fn invalid_assets(&self) -> Vec<&AssetCheck> {
        self.checks
            .iter()
            .filter(|check| check.is_invalid())
            .collect()
    }

    pub fn warning_messages(&self) -> Vec<String> {
        self.checks
            .iter()
            .filter(|check| check.status == AssetCheckStatus::Warning)
            .map(|check| format!("{}: {}", check.key, check.message))
            .collect()
    }
}

impl AssetCheck {
    pub fn is_missing(&self) -> bool {
        self.status == AssetCheckStatus::Warning && self.message.starts_with("missing ASCII asset")
    }

    pub fn is_unreadable(&self) -> bool {
        self.status == AssetCheckStatus::Warning
            && self.message.starts_with("failed to read ASCII asset")
    }

    pub fn is_invalid(&self) -> bool {
        self.status == AssetCheckStatus::Warning && !self.is_missing() && !self.is_unreadable()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DefaultThemeCheck {
    pub key: String,
    pub path: PathBuf,
    pub status: AssetCheckStatus,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DefaultThemeCheckReport {
    pub root: PathBuf,
    pub checks: Vec<DefaultThemeCheck>,
}

impl DefaultThemeCheckReport {
    pub fn is_ok(&self) -> bool {
        !self.has_warnings()
    }

    pub fn has_warnings(&self) -> bool {
        self.checks
            .iter()
            .any(|check| check.status == AssetCheckStatus::Warning)
    }

    pub fn warning_checks(&self) -> Vec<&DefaultThemeCheck> {
        self.checks
            .iter()
            .filter(|check| check.status == AssetCheckStatus::Warning)
            .collect()
    }
}

pub fn check_default_theme(root: &Path) -> DefaultThemeCheckReport {
    let resolver = AssetResolver::from_unchecked_root(root.to_path_buf());
    let mut checks = check_required_assets(root, crate::DEFAULT_THEME_ID)
        .checks
        .into_iter()
        .map(|check| DefaultThemeCheck {
            key: check.key,
            path: check.path,
            status: check.status,
            message: check.message,
        })
        .collect::<Vec<_>>();

    for file in EMBEDDED_DEFAULT_THEME_FILES.iter().filter(|file| {
        Path::new(file.relative_path)
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("png"))
    }) {
        let path = resolver.asset_path(crate::DEFAULT_THEME_ID, file.relative_path);
        let result = validate_default_theme_file(&resolver, file);
        checks.push(match result {
            Ok(()) => DefaultThemeCheck {
                key: file.key.to_string(),
                path,
                status: AssetCheckStatus::Pass,
                message: "theme file present and valid".to_string(),
            },
            Err(error) => DefaultThemeCheck {
                key: file.key.to_string(),
                path,
                status: AssetCheckStatus::Warning,
                message: error.to_string(),
            },
        });
    }

    DefaultThemeCheckReport {
        root: root.to_path_buf(),
        checks,
    }
}

pub fn check_required_assets(root: &Path, theme_id: &str) -> AssetCheckReport {
    let resolver = AssetResolver::from_unchecked_root(root.to_path_buf());
    let mut checks = Vec::new();

    for asset in required_assets() {
        let path = resolver.asset_path(theme_id, asset.relative_path);
        let result = validate_required_asset(&resolver, theme_id, &asset);

        checks.push(match result {
            Ok(()) => AssetCheck {
                key: asset.key.to_string(),
                path,
                kind: asset.kind,
                status: AssetCheckStatus::Pass,
                message: "asset present and valid".to_string(),
            },
            Err(error) => AssetCheck {
                key: asset.key.to_string(),
                path,
                kind: asset.kind,
                status: AssetCheckStatus::Warning,
                message: error.to_string(),
            },
        });
    }

    AssetCheckReport {
        root: root.to_path_buf(),
        theme_id: theme_id.to_string(),
        checks,
    }
}

pub(crate) fn validate_default_theme_file(
    resolver: &AssetResolver,
    file: &EmbeddedDefaultThemeFile,
) -> Result<(), AssetError> {
    if let Some(asset) = required_assets().iter().find(|asset| asset.key == file.key) {
        return validate_required_asset(resolver, crate::DEFAULT_THEME_ID, asset);
    }
    let contents = resolver.read_asset(crate::DEFAULT_THEME_ID, file.key, file.relative_path)?;
    // Canonical raster files have always used exact embedded-content validation.
    // Custom image paths referenced by a healthy catalog are loaded separately.
    if contents != file.contents {
        return Err(AssetError::InvalidAsset {
            asset: file.key.to_string(),
            message: "default theme image does not match the built-in default".to_string(),
        });
    }
    Ok(())
}

fn validate_required_asset(
    resolver: &AssetResolver,
    theme_id: &str,
    asset: &RequiredAsset,
) -> Result<(), AssetError> {
    match asset.kind {
        AssetKind::Text => {
            load_text_art(resolver, theme_id, asset.key, asset.relative_path).map(|_| ())
        }
        AssetKind::ArtSet => match asset.key {
            "explorer_icons" => load_explorer_icons(resolver, theme_id).map(|_| ()),
            "home_icons" => load_home_icon_catalog(resolver, theme_id).map(|_| ()),
            "launcher_icons" => load_launcher_icons(resolver, theme_id).map(|_| ()),
            _ => load_art_set(resolver, theme_id, asset.key, asset.relative_path).map(|_| ()),
        },
        AssetKind::Font => load_clock_font(resolver, theme_id).map(|_| ()),
    }
}

#[cfg(test)]
#[path = "../tests/unit/asset_validation/tests.rs"]
mod tests;
