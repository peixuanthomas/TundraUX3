use crate::{MAX_BUNDLE, REPOSITORY, ReleaseManifest, Result, WORKFLOW, invalid, release_version};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::Read,
    os::{
        fd::AsRawFd,
        unix::{
            fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
            process::CommandExt,
        },
    },
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

pub const ROOT: &str = "/var/lib/tundra/runtime";
pub const TRUST: &str = "/etc/tundra/update-trusted-root.jsonl";
pub const MAINTENANCE: &str = "/usr/libexec/tundra/tundra-system-maintenance";
const UNITS: &[&str] = &["tundra-sessiond.service", "tundra-privileged.service"];

pub(crate) fn root_only() -> Result<()> {
    if unsafe { libc::getuid() } != 0 || unsafe { libc::geteuid() } != 0 {
        return Err(invalid(
            "system maintenance requires a real root administrator",
        ));
    }
    Ok(())
}
/// Reject writable ancestors, symlinks and non-root objects before root opens paths.
pub(crate) fn trusted_path(path: &Path, directory: bool) -> Result<()> {
    if !path.is_absolute() {
        return Err(invalid("trusted path must be absolute"));
    }
    let mut part = PathBuf::from("/");
    for component in path.components().skip(1) {
        if !matches!(component, std::path::Component::Normal(_)) {
            return Err(invalid("invalid trusted path component"));
        }
        part.push(component);
        let metadata = fs::symlink_metadata(&part)?;
        if metadata.uid() != 0 || metadata.mode() & 0o022 != 0 || metadata.file_type().is_symlink()
        {
            return Err(invalid(format!("untrusted root path {}", part.display())));
        }
        if part != path && !metadata.is_dir() {
            return Err(invalid("non-directory ancestor"));
        }
    }
    let metadata = fs::metadata(path)?;
    if (directory && !metadata.is_dir()) || (!directory && !metadata.is_file()) {
        return Err(invalid("unexpected trusted object type"));
    }
    Ok(())
}
fn private_dir(path: &Path) -> Result<()> {
    if !path.exists() {
        let parent = path.parent().ok_or_else(|| invalid("missing parent"))?;
        trusted_path(parent, true)?;
        fs::DirBuilder::new().mode(0o700).create(path)?;
    }
    trusted_path(path, true)?;
    if fs::metadata(path)?.mode() & 0o077 != 0 {
        return Err(invalid("maintenance state directory must have mode 0700"));
    }
    Ok(())
}
use std::os::unix::fs::DirBuilderExt;
fn public_dir(path: &Path) -> Result<()> {
    if !path.exists() {
        trusted_path(
            path.parent().ok_or_else(|| invalid("missing parent"))?,
            true,
        )?;
        fs::DirBuilder::new().mode(0o755).create(path)?;
    }
    trusted_path(path, true)
}
fn state_lock() -> Result<File> {
    root_only()?;
    public_dir(Path::new("/var/lib/tundra"))?;
    public_dir(Path::new(ROOT))?;
    let path = Path::new(ROOT).join("maintenance.lock");
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)?;
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        return Err(invalid("another maintenance operation is active"));
    }
    Ok(file)
}
fn sync_tree(path: &Path) -> Result<()> {
    // Root services use umask 077; published directories must remain traversable by users.
    fs::set_permissions(path, fs::Permissions::from_mode(0o755))?;
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            sync_tree(&entry.path())?;
        } else {
            File::open(entry.path())?.sync_all()?;
        }
    }
    sync_dir(path)
}
fn ensure_idle_transaction() -> Result<()> {
    let path = Path::new(ROOT).join("transaction.json");
    if path.exists() {
        trusted_path(&path, false)?;
        let transaction: Transaction = serde_json::from_reader(File::open(path)?)?;
        if !transaction.committed {
            return Err(invalid("interrupted update requires recovery first"));
        }
    }
    Ok(())
}
fn sync_dir(path: &Path) -> Result<()> {
    File::open(path)?.sync_all()?;
    Ok(())
}
fn atomic_json<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let temp = path.with_extension("pending");
    // Private root state; a stale pending file is never read as committed state.
    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(&temp)?;
    serde_json::to_writer(&mut file, value)?;
    file.sync_all()?;
    fs::rename(temp, path)?;
    sync_dir(path.parent().unwrap())
}
fn clean_command(executable: &str) -> Result<Command> {
    trusted_path(Path::new(executable), false)?;
    let mut command = Command::new(executable);
    command
        .env_clear()
        .env("PATH", "/usr/bin:/usr/sbin")
        .env("HOME", "/nonexistent")
        .env("LANG", "C.UTF-8")
        .stdin(Stdio::null());
    Ok(command)
}
fn run_bounded(command: &mut Command, seconds: u64) -> Result<()> {
    let mut child = command
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn()?;
    let deadline = Instant::now() + Duration::from_secs(seconds);
    loop {
        if let Some(status) = child.try_wait()? {
            return if status.success() {
                Ok(())
            } else {
                Err(invalid(format!("trusted command failed: {status}")))
            };
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(invalid("trusted command timed out"));
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}
fn current_manifest() -> Result<ReleaseManifest> {
    let current = Path::new(ROOT).join("current");
    let target = fs::read_link(&current)?;
    if target.parent() != Some(Path::new("versions")) {
        return Err(invalid("invalid current version pointer"));
    }
    release_version(
        target
            .file_name()
            .and_then(|s| s.to_str())
            .ok_or_else(|| invalid("invalid version"))?,
    )?;
    let manifest = Path::new(ROOT).join(target).join("release.json");
    trusted_path(&manifest, false)?;
    Ok(serde_json::from_reader(File::open(manifest)?)?)
}
/// Download as the dedicated unprivileged service account, then verify a private copy.
pub fn prepare_official_release(release_id: &str) -> Result<ReleaseManifest> {
    let _lock = state_lock()?;
    release_version(release_id)?;
    let download = Path::new(ROOT).join("download.zip");
    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(&download)?;
    let (uid, gid, _, _) = crate::migration::account_named("nobody")?;
    if uid == 0 || gid == 0 {
        return Err(invalid("invalid downloader account"));
    }
    let mut command = clean_command("/usr/bin/curl")?;
    command
        .args([
            "--fail",
            "--silent",
            "--show-error",
            "--location",
            "--proto",
            "=https",
            "--proto-redir",
            "=https",
            "--max-redirs",
            "3",
            "--max-time",
            "120",
            "--max-filesize",
            &MAX_BUNDLE.to_string(),
        ])
        .arg(format!(
            "https://github.com/{REPOSITORY}/releases/download/{release_id}/tundra-linux-x86_64.zip"
        ));
    unsafe {
        command.pre_exec(move || {
            if libc::setgroups(0, std::ptr::null()) != 0
                || libc::setresgid(gid, gid, gid) != 0
                || libc::setresuid(uid, uid, uid) != 0
                || libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) != 0
            {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let mut child = command.stdout(Stdio::piped()).spawn()?;
    let copied = std::io::copy(
        &mut child.stdout.take().unwrap().take(MAX_BUNDLE + 1),
        &mut file,
    );
    if copied.as_ref().is_err() || copied.as_ref().is_ok_and(|n| *n > MAX_BUNDLE) {
        let _ = child.kill();
        let _ = child.wait();
        copied?;
        return Err(invalid("download exceeds size bound"));
    }
    if !child.wait()?.success() {
        return Err(invalid("official release download failed"));
    }
    file.sync_all()?;
    use std::io::Seek;
    file.rewind()?;
    let result = prepare_locked(release_id, file);
    let _ = fs::remove_file(download);
    result
}
/// The caller's stream is copied before parsing; never retain a user-owned file descriptor.
pub fn prepare_release(release_id: &str, bundle: &mut dyn Read) -> Result<ReleaseManifest> {
    let _lock = state_lock()?;
    release_version(release_id)?;
    let path = Path::new(ROOT).join("stream.zip");
    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(&path)?;
    let length = std::io::copy(&mut bundle.take(MAX_BUNDLE + 1), &mut file)?;
    if length > MAX_BUNDLE {
        return Err(invalid("bundle too large"));
    }
    file.sync_all()?;
    use std::io::Seek;
    file.rewind()?;
    let result = prepare_locked(release_id, file);
    let _ = fs::remove_file(path);
    result
}
fn prepare_locked(release_id: &str, file: File) -> Result<ReleaseManifest> {
    if std::env::consts::ARCH != "x86_64" {
        return Err(invalid("system updates support x86_64 only"));
    }
    ensure_idle_transaction()?;
    let current = current_manifest()?;
    let staging = Path::new(ROOT).join("staging");
    if staging.exists() {
        trusted_path(&staging, true)?;
        fs::remove_dir_all(&staging)?;
    }
    private_dir(&staging)?;
    let mut archive = zip::ZipArchive::new(file)?;
    if archive.len() != 3 {
        return Err(invalid(
            "expected exactly release.json, runtime.zip, attestation.jsonl",
        ));
    }
    for (name, limit) in [
        ("release.json", 16 * 1024),
        ("runtime.zip", MAX_BUNDLE),
        ("attestation.jsonl", 4 * 1024 * 1024),
    ] {
        let entry = archive.by_name(name)?;
        if entry.is_dir()
            || entry.size() > limit
            || entry
                .unix_mode()
                .is_some_and(|m| m & 0o170000 != 0o100000 && m & 0o170000 != 0)
        {
            return Err(invalid("invalid envelope entry"));
        }
        let mut out = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(staging.join(name))?;
        let expected_size = entry.size();
        if std::io::copy(&mut entry.take(expected_size.saturating_add(1)), &mut out)?
            != expected_size
        {
            return Err(invalid("envelope size mismatch"));
        }
        out.sync_all()?;
    }
    let manifest: ReleaseManifest =
        serde_json::from_reader(File::open(staging.join("release.json"))?)?;
    manifest.validate(release_id, Some(&current.version))?;
    let mut runtime = File::open(staging.join("runtime.zip"))?;
    let mut hash = Sha256::new();
    std::io::copy(&mut runtime, &mut hash)?;
    if format!("{:x}", hash.finalize()) != manifest.runtime_sha256 {
        return Err(invalid("runtime digest mismatch"));
    }
    trusted_path(Path::new(TRUST), false)?;
    let mut command = clean_command("/usr/bin/gh")?;
    command
        .args(["attestation", "verify"])
        .arg(staging.join("runtime.zip"))
        .arg("--bundle")
        .arg(staging.join("attestation.jsonl"))
        .args([
            "--repo",
            REPOSITORY,
            "--signer-workflow",
            WORKFLOW,
            "--cert-identity",
            &format!("https://github.com/{WORKFLOW}@refs/heads/master"),
            "--cert-oidc-issuer",
            "https://token.actions.githubusercontent.com",
            "--source-ref",
            "refs/heads/master",
            "--source-digest",
            &manifest.source_sha,
            "--signer-digest",
            &manifest.source_sha,
            "--deny-self-hosted-runners",
            "--custom-trusted-root",
            TRUST,
        ]);
    run_bounded(&mut command, 60)?;
    // Manifest must be inside the attested archive too, preventing version metadata substitution.
    let runtime_file = File::open(staging.join("runtime.zip"))?;
    let mut runtime_zip = zip::ZipArchive::new(runtime_file)?;
    let metadata: ReleaseManifest = serde_json::from_reader(
        runtime_zip
            .by_name("share/tundra/release.json")?
            .take(16 * 1024),
    )?;
    manifest.validate_attested_metadata(&metadata)?;
    drop(runtime_zip);
    let extracted = staging.join("runtime");
    private_dir(&extracted)?;
    crate::extract_runtime(File::open(staging.join("runtime.zip"))?, &extracted)?;
    atomic_json(&extracted.join("release.json"), &manifest)?;
    let versions = Path::new(ROOT).join("versions");
    public_dir(&versions)?;
    fs::set_permissions(&extracted, fs::Permissions::from_mode(0o755))?;
    let target = versions.join(release_id);
    if target.exists() {
        trusted_path(&target, true)?;
        let existing: ReleaseManifest =
            serde_json::from_reader(File::open(target.join("release.json"))?)?;
        if existing != manifest {
            return Err(invalid(
                "staged version identity conflict; refusing replacement",
            ));
        }
        // Retrying a cancelled consent reuses the previously verified immutable version.
        fs::remove_dir_all(&extracted)?;
    } else {
        sync_tree(&extracted)?;
        fs::rename(extracted, &target)?;
    }
    sync_dir(&versions)?;
    atomic_json(&Path::new(ROOT).join("prepared.json"), &manifest)?;
    Ok(manifest)
}
#[derive(Debug, Serialize, Deserialize)]
struct Transaction {
    previous: String,
    next: String,
    committed: bool,
}
fn point_to(version: &str) -> Result<()> {
    point_to_in(Path::new(ROOT), version)
}
fn point_to_in(root: &Path, version: &str) -> Result<()> {
    release_version(version)?;
    let pending = root.join("current.pending");
    if pending.exists() || fs::symlink_metadata(&pending).is_ok() {
        fs::remove_file(&pending)?;
    }
    std::os::unix::fs::symlink(Path::new("versions").join(version), &pending)?;
    fs::rename(pending, root.join("current"))?;
    sync_dir(root)
}
fn systemctl(verb: &str) -> Result<()> {
    // is-active with multiple units succeeds when ANY unit is active.
    // Check each independently; a failed privileged service must fail the update.
    if verb == "is-active" {
        for unit in UNITS {
            run_bounded(clean_command("/usr/bin/systemctl")?.arg(verb).arg(unit), 45)?;
        }
        return Ok(());
    }
    run_bounded(
        clean_command("/usr/bin/systemctl")?.arg(verb).args(UNITS),
        45,
    )
}
fn release_maintenance() -> Result<()> {
    release_maintenance_in(Path::new("/run/tundra/maintenance-ready"), trusted_path)
}
fn release_maintenance_in(
    marker: &Path,
    validate: impl Fn(&Path, bool) -> Result<()>,
) -> Result<()> {
    match fs::symlink_metadata(marker) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.into()),
        Ok(_) => {}
    }
    validate(marker, false)?;
    fs::remove_file(marker)?;
    sync_dir(
        marker
            .parent()
            .ok_or_else(|| invalid("marker has no parent"))?,
    )
}
fn assert_no_sessions() -> Result<()> {
    // sessiond clears its root-owned marker only after PAM close; fail closed if absent service integration.
    let path = Path::new("/run/tundra/maintenance-ready");
    trusted_path(path, false)?;
    if fs::read_to_string(path)?.trim() != "sessions-closed" {
        return Err(invalid("sessiond has not completed logout"));
    }
    Ok(())
}
pub fn apply_prepared(release_id: &str) -> Result<()> {
    let _lock = state_lock()?;
    release_version(release_id)?;
    ensure_idle_transaction()?;
    assert_no_sessions()?;
    let path = Path::new(ROOT).join("prepared.json");
    trusted_path(&path, false)?;
    let next: ReleaseManifest = serde_json::from_reader(File::open(path)?)?;
    let current = current_manifest()?;
    next.validate(release_id, Some(&current.version))?;
    let version = Path::new(ROOT).join("versions").join(release_id);
    trusted_path(&version, true)?;
    let txn_path = Path::new(ROOT).join("transaction.json");
    let mut txn = Transaction {
        previous: current.version,
        next: release_id.into(),
        committed: false,
    };
    atomic_json(&txn_path, &txn)?;
    let result = (|| {
        systemctl("stop")?;
        point_to(release_id)?;
        systemctl("start")?;
        systemctl("is-active")?;
        Ok(())
    })();
    if let Err(error) = result {
        systemctl("stop")?;
        point_to(&txn.previous)?;
        systemctl("start")?;
        systemctl("is-active")?;
        fs::remove_file(&txn_path)?;
        sync_dir(Path::new(ROOT))?;
        release_maintenance()?;
        return Err(error);
    }
    txn.committed = true;
    atomic_json(&txn_path, &txn)?;
    release_maintenance()?;
    Ok(())
}
/// Run before sessiond startup. Uncommitted transactions always restore the previous version.
pub fn recover() -> Result<()> {
    let _lock = state_lock()?;
    recover_in(
        Path::new(ROOT),
        Path::new("/run/tundra/maintenance-ready"),
        trusted_path,
    )
}
fn recover_in(
    root: &Path,
    marker: &Path,
    validate: impl Fn(&Path, bool) -> Result<()>,
) -> Result<()> {
    let path = root.join("transaction.json");
    match fs::symlink_metadata(&path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            // CloseSession can finish and publish its marker before the worker
            // writes its journal. Recovery precedes session startup and holds
            // the root maintenance lock, so this protected marker is stale.
            return release_maintenance_in(marker, validate);
        }
        Err(error) => return Err(error.into()),
        Ok(_) => {}
    }
    validate(&path, false)?;
    let transaction: Transaction = serde_json::from_reader(File::open(&path)?)?;
    if !transaction.committed {
        validate(&root.join("versions").join(&transaction.previous), true)?;
        point_to_in(root, &transaction.previous)?;
        fs::remove_file(&path)?;
        sync_dir(root)?;
    }
    release_maintenance_in(marker, validate)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn recovery_clears_pre_journal_marker_and_rolls_back_before_resuming() {
        let root =
            std::env::temp_dir().join(format!("tundra-recovery-test-{}", std::process::id()));
        fs::create_dir(&root).unwrap();
        let marker = root.join("maintenance-ready");
        point_to_in(&root, "v1.0.0").unwrap();
        fs::write(&marker, "sessions-closed").unwrap();
        // The fixture represents already-protected root storage. Production
        // always uses trusted_path and the fixed root-owned state/marker paths.
        recover_in(&root, &marker, |_, _| Ok(())).unwrap();
        assert!(!marker.exists());
        assert_eq!(
            fs::read_link(root.join("current")).unwrap(),
            Path::new("versions/v1.0.0")
        );
        fs::write(&marker, "sessions-closed").unwrap();
        assert!(recover_in(&root, &marker, trusted_path).is_err());
        assert!(marker.exists());
        point_to_in(&root, "v2.0.0").unwrap();
        atomic_json(
            &root.join("transaction.json"),
            &Transaction {
                previous: "v1.0.0".into(),
                next: "v2.0.0".into(),
                committed: false,
            },
        )
        .unwrap();
        recover_in(&root, &marker, |_, _| Ok(())).unwrap();
        assert!(!marker.exists());
        assert!(!root.join("transaction.json").exists());
        assert_eq!(
            fs::read_link(root.join("current")).unwrap(),
            Path::new("versions/v1.0.0")
        );
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn atomic_pointer_replaces_stale_pending_and_rejects_paths() {
        let root = std::env::temp_dir().join(format!("tundra-pointer-test-{}", std::process::id()));
        fs::create_dir(&root).unwrap();
        point_to_in(&root, "v1.0.0").unwrap();
        std::os::unix::fs::symlink("versions/incomplete", root.join("current.pending")).unwrap();
        point_to_in(&root, "v2.0.0").unwrap();
        assert_eq!(
            fs::read_link(root.join("current")).unwrap(),
            Path::new("versions/v2.0.0")
        );
        assert!(point_to_in(&root, "../../etc").is_err());
        assert_eq!(
            fs::read_link(root.join("current")).unwrap(),
            Path::new("versions/v2.0.0")
        );
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn public_writable_ancestors_are_not_trusted_root_paths() {
        assert!(trusted_path(Path::new("/tmp"), true).is_err());
    }
    #[test]
    fn published_directories_override_private_service_umask() {
        let root =
            std::env::temp_dir().join(format!("tundra-runtime-modes-test-{}", std::process::id()));
        fs::create_dir_all(root.join("share/locales")).unwrap();
        fs::set_permissions(root.join("share"), fs::Permissions::from_mode(0o700)).unwrap();
        fs::set_permissions(
            root.join("share/locales"),
            fs::Permissions::from_mode(0o700),
        )
        .unwrap();
        sync_tree(&root).unwrap();
        for path in [&root, &root.join("share"), &root.join("share/locales")] {
            assert_eq!(fs::metadata(path).unwrap().mode() & 0o777, 0o755);
        }
        fs::remove_dir_all(root).unwrap();
    }
}
