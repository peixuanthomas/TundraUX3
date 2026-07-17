#![cfg(any(windows, target_os = "macos"))]

use std::fs;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use tundra_platform::{TrashRestoreTarget, native_platform};

#[test]
#[ignore = "requires native system Trash and temporarily adds an item to it"]
fn native_trash_round_trip_for_temporary_file() {
    let source = unique_temp_path("file", "txt");
    fs::write(&source, b"temporary trash smoke test").expect("fixture");

    round_trip(&source);
    assert_eq!(
        fs::read(&source).expect("restored fixture"),
        b"temporary trash smoke test"
    );
    fs::remove_file(source).expect("cleanup fixture");
}

#[test]
#[ignore = "requires native system Trash and temporarily adds an item to it"]
fn native_trash_round_trip_for_temporary_directory() {
    let source = unique_temp_path("directory", "fixture");
    fs::create_dir(&source).expect("fixture directory");
    fs::write(source.join("payload.txt"), b"directory trash smoke test").expect("fixture payload");

    round_trip(&source);
    assert_eq!(
        fs::read(source.join("payload.txt")).expect("restored fixture payload"),
        b"directory trash smoke test"
    );
    fs::remove_dir_all(source).expect("cleanup fixture");
}

fn unique_temp_path(kind: &str, extension: &str) -> std::path::PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    std::env::temp_dir().join(format!(
        "tundra-platform-trash-smoke-{kind}-{}-{nonce}.{extension}",
        std::process::id(),
    ))
}

fn round_trip(source: &Path) {
    let platform = native_platform();
    #[cfg(windows)]
    let source_for_trash = {
        let canonical = fs::canonicalize(source).expect("canonicalize fixture");
        assert!(
            canonical.to_string_lossy().starts_with(r"\\?\"),
            "Windows regression fixture must use a verbatim path"
        );
        canonical
    };
    #[cfg(target_os = "macos")]
    let source_for_trash = source.to_path_buf();

    platform
        .move_to_trash(&[source_for_trash])
        .expect("move temporary fixture to Trash");
    assert!(!source.exists());

    let entry = platform
        .list_trash()
        .expect("list Trash")
        .into_iter()
        .find(|entry| entry.original_path.as_deref() == Some(source))
        .expect("temporary fixture in Trash");
    let restored = platform
        .restore_trash_item(&entry.id, TrashRestoreTarget::OriginalLocation)
        .expect("restore temporary fixture");
    assert_eq!(restored, source);
}
