use std::env;
use std::fs;
use std::path::PathBuf;

use sha2::{Digest, Sha256};

fn main() {
    let manifest_dir = PathBuf::from(
        env::var_os("CARGO_MANIFEST_DIR").expect("Cargo must set CARGO_MANIFEST_DIR"),
    );
    let patch_path = manifest_dir.join("../../patches/wezterm-managed-v1.patch");
    println!("cargo:rerun-if-changed={}", patch_path.display());

    let patch = fs::read(&patch_path).unwrap_or_else(|error| {
        panic!(
            "cannot read managed WezTerm patch {}: {error}",
            patch_path.display()
        )
    });
    let patch_sha256 = format!("{:x}", Sha256::digest(&patch));
    println!("cargo:rustc-env=TUNDRA_WEZTERM_PATCH_SHA256={patch_sha256}");
}
