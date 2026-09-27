//! Ubuntu/Arch local-package installation. Compilation never runs with elevated privileges.
use crate::installation::{Installation, PORTABLE_MARKER, UpdateBackend};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::process::Command;

pub const PACKAGE_NAME: &str = "tundraux3";

fn distribution_backend(text: &str) -> Option<UpdateBackend> {
    let id = text.lines().find_map(|line| line.strip_prefix("ID="))?;
    match id.trim_matches('"') {
        "ubuntu" => Some(UpdateBackend::SystemDeb),
        "arch" => Some(UpdateBackend::SystemArch),
        _ => None,
    }
}

fn query(program: &str, args: &[&str]) -> Result<std::process::Output, String> {
    Command::new(program)
        .args(args)
        .env("LC_ALL", "C")
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|error| format!("Could not run {program}: {error}"))
}

fn owned_by_tundra(backend: UpdateBackend, path: &Path) -> Result<bool, String> {
    let path = path.to_str().ok_or("Non-UTF-8 installation path")?;
    let result = match backend {
        UpdateBackend::SystemDeb => query("/usr/bin/dpkg-query", &["--search", path])?,
        UpdateBackend::SystemArch => query("/usr/bin/pacman", &["-Qqo", "--", path])?,
        _ => return Err("Unsupported local package format".into()),
    };
    if !result.status.success() {
        return Ok(false);
    }
    let text = String::from_utf8_lossy(&result.stdout);
    Ok(match backend {
        UpdateBackend::SystemDeb => {
            let expected = format!("{PACKAGE_NAME}: {path}");
            text.trim() == expected || text.trim() == format!("{PACKAGE_NAME}:amd64: {path}")
        }
        UpdateBackend::SystemArch => text.trim() == PACKAGE_NAME,
        _ => false,
    })
}

/// Reject collisions rather than overwriting manually installed files or another package.
pub fn validate_targets(backend: UpdateBackend) -> Result<(), String> {
    if !backend.builds_system_package() {
        return Err("Unsupported local package format".into());
    }
    for name in ["tundra-shell", "tundra-cli"] {
        let path = Path::new("/usr/bin").join(name);
        match std::fs::symlink_metadata(&path) {
            Ok(meta) if meta.is_file() && owned_by_tundra(backend, &path)? => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            _ => {
                return Err(format!(
                    "{} is not a regular file owned by the tundraux3 package; refusing to replace it",
                    path.display()
                ));
            }
        }
    }
    Ok(())
}

pub(super) fn detect(executable: &Path, uid: u32) -> Result<Option<Installation>, String> {
    let Some(backend) = distribution_backend(
        &std::fs::read_to_string("/etc/os-release").map_err(|e| e.to_string())?,
    ) else {
        return Ok(None);
    };
    let directory = executable.parent().ok_or("Missing executable directory")?;
    // Formal portable installations retain their existing file-replacement workflow.
    if directory.join(PORTABLE_MARKER).exists() {
        return Ok(None);
    }
    let system_install = owned_by_tundra(backend, executable)?;
    if !system_install {
        let meta = std::fs::metadata(executable).map_err(|e| e.to_string())?;
        if uid == 0
            || meta.uid() != uid
            || !meta.is_file()
            || !matches!(
                executable.file_name().and_then(|n| n.to_str()),
                Some("tundra-shell" | "tundra-cli")
            )
        {
            return Err("Source installation must be a tundra executable owned by the current ordinary user".into());
        }
    }
    if std::env::consts::ARCH != "x86_64" {
        return Err("Local package updates currently require x86_64".into());
    }
    validate_targets(backend)?;
    Ok(Some(Installation {
        backend,
        directory: Some(directory.to_owned()),
        rpm: None,
        reason: Some(if system_install {
            "Build a local package and upgrade it with the system package manager".into()
        } else {
            "Build a local package and install it to /usr/bin; the source build is kept unchanged"
                .into()
        }),
    }))
}

pub fn installed_version(backend: UpdateBackend) -> Result<String, String> {
    let output = match backend {
        UpdateBackend::SystemDeb => query(
            "/usr/bin/dpkg-query",
            &["-W", "-f=${Status}\t${Version}", PACKAGE_NAME],
        )?,
        UpdateBackend::SystemArch => query("/usr/bin/pacman", &["-Q", PACKAGE_NAME])?,
        _ => return Err("Unsupported local package format".into()),
    };
    let text = String::from_utf8_lossy(&output.stdout);
    let prefix = if backend == UpdateBackend::SystemDeb {
        "install ok installed\t"
    } else {
        "tundraux3 "
    };
    if output.status.success() {
        if let Some(version) = text.trim().strip_prefix(prefix) {
            if !version.is_empty() && !version.chars().any(char::is_whitespace) {
                return Ok(version.into());
            }
        }
    }
    Err("The package manager did not report a fully installed tundraux3 package".into())
}

/// Only the fixed native package manager receives privilege; no source build or script does.
pub fn install_command(backend: UpdateBackend, package: &Path) -> Result<Command, String> {
    if !package.is_absolute() || !std::fs::symlink_metadata(package).is_ok_and(|m| m.is_file()) {
        return Err("The prepared package must be an absolute regular file".into());
    }
    let mut command = Command::new("/usr/bin/sudo");
    command.arg("--");
    match backend {
        UpdateBackend::SystemDeb => {
            command.args([
                "/usr/bin/apt-get",
                "--no-remove",
                "--no-install-recommends",
                "install",
                "--",
            ]);
        }
        UpdateBackend::SystemArch => {
            command.args(["/usr/bin/pacman", "-U", "--"]);
        }
        _ => return Err("Unsupported local package format".into()),
    }
    command
        .arg(package)
        .current_dir("/")
        .env_remove("APT_CONFIG")
        .env_remove("DEBIAN_FRONTEND");
    Ok(command)
}

#[derive(Debug, Clone)]
pub struct PackageInstall {
    pub backend: UpdateBackend,
    pub path: PathBuf,
}

#[cfg(test)]
#[path = "../../tests/unit/linux/source_packages.rs"]
mod tests;
