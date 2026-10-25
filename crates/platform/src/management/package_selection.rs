use super::{ManagementCommand, ManagementError, ManagementKind};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const KEY: &str = "package_selection";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackageTarget {
    pub id: String,
    pub identity: BTreeMap<String, String>,
}

pub fn expand(command: &ManagementCommand) -> Result<Vec<ManagementCommand>, ManagementError> {
    let Some(selection) = command.values.get(KEY) else {
        return Ok(vec![command.clone()]);
    };
    let invalid =
        || ManagementError::InvalidInput(i18n::tr!("management-package-selection-invalid"));
    if command.kind != ManagementKind::Packages
        || !matches!(
            command.action.as_str(),
            "install" | "remove" | "upgrade" | "pacman_install" | "pacman_upgrade"
        )
        || command.target.is_some()
        || !command.identity.is_empty()
    {
        return Err(invalid());
    }
    let targets: Vec<PackageTarget> = serde_json::from_str(selection).map_err(|_| invalid())?;
    let mut seen = BTreeSet::new();
    if targets.is_empty()
        || targets
            .iter()
            .any(|target| target.id.is_empty() || !seen.insert(&target.id))
    {
        return Err(invalid());
    }
    let mut values = command.values.clone();
    values.remove(KEY);
    Ok(targets
        .into_iter()
        .map(|target| ManagementCommand {
            kind: command.kind,
            action: command.action.clone(),
            target: Some(target.id),
            identity: target.identity,
            values: values.clone(),
        })
        .collect())
}

#[cfg(test)]
#[path = "../../tests/unit/management/package_selection_tests.rs"]
mod tests;
