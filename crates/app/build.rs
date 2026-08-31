use std::path::PathBuf;

mod build_support;

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
    for path in build_support::tracked_files(&workspace) {
        println!("cargo:rerun-if-changed={}", path.display());
    }

    let explicit = std::env::var("TUNDRAUX3_BUILD_COMMIT")
        .ok()
        .filter(|value| !value.trim().is_empty());
    let commit = explicit
        .clone()
        .or_else(|| build_support::git_output(&workspace, &["rev-parse", "HEAD"]));
    let dirty = if explicit.is_some() {
        false
    } else {
        build_support::tracked_dirty(&workspace)
    };

    println!(
        "cargo:rustc-env=TUNDRAUX3_BUILD_COMMIT={}",
        commit.unwrap_or_else(|| "unknown".into())
    );
    println!("cargo:rustc-env=TUNDRAUX3_BUILD_DIRTY={dirty}");
}
