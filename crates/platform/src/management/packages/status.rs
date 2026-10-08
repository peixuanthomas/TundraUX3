//! Package locks, recovery guidance and preserved configuration candidates.
use super::{PackageBackend, query::run_read_command};
use crate::management::{
    ManagementAction, ManagementError, ManagementQuery, ManagementRow, ManagementSnapshot,
    OperationEvent, OperationInteraction,
};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageFailure {
    pub code: &'static str,
    pub summary: &'static str,
    pub next_step: &'static str,
    pub repair_actions: Vec<&'static str>,
}

pub fn classify_failure(backend: PackageBackend, output: &str) -> PackageFailure {
    let lower = output.to_ascii_lowercase();
    let has = |needles: &[&str]| needles.iter().any(|needle| lower.contains(needle));
    let (code, summary, next_step) = if has(&[
        "could not get lock",
        "unable to lock",
        "another app",
        "another process",
        "database is locked",
        "failed to lock database",
        "dnf is running",
    ]) {
        (
            "package_busy",
            "The package manager is busy.",
            "Wait for the current task, then check again.",
        )
    } else if has(&[
        "no space left",
        "not enough free disk",
        "insufficient disk",
        "disk full",
    ]) {
        (
            "package_space",
            "There is not enough disk space.",
            "Check disk space and file-count limits, then retry.",
        )
    } else if has(&[
        "no_pubkey",
        "gpg check failed",
        "invalid or corrupted package (pgp signature)",
        "signature",
        "not signed",
        "certificate verification failed",
    ]) {
        (
            "package_signature",
            "Package verification failed.",
            "Check the source, signing key and system clock.",
        )
    } else if has(&[
        "temporary failure resolving",
        "could not resolve",
        "failed to download",
        "could not connect",
        "network is unreachable",
        "connection timed out",
        "cannot download",
    ]) {
        (
            "package_network",
            "Package downloads failed.",
            "Check the network and software sources, then retry.",
        )
    } else if has(&[
        "unmet dependencies",
        "broken packages",
        "conflicting requests",
        "nothing provides",
        "could not satisfy dependencies",
        "dependency problems",
    ]) {
        (
            "package_dependencies",
            "Package dependencies could not be resolved.",
            if backend == PackageBackend::Apt {
                "Review the task output, then choose Repair dependencies."
            } else {
                "Check the database and sources; review conflicting packages before retrying."
            },
        )
    } else {
        (
            "package_partial",
            "The package operation failed.",
            if backend == PackageBackend::Apt {
                "Check the database; configure pending packages if the previous task was interrupted."
            } else {
                "Check the database and task output before retrying."
            },
        )
    };
    PackageFailure {
        code,
        summary,
        next_step,
        repair_actions: if backend == PackageBackend::Apt
            && matches!(code, "package_dependencies" | "package_partial")
        {
            vec!["check_database", "repair_configure", "repair_dependencies"]
        } else {
            vec!["check_database"]
        },
    }
}

pub(super) struct BusyStatus {
    pub busy: bool,
    pub message: String,
}

fn locks(backend: PackageBackend) -> &'static [&'static str] {
    match backend {
        PackageBackend::Apt => &[
            "/var/lib/dpkg/lock-frontend",
            "/var/lib/dpkg/lock",
            "/var/lib/apt/lists/lock",
            "/var/cache/apt/archives/lock",
        ],
        PackageBackend::Dnf4 | PackageBackend::Dnf5 => &[
            "/run/dnf.pid",
            "/var/run/dnf.pid",
            "/run/dnf5.pid",
            "/var/cache/dnf/metadata_lock.pid",
            "/var/lib/rpm/.rpm.lock",
        ],
        PackageBackend::Pacman => &["/var/lib/pacman/db.lck"],
    }
}

pub(super) fn busy_status(backend: PackageBackend) -> Result<BusyStatus, ManagementError> {
    let table = fs::read_to_string("/proc/locks").map_err(|error| {
        ManagementError::Unavailable(format!("Cannot inspect package locks: {error}"))
    })?;
    let mut owners = BTreeSet::new();
    let mut unknown = false;
    for path in locks(backend) {
        let metadata = match fs::metadata(path) {
            Ok(value) => value,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => {
                return Err(ManagementError::Unavailable(format!(
                    "Cannot inspect {path}: {error}"
                )));
            }
        };
        for pid in lock_owners(&table, metadata.dev(), metadata.ino()) {
            owners.insert(pid);
        }
        if backend == PackageBackend::Pacman {
            unknown = true;
        } else if path.ends_with(".pid") {
            if let Ok(pid) = fs::read_to_string(path)
                .unwrap_or_default()
                .trim()
                .parse::<u32>()
            {
                let executable = fs::read_link(format!("/proc/{pid}/exe")).ok();
                let arguments = fs::read(format!("/proc/{pid}/cmdline")).unwrap_or_default();
                if confirmed_dnf_process(executable.as_deref(), &arguments) {
                    owners.insert(pid);
                }
                // PID files may outlive a crashed DNF. Report the stale file separately,
                // without asserting that an unrelated reused PID owns the lock.
                else {
                    unknown = true;
                }
            } else {
                unknown = true;
            }
        }
    }
    let mut messages = Vec::new();
    for pid in owners {
        let cmd = fs::read_to_string(format!("/proc/{pid}/comm"))
            .map(|text| runtime_log::sanitize_text(text.trim()))
            .unwrap_or_else(|_| "name unavailable".into());
        messages.push(format!("PID {pid} ({cmd})"));
    }
    if unknown {
        messages.push("owner unknown; a package lock or stale PID file exists".into());
    }
    Ok(BusyStatus {
        busy: !messages.is_empty(),
        message: if messages.is_empty() {
            "Package manager is available".into()
        } else {
            format!("Package manager is occupied: {}.", messages.join(", "))
        },
    })
}

fn confirmed_dnf_process(executable: Option<&Path>, arguments: &[u8]) -> bool {
    match executable
        .and_then(Path::file_name)
        .and_then(|name| name.to_str())
    {
        Some("dnf" | "dnf5" | "dnf-3") => true,
        Some("python" | "python3") => arguments.split(|byte| *byte == 0).any(|argument| {
            matches!(
                argument,
                b"/usr/bin/dnf" | b"/usr/bin/dnf-3" | b"/usr/bin/dnf5"
            )
        }),
        _ => false,
    }
}

fn lock_owners(text: &str, device: u64, inode: u64) -> Vec<u32> {
    text.lines()
        .filter_map(|line| {
            let fields = line.split_whitespace().collect::<Vec<_>>();
            if fields.get(1) == Some(&"->") {
                return None;
            } // blocked waiter is not the owner
            let pid = fields.get(4)?.parse::<i64>().ok()?;
            let mut id = fields.get(5)?.split(':');
            let major = u32::from_str_radix(id.next()?, 16).ok()?;
            let minor = u32::from_str_radix(id.next()?, 16).ok()?;
            let ino = id.next()?.parse::<u64>().ok()?;
            (major == libc::major(device)
                && minor == libc::minor(device)
                && ino == inode
                && pid > 0)
                .then_some(pid as u32)
        })
        .collect()
}

pub(super) fn view_actions() -> Vec<ManagementAction> {
    [
        ("search", "Search packages"),
        ("installed", "Installed packages"),
        ("updates", "Available updates"),
        ("sources", "Software sources"),
        ("conflicts", "Configuration conflicts"),
        ("status", "Package status"),
    ]
    .into_iter()
    .map(|(scope, label)| ManagementAction {
        id: format!("scope_{scope}"),
        label: label.into(),
        values: BTreeMap::from([("scope".into(), scope.into())]),
        group: "view".into(),
        ..Default::default()
    })
    .collect()
}

fn recovery_actions(backend: PackageBackend) -> Vec<ManagementAction> {
    let mut actions = vec![ManagementAction {
        id: "check_database".into(),
        label: "Check package database".into(),
        group: "recovery".into(),
        ..Default::default()
    }];
    if backend == PackageBackend::Apt {
        for (id, label) in [
            ("repair_configure", "Configure pending packages"),
            ("repair_dependencies", "Repair dependencies"),
        ] {
            actions.push(ManagementAction {
                id: id.into(),
                label: label.into(),
                privileged: true,
                confirm: true,
                group: "recovery".into(),
                ..Default::default()
            });
        }
    }
    actions.extend(view_actions());
    actions
}

pub(super) fn query(
    request: &ManagementQuery,
    backend: PackageBackend,
    cancelled: &AtomicBool,
) -> Result<ManagementSnapshot, ManagementError> {
    if cancelled.load(Ordering::Acquire) {
        return Err(ManagementError::Cancelled);
    }
    if request.scope == "status" {
        let status = busy_status(backend)?;
        return Ok(ManagementSnapshot {
            columns: vec!["Status".into(), "Next step".into()],
            rows: vec![ManagementRow {
                id: "package-status".into(),
                cells: vec![
                    status.message.clone(),
                    if status.busy {
                        "Wait, then refresh"
                    } else {
                        "Check database after a failure"
                    }
                    .into(),
                ],
                detail: vec![(
                    "Lock safety".into(),
                    "Locks are inspected only; no lock is removed and no process is stopped".into(),
                )],
                ..Default::default()
            }],
            actions: recovery_actions(backend),
            backend: backend.id().into(),
            ..Default::default()
        });
    }
    let (candidates, denied, truncated) = conflict_files(Path::new("/etc"), cancelled)?;
    let filter = request.filter.to_ascii_lowercase();
    let rows = candidates.into_iter().filter(|path| filter.is_empty() || path.to_string_lossy().to_ascii_lowercase().contains(&filter)).map(|candidate| {
        let current = conflict_original(&candidate).unwrap();
        let values = BTreeMap::from([("path".into(), current.to_string_lossy().into_owned()), ("compare_path".into(), candidate.to_string_lossy().into_owned()), ("validator".into(), "auto".into())]);
        ManagementRow { id: candidate.to_string_lossy().into_owned(), cells: vec![current.to_string_lossy().into_owned(), candidate.to_string_lossy().into_owned()], detail: vec![("Current file".into(), current.to_string_lossy().into_owned()), ("Preserved configuration".into(), candidate.to_string_lossy().into_owned()), ("Merge guidance".into(), "Compare the preserved candidate, edit the current file and review the difference before saving".into())], actions: vec![ManagementAction { id: "compare_package_config".into(), label: "Compare configuration".into(), values, primary: true, ..Default::default() }], ..Default::default() }
    }).collect();
    let mut notices = Vec::new();
    if denied > 0 {
        notices.push(format!(
            "{denied} directories require permission; the conflict list may be incomplete"
        ));
    }
    if truncated {
        notices.push("Conflict scan limit reached; narrow the scan in the file manager".into());
    }
    Ok(ManagementSnapshot {
        columns: vec!["Current file".into(), "Preserved configuration".into()],
        rows,
        actions: recovery_actions(backend),
        notices,
        backend: backend.id().into(),
    })
}

fn conflict_original(path: &Path) -> Option<PathBuf> {
    let name = path.file_name()?.to_str()?;
    for suffix in [
        ".dpkg-dist",
        ".dpkg-old",
        ".dpkg-new",
        ".dpkg-bak",
        ".rpmnew",
        ".rpmsave",
        ".pacnew",
        ".pacsave",
    ] {
        if let Some(original) = name.strip_suffix(suffix) {
            if !original.is_empty() {
                return Some(path.with_file_name(original));
            }
        }
    }
    None
}

fn conflict_files(
    root: &Path,
    cancelled: &AtomicBool,
) -> Result<(Vec<PathBuf>, usize, bool), ManagementError> {
    let mut pending = vec![root.to_path_buf()];
    let mut files = Vec::new();
    let mut denied = 0;
    let mut visited = 0;
    while let Some(directory) = pending.pop() {
        if cancelled.load(Ordering::Acquire) {
            return Err(ManagementError::Cancelled);
        }
        let entries = match fs::read_dir(&directory) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::PermissionDenied => {
                denied += 1;
                continue;
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => return Err(ManagementError::Failed(error.to_string())),
        };
        for entry in entries {
            let entry = entry.map_err(|error| ManagementError::Failed(error.to_string()))?;
            visited += 1;
            if visited > 100000 || files.len() >= 2000 {
                files.sort();
                return Ok((files, denied, true));
            }
            let kind = entry
                .file_type()
                .map_err(|error| ManagementError::Failed(error.to_string()))?;
            if kind.is_dir() {
                pending.push(entry.path());
            } else if kind.is_file() && conflict_original(&entry.path()).is_some() {
                files.push(entry.path());
            }
            // Do not follow symlink trees, mount redirects, or candidate symlinks.
        }
    }
    files.sort();
    Ok((files, denied, false))
}

pub(super) fn check_database(
    backend: PackageBackend,
    interaction: &mut dyn OperationInteraction,
    cancelled: &AtomicBool,
) -> Result<String, ManagementError> {
    let (program, args): (&str, Vec<String>) = match backend {
        PackageBackend::Apt => ("/usr/bin/dpkg", vec!["--audit".into()]),
        PackageBackend::Dnf4 => ("/usr/bin/dnf", vec!["--cacheonly".into(), "check".into()]),
        PackageBackend::Dnf5 => ("/usr/bin/dnf5", vec!["--cacheonly".into(), "check".into()]),
        PackageBackend::Pacman => (
            "/usr/bin/pacman",
            vec!["-Dk".into(), "--color=never".into()],
        ),
    };
    let output = run_read_command(program, &args, cancelled)?;
    if !output.trim().is_empty() {
        interaction.emit(OperationEvent::Output {
            text: runtime_log::sanitize_text(&output),
        });
        return Ok("Database check completed; review its findings before making changes".into());
    }
    Ok("Database check found no reported problems".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn every_package_subview_offers_the_same_six_read_only_destinations() {
        let actions = view_actions();
        assert_eq!(
            actions
                .iter()
                .map(|action| action.id.as_str())
                .collect::<Vec<_>>(),
            [
                "scope_search",
                "scope_installed",
                "scope_updates",
                "scope_sources",
                "scope_conflicts",
                "scope_status"
            ]
        );
        for action in &actions {
            assert!(!action.privileged && !action.confirm);
            assert!(action.fields.is_empty());
            assert_eq!(
                action.values["scope"],
                action.id.strip_prefix("scope_").unwrap()
            );
            assert_eq!(action.group, "view");
        }
        for backend in [
            PackageBackend::Apt,
            PackageBackend::Dnf4,
            PackageBackend::Dnf5,
            PackageBackend::Pacman,
        ] {
            let recovery = recovery_actions(backend);
            for destination in &actions {
                assert_eq!(
                    recovery
                        .iter()
                        .filter(|action| action.id == destination.id)
                        .count(),
                    1
                );
            }
        }
    }
    #[test]
    fn stale_dnf_pid_never_identifies_an_unrelated_python_process() {
        let executable = Some(Path::new("/usr/bin/python3"));
        assert!(!confirmed_dnf_process(executable, b"python3\0server.py\0"));
        assert!(!confirmed_dnf_process(executable, b"python3\0/tmp/dnf\0"));
        assert!(confirmed_dnf_process(
            executable,
            b"python3\0/usr/bin/dnf\0upgrade\0"
        ));
        assert!(!confirmed_dnf_process(None, b"/usr/bin/dnf\0"));
    }
    #[test]
    fn lock_files_are_not_treated_as_busy_without_a_real_owner() {
        let device = libc::makedev(8, 1);
        let table = "1: POSIX ADVISORY WRITE 1234 08:01:555 0 EOF\n2: -> POSIX ADVISORY WRITE 9999 08:01:555 0 EOF\n3: POSIX ADVISORY WRITE 77 08:01:666 0 EOF\n";
        assert_eq!(lock_owners(table, device, 555), [1234]);
        assert!(lock_owners(table, device, 42).is_empty());
    }
    #[test]
    fn failure_categories_provide_specific_next_steps_without_unsafe_repairs() {
        for (output, code) in [
            ("Could not get lock /var/lib/dpkg/lock", "package_busy"),
            ("Temporary failure resolving archive", "package_network"),
            ("NO_PUBKEY AABB", "package_signature"),
            ("unmet dependencies", "package_dependencies"),
            ("No space left on device", "package_space"),
            ("maintainer script failed", "package_partial"),
        ] {
            let failure = classify_failure(PackageBackend::Apt, output);
            assert_eq!(failure.code, code);
            assert!(!failure.next_step.is_empty());
        }
        assert_eq!(
            classify_failure(PackageBackend::Pacman, "conflicting requests").repair_actions,
            ["check_database"]
        );
    }
    #[test]
    fn conflict_inventory_keeps_all_native_candidates_and_skips_symlinks() {
        let directory = tempfile::tempdir().unwrap();
        for suffix in [
            "dpkg-dist",
            "dpkg-old",
            "rpmnew",
            "rpmsave",
            "pacnew",
            "pacsave",
        ] {
            fs::write(directory.path().join(format!("demo.conf.{suffix}")), "new").unwrap();
        }
        std::os::unix::fs::symlink("/etc", directory.path().join("outside")).unwrap();
        let (files, denied, truncated) =
            conflict_files(directory.path(), &AtomicBool::new(false)).unwrap();
        assert_eq!(files.len(), 6);
        assert_eq!(denied, 0);
        assert!(!truncated);
        assert!(
            files
                .iter()
                .all(|file| conflict_original(file).unwrap().ends_with("demo.conf"))
        );
    }
}
