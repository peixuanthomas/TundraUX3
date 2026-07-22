use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use platform::{AppPaths, Platform};
use storage::{StorageLayout, StorageManager};

use crate::path_report::write_storage_files;

pub(crate) fn run_new<Stdout: Write, Stderr: Write>(
    platform: &dyn Platform,
    stdout: &mut Stdout,
    stderr: &mut Stderr,
) -> i32 {
    match platform.app_paths() {
        Ok(paths) => match reset_saved_content(&paths) {
            Ok(report) => {
                let _ = writeln!(stdout, "TundraUX3 storage reset");
                let _ = writeln!(stdout, "Removed paths:");
                for path in &report.removed_paths {
                    let _ = writeln!(stdout, "  {}", path.display());
                }
                let _ = writeln!(stdout);
                let _ = writeln!(stdout, "Recreated storage files:");
                let layout = StorageLayout::from_app_paths(&paths);
                write_storage_files(stdout, &layout);
                0
            }
            Err(error) => {
                let _ = writeln!(stderr, "ERROR: could not reset saved content: {error}");
                1
            }
        },
        Err(error) => {
            let _ = writeln!(stderr, "ERROR: {error}");
            1
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ResetReport {
    removed_paths: Vec<PathBuf>,
}

fn reset_saved_content(paths: &AppPaths) -> Result<ResetReport, std::io::Error> {
    let candidates = [
        paths.config_path(),
        paths.data_path(),
        paths.cache_path(),
        paths.logs_path(),
        paths.temp_path(),
    ];
    let mut removed_paths = Vec::new();

    for path in candidates {
        guard_reset_path(path)?;
        if path.exists() {
            remove_path(path)?;
            removed_paths.push(path.to_path_buf());
        }
    }

    StorageManager::open(paths.clone())
        .map_err(|error| std::io::Error::other(error.to_string()))?;

    Ok(ResetReport { removed_paths })
}

fn guard_reset_path(path: &Path) -> Result<(), std::io::Error> {
    if !path.is_absolute() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("refusing to reset non-absolute path {}", path.display()),
        ));
    }

    if path.parent().is_none() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("refusing to reset root path {}", path.display()),
        ));
    }

    Ok(())
}

fn remove_path(path: &Path) -> Result<(), std::io::Error> {
    if path.is_dir() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    }
}
