use crate::installation::{Installation, PORTABLE_MARKER, RpmIdentity, UpdateBackend};
use crate::service::ServiceError;
use std::io::Read;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const RPM: &str = "/usr/bin/rpm";
pub const PACKAGE_NAME: &str = "tundraux3";

pub fn detect_current() -> Installation {
    let result = (|| {
        let user =
            super::identity::LinuxUserContext::current().map_err(|error| error.to_string())?;
        let executable = std::env::current_exe()
            .and_then(|path| path.canonicalize())
            .map_err(|error| error.to_string())?;
        let directory = executable
            .parent()
            .ok_or("Executable has no installation directory")?
            .to_path_buf();
        classify_installation(
            &directory,
            rpm_owner(&executable),
            Path::new(RPM).exists(),
            is_fedora(),
            || validate_portable_directory(&directory, user.process.uid),
        )
    })();
    result.unwrap_or_else(Installation::unavailable)
}

fn classify_installation(
    directory: &Path,
    owner: Result<Option<RpmIdentity>, ServiceError>,
    rpm_available: bool,
    fedora: bool,
    portable: impl FnOnce() -> Result<(), ServiceError>,
) -> Result<Installation, String> {
    if let Ok(Some(rpm)) = owner {
        if rpm.name != PACKAGE_NAME {
            return Err("Executable belongs to another RPM package".into());
        }
        if !fedora {
            return Err("System package updates are supported only on Fedora".into());
        }
        return Ok(Installation {
            backend: UpdateBackend::SystemRpm,
            directory: Some(directory.to_owned()),
            rpm: Some(rpm),
            reason: None,
        });
    }
    if rpm_available && owner.is_err() {
        return Err("RPM executable ownership could not be verified".into());
    }
    portable().map_err(|error| error.to_string())?;
    Ok(Installation {
        backend: UpdateBackend::PortableUser,
        directory: Some(directory.to_owned()),
        rpm: None,
        reason: None,
    })
}

pub fn is_fedora() -> bool {
    std::fs::read_to_string("/etc/os-release").is_ok_and(|text| {
        text.lines()
            .any(|line| matches!(line, "ID=fedora" | "ID=\"fedora\""))
    })
}

fn parse_rpm(text: &str) -> Result<RpmIdentity, ServiceError> {
    let mut lines = text.lines();
    let fields: Vec<_> = lines
        .next()
        .ok_or(ServiceError::Unknown)?
        .split('\t')
        .collect();
    if fields.len() != 3
        || fields
            .iter()
            .any(|value| value.is_empty() || value.chars().any(char::is_control))
        || lines.next().is_some()
    {
        return Err(ServiceError::Unknown);
    }
    Ok(RpmIdentity {
        name: fields[0].into(),
        version: fields[1].into(),
        architecture: fields[2].into(),
    })
}

fn rpm_query(args: &[&std::ffi::OsStr]) -> Result<Option<RpmIdentity>, ServiceError> {
    let mut child = Command::new(RPM)
        .args(["--query", "--queryformat", "%{NAME}\t%{EVR}\t%{ARCH}\n"])
        .args(args)
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| ServiceError::ServiceUnavailable)?;
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if child
            .try_wait()
            .map_err(|_| ServiceError::Unknown)?
            .is_some()
        {
            break;
        }
        if Instant::now() >= deadline {
            // This is our read-only query process, never a package transaction.
            let _ = child.kill();
            let _ = child.wait();
            return Err(ServiceError::Timeout);
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let output = child
        .wait_with_output()
        .map_err(|_| ServiceError::Unknown)?;
    if output.status.code() == Some(1) {
        let expected = if args
            .first()
            .is_some_and(|arg| *arg == std::ffi::OsStr::new("--file"))
        {
            format!(
                "file {} is not owned by any package\n",
                args[1].to_string_lossy()
            )
        } else {
            format!("package {PACKAGE_NAME} is not installed\n")
        };
        return if output.stdout == expected.as_bytes() {
            Ok(None)
        } else {
            Err(ServiceError::Unknown)
        };
    }
    if !output.status.success() {
        return Err(ServiceError::Unknown);
    }
    parse_rpm(std::str::from_utf8(&output.stdout).map_err(|_| ServiceError::Unknown)?).map(Some)
}

fn rpm_owner(executable: &Path) -> Result<Option<RpmIdentity>, ServiceError> {
    rpm_query(&[std::ffi::OsStr::new("--file"), executable.as_os_str()])
}

pub fn installed_rpm() -> Result<Option<RpmIdentity>, ServiceError> {
    rpm_query(&[
        std::ffi::OsStr::new("--"),
        std::ffi::OsStr::new(PACKAGE_NAME),
    ])
}

/// Used again immediately before staging/recovery; never establishes privilege.
pub fn require_portable_directory(directory: &Path) -> Result<(), ServiceError> {
    let user =
        super::identity::LinuxUserContext::current().map_err(|_| ServiceError::PermissionDenied)?;
    validate_portable_directory(directory, user.process.uid)?;
    // RPM reports canonical file names, including when a caller used `..`.
    // Validate the supplied directory first so a symlink is not accepted as a marker root.
    let directory = directory
        .canonicalize()
        .map_err(|_| ServiceError::Unsupported)?;
    for name in ["tundra-shell", "tundra-cli"] {
        if Path::new(RPM).exists() && rpm_owner(&directory.join(name))?.is_some() {
            return Err(ServiceError::Unsupported);
        }
    }
    Ok(())
}

fn validate_portable_directory(directory: &Path, uid: u32) -> Result<(), ServiceError> {
    if !directory.is_absolute() {
        return Err(ServiceError::Unsupported);
    }
    if uid == 0 {
        return Err(ServiceError::PermissionDenied);
    }
    let root = std::fs::symlink_metadata(directory).map_err(|_| ServiceError::Unsupported)?;
    if !root.is_dir() || root.uid() != uid || root.permissions().mode() & 0o200 == 0 {
        return Err(ServiceError::PermissionDenied);
    }
    require_access(directory, libc::W_OK | libc::X_OK)?;
    for name in [PORTABLE_MARKER, "tundra-shell", "tundra-cli"] {
        let meta = std::fs::symlink_metadata(directory.join(name))
            .map_err(|_| ServiceError::Unsupported)?;
        if !meta.is_file() || meta.uid() != uid || meta.permissions().mode() & 0o200 == 0 {
            return Err(ServiceError::PermissionDenied);
        }
        require_access(&directory.join(name), libc::W_OK)?;
    }
    let mut contents = Vec::new();
    std::fs::File::open(directory.join(PORTABLE_MARKER))
        .map_err(|_| ServiceError::Unsupported)?
        .take(1025)
        .read_to_end(&mut contents)
        .map_err(|_| ServiceError::Unsupported)?;
    if contents.len() > 1024 {
        return Err(ServiceError::Unsupported);
    }
    let marker: serde_json::Value =
        serde_json::from_slice(&contents).map_err(|_| ServiceError::Unsupported)?;
    if marker.get("format").and_then(|v| v.as_u64()) != Some(1)
        || marker.get("kind").and_then(|v| v.as_str()) != Some("portable-user")
    {
        return Err(ServiceError::Unsupported);
    }
    Ok(())
}

fn require_access(path: &Path, mode: i32) -> Result<(), ServiceError> {
    let path = std::ffi::CString::new(path.as_os_str().as_bytes())
        .map_err(|_| ServiceError::Unsupported)?;
    // SAFETY: path is NUL-terminated. Real/effective IDs were checked before this probe.
    if unsafe { libc::access(path.as_ptr(), mode) } == 0 {
        Ok(())
    } else {
        Err(ServiceError::PermissionDenied)
    }
}

#[cfg(test)]
#[path = "../../tests/unit/linux/installation/tests.rs"]
mod tests;
