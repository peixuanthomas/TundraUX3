#![cfg(windows)]

use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

use tundra_platform::windows::WindowsPlatform;
use tundra_platform::{Platform, TrashRestoreTarget};

#[test]
#[ignore = "requires native system Trash and mutates the user's Recycle Bin"]
fn native_recycle_bin_round_trip_for_temporary_file() {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let source = std::env::temp_dir().join(format!(
        "tundra-platform-recycle-smoke-{}-{nonce}.txt",
        std::process::id()
    ));
    fs::write(&source, b"temporary recycle smoke test").expect("fixture");

    WindowsPlatform
        .move_to_trash(std::slice::from_ref(&source))
        .expect("move temporary fixture to Recycle Bin");
    assert!(!source.exists());

    let entry = WindowsPlatform
        .list_trash()
        .expect("list Recycle Bin")
        .into_iter()
        .find(|entry| entry.original_path.as_deref() == Some(source.as_path()))
        .expect("temporary fixture in Recycle Bin");
    let restored = WindowsPlatform
        .restore_trash_item(&entry.id, TrashRestoreTarget::OriginalLocation)
        .expect("restore temporary fixture");
    assert_eq!(restored, source);
    assert_eq!(
        fs::read(&source).expect("restored fixture"),
        b"temporary recycle smoke test"
    );
    fs::remove_file(source).expect("cleanup fixture");
}
