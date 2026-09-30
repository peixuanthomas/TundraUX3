use crate::installation::{Installation, PORTABLE_MARKER, UpdateBackend};
use crate::service::ServiceError;
use std::io::Read;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::Path;

pub fn detect_current() -> Installation {
    let result: Result<Installation, String> = (|| {
        let user =
            super::identity::LinuxUserContext::current().map_err(|error| error.to_string())?;
        let executable = std::env::current_exe()
            .and_then(|path| path.canonicalize())
            .map_err(|error| error.to_string())?;
        let directory = executable
            .parent()
            .ok_or("Executable has no installation directory")?
            .to_path_buf();
        validate_portable_directory(&directory, user.process.uid).map_err(|error| {
            format!("Updates require a writable, user-owned portable installation: {error}")
        })?;
        Ok(Installation {
            backend: UpdateBackend::PortableUser,
            directory: Some(directory),
            reason: None,
        })
    })();
    result.unwrap_or_else(Installation::unavailable)
}

/// Used again immediately before staging/recovery; never establishes privilege.
pub fn require_portable_directory(directory: &Path) -> Result<(), ServiceError> {
    let user =
        super::identity::LinuxUserContext::current().map_err(|_| ServiceError::PermissionDenied)?;
    validate_portable_directory(directory, user.process.uid)?;
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
