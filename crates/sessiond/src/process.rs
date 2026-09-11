use session_protocol::SystemUser;
use std::{
    collections::BTreeMap,
    ffi::CString,
    io,
    os::unix::{fs::MetadataExt, process::CommandExt},
    path::Path,
    process::Command,
};
pub fn account_by_name(name: &str) -> io::Result<SystemUser> {
    let name = CString::new(name)?;
    let mut buffer = vec![0u8; 1_048_576];
    let mut entry = std::mem::MaybeUninit::<libc::passwd>::uninit();
    let mut result = std::ptr::null_mut();
    let s = unsafe {
        libc::getpwnam_r(
            name.as_ptr(),
            entry.as_mut_ptr(),
            buffer.as_mut_ptr().cast(),
            buffer.len(),
            &mut result,
        )
    };
    if s != 0 {
        return Err(io::Error::from_raw_os_error(s));
    }
    if result.is_null() {
        return Err(io::Error::other("unknown system user"));
    }
    session_protocol::linux::account(unsafe { entry.assume_init().pw_uid })
}
pub fn environment(
    user: &SystemUser,
    pam: Vec<(String, String)>,
) -> io::Result<BTreeMap<String, String>> {
    let runtime = format!("/run/user/{}", user.uid);
    let meta = std::fs::symlink_metadata(&runtime)?;
    if !meta.is_dir() || meta.uid() != user.uid || meta.mode() & 0o077 != 0 {
        return Err(io::Error::other(
            "invalid user runtime directory ownership/mode",
        ));
    }
    let mut env = BTreeMap::new();
    // Session stacks may supply locale and session metadata; never inherit launcher's environment.
    for (k, v) in pam {
        if k == "LANG"
            || k.starts_with("LC_")
            || matches!(
                k.as_str(),
                "XDG_SESSION_ID" | "XDG_SEAT" | "XDG_VTNR" | "XDG_SESSION_CLASS"
            )
        {
            env.insert(k, v);
        }
    }
    for (k, v) in [
        ("HOME", user.home.to_string_lossy().into_owned()),
        ("USER", user.username.clone()),
        ("LOGNAME", user.username.clone()),
        ("SHELL", user.shell.to_string_lossy().into_owned()),
        ("PATH", "/usr/local/bin:/usr/bin:/bin".into()),
        ("XDG_RUNTIME_DIR", runtime.clone()),
        (
            "DBUS_SESSION_BUS_ADDRESS",
            format!("unix:path={runtime}/bus"),
        ),
        ("XDG_SESSION_TYPE", "tty".into()),
        (
            "XDG_CONFIG_HOME",
            user.home.join(".config").to_string_lossy().into_owned(),
        ),
        (
            "XDG_DATA_HOME",
            user.home
                .join(".local/share")
                .to_string_lossy()
                .into_owned(),
        ),
        (
            "XDG_CACHE_HOME",
            user.home.join(".cache").to_string_lossy().into_owned(),
        ),
        (
            "XDG_STATE_HOME",
            user.home
                .join(".local/state")
                .to_string_lossy()
                .into_owned(),
        ),
        ("TERM", "xterm-256color".into()),
        ("LIBSEAT_BACKEND", "logind".into()),
    ] {
        env.insert(k.into(), v);
    }
    env.entry("LANG".into()).or_insert("C.UTF-8".into());
    Ok(env)
}
pub fn trusted_executable(path: &Path) -> io::Result<std::path::PathBuf> {
    // Symlinks are permitted only when both their containing path and resolved
    // immutable version path remain entirely root-controlled. Execute exactly
    // this resolved target rather than looking up "current" a second time.
    fn inspect(path: &Path, depth: u32) -> io::Result<()> {
        if depth > 40 {
            return Err(io::Error::other("executable symlink chain too deep"));
        }
        for p in path.ancestors() {
            let m = std::fs::symlink_metadata(p)?;
            if m.uid() != 0 || (!m.file_type().is_symlink() && m.mode() & 0o022 != 0) {
                return Err(io::Error::other(format!(
                    "unsafe system path {}",
                    p.display()
                )));
            }
            if m.file_type().is_symlink() {
                let target = std::fs::read_link(p)?;
                let target = if target.is_absolute() {
                    target
                } else {
                    p.parent()
                        .ok_or_else(|| io::Error::other("invalid executable link"))?
                        .join(target)
                };
                inspect(&target, depth + 1)?;
            }
        }
        Ok(())
    }
    inspect(path, 0)?;
    let resolved = path.canonicalize()?;
    for p in resolved.ancestors() {
        let m = std::fs::symlink_metadata(p)?;
        if m.uid() != 0 || m.mode() & 0o022 != 0 || m.file_type().is_symlink() {
            return Err(io::Error::other("unsafe resolved executable path"));
        }
    }
    if !resolved.is_file() {
        return Err(io::Error::other("system executable is not a regular file"));
    }
    Ok(resolved)
}

pub fn demote(command: &mut Command, user: &SystemUser, keep_fd: Option<i32>) -> io::Result<()> {
    if user.uid == 0 {
        return Err(io::Error::other("refusing root desktop"));
    }
    let uid = user.uid;
    let gid = user.gid;
    let name = CString::new(user.username.as_str())?;
    let mut count = 0;
    unsafe {
        libc::getgrouplist(name.as_ptr(), gid, std::ptr::null_mut(), &mut count);
    }
    if !(1..=65536).contains(&count) {
        return Err(io::Error::other("invalid NSS group count"));
    }
    let mut groups = vec![0; count as usize];
    if unsafe { libc::getgrouplist(name.as_ptr(), gid, groups.as_mut_ptr(), &mut count) } < 0 {
        return Err(io::Error::other("NSS group membership changed"));
    }
    groups.truncate(count as usize);
    unsafe {
        command.pre_exec(move || {
            if libc::prctl(libc::PR_SET_KEEPCAPS, 0, 0, 0, 0) != 0 {
                return Err(io::Error::last_os_error());
            }
            if libc::syscall(libc::SYS_close_range, 3u32, u32::MAX, 4u32) < 0 {
                return Err(io::Error::last_os_error());
            }
            if libc::setgroups(groups.len(), groups.as_ptr()) != 0
                || libc::setresgid(gid, gid, gid) != 0
                || libc::setresuid(uid, uid, uid) != 0
            {
                return Err(io::Error::last_os_error());
            }
            if libc::getuid() != uid
                || libc::geteuid() != uid
                || libc::getgid() != gid
                || libc::getegid() != gid
            {
                return Err(io::Error::other("credential drop failed"));
            }
            if libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) != 0
                || libc::prctl(libc::PR_SET_DUMPABLE, 0, 0, 0, 0) != 0
            {
                return Err(io::Error::last_os_error());
            }
            // All descriptors are CLOEXEC by default. Explicitly preserve only the inherited private greeter channel.
            if let Some(fd) = keep_fd {
                if libc::fcntl(fd, libc::F_SETFD, 0) < 0 {
                    return Err(io::Error::last_os_error());
                }
            }
            libc::umask(0o077);
            Ok(())
        });
    }
    Ok(())
}

/// Drain this worker's verified logind cgroup without killing the PAM owner.
/// pidfds prevent a recycled numeric PID from receiving a delayed signal.
pub fn drain_session(identity: &session_protocol::SessionIdentity) -> io::Result<()> {
    use std::os::unix::io::{AsRawFd, FromRawFd};
    fn cgroup(pid: u32) -> io::Result<String> {
        let content = std::fs::read_to_string(format!("/proc/{pid}/cgroup"))?;
        content
            .lines()
            .find_map(|l| l.strip_prefix("0::").map(str::to_owned))
            .ok_or_else(|| io::Error::other("unified cgroup hierarchy required"))
    }
    fn members(path: &Path, result: &mut Vec<u32>) -> io::Result<()> {
        for line in std::fs::read_to_string(path.join("cgroup.procs"))?.lines() {
            if let Ok(pid) = line.parse() {
                result.push(pid)
            }
        }
        for item in std::fs::read_dir(path)? {
            let item = item?;
            if item.file_type()?.is_dir() {
                members(&item.path(), result)?;
            }
        }
        Ok(())
    }
    let own = std::process::id();
    let scope = cgroup(own)?;
    if !scope.ends_with(&format!("/session-{}.scope", identity.logind_session_id)) {
        return Err(io::Error::other(
            "PAM worker is not in the expected logind scope",
        ));
    }
    let path = Path::new("/sys/fs/cgroup").join(scope.trim_start_matches('/'));
    for signal in [libc::SIGTERM, libc::SIGKILL] {
        let mut pids = vec![];
        members(&path, &mut pids)?;
        for pid in pids {
            if pid == own {
                continue;
            }
            let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, pid, 0) } as i32;
            if fd < 0 {
                continue;
            }
            let fd = unsafe { std::os::fd::OwnedFd::from_raw_fd(fd) };
            if let Ok(actual) = cgroup(pid) {
                if actual == scope || actual.starts_with(&(scope.clone() + "/")) {
                    unsafe {
                        libc::syscall(
                            libc::SYS_pidfd_send_signal,
                            fd.as_raw_fd(),
                            signal,
                            std::ptr::null::<libc::siginfo_t>(),
                            0,
                        );
                    }
                }
            }
        }
        if signal == libc::SIGTERM {
            std::thread::sleep(std::time::Duration::from_millis(500));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn protected_distribution_symlink_resolves_to_fixed_executable() {
        let resolved = trusted_executable(Path::new("/bin/sh")).unwrap();
        assert!(resolved.is_absolute() && resolved.is_file());
        assert!(!std::fs::symlink_metadata(resolved)
            .unwrap()
            .file_type()
            .is_symlink());
    }
    #[test]
    fn world_writable_path_is_not_a_trusted_launcher() {
        assert!(trusted_executable(Path::new("/tmp")).is_err());
    }
}

/// Stock kmscon may accept --libseat while silently using its raw-device backend.
/// Only the pinned, capability-declared build shipped in our trusted runtime is
/// allowed. The sibling manifest is bound to the exact resolved binary digest.
pub fn verify_kmscon(binary: &Path) -> io::Result<()> {
    use sha2::Digest;
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Capabilities {
        sha256: String,
        libseat: bool,
        source_commit: String,
        pango_sha256: String,
    }
    let manifest = trusted_executable(&binary.with_file_name("kmscon-capabilities.json"))?;
    let document = std::fs::read(manifest)?;
    if document.len() > 4096 {
        return Err(io::Error::other("oversized kmscon capabilities manifest"));
    }
    let expected: Capabilities = serde_json::from_slice(&document).map_err(io::Error::other)?;
    if !expected.libseat || expected.source_commit != "ad9c77bc04f718d0f0d6dfc51291b7d652336429" {
        return Err(io::Error::other(
            "kmscon must be built from the pinned source with libseat enabled",
        ));
    }
    let module = trusted_executable(Path::new("/usr/libexec/tundra/modules/kmscon/mod-pango.so"))?;
    if binary.starts_with("/var/lib/tundra/runtime/versions") {
        let version = binary
            .parent()
            .and_then(Path::parent)
            .ok_or_else(|| io::Error::other("invalid runtime binary location"))?;
        if !module.starts_with(version.join("share/tundra/kmscon-modules")) {
            return Err(io::Error::other(
                "kmscon binary and module must belong to the same runtime version",
            ));
        }
    }
    let actual = format!("{:x}", sha2::Sha256::digest(std::fs::read(module)?));
    if actual != expected.pango_sha256 {
        return Err(io::Error::other("kmscon Pango module digest mismatch"));
    }
    let mut file = std::fs::File::open(binary)?;
    let mut digest = sha2::Sha256::new();
    std::io::copy(&mut file, &mut digest)?;
    if format!("{:x}", digest.finalize()) != expected.sha256 {
        return Err(io::Error::other(
            "kmscon binary and capabilities manifest disagree",
        ));
    }
    Ok(())
}
