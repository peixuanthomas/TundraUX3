#[cfg(target_os = "macos")]
use std::collections::{BTreeMap, BTreeSet};
#[cfg(target_os = "macos")]
use std::ffi::{CString, c_void};
use std::ffi::{OsStr, OsString};
use std::io::Write;
#[cfg(target_os = "macos")]
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

#[cfg(target_os = "macos")]
use crate::VolumeKind;
use crate::paths::home_dir_from_env;
use crate::{
    AppPaths, CapabilityStatus, DirectoryListing, FileAttributes, FileOpenPolicy, LocalVolume,
    Platform, PlatformCapabilities, PlatformError, PlatformKind, ProcessExit, ProcessSpec,
    TrashEntry, TrashEntryId, TrashRestoreTarget, TrashStats, UserDirs, build_macos_app_paths,
};

const OPEN: &str = "/usr/bin/open";
const PBCOPY: &str = "/usr/bin/pbcopy";
const PBPASTE: &str = "/usr/bin/pbpaste";
#[cfg(target_os = "macos")]
const OSASCRIPT: &str = "/usr/bin/osascript";

#[cfg(target_os = "macos")]
unsafe extern "C" {
    fn kill(pid: i32, signal: i32) -> i32;
    fn __error() -> *mut i32;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct MacosPlatform;

impl Platform for MacosPlatform {
    fn kind(&self) -> PlatformKind {
        PlatformKind::Macos
    }

    fn capabilities(&self) -> PlatformCapabilities {
        let mut capabilities = PlatformCapabilities::native_supported();
        capabilities.critical_dialog = CapabilityStatus::BestEffort;
        capabilities
    }

    fn is_native_backend(&self) -> bool {
        true
    }

    fn user_dirs(&self) -> Result<UserDirs, PlatformError> {
        let home = home_dir_from_env()?;
        UserDirs::new(
            home.join("Desktop"),
            home.join("Documents"),
            home.join("Downloads"),
            home.join("Pictures"),
            home.join("Movies"),
            home.join("Music"),
            home.join("Library").join("Application Support"),
        )
        .map_err(Into::into)
    }

    fn app_paths(&self) -> Result<AppPaths, PlatformError> {
        build_macos_app_paths(home_dir_from_env()?, std::env::temp_dir()).map_err(Into::into)
    }

    fn open_path(&self, path: &Path) -> Result<(), PlatformError> {
        run_open([OsString::from("--"), path.as_os_str().to_os_string()])
    }

    fn open_with(&self, path: &Path, application: &Path) -> Result<(), PlatformError> {
        run_open([
            OsString::from("-a"),
            application.as_os_str().to_os_string(),
            OsString::from("--"),
            path.as_os_str().to_os_string(),
        ])
    }

    fn open_uri(&self, uri: &str) -> Result<(), PlatformError> {
        if uri.trim().is_empty() {
            return Err(PlatformError::InvalidInput {
                message: "URI must not be empty".to_string(),
            });
        }

        run_open([OsString::from(uri)])
    }

    fn spawn_detached(&self, spec: &ProcessSpec) -> Result<(), PlatformError> {
        crate::process::spawn_detached_impl(spec, false)
    }

    fn spawn_wait(&self, spec: &ProcessSpec) -> Result<ProcessExit, PlatformError> {
        crate::process::spawn_wait_impl(spec, false)
    }

    fn read_clipboard_text(&self) -> Result<String, PlatformError> {
        let output = Command::new(PBPASTE)
            .output()
            .map_err(|error| PlatformError::Io {
                operation: "read clipboard",
                path: Some(PathBuf::from(PBPASTE)),
                message: error.to_string(),
            })?;

        if !output.status.success() {
            return Err(PlatformError::CommandFailed {
                program: PBPASTE.to_string(),
                status: output.status.code(),
                stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
            });
        }

        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    }

    fn write_clipboard_text(&self, text: &str) -> Result<(), PlatformError> {
        let mut child = Command::new(PBCOPY)
            .stdin(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| PlatformError::Io {
                operation: "write clipboard",
                path: Some(PathBuf::from(PBCOPY)),
                message: error.to_string(),
            })?;

        child
            .stdin
            .take()
            .ok_or_else(|| PlatformError::Native {
                operation: "write clipboard",
                message: "pbcopy stdin is unavailable".to_string(),
            })?
            .write_all(text.as_bytes())
            .map_err(|error| PlatformError::Io {
                operation: "write clipboard",
                path: Some(PathBuf::from(PBCOPY)),
                message: error.to_string(),
            })?;

        let output = child
            .wait_with_output()
            .map_err(|error| PlatformError::Io {
                operation: "write clipboard",
                path: Some(PathBuf::from(PBCOPY)),
                message: error.to_string(),
            })?;

        if output.status.success() {
            Ok(())
        } else {
            Err(PlatformError::CommandFailed {
                program: PBCOPY.to_string(),
                status: output.status.code(),
                stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
            })
        }
    }

    fn show_critical_error(&self, title: &str, body: &str) -> Result<(), PlatformError> {
        macos_show_critical_error(title, body)
    }

    fn is_process_alive(&self, pid: u32) -> Result<bool, PlatformError> {
        macos_is_process_alive(pid)
    }

    fn local_volumes(&self) -> Result<Vec<LocalVolume>, PlatformError> {
        macos_local_volumes()
    }

    fn list_trash(&self) -> Result<Vec<TrashEntry>, PlatformError> {
        macos_list_trash(self)
    }

    fn trash_stats(&self) -> Result<TrashStats, PlatformError> {
        let entries = macos_list_trash(self)?;
        Ok(TrashStats {
            item_count: entries.len() as u64,
            total_bytes: entries
                .iter()
                .fold(0u64, |total, entry| total.saturating_add(entry.size)),
        })
    }

    fn move_to_trash(&self, paths: &[PathBuf]) -> Result<(), PlatformError> {
        macos_move_to_trash(self, paths)
    }

    fn empty_trash(&self) -> Result<(), PlatformError> {
        macos_empty_trash(self)
    }

    fn restore_trash_item(
        &self,
        id: &TrashEntryId,
        target: TrashRestoreTarget,
    ) -> Result<PathBuf, PlatformError> {
        macos_restore_trash_item(self, id, target)
    }

    fn file_attributes(&self, path: &Path) -> Result<FileAttributes, PlatformError> {
        let mut attributes = crate::default_file_attributes(path)?;
        apply_macos_file_flags(path, &mut attributes)?;
        Ok(attributes)
    }

    fn read_directory(&self, path: &Path) -> Result<DirectoryListing, PlatformError> {
        crate::default_read_directory(self, path)
    }

    fn file_open_policy(&self, path: &Path, attributes: &FileAttributes) -> FileOpenPolicy {
        crate::default_file_open_policy(PlatformKind::Macos, path, attributes)
    }
}

#[cfg(target_os = "macos")]
fn macos_show_critical_error(title: &str, body: &str) -> Result<(), PlatformError> {
    if title.contains('\0') || body.contains('\0') {
        return Err(PlatformError::InvalidInput {
            message: "critical error title and body must not contain NUL characters".to_string(),
        });
    }

    // Values are passed as argv rather than interpolated into AppleScript, so
    // report content cannot alter the script or invoke a shell.
    const SCRIPT: &str = r#"on run argv
set argumentCount to count argv
set dialogTitle to item (argumentCount - 1) of argv
set dialogBody to item argumentCount of argv
display alert dialogTitle message dialogBody as critical buttons {"OK"} default button "OK"
end run"#;
    let output = Command::new(OSASCRIPT)
        .arg("-e")
        .arg(SCRIPT)
        .arg("--")
        .arg(title)
        .arg(body)
        .output()
        .map_err(|error| PlatformError::Io {
            operation: "show critical error dialog",
            path: Some(PathBuf::from(OSASCRIPT)),
            message: error.to_string(),
        })?;

    if output.status.success() {
        Ok(())
    } else {
        Err(PlatformError::CommandFailed {
            program: OSASCRIPT.to_string(),
            status: output.status.code(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        })
    }
}

#[cfg(not(target_os = "macos"))]
fn macos_show_critical_error(_title: &str, _body: &str) -> Result<(), PlatformError> {
    Err(PlatformError::Unsupported {
        capability: "critical_dialog.macos",
    })
}

#[cfg(target_os = "macos")]
fn macos_is_process_alive(pid: u32) -> Result<bool, PlatformError> {
    const EPERM: i32 = 1;
    const ESRCH: i32 = 3;

    if pid == 0 || pid > i32::MAX as u32 {
        return Ok(false);
    }
    if unsafe { kill(pid as i32, 0) } == 0 {
        return Ok(true);
    }

    let errno = unsafe { *__error() };
    match errno {
        ESRCH => Ok(false),
        // Permission is checked after the kernel resolves the PID, so EPERM
        // is positive evidence that the protected process exists.
        EPERM => Ok(true),
        _ => Err(PlatformError::Native {
            operation: "kill(pid, 0)",
            message: std::io::Error::from_raw_os_error(errno).to_string(),
        }),
    }
}

#[cfg(not(target_os = "macos"))]
fn macos_is_process_alive(_pid: u32) -> Result<bool, PlatformError> {
    Err(PlatformError::Unsupported {
        capability: "process_liveness.macos",
    })
}

#[cfg(target_os = "macos")]
#[derive(Debug, Clone)]
struct TrashIndexEntry {
    id: String,
    trash_path: PathBuf,
    original_path: PathBuf,
}

#[cfg(target_os = "macos")]
fn macos_local_volumes() -> Result<Vec<LocalVolume>, PlatformError> {
    let root = PathBuf::from("/");
    let root_probe = macos_volume_probe(&root);
    let mut physical_disks = BTreeSet::new();
    let mut mount_devices = BTreeSet::new();
    if let Ok(metadata) = std::fs::metadata(&root) {
        mount_devices.insert(metadata.dev());
    }
    if let Some(probe) = &root_probe {
        physical_disks.insert(probe.physical_disk.clone());
    }
    let mut volumes = vec![LocalVolume {
        root: root.clone(),
        label: Some("Macintosh HD".to_string()),
        kind: root_probe
            .map(|probe| probe.kind)
            .unwrap_or(VolumeKind::Fixed),
        total_bytes: None,
        available_bytes: None,
    }];
    match std::fs::read_dir("/Volumes") {
        Ok(directory) => {
            for entry in directory {
                let entry =
                    entry.map_err(|error| macos_io("enumerate mounted volumes", None, error))?;
                let path = entry.path();
                let metadata = match std::fs::symlink_metadata(&path) {
                    Ok(metadata) => metadata,
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                    Err(error) => {
                        return Err(macos_io("inspect mounted volume", Some(&path), error));
                    }
                };
                // /Volumes can contain aliases and network mounts. diskutil
                // only resolves local disk-backed mount points, so requiring a
                // successful result excludes SMB/NFS shares and stale links.
                if !metadata.is_dir() || metadata.file_type().is_symlink() {
                    continue;
                }
                if !mount_devices.insert(metadata.dev()) {
                    continue;
                }
                let Some(probe) = macos_volume_probe(&path) else {
                    mount_devices.remove(&metadata.dev());
                    continue;
                };
                if !physical_disks.insert(probe.physical_disk) {
                    mount_devices.remove(&metadata.dev());
                    continue;
                }
                volumes.push(LocalVolume {
                    label: path
                        .file_name()
                        .map(|name| name.to_string_lossy().into_owned()),
                    root: path,
                    kind: probe.kind,
                    total_bytes: None,
                    available_bytes: None,
                });
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(macos_io(
                "enumerate mounted volumes",
                Some(Path::new("/Volumes")),
                error,
            ));
        }
    }

    volumes.sort_by(|left, right| left.root.cmp(&right.root));
    volumes.dedup_by(|left, right| left.root == right.root);
    Ok(volumes)
}

#[cfg(target_os = "macos")]
struct MacVolumeProbe {
    kind: VolumeKind,
    physical_disk: String,
}

#[cfg(target_os = "macos")]
fn macos_volume_probe(root: &Path) -> Option<MacVolumeProbe> {
    let plist = diskutil_info(root.as_os_str())?;
    let physical_disk = plist_string(&plist, "ParentWholeDisk")
        .or_else(|| plist_string(&plist, "DeviceIdentifier"))?
        .to_string();
    let whole_disk = diskutil_info(OsStr::new(&format!("/dev/{physical_disk}")))?;
    if plist_string(&whole_disk, "VirtualOrPhysical") != Some("Physical")
        || plist_protocol_is_nonlocal(&plist)
        || plist_protocol_is_nonlocal(&whole_disk)
    {
        return None;
    }
    let kind = if plist_bool(&whole_disk, "Internal") == Some(true)
        && plist_bool(&whole_disk, "RemovableMedia") != Some(true)
        && plist_bool(&whole_disk, "Ejectable") != Some(true)
    {
        VolumeKind::Fixed
    } else {
        VolumeKind::Removable
    };
    Some(MacVolumeProbe {
        kind,
        physical_disk,
    })
}

#[cfg(target_os = "macos")]
fn diskutil_info(target: &OsStr) -> Option<String> {
    let output = Command::new("/usr/sbin/diskutil")
        .args(["info", "-plist"])
        .arg(target)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).into_owned())
}

#[cfg(target_os = "macos")]
fn plist_bool(plist: &str, key: &str) -> Option<bool> {
    let marker = format!("<key>{key}</key>");
    let remainder = plist.split_once(&marker)?.1.trim_start();
    if remainder.starts_with("<true/>") {
        Some(true)
    } else if remainder.starts_with("<false/>") {
        Some(false)
    } else {
        None
    }
}

#[cfg(target_os = "macos")]
fn plist_string<'a>(plist: &'a str, key: &str) -> Option<&'a str> {
    let marker = format!("<key>{key}</key>");
    let remainder = plist.split_once(&marker)?.1.trim_start();
    remainder
        .strip_prefix("<string>")?
        .split_once("</string>")
        .map(|(value, _)| value)
}

#[cfg(target_os = "macos")]
fn plist_protocol_is_nonlocal(plist: &str) -> bool {
    ["BusProtocol", "Protocol"]
        .into_iter()
        .filter_map(|key| plist_string(plist, key))
        .any(|protocol| {
            let protocol = protocol.to_ascii_lowercase();
            protocol.contains("network")
                || protocol.contains("disk image")
                || protocol.contains("virtual")
                || protocol.contains("smb")
                || protocol.contains("nfs")
        })
}

#[cfg(target_os = "macos")]
fn macos_list_trash(platform: &MacosPlatform) -> Result<Vec<TrashEntry>, PlatformError> {
    let index = load_trash_index(platform)?;
    let by_path: BTreeMap<&Path, &TrashIndexEntry> = index
        .values()
        .map(|entry| (entry.trash_path.as_path(), entry))
        .collect();
    let mut entries = Vec::new();
    for root in macos_trash_roots()? {
        let directory = match std::fs::read_dir(&root) {
            Ok(directory) => directory,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(macos_io("list Trash", Some(&root), error)),
        };
        for item in directory {
            let item = item.map_err(|error| macos_io("list Trash", Some(&root), error))?;
            let path = item.path();
            let metadata = std::fs::symlink_metadata(&path)
                .map_err(|error| macos_io("read Trash item metadata", Some(&path), error))?;
            let indexed = by_path.get(path.as_path()).copied();
            let id = indexed
                .map(|entry| entry.id.clone())
                .unwrap_or_else(|| format!("mac-path-v1-{}", encode_path(&path)));
            entries.push(TrashEntry {
                id: TrashEntryId::from_native(id),
                display_name: item.file_name().to_string_lossy().into_owned(),
                original_path: indexed.map(|entry| entry.original_path.clone()),
                deleted_at: metadata.modified().ok(),
                size: path_size(&path, &metadata),
                is_directory: metadata.is_dir(),
            });
        }
    }
    entries.sort_by(|left, right| {
        left.display_name
            .to_lowercase()
            .cmp(&right.display_name.to_lowercase())
            .then_with(|| left.id.cmp(&right.id))
    });
    Ok(entries)
}

#[cfg(target_os = "macos")]
fn macos_move_to_trash(platform: &MacosPlatform, paths: &[PathBuf]) -> Result<(), PlatformError> {
    if paths.is_empty() {
        return Err(PlatformError::InvalidInput {
            message: "at least one path is required to move to Trash".to_string(),
        });
    }
    let mut index = load_trash_index(platform)?;
    for source in paths {
        validate_trash_source(source)?;
        let trash_root = trash_root_for_source(source)?;
        ensure_trash_root(&trash_root)?;
        let name = source
            .file_name()
            .ok_or_else(|| PlatformError::InvalidInput {
                message: format!(
                    "cannot move a filesystem root to Trash: {}",
                    source.display()
                ),
            })?;
        let trash_path = unique_trash_path(&trash_root, name);
        let id = unique_trash_id(&index);
        index.insert(
            id.clone(),
            TrashIndexEntry {
                id: id.clone(),
                trash_path: trash_path.clone(),
                original_path: source.clone(),
            },
        );
        save_trash_index(platform, &index)?;
        if let Err(error) = macos_rename_exclusive(source, &trash_path) {
            index.remove(&id);
            let cleanup_error = save_trash_index(platform, &index).err();
            let mut mapped = if error.raw_os_error() == Some(18) {
                PlatformError::CrossDevice {
                    source: source.clone(),
                    target: trash_path,
                    message: "the source volume does not expose its per-user system Trash"
                        .to_string(),
                }
            } else {
                macos_io("move item to Trash", Some(source), error)
            };
            if let Some(cleanup_error) = cleanup_error {
                mapped = PlatformError::Native {
                    operation: "move item to Trash",
                    message: format!(
                        "{mapped}; additionally failed to roll back Trash index: {cleanup_error}"
                    ),
                };
            }
            return Err(mapped);
        }
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn macos_empty_trash(platform: &MacosPlatform) -> Result<(), PlatformError> {
    let mut first_error = None;
    for root in macos_trash_roots()? {
        let directory = match std::fs::read_dir(&root) {
            Ok(directory) => directory,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(macos_io("open Trash for emptying", Some(&root), error)),
        };
        for item in directory {
            let result = item
                .map_err(|error| macos_io("enumerate Trash for emptying", Some(&root), error))
                .and_then(|item| permanently_remove_trash_child(&root, &item.path()));
            if first_error.is_none() {
                first_error = result.err();
            }
        }
    }
    let mut index = load_trash_index(platform)?;
    index.retain(|_, entry| std::fs::symlink_metadata(&entry.trash_path).is_ok());
    save_trash_index(platform, &index)?;
    match first_error {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

#[cfg(target_os = "macos")]
fn macos_restore_trash_item(
    platform: &MacosPlatform,
    id: &TrashEntryId,
    target: TrashRestoreTarget,
) -> Result<PathBuf, PlatformError> {
    let mut index = load_trash_index(platform)?;
    let indexed = index.get(id.as_str()).cloned();
    let source = if let Some(entry) = &indexed {
        entry.trash_path.clone()
    } else if let Some(encoded) = id.as_str().strip_prefix("mac-path-v1-") {
        decode_path(encoded)?
    } else {
        return Err(PlatformError::InvalidInput {
            message: "Trash item no longer exists".to_string(),
        });
    };
    ensure_direct_trash_child(&source)?;
    std::fs::symlink_metadata(&source)
        .map_err(|error| macos_io("locate Trash item", Some(&source), error))?;

    let destination = match target {
        TrashRestoreTarget::OriginalLocation => indexed
            .as_ref()
            .map(|entry| entry.original_path.clone())
            .ok_or_else(|| PlatformError::InvalidInput {
                message: "this macOS Trash item has no recorded original path; provide an absolute destination path".to_string(),
            })?,
        TrashRestoreTarget::DestinationPath(path) => path,
    };
    validate_restore_destination(&destination)?;
    match macos_rename_exclusive(&source, &destination) {
        Ok(()) => {}
        Err(error) if error.raw_os_error() == Some(18) => {
            restore_across_volumes(&source, &destination)?;
        }
        Err(error) => {
            return Err(macos_io("restore Trash item", Some(&destination), error));
        }
    }

    if indexed.is_some() {
        index.remove(id.as_str());
        save_trash_index(platform, &index)?;
    }
    Ok(destination)
}

#[cfg(target_os = "macos")]
fn validate_trash_source(source: &Path) -> Result<(), PlatformError> {
    validate_lexically_safe_absolute_path(source, "Trash source")?;
    std::fs::symlink_metadata(source)
        .map_err(|error| macos_io("locate item to move to Trash", Some(source), error))?;
    let home_trash = home_dir_from_env()?.join(".Trash");
    let volumes = macos_local_volumes()?;
    let is_trash_path = source.starts_with(&home_trash)
        || volumes
            .iter()
            .filter(|volume| volume.root != Path::new("/"))
            .any(|volume| source.starts_with(volume.root.join(".Trashes")));
    let volume = volume_for_path(source, &volumes)?;
    if source.file_name().is_none() || source == volume.root || is_trash_path {
        return Err(PlatformError::InvalidInput {
            message: format!(
                "cannot move this filesystem, mounted volume, or Trash path: {}",
                source.display()
            ),
        });
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn validate_restore_destination(destination: &Path) -> Result<(), PlatformError> {
    validate_lexically_safe_absolute_path(destination, "restore destination")?;
    if destination.file_name().is_none() {
        return Err(PlatformError::InvalidInput {
            message: "restore destination must be a complete absolute path".to_string(),
        });
    }
    if macos_trash_roots()?
        .iter()
        .any(|root| destination.starts_with(root))
    {
        return Err(PlatformError::InvalidInput {
            message: "restore destination must be outside the system Trash".to_string(),
        });
    }
    match std::fs::symlink_metadata(destination) {
        Ok(_) => {
            return Err(PlatformError::InvalidInput {
                message: format!(
                    "restore destination already exists: {}",
                    destination.display()
                ),
            });
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(macos_io(
                "check restore destination",
                Some(destination),
                error,
            ));
        }
    }
    let parent = destination
        .parent()
        .ok_or_else(|| PlatformError::InvalidInput {
            message: "restore destination has no parent directory".to_string(),
        })?;
    if !std::fs::metadata(parent).is_ok_and(|metadata| metadata.is_dir()) {
        return Err(PlatformError::InvalidInput {
            message: format!(
                "restore destination parent is not a directory: {}",
                parent.display()
            ),
        });
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn trash_root_for_source(source: &Path) -> Result<PathBuf, PlatformError> {
    validate_lexically_safe_absolute_path(source, "Trash source")?;
    let volumes = macos_local_volumes()?;
    let volume = volume_for_path(source, &volumes)?;
    if volume.root == Path::new("/") {
        Ok(home_dir_from_env()?.join(".Trash"))
    } else {
        Ok(volume
            .root
            .join(".Trashes")
            .join(effective_uid().to_string()))
    }
}

#[cfg(target_os = "macos")]
fn macos_trash_roots() -> Result<Vec<PathBuf>, PlatformError> {
    let home_trash = home_dir_from_env()?.join(".Trash");
    let mut roots = Vec::new();
    if validate_existing_trash_root(&home_trash)? {
        roots.push(home_trash);
    }
    for volume in macos_local_volumes()?
        .into_iter()
        .filter(|volume| volume.root != Path::new("/"))
    {
        let root = volume
            .root
            .join(".Trashes")
            .join(effective_uid().to_string());
        if validate_existing_trash_root(&root)? {
            roots.push(root);
        }
    }
    roots.sort();
    roots.dedup();
    Ok(roots)
}

#[cfg(target_os = "macos")]
fn effective_uid() -> u32 {
    unsafe extern "C" {
        fn geteuid() -> u32;
    }
    unsafe { geteuid() }
}

#[cfg(target_os = "macos")]
#[link(name = "System")]
unsafe extern "C" {
    fn renamex_np(
        old_path: *const std::ffi::c_char,
        new_path: *const std::ffi::c_char,
        flags: u32,
    ) -> i32;
    fn copyfile(
        from: *const std::ffi::c_char,
        to: *const std::ffi::c_char,
        state: *mut c_void,
        flags: u32,
    ) -> i32;
}

#[cfg(target_os = "macos")]
fn ensure_trash_root(root: &Path) -> Result<(), PlatformError> {
    use std::os::unix::fs::PermissionsExt;
    let home_trash = home_dir_from_env()?.join(".Trash");
    if root != home_trash {
        let recognized = macos_local_volumes()?
            .into_iter()
            .filter(|volume| volume.root != Path::new("/"))
            .any(|volume| {
                root == volume
                    .root
                    .join(".Trashes")
                    .join(effective_uid().to_string())
            });
        if !recognized {
            return Err(PlatformError::InvalidInput {
                message: format!("unrecognized system Trash directory: {}", root.display()),
            });
        }
        let container = root.parent().expect("external Trash root has a parent");
        if !real_directory_exists(container, "inspect volume Trash directory")? {
            std::fs::create_dir(container).map_err(|error| {
                macos_io("create volume Trash directory", Some(container), error)
            })?;
            std::fs::set_permissions(container, std::fs::Permissions::from_mode(0o1777)).map_err(
                |error| macos_io("secure volume Trash directory", Some(container), error),
            )?;
        }
    }
    if !validate_existing_trash_root(root)? {
        std::fs::create_dir(root)
            .map_err(|error| macos_io("create per-user Trash directory", Some(root), error))?;
    }
    std::fs::set_permissions(root, std::fs::Permissions::from_mode(0o700))
        .map_err(|error| macos_io("secure per-user Trash directory", Some(root), error))?;
    Ok(())
}

#[cfg(target_os = "macos")]
fn ensure_direct_trash_child(path: &Path) -> Result<(), PlatformError> {
    validate_lexically_safe_absolute_path(path, "Trash item")?;
    let parent = path.parent().ok_or_else(|| PlatformError::InvalidInput {
        message: "invalid Trash item identifier".to_string(),
    })?;
    if macos_trash_roots()?.iter().any(|root| root == parent)
        && validate_existing_trash_root(parent)?
    {
        Ok(())
    } else {
        Err(PlatformError::InvalidInput {
            message: "Trash item identifier is outside a system Trash directory".to_string(),
        })
    }
}

#[cfg(target_os = "macos")]
fn validate_lexically_safe_absolute_path(
    path: &Path,
    description: &str,
) -> Result<(), PlatformError> {
    if !path.is_absolute()
        || path.components().any(|component| {
            matches!(
                component,
                std::path::Component::CurDir | std::path::Component::ParentDir
            )
        })
    {
        return Err(PlatformError::InvalidInput {
            message: format!(
                "{description} must be an absolute path without '.' or '..': {}",
                path.display()
            ),
        });
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn volume_for_path<'a>(
    path: &Path,
    volumes: &'a [LocalVolume],
) -> Result<&'a LocalVolume, PlatformError> {
    let volume = volumes
        .iter()
        .filter(|volume| path.starts_with(&volume.root))
        .max_by_key(|volume| volume.root.components().count())
        .ok_or_else(|| PlatformError::InvalidInput {
            message: format!(
                "path is not on a recognized local volume: {}",
                path.display()
            ),
        })?;
    if path.starts_with("/Volumes") && volume.root == Path::new("/") {
        return Err(PlatformError::InvalidInput {
            message: format!(
                "network or unrecognized volumes do not provide a supported system Trash: {}",
                path.display()
            ),
        });
    }
    Ok(volume)
}

#[cfg(target_os = "macos")]
fn real_directory_exists(path: &Path, operation: &'static str) -> Result<bool, PlatformError> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => Ok(true),
        Ok(_) => Err(PlatformError::InvalidInput {
            message: format!(
                "refusing to use a non-directory or symbolic-link Trash path: {}",
                path.display()
            ),
        }),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(macos_io(operation, Some(path), error)),
    }
}

#[cfg(target_os = "macos")]
fn validate_existing_trash_root(root: &Path) -> Result<bool, PlatformError> {
    let home_trash = home_dir_from_env()?.join(".Trash");
    if root != home_trash {
        let container = root.parent().ok_or_else(|| PlatformError::InvalidInput {
            message: "invalid volume Trash directory".to_string(),
        })?;
        if !real_directory_exists(container, "inspect volume Trash directory")? {
            return Ok(false);
        }
    }
    real_directory_exists(root, "inspect per-user Trash directory")
}

#[cfg(target_os = "macos")]
fn unique_trash_path(root: &Path, name: &OsStr) -> PathBuf {
    let original = Path::new(name);
    let stem = original.file_stem().unwrap_or(name).to_os_string();
    let extension = original.extension().map(|value| value.to_os_string());
    let mut candidate = root.join(name);
    let mut suffix = 2u64;
    while std::fs::symlink_metadata(&candidate).is_ok() {
        let mut candidate_name = stem.clone();
        candidate_name.push(format!(" {suffix}"));
        if let Some(extension) = &extension {
            candidate_name.push(".");
            candidate_name.push(extension);
        }
        candidate = root.join(candidate_name);
        suffix += 1;
    }
    candidate
}

#[cfg(target_os = "macos")]
fn unique_trash_id(index: &BTreeMap<String, TrashIndexEntry>) -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};
    static NEXT_ID: AtomicU64 = AtomicU64::new(0);
    loop {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let sequence = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let id = format!("mac-v1-{nanos:x}-{:x}-{sequence:x}", std::process::id());
        if !index.contains_key(&id) {
            return id;
        }
    }
}

#[cfg(target_os = "macos")]
fn trash_index_path(platform: &MacosPlatform) -> Result<PathBuf, PlatformError> {
    Ok(platform.app_paths()?.data_path().join("trash-index-v1"))
}

#[cfg(target_os = "macos")]
fn load_trash_index(
    platform: &MacosPlatform,
) -> Result<BTreeMap<String, TrashIndexEntry>, PlatformError> {
    let path = trash_index_path(platform)?;
    let contents = match std::fs::read_to_string(&path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(BTreeMap::new()),
        Err(error) => return Err(macos_io("read Trash index", Some(&path), error)),
    };
    let mut index = BTreeMap::new();
    for (line_number, line) in contents.lines().enumerate() {
        if line.is_empty() {
            continue;
        }
        let mut fields = line.split('\t');
        let version = fields.next();
        let id = fields.next();
        let trash_path = fields.next();
        let original_path = fields.next();
        if version != Some("v1") || fields.next().is_some() {
            return Err(PlatformError::Native {
                operation: "read Trash index",
                message: format!("invalid record on line {}", line_number + 1),
            });
        }
        let id = id
            .filter(|id| !id.is_empty())
            .ok_or_else(|| PlatformError::Native {
                operation: "read Trash index",
                message: format!("missing id on line {}", line_number + 1),
            })?;
        let entry = TrashIndexEntry {
            id: id.to_string(),
            trash_path: decode_path(trash_path.unwrap_or_default())?,
            original_path: decode_path(original_path.unwrap_or_default())?,
        };
        index.insert(entry.id.clone(), entry);
    }
    Ok(index)
}

#[cfg(target_os = "macos")]
fn save_trash_index(
    platform: &MacosPlatform,
    index: &BTreeMap<String, TrashIndexEntry>,
) -> Result<(), PlatformError> {
    use std::fs::OpenOptions;
    let path = trash_index_path(platform)?;
    let parent = path.parent().expect("Trash index path has a parent");
    std::fs::create_dir_all(parent)
        .map_err(|error| macos_io("create Trash index directory", Some(parent), error))?;
    let temp = parent.join(format!(
        ".trash-index-v1.tmp-{}-{}",
        std::process::id(),
        unique_trash_id(index)
    ));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp)
        .map_err(|error| macos_io("create temporary Trash index", Some(&temp), error))?;
    for entry in index.values() {
        writeln!(
            file,
            "v1\t{}\t{}\t{}",
            entry.id,
            encode_path(&entry.trash_path),
            encode_path(&entry.original_path)
        )
        .map_err(|error| macos_io("write Trash index", Some(&temp), error))?;
    }
    file.sync_all()
        .map_err(|error| macos_io("sync Trash index", Some(&temp), error))?;
    drop(file);
    if let Err(error) = std::fs::rename(&temp, &path) {
        let _ = std::fs::remove_file(&temp);
        return Err(macos_io("replace Trash index", Some(&path), error));
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn encode_path(path: &Path) -> String {
    use std::os::unix::ffi::OsStrExt;
    path.as_os_str()
        .as_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[cfg(target_os = "macos")]
fn decode_path(encoded: &str) -> Result<PathBuf, PlatformError> {
    use std::os::unix::ffi::OsStringExt;
    if encoded.len() % 2 != 0 {
        return Err(PlatformError::InvalidInput {
            message: "invalid Trash item identifier".to_string(),
        });
    }
    let mut bytes = Vec::with_capacity(encoded.len() / 2);
    for pair in encoded.as_bytes().chunks_exact(2) {
        let pair = std::str::from_utf8(pair).map_err(|_| PlatformError::InvalidInput {
            message: "invalid Trash item identifier".to_string(),
        })?;
        bytes.push(
            u8::from_str_radix(pair, 16).map_err(|_| PlatformError::InvalidInput {
                message: "invalid Trash item identifier".to_string(),
            })?,
        );
    }
    Ok(PathBuf::from(OsString::from_vec(bytes)))
}

#[cfg(target_os = "macos")]
fn path_size(path: &Path, metadata: &std::fs::Metadata) -> u64 {
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return metadata.len();
    }
    let mut total = metadata.len();
    if let Ok(directory) = std::fs::read_dir(path) {
        for entry in directory.flatten() {
            if let Ok(metadata) = std::fs::symlink_metadata(entry.path()) {
                total = total.saturating_add(path_size(&entry.path(), &metadata));
            }
        }
    }
    total
}

#[cfg(target_os = "macos")]
fn macos_rename_exclusive(source: &Path, destination: &Path) -> Result<(), std::io::Error> {
    use std::os::unix::ffi::OsStrExt;
    const RENAME_EXCL: u32 = 0x0000_0004;
    let source = CString::new(source.as_os_str().as_bytes())
        .map_err(|_| std::io::Error::from(std::io::ErrorKind::InvalidInput))?;
    let destination = CString::new(destination.as_os_str().as_bytes())
        .map_err(|_| std::io::Error::from(std::io::ErrorKind::InvalidInput))?;
    let status = unsafe { renamex_np(source.as_ptr(), destination.as_ptr(), RENAME_EXCL) };
    if status == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

#[cfg(target_os = "macos")]
fn restore_across_volumes(source: &Path, destination: &Path) -> Result<(), PlatformError> {
    let parent = destination
        .parent()
        .expect("validated restore destination has a parent");
    let staging = unique_restore_staging_path(parent);
    if let Err(error) =
        copy_path_for_restore(source, &staging).and_then(|()| sync_copied_tree(&staging))
    {
        let cleanup = cleanup_restore_staging(parent, &staging).err();
        return Err(match cleanup {
            Some(cleanup) => PlatformError::Native {
                operation: "stage cross-volume Trash restore",
                message: format!("{error}; additionally failed to clean staging data: {cleanup}"),
            },
            None => error,
        });
    }
    if let Err(error) = macos_rename_exclusive(&staging, destination) {
        let mapped = macos_io(
            "commit cross-volume Trash restore",
            Some(destination),
            error,
        );
        let cleanup = cleanup_restore_staging(parent, &staging).err();
        return Err(match cleanup {
            Some(cleanup) => PlatformError::Native {
                operation: "commit cross-volume Trash restore",
                message: format!("{mapped}; additionally failed to clean staging data: {cleanup}"),
            },
            None => mapped,
        });
    }
    sync_directory(parent)?;

    // The destination is durable before the Trash source is removed. A
    // removal failure can leave two copies, but can never lose the item.
    let trash_root = source.parent().expect("Trash item has a parent");
    permanently_remove_trash_child(trash_root, source)?;
    sync_directory(trash_root)?;
    Ok(())
}

#[cfg(target_os = "macos")]
fn unique_restore_staging_path(parent: &Path) -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};
    static NEXT_STAGE: AtomicU64 = AtomicU64::new(0);
    loop {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let sequence = NEXT_STAGE.fetch_add(1, Ordering::Relaxed);
        let candidate = parent.join(format!(
            ".tundra-restore-stage-{}-{nanos:x}-{sequence:x}",
            std::process::id()
        ));
        if matches!(
            std::fs::symlink_metadata(&candidate),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound
        ) {
            return candidate;
        }
    }
}

#[cfg(target_os = "macos")]
fn copy_path_for_restore(source: &Path, staging: &Path) -> Result<(), PlatformError> {
    use std::os::unix::ffi::OsStrExt;
    const COPYFILE_ALL: u32 = 0x0000_000f;
    const COPYFILE_RECURSIVE: u32 = 0x0000_8000;
    const COPYFILE_EXCL: u32 = 0x0002_0000;
    const COPYFILE_NOFOLLOW_SRC: u32 = 0x0004_0000;
    const COPYFILE_NOFOLLOW_DST: u32 = 0x0008_0000;
    let source_c =
        CString::new(source.as_os_str().as_bytes()).map_err(|_| PlatformError::InvalidInput {
            message: "Trash source contains an invalid NUL byte".to_string(),
        })?;
    let staging_c =
        CString::new(staging.as_os_str().as_bytes()).map_err(|_| PlatformError::InvalidInput {
            message: "restore destination contains an invalid NUL byte".to_string(),
        })?;
    let status = unsafe {
        copyfile(
            source_c.as_ptr(),
            staging_c.as_ptr(),
            std::ptr::null_mut(),
            COPYFILE_ALL
                | COPYFILE_RECURSIVE
                | COPYFILE_EXCL
                | COPYFILE_NOFOLLOW_SRC
                | COPYFILE_NOFOLLOW_DST,
        )
    };
    if status == 0 {
        Ok(())
    } else {
        Err(macos_io(
            "copy Trash item for cross-volume restore",
            Some(source),
            std::io::Error::last_os_error(),
        ))
    }
}

#[cfg(target_os = "macos")]
fn sync_copied_tree(path: &Path) -> Result<(), PlatformError> {
    let metadata = std::fs::symlink_metadata(path)
        .map_err(|error| macos_io("inspect restored staging data", Some(path), error))?;
    if metadata.file_type().is_symlink() {
        return Ok(());
    }
    if metadata.is_dir() {
        for entry in std::fs::read_dir(path)
            .map_err(|error| macos_io("read restored staging directory", Some(path), error))?
        {
            let entry = entry
                .map_err(|error| macos_io("read restored staging directory", Some(path), error))?;
            sync_copied_tree(&entry.path())?;
        }
        return sync_directory(path);
    }
    if metadata.is_file() {
        return std::fs::File::open(path)
            .and_then(|file| file.sync_all())
            .map_err(|error| macos_io("sync restored staging file", Some(path), error));
    }
    Err(PlatformError::InvalidInput {
        message: format!(
            "cross-volume restore does not support this special file: {}",
            path.display()
        ),
    })
}

#[cfg(target_os = "macos")]
fn sync_directory(path: &Path) -> Result<(), PlatformError> {
    std::fs::File::open(path)
        .and_then(|file| file.sync_all())
        .map_err(|error| macos_io("sync directory", Some(path), error))
}

#[cfg(target_os = "macos")]
fn cleanup_restore_staging(parent: &Path, staging: &Path) -> Result<(), PlatformError> {
    if staging.parent() != Some(parent)
        || !staging
            .file_name()
            .is_some_and(|name| name.to_string_lossy().starts_with(".tundra-restore-stage-"))
    {
        return Err(PlatformError::InvalidInput {
            message: "refusing to clean an unrecognized restore staging path".to_string(),
        });
    }
    match std::fs::symlink_metadata(staging) {
        Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {
            std::fs::remove_dir_all(staging)
                .map_err(|error| macos_io("clean restore staging directory", Some(staging), error))
        }
        Ok(_) => std::fs::remove_file(staging)
            .map_err(|error| macos_io("clean restore staging file", Some(staging), error)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(macos_io(
            "inspect restore staging data",
            Some(staging),
            error,
        )),
    }
}

#[cfg(target_os = "macos")]
fn permanently_remove_trash_child(root: &Path, child: &Path) -> Result<(), PlatformError> {
    if child.parent() != Some(root) {
        return Err(PlatformError::InvalidInput {
            message: "refusing to empty an item outside the system Trash".to_string(),
        });
    }
    let root_metadata = std::fs::symlink_metadata(root)
        .map_err(|error| macos_io("inspect Trash root before emptying", Some(root), error))?;
    if !root_metadata.is_dir() || root_metadata.file_type().is_symlink() {
        return Err(PlatformError::InvalidInput {
            message: "refusing to empty a non-directory or symbolic-link Trash root".to_string(),
        });
    }
    remove_tree_without_following_mounts(child, root_metadata.dev())
}

#[cfg(target_os = "macos")]
fn remove_tree_without_following_mounts(
    path: &Path,
    expected_device: u64,
) -> Result<(), PlatformError> {
    let metadata = std::fs::symlink_metadata(path)
        .map_err(|error| macos_io("inspect path before permanent removal", Some(path), error))?;
    if metadata.is_dir() && !metadata.file_type().is_symlink() {
        if metadata.dev() != expected_device {
            return Err(PlatformError::InvalidInput {
                message: format!(
                    "refusing to traverse a mounted filesystem while emptying Trash: {}",
                    path.display()
                ),
            });
        }
        for entry in std::fs::read_dir(path).map_err(|error| {
            macos_io("read directory before permanent removal", Some(path), error)
        })? {
            let entry = entry.map_err(|error| {
                macos_io("read directory before permanent removal", Some(path), error)
            })?;
            remove_tree_without_following_mounts(&entry.path(), expected_device)?;
        }
        std::fs::remove_dir(path)
            .map_err(|error| macos_io("permanently remove Trash directory", Some(path), error))
    } else {
        std::fs::remove_file(path)
            .map_err(|error| macos_io("permanently remove Trash item", Some(path), error))
    }
}

#[cfg(target_os = "macos")]
fn macos_io(operation: &'static str, path: Option<&Path>, error: std::io::Error) -> PlatformError {
    let message = if error.kind() == std::io::ErrorKind::PermissionDenied {
        format!("{error}; grant TundraUX Full Disk Access in System Settings")
    } else {
        error.to_string()
    };
    PlatformError::Io {
        operation,
        path: path.map(Path::to_path_buf),
        message,
    }
}

#[cfg(not(target_os = "macos"))]
fn macos_local_volumes() -> Result<Vec<LocalVolume>, PlatformError> {
    Err(PlatformError::Unsupported {
        capability: "local_volumes.macos",
    })
}

#[cfg(not(target_os = "macos"))]
fn macos_list_trash(_platform: &MacosPlatform) -> Result<Vec<TrashEntry>, PlatformError> {
    Err(PlatformError::Unsupported {
        capability: "trash.macos",
    })
}

#[cfg(not(target_os = "macos"))]
fn macos_move_to_trash(_platform: &MacosPlatform, _paths: &[PathBuf]) -> Result<(), PlatformError> {
    Err(PlatformError::Unsupported {
        capability: "trash.macos",
    })
}

#[cfg(not(target_os = "macos"))]
fn macos_empty_trash(_platform: &MacosPlatform) -> Result<(), PlatformError> {
    Err(PlatformError::Unsupported {
        capability: "trash.macos",
    })
}

#[cfg(not(target_os = "macos"))]
fn macos_restore_trash_item(
    _platform: &MacosPlatform,
    _id: &TrashEntryId,
    _target: TrashRestoreTarget,
) -> Result<PathBuf, PlatformError> {
    Err(PlatformError::Unsupported {
        capability: "trash.macos",
    })
}

#[cfg(target_os = "macos")]
fn apply_macos_file_flags(
    path: &Path,
    attributes: &mut FileAttributes,
) -> Result<(), PlatformError> {
    use std::fs;
    use std::os::macos::fs::MetadataExt;

    const UF_HIDDEN: u32 = 0x0000_8000;
    let metadata = fs::symlink_metadata(path).map_err(|error| PlatformError::Io {
        operation: "read macOS file flags",
        path: Some(path.to_path_buf()),
        message: error.to_string(),
    })?;
    attributes.hidden = attributes.hidden || metadata.st_flags() & UF_HIDDEN != 0;
    Ok(())
}

#[cfg(not(target_os = "macos"))]
fn apply_macos_file_flags(
    _path: &Path,
    _attributes: &mut FileAttributes,
) -> Result<(), PlatformError> {
    Ok(())
}

fn run_open<I, S>(args: I) -> Result<(), PlatformError>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let output = Command::new(OPEN)
        .args(args)
        .output()
        .map_err(|error| PlatformError::Io {
            operation: "open with macOS open",
            path: Some(PathBuf::from(OPEN)),
            message: error.to_string(),
        })?;

    if output.status.success() {
        Ok(())
    } else {
        Err(PlatformError::CommandFailed {
            program: OPEN.to_string(),
            status: output.status.code(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        })
    }
}
