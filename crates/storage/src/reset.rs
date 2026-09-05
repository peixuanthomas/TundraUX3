use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use platform::AppPaths;

use crate::StorageManager;

/// The result of removing all TundraUX3-owned saved content and recreating
/// the default storage documents.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StorageResetReport {
    pub removed_paths: Vec<PathBuf>,
}

/// Removes only the paths explicitly owned by `AppPaths`, then recreates an
/// empty, valid storage layout.
///
/// All candidates are validated before the first filesystem mutation so an
/// invalid layout cannot produce a partial reset.
pub fn reset_saved_content(paths: &AppPaths) -> Result<StorageResetReport, io::Error> {
    let candidates = [
        paths.config_path(),
        paths.data_path(),
        paths.cache_path(),
        paths.logs_path(),
        paths.temp_path(),
    ];

    validate_candidates(&candidates)?;

    let mut removed_paths = Vec::new();
    for path in candidates {
        if path.try_exists()? {
            remove_path(path)?;
            removed_paths.push(path.to_path_buf());
        }
    }

    StorageManager::open(paths.clone()).map_err(|error| io::Error::other(error.to_string()))?;

    Ok(StorageResetReport { removed_paths })
}

fn validate_candidates(candidates: &[&Path]) -> Result<(), io::Error> {
    for path in candidates {
        guard_reset_path(path)?;
    }

    for (index, first) in candidates.iter().enumerate() {
        for second in candidates.iter().skip(index + 1) {
            if first == second || first.starts_with(second) || second.starts_with(first) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!(
                        "refusing to reset overlapping paths {} and {}",
                        first.display(),
                        second.display()
                    ),
                ));
            }
        }
    }

    Ok(())
}

fn guard_reset_path(path: &Path) -> Result<(), io::Error> {
    if !path.is_absolute() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("refusing to reset non-absolute path {}", path.display()),
        ));
    }

    if path.parent().is_none() || path.file_name().is_none() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("refusing to reset root path {}", path.display()),
        ));
    }

    Ok(())
}

fn remove_path(path: &Path) -> Result<(), io::Error> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() {
        remove_symlink(path, &metadata.file_type())
    } else if metadata.is_file() {
        fs::remove_file(path)
    } else {
        fs::remove_dir_all(path)
    }
}

#[cfg(windows)]
fn remove_symlink(path: &Path, file_type: &fs::FileType) -> Result<(), io::Error> {
    use std::os::windows::fs::FileTypeExt;

    if file_type.is_symlink_dir() {
        fs::remove_dir(path)
    } else {
        fs::remove_file(path)
    }
}

#[cfg(not(windows))]
fn remove_symlink(path: &Path, _file_type: &fs::FileType) -> Result<(), io::Error> {
    fs::remove_file(path)
}

#[cfg(test)]
#[path = "tests/reset.rs"]
mod tests;
