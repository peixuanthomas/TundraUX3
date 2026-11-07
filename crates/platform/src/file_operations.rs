use std::fs;
use std::path::Path;

use crate::PlatformError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileIdentity {
    volume: u64,
    index: u64,
}

pub fn file_identity(path: &Path) -> Result<FileIdentity, PlatformError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let metadata = fs::symlink_metadata(path).map_err(|error| {
            PlatformError::from_io("inspect file identity", Some(path.to_path_buf()), &error)
        })?;
        Ok(FileIdentity {
            volume: metadata.dev(),
            index: metadata.ino(),
        })
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::Storage::FileSystem::{
            BY_HANDLE_FILE_INFORMATION, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
            GetFileInformationByHandle,
        };
        let read = || -> std::io::Result<FileIdentity> {
            let file = fs::OpenOptions::new()
                .read(true)
                .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
                .open(path)?;
            let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
            if unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut info) } == 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(FileIdentity {
                volume: u64::from(info.dwVolumeSerialNumber),
                index: u64::from(info.nFileIndexHigh) << 32 | u64::from(info.nFileIndexLow),
            })
        };
        read().map_err(|error| {
            PlatformError::from_io("inspect file identity", Some(path.to_path_buf()), &error)
        })
    }
}

/// Renames a path atomically, refusing to replace any existing destination.
pub fn rename_path_no_replace(source: &Path, target: &Path) -> Result<(), PlatformError> {
    #[cfg(target_os = "linux")]
    let result = crate::linux::rename_noreplace(source, target);
    #[cfg(target_os = "macos")]
    let result = crate::macos::macos_rename_exclusive(source, target);
    #[cfg(windows)]
    let result = {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::Storage::FileSystem::MoveFileExW;
        let source: Vec<u16> = source.as_os_str().encode_wide().chain(Some(0)).collect();
        let target: Vec<u16> = target.as_os_str().encode_wide().chain(Some(0)).collect();
        if unsafe { MoveFileExW(source.as_ptr(), target.as_ptr(), 0) } != 0 {
            Ok(())
        } else {
            Err(std::io::Error::last_os_error())
        }
    };
    result.map_err(|error| {
        if error.kind() == std::io::ErrorKind::CrossesDevices {
            PlatformError::CrossDevice {
                source: source.to_path_buf(),
                target: target.to_path_buf(),
                message: error.to_string(),
            }
        } else {
            PlatformError::from_io(
                "rename without replacing",
                Some(target.to_path_buf()),
                &error,
            )
        }
    })
}
