use std::collections::BTreeMap;

use serde::Deserialize;

use crate::artwork::{pad_lines, read_asset_to_string};
use crate::asset_error::AssetError;
use crate::asset_resolver::AssetResolver;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClockFontAsset {
    pub height: usize,
    pub spacing: usize,
    pub separator_spacing: usize,
    pub glyphs: BTreeMap<char, Vec<String>>,
}

pub(crate) fn load_clock_font(
    resolver: &AssetResolver,
    theme_id: &str,
) -> Result<ClockFontAsset, AssetError> {
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

#[derive(Debug, Deserialize)]
struct ClockFontFile {
    #[allow(dead_code)]
    name: Option<String>,
    height: usize,
    spacing: Option<usize>,
    separator_spacing: Option<usize>,
    glyphs: BTreeMap<String, Vec<String>>,
}
