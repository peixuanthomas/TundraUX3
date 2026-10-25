use super::{ManagementCommand, ManagementError, ManagementKind};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const KEY: &str = "selection";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Target {
    pub id: String,
    pub identity: BTreeMap<String, String>,
}

pub fn supports(kind: ManagementKind, action: &str) -> bool {
    match kind {
        ManagementKind::Packages => matches!(
            action,
            "install" | "remove" | "upgrade" | "pacman_install" | "pacman_upgrade"
        ),
        ManagementKind::Services => matches!(
            action,
            "start" | "stop" | "restart" | "reload" | "enable" | "disable"
        ),
        ManagementKind::Processes => matches!(action, "term" | "kill" | "stop" | "cont" | "nice"),
        ManagementKind::ScheduledTasks => matches!(action, "pause" | "resume" | "run_now"),
        _ => false,
    }
}

pub fn expand(command: &ManagementCommand) -> Result<Vec<ManagementCommand>, ManagementError> {
    let Some(selection) = command.values.get(KEY) else {
        return Ok(vec![command.clone()]);
    };
    let invalid = || ManagementError::InvalidInput(i18n::tr!("management-selection-invalid"));
    if !supports(command.kind, &command.action)
        || command.target.is_some()
        || !command.identity.is_empty()
    {
        return Err(invalid());
    }
    let targets: Vec<Target> = serde_json::from_str(selection).map_err(|_| invalid())?;
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
#[path = "../../tests/unit/management/selection_tests.rs"]
mod tests;
