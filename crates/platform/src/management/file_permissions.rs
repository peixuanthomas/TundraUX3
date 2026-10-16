//! Linux file ownership and permission changes, reviewed against a fresh scan.
use super::*;
use std::collections::BTreeSet;
use std::ffi::CString;
use std::ffi::OsString;
use std::fs::{self, File};
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::{
    ffi::{OsStrExt, OsStringExt},
    fs::{MetadataExt, PermissionsExt},
};
use std::path::Path;
use std::sync::atomic::Ordering;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PermissionEntry {
    pub path: PathBuf,
    pub device: u64,
    pub inode: u64,
    pub mode: u32,
    pub uid: u32,
    pub gid: u32,
    pub changed: (i64, i64),
    pub directory: bool,
    pub acl: bool,
    pub default_acl: bool,
    pub capabilities: bool,
    pub links: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PermissionScan {
    pub entries: Vec<PermissionEntry>,
    pub skipped: Vec<(PathBuf, String)>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PermissionChange {
    pub mask: u32,
    pub bits: u32,
    pub uid: Option<u32>,
    pub gid: Option<u32>,
}

fn failed(error: impl std::fmt::Display) -> ManagementError {
    ManagementError::Failed(error.to_string())
}
fn io_error(error: std::io::Error) -> ManagementError {
    if error.kind() == std::io::ErrorKind::PermissionDenied {
        ManagementError::PermissionDenied(error.to_string())
    } else {
        failed(error)
    }
}
fn check(flag: &AtomicBool) -> Result<(), ManagementError> {
    if flag.load(Ordering::Acquire) {
        Err(ManagementError::Cancelled)
    } else {
        Ok(())
    }
}
fn open(path: &Path) -> Result<File, ManagementError> {
    let (parent, name) = super::system_config::parent(path, false)?;
    let fd = unsafe {
        libc::openat(
            parent.as_raw_fd(),
            name.as_ptr(),
            libc::O_PATH | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if fd < 0 {
        return Err(io_error(std::io::Error::last_os_error()));
    }
    Ok(unsafe { File::from_raw_fd(fd) })
}
fn fd_path(file: &File) -> PathBuf {
    PathBuf::from(format!("/proc/self/fd/{}", file.as_raw_fd()))
}
fn path_label(path: &Path) -> String {
    format!("{path:?}")
}
pub fn encode_paths(paths: &[PathBuf]) -> String {
    serde_json::to_string(
        &paths
            .iter()
            .map(|path| path.as_os_str().as_bytes())
            .collect::<Vec<_>>(),
    )
    .expect("path bytes serialize")
}
fn has_attribute(file: &File, name: &str) -> Result<bool, ManagementError> {
    let path = CString::new(fd_path(file).as_os_str().as_bytes()).unwrap();
    let name = CString::new(name).unwrap();
    let result = unsafe { libc::getxattr(path.as_ptr(), name.as_ptr(), std::ptr::null_mut(), 0) };
    if result >= 0 {
        return Ok(true);
    }
    let error = std::io::Error::last_os_error();
    if matches!(error.raw_os_error(), Some(libc::ENODATA | libc::ENOTSUP)) {
        Ok(false)
    } else {
        Err(failed(error))
    }
}
fn entry(path: &Path, file: &File) -> Result<PermissionEntry, ManagementError> {
    let m = file.metadata().map_err(failed)?;
    Ok(PermissionEntry {
        path: path.into(),
        device: m.dev(),
        inode: m.ino(),
        mode: m.mode(),
        uid: m.uid(),
        gid: m.gid(),
        changed: (m.ctime(), m.ctime_nsec()),
        directory: m.is_dir(),
        acl: has_attribute(file, "system.posix_acl_access")?,
        default_acl: m.is_dir() && has_attribute(file, "system.posix_acl_default")?,
        capabilities: has_attribute(file, "security.capability")?,
        links: m.nlink(),
    })
}
fn mounts() -> Result<BTreeSet<PathBuf>, ManagementError> {
    Ok(fs::read_to_string("/proc/self/mountinfo")
        .map_err(failed)?
        .lines()
        .filter_map(|line| line.split_whitespace().nth(4))
        .map(|path| {
            PathBuf::from(
                path.replace("\\040", " ")
                    .replace("\\011", "\t")
                    .replace("\\012", "\n")
                    .replace("\\134", "\\"),
            )
        })
        .collect())
}

pub fn scan(
    paths: &[PathBuf],
    recursive: bool,
    flag: &AtomicBool,
) -> Result<PermissionScan, ManagementError> {
    let mount_points = mounts()?;
    let mut result = PermissionScan::default();
    let mut seen = BTreeSet::new();
    let mut pending: Vec<_> = paths.iter().cloned().map(|path| (path, true)).collect();
    while let Some((path, root)) = pending.pop() {
        check(flag)?;
        let file = open(&path)?;
        let metadata = file.metadata().map_err(failed)?;
        let reason = if metadata.file_type().is_symlink() {
            Some("permissions-skip-link")
        } else if !metadata.is_file() && !metadata.is_dir() {
            Some("permissions-skip-special")
        } else if !root && mount_points.contains(&path) {
            Some("permissions-skip-mount")
        } else {
            None
        };
        if let Some(reason) = reason {
            result.skipped.push((path, i18n::tr!(reason)));
            continue;
        }
        if !seen.insert((metadata.dev(), metadata.ino())) {
            continue;
        }
        if recursive && metadata.is_dir() {
            for child in fs::read_dir(fd_path(&file)).map_err(failed)? {
                pending.push((path.join(child.map_err(failed)?.file_name()), false));
            }
        }
        result.entries.push(entry(&path, &file)?);
    }
    result.entries.sort_by(|a, b| a.path.cmp(&b.path));
    result.skipped.sort();
    Ok(result)
}

fn roots(values: &BTreeMap<String, String>) -> Result<Vec<PathBuf>, ManagementError> {
    let paths: Vec<Vec<u8>> = serde_json::from_str(
        values
            .get("paths")
            .ok_or_else(|| ManagementError::InvalidInput(i18n::tr!("permissions-select-files")))?,
    )
    .map_err(failed)?;
    if paths.is_empty() {
        return Err(ManagementError::InvalidInput(i18n::tr!(
            "permissions-select-files"
        )));
    }
    Ok(paths
        .into_iter()
        .map(|path| PathBuf::from(OsString::from_vec(path)))
        .collect())
}
fn mixed(entries: &[&PermissionEntry]) -> String {
    (0..9)
        .map(|index| {
            let bit = 1 << (8 - index);
            match entries.first() {
                Some(first)
                    if entries
                        .iter()
                        .all(|entry| entry.mode & bit == first.mode & bit) =>
                {
                    if first.mode & bit != 0 { '1' } else { '0' }
                }
                _ => 'm',
            }
        })
        .collect()
}
fn choice(id: &str, label: String, value: String, choices: Vec<String>) -> ManagementField {
    ManagementField {
        id: id.into(),
        label,
        value,
        choices,
        control: FieldControl::SearchableChoice,
        ..Default::default()
    }
}
pub fn query(
    query: &ManagementQuery,
    flag: &AtomicBool,
) -> Result<ManagementSnapshot, ManagementError> {
    let paths = roots(&query.options)?;
    let result = match scan(&paths, false, flag) {
        Ok(result) => result,
        Err(ManagementError::PermissionDenied(reason)) => {
            return Ok(ManagementSnapshot {
                notices: vec![reason],
                needs_authorization: true,
                actions: vec![ManagementAction {
                    id: "inspect_permissions".into(),
                    label: i18n::tr!("permissions-authorize-read"),
                    privileged: true,
                    primary: true,
                    values: query.inspection_values(),
                    ..Default::default()
                }],
                ..Default::default()
            });
        }
        Err(error) => return Err(error),
    };
    let mut values = query.options.clone();
    let mut fields = Vec::new();
    for (kind, directory) in [("files", false), ("directories", true)] {
        let entries: Vec<_> = result
            .entries
            .iter()
            .filter(|entry| entry.directory == directory)
            .collect();
        let value = mixed(&entries);
        values.insert(format!("{kind}_initial"), value.clone());
        fields.push(ManagementField {
            id: kind.into(),
            label: i18n::tr!(if directory {
                "permissions-directories"
            } else {
                "permissions-files"
            }),
            value,
            control: FieldControl::Permissions,
            ..Default::default()
        });
    }
    fields.push(ManagementField {
        id: "recursive".into(),
        label: i18n::tr!("permissions-recursive"),
        value: "false".into(),
        control: FieldControl::Checkbox,
        ..Default::default()
    });
    let keep = i18n::tr!("permissions-keep");
    let accounts = users::accounts(None)?;
    let groups = users::groups()?;
    fields.push(choice(
        "owner",
        i18n::tr!("permissions-owner"),
        keep.clone(),
        std::iter::once(keep.clone())
            .chain(
                accounts
                    .iter()
                    .map(|u| format!("{} ({})", u.username, u.uid)),
            )
            .collect(),
    ));
    fields.push(choice(
        "group",
        i18n::tr!("permissions-group"),
        keep.clone(),
        std::iter::once(keep)
            .chain(groups.iter().map(|g| format!("{} ({})", g.name, g.gid)))
            .collect(),
    ));
    let rows = result
        .entries
        .iter()
        .map(|entry| {
            let owner = accounts.iter().find(|u| u.uid == entry.uid).map_or_else(
                || entry.uid.to_string(),
                |u| format!("{} ({})", u.username, u.uid),
            );
            let group = groups.iter().find(|g| g.gid == entry.gid).map_or_else(
                || entry.gid.to_string(),
                |g| format!("{} ({})", g.name, g.gid),
            );
            ManagementRow {
                id: path_label(&entry.path),
                cells: vec![
                    path_label(&entry.path),
                    format!("{:03o}", entry.mode & 0o777),
                    owner.clone(),
                    group.clone(),
                ],
                detail: vec![
                    (i18n::tr!("permissions-owner"), owner),
                    (i18n::tr!("permissions-group"), group),
                ],
                ..Default::default()
            }
        })
        .collect();
    Ok(ManagementSnapshot {
        columns: vec![
            i18n::tr!("permissions-path"),
            i18n::tr!("permissions-title"),
            i18n::tr!("permissions-owner"),
            i18n::tr!("permissions-group"),
        ],
        rows,
        actions: vec![ManagementAction {
            id: "change_permissions".into(),
            label: i18n::tr!("permissions-edit"),
            fields,
            values,
            privileged: true,
            primary: true,
            ..Default::default()
        }],
        notices: vec![i18n::tr!("permissions-help")],
        details_complete: true,
        ..Default::default()
    })
}
fn selected_id(value: Option<&String>, group: bool) -> Result<Option<u32>, ManagementError> {
    let Some(value) = value else {
        return Ok(None);
    };
    if value == &i18n::tr!("permissions-keep") {
        return Ok(None);
    }
    let (name, id) = value
        .rsplit_once(" (")
        .ok_or_else(|| ManagementError::InvalidInput(i18n::tr!("permissions-invalid")))?;
    let id: u32 = id.trim_end_matches(')').parse().map_err(failed)?;
    let valid = if group {
        users::groups()?
            .iter()
            .any(|g| g.name == name && g.gid == id)
    } else {
        users::accounts(Some(name))?
            .iter()
            .any(|u| u.username == name && u.uid == id)
    };
    if !valid {
        return Err(ManagementError::Conflict(i18n::tr!("permissions-changed")));
    }
    Ok(Some(id))
}
fn change(
    command: &ManagementCommand,
    directory: bool,
) -> Result<PermissionChange, ManagementError> {
    let kind = if directory { "directories" } else { "files" };
    let initial = command
        .values
        .get(&format!("{kind}_initial"))
        .map(String::as_str)
        .unwrap_or("mmmmmmmmm");
    let value = command
        .values
        .get(kind)
        .map(String::as_str)
        .unwrap_or(initial);
    if value.len() != 9
        || initial.len() != 9
        || !value.bytes().all(|c| matches!(c, b'0' | b'1' | b'm'))
    {
        return Err(ManagementError::InvalidInput(i18n::tr!(
            "permissions-invalid"
        )));
    }
    let mut change = PermissionChange {
        uid: selected_id(command.values.get("owner"), false)?,
        gid: selected_id(command.values.get("group"), true)?,
        ..Default::default()
    };
    for (i, (before, after)) in initial.bytes().zip(value.bytes()).enumerate() {
        if before != after && after != b'm' {
            change.mask |= 1 << (8 - i);
            if after == b'1' {
                change.bits |= 1 << (8 - i);
            }
        }
    }
    Ok(change)
}
fn apply(entry: &PermissionEntry, change: PermissionChange) -> Result<bool, ManagementError> {
    let file = open(&entry.path)?;
    if &self::entry(&entry.path, &file)? != entry {
        return Err(ManagementError::Conflict(i18n::tr!("permissions-changed")));
    }
    if change.uid.is_some_and(|uid| uid != entry.uid)
        || change.gid.is_some_and(|gid| gid != entry.gid)
    {
        let empty = c"";
        if unsafe {
            libc::fchownat(
                file.as_raw_fd(),
                empty.as_ptr(),
                change.uid.unwrap_or(u32::MAX),
                change.gid.unwrap_or(u32::MAX),
                libc::AT_EMPTY_PATH,
            )
        } != 0
        {
            return Err(failed(std::io::Error::last_os_error()));
        }
    }
    let current = file.metadata().map_err(failed)?.mode();
    if current & change.mask != change.bits & change.mask {
        fs::set_permissions(
            fd_path(&file),
            fs::Permissions::from_mode(
                (current & 0o7777 & !change.mask) | (change.bits & change.mask),
            ),
        )
        .map_err(failed)?;
    }
    let final_entry = self::entry(&entry.path, &file)?;
    if (final_entry.mode & change.mask) != (change.bits & change.mask)
        || change.uid.is_some_and(|uid| uid != final_entry.uid)
        || change.gid.is_some_and(|gid| gid != final_entry.gid)
    {
        return Err(failed(i18n::tr!("permissions-verify-failed")));
    }
    Ok((entry.mode & 0o6000 != final_entry.mode & 0o6000)
        || (entry.capabilities && !final_entry.capabilities))
}
pub fn execute(
    command: &ManagementCommand,
    _: &ExecutionContext,
    io: &mut dyn OperationInteraction,
    flag: &AtomicBool,
) -> Result<String, ManagementError> {
    if command.action == "inspect_permissions" {
        let snapshot = query(&ManagementQuery::from_inspection(command), flag)?
            .with_authorized_refresh(command);
        io.emit(OperationEvent::Snapshot { snapshot });
        return Ok(i18n::tr!("permissions-title"));
    }
    if command.action != "change_permissions" {
        return Err(ManagementError::InvalidInput(i18n::tr!(
            "permissions-invalid"
        )));
    }
    let paths = roots(&command.values)?;
    let recursive = command.values.get("recursive").map(String::as_str) == Some("true");
    let files = change(command, false)?;
    let directories = change(command, true)?;
    let before = scan(&paths, recursive, flag)?;
    let preview = i18n::tr!(
        "permissions-preview",
        files = before.entries.iter().filter(|e| !e.directory).count() as i64,
        directories = before.entries.iter().filter(|e| e.directory).count() as i64
    );
    let mut lines: Vec<_> = paths.iter().map(|path| path_label(path)).collect();
    for entry in &before.entries {
        let patch = if entry.directory { directories } else { files };
        lines.push(format!(
            "{}: {:04o} → {:04o}; {}:{} → {}:{}",
            path_label(&entry.path),
            entry.mode & 0o7777,
            (entry.mode & 0o7777 & !patch.mask) | patch.bits,
            entry.uid,
            entry.gid,
            patch.uid.unwrap_or(entry.uid),
            patch.gid.unwrap_or(entry.gid)
        ));
        if entry.acl {
            lines.push(i18n::tr!("permissions-acl"));
        }
        if entry.default_acl {
            lines.push(i18n::tr!("permissions-default-acl"));
        }
        if entry.links > 1 && !entry.directory {
            lines.push(i18n::tr!("permissions-hardlinks"));
        }
        if (patch.uid.is_some_and(|uid| uid != entry.uid)
            || patch.gid.is_some_and(|gid| gid != entry.gid))
            && (entry.mode & 0o6000 != 0 || entry.capabilities)
        {
            lines.push(i18n::tr!("permissions-special-cleared"));
        }
    }
    for (path, reason) in &before.skipped {
        lines.push(format!("{}: {reason}", path_label(path)));
    }
    super::configuration::confirm_preview("permissions-apply", &preview, &lines, io)?;
    if scan(&paths, recursive, flag)? != before {
        return Err(ManagementError::Conflict(i18n::tr!("permissions-changed")));
    }
    let mut entries = before.entries.clone();
    entries.sort_by_key(|entry| std::cmp::Reverse(entry.path.components().count()));
    for (index, entry) in entries.iter().enumerate() {
        let result = check(flag)
            .and_then(|()| apply(entry, if entry.directory { directories } else { files }));
        match result {
            Ok(cleared) => io.emit(OperationEvent::Output {
                text: format!(
                    "{}: {}{}",
                    i18n::tr!("permissions-done"),
                    path_label(&entry.path),
                    if cleared {
                        format!("\n{}", i18n::tr!("permissions-special-were-cleared"))
                    } else {
                        String::new()
                    }
                ),
            }),
            Err(error) => {
                io.emit(OperationEvent::Output {
                    text: format!(
                        "{}: {}: {error}",
                        i18n::tr!("permissions-failed"),
                        path_label(&entry.path)
                    ),
                });
                for pending in &entries[index + 1..] {
                    io.emit(OperationEvent::Output {
                        text: format!(
                            "{}: {}",
                            i18n::tr!("permissions-pending"),
                            path_label(&pending.path)
                        ),
                    });
                }
                return Err(error);
            }
        }
    }
    Ok(i18n::tr!("permissions-completed"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn linux_names_roundtrip_without_utf8_and_control_characters_are_visible() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp
            .path()
            .join(OsString::from_vec(b"file\n\x1b[2J\xff".to_vec()));
        fs::write(&path, "test").unwrap();
        let paths = roots(&BTreeMap::from([(
            "paths".into(),
            encode_paths(std::slice::from_ref(&path)),
        )]))
        .unwrap();
        assert_eq!(paths, vec![path.clone()]);
        assert!(path_label(&path).contains("\\n"));
        assert!(!path_label(&path).chars().any(char::is_control));
        assert_eq!(
            scan(&paths, false, &AtomicBool::new(false))
                .unwrap()
                .entries[0]
                .path,
            path
        );
    }
    #[test]
    fn directory_default_access_rules_are_detected_and_preserved() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("directory");
        fs::create_dir(&path).unwrap();
        let name = CString::new(path.as_os_str().as_bytes()).unwrap();
        let acl = [
            2u8, 0, 0, 0, 1, 0, 7, 0, 255, 255, 255, 255, 4, 0, 5, 0, 255, 255, 255, 255, 32, 0, 0,
            0, 255, 255, 255, 255,
        ];
        assert_eq!(
            unsafe {
                libc::setxattr(
                    name.as_ptr(),
                    c"system.posix_acl_default".as_ptr(),
                    acl.as_ptr().cast(),
                    acl.len(),
                    0,
                )
            },
            0
        );
        let before = entry(&path, &open(&path).unwrap()).unwrap();
        assert!(before.default_acl);
        apply(
            &before,
            PermissionChange {
                mask: 0o007,
                bits: 0,
                ..Default::default()
            },
        )
        .unwrap();
        assert!(entry(&path, &open(&path).unwrap()).unwrap().default_acl);
    }
    #[test]
    fn assigning_the_existing_owner_keeps_special_permissions() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("file");
        fs::write(&path, "test").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o4600)).unwrap();
        let before = entry(&path, &open(&path).unwrap()).unwrap();
        assert!(
            !apply(
                &before,
                PermissionChange {
                    uid: Some(before.uid),
                    gid: Some(before.gid),
                    ..Default::default()
                }
            )
            .unwrap()
        );
        assert_eq!(fs::metadata(path).unwrap().mode() & 0o7777, 0o4600);
    }
    #[test]
    fn masks_preserve_untouched_and_special_bits() {
        let command = ManagementCommand {
            kind: ManagementKind::FilePermissions,
            action: "change_permissions".into(),
            target: None,
            identity: BTreeMap::new(),
            values: BTreeMap::from([
                ("files_initial".into(), "1m0000000".into()),
                ("files".into(), "110000000".into()),
            ]),
        };
        let change = change(&command, false).unwrap();
        assert_eq!(change.mask, 0o200);
        assert_eq!((0o4755 & !change.mask) | change.bits, 0o4755);
    }
    #[test]
    fn scan_deduplicates_and_skips_links() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("root");
        fs::create_dir(&root).unwrap();
        let file = root.join("file");
        fs::write(&file, "test").unwrap();
        fs::hard_link(&file, root.join("hard")).unwrap();
        std::os::unix::fs::symlink(&file, root.join("link")).unwrap();
        let scan = scan(&[root.clone(), file], true, &AtomicBool::new(false)).unwrap();
        assert_eq!(scan.entries.len(), 2);
        assert_eq!(scan.skipped.len(), 1);
    }
    #[test]
    fn apply_checks_identity_and_changes_only_requested_bits() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("file");
        fs::write(&path, "test").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o640)).unwrap();
        let before = entry(&path, &open(&path).unwrap()).unwrap();
        apply(
            &before,
            PermissionChange {
                mask: 0o010,
                bits: 0o010,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(fs::metadata(&path).unwrap().mode() & 0o777, 0o650);
        assert!(matches!(
            apply(&before, PermissionChange::default()),
            Err(ManagementError::Conflict(_))
        ));
    }

    struct Review {
        accepted: bool,
        alter: Option<PathBuf>,
        cancel_after_output: Option<std::sync::Arc<AtomicBool>>,
        output: Vec<String>,
    }
    impl OperationInteraction for Review {
        fn emit(&mut self, event: OperationEvent) {
            if let OperationEvent::Output { text } = event {
                self.output.push(text);
                if let Some(flag) = &self.cancel_after_output {
                    flag.store(true, Ordering::Release);
                }
            }
        }
        fn ask(
            &mut self,
            _: &str,
            _: &str,
            choices: &[String],
            _: bool,
        ) -> Result<String, ManagementError> {
            if let Some(path) = self.alter.take() {
                fs::write(path, "changed").unwrap();
            }
            Ok(choices[usize::from(self.accepted)].clone())
        }
    }
    fn command(root: &Path) -> ManagementCommand {
        ManagementCommand {
            kind: ManagementKind::FilePermissions,
            action: "change_permissions".into(),
            target: None,
            identity: BTreeMap::new(),
            values: BTreeMap::from([
                ("paths".into(), encode_paths(&[root.into()])),
                ("recursive".into(), "true".into()),
                ("files_initial".into(), "mmmmmmmmm".into()),
                ("directories_initial".into(), "mmmmmmmmm".into()),
                ("files".into(), "100000000".into()),
                ("directories".into(), "101000000".into()),
            ]),
        }
    }
    fn context() -> ExecutionContext {
        ExecutionContext {
            actor_uid: unsafe { libc::getuid() },
            helper_path: PathBuf::new(),
            caller_pid: 0,
            caller_start_ticks: 0,
        }
    }
    #[test]
    fn recursive_changes_separate_files_and_directories_and_reject_stale_previews() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("root");
        fs::create_dir(&root).unwrap();
        let path = root.join("file");
        fs::write(&path, "test").unwrap();
        let mut review = Review {
            accepted: true,
            alter: Some(path.clone()),
            cancel_after_output: None,
            output: vec![],
        };
        assert!(matches!(
            execute(
                &command(&root),
                &context(),
                &mut review,
                &AtomicBool::new(false)
            ),
            Err(ManagementError::Conflict(_))
        ));
        execute(
            &command(&root),
            &context(),
            &mut review,
            &AtomicBool::new(false),
        )
        .unwrap();
        assert_eq!(fs::metadata(&path).unwrap().mode() & 0o777, 0o400);
        assert_eq!(fs::metadata(&root).unwrap().mode() & 0o777, 0o500);
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
    }
    #[test]
    fn decline_keeps_metadata_and_cancellation_reports_unprocessed_items() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("file");
        fs::write(&path, "test").unwrap();
        let mut review = Review {
            accepted: false,
            alter: None,
            cancel_after_output: None,
            output: vec![],
        };
        let before = entry(&path, &open(&path).unwrap()).unwrap();
        assert!(matches!(
            execute(
                &command(&path),
                &context(),
                &mut review,
                &AtomicBool::new(false)
            ),
            Err(ManagementError::Cancelled)
        ));
        assert_eq!(entry(&path, &open(&path).unwrap()).unwrap(), before);
        fs::write(temp.path().join("second"), "test").unwrap();
        let flag = std::sync::Arc::new(AtomicBool::new(false));
        review.accepted = true;
        review.cancel_after_output = Some(flag.clone());
        assert!(matches!(
            execute(&command(temp.path()), &context(), &mut review, &flag),
            Err(ManagementError::Cancelled)
        ));
        assert!(
            review
                .output
                .iter()
                .any(|line| line.starts_with(&i18n::tr!("permissions-pending")))
        );
    }
    #[test]
    fn each_checkbox_changes_its_own_bit() {
        for index in 0..9 {
            let temp = tempfile::tempdir().unwrap();
            let path = temp.path().join("file");
            fs::write(&path, "test").unwrap();
            fs::set_permissions(&path, fs::Permissions::from_mode(0o4000)).unwrap();
            let before = entry(&path, &open(&path).unwrap()).unwrap();
            let bit = 1 << index;
            apply(
                &before,
                PermissionChange {
                    mask: bit,
                    bits: bit,
                    ..Default::default()
                },
            )
            .unwrap();
            assert_eq!(fs::metadata(&path).unwrap().mode() & 0o7777, 0o4000 | bit);
        }
    }
}
