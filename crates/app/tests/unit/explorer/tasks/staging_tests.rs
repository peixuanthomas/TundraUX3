use super::*;
use std::os::unix::fs::PermissionsExt;

#[test]
fn staged_copy_is_private_before_writing_content() {
    let root = std::env::temp_dir().join(format!(
        "tundra-explorer-private-stage-{}-{}",
        std::process::id(),
        ENGINE_SEQUENCE.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&root).unwrap();
    let path = root.join(".tundra-stage-private.txt");
    let mut file = open_staging_file(&path).unwrap();

    // Inspect the exact creation helper before it can receive private bytes.
    // The result may be more restrictive under the caller's umask, but never
    // accessible to group or other users.
    assert_eq!(file.metadata().unwrap().permissions().mode() & 0o077, 0);
    file.write_all(b"private bytes").unwrap();
    assert_eq!(fs::metadata(&path).unwrap().permissions().mode() & 0o077, 0);
    assert!(open_staging_file(&path).is_err());
    drop(file);
    fs::remove_dir_all(root).unwrap();
}
