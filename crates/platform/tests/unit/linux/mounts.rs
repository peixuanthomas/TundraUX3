use super::*;
use std::io::Cursor;

#[test]
fn btrfs_mounts_resolve_backing_disks_without_collapsing_subvolumes() {
    let fixture = std::env::temp_dir().join(format!(
        "tundra-btrfs-disks-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(fixture.join("8:3")).unwrap();
    fs::create_dir_all(fixture.join("8:17")).unwrap();
    fs::write(fixture.join("8:17/removable"), "1\n").unwrap();
    let mounts = parse_mountinfo(Cursor::new(
        "41 1 0:33 /root / rw - btrfs /dev/sda3 rw\n\
         71 41 0:33 /home /home rw - btrfs /dev/sda3 rw\n\
         54 41 0:42 / /ssd rw - btrfs /dev/sdb1 rw\n\
         132 71 0:42 /@workloads /home/user/SSD\\040Workloads rw - btrfs /dev/sdb1 rw\n",
    ));
    let disks = mounts
        .iter()
        .map(|mount| {
            let disk = block_mount_device(mount, &fixture, |source| match source.to_str() {
                Some("/dev/sda3") => Some("8:3".into()),
                Some("/dev/sdb1") => Some("8:17".into()),
                _ => None,
            })
            .expect("Btrfs's anonymous mount device must resolve to a real backing disk");
            (mount.mount_point.clone(), disk)
        })
        .collect::<Vec<_>>();
    assert_eq!(
        disks
            .iter()
            .map(|(root, _)| root.as_path())
            .collect::<Vec<_>>(),
        [
            Path::new("/"),
            Path::new("/home"),
            Path::new("/ssd"),
            Path::new("/home/user/SSD Workloads")
        ]
    );
    assert_eq!(disks[0].1, fixture.join("8:3"));
    assert_eq!(disks[2].1, fixture.join("8:17"));
    assert_eq!(
        mount_kind_from_sysfs_path(&disks[2].1),
        VolumeKind::Removable
    );
    for fs_type in ["tmpfs", "overlay", "nfs4", "fuse.sshfs", "squashfs"] {
        let mount = MountInfo {
            fs_type: fs_type.into(),
            ..mounts[0].clone()
        };
        assert!(block_mount_device(&mount, &fixture, |_| Some("8:3".into())).is_none());
    }
    assert!(block_mount_device(&mounts[0], &fixture, |_| None).is_none());
    fs::remove_dir_all(fixture).unwrap();
}

#[test]
fn mountinfo_rejects_truncated_records_and_unescapes_device_sources() {
    let mounts = parse_mountinfo(Cursor::new(
        "1 0 8:1 / / - ext4 /dev/sda1 rw\n\
         1 0 8:1 / / rw - ext4\n\
         1 0 8:1 / /media/disk ro - ext4 /dev/disk/by-label/My\\040Disk ro\n",
    ));
    assert_eq!(mounts.len(), 1);
    assert_eq!(mounts[0].source, Path::new("/dev/disk/by-label/My Disk"));
    assert!(mounts[0].read_only);
    assert!(
        source_block_device_id(Path::new("/dev/null")).is_none(),
        "a character device is not a physical disk"
    );
    assert!(source_block_device_id(Path::new("/etc/passwd")).is_none());
}
