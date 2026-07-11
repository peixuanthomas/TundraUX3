use std::collections::BTreeMap;
use std::fs;

use serde::Deserialize;

use crate::asset_error::AssetError;
use crate::asset_resolver::AssetResolver;

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

pub(crate) fn load_text_art(
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

pub(crate) fn load_home_icon_catalog(
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

pub(crate) fn load_art_set(
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

pub(crate) fn read_asset_to_string(
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

pub(crate) fn pad_lines(mut lines: Vec<String>) -> Vec<String> {
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
