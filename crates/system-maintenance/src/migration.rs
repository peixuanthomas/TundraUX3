//! Offline, explicit migration. Writes occur in a child permanently dropped to the target UID.
use crate::{Result, invalid};
use serde::{Deserialize, Serialize};
use std::{
    ffi::{CStr, CString},
    fs::{self, File},
    io::{Read, Write},
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::process::CommandExt,
    },
    path::{Path, PathBuf},
    process::{Command, Stdio},
};
const LIMIT: u64 = 128 * 1024 * 1024;
#[derive(Debug, Serialize, Deserialize)]
pub struct MigrationReport {
    pub uid: u32,
    pub home: PathBuf,
    pub applied: bool,
    pub files: Vec<String>,
    pub excluded: Vec<String>,
}
#[derive(Serialize, Deserialize)]
struct Import {
    uid: u32,
    files: Vec<(String, Vec<u8>)>,
}
pub(crate) fn account_named(name: &str) -> Result<(u32, u32, String, PathBuf)> {
    let name = CString::new(name)?;
    let mut pwd = unsafe { std::mem::zeroed() };
    let mut found = std::ptr::null_mut();
    let mut buf = vec![0_u8; 65536];
    if unsafe {
        libc::getpwnam_r(
            name.as_ptr(),
            &mut pwd,
            buf.as_mut_ptr().cast(),
            buf.len(),
            &mut found,
        )
    } != 0
        || found.is_null()
    {
        return Err(invalid("account not found"));
    }
    account_value(&pwd)
}
fn account(uid: u32) -> Result<(u32, u32, String, PathBuf)> {
    let mut pwd = unsafe { std::mem::zeroed() };
    let mut found = std::ptr::null_mut();
    let mut buf = vec![0_u8; 65536];
    if unsafe {
        libc::getpwuid_r(
            uid,
            &mut pwd,
            buf.as_mut_ptr().cast(),
            buf.len(),
            &mut found,
        )
    } != 0
        || found.is_null()
    {
        return Err(invalid("UID not found"));
    }
    account_value(&pwd)
}
fn account_value(pwd: &libc::passwd) -> Result<(u32, u32, String, PathBuf)> {
    let name = unsafe { CStr::from_ptr(pwd.pw_name) }.to_str()?.to_owned();
    let home = PathBuf::from(unsafe { CStr::from_ptr(pwd.pw_dir) }.to_str()?);
    if !home.is_absolute() {
        return Err(invalid("NSS HOME must be absolute"));
    }
    Ok((pwd.pw_uid, pwd.pw_gid, name, home))
}
fn no_sessions(uid: u32) -> Result<()> {
    crate::linux::trusted_path(Path::new("/usr/bin/loginctl"), false)?;
    let output = Command::new("/usr/bin/loginctl")
        .args([
            "show-user",
            &uid.to_string(),
            "--property=Sessions",
            "--value",
        ])
        .env_clear()
        .env("PATH", "/usr/bin")
        .output()?;
    // A user unknown to logind has no sessions; distinguish this through list-users instead of trusting errors.
    if output.status.success() {
        if !output.stdout.iter().all(|c| c.is_ascii_whitespace()) {
            return Err(invalid("target user has active login sessions"));
        }
        return Ok(());
    }
    let output = Command::new("/usr/bin/loginctl")
        .args(["list-users", "--no-legend", "--no-pager"])
        .env_clear()
        .env("PATH", "/usr/bin")
        .output()?;
    if !output.status.success() {
        return Err(invalid("cannot establish offline logind state"));
    }
    if String::from_utf8(output.stdout)?
        .lines()
        .any(|l| l.split_whitespace().next() == Some(&uid.to_string()))
    {
        return Err(invalid("cannot establish target session state"));
    }
    Ok(())
}
fn sanitized_config(input: &str) -> Result<Vec<u8>> {
    let input: toml::Value = toml::from_str(input)?;
    let source = input
        .as_table()
        .ok_or_else(|| invalid("config must be a table"))?;
    let mut clean = toml::Table::new();
    for key in [
        "theme",
        "language",
        "timezone",
        "weather_location",
        "appearance",
        "explorer",
        "editor",
    ] {
        if let Some(value) = source.get(key) {
            clean.insert(key.into(), value.clone());
        }
    }
    // Deserialize typed known fields then serialize defaults, stripping arbitrary nested unknown keys.
    let mut defaults = toml::Value::try_from(storage::StorageConfig::default())?;
    defaults.as_table_mut().unwrap().extend(clean);
    let config: storage::StorageConfig = defaults.try_into()?;
    Ok(toml::to_string_pretty(&config)?.into_bytes())
}
fn content(
    source: &Path,
    relative: &Path,
    files: &mut Vec<(String, Vec<u8>)>,
    total: &mut u64,
) -> Result<()> {
    if files.len() > 4096 {
        return Err(invalid("too many migration files"));
    }
    crate::linux::trusted_path(source, true)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let path = entry.path();
        let name = relative.join(entry.file_name());
        if !super::safe_relative(&name) {
            return Err(invalid("unsafe content path"));
        }
        if entry.file_type()?.is_dir() {
            content(&path, &name, files, total)?;
            continue;
        }
        crate::linux::trusted_path(&path, false)?;
        let size = entry.metadata()?.len();
        *total = total
            .checked_add(size)
            .ok_or_else(|| invalid("size overflow"))?;
        if *total > LIMIT {
            return Err(invalid("migration content exceeds 128 MiB"));
        }
        files.push((
            format!(
                "Documents/Tundra-import/{}",
                name.to_str()
                    .ok_or_else(|| invalid("content path must be UTF-8"))?
            ),
            fs::read(path)?,
        ));
    }
    Ok(())
}
pub fn run(source: &Path, uid: u32, apply: bool) -> Result<MigrationReport> {
    crate::linux::root_only()?;
    if uid == 0 {
        return Err(invalid("target must be an ordinary non-root user"));
    }
    let (_, gid, name, home) = account(uid)?;
    no_sessions(uid)?;
    crate::linux::trusted_path(source, true)?;
    let config = source.join("config.toml");
    let mut files = Vec::new();
    let mut total = 0;
    if config.exists() {
        crate::linux::trusted_path(&config, false)?;
        if fs::metadata(&config)?.len() > 1024 * 1024 {
            return Err(invalid("config too large"));
        }
        files.push((
            ".config/TundraUX3/config.toml".into(),
            sanitized_config(&fs::read_to_string(config)?)?,
        ));
    }
    let content_dir = source.join("content");
    if content_dir.exists() {
        content(&content_dir, Path::new(""), &mut files, &mut total)?;
    }
    let report = MigrationReport {
        uid,
        home: home.clone(),
        applied: apply,
        files: files.iter().map(|(p, _)| {
            let target=home.join(p);
            let action=if fs::symlink_metadata(&target).is_ok() {"existing destination: skip"} else {"new destination: create"};
            format!("{} ({action})",target.display())
        }).collect(),
        excluded: vec!["User records, credentials, roles, login/session state, history, security settings, launcher commands and shortcuts are never imported".into()],
    };
    if apply {
        no_sessions(uid)?;
        crate::linux::trusted_path(Path::new(crate::linux::MAINTENANCE), false)?;
        let mut command = Command::new(crate::linux::MAINTENANCE);
        command
            .arg("__import")
            .env_clear()
            .env("HOME", &home)
            .env("USER", &name)
            .env("PATH", "/usr/bin")
            .stdin(Stdio::piped())
            .stdout(Stdio::inherit());
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
        let mut child = command.spawn()?;
        let result = serde_json::to_writer(child.stdin.take().unwrap(), &Import { uid, files });
        if let Err(error) = result {
            let _ = child.kill();
            let _ = child.wait();
            return Err(error.into());
        }
        if !child.wait()?.success() {
            return Err(invalid("unprivileged import worker failed"));
        }
    }
    Ok(report)
}
/// Open each directory relative to a held FD; even user-controlled symlink races cannot redirect writes.
fn open_dir(parent: &File, name: &str) -> Result<File> {
    let name = CString::new(name)?;
    let fd = unsafe {
        libc::openat(
            parent.as_raw_fd(),
            name.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if fd < 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    Ok(unsafe { File::from_raw_fd(fd) })
}
pub fn import_stdin() -> Result<()> {
    let uid = unsafe { libc::geteuid() };
    if uid == 0 || unsafe { libc::getuid() } != uid {
        return Err(invalid(
            "import worker must already be permanently unprivileged",
        ));
    }
    let import: Import = serde_json::from_reader(std::io::stdin().take(LIMIT * 5))?;
    if import.uid != uid {
        return Err(invalid("import UID mismatch"));
    }
    let (_, _, _, home) = account(uid)?;
    let mut root = File::open("/")?;
    for part in home.components().skip(1) {
        let std::path::Component::Normal(name) = part else {
            return Err(invalid("unsafe HOME"));
        };
        root = open_dir(&root, name.to_str().ok_or_else(|| invalid("invalid HOME"))?)?;
    }
    for (path, bytes) in import.files {
        let path = Path::new(&path);
        if !super::safe_relative(path)
            || !(path.starts_with("Documents/Tundra-import")
                || path == Path::new(".config/TundraUX3/config.toml"))
        {
            return Err(invalid("invalid import target"));
        }
        let mut dir = root.try_clone()?;
        let parts: Vec<_> = path.components().collect();
        for part in &parts[..parts.len() - 1] {
            let name = part
                .as_os_str()
                .to_str()
                .ok_or_else(|| invalid("invalid target"))?;
            let cname = CString::new(name)?;
            if unsafe { libc::mkdirat(dir.as_raw_fd(), cname.as_ptr(), 0o700) } != 0
                && std::io::Error::last_os_error().raw_os_error() != Some(libc::EEXIST)
            {
                return Err(std::io::Error::last_os_error().into());
            }
            dir = open_dir(&dir, name)?;
        }
        let name = CString::new(parts.last().unwrap().as_os_str().to_str().unwrap())?;
        let fd = unsafe {
            libc::openat(
                dir.as_raw_fd(),
                name.as_ptr(),
                libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                0o600,
            )
        };
        if fd < 0 {
            if std::io::Error::last_os_error().raw_os_error() == Some(libc::EEXIST) {
                println!("Skipped existing {}", path.display());
                continue;
            }
            return Err(std::io::Error::last_os_error().into());
        }
        let mut file = unsafe { File::from_raw_fd(fd) };
        file.write_all(&bytes)?;
        file.sync_all()?;
        dir.sync_all()?;
        println!("Imported {}", path.display());
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sanitizes_authority_and_nested_unknown_fields() {
        let result = sanitized_config(
            "theme='dark'\n[security]\nrole='Admin'\n[appearance]\nrole='Admin'\n",
        )
        .unwrap();
        let text = String::from_utf8(result).unwrap();
        assert!(!text.contains("Admin"));
        assert!(!text.contains("password_hash"));
        assert!(text.contains("dark"));
    }
    #[test]
    fn directory_fd_traversal_refuses_symlinks() {
        let root =
            std::env::temp_dir().join(format!("tundra-import-dir-test-{}", std::process::id()));
        fs::create_dir(&root).unwrap();
        std::os::unix::fs::symlink("/", root.join("redirect")).unwrap();
        let fd = File::open(&root).unwrap();
        assert!(open_dir(&fd, "redirect").is_err());
        fs::create_dir(root.join("real")).unwrap();
        assert!(open_dir(&fd, "real").is_ok());
        fs::remove_dir_all(root).unwrap();
    }
}
