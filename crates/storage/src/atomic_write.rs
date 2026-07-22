use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::error::StorageError;

pub(crate) fn create_dir(path: &Path, operation: &'static str) -> Result<(), StorageError> {
    fs::create_dir_all(path).map_err(|error| StorageError::Io {
        operation,
        path: path.to_path_buf(),
        message: error.to_string(),
    })
}

pub(crate) fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), StorageError> {
    let parent = path.parent().ok_or_else(|| StorageError::MissingParent {
        path: path.to_path_buf(),
    })?;
    create_dir(parent, "create storage parent directory")?;

    for _ in 0..64 {
        let temp_path = temp_write_path(path);
        let mut file = match OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temp_path)
        {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(StorageError::Io {
                    operation: "create temporary storage file",
                    path: temp_path,
                    message: error.to_string(),
                });
            }
        };

        if let Err(error) = write_and_sync(&mut file, bytes) {
            let _ = fs::remove_file(&temp_path);
            return Err(StorageError::Io {
                operation: "write temporary storage file",
                path: temp_path,
                message: error.to_string(),
            });
        }

        drop(file);

        if let Err(error) = replace_file(&temp_path, path) {
            let _ = fs::remove_file(&temp_path);
            return Err(StorageError::Io {
                operation: "replace storage file",
                path: path.to_path_buf(),
                message: error.to_string(),
            });
        }

        return Ok(());
    }

    Err(StorageError::Io {
        operation: "create temporary storage file",
        path: parent.to_path_buf(),
        message: "could not create a unique temporary file".to_string(),
    })
}

fn write_and_sync(file: &mut File, bytes: &[u8]) -> Result<(), std::io::Error> {
    file.write_all(bytes)?;
    file.sync_all()
}

fn temp_write_path(path: &Path) -> PathBuf {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let file_name = path
        .file_name()
        .map(|name| name.to_string_lossy())
        .unwrap_or_else(|| "document".into());

    parent.join(format!(
        ".{file_name}.tmp.{}.{}",
        process::id(),
        unix_nanos()
    ))
}

fn unix_nanos() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0)
}

#[cfg(not(windows))]
fn replace_file(from: &Path, to: &Path) -> Result<(), std::io::Error> {
    fs::rename(from, to)
}

#[cfg(windows)]
fn replace_file(from: &Path, to: &Path) -> Result<(), std::io::Error> {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;

    const MOVEFILE_REPLACE_EXISTING: u32 = 0x1;
    const MOVEFILE_WRITE_THROUGH: u32 = 0x8;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn MoveFileExW(
            existing_file_name: *const u16,
            new_file_name: *const u16,
            flags: u32,
        ) -> i32;
    }

    fn wide_null(value: &OsStr) -> Vec<u16> {
        value.encode_wide().chain(std::iter::once(0)).collect()
    }

    let from = wide_null(from.as_os_str());
    let to = wide_null(to.as_os_str());
    let result = unsafe {
        MoveFileExW(
            from.as_ptr(),
            to.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };

    if result == 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}
