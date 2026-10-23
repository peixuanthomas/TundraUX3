//! Download the project's pinned, prebuilt Vim without a local compiler.
use super::*;
use sha2::{Digest, Sha256};
use std::io::Write;

const PIN: &str = include_str!("../../../../../packaging/vim.json");

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VimInstallPhase {
    Checking,
    Downloading,
    Extracting,
    Verifying,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VimInstallProgress {
    pub phase: VimInstallPhase,
    pub downloaded: u64,
    pub total: u64,
}

impl Default for VimInstallProgress {
    fn default() -> Self {
        Self {
            phase: VimInstallPhase::Checking,
            downloaded: 0,
            total: 0,
        }
    }
}

pub fn installed_vim_directory(platform: &dyn Platform) -> Result<PathBuf, UpdateError> {
    Ok(platform
        .app_paths()
        .map_err(|error| UpdateError::new(error.to_string()))?
        .data_path()
        .join("vim"))
}

pub fn install_vim_engine(
    directory: &Path,
    progress: &mut dyn FnMut(VimInstallProgress),
    cancel: &UpdateCancellation,
) -> Result<(), UpdateError> {
    let target = match (std::env::consts::OS, std::env::consts::ARCH) {
        ("windows", "x86_64") => "windows-x64",
        ("linux", "x86_64") => "linux-x86_64",
        ("macos", "x86_64" | "aarch64") => "macos-universal",
        _ => return Err(UpdateError::new(i18n::tr!("vim-install-unsupported"))),
    };
    let pin: serde_json::Value =
        serde_json::from_str(PIN).map_err(|error| UpdateError::new(error.to_string()))?;
    let version = pin["version"].as_str().unwrap();
    let name = format!("TundraUX3-vim-{version}-{target}.zip");
    let prefix = format!(
        "https://github.com/{GITHUB_OWNER}/{GITHUB_REPO}/releases/download/vim-v{version}/"
    );
    install_from(&prefix, &name, directory, progress, cancel)
}

fn install_from(
    asset_prefix: &str,
    name: &str,
    directory: &Path,
    progress: &mut dyn FnMut(VimInstallProgress),
    cancel: &UpdateCancellation,
) -> Result<(), UpdateError> {
    cancel.check()?;
    progress(VimInstallProgress::default());
    let client = Client::new(Duration::from_secs(300), cancel)?;
    let archive_url = format!("{asset_prefix}{name}");
    let response = client.get(format!("{archive_url}.sha256")).send()?;
    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Err(UpdateError::new(i18n::tr!("vim-install-not-published")));
    }
    let checksum = checked(response)?.text()?;
    let digest = checksum
        .split_whitespace()
        .next()
        .filter(|digest| digest.len() == 64 && digest.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .ok_or_else(|| UpdateError::new(i18n::tr!("vim-install-invalid-checksum")))?;
    platform::validate_no_follow_path(directory, false)
        .map_err(|error| UpdateError::new(error.to_string()))?;
    let work = platform::create_temp_dir(directory.parent().unwrap(), "vim-install")
        .map_err(|error| UpdateError::new(error.to_string()))?;
    let _owned = WorkDirectory::new(work.clone());
    let archive = work.join("vim.zip");
    let mut output = File::create(&archive)?;
    let mut response = checked(client.get(&archive_url).send()?)?;
    let mut hash = Sha256::new();
    let mut meter = VimInstallProgress {
        phase: VimInstallPhase::Downloading,
        downloaded: 0,
        total: response.content_length().unwrap_or(0),
    };
    progress(meter);
    let mut bytes = [0; 64 * 1024];
    loop {
        cancel.check()?;
        let size = response.read(&mut bytes)?;
        if size == 0 {
            break;
        }
        output.write_all(&bytes[..size])?;
        hash.update(&bytes[..size]);
        meter.downloaded += size as u64;
        progress(meter);
    }
    output.sync_all()?;
    drop(output);
    if (meter.total != 0 && meter.downloaded != meter.total)
        || format!("{:x}", hash.finalize()) != digest.to_ascii_lowercase()
    {
        return Err(UpdateError::new(i18n::tr!("vim-install-damaged-download")));
    }
    meter.total = meter.downloaded;
    meter.phase = VimInstallPhase::Extracting;
    progress(meter);
    let mut zip = zip::ZipArchive::new(File::open(archive)?)
        .map_err(|error| UpdateError::new(error.to_string()))?;
    let staged = work.join("unpacked");
    for index in 0..zip.len() {
        cancel.check()?;
        let mut entry = zip
            .by_index(index)
            .map_err(|error| UpdateError::new(error.to_string()))?;
        let relative = entry
            .enclosed_name()
            .filter(|path| path.starts_with("vim"))
            .ok_or_else(|| UpdateError::new(i18n::tr!("vim-install-unsafe-archive")))?;
        if entry.is_symlink() {
            return Err(UpdateError::new(i18n::tr!("vim-install-unsafe-archive")));
        }
        let path = staged.join(relative);
        if entry.is_dir() {
            fs::create_dir_all(&path)?;
            continue;
        }
        fs::create_dir_all(path.parent().unwrap())?;
        let mut file = fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&path)?;
        super::super::cancellation::copy_stream(&mut entry, &mut file, cancel)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(
                &path,
                fs::Permissions::from_mode(entry.unix_mode().unwrap_or(0o644) & 0o777),
            )?;
        }
    }
    meter.phase = VimInstallPhase::Verifying;
    progress(meter);
    let staged = staged.join("vim");
    super::validate_bundle(&staged)?;
    let actual: serde_json::Value =
        serde_json::from_slice(&fs::read(staged.join("UPSTREAM.json"))?)
            .map_err(|error| UpdateError::new(error.to_string()))?;
    if actual != serde_json::from_str::<serde_json::Value>(PIN).unwrap() {
        return Err(UpdateError::new(i18n::tr!("vim-install-mismatched-bundle")));
    }
    cancel.check()?;
    let previous = work.join("previous");
    let replacing = directory.exists();
    if replacing {
        fs::rename(directory, &previous)?;
    }
    if let Err(error) = fs::rename(staged, directory) {
        if replacing {
            fs::rename(&previous, directory)?;
        }
        return Err(error.into());
    }
    Ok(())
}

#[cfg(test)]
#[path = "../../../tests/unit/vim_install.rs"]
mod tests;
