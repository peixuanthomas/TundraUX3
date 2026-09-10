use super::*;

// Run explicitly as root (or in a user namespace with mapped UIDs 0, 1000,
// and 1001). The child has a private HOME and never empties the system Trash.
#[test]
#[ignore = "requires root with mapped UIDs 1000 and 1001; uses isolated Trash fixtures"]
fn root_trash_handles_other_users_and_preserves_private_metadata() {
    assert_eq!(
        unsafe { libc::geteuid() },
        0,
        "run in a mapped root namespace"
    );
    if std::env::var_os("TUNDRA_ROOT_TRASH_CHILD").is_none() {
        let home = std::env::temp_dir().join(format!(
            "tundra-root-trash-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&home).unwrap();
        let result = Command::new(std::env::current_exe().unwrap())
            .args([
                "--ignored",
                "--exact",
                "linux::root_trash_tests::root_trash_handles_other_users_and_preserves_private_metadata",
                "--nocapture",
            ])
            .env("TUNDRA_ROOT_TRASH_CHILD", "1")
            .env("HOME", &home)
            .env("XDG_DATA_HOME", home.join("data"))
            .status();
        fs::remove_dir_all(&home).unwrap();
        assert!(
            result.unwrap().success(),
            "root Trash regression child failed"
        );
        return;
    }

    let home = PathBuf::from(std::env::var_os("HOME").unwrap());
    let root = home_trash_root().unwrap();
    let file = home.join("用户文件.txt");
    fs::write(&file, b"ordinary user's content").unwrap();
    set_owner(&file, 1000);
    let directory = home.join("用户目录");
    fs::create_dir(&directory).unwrap();
    let nested = directory.join("nested.txt");
    fs::write(&nested, b"another user's content").unwrap();
    set_owner(&nested, 1001);
    set_owner(&directory, 1000);

    for source in [&file, &directory] {
        move_one_to_trash(source).expect("root can trash another user's item");
        assert!(!source.exists());
    }
    let entries = LinuxPlatform.list_trash().unwrap();
    for source in [&file, &directory] {
        let entry = entries
            .iter()
            .find(|entry| entry.original_path.as_ref() == Some(source))
            .unwrap();
        LinuxPlatform
            .restore_trash_item(&entry.id, TrashRestoreTarget::OriginalLocation)
            .unwrap();
        assert_eq!(fs::symlink_metadata(source).unwrap().uid(), 1000);
    }
    assert_eq!(fs::read(&file).unwrap(), b"ordinary user's content");
    assert_eq!(fs::symlink_metadata(&nested).unwrap().uid(), 1001);

    // Force the cross-device restore branch, including a mixed-owner directory.
    for source in [&file, &directory] {
        move_one_to_trash(source).unwrap();
        let name = source.file_name().unwrap().to_str().unwrap();
        restore_trash_item_from_root_with(
            &root,
            name,
            TrashRestoreTarget::OriginalLocation,
            |_, _| Err(io::Error::from_raw_os_error(libc::EXDEV)),
        )
        .unwrap();
        assert_eq!(fs::symlink_metadata(source).unwrap().uid(), 1000);
    }
    assert_eq!(fs::symlink_metadata(&nested).unwrap().uid(), 1001);
    assert_eq!(fs::read(&nested).unwrap(), b"another user's content");

    for source in [&file, &directory] {
        move_one_to_trash(source).unwrap();
    }
    let info = root.info.join("用户文件.txt.trashinfo");
    assert_eq!(fs::metadata(&info).unwrap().uid(), 0);
    assert_eq!(fs::metadata(&root.files).unwrap().uid(), 0);
    assert_eq!(fs::metadata(&root.files).unwrap().mode() & 0o777, 0o700);

    // Accepting foreign-owned contents must not accept foreign-owned indexes.
    set_owner(&info, 1000);
    assert!(parse_trashinfo(&info, None).is_err());
    assert!(
        restore_trash_item_from_root(&root, "用户文件.txt", TrashRestoreTarget::OriginalLocation)
            .is_err()
    );
    empty_trash_root(&root).unwrap();
    assert!(root.files.join("用户文件.txt").exists());
    assert!(!root.files.join("用户目录").exists());
    set_owner(&info, 0);
    empty_trash_root(&root).unwrap();
    assert!(list_trash_root(&root).unwrap().is_empty());
    assert!(!root.files.join("用户文件.txt").exists());

    let foreign_root = home.join("foreign-trash");
    fs::create_dir(&foreign_root).unwrap();
    set_owner(&foreign_root, 1000);
    assert!(private_trash_root(foreign_root).is_err());
}

fn set_owner(path: &Path, uid: u32) {
    let path = CString::new(path.as_os_str().as_bytes()).unwrap();
    assert_eq!(
        unsafe { libc::lchown(path.as_ptr(), uid, uid) },
        0,
        "{}",
        io::Error::last_os_error()
    );
}
