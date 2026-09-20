use super::{
    LinuxPlatform, MountInfo, XdgBaseDirs, ensure_private_dir, format_trash_timestamp,
    is_local_block_mount_with_sysfs, linux_interface_kind_with_sysfs, list_trash_root,
    mount_kind_from_sysfs_path, move_one_to_trash_root, parse_mountinfo, parse_trash_timestamp,
    parse_trashinfo, percent_decode_path, percent_encode_path, private_trash_root,
    private_trash_root_with_topdir, restore_trash_item_from_root,
    restore_trash_item_from_root_with, spawn_detached_child, validate_desktop_entry,
};
use crate::{NetworkInterfaceKind, Platform, PlatformError, TrashRestoreTarget, VolumeKind};
use std::ffi::OsString;
use std::fs::{self, OpenOptions};
use std::io::Cursor;
use std::os::unix::fs::{OpenOptionsExt, symlink};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

fn test_path(label: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "tundra-linux-{label}-{}-{nonce}",
        std::process::id()
    ))
}

fn write_private(path: &Path, contents: &[u8]) {
    use std::io::Write;
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .mode(0o600)
        .open(path)
        .unwrap();
    file.write_all(contents).unwrap();
}

#[test]
fn xdg_base_dirs_ignore_relative_values() {
    let home = Path::new("/home/tundra");
    let dirs = XdgBaseDirs::resolve(
        home,
        Some(PathBuf::from("relative")),
        Some(PathBuf::from("/data")),
        None,
        None,
    );
    assert_eq!(dirs.config, home.join(".config"));
    assert_eq!(dirs.data, Path::new("/data"));
}

#[test]
fn detached_spawn_reports_missing_program_before_queuing_a_reaper() {
    let missing = PathBuf::from("/definitely/missing/tundra-open-helper");
    let mut command = Command::new(&missing);
    let error = spawn_detached_child(
        &mut command,
        "open with Linux helper",
        &OsString::from(missing.as_os_str()),
    )
    .expect_err("a missing helper must be reported synchronously");
    assert!(matches!(
        error,
        PlatformError::DetailedIo {
            operation: "open with Linux helper",
            path: Some(path),
            ..
        } if path == missing
    ));
}

#[test]
fn detached_spawn_does_not_wait_for_the_child() {
    let mut command = Command::new("/bin/sh");
    command.args(["-c", "sleep 5"]);
    let started = Instant::now();
    let mut child = spawn_detached_child(
        &mut command,
        "start nonblocking Linux helper",
        &OsString::from("/bin/sh"),
    )
    .expect("shell fixture should start");
    assert!(started.elapsed() < Duration::from_secs(1));
    assert!(child.try_wait().expect("probe child").is_none());
    let _ = child.kill();
    let _ = child.wait();
}
#[test]
fn account_user_dirs_do_not_fall_back_to_process_user_for_unknown_accounts() {
    for username in ["", "--help", "bad\nname", "tundra-missing-user-7f203f4a"] {
        assert!(
            LinuxPlatform.user_dirs_for_user(username).is_err(),
            "{username:?}"
        );
    }
}
#[test]
fn mountinfo_parser_unescapes_mount_points() {
    let mounts = parse_mountinfo(Cursor::new(
        "24 22 8:1 / /media/tundra/My\\040Disk rw - ext4 /dev/sda1 rw\n",
    ));
    assert_eq!(mounts[0].mount_point, Path::new("/media/tundra/My Disk"));
    assert_eq!(mounts[0].fs_type, "ext4");
    assert!(!mounts[0].read_only);
    let mounts = parse_mountinfo(Cursor::new("24 22 8:1 / / ro - ext4 /dev/sda1 ro\n"));
    assert!(mounts[0].read_only);
}

#[test]
fn network_kind_uses_virtual_prefixes_and_wireless_sysfs_marker() {
    let sysfs = test_path("sys-class-net");
    fs::create_dir_all(sysfs.join("wlan0/wireless")).unwrap();
    fs::create_dir_all(sysfs.join("eno1")).unwrap();
    assert_eq!(
        linux_interface_kind_with_sysfs("wlan0", &sysfs, &sysfs.join("virtual")),
        NetworkInterfaceKind::Wireless
    );
    assert_eq!(
        linux_interface_kind_with_sysfs("eno1", &sysfs, &sysfs.join("virtual")),
        NetworkInterfaceKind::Wired
    );
    assert_eq!(
        linux_interface_kind_with_sysfs("veth123", &sysfs, &sysfs.join("virtual")),
        NetworkInterfaceKind::Virtual
    );
    assert_eq!(
        linux_interface_kind_with_sysfs("mystery", &sysfs, &sysfs.join("virtual")),
        NetworkInterfaceKind::Unknown
    );
    let _ = fs::remove_dir_all(sysfs);
}

#[test]
fn network_kind_detects_virtual_devices_without_known_names() {
    let sysfs = test_path("sys-virtual-net");
    let class = sysfs.join("class");
    let virtual_net = sysfs.join("devices/virtual/net");
    fs::create_dir_all(virtual_net.join("unexpected0")).unwrap();
    fs::create_dir_all(&class).unwrap();
    symlink(virtual_net.join("unexpected0"), class.join("unexpected0")).unwrap();
    assert_eq!(
        linux_interface_kind_with_sysfs("unexpected0", &class, &virtual_net),
        NetworkInterfaceKind::Virtual
    );
    for name in ["br0", "cni0", "flannel.1"] {
        assert_eq!(
            linux_interface_kind_with_sysfs(name, &class, &virtual_net),
            NetworkInterfaceKind::Virtual
        );
    }
    let _ = fs::remove_dir_all(sysfs);
}
#[test]
fn trash_path_encoding_round_trips() {
    let path = Path::new("/tmp/space % 汉字");
    assert_eq!(
        percent_decode_path(&percent_encode_path(path)),
        Some(path.to_path_buf())
    );
}
#[test]
fn trash_timestamp_is_freedesktop_iso_and_round_trips() {
    let timestamp = format_trash_timestamp(1_700_000_000);
    assert_eq!(timestamp.len(), 19);
    assert_eq!(parse_trash_timestamp(&timestamp), Some(1_700_000_000));
    assert_eq!(parse_trash_timestamp("not-a-date"), None);
    assert_eq!(parse_trash_timestamp("2025-02-31T12:00:00"), None);
}

#[test]
fn local_mount_filter_requires_a_real_block_device_and_rejects_network_filesystems() {
    let sysfs = test_path("sys-dev-block");
    fs::create_dir_all(sysfs.join("8:1")).unwrap();
    let local = MountInfo {
        mount_point: PathBuf::from("/media/local"),
        source: PathBuf::from("/dev/tundra-test-missing"),
        fs_type: "ext4".to_string(),
        major_minor: "8:1".to_string(),
        read_only: false,
    };
    let network = MountInfo {
        fs_type: "nfs4".to_string(),
        ..local.clone()
    };
    let missing = MountInfo {
        major_minor: "8:2".to_string(),
        ..local.clone()
    };
    assert!(is_local_block_mount_with_sysfs(&local, &sysfs));
    assert!(!is_local_block_mount_with_sysfs(&network, &sysfs));
    assert!(!is_local_block_mount_with_sysfs(&missing, &sysfs));
    let _ = fs::remove_dir_all(sysfs);
}

#[test]
fn removable_marker_is_discovered_on_the_parent_block_device() {
    let sysfs = test_path("sys-removable");
    let partition = sysfs.join("devices/block/sdb/sdb1");
    fs::create_dir_all(&partition).unwrap();
    fs::write(sysfs.join("devices/block/sdb/removable"), "1\n").unwrap();
    assert_eq!(
        mount_kind_from_sysfs_path(&partition),
        VolumeKind::Removable
    );
    fs::write(sysfs.join("devices/block/sdb/removable"), "0\n").unwrap();
    assert_eq!(mount_kind_from_sysfs_path(&partition), VolumeKind::Fixed);
    let _ = fs::remove_dir_all(sysfs);
}

#[test]
fn private_directory_creation_rejects_symlinked_ancestors() {
    let fixture = test_path("private-dir-symlink");
    let real = fixture.join("real");
    fs::create_dir_all(&real).unwrap();
    let linked = fixture.join("linked");
    symlink(&real, &linked).unwrap();

    let error = ensure_private_dir(&linked.join("Trash"))
        .expect_err("a private directory must never follow an ancestor symlink");

    assert!(error.to_string().contains("without following links"));
    assert!(!real.join("Trash").exists());
    let _ = fs::remove_dir_all(fixture);
}

#[test]
fn home_trash_round_trips_files_and_directories_without_touching_real_trash() {
    let fixture = test_path("trash-roundtrip");
    let root = private_trash_root(fixture.join("Trash")).unwrap();
    let source_dir = fixture.join("source");
    fs::create_dir_all(source_dir.join("folder")).unwrap();
    let file = source_dir.join("hello.txt");
    let directory = source_dir.join("folder");
    fs::write(&file, "hello").unwrap();
    fs::write(directory.join("nested.txt"), "nested").unwrap();

    move_one_to_trash_root(&file, &root).unwrap();
    move_one_to_trash_root(&directory, &root).unwrap();
    assert!(!file.exists());
    assert!(!directory.exists());
    let entries = list_trash_root(&root).unwrap();
    assert_eq!(entries.len(), 2);

    restore_trash_item_from_root(&root, "hello.txt", TrashRestoreTarget::OriginalLocation).unwrap();
    restore_trash_item_from_root(&root, "folder", TrashRestoreTarget::OriginalLocation).unwrap();
    assert_eq!(fs::read_to_string(file).unwrap(), "hello");
    assert_eq!(
        fs::read_to_string(directory.join("nested.txt")).unwrap(),
        "nested"
    );
    assert!(list_trash_root(&root).unwrap().is_empty());
    let _ = fs::remove_dir_all(fixture);
}

#[test]
fn per_volume_trash_stores_relative_paths_and_resolves_them_safely() {
    let fixture = test_path("trash-volume");
    let topdir = fixture.join("volume");
    fs::create_dir_all(topdir.join("docs")).unwrap();
    let root =
        private_trash_root_with_topdir(fixture.join("volume-trash"), Some(topdir.clone())).unwrap();
    let source = topdir.join("docs/report.txt");
    fs::write(&source, "report").unwrap();

    move_one_to_trash_root(&source, &root).unwrap();
    let metadata = fs::read_to_string(root.info.join("report.txt.trashinfo")).unwrap();
    assert!(metadata.contains("\nPath=docs/report.txt\n"));
    let parsed = parse_trashinfo(
        &root.info.join("report.txt.trashinfo"),
        root.topdir.as_deref(),
    )
    .unwrap();
    assert_eq!(parsed.path, source);
    restore_trash_item_from_root(&root, "report.txt", TrashRestoreTarget::OriginalLocation)
        .unwrap();
    assert_eq!(fs::read_to_string(source).unwrap(), "report");
    let _ = fs::remove_dir_all(fixture);
}

#[test]
fn cross_volume_restore_uses_safe_copy_then_removes_trash_source() {
    let fixture = test_path("trash-cross-volume");
    let root = private_trash_root(fixture.join("Trash")).unwrap();
    let source = fixture.join("source.txt");
    fs::write(&source, "cross-volume").unwrap();
    move_one_to_trash_root(&source, &root).unwrap();
    let destination = fixture.join("restored/copy.txt");

    restore_trash_item_from_root_with(
        &root,
        "source.txt",
        TrashRestoreTarget::DestinationPath(destination.clone()),
        |_, _| Err(std::io::Error::from_raw_os_error(libc::EXDEV)),
    )
    .unwrap();

    assert_eq!(fs::read_to_string(destination).unwrap(), "cross-volume");
    assert!(!root.files.join("source.txt").exists());
    assert!(!root.info.join("source.txt.trashinfo").exists());
    let _ = fs::remove_dir_all(fixture);
}

#[test]
fn malformed_metadata_and_symbolic_link_entries_are_never_listed() {
    let fixture = test_path("trash-malformed");
    let root = private_trash_root(fixture.join("Trash")).unwrap();
    fs::write(root.files.join("bad"), "data").unwrap();
    write_private(
        &root.info.join("bad.trashinfo"),
        b"[Trash Info]\nPath=%ZZ\nDeletionDate=2025-01-01T00:00:00\n",
    );
    fs::write(root.files.join("linked"), "data").unwrap();
    symlink(
        root.info.join("bad.trashinfo"),
        root.info.join("linked.trashinfo"),
    )
    .unwrap();
    symlink(root.files.join("bad"), root.files.join("content-symlink")).unwrap();
    write_private(
        &root.info.join("content-symlink.trashinfo"),
        b"[Trash Info]\nPath=/tmp/content\nDeletionDate=2025-01-01T00:00:00\n",
    );

    assert!(list_trash_root(&root).unwrap().is_empty());
    let _ = fs::remove_dir_all(fixture);
}

#[test]
fn stale_metadata_and_existing_names_allocate_a_unique_trash_name() {
    let fixture = test_path("trash-conflict");
    let root = private_trash_root(fixture.join("Trash")).unwrap();
    write_private(
        &root.info.join("same.txt.trashinfo"),
        b"[Trash Info]\nPath=/tmp/stale\nDeletionDate=2025-01-01T00:00:00\n",
    );
    let source = fixture.join("same.txt");
    fs::write(&source, "new").unwrap();

    move_one_to_trash_root(&source, &root).unwrap();

    assert_eq!(
        fs::read_to_string(root.files.join("same.txt.1")).unwrap(),
        "new"
    );
    assert!(root.info.join("same.txt.1.trashinfo").is_file());
    let _ = fs::remove_dir_all(fixture);
}

#[test]
fn restore_rejects_symlinked_parent_and_preserves_trash_item() {
    let fixture = test_path("trash-restore-symlink");
    let root = private_trash_root(fixture.join("Trash")).unwrap();
    let source = fixture.join("source.txt");
    fs::write(&source, "keep").unwrap();
    move_one_to_trash_root(&source, &root).unwrap();
    let real_parent = fixture.join("real-parent");
    fs::create_dir(&real_parent).unwrap();
    let linked_parent = fixture.join("linked-parent");
    symlink(&real_parent, &linked_parent).unwrap();

    let error = restore_trash_item_from_root(
        &root,
        "source.txt",
        TrashRestoreTarget::DestinationPath(linked_parent.join("restored.txt")),
    )
    .expect_err("symlink parent must be rejected");

    assert!(error.to_string().contains("without following links"));
    assert!(root.files.join("source.txt").is_file());
    assert!(root.info.join("source.txt.trashinfo").is_file());
    assert!(fs::read_dir(real_parent).unwrap().next().is_none());
    let _ = fs::remove_dir_all(fixture);
}

#[test]
fn desktop_entry_validation_covers_type_exec_try_exec_permissions_and_links() {
    use std::os::unix::fs::PermissionsExt;

    let fixture = test_path("desktop-entry");
    fs::create_dir_all(&fixture).unwrap();
    let valid = fixture.join("valid.desktop");
    write_private(
        &valid,
        b"[Desktop Entry]\nType=Application\nName=Valid\nExec=/bin/true\nTryExec=/bin/true\n",
    );
    assert!(validate_desktop_entry(&valid).is_ok());

    let wrong_type = fixture.join("link.desktop");
    write_private(
        &wrong_type,
        b"[Desktop Entry]\nType=Link\nName=Link\nExec=/bin/true\n",
    );
    assert!(validate_desktop_entry(&wrong_type).is_err());

    let missing_exec = fixture.join("missing.desktop");
    write_private(
        &missing_exec,
        b"[Desktop Entry]\nType=Application\nName=Missing\n",
    );
    assert!(validate_desktop_entry(&missing_exec).is_err());

    let missing_try_exec = fixture.join("try.desktop");
    write_private(
        &missing_try_exec,
        b"[Desktop Entry]\nType=Application\nName=Missing\nExec=/bin/true\nTryExec=/definitely/missing\n",
    );
    assert!(validate_desktop_entry(&missing_try_exec).is_err());

    let writable = fixture.join("writable.desktop");
    write_private(
        &writable,
        b"[Desktop Entry]\nType=Application\nName=Writable\nExec=/bin/true\n",
    );
    fs::set_permissions(&writable, fs::Permissions::from_mode(0o666)).unwrap();
    assert!(validate_desktop_entry(&writable).is_err());

    let linked = fixture.join("linked.desktop");
    symlink(&valid, &linked).unwrap();
    assert!(validate_desktop_entry(&linked).is_err());
    let _ = fs::remove_dir_all(fixture);
}
