use super::{ManagementError, action, validate_name};
use crate::management::ManagementRow;
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PackageType {
    Formula,
    Cask,
}

impl PackageType {
    pub(super) fn flag(self) -> &'static str {
        match self {
            Self::Formula => "--formula",
            Self::Cask => "--cask",
        }
    }

    fn id(self) -> &'static str {
        match self {
            Self::Formula => "formula",
            Self::Cask => "cask",
        }
    }

    fn label(self) -> String {
        i18n::tr!(match self {
            Self::Formula => "management-brew-formula",
            Self::Cask => "management-brew-cask",
        })
    }
}

#[derive(Debug, Clone)]
pub(super) struct Package {
    pub kind: PackageType,
    pub name: String,
    pub installed: String,
    pub available: String,
    pub description: String,
    homepage: String,
    tap: String,
    pinned: bool,
}

impl Package {
    pub(super) fn search_result(kind: PackageType, name: &str) -> Self {
        Self {
            kind,
            name: name.into(),
            installed: String::new(),
            available: String::new(),
            description: String::new(),
            homepage: String::new(),
            tap: String::new(),
            pinned: false,
        }
    }

    pub(super) fn id(&self) -> String {
        format!("{}:{}", self.kind.id(), self.name)
    }

    pub(super) fn identity(&self) -> BTreeMap<String, String> {
        let mut identity = BTreeMap::from([
            ("backend".into(), "brew".into()),
            ("name".into(), self.name.clone()),
            ("package_type".into(), self.kind.id().into()),
            ("installed_version".into(), self.installed.clone()),
        ]);
        if !self.available.is_empty() {
            identity.insert("available_version".into(), self.available.clone());
        }
        identity
    }

    pub(super) fn into_row(self) -> ManagementRow {
        let mut actions = if self.installed.is_empty() {
            vec![action("install")]
        } else {
            vec![action("remove"), action("upgrade")]
        };
        if self.pinned {
            for action in &mut actions {
                if action.id == "upgrade" {
                    action.disabled_reason = Some(i18n::tr!("management-brew-pinned"));
                }
            }
        }
        ManagementRow {
            id: self.id(),
            identity: self.identity(),
            cells: vec![
                self.name.clone(),
                self.kind.label(),
                self.installed.clone(),
                self.available.clone(),
                self.description.clone(),
            ],
            detail: vec![
                (i18n::tr!("management-detail-package"), self.name),
                (i18n::tr!("management-column-type"), self.kind.label()),
                (i18n::tr!("management-detail-installed"), self.installed),
                (i18n::tr!("management-detail-available"), self.available),
                (i18n::tr!("management-detail-description"), self.description),
                (i18n::tr!("management-brew-homepage"), self.homepage),
                (i18n::tr!("management-detail-source"), self.tap),
            ],
            actions,
        }
    }
}

fn invalid_metadata() -> ManagementError {
    ManagementError::Failed(i18n::tr!("management-brew-invalid-metadata"))
}

fn text<'a>(value: &'a Value, key: &str) -> Result<&'a str, ManagementError> {
    value
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(invalid_metadata)
}

fn catalog(output: &str) -> Result<Value, ManagementError> {
    let value: Value = serde_json::from_str(output).map_err(|_| invalid_metadata())?;
    if ["formulae", "casks"]
        .iter()
        .all(|key| value[*key].is_array())
    {
        Ok(value)
    } else {
        Err(invalid_metadata())
    }
}

pub(super) fn parse_info(output: &str) -> Result<Vec<Package>, ManagementError> {
    let value = catalog(output)?;
    let mut packages = Vec::new();
    for (kind, key) in [
        (PackageType::Formula, "formulae"),
        (PackageType::Cask, "casks"),
    ] {
        for record in value[key].as_array().unwrap() {
            let name_key = if kind == PackageType::Formula {
                "full_name"
            } else {
                "full_token"
            };
            let name = text(record, name_key)?;
            validate_name(name)?;
            let mut package = Package::search_result(kind, name);
            if kind == PackageType::Formula {
                package.installed = record["installed"]
                    .as_array()
                    .ok_or_else(invalid_metadata)?
                    .iter()
                    .map(|installed| text(installed, "version").map(str::to_string))
                    .collect::<Result<Vec<_>, _>>()?
                    .join(", ");
                package.available = record["versions"]["stable"]
                    .as_str()
                    .or_else(|| record["versions"]["head"].as_str())
                    .unwrap_or("")
                    .into();
                if let Some(revision) = record["revision"].as_u64().filter(|revision| *revision > 0)
                {
                    package.available.push_str(&format!("_{revision}"));
                }
            } else {
                package.installed = match &record["installed"] {
                    Value::Null => String::new(),
                    Value::String(version) => version.clone(),
                    _ => return Err(invalid_metadata()),
                };
                package.available = text(record, "version")?.into();
            }
            package.description = record["desc"].as_str().unwrap_or("").into();
            package.homepage = record["homepage"].as_str().unwrap_or("").into();
            package.tap = record["tap"].as_str().unwrap_or("").into();
            package.pinned = record["pinned"].as_bool().unwrap_or(false);
            packages.push(package);
        }
    }
    Ok(packages)
}

pub(super) fn parse_outdated(output: &str) -> Result<Vec<Package>, ManagementError> {
    let value = catalog(output)?;
    let mut packages = Vec::new();
    for (kind, key) in [
        (PackageType::Formula, "formulae"),
        (PackageType::Cask, "casks"),
    ] {
        for record in value[key].as_array().unwrap() {
            let name = text(record, "name")?;
            validate_name(name)?;
            let mut package = Package::search_result(kind, name);
            package.installed = record["installed_versions"]
                .as_array()
                .ok_or_else(invalid_metadata)?
                .iter()
                .map(|version| {
                    version
                        .as_str()
                        .map(str::to_string)
                        .ok_or_else(invalid_metadata)
                })
                .collect::<Result<Vec<_>, _>>()?
                .join(", ");
            package.available = text(record, "current_version")?.into();
            package.pinned = record["pinned"].as_bool().unwrap_or(false);
            packages.push(package);
        }
    }
    Ok(packages)
}
