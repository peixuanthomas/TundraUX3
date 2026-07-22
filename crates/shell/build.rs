use std::path::PathBuf;

fn main() {
    println!(
        "cargo:rerun-if-changed={}",
        ascii_assets::CANONICAL_ASSETS_DIR
    );

    let out_dir = PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR must be set"));
    if let Err(error) = ascii_assets::copy_canonical_assets_to_profile_dir(&out_dir) {
        panic!("failed to copy ASCII assets: {error}");
    }
}
