use super::*;
use std::cmp::Reverse;
use std::collections::{BTreeMap, BinaryHeap, HashSet};
use std::fs::File;
use std::os::fd::{AsRawFd, FromRawFd, IntoRawFd};
use std::os::unix::ffi::{OsStrExt, OsStringExt};
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

fn open_child(
    parent: &impl AsRawFd,
    name: &std::ffi::OsStr,
    flags: i32,
) -> Result<File, ManagementError> {
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

/// A single descriptor query supplies both accounting data and mount identity.
struct ScanMetadata(libc::statx);

impl ScanMetadata {
    fn device(&self) -> u64 {
        libc::makedev(self.0.stx_dev_major, self.0.stx_dev_minor)
    }
    fn inode(&self) -> u64 {
        self.0.stx_ino
    }
    fn mount(&self) -> u64 {
        self.0.stx_mnt_id
    }
    fn is_type(&self, kind: u32) -> bool {
        u32::from(self.0.stx_mode) & libc::S_IFMT == kind
    }
}

fn scan_metadata(file: &File) -> Result<ScanMetadata, ManagementError> {
    let mut result = std::mem::MaybeUninit::<libc::statx>::zeroed();
    let status = unsafe {
        libc::statx(
            file.as_raw_fd(),
            c"".as_ptr(),
            libc::AT_EMPTY_PATH | libc::AT_SYMLINK_NOFOLLOW,
            libc::STATX_BASIC_STATS | libc::STATX_MNT_ID,
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
    const REQUIRED: u32 = libc::STATX_TYPE
        | libc::STATX_INO
        | libc::STATX_SIZE
        | libc::STATX_BLOCKS
        | libc::STATX_MNT_ID;
    if result.stx_mask & REQUIRED != REQUIRED {
        return Err(ManagementError::Unavailable(
            "This filesystem does not expose scan metadata and mount IDs; safe accounting is unavailable"
                .into(),
        ));
    }
    Ok(ScanMetadata(result))
}

/// Own the directory cursor and its descriptor together. Unlike read_dir on
/// /proc/self/fd, fdopendir consumes the existing descriptor instead of opening
/// a second one. Only active ancestors are retained during depth-first scans.
struct ScanDirectory(std::ptr::NonNull<libc::DIR>);

impl ScanDirectory {
    fn new(file: File) -> Result<Self, ManagementError> {
        let fd = file.into_raw_fd();
        // On success DIR owns fd; on failure the caller still owns it.
        match std::ptr::NonNull::new(unsafe { libc::fdopendir(fd) }) {
            Some(directory) => Ok(Self(directory)),
            None => {
                let error = std::io::Error::last_os_error();
                drop(unsafe { File::from_raw_fd(fd) });
                Err(io_error(error))
            }
        }
    }

    fn next_name(&mut self) -> Result<Option<std::ffi::OsString>, ManagementError> {
        loop {
            // readdir returns null for both EOF and failure; errno distinguishes
            // them. This stream is used by exactly one worker at a time.
            unsafe {
                *libc::__errno_location() = 0;
            }
            let entry = unsafe { libc::readdir(self.0.as_ptr()) };
            if entry.is_null() {
                let error = std::io::Error::last_os_error();
                return if error.raw_os_error() == Some(0) {
                    Ok(None)
                } else {
                    Err(io_error(error))
                };
            }
            // d_name is NUL-terminated and valid until the next readdir call.
            let name = unsafe { std::ffi::CStr::from_ptr((*entry).d_name.as_ptr()) }.to_bytes();
            if name != b"." && name != b".." {
                return Ok(Some(std::ffi::OsString::from_vec(name.to_vec())));
            }
        }
    }
}

impl AsRawFd for ScanDirectory {
    fn as_raw_fd(&self) -> std::os::fd::RawFd {
        unsafe { libc::dirfd(self.0.as_ptr()) }
    }
}

impl Drop for ScanDirectory {
    fn drop(&mut self) {
        unsafe {
            libc::closedir(self.0.as_ptr());
        }
    }
}

struct ScanFrame {
    directory: ScanDirectory,
    path: PathBuf,
    category: Option<String>,
}

fn directory_handle_limit() -> Result<usize, ManagementError> {
    let mut limit = std::mem::MaybeUninit::<libc::rlimit>::uninit();
    if unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, limit.as_mut_ptr()) } != 0 {
        return Err(io_error(std::io::Error::last_os_error()));
    }
    // Reserve room for the host, child probes, and diagnostics. Stop explicitly
    // at excessive depth rather than quietly losing arbitrary sibling trees.
    Ok(unsafe { limit.assume_init() }
        .rlim_cur
        .saturating_sub(32)
        .clamp(1, 256) as usize)
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
    let root_metadata = scan_metadata(&root)?;
    let root_mount = root_metadata.mount();
    let handle_limit = directory_handle_limit()?;
    let mut stack = vec![ScanFrame {
        directory: ScanDirectory::new(root)?,
        path: path.to_path_buf(),
        category: None,
    }];
    let mut directories = BTreeMap::<String, Totals>::new();
    let mut top_entries = BTreeMap::<String, (u64, u64, bool)>::new();
    let mut total = Totals::default();
    let mut hardlinks = HashSet::new();
    let mut directory_ids = HashSet::new();
    directory_ids.insert((root_metadata.device(), root_metadata.inode()));
    let mut largest = BinaryHeap::<Reverse<(u64, u64, String, u64, u64)>>::new();
    let mut errors = Vec::new();
    let mut unavailable = 0_u64;
    let mut skipped_links = 0_u64;
    let mut skipped_mounts = 0_u64;
    let mut entries_seen = 0_u64;
    let mut progress = Instant::now();
    while let Some(frame) = stack.last_mut() {
        check_cancelled(cancelled)?;
        let name = match frame.directory.next_name() {
            Ok(Some(name)) => name,
            Ok(None) => {
                stack.pop();
                continue;
            }
            Err(e) => {
                unavailable += 1;
                if errors.len() < 20 {
                    errors.push(format!("{}: {e}", frame.path.display()));
                }
                stack.pop();
                continue;
            }
        };
        entries_seen += 1;
        if entries_seen > 2_000_000 {
            return Err(ManagementError::Unavailable(
                "Scan exceeded two million entries; choose a smaller directory".into(),
            ));
        }
        let child_path = frame.path.join(&name);
        let child = match open_child(&frame.directory, &name, libc::O_PATH) {
            Ok(c) => c,
            Err(e) => {
                unavailable += 1;
                if errors.len() < 20 {
                    errors.push(format!("{}: {e}", child_path.display()));
                }
                continue;
            }
        };
        let metadata = scan_metadata(&child)?;
        if metadata.is_type(libc::S_IFLNK) {
            skipped_links += 1;
            continue;
        }
        if metadata.mount() != root_mount || metadata.device() != root_metadata.device() {
            skipped_mounts += 1;
            continue;
        }
        let top_level = frame
            .category
            .as_deref()
            .map(std::borrow::Cow::Borrowed)
            .unwrap_or_else(|| name.to_string_lossy());
        if frame.category.is_none() {
            top_entries.insert(
                top_level.to_string(),
                (
                    metadata.device(),
                    metadata.inode(),
                    metadata.is_type(libc::S_IFDIR),
                ),
            );
        }
        if metadata.is_type(libc::S_IFDIR) {
            if !directory_ids.insert((metadata.device(), metadata.inode())) {
                continue;
            }
            match open_child(&frame.directory, &name, libc::O_RDONLY | libc::O_DIRECTORY) {
                Ok(child) => {
                    let current = scan_metadata(&child)?;
                    if current.inode() != metadata.inode()
                        || current.device() != metadata.device()
                        || current.mount() != root_mount
                    {
                        unavailable += 1;
                        if errors.len() < 20 {
                            errors
                                .push(format!("{} changed during the scan", child_path.display()));
                        }
                        continue;
                    }
                    let category = Some(top_level.into_owned());
                    if stack.len() >= handle_limit {
                        return Err(ManagementError::Unavailable(format!(
                            "Scan reached its {handle_limit}-directory handle limit; choose a less deeply nested directory"
                        )));
                    }
                    stack.push(ScanFrame {
                        directory: ScanDirectory::new(child)?,
                        path: child_path,
                        category,
                    });
                }
                Err(e) => {
                    unavailable += 1;
                    if errors.len() < 20 {
                        errors.push(format!("{}: {e}", child_path.display()));
                    }
                }
            }
        } else if metadata.is_type(libc::S_IFREG) {
            if !hardlinks.insert((metadata.device(), metadata.inode())) {
                continue;
            }
            // This accounts for readable metadata and never opens file contents.
            let logical = metadata.0.stx_size;
            let allocated = metadata.0.stx_blocks.saturating_mul(512);
            total.logical = total.logical.saturating_add(logical);
            total.allocated = total.allocated.saturating_add(allocated);
            total.files += 1;
            let directory = if let Some(directory) = directories.get_mut(top_level.as_ref()) {
                directory
            } else {
                directories.entry(top_level.into_owned()).or_default()
            };
            directory.logical = directory.logical.saturating_add(logical);
            directory.allocated = directory.allocated.saturating_add(allocated);
            directory.files += 1;
            // Avoid allocating a display string or touching the heap for
            // files smaller than its current minimum. Equal sizes still
            // use the full tuple so tie-breaking remains deterministic.
            if largest.len() < 100
                || largest
                    .peek()
                    .is_some_and(|Reverse(min)| (allocated, logical) >= (min.0, min.1))
            {
                largest.push(Reverse((
                    allocated,
                    logical,
                    child_path.display().to_string(),
                    metadata.device(),
                    metadata.inode(),
                )));
                if largest.len() > 100 {
                    largest.pop();
                }
            }
        }
        if progress.elapsed() > Duration::from_millis(250) {
            interaction.emit(OperationEvent::Progress { message: format!("Scanned {entries_seen} entries; {} unique files, {} on-disk bytes; {unavailable} unavailable entries", total.files, total.allocated), percent: None });
            progress = Instant::now();
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
    fn descriptor_cursor_stays_on_the_opened_directory_after_path_replacement() {
        // DrvFS/9p can reject openat on a renamed directory with ENXIO.
        // Exercise descriptor identity on the native Linux filesystem here;
        // the scan reports failed shared-filesystem entries as unavailable.
        let fixture = tempfile::tempdir().unwrap();
        let root = fixture.path();
        fs::create_dir(root.join("original")).unwrap();
        fs::create_dir(root.join("outside")).unwrap();
        fs::write(root.join("original/inside"), b"safe").unwrap();
        fs::write(root.join("outside/inside"), b"must not read this").unwrap();
        let mut cursor =
            ScanDirectory::new(open_directory(&root.join("original")).unwrap()).unwrap();
        fs::rename(root.join("original"), root.join("moved")).unwrap();
        std::os::unix::fs::symlink(root.join("outside"), root.join("original")).unwrap();
        let name = cursor.next_name().unwrap().unwrap();
        assert_eq!(name, "inside");
        let metadata = scan_metadata(&open_child(&cursor, &name, libc::O_PATH).unwrap()).unwrap();
        assert_eq!(metadata.0.stx_size, 4);
        assert!(cursor.next_name().unwrap().is_none());
    }

    #[test]
    fn wide_scans_work_with_a_small_descriptor_budget() {
        // RLIMIT_NOFILE is process-wide: change it only in an isolated child.
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "management::disks::scan::tests::descriptor_budget_fixture",
                "--nocapture",
            ])
            .env("TUNDRA_SCAN_FD_FIXTURE", "1")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[test]
    fn descriptor_budget_fixture() {
        if std::env::var_os("TUNDRA_SCAN_FD_FIXTURE").is_none() {
            return;
        }
        let fixture = Fixture::new();
        for index in 0..320 {
            let directory = fixture.0.join(index.to_string());
            fs::create_dir(&directory).unwrap();
            fs::write(directory.join("file"), b"x").unwrap();
        }
        let mut limit = libc::rlimit {
            rlim_cur: 0,
            rlim_max: 0,
        };
        assert_eq!(
            unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, &mut limit) },
            0
        );
        let original_limit = limit;
        limit.rlim_cur = limit.rlim_cur.min(128);
        assert_eq!(unsafe { libc::setrlimit(libc::RLIMIT_NOFILE, &limit) }, 0);
        let fd_count = || fs::read_dir("/proc/self/fd").unwrap().count();
        let before = fd_count();
        for _ in 0..3 {
            let snapshot = scan(&fixture.0, &mut Interaction, &AtomicBool::new(false)).unwrap();
            assert!(snapshot.notices[0].contains("320 unique files; 320 logical bytes"));
            assert!(snapshot.notices[1].contains("0 entries could not be read"));
            assert_eq!(fd_count(), before);
        }
        let deep = Fixture::new();
        let mut path = deep.0.clone();
        for _ in 0..directory_handle_limit().unwrap() + 2 {
            path.push("d");
            fs::create_dir(&path).unwrap();
        }
        let error = scan(&deep.0, &mut Interaction, &AtomicBool::new(false)).unwrap_err();
        assert!(
            error.to_string().contains("directory handle limit"),
            "{error}"
        );
        assert_eq!(fd_count(), before);
        assert_eq!(
            unsafe { libc::setrlimit(libc::RLIMIT_NOFILE, &original_limit) },
            0
        );
    }

    #[test]
    #[ignore = "requires unshare user/mount namespaces; mounts only inside an isolated child"]
    fn scan_skips_bind_mounts_in_private_namespace() {
        let output = std::process::Command::new("unshare")
            .args(["--user", "--map-root-user", "--mount", "--fork"])
            .arg(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "management::disks::scan::tests::bind_mount_fixture",
                "--nocapture",
            ])
            .env(
                "TUNDRA_SCAN_PARENT_MOUNT_NS",
                fs::read_link("/proc/self/ns/mnt").unwrap(),
            )
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[test]
    fn bind_mount_fixture() {
        let Some(parent_namespace) = std::env::var_os("TUNDRA_SCAN_PARENT_MOUNT_NS") else {
            return;
        };
        assert_ne!(
            fs::read_link("/proc/self/ns/mnt").unwrap().as_os_str(),
            parent_namespace
        );
        // Use native /tmp: network/shared filesystems may forbid bind mounts
        // in a user namespace. The fixture and its mount die with this child.
        let fixture = tempfile::tempdir().unwrap();
        let source = fixture.path().join("source");
        let root = fixture.path().join("root");
        let target = root.join("bound");
        fs::create_dir(&source).unwrap();
        fs::create_dir(&root).unwrap();
        fs::create_dir(&target).unwrap();
        fs::write(source.join("outside"), b"must not count").unwrap();
        fs::write(root.join("inside"), b"x").unwrap();
        let source_c = CString::new(source.as_os_str().as_bytes()).unwrap();
        let target_c = CString::new(target.as_os_str().as_bytes()).unwrap();
        assert_eq!(
            unsafe {
                libc::mount(
                    source_c.as_ptr(),
                    target_c.as_ptr(),
                    std::ptr::null(),
                    libc::MS_BIND,
                    std::ptr::null(),
                )
            },
            0,
            "{}",
            std::io::Error::last_os_error()
        );
        let result = scan(&root, &mut Interaction, &AtomicBool::new(false));
        assert_eq!(unsafe { libc::umount2(target_c.as_ptr(), 0) }, 0);
        let snapshot = result.unwrap();
        assert!(snapshot.notices[0].contains("1 unique files; 1 logical bytes"));
        assert!(snapshot.notices[1].contains("and 1 other mounts"));
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
