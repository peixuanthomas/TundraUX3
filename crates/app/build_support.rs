use std::path::{Path, PathBuf};
use std::process::Command;

pub(crate) fn tracked_files(workspace: &Path) -> Vec<PathBuf> {
    let Ok(output) = Command::new("git")
        .args(["ls-files", "-z"])
        .current_dir(workspace)
        .output()
    else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }
    output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|path| !path.is_empty())
        .filter_map(|relative| std::str::from_utf8(relative).ok())
        .map(|relative| workspace.join(relative))
        .collect()
}

pub(crate) fn tracked_dirty(workspace: &Path) -> bool {
    Command::new("git")
        .args(["status", "--porcelain", "--untracked-files=no"])
        .current_dir(workspace)
        .output()
        .map(|output| output.status.success() && !output.stdout.is_empty())
        .unwrap_or(false)
}

pub(crate) fn git_output(workspace: &Path, args: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(workspace)
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
}
