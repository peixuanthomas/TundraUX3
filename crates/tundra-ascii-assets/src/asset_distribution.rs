use std::fs;
use std::path::{Path, PathBuf};

use crate::asset_error::AssetError;
use crate::asset_manifest::CANONICAL_ASSETS_DIR;

pub fn copy_canonical_assets_to_profile_dir(out_dir: &Path) -> Result<PathBuf, AssetError> {
    let profile_dir = cargo_profile_dir_from_out_dir(out_dir)?;
    let destination = profile_dir.join("assets");
    copy_dir_recursive(Path::new(CANONICAL_ASSETS_DIR), &destination).map_err(|error| {
        AssetError::CopyAssets {
            from: PathBuf::from(CANONICAL_ASSETS_DIR),
            destination: destination.clone(),
            error: error.to_string(),
        }
    })?;
    Ok(destination)
}

pub fn cargo_profile_dir_from_out_dir(out_dir: &Path) -> Result<PathBuf, AssetError> {
    let mut cursor = out_dir;
    while let Some(parent) = cursor.parent() {
        if cursor.file_name().is_some_and(|name| name == "build") {
            return Ok(parent.to_path_buf());
        }
        cursor = parent;
    }

    Err(AssetError::InvalidOutDir {
        out_dir: out_dir.to_path_buf(),
    })
}

fn copy_dir_recursive(source: &Path, destination: &Path) -> Result<(), std::io::Error> {
    fs::create_dir_all(destination)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            copy_dir_recursive(&source_path, &destination_path)?;
        } else if file_type.is_file() {
            if let Some(parent) = destination_path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::copy(&source_path, &destination_path)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derives_profile_dir_from_build_out_dir() {
        let out_dir = Path::new("/repo/target/debug/build/tundra-cli-abc/out");

        let profile_dir = cargo_profile_dir_from_out_dir(out_dir).expect("profile dir");

        assert_eq!(profile_dir, PathBuf::from("/repo/target/debug"));
    }
}
