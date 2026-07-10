use serde::Deserialize;
use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

pub const ENV_ASSETS_DIR: &str = "TUNDRA_ASCII_ASSETS_DIR";
pub const DEFAULT_THEME_ID: &str = "default";
pub const CANONICAL_ASSETS_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/assets");

const REQUIRED_TEXT_ARTS: &[(&str, &str)] = &[
    ("weathr/animation/airplane", "weathr/animation/airplane.txt"),
    ("weathr/animation/cloud_0", "weathr/animation/cloud_0.txt"),
    ("weathr/animation/cloud_1", "weathr/animation/cloud_1.txt"),
    ("weathr/animation/cloud_2", "weathr/animation/cloud_2.txt"),
    ("weathr/animation/cloud_3", "weathr/animation/cloud_3.txt"),
    ("weathr/animation/sun_0", "weathr/animation/sun_0.txt"),
    ("weathr/animation/sun_1", "weathr/animation/sun_1.txt"),
    (
        "weathr/animation/moon/phase_0",
        "weathr/animation/moon/phase_0.txt",
    ),
    (
        "weathr/animation/moon/phase_1",
        "weathr/animation/moon/phase_1.txt",
    ),
    (
        "weathr/animation/moon/phase_2",
        "weathr/animation/moon/phase_2.txt",
    ),
    (
        "weathr/animation/moon/phase_3",
        "weathr/animation/moon/phase_3.txt",
    ),
    (
        "weathr/animation/moon/phase_4",
        "weathr/animation/moon/phase_4.txt",
    ),
    (
        "weathr/animation/moon/phase_5",
        "weathr/animation/moon/phase_5.txt",
    ),
    (
        "weathr/animation/moon/phase_6",
        "weathr/animation/moon/phase_6.txt",
    ),
    (
        "weathr/animation/moon/phase_7",
        "weathr/animation/moon/phase_7.txt",
    ),
    ("weathr/world/fence", "weathr/world/fence.txt"),
    ("weathr/world/house", "weathr/world/house.txt"),
    ("weathr/world/mailbox", "weathr/world/mailbox.txt"),
    ("weathr/world/pine_tree", "weathr/world/pine_tree.txt"),
    ("weathr/world/tree", "weathr/world/tree.txt"),
];

const REQUIRED_TOML_ASSETS: &[(&str, &str, AssetKind)] = &[
    ("banner", "banner.toml", AssetKind::ArtSet),
    ("home_icons", "home_icons.toml", AssetKind::ArtSet),
    (
        "weathr/render/clock_font",
        "weathr/render/clock_font.toml",
        AssetKind::Font,
    ),
];

#[derive(Debug, thiserror::Error)]
pub enum AssetError {
    #[error("failed to resolve current executable path: {source}")]
    CurrentExe {
        #[source]
        source: std::io::Error,
    },

    #[error("current executable path has no parent: {path}")]
    MissingCurrentExeParent { path: PathBuf },

    #[error("ASCII asset root does not exist: {path}")]
    MissingRoot { path: PathBuf },

    #[error("ASCII asset root is not a directory: {path}")]
    RootNotDirectory { path: PathBuf },

    #[error("missing ASCII asset {asset} at {path}")]
    MissingAsset { asset: String, path: PathBuf },

    #[error("failed to read ASCII asset {asset} at {path}: {source}")]
    ReadAsset {
        asset: String,
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("failed to parse TOML asset {asset} at {path}: {source}")]
    ParseToml {
        asset: String,
        path: PathBuf,
        #[source]
        source: Box<toml::de::Error>,
    },

    #[error("invalid ASCII asset {asset}: {message}")]
    InvalidAsset { asset: String, message: String },

    #[error("unknown ASCII asset {asset}")]
    UnknownAsset { asset: String },

    #[error("failed to copy ASCII assets from {from} to {destination}: {error}")]
    CopyAssets {
        from: PathBuf,
        destination: PathBuf,
        error: String,
    },

    #[error("failed to derive Cargo profile dir from OUT_DIR {out_dir}")]
    InvalidOutDir { out_dir: PathBuf },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssetKind {
    Text,
    ArtSet,
    Font,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequiredAsset {
    pub key: &'static str,
    pub relative_path: &'static str,
    pub kind: AssetKind,
}

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
pub struct TextArt {
    key: String,
    lines: Vec<String>,
    width: usize,
    height: usize,
}

impl TextArt {
    pub fn key(&self) -> &str {
        &self.key
    }

    pub fn lines(&self) -> &[String] {
        &self.lines
    }

    pub fn to_vec(&self) -> Vec<String> {
        self.lines.clone()
    }

    pub fn width(&self) -> usize {
        self.width
    }

    pub fn height(&self) -> usize {
        self.height
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtItem {
    pub key: String,
    pub label: Option<String>,
    pub lines: Vec<String>,
    pub width: usize,
    pub height: usize,
}

impl ArtItem {
    pub fn key(&self) -> &str {
        &self.key
    }

    pub fn label(&self) -> Option<&str> {
        self.label.as_deref()
    }

    pub fn lines(&self) -> &[String] {
        &self.lines
    }

    pub fn width(&self) -> usize {
        self.width
    }

    pub fn height(&self) -> usize {
        self.height
    }
}

pub type HomeIcon = ArtItem;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtSet {
    items: BTreeMap<String, ArtItem>,
}

impl ArtSet {
    pub fn get(&self, key: &str) -> Option<&ArtItem> {
        self.items.get(key)
    }

    pub fn items(&self) -> impl Iterator<Item = &ArtItem> {
        self.items.values()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HomeIconCatalog {
    icons: BTreeMap<String, ArtItem>,
    labels: BTreeMap<String, String>,
}

impl HomeIconCatalog {
    pub fn icon_for_label(&self, label: &str) -> Option<&ArtItem> {
        let key = match label {
            "Explorer" => "explorer",
            "Launcher" => "launcher",
            "Editor" => "editor",
            "Settings" => "settings",
            "Diagnostics" => "diagnostics",
            "User Management" => "user_management",
            "User Profile" => "user_profile",
            _ => self
                .labels
                .get(label)
                .map(String::as_str)
                .unwrap_or("default"),
        };

        self.icons.get(key).or_else(|| self.icons.get("default"))
    }

    pub fn icon(&self, key: &str) -> Option<&ArtItem> {
        self.icons.get(key)
    }

    pub fn icon_for_key(&self, key: &str) -> Option<&ArtItem> {
        self.icon(key)
    }

    pub fn icons(&self) -> impl Iterator<Item = &ArtItem> {
        self.icons.values()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClockFontAsset {
    pub height: usize,
    pub spacing: usize,
    pub separator_spacing: usize,
    pub glyphs: BTreeMap<char, Vec<String>>,
}

#[derive(Debug, Clone)]
pub struct AsciiAssetStore {
    resolver: AssetResolver,
    theme_id: String,
    banners: ArtSet,
    home_icons: HomeIconCatalog,
    clock_font: ClockFontAsset,
    text_arts: BTreeMap<String, TextArt>,
}

impl AsciiAssetStore {
    pub fn load_default() -> Result<Self, AssetError> {
        Self::load_theme(DEFAULT_THEME_ID)
    }

    pub fn load_theme(theme_id: &str) -> Result<Self, AssetError> {
        Self::load_with_resolver(AssetResolver::from_env_or_current_exe()?, theme_id)
    }

    pub fn load_with_root(root: impl Into<PathBuf>, theme_id: &str) -> Result<Self, AssetError> {
        Self::load_with_resolver(AssetResolver::new(root.into())?, theme_id)
    }

    pub fn load_with_resolver(resolver: AssetResolver, theme_id: &str) -> Result<Self, AssetError> {
        let banners = load_art_set(&resolver, theme_id, "banner", "banner.toml")?;
        let home_icons = load_home_icon_catalog(&resolver, theme_id)?;
        let clock_font = load_clock_font(&resolver, theme_id)?;
        let mut text_arts = BTreeMap::new();
        for (key, relative_path) in REQUIRED_TEXT_ARTS {
            let art = load_text_art(&resolver, theme_id, key, relative_path)?;
            text_arts.insert((*key).to_string(), art);
        }

        Ok(Self {
            resolver,
            theme_id: theme_id.to_string(),
            banners,
            home_icons,
            clock_font,
            text_arts,
        })
    }

    pub fn reload(&mut self) -> Result<(), AssetError> {
        *self = Self::load_with_resolver(self.resolver.clone(), &self.theme_id)?;
        Ok(())
    }

    pub fn root(&self) -> &Path {
        self.resolver.root()
    }

    pub fn theme_id(&self) -> &str {
        &self.theme_id
    }

    pub fn banner_lines(&self, key: &str) -> Result<&[String], AssetError> {
        self.banners
            .get(key)
            .map(ArtItem::lines)
            .ok_or_else(|| AssetError::UnknownAsset {
                asset: format!("banner/{key}"),
            })
    }

    pub fn home_icon_catalog(&self) -> &HomeIconCatalog {
        &self.home_icons
    }

    pub fn clock_font(&self) -> &ClockFontAsset {
        &self.clock_font
    }

    pub fn text_art(&self, key: &str) -> Result<&TextArt, AssetError> {
        self.text_arts
            .get(key)
            .ok_or_else(|| AssetError::UnknownAsset {
                asset: key.to_string(),
            })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssetResolver {
    root: PathBuf,
}

impl AssetResolver {
    pub fn from_env_or_current_exe() -> Result<Self, AssetError> {
        if let Some(root) = env::var_os(ENV_ASSETS_DIR) {
            return Self::new(PathBuf::from(root));
        }

        let exe = env::current_exe().map_err(|source| AssetError::CurrentExe { source })?;
        let Some(parent) = exe.parent() else {
            return Err(AssetError::MissingCurrentExeParent { path: exe });
        };
        let primary = parent.join("assets");
        if primary.exists() {
            return Self::new(primary);
        }
        if parent.file_name().is_some_and(|name| name == "deps")
            && let Some(profile_dir) = parent.parent()
        {
            let profile_assets = profile_dir.join("assets");
            if profile_assets.exists() {
                return Self::new(profile_assets);
            }
        }
        Self::new(primary)
    }

    pub fn canonical() -> Result<Self, AssetError> {
        Self::new(CANONICAL_ASSETS_DIR)
    }

    pub fn new(root: impl Into<PathBuf>) -> Result<Self, AssetError> {
        let root = root.into();
        if !root.exists() {
            return Err(AssetError::MissingRoot { path: root });
        }
        if !root.is_dir() {
            return Err(AssetError::RootNotDirectory { path: root });
        }
        Ok(Self { root })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn theme_path(&self, theme_id: &str) -> PathBuf {
        self.root.join("themes").join(theme_id)
    }

    pub fn asset_path(&self, theme_id: &str, relative_path: &str) -> PathBuf {
        self.theme_path(theme_id).join(relative_path)
    }
}

pub fn asset_root_from_env_or_current_exe() -> Result<PathBuf, AssetError> {
    AssetResolver::from_env_or_current_exe().map(|resolver| resolver.root)
}

pub fn required_assets() -> Vec<RequiredAsset> {
    let mut assets = REQUIRED_TOML_ASSETS
        .iter()
        .map(|(key, relative_path, kind)| RequiredAsset {
            key,
            relative_path,
            kind: *kind,
        })
        .collect::<Vec<_>>();
    assets.extend(
        REQUIRED_TEXT_ARTS
            .iter()
            .map(|(key, relative_path)| RequiredAsset {
                key,
                relative_path,
                kind: AssetKind::Text,
            }),
    );
    assets
}

pub fn check_required_assets(root: &Path, theme_id: &str) -> AssetCheckReport {
    let resolver = AssetResolver {
        root: root.to_path_buf(),
    };
    let mut checks = Vec::new();

    for asset in required_assets() {
        let path = resolver.asset_path(theme_id, asset.relative_path);
        let result = match asset.kind {
            AssetKind::Text => {
                load_text_art(&resolver, theme_id, asset.key, asset.relative_path).map(|_| ())
            }
            AssetKind::ArtSet => {
                load_art_set(&resolver, theme_id, asset.key, asset.relative_path).map(|_| ())
            }
            AssetKind::Font => load_clock_font(&resolver, theme_id).map(|_| ()),
        };

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

pub fn copy_canonical_assets_to_profile_dir(out_dir: &Path) -> Result<PathBuf, AssetError> {
    let profile_dir = cargo_profile_dir_from_out_dir(out_dir)?;
    let destination = profile_dir.join("assets");
    copy_dir_recursive(Path::new(CANONICAL_ASSETS_DIR), &destination).map_err(|error| {
        AssetError::CopyAssets {
            from: PathBuf::from(CANONICAL_ASSETS_DIR),
            destination: destination.clone(),
            error: error.to_string(),
        }
    })?;
    Ok(destination)
}

pub fn cargo_profile_dir_from_out_dir(out_dir: &Path) -> Result<PathBuf, AssetError> {
    let mut cursor = out_dir;
    while let Some(parent) = cursor.parent() {
        if cursor.file_name().is_some_and(|name| name == "build") {
            return Ok(parent.to_path_buf());
        }
        cursor = parent;
    }

    Err(AssetError::InvalidOutDir {
        out_dir: out_dir.to_path_buf(),
    })
}

fn copy_dir_recursive(source: &Path, destination: &Path) -> Result<(), std::io::Error> {
    fs::create_dir_all(destination)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            copy_dir_recursive(&source_path, &destination_path)?;
        } else if file_type.is_file() {
            if let Some(parent) = destination_path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::copy(&source_path, &destination_path)?;
        }
    }
    Ok(())
}

fn load_text_art(
    resolver: &AssetResolver,
    theme_id: &str,
    key: &str,
    relative_path: &str,
) -> Result<TextArt, AssetError> {
    let source = read_asset_to_string(resolver, theme_id, key, relative_path)?;
    let lines = split_preserved_lines(&source);
    let (width, height) = measure_lines(&lines);
    if height == 0 {
        return Err(AssetError::InvalidAsset {
            asset: key.to_string(),
            message: "text art must contain at least one line".to_string(),
        });
    }

    Ok(TextArt {
        key: key.to_string(),
        lines,
        width,
        height,
    })
}

fn load_home_icon_catalog(
    resolver: &AssetResolver,
    theme_id: &str,
) -> Result<HomeIconCatalog, AssetError> {
    let art_set = load_art_set(resolver, theme_id, "home_icons", "home_icons.toml")?;
    let mut icons = BTreeMap::new();
    let mut labels = BTreeMap::new();
    for item in art_set.items.into_values() {
        if let Some(label) = &item.label {
            labels.insert(label.clone(), item.key.clone());
        }
        icons.insert(item.key.clone(), item);
    }

    for required in [
        "explorer",
        "launcher",
        "editor",
        "settings",
        "diagnostics",
        "user_management",
        "user_profile",
        "default",
    ] {
        if !icons.contains_key(required) {
            return Err(AssetError::InvalidAsset {
                asset: "home_icons".to_string(),
                message: format!("missing required home icon {required}"),
            });
        }
    }

    Ok(HomeIconCatalog { icons, labels })
}

fn load_art_set(
    resolver: &AssetResolver,
    theme_id: &str,
    key: &str,
    relative_path: &str,
) -> Result<ArtSet, AssetError> {
    let path = resolver.asset_path(theme_id, relative_path);
    let source = read_asset_to_string(resolver, theme_id, key, relative_path)?;
    let file: ArtSetFile = toml::from_str(&source).map_err(|source| AssetError::ParseToml {
        asset: key.to_string(),
        path,
        source: Box::new(source),
    })?;
    if file.schema_version != 1 {
        return Err(AssetError::InvalidAsset {
            asset: key.to_string(),
            message: format!("unsupported schema_version {}", file.schema_version),
        });
    }

    let mut items = BTreeMap::new();
    for (item_key, item_file) in file.items {
        let lines = match (item_file.lines, item_file.body) {
            (Some(lines), None) => lines,
            (None, Some(body)) => split_preserved_lines(&body),
            (Some(_), Some(_)) => {
                return Err(AssetError::InvalidAsset {
                    asset: key.to_string(),
                    message: format!("art item {item_key} must use either lines or body, not both"),
                });
            }
            (None, None) => {
                return Err(AssetError::InvalidAsset {
                    asset: key.to_string(),
                    message: format!("art item {item_key} is missing lines or body"),
                });
            }
        };
        let (width, height) = measure_lines(&lines);
        if height == 0 {
            return Err(AssetError::InvalidAsset {
                asset: key.to_string(),
                message: format!("art item {item_key} must contain at least one line"),
            });
        }
        items.insert(
            item_key.clone(),
            ArtItem {
                key: item_key,
                label: item_file.label,
                lines,
                width,
                height,
            },
        );
    }

    Ok(ArtSet { items })
}

fn load_clock_font(resolver: &AssetResolver, theme_id: &str) -> Result<ClockFontAsset, AssetError> {
    let relative_path = "weathr/render/clock_font.toml";
    let key = "weathr/render/clock_font";
    let path = resolver.asset_path(theme_id, relative_path);
    let source = read_asset_to_string(resolver, theme_id, key, relative_path)?;
    let file: ClockFontFile = toml::from_str(&source).map_err(|source| AssetError::ParseToml {
        asset: key.to_string(),
        path,
        source: Box::new(source),
    })?;

    if file.height == 0 {
        return Err(AssetError::InvalidAsset {
            asset: key.to_string(),
            message: "clock font height must be greater than zero".to_string(),
        });
    }

    let mut glyphs = BTreeMap::new();
    for (glyph_key, lines) in file.glyphs {
        let mut chars = glyph_key.chars();
        let Some(ch) = chars.next() else {
            return Err(AssetError::InvalidAsset {
                asset: key.to_string(),
                message: "clock font glyph key cannot be empty".to_string(),
            });
        };
        if chars.next().is_some() {
            return Err(AssetError::InvalidAsset {
                asset: key.to_string(),
                message: format!("clock font glyph key {glyph_key:?} must be one character"),
            });
        }
        if lines.len() != file.height {
            return Err(AssetError::InvalidAsset {
                asset: key.to_string(),
                message: format!(
                    "clock font glyph {glyph_key:?} has {} rows, expected {}",
                    lines.len(),
                    file.height
                ),
            });
        }
        glyphs.insert(ch, pad_lines(lines));
    }

    for required in "0123456789: APM".chars() {
        if !glyphs.contains_key(&required) {
            return Err(AssetError::InvalidAsset {
                asset: key.to_string(),
                message: format!("clock font is missing required glyph {required:?}"),
            });
        }
    }

    Ok(ClockFontAsset {
        height: file.height,
        spacing: file.spacing.unwrap_or(1),
        separator_spacing: file
            .separator_spacing
            .unwrap_or_else(|| file.spacing.unwrap_or(1)),
        glyphs,
    })
}

fn read_asset_to_string(
    resolver: &AssetResolver,
    theme_id: &str,
    key: &str,
    relative_path: &str,
) -> Result<String, AssetError> {
    let path = resolver.asset_path(theme_id, relative_path);
    if !path.exists() {
        return Err(AssetError::MissingAsset {
            asset: key.to_string(),
            path,
        });
    }
    fs::read_to_string(&path).map_err(|source| AssetError::ReadAsset {
        asset: key.to_string(),
        path,
        source,
    })
}

fn split_preserved_lines(source: &str) -> Vec<String> {
    source
        .trim_end_matches(['\r', '\n'])
        .lines()
        .map(|line| line.trim_end_matches('\r').to_string())
        .collect()
}

fn measure_lines(lines: &[String]) -> (usize, usize) {
    (
        lines
            .iter()
            .map(|line| line.chars().count())
            .max()
            .unwrap_or(0),
        lines.len(),
    )
}

fn pad_lines(mut lines: Vec<String>) -> Vec<String> {
    let (width, _) = measure_lines(&lines);
    for line in &mut lines {
        let padding = width.saturating_sub(line.chars().count());
        line.push_str(&" ".repeat(padding));
    }
    lines
}

#[derive(Debug, Deserialize)]
struct ArtSetFile {
    schema_version: u16,
    #[allow(dead_code)]
    name: Option<String>,
    items: BTreeMap<String, ArtItemFile>,
}

#[derive(Debug, Deserialize)]
struct ArtItemFile {
    label: Option<String>,
    lines: Option<Vec<String>>,
    body: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ClockFontFile {
    #[allow(dead_code)]
    name: Option<String>,
    height: usize,
    spacing: Option<usize>,
    separator_spacing: Option<usize>,
    glyphs: BTreeMap<String, Vec<String>>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn default_store_loads_canonical_assets() {
        let store = AsciiAssetStore::load_with_root(CANONICAL_ASSETS_DIR, DEFAULT_THEME_ID)
            .expect("canonical assets should load");

        assert_eq!(store.banner_lines("tundraux3").unwrap().len(), 10);
        assert!(store.home_icon_catalog().icon("explorer").is_some());
        assert_eq!(store.clock_font().height, 7);
        assert!(store.text_art("weathr/world/house").unwrap().height() >= 10);
    }

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
    fn derives_profile_dir_from_build_out_dir() {
        let out_dir = Path::new("/repo/target/debug/build/tundra-cli-abc/out");

        let profile_dir = cargo_profile_dir_from_out_dir(out_dir).expect("profile dir");

        assert_eq!(profile_dir, PathBuf::from("/repo/target/debug"));
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
}
