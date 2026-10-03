//! Read-only block inventory, fixed mount operations and scans under the actor's UID.
mod scan;

use super::network::{check_cancelled, io_error, program, run};
use super::*;
use serde_json::Value as Json;
use std::collections::{HashMap, HashSet};
use std::ffi::CString;
use std::fs;
use std::os::unix::fs::{DirBuilderExt, FileTypeExt, MetadataExt};
use std::path::Path;
use zbus::blocking::{Connection, Proxy, connection::Builder};
use zbus::zvariant::{OwnedObjectPath, Value};

const UDISKS: &str = "org.freedesktop.UDisks2";

#[derive(Debug, Clone, PartialEq, Eq)]
struct Mount {
    id: String,
    device: String,
    path: PathBuf,
    fs_type: String,
    source: String,
}

fn mounts() -> Result<Vec<Mount>, ManagementError> {
    Ok(parse_mounts(
        &fs::read_to_string("/proc/self/mountinfo").map_err(io_error)?,
    ))
}

fn parse_mounts(text: &str) -> Vec<Mount> {
    text.lines()
        .filter_map(|line| {
            let (before, after) = line.split_once(" - ")?;
            let before = before.split_whitespace().collect::<Vec<_>>();
            let after = after.split_whitespace().collect::<Vec<_>>();
            if before.len() < 6 || after.len() < 2 {
                return None;
            }
            Some(Mount {
                id: before[0].into(),
                device: before[2].into(),
                path: PathBuf::from(unescape(before[4])),
                fs_type: after[0].into(),
                source: unescape(after[1]),
            })
        })
        .collect()
}

fn associate_btrfs(devices: &[Json], mounted: &mut Vec<Mount>) {
    let originals = mounted
        .iter()
        .filter(|m| m.fs_type == "btrfs")
        .cloned()
        .collect::<Vec<_>>();
    for mount in originals {
        let source_number = fs::metadata(&mount.source)
            .ok()
            .filter(|m| m.file_type().is_block_device())
            .map(|m| format!("{}:{}", libc::major(m.rdev()), libc::minor(m.rdev())));
        let source = devices.iter().find(|d| {
            d["name"].as_str() == Some(&mount.source)
                || source_number
                    .as_ref()
                    .is_some_and(|n| d["maj:min"].as_str() == Some(n.as_str()))
        });
        if let Some(source) = source {
            let uuid = text(source, "uuid");
            for device in devices.iter().filter(|d| {
                text(d, "fstype") == "btrfs"
                    && (text(d, "maj:min") == text(source, "maj:min")
                        || !uuid.is_empty() && text(d, "uuid") == uuid)
            }) {
                let mut alias = mount.clone();
                alias.device = text(device, "maj:min");
                if !mounted
                    .iter()
                    .any(|m| m.id == alias.id && m.device == alias.device)
                {
                    mounted.push(alias);
                }
            }
        }
    }
}

fn unescape(text: &str) -> String {
    let mut output = Vec::new();
    let bytes = text.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'\\' && index + 3 < bytes.len() {
            if let Ok(value) = u8::from_str_radix(
                std::str::from_utf8(&bytes[index + 1..index + 4]).unwrap_or(""),
                8,
            ) {
                output.push(value);
                index += 4;
                continue;
            }
        }
        output.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&output).into_owned()
}

fn inventory(cancelled: &AtomicBool) -> Result<Vec<Json>, ManagementError> {
    let json: Json = serde_json::from_str(&run(
        "lsblk",
        &[
            "--json",
            "--bytes",
            "--paths",
            "--output",
            "NAME,KNAME,PKNAME,TYPE,SIZE,FSTYPE,UUID,MAJ:MIN,MODEL,RO,RM",
        ],
        cancelled,
    )?)
    .map_err(|e| ManagementError::Failed(format!("Invalid lsblk output: {e}")))?;
    fn flatten(values: &[Json], output: &mut Vec<Json>, seen: &mut HashSet<String>) {
        for value in values {
            if let Some(name) = value["name"].as_str() {
                if seen.insert(name.into()) {
                    output.push(value.clone());
                }
            }
            if let Some(children) = value["children"].as_array() {
                flatten(children, output, seen);
            }
        }
    }
    let mut output = Vec::new();
    flatten(
        json["blockdevices"]
            .as_array()
            .ok_or_else(|| ManagementError::Failed("lsblk did not return devices".into()))?,
        &mut output,
        &mut HashSet::new(),
    );
    Ok(output)
}

fn text(value: &Json, key: &str) -> String {
    value[key].as_str().unwrap_or("").into()
}
fn number(value: &Json, key: &str) -> String {
    value[key]
        .as_u64()
        .map(|n| n.to_string())
        .unwrap_or_else(|| text(value, key))
}
fn boolean(value: &Json, key: &str) -> bool {
    value[key]
        .as_bool()
        .unwrap_or_else(|| value[key].as_u64().unwrap_or(0) != 0)
}

fn identity(device: &Json, mounts: &[Mount]) -> BTreeMap<String, String> {
    BTreeMap::from([
        ("device".into(), text(device, "maj:min")),
        ("uuid".into(), text(device, "uuid")),
        ("size".into(), number(device, "size")),
        (
            "mounts".into(),
            mounts
                .iter()
                .filter(|m| m.device == text(device, "maj:min"))
                .map(|m| format!("{}:{}", m.id, m.path.display()))
                .collect::<Vec<_>>()
                .join("\n"),
        ),
    ])
}

fn protected_mount(path: &Path) -> bool {
    matches!(
        path.to_str(),
        Some(
            "/" | "/boot"
                | "/boot/efi"
                | "/usr"
                | "/etc"
                | "/var"
                | "/run"
                | "/dev"
                | "/proc"
                | "/sys"
                | "/home"
        )
    )
}

fn allowed_filesystem(fs: &str) -> bool {
    matches!(
        fs,
        "ext2"
            | "ext3"
            | "ext4"
            | "btrfs"
            | "xfs"
            | "vfat"
            | "exfat"
            | "ntfs"
            | "ntfs3"
            | "f2fs"
            | "iso9660"
            | "udf"
    )
}

fn mount_field(id: &str, label: &str, value: &str, choices: Vec<String>) -> ManagementField {
    ManagementField {
        id: id.into(),
        label: label.into(),
        value: value.into(),
        required: true,
        choices,
        ..Default::default()
    }
}

pub fn query(
    query: &ManagementQuery,
    cancelled: &AtomicBool,
) -> Result<ManagementSnapshot, ManagementError> {
    let devices = inventory(cancelled)?;
    let mut mounted = mounts()?;
    associate_btrfs(&devices, &mut mounted);
    let udisks = udisks_available();
    let mut snapshot = ManagementSnapshot {
        columns: vec![
            "Device".into(),
            "Type".into(),
            "Filesystem".into(),
            "Bytes".into(),
            "Mounted at".into(),
            "Available bytes".into(),
            "Available inodes".into(),
        ],
        backend: if udisks {
            "UDisks2".into()
        } else {
            "Linux inventory; fixed mount/umount fallback".into()
        },
        ..Default::default()
    };
    snapshot.actions.push(ManagementAction {
        id: "scan".into(),
        label: "Scan directory usage and large files".into(),
        fields: vec![mount_field(
            "directory",
            "Absolute directory",
            "/",
            Vec::new(),
        )],
        ..Default::default()
    });
    for device in devices {
        check_cancelled(cancelled)?;
        let name = text(&device, "name");
        let fs_type = text(&device, "fstype");
        let id = text(&device, "maj:min");
        if !query.filter.is_empty()
            && !name.contains(&query.filter)
            && !fs_type.contains(&query.filter)
        {
            continue;
        }
        let device_mounts = mounted
            .iter()
            .filter(|m| m.device == id)
            .collect::<Vec<_>>();
        let mountpoints = device_mounts
            .iter()
            .map(|m| m.path.display().to_string())
            .collect::<Vec<_>>();
        let mut row = ManagementRow {
            id: name.clone(),
            cells: vec![
                name.clone(),
                text(&device, "type"),
                fs_type.clone(),
                number(&device, "size"),
                mountpoints.join(", "),
                String::new(),
                String::new(),
            ],
            identity: identity(&device, &mounted),
            detail: vec![
                ("Model".into(), text(&device, "model")),
                ("Parent".into(), text(&device, "pkname")),
                ("Filesystem UUID".into(), text(&device, "uuid")),
                (
                    "Read-only device".into(),
                    boolean(&device, "ro").to_string(),
                ),
                ("Removable".into(), boolean(&device, "rm").to_string()),
            ],
            ..Default::default()
        };
        for mount in &device_mounts {
            match space(&mount.path) {
                Ok(s) => {
                    row.detail.push((
                        format!("{} capacity", mount.path.display()),
                        format!(
                            "total={} available={} inodes={} available-inodes={}",
                            s.0, s.1, s.2, s.3
                        ),
                    ));
                    if row.cells[5].is_empty() {
                        row.cells[5] = s.1.to_string();
                        row.cells[6] = s.3.to_string();
                    }
                }
                Err(e) => row.detail.push((
                    format!("{} capacity", mount.path.display()),
                    format!("Unavailable: {e}"),
                )),
            }
        }
        if let Some(mountpoint) = mountpoints.first() {
            row.identity.insert("directory".into(), mountpoint.clone());
            row.actions.push(ManagementAction {
                id: "open_directory".into(),
                label: "Open mountpoint in Explorer".into(),
                fields: vec![mount_field(
                    "directory",
                    "Directory",
                    mountpoint,
                    mountpoints.clone(),
                )],
                ..Default::default()
            });
        }
        if allowed_filesystem(&fs_type) {
            if device_mounts.is_empty() {
                row.actions.push(ManagementAction {
                    id: "mount".into(),
                    label: "Mount filesystem".into(),
                    privileged: true,
                    confirm: true,
                    fields: vec![mount_field(
                        "mode",
                        "Mount mode",
                        "read_only",
                        vec!["read_only".into(), "read_write".into()],
                    )],
                    disabled_reason: (!udisks && program("mount").is_err())
                        .then(|| "Neither UDisks2 nor mount is available".into()),
                });
            } else {
                let reason = if device_mounts.iter().any(|m| protected_mount(&m.path)) {
                    Some("This filesystem has a system mount that Tundra will not unmount".into())
                } else if device_mounts.len() != 1 {
                    Some("Several mountpoints refer to this filesystem; unmount them with the owning system tool".into())
                } else {
                    None
                };
                row.actions.push(ManagementAction {
                    id: "unmount".into(),
                    label: "Unmount filesystem".into(),
                    privileged: true,
                    confirm: true,
                    disabled_reason: reason,
                    ..Default::default()
                });
                row.actions.push(ManagementAction {
                    id: "scan".into(),
                    label: "Scan directory usage and large files".into(),
                    fields: vec![mount_field(
                        "directory",
                        "Absolute directory",
                        &mountpoints[0],
                        Vec::new(),
                    )],
                    ..Default::default()
                });
            }
        } else if !fs_type.is_empty() {
            row.detail.push(("Mount availability".into(), "Unsupported filesystem or encrypted/swap/member device; formatting and unlocking are outside this app".into()));
        }
        snapshot.rows.push(row);
    }
    snapshot.notices.push("Mounting uses UDisks2 when available. The fallback chooses its own directory under /run and accepts no custom mount flags. No formatting, partition changes, or fstab edits are offered.".into());
    snapshot.notices.push("Directory scans keep the original user's permissions, skip symbolic links and other mounts, and count each hard-linked file once. Unreadable entries are reported.".into());
    Ok(snapshot)
}

fn space(path: &Path) -> Result<(u64, u64, u64, u64), ManagementError> {
    use std::os::unix::ffi::OsStrExt;
    let path = CString::new(path.as_os_str().as_bytes())
        .map_err(|_| ManagementError::InvalidInput("Path contains NUL".into()))?;
    let mut stats = std::mem::MaybeUninit::<libc::statvfs>::zeroed();
    if unsafe { libc::statvfs(path.as_ptr(), stats.as_mut_ptr()) } != 0 {
        return Err(io_error(std::io::Error::last_os_error()));
    }
    let stats = unsafe { stats.assume_init() };
    let block = stats.f_frsize.max(1);
    Ok((
        stats.f_blocks.saturating_mul(block),
        stats.f_bavail.saturating_mul(block),
        stats.f_files,
        stats.f_favail,
    ))
}

pub fn execute(
    command: &ManagementCommand,
    context: &ExecutionContext,
    interaction: &mut dyn OperationInteraction,
    cancelled: &AtomicBool,
) -> Result<String, ManagementError> {
    if command.action == "scan" {
        if unsafe { libc::geteuid() } != context.actor_uid {
            return Err(ManagementError::PermissionDenied("Scans must run under the original user's UID; this helper has not dropped its privileges".into()));
        }
        let directory = command
            .values
            .get("directory")
            .ok_or_else(|| ManagementError::InvalidInput("Choose an absolute directory".into()))?;
        let snapshot = scan::scan(Path::new(directory), interaction, cancelled)?;
        let summary = snapshot.notices.join("\n");
        interaction.emit(OperationEvent::Snapshot { snapshot });
        return Ok(summary);
    }
    if !matches!(command.action.as_str(), "mount" | "unmount") {
        return Err(ManagementError::InvalidInput("Unknown disk action".into()));
    }
    if unsafe { libc::geteuid() } != 0 {
        return Err(ManagementError::PermissionDenied(
            "Mount operations require the authorized helper".into(),
        ));
    }
    let target = command
        .target
        .as_deref()
        .ok_or_else(|| ManagementError::InvalidInput("Select a block device".into()))?;
    let devices = inventory(cancelled)?;
    let mut mounted = mounts()?;
    associate_btrfs(&devices, &mut mounted);
    let device = devices
        .iter()
        .find(|d| d["name"].as_str() == Some(target))
        .ok_or_else(|| {
            ManagementError::Conflict("Block device disappeared; refresh the list".into())
        })?;
    let current_identity = identity(device, &mounted);
    if current_identity
        .iter()
        .any(|(key, value)| command.identity.get(key) != Some(value))
    {
        return Err(ManagementError::Conflict(
            "Block device or its mounts changed; refresh the list".into(),
        ));
    }
    let canonical = fs::canonicalize(target).map_err(io_error)?;
    if !canonical.starts_with("/dev") {
        return Err(ManagementError::InvalidInput(
            "A disk operation requires a device under /dev".into(),
        ));
    }
    let metadata = fs::metadata(&canonical).map_err(io_error)?;
    if !metadata.file_type().is_block_device() {
        return Err(ManagementError::InvalidInput(
            "The selected path is not a block device".into(),
        ));
    }
    let device_number = format!(
        "{}:{}",
        libc::major(metadata.rdev()),
        libc::minor(metadata.rdev())
    );
    if command.identity.get("device") != Some(&device_number) {
        return Err(ManagementError::Conflict(
            "Device node changed since it was displayed".into(),
        ));
    }
    let fs_type = text(device, "fstype");
    if !allowed_filesystem(&fs_type) {
        return Err(ManagementError::Unavailable(
            "This device has no supported mountable filesystem".into(),
        ));
    }
    let device_mounts = mounted
        .iter()
        .filter(|m| m.device == device_number)
        .collect::<Vec<_>>();
    if command.action == "mount" && !device_mounts.is_empty() {
        return Err(ManagementError::Conflict(
            "The filesystem is already mounted".into(),
        ));
    }
    if command.action == "unmount"
        && (device_mounts.len() != 1 || device_mounts.iter().any(|m| protected_mount(&m.path)))
    {
        return Err(ManagementError::Unavailable(
            "Unmount requires one non-system mountpoint".into(),
        ));
    }
    let readonly = match command.values.get("mode").map(String::as_str) {
        Some("read_only") | None => true,
        Some("read_write") if !boolean(device, "ro") => false,
        _ => {
            return Err(ManagementError::InvalidInput(
                "Unsupported mount mode or read-only device".into(),
            ));
        }
    };
    check_cancelled(cancelled)?;
    interaction.emit(OperationEvent::Progress {
        message: format!("{} {target}", command.action),
        percent: None,
    });
    let mount_path = if udisks_available() {
        udisks_operation(
            &canonical,
            command.action == "mount",
            readonly,
            context.actor_uid,
        )?
    } else if command.action == "mount" {
        let mountpoint = fallback_directory(context.actor_uid, &device_number)?;
        let mode = if readonly {
            "nodev,nosuid,ro"
        } else {
            "nodev,nosuid,rw"
        };
        run(
            "mount",
            &[
                "--internal-only",
                "--no-canonicalize",
                "--types",
                &fs_type,
                "--options",
                mode,
                "--",
                canonical.to_str().ok_or_else(|| {
                    ManagementError::InvalidInput("Device path is not UTF-8".into())
                })?,
                mountpoint.to_str().unwrap(),
            ],
            cancelled,
        )?;
        mountpoint
    } else {
        run(
            "umount",
            &[
                "--",
                device_mounts[0].path.to_str().ok_or_else(|| {
                    ManagementError::InvalidInput("Mountpoint is not UTF-8".into())
                })?,
            ],
            cancelled,
        )?;
        device_mounts[0].path.clone()
    };
    let mut readback = mounts()?;
    associate_btrfs(&devices, &mut readback);
    if command.action == "mount" {
        if !readback
            .iter()
            .any(|m| m.device == device_number && m.path == mount_path)
        {
            return Err(ManagementError::Failed(
                "Mount command returned but the expected filesystem was not present in mountinfo"
                    .into(),
            ));
        }
        Ok(format!("Mounted {target} at {}", mount_path.display()))
    } else {
        if readback.iter().any(|m| m.id == device_mounts[0].id) {
            return Err(ManagementError::Failed(
                "Unmount command returned but the mount is still present".into(),
            ));
        }
        Ok(format!("Unmounted {target} from {}", mount_path.display()))
    }
}

fn disk_bus() -> Result<Connection, ManagementError> {
    Builder::system()
        .map_err(udisks_error)?
        .method_timeout(std::time::Duration::from_secs(60))
        .build()
        .map_err(udisks_error)
}
fn udisks_error(e: zbus::Error) -> ManagementError {
    ManagementError::Failed(format!("UDisks2: {e}"))
}

fn udisks_available() -> bool {
    let Ok(bus) = disk_bus() else { return false };
    let Ok(proxy) = zbus::blocking::fdo::DBusProxy::new(&bus) else {
        return false;
    };
    proxy
        .name_has_owner(UDISKS.try_into().unwrap())
        .unwrap_or(false)
        || proxy
            .list_activatable_names()
            .is_ok_and(|names| names.iter().any(|n| n.as_str() == UDISKS))
}

fn actor_name(uid: u32) -> Result<String, ManagementError> {
    let mut entry = std::mem::MaybeUninit::<libc::passwd>::zeroed();
    let mut result = std::ptr::null_mut();
    let mut buffer = vec![0_u8; 64 * 1024];
    let status = unsafe {
        libc::getpwuid_r(
            uid,
            entry.as_mut_ptr(),
            buffer.as_mut_ptr().cast(),
            buffer.len(),
            &mut result,
        )
    };
    if status != 0 || result.is_null() {
        return Err(ManagementError::InvalidInput(
            "The original user account no longer exists".into(),
        ));
    }
    let entry = unsafe { entry.assume_init() };
    Ok(unsafe { std::ffi::CStr::from_ptr(entry.pw_name) }
        .to_string_lossy()
        .into_owned())
}

fn udisks_operation(
    device: &Path,
    mount: bool,
    readonly: bool,
    actor_uid: u32,
) -> Result<PathBuf, ManagementError> {
    let bus = disk_bus()?;
    let manager = Proxy::new(
        &bus,
        UDISKS,
        "/org/freedesktop/UDisks2/Manager",
        "org.freedesktop.UDisks2.Manager",
    )
    .map_err(udisks_error)?;
    let empty = HashMap::<&str, Value<'_>>::new();
    let specification = HashMap::from([(
        "path",
        Value::from(
            device
                .to_str()
                .ok_or_else(|| ManagementError::InvalidInput("Invalid device path".into()))?,
        ),
    )]);
    let paths: Vec<OwnedObjectPath> = manager
        .call("ResolveDevice", &(specification, &empty))
        .map_err(udisks_error)?;
    if paths.len() != 1 {
        return Err(ManagementError::Conflict(
            "UDisks2 did not resolve exactly one filesystem".into(),
        ));
    }
    let filesystem = Proxy::new(
        &bus,
        UDISKS,
        paths[0].as_str(),
        "org.freedesktop.UDisks2.Filesystem",
    )
    .map_err(udisks_error)?;
    if mount {
        let name = actor_name(actor_uid)?;
        let options = HashMap::from([
            ("as-user", Value::from(name.as_str())),
            (
                "options",
                Value::from(if readonly {
                    "nodev,nosuid,ro"
                } else {
                    "nodev,nosuid,rw"
                }),
            ),
        ]);
        let path: String = filesystem
            .call("Mount", &(options,))
            .map_err(udisks_error)?;
        Ok(PathBuf::from(path))
    } else {
        let mountpoints: Vec<Vec<u8>> = filesystem
            .get_property("MountPoints")
            .map_err(udisks_error)?;
        let path = mountpoints
            .first()
            .map(|v| String::from_utf8_lossy(v.strip_suffix(&[0]).unwrap_or(v)).to_string())
            .unwrap_or_default();
        filesystem
            .call::<_, _, ()>("Unmount", &(empty,))
            .map_err(udisks_error)?;
        Ok(PathBuf::from(path))
    }
}

fn fallback_directory(uid: u32, device: &str) -> Result<PathBuf, ManagementError> {
    let root = PathBuf::from("/run/tundraux3-mounts");
    let user = root.join(uid.to_string());
    let target = user.join(device.replace(':', "-"));
    for path in [&root, &user, &target] {
        if !path.exists() {
            fs::DirBuilder::new()
                .mode(0o755)
                .create(path)
                .map_err(io_error)?;
        }
        let m = fs::symlink_metadata(path).map_err(io_error)?;
        if !m.is_dir() || m.file_type().is_symlink() || m.uid() != 0 || m.mode() & 0o022 != 0 {
            return Err(ManagementError::PermissionDenied(
                "Fallback mount directory is not trusted".into(),
            ));
        }
    }
    if fs::read_dir(&target).map_err(io_error)?.next().is_some() {
        return Err(ManagementError::Conflict(
            "Fallback mount directory is not empty".into(),
        ));
    }
    Ok(target)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn mountinfo_preserves_mount_identity_and_escaped_paths() {
        let parsed = parse_mounts(
            "1 0 8:1 / / rw - ext4 /dev/sda1 rw\n2 1 8:1 /sub /data\\040disk rw - ext4 /dev/sda1 rw\ntruncated\n",
        );
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[1].path, Path::new("/data disk"));
        assert_ne!(parsed[0].id, parsed[1].id);
    }
    #[test]
    fn encrypted_and_swap_devices_cannot_be_mounted() {
        for fs in ["", "crypto_LUKS", "swap", "LVM2_member", "../../evil"] {
            assert!(!allowed_filesystem(fs));
        }
        assert!(allowed_filesystem("ext4"));
        assert!(protected_mount(Path::new("/")));
        assert!(!protected_mount(Path::new("/mnt/data")));
    }
    #[test]
    fn btrfs_anonymous_devices_are_associated_with_all_members() {
        let devices = vec![
            serde_json::json!({"name":"/dev/test-a", "maj:min":"8:1", "fstype":"btrfs", "uuid":"same"}),
            serde_json::json!({"name":"/dev/test-b", "maj:min":"8:2", "fstype":"btrfs", "uuid":"same"}),
        ];
        let mut mounted = parse_mounts("1 0 0:44 / / rw - btrfs /dev/test-a rw\n");
        associate_btrfs(&devices, &mut mounted);
        for number in ["8:1", "8:2"] {
            assert!(
                mounted
                    .iter()
                    .any(|m| m.device == number && m.path == Path::new("/"))
            );
        }
    }
}
