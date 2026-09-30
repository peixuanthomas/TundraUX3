use super::*;
#[test]
fn writable_directory_without_marker_is_not_a_portable_installation() {
    use std::os::unix::fs::symlink;
    let uid = super::super::identity::LinuxUserContext::current()
        .unwrap()
        .process
        .uid;
    let temp_root =
        std::env::temp_dir().join(format!("tundra-installation-test-{}", std::process::id()));
    let root = crate::create_temp_dir(&temp_root, "installation").unwrap();
    for name in ["tundra-shell", "tundra-cli"] {
        std::fs::write(root.join(name), b"fixture").unwrap();
    }
    assert!(validate_portable_directory(&root, uid).is_err());
    std::fs::write(
        root.join(PORTABLE_MARKER),
        crate::installation::PORTABLE_MARKER_CONTENT,
    )
    .unwrap();
    assert!(validate_portable_directory(&root, uid).is_ok());
    std::fs::remove_file(root.join("tundra-cli")).unwrap();
    symlink("tundra-shell", root.join("tundra-cli")).unwrap();
    assert!(validate_portable_directory(&root, uid).is_err());
    std::fs::remove_dir_all(temp_root).unwrap();
}
