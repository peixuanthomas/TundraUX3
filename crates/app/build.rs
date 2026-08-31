use std::path::PathBuf;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-env-changed=TUNDRAUX3_BUILD_COMMIT");
    let workspace = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap()).join("../..");
    println!(
        "cargo:rerun-if-changed={}",
        workspace.join(".git/HEAD").display()
    );
    println!(
        "cargo:rerun-if-changed={}",
        workspace.join(".git/index").display()
    );

    if let Ok(head) = std::fs::read_to_string(workspace.join(".git/HEAD")) {
        if let Some(reference) = head.trim().strip_prefix("ref: ") {
            println!(
                "cargo:rerun-if-changed={}",
                workspace.join(".git").join(reference).display()
            );
        }
    }
    emit_tracked_file_rerun_directives(&workspace);

    let explicit = std::env::var("TUNDRAUX3_BUILD_COMMIT")
        .ok()
        .filter(|value| !value.trim().is_empty());
    let commit = explicit
        .clone()
        .or_else(|| git_output(&workspace, &["rev-parse", "HEAD"]));
    let dirty = if explicit.is_some() {
        false
    } else {
        Command::new("git")
            .args(["status", "--porcelain", "--untracked-files=normal"])
            .current_dir(&workspace)
            .output()
            .map(|output| output.status.success() && !output.stdout.is_empty())
            .unwrap_or(false)
    };

    println!(
        "cargo:rustc-env=TUNDRAUX3_BUILD_COMMIT={}",
        commit.unwrap_or_else(|| "unknown".into())
    );
    println!("cargo:rustc-env=TUNDRAUX3_BUILD_DIRTY={dirty}");
}

fn emit_tracked_file_rerun_directives(workspace: &std::path::Path) {
    let Ok(output) = Command::new("git")
        .args(["ls-files", "-z"])
        .current_dir(workspace)
        .output()
    else {
        return;
    };
    if !output.status.success() {
        return;
    }
    for relative in output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|path| !path.is_empty())
    {
        let Ok(relative) = std::str::from_utf8(relative) else {
            continue;
        };
        let path = workspace.join(relative);
        if path.is_file() {
            println!("cargo:rerun-if-changed={}", path.display());
        }
    }
}

fn git_output(workspace: &std::path::Path, args: &[&str]) -> Option<String> {
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
