use crate::{LanguageError, LanguageErrorKind, RepairDiagnostic, RepairKind};
use serde::Deserialize;
use std::{
    fs,
    path::{Path, PathBuf},
};
use unic_langid::LanguageIdentifier;

pub const DEFAULT_LANGUAGE: &str = "en-US";

pub fn canonical_language_code(code: &str) -> Result<String, LanguageError> {
    let parsed: LanguageIdentifier = code.parse().map_err(|_| {
        LanguageError::new(
            LanguageErrorKind::UnknownLanguage,
            None,
            format!("Invalid language code: {code}"),
        )
    })?;
    let code = parsed.to_string();
    Ok(if code == "zh-Hans" {
        "zh-CN".to_owned()
    } else {
        code
    })
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct LanguageOption {
    pub code: String,
    pub native_name: String,
}

#[derive(Debug, Deserialize)]
struct Manifest {
    format_version: u32,
    code: String,
    native_name: String,
}

pub(crate) fn parse_manifest(
    source: &str,
    expected: &str,
    path: &Path,
) -> Result<LanguageOption, LanguageError> {
    let error =
        |message| LanguageError::new(LanguageErrorKind::Manifest, Some(path.to_owned()), message);
    let manifest: Manifest =
        toml::from_str(source).map_err(|err| error(format!("Invalid manifest: {err}")))?;
    if manifest.format_version != 1
        || manifest.native_name.trim().is_empty()
        || manifest.code != expected
    {
        return Err(error(format!(
            "Expected format_version=1, code={expected}, and a nonempty native_name"
        )));
    }
    if canonical_language_code(&manifest.code)? != manifest.code {
        return Err(error("Manifest code must be canonical".to_owned()));
    }
    Ok(LanguageOption {
        code: manifest.code,
        native_name: manifest.native_name,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LanguageCatalog {
    options: Vec<LanguageOption>,
    pub diagnostics: Vec<RepairDiagnostic>,
}

impl LanguageCatalog {
    /// Static selector metadata, available without probing the filesystem.
    pub fn built_in() -> Self {
        Self {
            options: vec![
                LanguageOption {
                    code: "en-US".to_owned(),
                    native_name: "English".to_owned(),
                },
                LanguageOption {
                    code: "zh-CN".to_owned(),
                    native_name: "简体中文".to_owned(),
                },
            ],
            diagnostics: Vec::new(),
        }
    }

    /// `root` is the asset root, shared with ascii-assets, not the locales directory.
    /// Embedded English remains selectable even with an absent/read-only asset root.
    pub fn discover(root: impl AsRef<Path>) -> Result<Self, LanguageError> {
        let root = root.as_ref().join("locales");
        let mut catalog = Self {
            options: vec![LanguageOption {
                code: DEFAULT_LANGUAGE.to_owned(),
                native_name: "English".to_owned(),
            }],
            diagnostics: Vec::new(),
        };
        let entries = match fs::read_dir(&root) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(catalog),
            Err(error) => {
                return Err(LanguageError::new(
                    LanguageErrorKind::Io,
                    Some(root),
                    error.to_string(),
                ));
            }
        };
        for entry in entries {
            let entry = entry.map_err(|error| {
                LanguageError::new(LanguageErrorKind::Io, Some(root.clone()), error.to_string())
            })?;
            let path = entry.path().join("manifest.toml");
            if !entry.path().is_dir() {
                continue;
            }
            let code = entry.file_name().to_string_lossy().into_owned();
            let result = fs::read_to_string(&path)
                .map_err(|error| {
                    LanguageError::new(LanguageErrorKind::Io, Some(path.clone()), error.to_string())
                })
                .and_then(|source| parse_manifest(&source, &code, &path));
            match result {
                Ok(option) => {
                    catalog
                        .options
                        .retain(|existing| existing.code != option.code);
                    catalog.options.push(option);
                }
                Err(error) => catalog.diagnostics.push(RepairDiagnostic {
                    kind: RepairKind::InvalidManifest,
                    path,
                    message: error.to_string(),
                    repaired: false,
                }),
            }
        }
        catalog.options.sort_by(|a, b| a.code.cmp(&b.code));
        catalog.diagnostics.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(catalog)
    }

    pub fn options(&self) -> &[LanguageOption] {
        &self.options
    }

    pub fn discover_default() -> Result<Self, LanguageError> {
        let root = default_asset_root()?;
        Self::discover(root)
    }
}

pub fn default_asset_root() -> Result<PathBuf, LanguageError> {
    ascii_assets::asset_root_for_recovery_from_env_or_current_exe()
        .map_err(|error| LanguageError::new(LanguageErrorKind::Io, None, error.to_string()))
}
