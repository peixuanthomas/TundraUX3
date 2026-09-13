//! Current process identity. No account enumeration, passwords or session creation.
use std::ffi::{CStr, OsStr};
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProcessIdentity {
    pub uid: u32,
    pub effective_uid: u32,
    pub gid: u32,
    pub effective_gid: u32,
}

impl ProcessIdentity {
    pub fn current() -> Self {
        // SAFETY: these libc getters take no pointers and do not mutate identity.
        unsafe {
            Self {
                uid: libc::getuid(),
                effective_uid: libc::geteuid(),
                gid: libc::getgid(),
                effective_gid: libc::getegid(),
            }
        }
    }

    pub fn validate(self) -> io::Result<Self> {
        if self.uid == 0
            || self.effective_uid == 0
            || self.uid != self.effective_uid
            || self.gid != self.effective_gid
        {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "Tundra must run as your logged-in ordinary Linux user with matching real/effective UID and GID. Root and set-ID execution are unsupported.",
            ));
        }
        Ok(self)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinuxUserContext {
    pub process: ProcessIdentity,
    pub gid: u32,
    pub supplementary_groups: Vec<u32>,
    pub username: String,
    pub home: PathBuf,
    pub shell: PathBuf,
    pub config_home: PathBuf,
    pub data_home: PathBuf,
    pub cache_home: PathBuf,
    pub state_home: PathBuf,
    pub runtime_dir: Option<PathBuf>,
}

impl LinuxUserContext {
    pub fn current() -> io::Result<Self> {
        let process = ProcessIdentity::current().validate()?;
        let mut capacity = 16384;
        let (username, home, shell, gid) = loop {
            let mut record = std::mem::MaybeUninit::<libc::passwd>::uninit();
            let mut buffer = vec![0u8; capacity];
            let mut result = std::ptr::null_mut();
            // SAFETY: both output buffers remain valid until their strings are copied.
            let status = unsafe {
                libc::getpwuid_r(
                    process.uid,
                    record.as_mut_ptr(),
                    buffer.as_mut_ptr().cast(),
                    buffer.len(),
                    &mut result,
                )
            };
            if status == libc::ERANGE && capacity < 1024 * 1024 {
                capacity *= 2;
                continue;
            }
            if status != 0 {
                return Err(io::Error::from_raw_os_error(status));
            }
            if result.is_null() {
                return Err(io::Error::new(
                    io::ErrorKind::NotFound,
                    "NSS has no account for the current process UID",
                ));
            }
            // SAFETY: successful getpwuid_r initialized record and its strings.
            let record = unsafe { record.assume_init() };
            if record.pw_uid != process.uid
                || record.pw_name.is_null()
                || record.pw_dir.is_null()
                || record.pw_shell.is_null()
            {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "Invalid NSS account record",
                ));
            }
            let username = unsafe { CStr::from_ptr(record.pw_name) }
                .to_str()
                .map_err(|_| {
                    io::Error::new(io::ErrorKind::InvalidData, "NSS username is not UTF-8")
                })?
                .to_owned();
            let home = PathBuf::from(OsStr::from_bytes(
                unsafe { CStr::from_ptr(record.pw_dir) }.to_bytes(),
            ));
            let shell = PathBuf::from(OsStr::from_bytes(
                unsafe { CStr::from_ptr(record.pw_shell) }.to_bytes(),
            ));
            if username.is_empty()
                || username.chars().any(char::is_control)
                || !home.is_absolute()
                || (!shell.as_os_str().is_empty() && !shell.is_absolute())
            {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "Invalid NSS account paths or name",
                ));
            }
            break (
                username,
                home,
                if shell.as_os_str().is_empty() {
                    PathBuf::from("/bin/sh")
                } else {
                    shell
                },
                record.pw_gid,
            );
        };
        // SAFETY: first query obtains length; the second is bounded by the allocated buffer.
        let count = unsafe { libc::getgroups(0, std::ptr::null_mut()) };
        if count < 0 {
            return Err(io::Error::last_os_error());
        }
        let mut supplementary_groups = vec![0; count as usize];
        let count = unsafe { libc::getgroups(count, supplementary_groups.as_mut_ptr()) };
        if count < 0 {
            return Err(io::Error::last_os_error());
        }
        supplementary_groups.truncate(count as usize);
        let dirs = super::XdgBaseDirs::from_environment(&home);
        let runtime_dir = std::env::var_os("XDG_RUNTIME_DIR")
            .map(PathBuf::from)
            .filter(|path| {
                path.is_absolute() && super::path_is_private_directory(path).unwrap_or(false)
            });
        Ok(Self {
            process,
            gid,
            supplementary_groups,
            username,
            home,
            shell,
            config_home: dirs.config,
            data_home: dirs.data,
            cache_home: dirs.cache,
            state_home: dirs.state,
            runtime_dir,
        })
    }

    pub fn environment(&self) -> [(&'static str, &OsStr); 8] {
        [
            ("HOME", self.home.as_os_str()),
            ("USER", OsStr::new(&self.username)),
            ("LOGNAME", OsStr::new(&self.username)),
            ("SHELL", self.shell.as_os_str()),
            ("XDG_CONFIG_HOME", self.config_home.as_os_str()),
            ("XDG_DATA_HOME", self.data_home.as_os_str()),
            ("XDG_CACHE_HOME", self.cache_home.as_os_str()),
            ("XDG_STATE_HOME", self.state_home.as_os_str()),
        ]
    }

    /// Call only at the single-threaded executable entry point, before any workers.
    ///
    /// # Safety
    /// No other thread may read or write the process environment during this call.
    pub unsafe fn install_environment(&self) {
        for (name, value) in self.environment() {
            // SAFETY: caller guarantees exclusive access to the environment.
            unsafe {
                std::env::set_var(name, value);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_root_and_mismatched_credentials() {
        for (uid, effective_uid, gid, effective_gid) in [
            (0, 0, 0, 0),
            (1000, 0, 1000, 1000),
            (0, 1000, 1000, 1000),
            (1000, 1001, 1000, 1000),
            (1000, 1000, 1000, 1001),
        ] {
            assert!(
                ProcessIdentity {
                    uid,
                    effective_uid,
                    gid,
                    effective_gid
                }
                .validate()
                .is_err()
            );
        }
        // No UID_MIN/UID_MAX or group-name policy.
        assert!(
            ProcessIdentity {
                uid: 42,
                effective_uid: 42,
                gid: 7,
                effective_gid: 7
            }
            .validate()
            .is_ok()
        );
    }
}
