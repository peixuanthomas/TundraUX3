use super::*;

fn test_path(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "tundra-storage-{name}-{}-{}",
        process::id(),
        unix_nanos()
    ))
}

#[test]
fn atomic_write_replaces_contents() {
    let directory = test_path("replace");
    let path = directory.join("state.json");

    atomic_write(&path, b"first").expect("initial write");
    atomic_write(&path, b"second").expect("replacement write");

    assert_eq!(fs::read(&path).expect("read replacement"), b"second");
    let _ = fs::remove_dir_all(directory);
}

#[cfg(unix)]
#[test]
fn application_directories_and_files_are_private() {
    use std::os::unix::fs::PermissionsExt;

    let directory = test_path("permissions");
    let path = directory.join("users.json");
    atomic_write(&path, b"{}").expect("write private file");

    assert_eq!(
        fs::metadata(&directory).unwrap().permissions().mode() & 0o777,
        0o700
    );
    assert_eq!(
        fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600
    );

    fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
    tighten_file(&path, "test tighten").expect("tighten existing file");
    assert_eq!(
        fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600
    );
    let _ = fs::remove_dir_all(directory);
}

#[cfg(unix)]
#[test]
fn symlinked_storage_destination_is_rejected() {
    use std::os::unix::fs::symlink;

    let directory = test_path("symlink");
    fs::create_dir_all(&directory).unwrap();
    let target = directory.join("outside");
    fs::write(&target, b"keep").unwrap();
    let path = directory.join("state.json");
    symlink(&target, &path).unwrap();

    let error = atomic_write(&path, b"replacement").expect_err("symlink must be rejected");
    assert!(error.to_string().contains("symbolic links"));
    assert_eq!(fs::read(&target).unwrap(), b"keep");
    let _ = fs::remove_dir_all(directory);
}

#[cfg(unix)]
#[test]
fn symlinked_storage_directory_is_rejected() {
    use std::os::unix::fs::symlink;

    let directory = test_path("symlink-dir");
    let target = test_path("symlink-target");
    fs::create_dir_all(&target).unwrap();
    symlink(&target, &directory).unwrap();

    let error = create_dir(&directory, "test directory").expect_err("symlink must be rejected");
    assert!(error.to_string().contains("symbolic links"));
    let _ = fs::remove_file(&directory);
    let _ = fs::remove_dir_all(target);
}

#[cfg(target_os = "linux")]
#[test]
fn symlinked_ancestor_cannot_redirect_atomic_write() {
    use std::os::unix::fs::symlink;

    let base = test_path("ancestor-symlink");
    let application_root = base.join("application");
    let attacker_root = base.join("attacker");
    fs::create_dir_all(&application_root).unwrap();
    fs::create_dir_all(&attacker_root).unwrap();
    let redirected_component = application_root.join("state");
    symlink(&attacker_root, &redirected_component).unwrap();
    let destination = redirected_component.join("nested").join("users.json");

    let error =
        atomic_write(&destination, b"secret").expect_err("ancestor symlink must be rejected");
    assert!(error.to_string().contains("symbolic links"));
    assert!(
        !attacker_root.join("nested").exists(),
        "write must not create entries below the symlink target"
    );
    let _ = fs::remove_dir_all(base);
}

#[cfg(target_os = "linux")]
#[test]
fn symlinked_ancestor_cannot_be_traversed_when_tightening_file() {
    use std::os::unix::fs::{PermissionsExt, symlink};

    let base = test_path("tighten-ancestor-symlink");
    let application_root = base.join("application");
    let attacker_root = base.join("attacker");
    fs::create_dir_all(&application_root).unwrap();
    fs::create_dir_all(&attacker_root).unwrap();
    let attacker_file = attacker_root.join("users.json");
    fs::write(&attacker_file, b"outside").unwrap();
    fs::set_permissions(&attacker_file, fs::Permissions::from_mode(0o644)).unwrap();
    symlink(&attacker_root, application_root.join("state")).unwrap();
    let redirected_file = application_root.join("state").join("users.json");

    let error = tighten_file(&redirected_file, "test tighten")
        .expect_err("ancestor symlink must be rejected");
    assert!(error.to_string().contains("symbolic links"));
    assert_eq!(
        fs::metadata(&attacker_file).unwrap().permissions().mode() & 0o777,
        0o644
    );
    let _ = fs::remove_dir_all(base);
}
