#[path = "../build_support.rs"]
mod build_support;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

struct TempRepository(PathBuf);

impl Drop for TempRepository {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn git(repository: &Path, args: &[&str]) {
    let status = Command::new("git")
        .args(args)
        .current_dir(repository)
        .status()
        .expect("git must be available for the build identity regression test");
    assert!(status.success(), "git command failed: {args:?}");
}

#[test]
fn deleted_tracked_files_remain_watched_and_untracked_files_do_not_mark_dirty() {
    let root = std::env::temp_dir().join(format!(
        "tundra-build-identity-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&root).unwrap();
    let repository = TempRepository(root);
    git(&repository.0, &["init", "--quiet"]);
    let tracked = repository.0.join("tracked.txt");
    fs::write(&tracked, b"original\n").unwrap();
    git(&repository.0, &["add", "tracked.txt"]);
    git(
        &repository.0,
        &[
            "-c",
            "user.name=Codex Test",
            "-c",
            "user.email=codex-test@local",
            "commit",
            "--quiet",
            "-m",
            "fixture",
        ],
    );
    assert!(build_support::git_output(&repository.0, &["rev-parse", "HEAD"]).is_some());

    fs::remove_file(&tracked).unwrap();
    assert!(build_support::tracked_dirty(&repository.0));
    assert!(build_support::tracked_files(&repository.0).contains(&tracked));

    fs::write(&tracked, b"original\n").unwrap();
    assert!(!build_support::tracked_dirty(&repository.0));
    assert!(build_support::tracked_files(&repository.0).contains(&tracked));

    fs::write(repository.0.join("untracked.txt"), b"untracked\n").unwrap();
    assert!(!build_support::tracked_dirty(&repository.0));
}
