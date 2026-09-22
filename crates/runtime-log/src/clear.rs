//! Maintenance of log files in the configured Tundra log directory.
use crate::writer::{nofollow, retention_lock};
use fs2::FileExt;
use std::{
    fs::{self, OpenOptions},
    io,
    path::{Component, Path, PathBuf},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogFileType {
    Runtime,
    Incidents,
    Snapshots,
    Legacy,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LogClearTarget {
    All,
    Type(LogFileType),
    File(PathBuf),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogClearAction {
    Preview,
    Removed,
    Truncated,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogClearEntry {
    pub path: PathBuf,
    pub bytes: u64,
    pub action: LogClearAction,
}

#[derive(Debug, Default)]
pub struct LogClearReport {
    pub files: Vec<LogClearEntry>,
    pub failures: Vec<(PathBuf, String)>,
}

/// Preview or clear recognized log files only. Runtime segment clearing shares
/// the writer's reservation lock; active segments are truncated, never unlinked.
/// New events may be written after this operation. Application state, policy
/// files, arbitrary exports, and system journal files are outside this scope.
pub fn clear_logs(
    root: &Path,
    target: &LogClearTarget,
    execute: bool,
) -> io::Result<LogClearReport> {
    let mut report = LogClearReport::default();
    let candidates = match target {
        LogClearTarget::File(path) => {
            let relative = if path.is_absolute() {
                path.strip_prefix(root)
                    .map_err(|_| invalid("log file is outside the logs directory"))?
            } else {
                path.as_path()
            };
            if !relative
                .components()
                .all(|c| matches!(c, Component::Normal(_)))
                || classify(relative).is_none()
            {
                return Err(invalid(
                    "expected a runtime log, incident report, snapshot, or root .log file",
                ));
            }
            vec![root.join(relative)]
        }
        _ => {
            let mut paths = Vec::new();
            for relative in ["", "runtime", "crashes", "runtime/snapshots"] {
                let directory_type = match relative {
                    "runtime" => LogFileType::Runtime,
                    "crashes" => LogFileType::Incidents,
                    "runtime/snapshots" => LogFileType::Snapshots,
                    _ => LogFileType::Legacy,
                };
                if matches!(target, LogClearTarget::Type(kind) if *kind != directory_type) {
                    continue;
                }
                let directory = root.join(relative);
                match validate_directory(root, &directory) {
                    Ok(false) => continue,
                    Err(error) => {
                        report.failures.push((directory, error.to_string()));
                        continue;
                    }
                    Ok(true) => {}
                }
                match fs::read_dir(&directory) {
                    Ok(entries) => {
                        for entry in entries {
                            match entry {
                                Ok(entry) => {
                                    let path = entry.path();
                                    let Some(kind) = classify(
                                        path.strip_prefix(root).expect("entry below root"),
                                    ) else {
                                        continue;
                                    };
                                    if matches!(target, LogClearTarget::All)
                                        || *target == LogClearTarget::Type(kind)
                                    {
                                        paths.push(path);
                                    }
                                }
                                Err(error) => {
                                    report.failures.push((directory.clone(), error.to_string()))
                                }
                            }
                        }
                    }
                    Err(error) => report.failures.push((directory, error.to_string())),
                }
            }
            paths.sort();
            paths
        }
    };
    for path in candidates {
        match clear_file(root, &path, execute) {
            Ok(entry) => report.files.push(entry),
            Err(error) => report.failures.push((path, error.to_string())),
        }
    }
    Ok(report)
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

fn classify(path: &Path) -> Option<LogFileType> {
    let name = path.file_name()?.to_str()?;
    let parent = path.parent()?;
    if parent == Path::new("runtime") && name.starts_with("runtime-") && name.ends_with(".jsonl") {
        Some(LogFileType::Runtime)
    } else if parent == Path::new("crashes")
        && matches!(path.extension()?.to_str()?, "json" | "txt" | "log")
    {
        Some(LogFileType::Incidents)
    } else if parent == Path::new("runtime/snapshots")
        && name.starts_with("snapshot-")
        && matches!(path.extension()?.to_str()?, "json" | "jsonl")
    {
        Some(LogFileType::Snapshots)
    } else if parent.as_os_str().is_empty() && name.ends_with(".log") {
        Some(LogFileType::Legacy)
    } else {
        None
    }
}

fn validate_directory(root: &Path, directory: &Path) -> io::Result<bool> {
    let mut current = root.to_path_buf();
    let relative = directory
        .strip_prefix(root)
        .map_err(|_| invalid("directory outside log root"))?;
    for part in std::iter::once(None).chain(relative.components().map(Some)) {
        if let Some(Component::Normal(name)) = part {
            current.push(name);
        }
        match fs::symlink_metadata(&current) {
            Ok(meta) if meta.is_dir() && !meta.file_type().is_symlink() => {}
            Ok(_) => return Err(invalid("refusing a symlink or non-directory log path")),
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
            Err(error) => return Err(error),
        }
    }
    Ok(true)
}

fn clear_file(root: &Path, path: &Path, execute: bool) -> io::Result<LogClearEntry> {
    if !validate_directory(root, path.parent().expect("log parent"))? {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "log directory does not exist",
        ));
    }
    let meta = fs::symlink_metadata(path)?;
    if !meta.is_file() || meta.file_type().is_symlink() {
        return Err(invalid("refusing a symlink or non-file log"));
    }
    if !execute {
        return Ok(LogClearEntry {
            path: path.to_path_buf(),
            bytes: meta.len(),
            action: LogClearAction::Preview,
        });
    }

    let in_runtime = path.starts_with(root.join("runtime"));
    let _reservation = if in_runtime {
        let guard = retention_lock(&root.join("runtime"))?;
        guard.lock_exclusive()?;
        Some(guard)
    } else {
        None
    };
    // Recheck after acquiring the reservation, including symlink substitutions.
    if !validate_directory(root, path.parent().unwrap())? {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "log directory disappeared",
        ));
    }
    let mut options = OpenOptions::new();
    options.read(true).write(true);
    nofollow(&mut options);
    let file = options.open(path)?;
    let meta = file.metadata()?;
    if !meta.is_file() || fs::symlink_metadata(path)?.file_type().is_symlink() {
        return Err(invalid("refusing a non-file log"));
    }
    let action = match file.try_lock_exclusive() {
        Ok(()) => {
            fs::remove_file(path)?;
            LogClearAction::Removed
        }
        Err(error)
            if error.raw_os_error() == fs2::lock_contended_error().raw_os_error()
                && path.parent() == Some(root.join("runtime").as_path()) =>
        {
            file.set_len(0)?;
            file.sync_data()?;
            LogClearAction::Truncated
        }
        Err(error) => return Err(error),
    };
    Ok(LogClearEntry {
        path: path.to_path_buf(),
        bytes: meta.len(),
        action,
    })
}
