//! Repository package management. The privileged helper owns the running PTY.
//! Queries never refresh repositories or mutate the package database.

mod pacman;
mod parsing;
mod query;
mod runner;

use super::{
    ExecutionContext, ManagementCommand, ManagementError, ManagementQuery, ManagementSnapshot,
    OperationInteraction,
};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackageBackend {
    Apt,
    Dnf4,
    Dnf5,
    Pacman,
}

impl PackageBackend {
    pub fn id(self) -> &'static str {
        match self {
            Self::Apt => "apt/dpkg",
            Self::Dnf4 => "dnf4",
            Self::Dnf5 => "dnf5",
            Self::Pacman => "pacman",
        }
    }
    fn program(self) -> &'static str {
        match self {
            Self::Apt => "/usr/bin/apt-get",
            Self::Dnf4 => "/usr/bin/dnf",
            Self::Dnf5 => "/usr/bin/dnf5",
            Self::Pacman => "/usr/bin/pacman",
        }
    }
}

/// Backend choice comes from installed tools, never from caller-supplied executable paths.
pub fn detect_backend(cancelled: &AtomicBool) -> Result<PackageBackend, ManagementError> {
    if cancelled.load(Ordering::Acquire) {
        return Err(ManagementError::Cancelled);
    }
    let os = std::fs::read_to_string("/etc/os-release").unwrap_or_default();
    let dnf = Path::new("/usr/bin/dnf").is_file();
    let dnf_alias_is_5 = dnf
        && std::fs::canonicalize("/usr/bin/dnf")
            .ok()
            .and_then(|path| path.file_name().map(|name| name == "dnf5"))
            .unwrap_or(false);
    choose_backend(
        &os,
        apt_available(),
        Path::new("/usr/bin/pacman").is_file(),
        Path::new("/usr/bin/dnf5").is_file(),
        dnf,
        dnf_alias_is_5,
    )
}

fn os_family(os: &str, ids: &[&str]) -> bool {
    os.lines().any(|line| {
        let Some((key, value)) = line.trim().split_once('=') else {
            return false;
        };
        matches!(key, "ID" | "ID_LIKE")
            && value
                .trim()
                .trim_matches(['\'', '"'])
                .split_ascii_whitespace()
                .any(|id| ids.contains(&id))
    })
}

fn choose_backend(
    os: &str,
    apt: bool,
    pacman: bool,
    dnf5: bool,
    dnf: bool,
    dnf_alias_is_5: bool,
) -> Result<PackageBackend, ManagementError> {
    // An ancillary manager (for example APT used to build Debian packages)
    // must not replace the native Arch package database.
    if os_family(os, &["arch"]) {
        return if pacman {
            Ok(PackageBackend::Pacman)
        } else {
            Err(ManagementError::Unavailable(
                "Arch Linux package management requires /usr/bin/pacman".into(),
            ))
        };
    }
    if os_family(os, &["debian", "ubuntu"]) && apt {
        return Ok(PackageBackend::Apt);
    }
    if dnf5 {
        return Ok(PackageBackend::Dnf5);
    }
    if dnf {
        if dnf_alias_is_5 {
            // Some installations expose only the dnf alias.
            return Err(ManagementError::Unavailable(
                "DNF5 was found but /usr/bin/dnf5 is missing".into(),
            ));
        }
        return Ok(PackageBackend::Dnf4);
    }
    if apt {
        return Ok(PackageBackend::Apt);
    }
    if pacman {
        return Ok(PackageBackend::Pacman);
    }
    Err(ManagementError::Unavailable(
        "APT/dpkg, DNF4/DNF5 or pacman is required; PackageKit is not required".into(),
    ))
}

fn apt_available() -> bool {
    [
        "/usr/bin/apt-get",
        "/usr/bin/apt-cache",
        "/usr/bin/dpkg-query",
    ]
    .iter()
    .all(|path| Path::new(path).is_file())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PackageRecord {
    pub name: String,
    pub architecture: String,
    pub version: String,
    pub summary: String,
    pub description: String,
    pub repository: String,
    pub installed_version: Option<String>,
    pub details: Vec<(String, String)>,
}

impl PackageRecord {
    fn id(&self, backend: PackageBackend) -> String {
        if self.architecture.is_empty() || backend == PackageBackend::Pacman {
            self.name.clone()
        } else {
            format!(
                "{}{}{}",
                self.name,
                if backend == PackageBackend::Apt {
                    ':'
                } else {
                    '.'
                },
                self.architecture
            )
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PackageCommand {
    pub program: PathBuf,
    pub args: Vec<String>,
}

/// Only repository package names are accepted. Paths, globs, option names and
/// dependency expressions are not delegated to a privileged package manager.
pub fn validate_target(target: &str, backend: PackageBackend) -> Result<(), ManagementError> {
    let valid = !target.is_empty()
        && target.len() <= 256
        && if backend == PackageBackend::Pacman {
            // PKGBUILD permits @, _, + as initial characters, but never - or .
            !matches!(target.as_bytes()[0], b'-' | b'.')
                && target.bytes().all(|byte| {
                    byte.is_ascii_alphanumeric() || matches!(byte, b'@' | b'+' | b'-' | b'.' | b'_')
                })
        } else {
            target.as_bytes()[0].is_ascii_alphanumeric()
                && target.bytes().all(|byte| {
                    byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'-' | b'.' | b'_' | b':')
                })
                && (backend != PackageBackend::Apt
                    || target.bytes().filter(|b| *b == b':').count() <= 1)
                && (backend == PackageBackend::Apt || !target.contains(':'))
        };
    if valid {
        Ok(())
    } else {
        Err(ManagementError::InvalidInput(
            "Select a repository package name; options, paths and wildcards are not accepted"
                .into(),
        ))
    }
}

fn build_command(
    command: &ManagementCommand,
    backend: PackageBackend,
) -> Result<PackageCommand, ManagementError> {
    if !command.values.is_empty() {
        return Err(ManagementError::InvalidInput(
            "Package operations do not accept additional command options".into(),
        ));
    }
    if backend == PackageBackend::Pacman {
        return build_pacman_command(command);
    }
    let mut args: Vec<String> = match backend {
        PackageBackend::Apt => [
            "-o",
            "APT::Color=0",
            "-o",
            "Dpkg::Use-Pty=0",
            "-o",
            // fd 2 is stderr, already connected to the AutoAdmin terminal.
            // dpkg closes a status logger's stdout, so cat cannot forward it.
            "Dpkg::Options::=--status-fd=2",
            "-o",
            "DPkg::Lock::Timeout=30",
        ]
        .into_iter()
        .map(String::from)
        .collect(),
        PackageBackend::Dnf4 | PackageBackend::Dnf5 => vec!["--color=never".into()],
        PackageBackend::Pacman => unreachable!("pacman commands are built separately"),
    };
    match command.action.as_str() {
        "refresh" | "upgrade_all" => {
            if command
                .target
                .as_ref()
                .is_some_and(|target| !target.is_empty())
            {
                return Err(ManagementError::InvalidInput(
                    "This package operation does not take a target".into(),
                ));
            }
            if command.action == "refresh" {
                if backend == PackageBackend::Apt {
                    // APT otherwise exits zero after some repository download
                    // failures, which would turn a partial refresh into success.
                    args.extend(["-o".into(), "APT::Update::Error-Mode=any".into()]);
                    args.push("update".into());
                } else {
                    args.extend(["--refresh".into(), "makecache".into()]);
                }
            } else {
                if backend == PackageBackend::Apt {
                    // Permit dependencies needed by updates, while keeping the
                    // ordinary upgrade rule that existing packages are not removed.
                    args.push("--with-new-pkgs".into());
                }
                args.push("upgrade".into());
            }
        }
        "install" | "remove" | "upgrade" => {
            let target = command
                .target
                .as_deref()
                .ok_or_else(|| ManagementError::InvalidInput("Select a package first".into()))?;
            validate_target(target, backend)?;
            if command.action == "remove" && backend != PackageBackend::Apt {
                // Ordinary removal never expands into cleanup of unrelated orphan packages.
                args.push("--setopt=clean_requirements_on_remove=False".into());
            }
            if command.action == "upgrade" && backend == PackageBackend::Apt {
                args.extend(["install".into(), "--only-upgrade".into()]);
            } else {
                args.push(command.action.clone());
            }
            args.extend(["--".into(), target.into()]);
        }
        _ => {
            return Err(ManagementError::InvalidInput(
                "Unsupported package operation".into(),
            ));
        }
    }
    Ok(PackageCommand {
        program: backend.program().into(),
        args,
    })
}

fn build_pacman_command(command: &ManagementCommand) -> Result<PackageCommand, ManagementError> {
    let mut args = vec!["--color=never".into()];
    match command.action.as_str() {
        "pacman_upgrade_all" => {
            if command
                .target
                .as_ref()
                .is_some_and(|target| !target.is_empty())
            {
                return Err(ManagementError::InvalidInput(
                    "The full-system upgrade does not take a target".into(),
                ));
            }
            args.extend(["-Syu".into(), "--needed".into()]);
        }
        "pacman_install" | "pacman_upgrade" | "remove" => {
            let target = command
                .target
                .as_deref()
                .ok_or_else(|| ManagementError::InvalidInput("Select a package first".into()))?;
            validate_target(target, PackageBackend::Pacman)?;
            if command.action == "remove" {
                // No recursive/cascade removal, and retain backup config files.
                args.push("-R".into());
            } else {
                // Never create an unsupported partial upgrade with -Sy or a
                // targeted -S after refreshing databases. AA labels explicitly
                // disclose that installing/upgrading also upgrades the system.
                args.extend(["-Syu".into(), "--needed".into()]);
            }
            args.extend(["--".into(), target.into()]);
        }
        _ => {
            return Err(ManagementError::InvalidInput(
                "Arch package operations require an explicitly confirmed full-system upgrade; standalone repository refresh and partial upgrades are not supported".into(),
            ));
        }
    }
    Ok(PackageCommand {
        program: PackageBackend::Pacman.program().into(),
        args,
    })
}

pub fn query(
    request: &ManagementQuery,
    cancelled: &AtomicBool,
) -> Result<ManagementSnapshot, ManagementError> {
    query::query(request, cancelled)
}

pub fn execute(
    command: &ManagementCommand,
    context: &ExecutionContext,
    interaction: &mut dyn OperationInteraction,
    cancelled: &AtomicBool,
) -> Result<String, ManagementError> {
    if command.kind != super::ManagementKind::Packages {
        return Err(ManagementError::InvalidInput(
            "This operation is not a package operation".into(),
        ));
    }
    let backend = detect_backend(cancelled)?;
    if unsafe { libc::geteuid() } != 0 {
        return Err(ManagementError::PermissionDenied(
            "Package changes must be run by the authorized system helper".into(),
        ));
    }
    if let Some(expected) = command.identity.get("backend") {
        if expected != backend.id() {
            return Err(ManagementError::Conflict(
                "The package backend changed; refresh the list before continuing".into(),
            ));
        }
    }
    let spec = build_command(command, backend)?;
    if let Some(target) = command.target.as_deref() {
        query::validate_identity(
            target,
            backend,
            &command.action,
            &command.identity,
            cancelled,
        )?;
    }
    let _ = context;
    runner::execute(spec, backend, interaction, cancelled)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn command(action: &str, target: Option<&str>) -> ManagementCommand {
        ManagementCommand {
            kind: super::super::ManagementKind::Packages,
            action: action.into(),
            target: target.map(String::from),
            values: BTreeMap::new(),
            identity: BTreeMap::new(),
        }
    }

    #[test]
    fn package_targets_cannot_escape_to_options_or_paths() {
        for invalid in [
            "--allow-remove-essential",
            "./local.deb",
            "bash*",
            "foo;id",
            "foo\nbar",
            "foo=1.2",
            "",
            "foo/bar",
        ] {
            for backend in [
                PackageBackend::Apt,
                PackageBackend::Dnf4,
                PackageBackend::Dnf5,
                PackageBackend::Pacman,
            ] {
                assert!(
                    validate_target(invalid, backend).is_err(),
                    "accepted {invalid}"
                );
            }
        }
        assert!(validate_target("libc6:amd64", PackageBackend::Apt).is_ok());
        assert!(validate_target("libstdc++.x86_64", PackageBackend::Dnf5).is_ok());
        assert!(validate_target("libstdc++", PackageBackend::Pacman).is_ok());
        assert!(validate_target("bash:x86_64", PackageBackend::Pacman).is_err());
        for name in ["@demo", "_demo", "+demo", "foo@bar"] {
            assert!(validate_target(name, PackageBackend::Pacman).is_ok());
        }
        assert!(validate_target("@core", PackageBackend::Apt).is_err());
        assert!(validate_target("@core", PackageBackend::Dnf5).is_err());
    }

    #[test]
    fn command_never_bypasses_the_real_confirmation_or_runs_a_shell() {
        for backend in [
            PackageBackend::Apt,
            PackageBackend::Dnf4,
            PackageBackend::Dnf5,
        ] {
            for action in ["install", "remove", "upgrade"] {
                let spec = build_command(&command(action, Some("bash")), backend).unwrap();
                assert!(spec.program.is_absolute());
                assert!(!spec.args.iter().any(|arg| matches!(
                    arg.as_str(),
                    "-y" | "--assumeyes"
                        | "--yes"
                        | "--force-yes"
                        | "--allow-unauthenticated"
                        | "--purge"
                        | "autoremove"
                )));
                assert_eq!(&spec.args[spec.args.len() - 2..], &["--", "bash"]);
            }
        }
    }

    #[test]
    fn extra_options_and_targeted_global_operations_are_rejected() {
        let mut request = command("install", Some("bash"));
        request
            .values
            .insert("flags".into(), "--allow-unauthenticated".into());
        assert!(build_command(&request, PackageBackend::Apt).is_err());
        assert!(build_command(&command("upgrade_all", Some("bash")), PackageBackend::Apt).is_err());
        assert!(build_command(&command("purge", Some("bash")), PackageBackend::Apt).is_err());
    }

    #[test]
    fn arch_and_derivatives_prefer_the_native_backend() {
        for os in [
            "ID=arch\n",
            "ID=manjaro\nID_LIKE=\"arch\"\n",
            "ID=endeavouros\nID_LIKE='arch other'\n",
        ] {
            assert_eq!(
                choose_backend(os, true, true, true, true, false),
                Ok(PackageBackend::Pacman)
            );
            assert!(matches!(
                choose_backend(os, true, false, true, true, false),
                Err(ManagementError::Unavailable(_))
            ));
        }
        assert!(!os_family(
            "ID=archival\nID_LIKE=archlinux\nNAME=arch\n",
            &["arch"]
        ));
        assert!(!os_family("# ID=arch\nID_LIKE=notarch\n", &["arch"]));
        assert_eq!(
            choose_backend("ID=debian\n", true, true, true, true, false),
            Ok(PackageBackend::Apt)
        );
        assert_eq!(
            choose_backend("ID=fedora\n", false, true, true, true, false),
            Ok(PackageBackend::Dnf5)
        );
        assert_eq!(
            choose_backend("", false, true, false, false, false),
            Ok(PackageBackend::Pacman)
        );
        assert!(choose_backend("", false, false, false, true, true).is_err());
        assert!(choose_backend("", false, false, false, false, false).is_err());
        assert_eq!(
            detect_backend(&AtomicBool::new(true)),
            Err(ManagementError::Cancelled)
        );
    }

    #[test]
    fn pacman_uses_full_system_upgrades_and_nonrecursive_removal() {
        for action in ["pacman_install", "pacman_upgrade"] {
            let spec =
                build_command(&command(action, Some("libstdc++")), PackageBackend::Pacman).unwrap();
            assert_eq!(spec.program, PathBuf::from("/usr/bin/pacman"));
            assert_eq!(
                spec.args,
                ["--color=never", "-Syu", "--needed", "--", "libstdc++"]
            );
        }
        let all =
            build_command(&command("pacman_upgrade_all", None), PackageBackend::Pacman).unwrap();
        assert_eq!(all.args, ["--color=never", "-Syu", "--needed"]);
        let remove =
            build_command(&command("remove", Some("bash")), PackageBackend::Pacman).unwrap();
        assert_eq!(remove.args, ["--color=never", "-R", "--", "bash"]);
        for action in [
            "refresh",
            "install",
            "upgrade",
            "upgrade_all",
            "purge",
            "autoremove",
        ] {
            assert!(build_command(&command(action, Some("bash")), PackageBackend::Pacman).is_err());
        }
        assert!(
            build_command(
                &command("pacman_upgrade_all", Some("bash")),
                PackageBackend::Pacman
            )
            .is_err()
        );
        assert!(build_command(&command("pacman_install", None), PackageBackend::Pacman).is_err());
        let mut extra = command("pacman_install", Some("bash"));
        extra.values.insert("flags".into(), "--noconfirm".into());
        assert!(build_command(&extra, PackageBackend::Pacman).is_err());
        assert!(
            build_command(
                &command("pacman_install", Some("core/bash")),
                PackageBackend::Pacman
            )
            .is_err()
        );
        assert!(
            build_command(
                &command("pacman_install", Some("bash")),
                PackageBackend::Apt
            )
            .is_err()
        );
    }
}
