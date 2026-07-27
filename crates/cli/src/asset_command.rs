use std::fmt;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use ascii_assets::{AssetKind, AssetResolver, DEFAULT_THEME_ID, RequiredAsset, required_assets};

use crate::arguments::{AssetAction, AssetOutput};
use crate::help_text::write_asset_help;

pub(crate) fn run_asset<Stdout: Write, Stderr: Write>(
    stdout: &mut Stdout,
    stderr: &mut Stderr,
    action: AssetAction,
    asset_root: Option<&Path>,
) -> i32 {
    let result = match action {
        AssetAction::Help => write_asset_help(stdout).map_err(AssetCommandError::Write),
        AssetAction::Show { name, output } => show_asset(stdout, &name, output, asset_root),
    };

    match result {
        Ok(()) => 0,
        Err(error) => {
            let _ = writeln!(stderr, "ERROR: {error}");
            1
        }
    }
}

fn show_asset(
    output: &mut impl Write,
    name: &str,
    requested_output: AssetOutput,
    asset_root: Option<&Path>,
) -> Result<(), AssetCommandError> {
    let asset = find_asset(name)?;
    let resolver = match asset_root {
        Some(root) => AssetResolver::new(root.to_path_buf()),
        None => AssetResolver::from_env_or_current_exe(),
    }
    .map_err(|source| AssetCommandError::Resolve { source })?;
    let path = resolver.asset_path(DEFAULT_THEME_ID, asset.relative_path);

    match requested_output {
        AssetOutput::Source => write_source(output, &path, asset.key),
        AssetOutput::RenderAll => render_asset(output, &path, &asset, None),
        AssetOutput::Item(item) => render_asset(output, &path, &asset, Some(&item)),
    }
}

fn find_asset(name: &str) -> Result<RequiredAsset, AssetCommandError> {
    let requested = normalize_name(name);
    let matches = required_assets()
        .into_iter()
        .filter(|asset| asset_aliases(asset).iter().any(|alias| alias == &requested))
        .collect::<Vec<_>>();

    match matches.as_slice() {
        [asset] => Ok(asset.clone()),
        [] => Err(AssetCommandError::UnknownAsset {
            name: name.to_string(),
        }),
        _ => Err(AssetCommandError::AmbiguousAsset {
            name: name.to_string(),
            matches: matches
                .iter()
                .map(|asset| asset.key)
                .collect::<Vec<_>>()
                .join(", "),
        }),
    }
}

fn normalize_name(name: &str) -> String {
    name.trim()
        .trim_start_matches("./")
        .replace('\\', "/")
        .replace('-', "_")
}

fn asset_aliases(asset: &RequiredAsset) -> Vec<String> {
    let path = Path::new(asset.relative_path);
    let path_without_extension = path.with_extension("");
    [
        asset.key.to_string(),
        asset.relative_path.to_string(),
        path_without_extension.to_string_lossy().into_owned(),
        path.file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default(),
        path.file_stem()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default(),
    ]
    .into_iter()
    .map(|alias| normalize_name(&alias))
    .collect()
}

fn write_source(
    output: &mut impl Write,
    path: &Path,
    asset_name: &str,
) -> Result<(), AssetCommandError> {
    let source = fs::read(path).map_err(|source| AssetCommandError::Read {
        asset: asset_name.to_string(),
        path: path.to_path_buf(),
        source,
    })?;
    output.write_all(&source).map_err(AssetCommandError::Write)
}

fn render_asset(
    output: &mut impl Write,
    path: &Path,
    asset: &RequiredAsset,
    item: Option<&str>,
) -> Result<(), AssetCommandError> {
    if asset.kind == AssetKind::Text {
        if let Some(item) = item {
            return Err(AssetCommandError::ItemNotSupported {
                asset: asset.key.to_string(),
                item: item.to_string(),
            });
        }
        return write_source(output, path, asset.key);
    }

    let source = fs::read_to_string(path).map_err(|source| AssetCommandError::Read {
        asset: asset.key.to_string(),
        path: path.to_path_buf(),
        source,
    })?;
    let document =
        toml::from_str::<toml::Value>(&source).map_err(|source| AssetCommandError::ParseToml {
            asset: asset.key.to_string(),
            path: path.to_path_buf(),
            source: Box::new(source),
        })?;
    let table_name = match asset.kind {
        AssetKind::ArtSet => "items",
        AssetKind::Font => "glyphs",
        AssetKind::Text => unreachable!("text assets return before TOML parsing"),
    };
    let entries = document
        .get(table_name)
        .and_then(toml::Value::as_table)
        .ok_or_else(|| AssetCommandError::MissingTable {
            asset: asset.key.to_string(),
            table: table_name,
        })?;

    match item {
        Some(item) => {
            let (key, value) = find_item(entries, item, asset.kind).ok_or_else(|| {
                AssetCommandError::UnknownItem {
                    asset: asset.key.to_string(),
                    item: item.to_string(),
                    available: entries
                        .keys()
                        .map(|key| selector_name(key))
                        .collect::<Vec<_>>()
                        .join(", "),
                }
            })?;
            let lines = item_lines(asset.key, key, value, asset.kind)?;
            write_lines(output, &lines)
        }
        None => {
            if entries.is_empty() {
                return Err(AssetCommandError::EmptyAsset {
                    asset: asset.key.to_string(),
                });
            }
            let show_headings = entries.len() > 1;
            for (index, (key, value)) in entries.iter().enumerate() {
                if index > 0 {
                    writeln!(output).map_err(AssetCommandError::Write)?;
                }
                if show_headings {
                    writeln!(output, "--{}", selector_name(key))
                        .map_err(AssetCommandError::Write)?;
                }
                let lines = item_lines(asset.key, key, value, asset.kind)?;
                write_lines(output, &lines)?;
            }
            Ok(())
        }
    }
}

fn find_item<'a>(
    entries: &'a toml::Table,
    requested: &str,
    kind: AssetKind,
) -> Option<(&'a str, &'a toml::Value)> {
    let requested = normalized_item_name(requested, kind);
    entries
        .iter()
        .find(|(key, _)| normalized_item_name(key, kind) == requested)
        .map(|(key, value)| (key.as_str(), value))
}

fn normalized_item_name(name: &str, kind: AssetKind) -> String {
    let name = name.trim().trim_start_matches("--");
    if kind == AssetKind::Font {
        match name {
            "colon" => return ":".to_string(),
            "space" => return " ".to_string(),
            _ => {}
        }
    }
    name.replace('-', "_")
}

fn selector_name(key: &str) -> String {
    match key {
        ":" => "colon".to_string(),
        " " => "space".to_string(),
        other => other.to_string(),
    }
}

fn item_lines(
    asset: &str,
    key: &str,
    value: &toml::Value,
    kind: AssetKind,
) -> Result<Vec<String>, AssetCommandError> {
    if kind == AssetKind::Font {
        return string_array(value).ok_or_else(|| AssetCommandError::InvalidItem {
            asset: asset.to_string(),
            item: key.to_string(),
            reason: "glyph must be an array of strings",
        });
    }

    let table = value
        .as_table()
        .ok_or_else(|| AssetCommandError::InvalidItem {
            asset: asset.to_string(),
            item: key.to_string(),
            reason: "art item must be a TOML table",
        })?;
    match (table.get("lines"), table.get("body")) {
        (Some(lines), None) => string_array(lines).ok_or_else(|| AssetCommandError::InvalidItem {
            asset: asset.to_string(),
            item: key.to_string(),
            reason: "lines must be an array of strings",
        }),
        (None, Some(body)) => {
            let body = body
                .as_str()
                .ok_or_else(|| AssetCommandError::InvalidItem {
                    asset: asset.to_string(),
                    item: key.to_string(),
                    reason: "body must be a string",
                })?;
            Ok(split_preserved_lines(body))
        }
        (Some(_), Some(_)) => Err(AssetCommandError::InvalidItem {
            asset: asset.to_string(),
            item: key.to_string(),
            reason: "art item cannot define both lines and body",
        }),
        (None, None) => Err(AssetCommandError::InvalidItem {
            asset: asset.to_string(),
            item: key.to_string(),
            reason: "art item must define lines or body",
        }),
    }
}

fn string_array(value: &toml::Value) -> Option<Vec<String>> {
    value
        .as_array()?
        .iter()
        .map(|line| line.as_str().map(ToOwned::to_owned))
        .collect()
}

fn split_preserved_lines(source: &str) -> Vec<String> {
    source
        .trim_end_matches(['\r', '\n'])
        .lines()
        .map(|line| line.trim_end_matches('\r').to_string())
        .collect()
}

fn write_lines(output: &mut impl Write, lines: &[String]) -> Result<(), AssetCommandError> {
    for line in lines {
        writeln!(output, "{line}").map_err(AssetCommandError::Write)?;
    }
    Ok(())
}

#[derive(Debug)]
enum AssetCommandError {
    Resolve {
        source: ascii_assets::AssetError,
    },
    UnknownAsset {
        name: String,
    },
    AmbiguousAsset {
        name: String,
        matches: String,
    },
    Read {
        asset: String,
        path: PathBuf,
        source: std::io::Error,
    },
    ParseToml {
        asset: String,
        path: PathBuf,
        source: Box<toml::de::Error>,
    },
    MissingTable {
        asset: String,
        table: &'static str,
    },
    EmptyAsset {
        asset: String,
    },
    UnknownItem {
        asset: String,
        item: String,
        available: String,
    },
    ItemNotSupported {
        asset: String,
        item: String,
    },
    InvalidItem {
        asset: String,
        item: String,
        reason: &'static str,
    },
    Write(std::io::Error),
}

impl fmt::Display for AssetCommandError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Resolve { source } => write!(formatter, "could not resolve asset root: {source}"),
            Self::UnknownAsset { name } => write!(
                formatter,
                "unknown asset {name:?}; run `tundra-cli asset` to list available assets"
            ),
            Self::AmbiguousAsset { name, matches } => {
                write!(
                    formatter,
                    "asset name {name:?} is ambiguous; use one of: {matches}"
                )
            }
            Self::Read {
                asset,
                path,
                source,
            } => write!(
                formatter,
                "could not read asset {asset:?} at {}: {source}",
                path.display()
            ),
            Self::ParseToml {
                asset,
                path,
                source,
            } => write!(
                formatter,
                "could not parse asset {asset:?} at {}: {source}",
                path.display()
            ),
            Self::MissingTable { asset, table } => {
                write!(formatter, "asset {asset:?} does not define [{table}]")
            }
            Self::EmptyAsset { asset } => write!(formatter, "asset {asset:?} contains no items"),
            Self::UnknownItem {
                asset,
                item,
                available,
            } => write!(
                formatter,
                "asset {asset:?} has no item {item:?}; available items: {available}"
            ),
            Self::ItemNotSupported { asset, item } => write!(
                formatter,
                "asset {asset:?} is a text file and does not support item selector --{item}"
            ),
            Self::InvalidItem {
                asset,
                item,
                reason,
            } => write!(
                formatter,
                "invalid item {item:?} in asset {asset:?}: {reason}"
            ),
            Self::Write(source) => write!(formatter, "could not write asset output: {source}"),
        }
    }
}
