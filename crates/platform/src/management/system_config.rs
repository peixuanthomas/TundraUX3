//! Reviewed, version-checked text configuration writes. No arbitrary privileged
//! command is accepted. All file operations are relative to no-follow handles.
use super::*;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::ffi::{CString, OsStr};
use std::fs::{self, File};
use std::io::{Read, Write};
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::{ffi::OsStrExt, fs::MetadataExt, process::CommandExt};
use std::path::{Component, Path};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const MAX_TEXT: usize = 256 * 1024;
const MAX_BACKUP: u64 = 8 * 1024 * 1024;
const BACKUPS: &str = "/var/lib/tundraux3/config-backups";

fn failure(error: std::io::Error) -> ManagementError {
    if error.kind() == std::io::ErrorKind::PermissionDenied {
        ManagementError::PermissionDenied(error.to_string())
    } else {
        ManagementError::Failed(error.to_string())
    }
}
fn invalid(message: &str) -> ManagementError {
    ManagementError::InvalidInput(message.into())
}
fn c(value: &OsStr) -> Result<CString, ManagementError> {
    CString::new(value.as_bytes()).map_err(|_| invalid("Invalid path"))
}
fn cancelled(flag: &AtomicBool) -> Result<(), ManagementError> {
    if flag.load(Ordering::Acquire) {
        Err(ManagementError::Cancelled)
    } else {
        Ok(())
    }
}

fn checker_nonblocking(file: &impl AsRawFd) -> Result<(), ManagementError> {
    let fd = file.as_raw_fd();
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
        return Err(failure(std::io::Error::last_os_error()));
    }
    Ok(())
}

fn checker_drain(reader: &mut impl Read, bytes: &mut Vec<u8>) -> Result<(), ManagementError> {
    let mut buffer = [0u8; 4096];
    loop {
        match reader.read(&mut buffer) {
            Ok(0) => return Ok(()),
            Ok(count) => {
                if bytes.len().saturating_add(count) > 65536 {
                    return Err(ManagementError::Failed("Configuration checker output exceeded 64 KiB; inspect the configuration and retry".into()));
                }
                bytes.extend_from_slice(&buffer[..count]);
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => return Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(failure(error)),
        }
    }
}

/// Both pipes are drained by the same watchdog-owned operation. Every error,
/// cancellation or timeout reaps the checker, so no reader thread can outlive it.
fn collect_checker(
    child: &mut Child,
    flag: &AtomicBool,
    timeout: Duration,
) -> Result<(Option<ExitStatus>, Vec<u8>), ManagementError> {
    let result = (|| {
        let mut stdout = child
            .stdout
            .take()
            .ok_or_else(|| invalid("Configuration checker stdout is unavailable"))?;
        let mut stderr = child
            .stderr
            .take()
            .ok_or_else(|| invalid("Configuration checker stderr is unavailable"))?;
        checker_nonblocking(&stdout)?;
        checker_nonblocking(&stderr)?;
        let mut output = Vec::new();
        let started = Instant::now();
        loop {
            checker_drain(&mut stdout, &mut output)?;
            checker_drain(&mut stderr, &mut output)?;
            if let Some(status) = child.try_wait().map_err(failure)? {
                checker_drain(&mut stdout, &mut output)?;
                checker_drain(&mut stderr, &mut output)?;
                return Ok((Some(status), output));
            }
            cancelled(flag)?;
            if started.elapsed() >= timeout {
                return Ok((None, output));
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    })();
    if !matches!(child.try_wait(), Ok(Some(_))) {
        let _ = child.kill();
    }
    let _ = child.wait();
    result
}

/// Open every directory separately. A path replacement cannot redirect renameat.
fn parent(path: &Path, create: bool) -> Result<(File, CString), ManagementError> {
    if !path.is_absolute() || path.components().any(|p| matches!(p, Component::ParentDir)) {
        return Err(invalid("Select an absolute file path without '..'"));
    }
    let name = path
        .file_name()
        .ok_or_else(|| invalid("Select a regular file"))?;
    let mut directory = File::open("/").map_err(failure)?;
    for part in path.parent().unwrap().components() {
        let Component::Normal(part) = part else {
            continue;
        };
        let part = c(part)?;
        let flags = libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC;
        let mut fd = unsafe { libc::openat(directory.as_raw_fd(), part.as_ptr(), flags) };
        if fd < 0
            && create
            && std::io::Error::last_os_error().kind() == std::io::ErrorKind::NotFound
        {
            let inherited = directory.metadata().map_err(failure)?;
            if unsafe { libc::mkdirat(directory.as_raw_fd(), part.as_ptr(), 0o755) } < 0 {
                return Err(failure(std::io::Error::last_os_error()));
            }
            fd = unsafe { libc::openat(directory.as_raw_fd(), part.as_ptr(), flags) };
            if fd >= 0 && unsafe { libc::fchown(fd, inherited.uid(), inherited.gid()) } != 0 {
                let error = std::io::Error::last_os_error();
                unsafe {
                    libc::close(fd);
                }
                return Err(failure(error));
            }
        }
        if fd < 0 {
            return Err(failure(std::io::Error::last_os_error()));
        }
        directory = unsafe { File::from_raw_fd(fd) };
    }
    Ok((directory, c(name)?))
}

fn attributes(file: &File) -> Result<BTreeMap<String, Vec<u8>>, ManagementError> {
    let fd = file.as_raw_fd();
    let size = unsafe { libc::flistxattr(fd, std::ptr::null_mut(), 0) };
    if size < 0 {
        let error = std::io::Error::last_os_error();
        if error.raw_os_error() == Some(libc::ENOTSUP) {
            return Ok(BTreeMap::new());
        }
        return Err(failure(error));
    }
    if size as usize > MAX_TEXT {
        return Err(invalid("File attributes are too large"));
    }
    let mut names = vec![0u8; size as usize];
    if size > 0 && unsafe { libc::flistxattr(fd, names.as_mut_ptr().cast(), names.len()) } != size {
        return Err(ManagementError::Conflict(
            "File attributes changed; reload the file".into(),
        ));
    }
    let mut output = BTreeMap::new();
    for name in names.split(|b| *b == 0).filter(|n| !n.is_empty()) {
        let key =
            std::str::from_utf8(name).map_err(|_| invalid("Unsupported file attribute name"))?;
        let name = CString::new(name).unwrap();
        let size = unsafe { libc::fgetxattr(fd, name.as_ptr(), std::ptr::null_mut(), 0) };
        if size < 0 {
            return Err(failure(std::io::Error::last_os_error()));
        }
        if size as usize > MAX_TEXT {
            return Err(invalid("File attribute is too large"));
        }
        let mut value = vec![0u8; size as usize];
        if unsafe { libc::fgetxattr(fd, name.as_ptr(), value.as_mut_ptr().cast(), value.len()) }
            != size
        {
            return Err(ManagementError::Conflict(
                "File attributes changed; reload the file".into(),
            ));
        }
        output.insert(key.into(), value);
        if output.values().map(Vec::len).sum::<usize>() > MAX_TEXT {
            return Err(invalid("File attributes exceed 256 KiB in total"));
        }
    }
    Ok(output)
}

#[derive(Serialize, Deserialize)]
struct StoredFile {
    document: ConfigDocument,
    attributes: BTreeMap<String, Vec<u8>>,
}

fn read_stored(path: &Path) -> Result<StoredFile, ManagementError> {
    let mut ancestor = path
        .parent()
        .ok_or_else(|| invalid("Select a regular file"))?;
    let inherited = loop {
        match parent(&ancestor.join(".tundra-owner-probe"), false) {
            Ok((directory, _)) => break directory.metadata().map_err(failure)?,
            Err(ManagementError::Failed(_)) if !ancestor.try_exists().map_err(failure)? => {
                ancestor = ancestor
                    .parent()
                    .ok_or_else(|| invalid("Missing parent directory"))?;
            }
            Err(error) => return Err(error),
        }
    };
    let missing = || StoredFile {
        document: ConfigDocument {
            path: path.into(),
            content: String::new(),
            version: "missing".into(),
            existed: false,
            uid: inherited.uid(),
            gid: inherited.gid(),
            mode: 0o644,
            validator: detect_validator(path).into(),
            check: ConfigCheck::NotChecked,
            backup_id: None,
        },
        attributes: BTreeMap::new(),
    };
    // Missing parents are allowed for proposed new service drop-ins. Validate
    // syntax first; creation occurs only after review/authorization in apply.
    match crate::document::validate_no_follow_path(path, false) {
        Ok(()) => {}
        Err(e) => {
            return Err(match &e {
                crate::PlatformError::DetailedIo { error, .. }
                    if matches!(error.os_error_code, Some(libc::EACCES) | Some(libc::EPERM)) =>
                {
                    ManagementError::PermissionDenied(e.to_string())
                }
                _ => ManagementError::Failed(e.to_string()),
            });
        }
    }
    if !path.try_exists().map_err(failure)? {
        return Ok(missing());
    }
    let (directory, name) = parent(path, false)?;
    let fd = unsafe {
        libc::openat(
            directory.as_raw_fd(),
            name.as_ptr(),
            libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK,
        )
    };
    if fd < 0 {
        return Err(failure(std::io::Error::last_os_error()));
    }
    let file = unsafe { File::from_raw_fd(fd) };
    let before = file.metadata().map_err(failure)?;
    if !before.is_file() {
        return Err(invalid("Select a regular text file"));
    }
    let mut bytes = Vec::new();
    (&file)
        .take(MAX_TEXT as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(failure)?;
    if bytes.len() > MAX_TEXT {
        return Err(invalid("System configuration exceeds 256 KiB"));
    }
    let attrs = attributes(&file)?;
    let after = file.metadata().map_err(failure)?;
    if (
        before.len(),
        before.mtime(),
        before.mtime_nsec(),
        before.ctime(),
        before.ctime_nsec(),
    ) != (
        after.len(),
        after.mtime(),
        after.mtime_nsec(),
        after.ctime(),
        after.ctime_nsec(),
    ) {
        return Err(ManagementError::Conflict(
            "File changed while reading; reload it".into(),
        ));
    }
    let mut digest = Sha256::new();
    digest.update(&bytes);
    digest.update(format!(
        "{}:{}:{}:{}:{}:{}:{}:{}:{}",
        after.dev(),
        after.ino(),
        after.uid(),
        after.gid(),
        after.mode(),
        after.mtime(),
        after.mtime_nsec(),
        after.ctime(),
        after.ctime_nsec()
    ));
    digest.update(serde_json::to_vec(&attrs).map_err(|e| ManagementError::Failed(e.to_string()))?);
    let content = String::from_utf8(bytes)
        .map_err(|_| invalid("Only UTF-8 text configurations are supported"))?;
    if content.contains('\0') {
        return Err(invalid("The file contains binary data"));
    }
    Ok(StoredFile {
        document: ConfigDocument {
            path: path.into(),
            content,
            version: format!("{:x}", digest.finalize()),
            existed: true,
            uid: after.uid(),
            gid: after.gid(),
            mode: after.mode() & 0o7777,
            validator: detect_validator(path).into(),
            check: ConfigCheck::NotChecked,
            backup_id: None,
        },
        attributes: attrs,
    })
}

pub fn read(path: &Path) -> Result<ConfigDocument, ManagementError> {
    Ok(read_stored(path)?.document)
}

fn detect_validator(path: &Path) -> &'static str {
    let p = path.to_string_lossy();
    if p == "/etc/ssh/sshd_config" || p.starts_with("/etc/ssh/sshd_config.d/") {
        "sshd"
    } else if p == "/etc/fstab" {
        "fstab"
    } else if p.contains("/systemd/") && (p.ends_with(".service") || p.contains(".service.d/")) {
        "systemd"
    } else if p == "/etc/apt/sources.list"
        || p.starts_with("/etc/apt/sources.list.d/")
        || p.starts_with("/etc/yum.repos.d/")
        || p == "/etc/pacman.conf"
        || p.starts_with("/etc/pacman.d/")
    {
        "sources"
    } else {
        "none"
    }
}

fn native_check(
    path: &Path,
    text: &str,
    validator: &str,
    flag: &AtomicBool,
) -> Result<ConfigCheck, ManagementError> {
    validate_candidate(path, text, validator, flag, false)
}

fn ssh_main(path: &Path) -> PathBuf {
    if path.file_name() == Some(OsStr::new("sshd_config")) {
        path.into()
    } else if path.parent().and_then(Path::file_name) == Some(OsStr::new("sshd_config.d")) {
        path.parent().unwrap().parent().unwrap().join("sshd_config")
    } else {
        PathBuf::from("/etc/ssh/sshd_config")
    }
}

fn ssh_trace_contains_candidate(output: &[u8], path: &Path) -> bool {
    let marker = format!("parse_server_config_depth: config {} len ", path.display());
    String::from_utf8_lossy(output).lines().any(|line| {
        line.strip_prefix("debug2: ")
            .is_some_and(|line| line.starts_with(&marker))
    })
}

fn systemd_unit(path: &Path) -> Option<String> {
    path.components()
        .filter_map(|p| {
            if let Component::Normal(s) = p {
                s.to_str()
            } else {
                None
            }
        })
        .find_map(|s| s.strip_suffix(".service.d").map(|n| format!("{n}.service")))
        .or_else(|| {
            path.file_name()
                .and_then(OsStr::to_str)
                .filter(|s| s.ends_with(".service"))
                .map(str::to_owned)
        })
}

fn systemd_directory(path: &Path) -> PathBuf {
    let parent = path.parent().expect("configuration has a parent");
    if parent
        .file_name()
        .and_then(OsStr::to_str)
        .is_some_and(|n| n.ends_with(".service.d"))
    {
        parent
            .parent()
            .expect("drop-in has a unit directory")
            .into()
    } else {
        parent.into()
    }
}

fn systemd_is_user(path: &Path) -> bool {
    path.components()
        .filter_map(|c| c.as_os_str().to_str())
        .collect::<Vec<_>>()
        .windows(2)
        .any(|p| p == ["systemd", "user"])
}

fn validate_candidate(
    path: &Path,
    text: &str,
    validator: &str,
    flag: &AtomicBool,
    remove: bool,
) -> Result<ConfigCheck, ManagementError> {
    validate_candidate_with_program(path, text, validator, flag, remove, None)
}

fn validate_candidate_with_program(
    path: &Path,
    text: &str,
    validator: &str,
    flag: &AtomicBool,
    remove: bool,
    program_override: Option<&Path>,
) -> Result<ConfigCheck, ManagementError> {
    cancelled(flag)?;
    if text.len() > MAX_TEXT || text.contains('\0') {
        return Err(invalid("Select UTF-8 text up to 256 KiB"));
    }
    let detected = detect_validator(path);
    if detected != "none" && !matches!(validator, "auto" | "") && validator != detected {
        return Err(invalid(
            "The checker for this system file cannot be changed",
        ));
    }
    let validator = if validator == "auto" || validator.is_empty() {
        detect_validator(path)
    } else {
        validator
    };
    if validator == "none" {
        return Ok(ConfigCheck::Unavailable(
            "No configuration checker is available for this file".into(),
        ));
    }
    if validator == "sources" {
        return Ok(match super::packages::validate_source_config(path, text) {
            Ok(()) => ConfigCheck::Passed,
            Err(e) => ConfigCheck::Failed(e.to_string()),
        });
    }
    let (program, args): (&str, Vec<String>) = match validator {
        "sshd" => (
            if Path::new("/usr/sbin/sshd").is_file() {
                "/usr/sbin/sshd"
            } else {
                "/usr/bin/sshd"
            },
            vec![
                "-t".into(),
                "-ddd".into(),
                "-f".into(),
                ssh_main(path).display().to_string(),
            ],
        ),
        "fstab" => (
            "/usr/bin/findmnt",
            vec![
                "--verify".into(),
                "--tab-file".into(),
                path.display().to_string(),
            ],
        ),
        "systemd" => (
            "/usr/bin/systemd-analyze",
            vec![
                "verify".into(),
                "--man=no".into(),
                if systemd_is_user(path) {
                    "--user".into()
                } else {
                    "--system".into()
                },
                systemd_unit(path).unwrap_or_else(|| path.display().to_string()),
            ],
        ),
        _ => return Err(invalid("Unknown configuration checker")),
    };
    let program = program_override
        .map(|p| p.to_str().ok_or_else(|| invalid("Invalid checker path")))
        .transpose()?
        .unwrap_or(program);
    if !Path::new(program).is_file() {
        return Ok(ConfigCheck::Unavailable(format!(
            "{program} is missing; install the checker"
        )));
    }
    // Overlay the candidate only in the validation child. Includes and drop-ins
    // see their real absolute paths; the host configuration is never changed.
    let mut base = path
        .parent()
        .ok_or_else(|| invalid("Missing parent directory"))?
        .to_path_buf();
    while !base.is_dir() {
        if !base.pop() {
            return Err(invalid("Missing configuration directory"));
        }
    }
    if path.starts_with("/etc") {
        base = PathBuf::from("/etc");
    }
    if base == Path::new("/") {
        return Ok(ConfigCheck::Unavailable(
            "No isolated configuration directory is available".into(),
        ));
    }
    let temp = tempfile::Builder::new()
        .prefix("tundra-check-")
        .tempdir()
        .map_err(failure)?;
    let upper = temp.path().join("upper");
    let work = temp.path().join("work");
    fs::create_dir(&upper).map_err(failure)?;
    fs::create_dir(&work).map_err(failure)?;
    let candidate = upper.join(
        path.strip_prefix(&base)
            .map_err(|_| invalid("Invalid candidate directory"))?,
    );
    if let Some(p) = candidate.parent() {
        fs::create_dir_all(p).map_err(failure)?;
    }
    if remove {
        let candidate = c(candidate.as_os_str())?;
        if unsafe { libc::mknod(candidate.as_ptr(), libc::S_IFCHR | 0o600, 0) } != 0 {
            return Ok(ConfigCheck::Unavailable(
                "Cannot check removal in an isolated configuration view".into(),
            ));
        }
    } else {
        fs::write(&candidate, text).map_err(failure)?;
    }
    let target = c(base.as_os_str())?;
    let options = format!(
        "lowerdir={},upperdir={},workdir={}",
        base.display(),
        upper.display(),
        work.display()
    );
    if [base.as_path(), upper.as_path(), work.as_path()]
        .iter()
        .any(|p| {
            p.as_os_str()
                .as_bytes()
                .iter()
                .any(|b| matches!(b, b',' | b':'))
        })
    {
        return Err(invalid("This directory cannot be isolated for checking"));
    }
    let options = CString::new(options).unwrap();
    let mut command = Command::new(program);
    command
        .args(args)
        .env_clear()
        .env("PATH", "/usr/sbin:/usr/bin:/sbin:/bin")
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if validator == "systemd" {
        // The trailing colon keeps systemd's distribution unit directories.
        // The explicit directory makes a user's original unit and all of its
        // drop-ins visible even when validation runs in the root helper.
        command.env(
            "SYSTEMD_UNIT_PATH",
            format!("{}:", systemd_directory(path).display()),
        );
        if systemd_is_user(path) {
            // Verification uses an offline user manager, not the authorized
            // helper's live session or a missing /run/user/0 directory.
            command.env("XDG_RUNTIME_DIR", temp.path());
        }
    }
    #[cfg(test)]
    if program_override.is_some() {
        if let Some(libraries) = std::env::var_os("TUNDRA_TEST_CHECKER_LIBRARIES") {
            command.env("LD_LIBRARY_PATH", libraries);
        }
    }
    unsafe {
        command.pre_exec(move || {
            if libc::unshare(libc::CLONE_NEWNS) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            if libc::mount(
                std::ptr::null(),
                c"/".as_ptr(),
                std::ptr::null(),
                libc::MS_REC | libc::MS_PRIVATE,
                std::ptr::null(),
            ) != 0
            {
                return Err(std::io::Error::last_os_error());
            }
            if libc::mount(
                c"overlay".as_ptr(),
                target.as_ptr(),
                c"overlay".as_ptr(),
                0,
                options.as_ptr().cast(),
            ) != 0
            {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(e) => {
            return Ok(ConfigCheck::Unavailable(format!(
                "Cannot run isolated configuration check: {e}"
            )));
        }
    };
    let (status, output) = collect_checker(&mut child, flag, Duration::from_secs(30))?;
    cancelled(flag)?;
    Ok(match status {
        Some(s) if s.success() && validator == "sshd" && !remove
            && path != ssh_main(path) && !ssh_trace_contains_candidate(&output,path) => {
            ConfigCheck::Failed("The SSH main configuration did not include this file; edit its Include directive first".into())
        }
        Some(s) if s.success() => ConfigCheck::Passed,
        Some(_) => ConfigCheck::Failed(String::from_utf8_lossy(&output).lines().map(runtime_log::sanitize_text).collect::<Vec<_>>().join("\n")),
        None => ConfigCheck::Failed("Configuration check timed out".into()),
    })
}

pub fn check(
    path: &Path,
    content: &str,
    validator: &str,
    flag: &AtomicBool,
) -> Result<ConfigCheck, ManagementError> {
    native_check(path, content, validator, flag)
}

fn backup(previous: &StoredFile, root: &Path) -> Result<String, ManagementError> {
    let dummy = root.join("record");
    let (directory, _) = parent(&dummy, true)?;
    let metadata = directory.metadata().map_err(failure)?;
    if metadata.uid() != unsafe { libc::geteuid() } || metadata.mode() & 0o022 != 0 {
        return Err(ManagementError::PermissionDenied(
            "Recovery directory has unsafe ownership or permissions".into(),
        ));
    }
    if unsafe { libc::fchmod(directory.as_raw_fd(), 0o700) } != 0 {
        return Err(failure(std::io::Error::last_os_error()));
    }
    let id = format!(
        "{}-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| ManagementError::Failed(e.to_string()))?
            .as_nanos(),
        std::process::id()
    );
    let name = CString::new(format!("{id}.json")).unwrap();
    let fd = unsafe {
        libc::openat(
            directory.as_raw_fd(),
            name.as_ptr(),
            libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            0o600,
        )
    };
    if fd < 0 {
        return Err(failure(std::io::Error::last_os_error()));
    }
    let mut file = unsafe { File::from_raw_fd(fd) };
    serde_json::to_writer(&mut file, previous)
        .map_err(|e| ManagementError::Failed(e.to_string()))?;
    file.sync_all().map_err(failure)?;
    directory.sync_all().map_err(failure)?;
    Ok(id)
}

fn install(
    previous: &StoredFile,
    content: &str,
    uid: u32,
    gid: u32,
    mode: u32,
    attrs: &BTreeMap<String, Vec<u8>>,
    root: &Path,
    before_commit: impl FnOnce() -> Result<(), ManagementError>,
) -> Result<ConfigDocument, ManagementError> {
    let path = &previous.document.path;
    let (directory, name) = parent(path, true)?;
    let mut temporary = tempfile::Builder::new()
        .prefix(".tundra-config-")
        .tempfile_in(format!("/proc/self/fd/{}", directory.as_raw_fd()))
        .map_err(failure)?;
    temporary.write_all(content.as_bytes()).map_err(failure)?;
    let fd = temporary.as_file().as_raw_fd();
    if unsafe { libc::fchown(fd, uid, gid) } != 0 || unsafe { libc::fchmod(fd, mode) } != 0 {
        return Err(failure(std::io::Error::last_os_error()));
    }
    // A newly created inode can inherit a directory default ACL or label. The
    // candidate must retain the original file's actual attributes, not acquire
    // extra grants from its parent merely because we save by replacement.
    for name in attributes(temporary.as_file())?.keys() {
        if !attrs.contains_key(name) {
            let name =
                CString::new(name.as_bytes()).map_err(|_| invalid("Invalid file attribute"))?;
            if unsafe { libc::fremovexattr(fd, name.as_ptr()) } != 0 {
                return Err(failure(std::io::Error::last_os_error()));
            }
        }
    }
    for (key, value) in attrs {
        let key = CString::new(key.as_bytes()).map_err(|_| invalid("Invalid file attribute"))?;
        if unsafe { libc::fsetxattr(fd, key.as_ptr(), value.as_ptr().cast(), value.len(), 0) } != 0
        {
            return Err(failure(std::io::Error::last_os_error()));
        }
    }
    // Applying an ACL can change the mode bits. Apply an explicitly requested
    // mode last; unchanged ownership/ACL/xattrs must survive byte for byte.
    if unsafe { libc::fchmod(fd, mode) } != 0 {
        return Err(failure(std::io::Error::last_os_error()));
    }
    let actual = temporary.as_file().metadata().map_err(failure)?;
    let actual_attrs = attributes(temporary.as_file())?;
    if (actual.uid(), actual.gid(), actual.mode() & 0o7777) != (uid, gid, mode)
        || actual_attrs.keys().any(|name| !attrs.contains_key(name))
        || attrs.iter().any(|(name, value)| {
            !(name == "system.posix_acl_access" && mode != previous.document.mode)
                && actual_attrs.get(name) != Some(value)
        })
    {
        return Err(ManagementError::Failed(
            "Cannot preserve file ownership, permissions or extended attributes".into(),
        ));
    }
    temporary.as_file().sync_all().map_err(failure)?;
    if read(path)?.version != previous.document.version {
        return Err(ManagementError::Conflict(
            "File changed; reload and review the difference".into(),
        ));
    }
    let (fresh, _) = parent(path, false)?;
    let a = directory.metadata().map_err(failure)?;
    let b = fresh.metadata().map_err(failure)?;
    if (a.dev(), a.ino()) != (b.dev(), b.ino()) {
        return Err(ManagementError::Conflict(
            "Parent directory changed; reopen the file".into(),
        ));
    }
    let backup_id = backup(previous, root)?;
    before_commit()?;
    if read(path)?.version != previous.document.version {
        return Err(ManagementError::Conflict(
            "File changed during backup; review changes before saving".into(),
        ));
    }
    verify_parent(path, &directory)?;
    let temporary_name = c(temporary.path().file_name().unwrap())?;
    let flags = if previous.document.existed {
        0
    } else {
        libc::RENAME_NOREPLACE
    };
    if unsafe {
        libc::renameat2(
            directory.as_raw_fd(),
            temporary_name.as_ptr(),
            directory.as_raw_fd(),
            name.as_ptr(),
            flags,
        )
    } != 0
    {
        return Err(failure(std::io::Error::last_os_error()));
    }
    directory.sync_all().map_err(failure)?;
    let mut document = read(path)?;
    document.backup_id = Some(backup_id);
    Ok(document)
}

fn verify_parent(path: &Path, original: &File) -> Result<(), ManagementError> {
    let (fresh, _) = parent(path, false)?;
    let a = original.metadata().map_err(failure)?;
    let b = fresh.metadata().map_err(failure)?;
    if (a.dev(), a.ino()) != (b.dev(), b.ino()) {
        return Err(ManagementError::Conflict(
            "Parent directory changed; reopen the file".into(),
        ));
    }
    Ok(())
}

fn restore_absence(
    previous: &StoredFile,
    root: &Path,
    before_commit: impl FnOnce() -> Result<(), ManagementError>,
) -> Result<ConfigDocument, ManagementError> {
    let path = &previous.document.path;
    let (directory, name) = parent(path, false)?;
    let id = backup(previous, root)?;
    before_commit()?;
    if read(path)?.version != previous.document.version {
        return Err(ManagementError::Conflict(
            "File changed; review it before restoring".into(),
        ));
    }
    verify_parent(path, &directory)?;
    if previous.document.existed
        && unsafe { libc::unlinkat(directory.as_raw_fd(), name.as_ptr(), 0) } != 0
    {
        return Err(failure(std::io::Error::last_os_error()));
    }
    directory.sync_all().map_err(failure)?;
    let mut restored = read(path)?;
    restored.backup_id = Some(id);
    Ok(restored)
}

pub fn query(
    query: &ManagementQuery,
    flag: &AtomicBool,
) -> Result<ManagementSnapshot, ManagementError> {
    cancelled(flag)?;
    let mut snapshot = ManagementSnapshot {
        backend: "System configuration".into(),
        columns: vec!["Recovery".into(), "File".into()],
        ..Default::default()
    };
    if query.scope == "history" {
        let entries = match fs::read_dir(BACKUPS) {
            Ok(e) => e,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(snapshot),
            Err(e) => return Err(failure(e)),
        };
        for entry in entries.take(1000) {
            cancelled(flag)?;
            let entry = entry.map_err(failure)?;
            let stored = read_backup(&entry.path())?;
            if query
                .target
                .as_ref()
                .is_some_and(|p| Path::new(p) != stored.document.path)
            {
                continue;
            }
            let id = entry
                .path()
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            snapshot.rows.push(ManagementRow {
                id: id.clone(),
                cells: vec![id.clone(), stored.document.path.display().to_string()],
                identity: BTreeMap::from([(
                    "path".into(),
                    stored.document.path.display().to_string(),
                )]),
                actions: vec![ManagementAction {
                    id: "restore".into(),
                    label: "Restore old version".into(),
                    privileged: true,
                    confirm: true,
                    values: BTreeMap::from([
                        ("backup_id".into(), id),
                        ("path".into(), stored.document.path.display().to_string()),
                    ]),
                    ..Default::default()
                }],
                ..Default::default()
            });
        }
        snapshot.rows.sort_by(|a, b| b.id.cmp(&a.id));
    }
    Ok(snapshot)
}

fn read_backup(path: &Path) -> Result<StoredFile, ManagementError> {
    let (directory, name) = parent(path, false)?;
    let fd = unsafe {
        libc::openat(
            directory.as_raw_fd(),
            name.as_ptr(),
            libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if fd < 0 {
        return Err(failure(std::io::Error::last_os_error()));
    }
    let file = unsafe { File::from_raw_fd(fd) };
    let metadata = file.metadata().map_err(failure)?;
    if !metadata.is_file()
        || metadata.uid() != unsafe { libc::geteuid() }
        || metadata.mode() & 0o077 != 0
    {
        return Err(ManagementError::PermissionDenied(
            "Recovery file has unsafe permissions".into(),
        ));
    }
    if metadata.len() > MAX_BACKUP {
        return Err(invalid("Recovery file is too large"));
    }
    serde_json::from_reader(file.take(MAX_BACKUP)).map_err(|_| invalid("Recovery file is damaged"))
}

pub fn execute(
    command: &ManagementCommand,
    _context: &ExecutionContext,
    io: &mut dyn OperationInteraction,
    flag: &AtomicBool,
) -> Result<String, ManagementError> {
    cancelled(flag)?;
    let path = command
        .values
        .get("path")
        .or(command.target.as_ref())
        .ok_or_else(|| invalid("Select a configuration file"))?;
    let path = Path::new(path);
    if command.action == "read" {
        io.emit(OperationEvent::ConfigDocument {
            document: read(path)?,
        });
        return Ok("Configuration read".into());
    }
    if command.action == "history" {
        let request = ManagementQuery {
            kind: ManagementKind::SystemConfig,
            scope: "history".into(),
            target: Some(path.display().to_string()),
            ..ManagementQuery::new(ManagementKind::SystemConfig)
        };
        io.emit(OperationEvent::Snapshot {
            snapshot: query(&request, flag)?,
        });
        return Ok("Recovery records read".into());
    }
    let previous = read_stored(path)?;
    if command.action == "reload" {
        let unit = command
            .values
            .get("service")
            .ok_or_else(|| invalid("This file has no known service to reload"))?;
        if detect_validator(path) != "sshd" && detect_validator(path) != "systemd" {
            return Err(invalid("This file has no known reload operation"));
        }
        if detect_validator(path) == "sshd"
            && !matches!(unit.as_str(), "ssh.service" | "sshd.service")
        {
            return Err(invalid("SSH configuration can reload only the SSH service"));
        }
        if detect_validator(path) == "systemd" && systemd_unit(path).as_deref() != Some(unit) {
            return Err(invalid(
                "The service does not match this configuration file",
            ));
        }
        let scope = command
            .values
            .get("scope")
            .map(String::as_str)
            .unwrap_or("system");
        if !matches!(scope, "user" | "system") {
            return Err(invalid("Service scope must be system or user"));
        }
        if detect_validator(path) == "systemd" && (scope == "user") != systemd_is_user(path) {
            return Err(invalid(
                "The service scope does not match this configuration file",
            ));
        }
        if detect_validator(path) == "sshd" && scope != "system" {
            return Err(invalid("SSH reload requires the system service"));
        }
        if detect_validator(path) == "systemd" {
            super::services::execute(
                &ManagementCommand {
                    kind: ManagementKind::Services,
                    action: "daemon_reload".into(),
                    target: None,
                    values: BTreeMap::from([("scope".into(), scope.into())]),
                    identity: BTreeMap::new(),
                },
                _context,
                io,
                flag,
            )?;
            if command.values.get("reload_service").map(String::as_str) != Some("true") {
                let snapshot = super::services::query(
                    &ManagementQuery {
                        kind: ManagementKind::Services,
                        scope: scope.into(),
                        filter: unit.clone(),
                        target: Some(unit.clone()),
                        options: BTreeMap::new(),
                    },
                    flag,
                )?;
                if !snapshot.rows.iter().any(|row| row.id == *unit) {
                    return Err(ManagementError::Failed("Service definitions were reloaded, but the service was not found; inspect the unit and logs".into()));
                }
                io.emit(OperationEvent::Snapshot { snapshot });
                return Ok(
                    "Service definitions reloaded and result checked; saved changes were kept"
                        .into(),
                );
            }
        }
        super::services::execute(
            &ManagementCommand {
                kind: ManagementKind::Services,
                action: "reload".into(),
                target: Some(unit.clone()),
                values: command.values.clone(),
                identity: BTreeMap::from([
                    ("unit".into(), unit.clone()),
                    (
                        "scope".into(),
                        command
                            .values
                            .get("scope")
                            .cloned()
                            .unwrap_or_else(|| "system".into()),
                    ),
                ]),
            },
            _context,
            io,
            flag,
        )?;
        let snapshot = super::services::query(
            &ManagementQuery {
                kind: ManagementKind::Services,
                scope: scope.into(),
                filter: unit.clone(),
                target: Some(unit.clone()),
                options: BTreeMap::new(),
            },
            flag,
        )?;
        let row = snapshot
            .rows
            .iter()
            .find(|row| row.id == *unit)
            .ok_or_else(|| {
                ManagementError::Failed(
                    "Reload completed, but the service was not found; inspect the logs".into(),
                )
            })?;
        let failed = row
            .cells
            .iter()
            .any(|cell| cell == "failed" || cell == "error")
            || row
                .detail
                .iter()
                .any(|(key, value)| key == "Result" && value != "success");
        io.emit(OperationEvent::Snapshot { snapshot });
        if failed {
            return Err(ManagementError::Failed("The saved service configuration remains in place, but the service failed; view logs or restore the old version".into()));
        }
        return Ok("Service reloaded and result checked; saved changes were kept".into());
    }
    let expected = command
        .values
        .get("expected_version")
        .ok_or_else(|| invalid("Read the file first and supply its expected version"))?;
    if expected != &previous.document.version {
        return Err(ManagementError::Conflict(
            "File changed; reload and review the difference".into(),
        ));
    }
    let restoring = if command.action == "restore"
        || command.action == "preview_restore"
        || (command.action == "check" && command.values.contains_key("backup_id"))
    {
        let id = command
            .values
            .get("backup_id")
            .ok_or_else(|| invalid("Select a recovery record"))?;
        if id.is_empty() || !id.bytes().all(|b| b.is_ascii_digit() || b == b'-') {
            return Err(invalid("Invalid recovery record"));
        }
        let stored = read_backup(&Path::new(BACKUPS).join(format!("{id}.json")))?;
        if stored.document.path != path {
            return Err(invalid("Recovery record belongs to another file"));
        }
        Some(stored)
    } else {
        None
    };
    if command.action == "preview_restore" {
        let mut document = restoring.expect("restore record loaded").document;
        document.version = previous.document.version;
        io.emit(OperationEvent::ConfigDocument { document });
        return Ok("Recovery difference ready".into());
    }
    let content = restoring
        .as_ref()
        .map(|s| s.document.content.as_str())
        .or_else(|| command.values.get("content").map(String::as_str))
        .ok_or_else(|| invalid("Provide candidate text"))?;
    let validator = command
        .values
        .get("validator")
        .map(String::as_str)
        .unwrap_or("auto");
    let remove = restoring.as_ref().is_some_and(|s| !s.document.existed);
    let result = validate_candidate(path, content, validator, flag, remove)?;
    if command.action == "check" {
        let mut document = previous.document;
        document.content = content.into();
        document.check = result;
        io.emit(OperationEvent::ConfigDocument { document });
        return Ok("Configuration check finished".into());
    }
    if !matches!(command.action.as_str(), "apply" | "permissions" | "restore") {
        return Err(invalid("Unknown configuration operation"));
    }
    match &result {
        ConfigCheck::Failed(_) => {
            return Err(ManagementError::Failed(
                "Configuration check failed; run check to inspect the details".into(),
            ));
        }
        ConfigCheck::Unavailable(_) | ConfigCheck::NotChecked
            if command.values.get("allow_unvalidated").map(String::as_str) != Some("true") =>
        {
            return Err(ManagementError::Unavailable(
                "Configuration was not checked; explicitly review unvalidated saving".into(),
            ));
        }
        _ => {}
    }
    let metadata = restoring.as_ref().unwrap_or(&previous);
    let number = |key: &str, default: u32, radix: u32| -> Result<u32, ManagementError> {
        command.values.get(key).map_or(Ok(default), |v| {
            u32::from_str_radix(v, radix)
                .map_err(|_| invalid("Invalid file ownership or permission value"))
        })
    };
    let uid = number("uid", metadata.document.uid, 10)?;
    let gid = number("gid", metadata.document.gid, 10)?;
    let mode = number("mode", metadata.document.mode, 8)?;
    if mode > 0o7777 {
        return Err(invalid("File mode must contain only permission bits"));
    }
    cancelled(flag)?;
    if remove {
        let mut restored = restore_absence(&previous, Path::new(BACKUPS), || cancelled(flag))?;
        restored.check = result;
        io.emit(OperationEvent::ConfigDocument { document: restored });
        return Ok("Restored the previous absence of this file; reload when ready".into());
    }
    let mut saved = install(
        &previous,
        content,
        uid,
        gid,
        mode,
        &metadata.attributes,
        Path::new(BACKUPS),
        || cancelled(flag),
    )?;
    saved.check = result;
    io.emit(OperationEvent::ConfigDocument { document: saved });
    Ok("Configuration saved; reload only when ready".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn checker_capture_drains_both_pipes_and_reaps_on_cancel_timeout_or_overflow() {
        let spawn = |program: &str, args: &[&str]| {
            Command::new(program)
                .args(args)
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap()
        };
        let flag = AtomicBool::new(false);
        let mut child = spawn("/bin/sh", &["-c", "printf stdout; printf stderr >&2"]);
        let (status, output) = collect_checker(&mut child, &flag, Duration::from_secs(2)).unwrap();
        assert!(status.unwrap().success());
        let output = String::from_utf8(output).unwrap();
        assert!(output.contains("stdout") && output.contains("stderr"));
        let mut child = spawn("/usr/bin/sleep", &["10"]);
        assert!(matches!(
            collect_checker(&mut child, &AtomicBool::new(true), Duration::from_secs(2)),
            Err(ManagementError::Cancelled)
        ));
        assert!(child.try_wait().unwrap().is_some());
        let mut child = spawn("/usr/bin/sleep", &["10"]);
        assert!(
            collect_checker(&mut child, &flag, Duration::from_millis(20))
                .unwrap()
                .0
                .is_none()
        );
        assert!(child.try_wait().unwrap().is_some());
        let mut child = spawn("/usr/bin/head", &["-c", "70000", "/dev/zero"]);
        assert!(matches!(
            collect_checker(&mut child, &flag, Duration::from_secs(2)),
            Err(ManagementError::Failed(_))
        ));
        assert!(child.try_wait().unwrap().is_some());
    }
    #[test]
    fn versions_include_metadata_and_install_preserves_attributes_and_backup() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("config");
        fs::write(&path, "old\n").unwrap();
        let old = read_stored(&path).unwrap();
        let backup_root = temp.path().join("backups");
        let saved = install(
            &old,
            "new\n",
            old.document.uid,
            old.document.gid,
            0o640,
            &old.attributes,
            &backup_root,
            || Ok(()),
        )
        .unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "new\n");
        assert_eq!(saved.mode, 0o640);
        assert_ne!(saved.version, old.document.version);
        let recovery =
            read_backup(&backup_root.join(format!("{}.json", saved.backup_id.unwrap()))).unwrap();
        assert_eq!(recovery.document.content, "old\n");
        assert!(matches!(
            install(
                &old,
                "stale",
                old.document.uid,
                old.document.gid,
                0o600,
                &old.attributes,
                &backup_root,
                || Ok(())
            ),
            Err(ManagementError::Conflict(_))
        ));
    }
    #[test]
    fn rejects_links_and_binary_files() {
        let temp = tempfile::tempdir().unwrap();
        let file = temp.path().join("file");
        fs::write(&file, b"a\0b").unwrap();
        assert!(read(&file).is_err());
        let link = temp.path().join("link");
        std::os::unix::fs::symlink(&file, &link).unwrap();
        assert!(read(&link).is_err());
        assert!(parent(Path::new("/tmp/../etc/test"), false).is_err());
    }

    #[test]
    fn cancelled_or_denied_save_keeps_the_original_and_a_private_recovery_record() {
        for error in [
            ManagementError::Cancelled,
            ManagementError::PermissionDenied("Denied".into()),
        ] {
            let temp = tempfile::tempdir().unwrap();
            let path = temp.path().join("config");
            fs::write(&path, "original\n").unwrap();
            let previous = read_stored(&path).unwrap();
            let root = temp.path().join("backups");
            let result = install(
                &previous,
                "candidate",
                previous.document.uid,
                previous.document.gid,
                previous.document.mode,
                &previous.attributes,
                &root,
                || Err(error.clone()),
            );
            assert_eq!(result.unwrap_err(), error);
            assert_eq!(fs::read_to_string(path).unwrap(), "original\n");
            assert_eq!(fs::metadata(&root).unwrap().mode() & 0o777, 0o700);
            let file = fs::read_dir(root).unwrap().next().unwrap().unwrap().path();
            assert_eq!(fs::metadata(&file).unwrap().mode() & 0o777, 0o600);
            assert_eq!(read_backup(&file).unwrap().document.content, "original\n");
        }
    }

    #[test]
    fn external_change_during_backup_is_kept_and_parent_replacement_is_rejected() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("directory/config");
        fs::create_dir(path.parent().unwrap()).unwrap();
        fs::write(&path, "original").unwrap();
        let previous = read_stored(&path).unwrap();
        let result = install(
            &previous,
            "candidate",
            previous.document.uid,
            previous.document.gid,
            previous.document.mode,
            &previous.attributes,
            &temp.path().join("backups"),
            || {
                fs::write(&path, "external").unwrap();
                Ok(())
            },
        );
        assert!(matches!(result, Err(ManagementError::Conflict(_))));
        assert_eq!(fs::read_to_string(&path).unwrap(), "external");
        let current = read_stored(&path).unwrap();
        let result = install(
            &current,
            "candidate",
            current.document.uid,
            current.document.gid,
            current.document.mode,
            &current.attributes,
            &temp.path().join("backups"),
            || {
                fs::rename(path.parent().unwrap(), temp.path().join("old-directory")).unwrap();
                fs::create_dir(path.parent().unwrap()).unwrap();
                fs::write(&path, "replacement").unwrap();
                Ok(())
            },
        );
        assert!(matches!(result, Err(ManagementError::Conflict(_))));
        assert_eq!(fs::read_to_string(&path).unwrap(), "replacement");
        assert_eq!(
            fs::read_to_string(temp.path().join("old-directory/config")).unwrap(),
            "external"
        );
    }

    #[test]
    fn restoration_can_restore_a_missing_file_and_restore_afterwards() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("new-file");
        let missing = read_stored(&path).unwrap();
        let root = temp.path().join("backups");
        let created = install(
            &missing,
            "created",
            missing.document.uid,
            missing.document.gid,
            missing.document.mode,
            &missing.attributes,
            &root,
            || Ok(()),
        )
        .unwrap();
        let backup_missing =
            read_backup(&root.join(format!("{}.json", created.backup_id.unwrap()))).unwrap();
        assert!(!backup_missing.document.existed);
        let current = read_stored(&path).unwrap();
        let restored = restore_absence(&current, &root, || Ok(())).unwrap();
        assert!(!restored.existed && !path.exists());
        let undo =
            read_backup(&root.join(format!("{}.json", restored.backup_id.unwrap()))).unwrap();
        install(
            &read_stored(&path).unwrap(),
            &undo.document.content,
            undo.document.uid,
            undo.document.gid,
            undo.document.mode,
            &undo.attributes,
            &root,
            || Ok(()),
        )
        .unwrap();
        assert_eq!(fs::read_to_string(path).unwrap(), "created");
    }

    #[test]
    fn text_save_preserves_special_mode_acl_and_extended_attributes() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("config");
        fs::write(&path, "old").unwrap();
        let file = File::open(&path).unwrap();
        assert_eq!(unsafe { libc::fchmod(file.as_raw_fd(), 0o2640) }, 0);
        assert_eq!(
            unsafe {
                libc::fsetxattr(
                    file.as_raw_fd(),
                    c"user.tundra-test".as_ptr(),
                    b"value".as_ptr().cast(),
                    5,
                    0,
                )
            },
            0
        );
        // A named-user POSIX ACL, including its mode mask, stored in the native
        // kernel format. No external setfacl tool or host account is required.
        let mut acl = 2u32.to_le_bytes().to_vec();
        for (tag, perm, id) in [
            (1u16, 6u16, u32::MAX),
            (2, 4, 12345),
            (4, 4, u32::MAX),
            (16, 4, u32::MAX),
            (32, 0, u32::MAX),
        ] {
            acl.extend(tag.to_le_bytes());
            acl.extend(perm.to_le_bytes());
            acl.extend(id.to_le_bytes());
        }
        assert_eq!(
            unsafe {
                libc::fsetxattr(
                    file.as_raw_fd(),
                    c"system.posix_acl_access".as_ptr(),
                    acl.as_ptr().cast(),
                    acl.len(),
                    0,
                )
            },
            0
        );
        let previous = read_stored(&path).unwrap();
        let saved = install(
            &previous,
            "new",
            previous.document.uid,
            previous.document.gid,
            previous.document.mode,
            &previous.attributes,
            &temp.path().join("backups"),
            || Ok(()),
        )
        .unwrap();
        let actual = read_stored(&path).unwrap();
        assert_eq!(actual.attributes, previous.attributes);
        assert_eq!(saved.mode, previous.document.mode);
        assert_eq!(saved.uid, previous.document.uid);
        assert_eq!(saved.gid, previous.document.gid);
    }

    #[test]
    fn saving_does_not_add_a_default_acl_from_the_parent_directory() {
        let temp = tempfile::tempdir().unwrap();
        let directory = temp.path().join("with-default-acl");
        fs::create_dir(&directory).unwrap();
        let path = directory.join("config");
        fs::write(&path, "old").unwrap();
        fs::set_permissions(&path, std::os::unix::fs::PermissionsExt::from_mode(0o640)).unwrap();
        let original = read_stored(&path).unwrap();
        assert!(!original.attributes.contains_key("system.posix_acl_access"));
        let mut acl = 2u32.to_le_bytes().to_vec();
        for (tag, perm, id) in [
            (1u16, 7u16, u32::MAX),
            (2, 4, 12345),
            (4, 5, u32::MAX),
            (16, 5, u32::MAX),
            (32, 0, u32::MAX),
        ] {
            acl.extend(tag.to_le_bytes());
            acl.extend(perm.to_le_bytes());
            acl.extend(id.to_le_bytes());
        }
        let parent = File::open(&directory).unwrap();
        assert_eq!(
            unsafe {
                libc::fsetxattr(
                    parent.as_raw_fd(),
                    c"system.posix_acl_default".as_ptr(),
                    acl.as_ptr().cast(),
                    acl.len(),
                    0,
                )
            },
            0
        );
        // Demonstrate that the same directory really adds an ACL to new files.
        let inherited = directory.join("control");
        fs::write(&inherited, "control").unwrap();
        assert!(
            read_stored(&inherited)
                .unwrap()
                .attributes
                .contains_key("system.posix_acl_access")
        );
        install(
            &original,
            "new",
            original.document.uid,
            original.document.gid,
            original.document.mode,
            &original.attributes,
            &temp.path().join("backups"),
            || Ok(()),
        )
        .unwrap();
        assert_eq!(read_stored(&path).unwrap().attributes, original.attributes);
        assert_eq!(fs::read_to_string(path).unwrap(), "new");
    }

    #[test]
    fn ssh_fragment_proof_and_user_unit_resolution_are_exact() {
        let path = Path::new("/etc/ssh/sshd_config.d/a.conf");
        assert!(ssh_trace_contains_candidate(
            b"debug2: parse_server_config_depth: config /etc/ssh/sshd_config.d/a.conf len 3\n",
            path
        ));
        assert!(!ssh_trace_contains_candidate(
            b"debug2: /etc/ssh/sshd_config line 1: new include /etc/ssh/sshd_config.d/*.conf\n",
            path
        ));
        assert!(!ssh_trace_contains_candidate(b"debug2: parse_server_config_depth: config /etc/ssh/sshd_config.d/a.conf-other len 3\n",path));
        assert_eq!(
            ssh_main(Path::new("/tmp/private/sshd_config.d/a.conf")),
            Path::new("/tmp/private/sshd_config")
        );
        let path = Path::new("/home/alice/.config/systemd/user/worker@blue.service.d/tundra.conf");
        assert!(systemd_is_user(path));
        assert_eq!(systemd_unit(path).as_deref(), Some("worker@blue.service"));
        assert_eq!(
            systemd_directory(path),
            Path::new("/home/alice/.config/systemd/user")
        );
    }

    #[test]
    #[ignore = "requires a private root mount namespace and a test SSH binary; see crates/platform/docs/system-config.md"]
    fn real_isolated_ssh_main_includes_and_host_keys() {
        assert_eq!(
            unsafe { libc::geteuid() },
            0,
            "Run only in the disposable root validation namespace"
        );
        let binary = std::env::var_os("TUNDRA_TEST_SSHD")
            .expect("Provide a privately extracted OpenSSH binary");
        let binary = Path::new(&binary);
        let temp = tempfile::tempdir().unwrap();
        assert_ne!(
            fs::read_link("/proc/self/ns/mnt").unwrap(),
            fs::read_link("/proc/1/ns/mnt").unwrap(),
            "Start this test in a private mount namespace"
        );
        let passwd = temp.path().join("passwd");
        let mut accounts = fs::read_to_string("/etc/passwd").unwrap();
        if !accounts.lines().any(|line| line.starts_with("sshd:")) {
            accounts.push_str("\nsshd:x:9999:9999:Private test privilege separation:/nonexistent:/usr/sbin/nologin\n");
        }
        fs::write(&passwd, accounts).unwrap();
        assert_eq!(
            unsafe {
                libc::mount(
                    c(passwd.as_os_str()).unwrap().as_ptr(),
                    c"/etc/passwd".as_ptr(),
                    std::ptr::null(),
                    libc::MS_BIND,
                    std::ptr::null(),
                )
            },
            0,
            "Bind only in the private test namespace"
        );
        struct TestPasswdMount;
        impl Drop for TestPasswdMount {
            fn drop(&mut self) {
                unsafe {
                    libc::umount(c"/etc/passwd".as_ptr());
                }
            }
        }
        let _mount = TestPasswdMount;
        let key = temp.path().join("host-key");
        assert!(
            Command::new("/usr/bin/ssh-keygen")
                .args(["-q", "-t", "ed25519", "-N", ""])
                .arg("-f")
                .arg(&key)
                .status()
                .unwrap()
                .success()
        );
        let main = temp.path().join("sshd_config");
        let fragments = temp.path().join("sshd_config.d");
        fs::create_dir(&fragments).unwrap();
        let path = fragments.join("a.conf");
        fs::write(&path, "PasswordAuthentication no\n").unwrap();
        let main_text = format!(
            "HostKey {}\nInclude {}/*.conf\n",
            key.display(),
            fragments.display()
        );
        fs::write(&main, &main_text).unwrap();
        let flag = AtomicBool::new(false);
        let run = |path: &Path, text: &str| {
            validate_candidate_with_program(path, text, "sshd", &flag, false, Some(binary)).unwrap()
        };
        assert_eq!(
            run(&path, "PasswordAuthentication yes\n"),
            ConfigCheck::Passed
        );
        assert!(matches!(
            run(&path, "UnknownTundraDirective yes\n"),
            ConfigCheck::Failed(_)
        ));
        let nested = fragments.join("b.conf");
        fs::write(&nested, "UnknownTundraSibling yes\n").unwrap();
        assert!(matches!(
            run(&path, "PasswordAuthentication yes\n"),
            ConfigCheck::Failed(_)
        ));
        fs::remove_file(&nested).unwrap();
        fs::write(&main, format!("HostKey {}\n", key.display())).unwrap();
        assert!(matches!(
            run(&path, "PasswordAuthentication yes\n"),
            ConfigCheck::Failed(_)
        ));
        fs::write(&main, &main_text).unwrap();
        fs::write(&key, "damaged host key").unwrap();
        assert!(matches!(
            run(&path, "PasswordAuthentication yes\n"),
            ConfigCheck::Failed(_)
        ));
        assert_eq!(
            fs::read_to_string(path).unwrap(),
            "PasswordAuthentication no\n"
        );
    }

    #[test]
    #[ignore = "requires root for an isolated native systemd checker"]
    fn real_isolated_user_service_uses_original_unit_and_all_dropins() {
        assert_eq!(unsafe { libc::geteuid() }, 0);
        let temp = tempfile::tempdir().unwrap();
        let units = temp.path().join("systemd/user");
        let dropins = units.join("tundra-test.service.d");
        fs::create_dir_all(&dropins).unwrap();
        fs::write(
            units.join("tundra-test.service"),
            "[Unit]\nDescription=Private checker\n[Service]\nExecStart=/usr/bin/true\n",
        )
        .unwrap();
        let candidate = dropins.join("candidate.conf");
        fs::write(&candidate, "[Service]\nEnvironment=OLD=1\n").unwrap();
        let flag = AtomicBool::new(false);
        assert_eq!(
            check(
                &candidate,
                "[Service]\nEnvironment=NEW=1\n",
                "systemd",
                &flag
            )
            .unwrap(),
            ConfigCheck::Passed
        );
        fs::write(
            dropins.join("other.conf"),
            "[Service]\nType=tundra-invalid-type\n",
        )
        .unwrap();
        // systemd ignores unknown Type values with a warning; a broken command
        // path is a verification error, and must not be hidden by our candidate.
        fs::write(
            dropins.join("other.conf"),
            "[Service]\nExecStart=\nExecStart=/tundra/missing-program\n",
        )
        .unwrap();
        assert!(matches!(
            check(
                &candidate,
                "[Service]\nEnvironment=NEW=1\n",
                "systemd",
                &flag
            )
            .unwrap(),
            ConfigCheck::Failed(_)
        ));
        assert_eq!(
            fs::read_to_string(candidate).unwrap(),
            "[Service]\nEnvironment=OLD=1\n"
        );
        let owned = temp.path().join("owned-user-directory");
        fs::create_dir(&owned).unwrap();
        assert_eq!(
            unsafe { libc::chown(c(owned.as_os_str()).unwrap().as_ptr(), 1000, 1000) },
            0
        );
        let new_path = owned.join("systemd/user/new-user.service");
        let previous = read_stored(&new_path).unwrap();
        assert_eq!((previous.document.uid, previous.document.gid), (1000, 1000));
        let saved = install(
            &previous,
            "[Service]\nExecStart=/usr/bin/true\n",
            previous.document.uid,
            previous.document.gid,
            0o644,
            &previous.attributes,
            &temp.path().join("backups"),
            || Ok(()),
        )
        .unwrap();
        assert_eq!((saved.uid, saved.gid), (1000, 1000));
        for directory in [owned.join("systemd"), owned.join("systemd/user")] {
            let owner = fs::metadata(directory).unwrap();
            assert_eq!((owner.uid(), owner.gid()), (1000, 1000));
        }
    }
}
