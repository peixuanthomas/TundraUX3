//! Homebrew packages use the invoking macOS account, including native Cask prompts.
pub(super) mod client;
mod metadata;
mod process;

use super::{
    ExecutionContext, ManagementAction, ManagementCommand, ManagementError, ManagementKind,
    ManagementQuery, ManagementSnapshot, OperationInteraction,
};
use metadata::{Package, PackageType, parse_info, parse_outdated};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

fn detect_brew() -> Result<PathBuf, ManagementError> {
    std::env::var_os("PATH")
        .into_iter()
        .flat_map(|path| std::env::split_paths(&path).collect::<Vec<_>>())
        .map(|path| path.join("brew"))
        .chain([
            PathBuf::from("/opt/homebrew/bin/brew"),
            PathBuf::from("/usr/local/bin/brew"),
        ])
        .find(|path| {
            path.metadata().is_ok_and(|metadata| {
                metadata.is_file() && metadata.permissions().mode() & 0o111 != 0
            })
        })
        .and_then(|path| std::path::absolute(path).ok())
        .ok_or_else(|| ManagementError::MissingDependency(i18n::tr!("management-brew-missing")))
}

fn check_cancelled(cancelled: &AtomicBool) -> Result<(), ManagementError> {
    if cancelled.load(Ordering::Acquire) {
        Err(ManagementError::Cancelled)
    } else {
        Ok(())
    }
}

fn parse_target(target: &str) -> Result<(PackageType, &str), ManagementError> {
    let (kind, name) = target.split_once(':').unwrap_or_default();
    let kind = match kind {
        "formula" => PackageType::Formula,
        "cask" => PackageType::Cask,
        _ => return Err(invalid_target()),
    };
    validate_name(name)?;
    Ok((kind, name))
}

fn validate_name(name: &str) -> Result<(), ManagementError> {
    let segments: Vec<_> = name.split('/').collect();
    if name.len() <= 256
        && matches!(segments.len(), 1 | 3)
        && !name.ends_with(".rb")
        && segments.iter().all(|segment| {
            segment
                .as_bytes()
                .first()
                .is_some_and(u8::is_ascii_alphanumeric)
                && segment.bytes().all(|byte| {
                    byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'-' | b'.' | b'_' | b'@')
                })
        })
    {
        Ok(())
    } else {
        Err(invalid_target())
    }
}

fn invalid_target() -> ManagementError {
    ManagementError::InvalidInput(i18n::tr!("management-brew-invalid-target"))
}

fn info(
    program: &Path,
    target: Option<(PackageType, &str)>,
    cancelled: &AtomicBool,
) -> Result<Vec<Package>, ManagementError> {
    let mut args = vec!["info".into(), "--json=v2".into()];
    if let Some((kind, name)) = target {
        args.extend([kind.flag().into(), "--".into(), name.into()]);
    } else {
        args.push("--installed".into());
    }
    parse_info(&process::read(program, &args, cancelled)?)
}

pub fn query(
    request: &ManagementQuery,
    cancelled: &AtomicBool,
) -> Result<ManagementSnapshot, ManagementError> {
    check_cancelled(cancelled)?;
    query_with_program(&detect_brew()?, request, cancelled)
}

fn query_with_program(
    program: &Path,
    request: &ManagementQuery,
    cancelled: &AtomicBool,
) -> Result<ManagementSnapshot, ManagementError> {
    check_cancelled(cancelled)?;
    let scope = if request.scope.is_empty() {
        "installed"
    } else {
        &request.scope
    };
    if request.kind != ManagementKind::Packages
        || !matches!(scope, "installed" | "updates" | "search")
        || !request.options.is_empty()
        || request.filter.len() > 512
        || request.filter.chars().any(char::is_control)
    {
        return Err(ManagementError::InvalidInput(i18n::tr!(
            "management-brew-invalid-query"
        )));
    }
    let mut packages = if let Some(target) = request.target.as_deref() {
        let target = parse_target(target)?;
        info(program, Some(target), cancelled)?
    } else if scope == "updates" {
        parse_outdated(&process::read(
            program,
            &["outdated".into(), "--json=v2".into()],
            cancelled,
        )?)?
    } else if scope == "search" {
        let mut results = Vec::new();
        if !request.filter.trim().is_empty() {
            let installed = info(program, None, cancelled)?;
            for kind in [PackageType::Formula, PackageType::Cask] {
                let output = process::read(
                    program,
                    &[
                        "search".into(),
                        kind.flag().into(),
                        "--".into(),
                        request.filter.clone(),
                    ],
                    cancelled,
                )?;
                for name in output
                    .lines()
                    .map(str::trim)
                    .filter(|name| !name.is_empty())
                {
                    validate_name(name)?;
                    results.push(
                        installed
                            .iter()
                            .find(|package| package.kind == kind && package.name == name)
                            .cloned()
                            .unwrap_or_else(|| Package::search_result(kind, name)),
                    );
                }
            }
        }
        results
    } else {
        info(program, None, cancelled)?
    };
    let filter = request.filter.to_lowercase();
    packages.retain(|package| {
        request
            .target
            .as_ref()
            .is_none_or(|target| package.id() == *target)
            && (scope == "search"
                || filter.is_empty()
                || format!("{} {}", package.name, package.description)
                    .to_lowercase()
                    .contains(&filter))
    });
    packages.sort_by_key(Package::id);
    packages.dedup_by(|a, b| a.id() == b.id());
    let mut notices = vec![i18n::tr!("management-brew-cache")];
    if scope == "search" && request.filter.trim().is_empty() {
        notices.push(i18n::tr!("management-brew-search-prompt"));
    }
    if packages.len() > 2_000 {
        packages.truncate(2_000);
        notices.push(i18n::tr!("management-brew-truncated"));
    }
    Ok(ManagementSnapshot {
        columns: vec![
            i18n::tr!("management-column-name"),
            i18n::tr!("management-column-type"),
            i18n::tr!("management-column-installed"),
            i18n::tr!("management-column-available"),
            i18n::tr!("management-column-description"),
        ],
        rows: packages.into_iter().map(Package::into_row).collect(),
        actions: [
            "scope_installed",
            "scope_updates",
            "scope_search",
            "refresh",
            "upgrade_all",
        ]
        .into_iter()
        .map(action)
        .collect(),
        notices,
        backend: "brew".into(),
        details_complete: scope == "installed" || request.target.is_some(),
        ..Default::default()
    })
}

fn action(id: &str) -> ManagementAction {
    ManagementAction {
        id: id.into(),
        label: i18n::tr!(format!("management-action-{}", id.replace('_', "-"))),
        requires_approval: !id.starts_with("scope_"),
        primary: matches!(id, "install" | "upgrade"),
        group: if id.starts_with("scope_") {
            "view"
        } else {
            "package"
        }
        .into(),
        ..Default::default()
    }
}

fn command_args(command: &ManagementCommand) -> Result<Vec<String>, ManagementError> {
    if command.kind != ManagementKind::Packages || !command.values.is_empty() {
        return Err(ManagementError::InvalidInput(i18n::tr!(
            "management-brew-invalid-command"
        )));
    }
    match command.action.as_str() {
        "refresh" | "upgrade_all" if command.target.is_none() => Ok(vec![
            if command.action == "refresh" {
                "update"
            } else {
                "upgrade"
            }
            .into(),
        ]),
        "install" | "remove" | "upgrade" => {
            let (kind, name) = parse_target(command.target.as_deref().ok_or_else(invalid_target)?)?;
            Ok(vec![
                if command.action == "remove" {
                    "uninstall"
                } else {
                    &command.action
                }
                .into(),
                kind.flag().into(),
                "--".into(),
                name.into(),
            ])
        }
        _ => Err(ManagementError::InvalidInput(i18n::tr!(
            "management-brew-invalid-command"
        ))),
    }
}

pub fn execute(
    command: &ManagementCommand,
    context: &ExecutionContext,
    interaction: &mut dyn OperationInteraction,
    cancelled: &AtomicBool,
) -> Result<String, ManagementError> {
    check_cancelled(cancelled)?;
    let uid = unsafe { libc::getuid() };
    if uid == 0
        || uid != unsafe { libc::geteuid() }
        || context.actor_uid != uid
        || unsafe { libc::getgid() } != unsafe { libc::getegid() }
    {
        return Err(ManagementError::PermissionDenied(i18n::tr!(
            "management-brew-current-user"
        )));
    }
    execute_with_program(&detect_brew()?, command, interaction, cancelled)
}

fn execute_with_program(
    program: &Path,
    command: &ManagementCommand,
    interaction: &mut dyn OperationInteraction,
    cancelled: &AtomicBool,
) -> Result<String, ManagementError> {
    let commands = super::package_selection::expand(command)?;
    let groups = command_groups(&commands)?;
    for command in &commands {
        if let Some(target) = command.target.as_deref() {
            let current = info(program, Some(parse_target(target)?), cancelled)?
                .into_iter()
                .find(|package| package.id() == target)
                .ok_or_else(|| ManagementError::Conflict(i18n::tr!("management-brew-changed")))?;
            let identity = current.identity();
            if command
                .identity
                .iter()
                .any(|(key, expected)| identity.get(key) != Some(expected))
            {
                return Err(ManagementError::Conflict(i18n::tr!(
                    "management-brew-changed"
                )));
            }
        } else if command
            .identity
            .get("backend")
            .is_some_and(|backend| backend != "brew")
        {
            return Err(ManagementError::Conflict(i18n::tr!(
                "management-brew-changed"
            )));
        }
    }
    let mut message = String::new();
    for args in groups {
        check_cancelled(cancelled)?;
        message = process::execute(program, &args, interaction, cancelled)?;
    }
    Ok(message)
}

fn command_groups(commands: &[ManagementCommand]) -> Result<Vec<Vec<String>>, ManagementError> {
    let mut groups: Vec<Vec<String>> = Vec::new();
    for command in commands {
        let args = command_args(command)?;
        if command.target.is_some()
            && let Some(group) = groups.iter_mut().find(|group| group[..2] == args[..2])
        {
            group.push(args.last().expect("package target").clone());
        } else {
            groups.push(args);
        }
    }
    Ok(groups)
}

#[cfg(test)]
#[path = "../../../tests/unit/management/homebrew_tests.rs"]
mod tests;
