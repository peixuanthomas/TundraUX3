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
mod tests {
    use super::*;
    use platform::AppPaths;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(1);

    struct FixtureRoot(PathBuf);

    impl FixtureRoot {
        fn new(label: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "tundra-storage-reset-{label}-{}-{}",
                std::process::id(),
                NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(&path).expect("fixture root");
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for FixtureRoot {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn fixture_paths(root: &Path) -> AppPaths {
        AppPaths::from_parts(
            root.join("config").join("config.toml"),
            root.join("data"),
            root.join("cache"),
            root.join("logs"),
            root.join("temp"),
        )
        .expect("absolute fixture paths")
    }

    #[test]
    fn reset_removes_owned_content_and_recreates_defaults() {
        let fixture = FixtureRoot::new("recreate");
        let paths = fixture_paths(fixture.path());
        StorageManager::open(paths.clone()).expect("initial storage");
        fs::write(paths.data_path().join("private.txt"), "saved").expect("saved content");
        fs::create_dir_all(paths.cache_path()).expect("cache");
        fs::write(paths.cache_path().join("cache.bin"), b"cache").expect("cache content");

        let report = reset_saved_content(&paths).expect("reset");

        assert!(
            report
                .removed_paths
                .contains(&paths.config_path().to_path_buf())
        );
        assert!(
            report
                .removed_paths
                .contains(&paths.data_path().to_path_buf())
        );
        assert!(!paths.data_path().join("private.txt").exists());
        assert!(paths.config_path().exists());
        assert!(paths.data_path().exists());
        assert!(StorageManager::open(paths).is_ok());
    }

    #[test]
    fn validation_rejects_overlapping_candidates_before_deleting() {
        let fixture = FixtureRoot::new("overlap");
        let root = fixture.path();
        let marker = root.join("owned").join("marker.txt");
        fs::create_dir_all(marker.parent().expect("parent")).expect("directory");
        fs::write(&marker, "keep").expect("marker");
        let paths = AppPaths::from_parts(
            root.join("owned"),
            root.join("owned").join("data"),
            root.join("cache"),
            root.join("logs"),
            root.join("temp"),
        )
        .expect("absolute paths");

        let error = reset_saved_content(&paths).expect_err("overlap must fail");

        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
        assert!(marker.exists(), "validation must happen before deletion");
    }

    #[cfg(unix)]
    #[test]
    fn reset_unlinks_owned_directory_symlinks_without_following_them() {
        let fixture = FixtureRoot::new("symlink");
        let paths = fixture_paths(fixture.path());
        StorageManager::open(paths.clone()).expect("initial storage");
        let external = fixture.path().join("outside-owned-paths");
        fs::create_dir_all(&external).expect("external directory");
        let marker = external.join("keep.txt");
        fs::write(&marker, "keep").expect("external marker");
        if paths.cache_path().try_exists().expect("cache exists check") {
            fs::remove_dir_all(paths.cache_path()).expect("remove cache directory");
        }
        std::os::unix::fs::symlink(&external, paths.cache_path()).expect("cache symlink");

        reset_saved_content(&paths).expect("reset");

        assert!(marker.exists(), "reset must not follow directory symlinks");
        assert!(!paths.cache_path().is_symlink());
    }
}
