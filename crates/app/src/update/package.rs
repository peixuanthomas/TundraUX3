//! Build native packages from the exact source commit already checked by the updater.
use super::*;
use platform::installation::UpdateBackend;
use platform::linux::source_packages;
use std::os::unix::fs::PermissionsExt;

pub struct PreparedPackage {
    pub work_dir: PathBuf,
    pub path: PathBuf,
    pub backend: UpdateBackend,
    pub version: String,
    pub target_sha: String,
}

pub fn prepare(
    platform: &dyn Platform,
    check: &UpdateCheckResult,
    progress: &mut dyn FnMut(UpdateProgress),
) -> Result<PreparedPackage, UpdateError> {
    let installation = platform::installation::current_installation();
    let backend = installation.backend;
    if !backend.builds_system_package() {
        return Err(UpdateError::new(
            "This installation does not support source-built system packages",
        ));
    }
    source_packages::validate_targets(backend).map_err(UpdateError::new)?;
    // Fail before downloading/building if required packaging or installation tools are absent.
    let tools: &[&str] = match backend {
        UpdateBackend::SystemDeb => &[
            "/usr/bin/dpkg-deb",
            "/usr/bin/dpkg-shlibdeps",
            "/usr/bin/apt-get",
            "/usr/bin/sudo",
        ],
        _ => &[
            "/usr/bin/makepkg",
            "/usr/bin/fakeroot",
            "/usr/bin/pacman",
            "/usr/bin/sudo",
        ],
    };
    for tool in tools {
        if !Path::new(tool).is_file() {
            return Err(UpdateError::new(format!(
                "Required package tool is missing: {tool}"
            )));
        }
    }
    let prepared = prepare_update(platform, check, progress)?;
    let result = (|| {
        let roots = fs::read_dir(prepared.work_dir.join("source"))?
            .map(|entry| entry.map(|entry| entry.path()))
            .collect::<Result<Vec<_>, _>>()?;
        if roots.len() != 1 {
            return Err(UpdateError::new("Expected one downloaded source directory"));
        }
        notify(
            progress,
            UpdatePhase::Packaging,
            "Building system package as the current user",
        );
        build(platform, &prepared, &roots[0], backend)
    })();
    if result.is_err() {
        let _ = platform.cleanup_temp_path(&prepared.work_dir);
    }
    result
}

fn copy_file(source: &Path, destination: &Path, mode: u32) -> Result<(), UpdateError> {
    if !fs::symlink_metadata(source)?.is_file() {
        return Err(UpdateError::new(
            "Package payload must contain only regular files and directories",
        ));
    }
    fs::create_dir_all(destination.parent().unwrap())?;
    fs::copy(source, destination)?;
    fs::set_permissions(destination, fs::Permissions::from_mode(mode))?;
    Ok(())
}

fn copy_tree(source: &Path, destination: &Path) -> Result<(), UpdateError> {
    if !fs::symlink_metadata(source)?.is_dir() {
        return Err(UpdateError::new(
            "Package assets must be a regular directory",
        ));
    }
    fs::create_dir_all(destination)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let output = destination.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_tree(&entry.path(), &output)?;
        } else {
            copy_file(&entry.path(), &output, 0o644)?;
        }
    }
    Ok(())
}

fn directory_modes(directory: &Path) -> Result<(), UpdateError> {
    // A private user umask must not become /usr/bin mode 0700 in the installed package.
    fs::set_permissions(directory, fs::Permissions::from_mode(0o755))?;
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            directory_modes(&entry.path())?;
        }
    }
    Ok(())
}

fn installed_size_kib(directory: &Path) -> Result<u64, UpdateError> {
    let mut size = 4;
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        size += if entry.file_type()?.is_dir() {
            installed_size_kib(&entry.path())?
        } else {
            entry.metadata()?.len().div_ceil(1024)
        };
    }
    Ok(size)
}

fn package_version(
    source: &Path,
    sha: &str,
    backend: UpdateBackend,
    timestamp: u64,
) -> Result<String, UpdateError> {
    source_archive_url(sha)?;
    let manifest: toml::Value = fs::read_to_string(source.join("Cargo.toml"))?
        .parse()
        .map_err(|e| UpdateError::new(format!("Invalid source manifest: {e}")))?;
    let version = manifest
        .get("workspace")
        .and_then(|v| v.get("package"))
        .and_then(|v| v.get("version"))
        .and_then(toml::Value::as_str)
        .ok_or_else(|| UpdateError::new("Missing workspace package version"))?;
    let v = Version::parse(version).map_err(|e| UpdateError::new(e.to_string()))?;
    let base = format!("{}.{}.{}", v.major, v.minor, v.patch);
    match backend {
        UpdateBackend::SystemDeb => Ok(format!("{base}+git{timestamp}.{sha}")),
        UpdateBackend::SystemArch => Ok(format!("{base}.r{timestamp}.g{sha}-1")),
        _ => Err(UpdateError::new("Unsupported local package format")),
    }
}

// Kept separate from downloading/compilation so real package tools can be tested offline.
fn build(
    platform: &dyn Platform,
    prepared: &PreparedUpdate,
    source: &Path,
    backend: UpdateBackend,
) -> Result<PreparedPackage, UpdateError> {
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| UpdateError::new(e.to_string()))?
        .as_secs();
    let version = package_version(source, &prepared.target_sha, backend, timestamp)?;
    let build_dir = prepared.work_dir.join("package-build");
    let root = build_dir.join("payload");
    copy_file(
        &prepared.shell_exe,
        &root.join("usr/bin/tundra-shell"),
        0o755,
    )?;
    copy_file(&prepared.cli_exe, &root.join("usr/bin/tundra-cli"), 0o755)?;
    copy_tree(
        &source.join("crates/ascii-assets/assets"),
        &root.join("usr/share/tundraux3/assets"),
    )?;
    for (from, to) in [
        (
            "packaging/debian/tundraux3.desktop",
            "usr/share/applications/tundraux3.desktop",
        ),
        ("LICENSE", "usr/share/licenses/tundraux3/LICENSE"),
        (
            "crates/weathr/LICENSE.weathr",
            "usr/share/licenses/tundraux3/LICENSE.weathr",
        ),
    ] {
        copy_file(&source.join(from), &root.join(to), 0o644)?;
    }
    directory_modes(&root)?;
    let path = match backend {
        UpdateBackend::SystemDeb => {
            // Compute shared-library dependencies against the machine that built the executables.
            fs::create_dir_all(build_dir.join("debian"))?;
            fs::write(
                build_dir.join("debian/control"),
                "Source: tundraux3\nSection: utils\nPriority: optional\nMaintainer: TundraUX3 contributors <maintainers@tundraux3.invalid>\n\nPackage: tundraux3\nArchitecture: any\nDescription: Terminal desktop environment\n",
            )?;
            let deps = run_checked(
                platform,
                ProcessSpec::new("/usr/bin/dpkg-shlibdeps")
                    .arg("-O")
                    .arg(format!("-e{}", prepared.shell_exe.display()))
                    .arg(format!("-e{}", prepared.cli_exe.display()))
                    .current_dir(&build_dir),
                "dpkg-shlibdeps",
            )?;
            let deps = deps.stdout.utf8_lossy();
            let deps = deps
                .lines()
                .find_map(|s| s.strip_prefix("shlibs:Depends="))
                .ok_or_else(|| UpdateError::new("dpkg-shlibdeps returned no dependency list"))?;
            let size = installed_size_kib(&root)?;
            fs::create_dir_all(root.join("DEBIAN"))?;
            fs::set_permissions(root.join("DEBIAN"), fs::Permissions::from_mode(0o755))?;
            fs::write(
                root.join("DEBIAN/control"),
                format!(
                    "Package: tundraux3\nVersion: {version}\nSection: utils\nPriority: optional\nArchitecture: amd64\nInstalled-Size: {size}\nMaintainer: TundraUX3 contributors <maintainers@tundraux3.invalid>\nDepends: xdg-utils, libglib2.0-bin, {deps}\nDescription: Terminal desktop environment built from GitHub\n Commit {}\n",
                    prepared.target_sha
                ),
            )?;
            let output = build_dir.join("tundraux3.deb");
            run_checked(
                platform,
                ProcessSpec::new("/usr/bin/dpkg-deb")
                    .args(["--build", "--root-owner-group"])
                    .arg(root.to_string_lossy())
                    .arg(output.to_string_lossy()),
                "dpkg-deb",
            )?;
            output
        }
        UpdateBackend::SystemArch => {
            let pkgver = version.strip_suffix("-1").unwrap();
            // Fixed recipe packages only our staged files. No downloaded install scripts run as root.
            fs::write(build_dir.join("PKGBUILD"), arch_recipe(pkgver))?;
            run_checked(
                platform,
                ProcessSpec::new("/usr/bin/makepkg")
                    .args([
                        "--force",
                        "--nodeps",
                        "--noconfirm",
                        "--config",
                        "/etc/makepkg.conf",
                    ])
                    .current_dir(&build_dir)
                    .env("PKGDEST", build_dir.to_string_lossy())
                    .env("PKGEXT", ".pkg.tar.zst"),
                "makepkg",
            )?;
            build_dir.join(format!("tundraux3-{version}-x86_64.pkg.tar.zst"))
        }
        _ => return Err(UpdateError::new("Unsupported local package format")),
    };
    if !path.is_file() {
        return Err(UpdateError::new(
            "Package tool did not produce the expected file",
        ));
    }
    Ok(PreparedPackage {
        work_dir: prepared.work_dir.clone(),
        path,
        backend,
        version,
        target_sha: prepared.target_sha.clone(),
    })
}

fn arch_recipe(pkgver: &str) -> String {
    format!(
        "pkgname=tundraux3\npkgver={pkgver}\npkgrel=1\npkgdesc='Terminal desktop environment'\narch=('x86_64')\nlicense=('MIT' 'GPL-3.0-only')\ndepends=('glibc' 'gcc-libs' 'zlib' 'xdg-utils' 'glib2')\noptions=('!strip' '!debug')\nPKGEXT='.pkg.tar.zst'\npackage() {{\n  cp -a \"$startdir/payload/.\" \"$pkgdir/\"\n}}\n"
    )
}

pub fn verify_installed(package: &PreparedPackage) -> Result<(), UpdateError> {
    let version = source_packages::installed_version(package.backend).map_err(UpdateError::new)?;
    if version != package.version {
        return Err(UpdateError::new(format!(
            "Installed version {version} does not match {}; inspect the package manager before retrying",
            package.version
        )));
    }
    source_packages::validate_targets(package.backend).map_err(UpdateError::new)?;
    validate_update_probe(Path::new("/usr/bin/tundra-shell"), &package.target_sha)?;
    validate_update_probe(Path::new("/usr/bin/tundra-cli"), &package.target_sha)
}

#[cfg(test)]
#[path = "../../tests/unit/update_package.rs"]
mod tests;
