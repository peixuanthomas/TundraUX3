use super::*;
use std::io::Write;

#[test]
fn update_cancelled_check_never_runs_git_fallback() {
    let cancel = UpdateCancellation::default();
    cancel.cancel();
    let result = check_with_fallback(
        &current_build_identity(),
        "http://127.0.0.1:1",
        || panic!("cancelled API request must not start Git"),
        &cancel,
    );
    assert!(result.unwrap_err().is_cancelled());
}

#[test]
fn update_copy_observes_cancel_between_blocks() {
    struct Output {
        bytes: Vec<u8>,
        cancel: UpdateCancellation,
    }
    impl Write for Output {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.bytes.extend_from_slice(bytes);
            self.cancel.cancel();
            Ok(bytes.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    let cancel = UpdateCancellation::default();
    let mut output = Output {
        bytes: Vec::new(),
        cancel: cancel.clone(),
    };
    let input = vec![42; 1024 * 1024];
    assert!(
        copy_stream(&mut Cursor::new(&input), &mut output, &cancel)
            .unwrap_err()
            .is_cancelled()
    );
    assert_eq!(output.bytes.len(), 64 * 1024);
}

#[test]
fn update_cancelled_extraction_does_not_create_output() {
    let root = tests::update_test_root("cancel-before-extract");
    let cancel = UpdateCancellation::default();
    cancel.cancel();
    assert!(
        extract_archive(&[], &root, &cancel)
            .unwrap_err()
            .is_cancelled()
    );
    assert!(!root.exists());
}

#[test]
fn update_failed_cleanup_reports_the_retained_path() {
    let root = tests::update_test_root("cleanup-failure");
    fs::write(&root, b"not a directory").unwrap();
    let error = cleanup_update_work(&root).unwrap_err();
    assert!(error.to_string().contains(&root.display().to_string()));
    assert!(!error.is_cancelled());
    assert_eq!(fs::read(&root).unwrap(), b"not a directory");
    fs::remove_file(root).unwrap();
}

#[test]
fn update_owned_work_cleanup_never_removes_neighbouring_files() {
    let root = tests::update_test_root("owned-cleanup");
    fs::create_dir_all(root.join("owned")).unwrap();
    fs::write(root.join("user-file"), b"keep").unwrap();
    fs::write(root.join("owned/payload"), b"remove").unwrap();
    let mut owned = WorkDirectory::new(root.join("owned"));
    assert!(
        owned
            .finish::<()>(Err(UpdateError::cancelled()))
            .unwrap_err()
            .is_cancelled()
    );
    assert!(!root.join("owned").exists());
    assert_eq!(fs::read(root.join("user-file")).unwrap(), b"keep");
    fs::remove_dir_all(root).unwrap();
}

#[cfg(target_os = "linux")]
#[test]
fn update_release_download_cancels_after_first_verified_block() {
    use sha2::{Digest, Sha256};
    let cancel = UpdateCancellation::default();
    let bytes = vec![19; 512 * 1024];
    let hash = format!("{:x}", Sha256::digest(&bytes));
    let mut output = Vec::new();
    let result = release::download_archive(
        Cursor::new(&bytes),
        &mut output,
        Some(bytes.len() as u64),
        &hash,
        &mut |_| cancel.cancel(),
        &cancel,
    );
    assert!(result.unwrap_err().is_cancelled());
    assert_eq!(output.len(), 64 * 1024);
}
