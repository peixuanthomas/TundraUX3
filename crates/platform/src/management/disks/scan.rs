use super::*;
use std::cmp::Reverse;
use std::collections::{BTreeMap, BinaryHeap, HashSet};
use std::fs::File;
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::ffi::OsStrExt;
use std::path::Component;
use std::time::{Duration, Instant};

#[derive(Debug, Default)]
struct Totals {
    logical: u64,
    allocated: u64,
    files: u64,
}

fn open_directory(path: &Path) -> Result<File, ManagementError> {
    if !path.is_absolute() {
        return Err(ManagementError::InvalidInput(
            "Scan directory must be absolute".into(),
        ));
    }
    let root = CString::new("/").unwrap();
    let fd = unsafe {
        libc::open(
            root.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if fd < 0 {
        return Err(io_error(std::io::Error::last_os_error()));
    }
    let mut file = unsafe { File::from_raw_fd(fd) };
    for component in path.components() {
        let name = match component {
            Component::RootDir | Component::CurDir => continue,
            Component::Normal(n) => n,
            _ => {
                return Err(ManagementError::InvalidInput(
                    "Scan directory must not contain parent components".into(),
                ));
            }
        };
        file = open_child(&file, name, libc::O_RDONLY | libc::O_DIRECTORY)?;
    }
    Ok(file)
}

fn open_child(parent: &File, name: &std::ffi::OsStr, flags: i32) -> Result<File, ManagementError> {
    let name = CString::new(name.as_bytes())
        .map_err(|_| ManagementError::InvalidInput("Entry contains NUL".into()))?;
    let fd = unsafe {
        libc::openat(
            parent.as_raw_fd(),
            name.as_ptr(),
            flags | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if fd < 0 {
        return Err(io_error(std::io::Error::last_os_error()));
    }
    Ok(unsafe { File::from_raw_fd(fd) })
}

fn mount_id(file: &File) -> Result<u64, ManagementError> {
    let empty = CString::new("").unwrap();
    let mut result = std::mem::MaybeUninit::<libc::statx>::zeroed();
    let status = unsafe {
        libc::statx(
            file.as_raw_fd(),
            empty.as_ptr(),
            libc::AT_EMPTY_PATH | libc::AT_SYMLINK_NOFOLLOW,
            libc::STATX_MNT_ID,
            result.as_mut_ptr(),
        )
    };
    if status != 0 {
        return Err(ManagementError::Unavailable(format!(
            "This kernel cannot identify mounts during a scan: {}",
            std::io::Error::last_os_error()
        )));
    }
    let result = unsafe { result.assume_init() };
    if result.stx_mask & libc::STATX_MNT_ID == 0 {
        return Err(ManagementError::Unavailable(
            "This kernel does not expose mount IDs; Tundra cannot promise to skip bind mounts"
                .into(),
        ));
    }
    Ok(result.stx_mnt_id)
}

#[cfg(test)]
pub(super) fn scan(
    path: &Path,
    interaction: &mut dyn OperationInteraction,
    cancelled: &AtomicBool,
) -> Result<ManagementSnapshot, ManagementError> {
    scan_sorted(path, "allocated", interaction, cancelled)
}

pub(super) fn scan_sorted(
    path: &Path,
    sort: &str,
    interaction: &mut dyn OperationInteraction,
    cancelled: &AtomicBool,
) -> Result<ManagementSnapshot, ManagementError> {
    if !matches!(sort, "allocated" | "logical" | "files") {
        return Err(ManagementError::InvalidInput(
            "Choose size or file count for sorting.".into(),
        ));
    }
    check_cancelled(cancelled)?;
    let root = open_directory(path)?;
    let root_metadata = root.metadata().map_err(io_error)?;
    let root_mount = mount_id(&root)?;
    let mut queue = vec![(root, path.to_path_buf(), None::<String>)];
    let mut directories = BTreeMap::<String, Totals>::new();
    let mut top_entries = BTreeMap::<String, (u64, u64, bool)>::new();
    let mut total = Totals::default();
    let mut hardlinks = HashSet::new();
    let mut directory_ids = HashSet::new();
    directory_ids.insert((root_metadata.dev(), root_metadata.ino()));
    let mut largest = BinaryHeap::<Reverse<(u64, u64, String, u64, u64)>>::new();
    let mut errors = Vec::new();
    let mut unavailable = 0_u64;
    let mut skipped_links = 0_u64;
    let mut skipped_mounts = 0_u64;
    let mut entries_seen = 0_u64;
    let mut progress = Instant::now();
    while let Some((directory, display_path, category)) = queue.pop() {
        check_cancelled(cancelled)?;
        let entries = match fs::read_dir(format!("/proc/self/fd/{}", directory.as_raw_fd())) {
            Ok(e) => e,
            Err(e) => {
                unavailable += 1;
                if errors.len() < 20 {
                    errors.push(format!("{}: {e}", display_path.display()));
                }
                continue;
            }
        };
        for entry in entries {
            check_cancelled(cancelled)?;
            entries_seen += 1;
            if entries_seen > 2_000_000 {
                return Err(ManagementError::Unavailable(
                    "Scan exceeded two million entries; choose a smaller directory".into(),
                ));
            }
            let entry = match entry {
                Ok(e) => e,
                Err(e) => {
                    unavailable += 1;
                    if errors.len() < 20 {
                        errors.push(format!("{}: {e}", display_path.display()));
                    }
                    continue;
                }
            };
            let name = entry.file_name();
            let child_path = display_path.join(&name);
            let child = match open_child(&directory, &name, libc::O_PATH) {
                Ok(c) => c,
                Err(e) => {
                    unavailable += 1;
                    if errors.len() < 20 {
                        errors.push(format!("{}: {e}", child_path.display()));
                    }
                    continue;
                }
            };
            let metadata = match child.metadata() {
                Ok(m) => m,
                Err(e) => {
                    unavailable += 1;
                    if errors.len() < 20 {
                        errors.push(format!("{}: {e}", child_path.display()));
                    }
                    continue;
                }
            };
            if metadata.file_type().is_symlink() {
                skipped_links += 1;
                continue;
            }
            if mount_id(&child)? != root_mount || metadata.dev() != root_metadata.dev() {
                skipped_mounts += 1;
                continue;
            }
            let top_level = category
                .clone()
                .unwrap_or_else(|| name.to_string_lossy().into_owned());
            if category.is_none() {
                top_entries.insert(
                    top_level.clone(),
                    (metadata.dev(), metadata.ino(), metadata.is_dir()),
                );
            }
            if metadata.is_dir() {
                if !directory_ids.insert((metadata.dev(), metadata.ino())) {
                    continue;
                }
                match open_child(&directory, &name, libc::O_RDONLY | libc::O_DIRECTORY) {
                    Ok(child) => {
                        let current = child.metadata().map_err(io_error)?;
                        if current.ino() != metadata.ino()
                            || current.dev() != metadata.dev()
                            || mount_id(&child)? != root_mount
                        {
                            unavailable += 1;
                            if errors.len() < 20 {
                                errors.push(format!(
                                    "{} changed during the scan",
                                    child_path.display()
                                ));
                            }
                            continue;
                        }
                        if queue.len() >= 100_000 {
                            return Err(ManagementError::Unavailable(
                                "Too many pending directories; choose a smaller directory".into(),
                            ));
                        }
                        queue.push((child, child_path, Some(top_level)));
                    }
                    Err(e) => {
                        unavailable += 1;
                        if errors.len() < 20 {
                            errors.push(format!("{}: {e}", child_path.display()));
                        }
                    }
                }
            } else if metadata.is_file() {
                if !hardlinks.insert((metadata.dev(), metadata.ino())) {
                    continue;
                }
                // This accounts for readable metadata and never opens file contents.
                let logical = metadata.len();
                let allocated = metadata.blocks().saturating_mul(512);
                total.logical = total.logical.saturating_add(logical);
                total.allocated = total.allocated.saturating_add(allocated);
                total.files += 1;
                let directory = directories.entry(top_level).or_default();
                directory.logical = directory.logical.saturating_add(logical);
                directory.allocated = directory.allocated.saturating_add(allocated);
                directory.files += 1;
                largest.push(Reverse((
                    allocated,
                    logical,
                    child_path.display().to_string(),
                    metadata.dev(),
                    metadata.ino(),
                )));
                if largest.len() > 100 {
                    largest.pop();
                }
            }
            if progress.elapsed() > Duration::from_millis(250) {
                interaction.emit(OperationEvent::Progress { message: format!("Scanned {entries_seen} entries; {} unique files, {} on-disk bytes; {unavailable} unavailable entries", total.files, total.allocated), percent: None });
                progress = Instant::now();
            }
        }
    }
    let mut snapshot = ManagementSnapshot {
        columns: vec![
            "Path".into(),
            "Result type".into(),
            "Logical bytes".into(),
            "On-disk bytes".into(),
            "Unique files".into(),
        ],
        backend: "Linux descriptor-based filesystem scan".into(),
        ..Default::default()
    };
    let mut categories = directories.into_iter().collect::<Vec<_>>();
    categories.sort_by(|a, b| {
        match sort {
            "files" => b.1.files.cmp(&a.1.files),
            "logical" => b.1.logical.cmp(&a.1.logical),
            _ => b.1.allocated.cmp(&a.1.allocated),
        }
        .then(a.0.cmp(&b.0))
    });
    for (name, total) in categories.into_iter().take(1000) {
        let entry_path = path.join(&name);
        let (device, inode, directory) = top_entries.get(&name).copied().unwrap_or((0, 0, true));
        snapshot.rows.push(ManagementRow {
            id: entry_path.display().to_string(),
            cells: vec![
                path.join(&name).display().to_string(),
                if directory {
                    "Directory total"
                } else {
                    "File total"
                }
                .into(),
                total.logical.to_string(),
                total.allocated.to_string(),
                total.files.to_string(),
            ],
            identity: BTreeMap::from([
                (
                    "kind".into(),
                    if directory { "directory" } else { "file" }.into(),
                ),
                ("device".into(), device.to_string()),
                ("inode".into(), inode.to_string()),
            ]),
            actions: vec![open_result(&entry_path, directory)],
            ..Default::default()
        });
    }
    let mut largest = largest.into_iter().map(|r| r.0).collect::<Vec<_>>();
    largest.sort_by(|a, b| b.0.cmp(&a.0));
    for (allocated, logical, path, device, inode) in largest {
        snapshot.rows.push(ManagementRow {
            id: format!("file:{path}"),
            cells: vec![
                path.clone(),
                "Large file".into(),
                logical.to_string(),
                allocated.to_string(),
                "1".into(),
            ],
            identity: BTreeMap::from([
                ("kind".into(), "file".into()),
                ("device".into(), device.to_string()),
                ("inode".into(), inode.to_string()),
            ]),
            actions: vec![open_result(Path::new(&path), false)],
            ..Default::default()
        });
    }
    snapshot.notices.push(format!("Scan {}: {} unique files; {} logical bytes; {} on-disk bytes. Hard links are counted once. Directory metadata and filesystem shared extents are not charged; on-disk bytes use each file's reported allocated blocks.", path.display(), total.files, total.logical, total.allocated));
    snapshot.notices.push(format!("Skipped {skipped_links} symbolic links and {skipped_mounts} other mounts. {unavailable} entries could not be read or changed during scanning; totals are incomplete when this count is nonzero."));
    snapshot.notices.extend(errors);
    sort_scan_results(&mut snapshot, sort)?;
    snapshot.actions.push(ManagementAction {
        id: "sort_scan".into(),
        label: "Sort results".into(),
        fields: vec![mount_field(
            "sort",
            "Sort by",
            sort,
            vec!["allocated".into(), "logical".into(), "files".into()],
        )],
        ..Default::default()
    });
    Ok(snapshot)
}

fn open_result(path: &Path, directory: bool) -> ManagementAction {
    ManagementAction {
        id: "open_path".into(),
        label: if directory {
            "Open directory"
        } else {
            "Show file"
        }
        .into(),
        primary: true,
        values: BTreeMap::from([("path".into(), path.display().to_string())]),
        ..Default::default()
    }
}

pub fn sort_scan_results(
    snapshot: &mut ManagementSnapshot,
    by: &str,
) -> Result<(), ManagementError> {
    let column = match by {
        "logical" => 2,
        "allocated" => 3,
        "files" => 4,
        _ => {
            return Err(ManagementError::InvalidInput(
                "Choose size or file count for sorting.".into(),
            ));
        }
    };
    if snapshot.backend != "Linux descriptor-based filesystem scan" {
        return Err(ManagementError::InvalidInput(
            "Only scan results can be sorted this way.".into(),
        ));
    }
    snapshot.rows.sort_by(|a, b| {
        let value = |row: &ManagementRow| {
            row.cells
                .get(column)
                .and_then(|v| v.parse::<u64>().ok())
                .unwrap_or(0)
        };
        value(b).cmp(&value(a)).then(a.id.cmp(&b.id))
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Interaction;
    impl OperationInteraction for Interaction {
        fn emit(&mut self, _: OperationEvent) {}
        fn ask(
            &mut self,
            _: &str,
            _: &str,
            _: &[String],
            _: bool,
        ) -> Result<String, ManagementError> {
            unreachable!()
        }
    }
    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            let root = std::env::current_dir()
                .unwrap()
                .join("target")
                .join(format!(
                    "tundra-disk-scan-{}-{}",
                    std::process::id(),
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_nanos()
                ));
            fs::create_dir_all(&root).unwrap();
            Self(root)
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    #[test]
    fn scan_skips_links_and_deduplicates_hardlinks() {
        let fixture = Fixture::new();
        fs::write(fixture.0.join("a"), b"hello").unwrap();
        fs::hard_link(fixture.0.join("a"), fixture.0.join("b")).unwrap();
        std::os::unix::fs::symlink("/", fixture.0.join("outside")).unwrap();
        let snapshot = scan(&fixture.0, &mut Interaction, &AtomicBool::new(false)).unwrap();
        assert!(snapshot.notices[0].contains("1 unique files; 5 logical bytes"));
        assert!(snapshot.notices[1].contains("Skipped 1 symbolic links"));
    }
    #[test]
    fn scan_rejects_symlink_root_and_cancellation() {
        let fixture = Fixture::new();
        fs::create_dir(fixture.0.join("real")).unwrap();
        std::os::unix::fs::symlink("real", fixture.0.join("link")).unwrap();
        assert!(
            scan(
                &fixture.0.join("link"),
                &mut Interaction,
                &AtomicBool::new(false)
            )
            .is_err()
        );
        assert_eq!(
            scan(&fixture.0, &mut Interaction, &AtomicBool::new(true)),
            Err(ManagementError::Cancelled)
        );
    }
    #[test]
    fn count_sort_and_file_actions_preserve_scan_identity() {
        let fixture = Fixture::new();
        fs::create_dir(fixture.0.join("many")).unwrap();
        for name in ["a", "b", "c"] {
            fs::write(fixture.0.join("many").join(name), b"x").unwrap();
        }
        fs::write(fixture.0.join("large"), [0_u8; 8192]).unwrap();
        let mut snapshot = scan_sorted(
            &fixture.0,
            "files",
            &mut Interaction,
            &AtomicBool::new(false),
        )
        .unwrap();
        assert_eq!(snapshot.rows[0].cells[4], "3");
        assert_eq!(snapshot.rows[0].identity["kind"], "directory");
        assert_eq!(snapshot.rows[0].actions[0].id, "open_path");
        assert!(snapshot.rows.iter().all(|row| row.identity["inode"] != "0"));
        sort_scan_results(&mut snapshot, "logical").unwrap();
        assert_eq!(snapshot.rows[0].cells[2], "8192");
        assert_eq!(snapshot.rows[0].identity["kind"], "file");
    }
}
