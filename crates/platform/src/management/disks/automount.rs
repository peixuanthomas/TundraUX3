use super::*;
use std::path::Component;

fn device_sources(device: &Json) -> Vec<String> {
    let mut sources = vec![
        format!("UUID={}", text(device, "uuid")),
        text(device, "name"),
    ];
    let label = text(device, "label");
    if !label.is_empty() {
        sources.push(format!("LABEL={label}"));
    }
    sources
}

pub(super) fn defaults_for_device(text: &str, device: &Json) -> Option<(String, String, bool)> {
    let sources = device_sources(device);
    text.lines()
        .map(str::trim)
        .filter(|line| !line.starts_with('#'))
        .find_map(|line| {
            let fields = line.split_whitespace().collect::<Vec<_>>();
            if fields.len() < 4 || !sources.contains(&unescape(fields[0])) {
                return None;
            }
            Some((
                unescape(fields[1]),
                fields[3].into(),
                !fields[3].split(',').any(|o| o == "noauto"),
            ))
        })
}

fn escape(value: &str) -> String {
    value
        .replace('\\', "\\134")
        .replace(' ', "\\040")
        .replace('\t', "\\011")
}

/// Produces a draft only. Saving is performed by the controlled editor and does
/// not mount the device or reload any service.
pub fn automatic_mount_draft(
    command: &ManagementCommand,
    cancelled: &AtomicBool,
) -> Result<ConfigDraft, ManagementError> {
    let target = command
        .target
        .as_deref()
        .ok_or_else(|| ManagementError::InvalidInput("Select a filesystem.".into()))?;
    let devices = inventory(cancelled)?;
    let mut mounted = mounts()?;
    associate_btrfs(&devices, &mut mounted);
    let device = devices
        .iter()
        .find(|d| d["name"].as_str() == Some(target))
        .ok_or_else(|| {
            ManagementError::Conflict("The disk disappeared. Refresh the list.".into())
        })?;
    if identity(device, &mounted)
        .iter()
        .any(|(key, value)| command.identity.get(key) != Some(value))
    {
        return Err(ManagementError::Conflict(
            "The disk or mount changed. Refresh the list.".into(),
        ));
    }
    let uuid = text(device, "uuid");
    if devices.iter().filter(|d| text(d, "uuid") == uuid).count() > 1
        && text(device, "fstype") != "btrfs"
    {
        return Err(ManagementError::Conflict("Several filesystems have this UUID. Assign a unique UUID before enabling automatic mount.".into()));
    }
    let original = fs::read_to_string("/etc/fstab").map_err(io_error)?;
    let content = build_for_sources(
        &original,
        &uuid,
        &text(device, "fstype"),
        &command.values,
        &device_sources(device),
    )?;
    check_cancelled(cancelled)?;
    Ok(ConfigDraft {
        expected_content: Some(original),
        path: "/etc/fstab".into(),
        content,
        validator: "fstab".into(),
        service: None,
        scope: "system".into(),
    })
}

#[cfg(test)]
fn build(
    original: &str,
    uuid: &str,
    fs_type: &str,
    values: &BTreeMap<String, String>,
) -> Result<String, ManagementError> {
    build_for_sources(original, uuid, fs_type, values, &[])
}

fn build_for_sources(
    original: &str,
    uuid: &str,
    fs_type: &str,
    values: &BTreeMap<String, String>,
    aliases: &[String],
) -> Result<String, ManagementError> {
    if uuid.is_empty()
        || !uuid.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-')
        || !allowed_filesystem(fs_type)
    {
        return Err(ManagementError::InvalidInput(
            "This filesystem has no usable stable UUID.".into(),
        ));
    }
    let path = values
        .get("mountpoint")
        .filter(|s| !s.is_empty() && !s.contains(['\n', '\r', '\0']))
        .ok_or_else(|| ManagementError::InvalidInput("Enter an absolute mountpoint.".into()))?;
    let mountpoint = Path::new(path);
    if !mountpoint.is_absolute()
        || mountpoint
            .components()
            .any(|c| !matches!(c, Component::RootDir | Component::Normal(_)))
        || protected_mount(mountpoint)
    {
        return Err(ManagementError::InvalidInput("Choose an absolute data mountpoint without parent components. Edit system mounts in the configuration editor.".into()));
    }
    let enabled = match values.get("enabled").map(String::as_str) {
        Some("true") => true,
        Some("false") => false,
        _ => {
            return Err(ManagementError::InvalidInput(
                "Choose whether to mount at startup.".into(),
            ));
        }
    };
    let options = values
        .get("options")
        .map(String::as_str)
        .unwrap_or("defaults,nofail");
    if options.is_empty()
        || options
            .bytes()
            .any(|b| b.is_ascii_whitespace() || b == 0 || b == b'#' || b == b'\\')
        || options.split(',').any(str::is_empty)
    {
        return Err(ManagementError::InvalidInput(
            "Enter comma-separated mount options without spaces.".into(),
        ));
    }
    // The disk UI controls automatic startup only; it keeps the remaining options.
    let mut options = options
        .split(',')
        .filter(|o| !matches!(*o, "auto" | "noauto"))
        .collect::<Vec<_>>();
    if !enabled {
        options.push("noauto");
    }
    let options = if options.is_empty() {
        "defaults".into()
    } else {
        options.join(",")
    };
    let source = format!("UUID={uuid}");
    let mut replaced = 0;
    let mut output = String::new();
    for raw in original.split_inclusive('\n') {
        let line = raw.trim_start();
        let fields = line.split_whitespace().collect::<Vec<_>>();
        if !line.starts_with('#') && fields.len() >= 4 {
            if fields[0] == source || aliases.contains(&unescape(fields[0])) {
                replaced += 1;
                if replaced > 1 {
                    return Err(ManagementError::Conflict("This UUID has several fstab entries. Edit them in the configuration editor.".into()));
                }
                let dump = fields
                    .get(4)
                    .copied()
                    .filter(|s| s.parse::<u32>().is_ok())
                    .unwrap_or("0");
                let pass = fields
                    .get(5)
                    .copied()
                    .filter(|s| s.parse::<u32>().is_ok())
                    .unwrap_or("0");
                let comment = raw
                    .find('#')
                    .map(|index| format!(" {}", raw[index..].trim_end()))
                    .unwrap_or_default();
                output.push_str(&format!(
                    "{source}\t{}\t{fs_type}\t{options}\t{dump}\t{pass}{comment}\n",
                    escape(path)
                ));
                continue;
            }
            if unescape(fields[1]) == *path {
                return Err(ManagementError::Conflict("Another fstab entry uses this mountpoint. Choose another directory or edit the existing entry.".into()));
            }
        }
        output.push_str(raw);
    }
    if replaced == 0 {
        if !output.is_empty() && !output.ends_with('\n') {
            output.push('\n');
        }
        let pass = if matches!(fs_type, "ext2" | "ext3" | "ext4") {
            "2"
        } else {
            "0"
        };
        output.push_str(&format!(
            "{source}\t{}\t{fs_type}\t{options}\t0\t{pass}\n",
            escape(path)
        ));
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn values(path: &str, enabled: bool) -> BTreeMap<String, String> {
        BTreeMap::from([
            ("mountpoint".into(), path.into()),
            ("enabled".into(), enabled.to_string()),
            ("options".into(), "defaults,nofail".into()),
        ])
    }
    #[test]
    fn drafts_preserve_unrelated_entries_comments_and_stable_device_id() {
        let original = "# existing\nUUID=other / ext4 defaults 0 1\nUUID=1234 /mnt/old ext4 defaults,noauto 0 2 # data\n";
        let draft = build(original, "1234", "ext4", &values("/mnt/data disk", true)).unwrap();
        assert!(draft.starts_with("# existing\nUUID=other / ext4 defaults 0 1\n"));
        assert!(
            draft.contains("UUID=1234\t/mnt/data\\040disk\text4\tdefaults,nofail\t0\t2 # data")
        );
        assert!(!draft.contains("noauto"));
        assert!(
            build(&draft, "1234", "ext4", &values("/mnt/data disk", false))
                .unwrap()
                .contains("defaults,nofail,noauto")
        );
    }
    #[test]
    fn drafts_reject_duplicate_mountpoints_and_system_paths() {
        assert!(
            build(
                "UUID=other /mnt/data ext4 defaults 0 2\n",
                "1234",
                "ext4",
                &values("/mnt/data", true)
            )
            .is_err()
        );
        for path in ["/", "/etc", "/mnt/../etc", "relative", "/mnt/a\nentry"] {
            assert!(build("", "1234", "ext4", &values(path, true)).is_err());
        }
        assert!(build("", "invalid UUID", "ext4", &values("/mnt/data", true)).is_err());
    }
    #[test]
    fn existing_device_path_is_replaced_by_uuid_without_duplicate_entry() {
        let original = "/dev/test1 /mnt/data ext4 defaults 0 2\n";
        let draft = build_for_sources(
            original,
            "1234",
            "ext4",
            &values("/mnt/data", true),
            &["/dev/test1".into()],
        )
        .unwrap();
        assert_eq!(draft.lines().count(), 1);
        assert!(draft.starts_with("UUID=1234\t"));
    }
}
