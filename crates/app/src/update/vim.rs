//! The pinned Vim directory is one payload in the existing update transaction.
use super::*;

#[cfg(test)]
pub(super) const UPSTREAM_MANIFEST: &str = include_str!("../../../../packaging/vim.json");
const EXECUTABLE: &str = if cfg!(windows) { "vim.exe" } else { "vim" };

#[derive(Deserialize)]
struct UpstreamManifest {
    name: String,
    version: String,
    commit: String,
    source_url: String,
    source_sha256: String,
    license: String,
    required_features: Vec<String>,
}

pub(super) fn prepared_directory(prepared: &PreparedUpdate) -> Result<PathBuf, UpdateError> {
    prepared
        .shell_exe
        .parent()
        .map(|parent| parent.join("vim"))
        .ok_or_else(|| UpdateError::new("compiled Shell has no parent directory"))
}

pub(super) fn validate_bundle(directory: &Path) -> Result<(), UpdateError> {
    for relative in [
        EXECUTABLE,
        "runtime/defaults.vim",
        "runtime/doc/help.txt",
        "LICENSE",
        "UPSTREAM.json",
    ] {
        let path = directory.join(relative);
        platform::validate_no_follow_path(&path, true)
            .map_err(|error| UpdateError::new(format!("unsafe bundled Vim path: {error}")))?;
        let metadata = fs::metadata(&path)?;
        if !metadata.is_file() || metadata.len() == 0 {
            return Err(UpdateError::new(format!(
                "bundled Vim file is missing or empty: {}",
                path.display()
            )));
        }
    }
    let metadata: UpstreamManifest =
        serde_json::from_slice(&fs::read(directory.join("UPSTREAM.json"))?)
            .map_err(|error| UpdateError::new(format!("invalid bundled Vim metadata: {error}")))?;
    let valid_hex = |value: &str, length| {
        value.len() == length && value.bytes().all(|byte| byte.is_ascii_hexdigit())
    };
    // The new build owns its exact pin; older updaters must also accept a
    // future release that deliberately updates the bundled Vim version.
    if metadata.name != "Vim"
        || Version::parse(&metadata.version).is_err()
        || !valid_hex(&metadata.commit, 40)
        || !valid_hex(&metadata.source_sha256, 64)
        || metadata.source_url
            != format!(
                "https://codeload.github.com/vim/vim/tar.gz/{}",
                metadata.commit
            )
        || metadata.license != "Vim License"
        || ["eval", "channel", "autocmd", "timers", "multi_byte"]
            .iter()
            .any(|required| {
                !metadata
                    .required_features
                    .iter()
                    .any(|feature| feature == required)
            })
    {
        return Err(UpdateError::new(
            "bundled Vim has invalid upstream metadata or required features",
        ));
    }
    Ok(())
}

#[cfg(any(windows, target_os = "linux"))]
pub(super) fn copy_directory(
    source: &Path,
    target: &Path,
    cancel: &UpdateCancellation,
) -> Result<(), UpdateError> {
    cancel.check()?;
    platform::validate_no_follow_path(source, true)
        .map_err(|error| UpdateError::new(format!("unsafe bundled Vim directory: {error}")))?;
    fs::create_dir(target)?;
    for entry in fs::read_dir(source)? {
        cancel.check()?;
        let entry = entry?;
        let source = entry.path();
        let target = target.join(entry.file_name());
        platform::validate_no_follow_path(&source, true)
            .map_err(|error| UpdateError::new(format!("unsafe bundled Vim entry: {error}")))?;
        if entry.file_type()?.is_dir() {
            copy_directory(&source, &target, cancel)?;
        } else if entry.file_type()?.is_file() {
            copy_file(&source, &target, cancel)?;
        } else {
            return Err(UpdateError::new(format!(
                "unsupported bundled Vim entry: {}",
                source.display()
            )));
        }
    }
    cancel.check()
}

#[cfg(any(windows, target_os = "linux"))]
pub(super) fn replace_directory(
    target: &Path,
    replacement: &Path,
    backup: &Path,
) -> Result<(), UpdateError> {
    for (path, required) in [(target, false), (replacement, true), (backup, false)] {
        platform::validate_no_follow_path(path, required)
            .map_err(|error| UpdateError::new(format!("unsafe Vim replacement path: {error}")))?;
    }
    if target.exists() {
        fs::rename(target, backup)?;
    }
    fs::rename(replacement, target)?;
    Ok(())
}
