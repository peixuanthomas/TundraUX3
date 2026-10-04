//! State belonging to one REPL's operating-system commands, never the host
//! process. Child commands retain their inherited terminal streams.

use std::io;
use std::process::Command;

use std::{
    collections::BTreeMap,
    ffi::OsString,
    path::{Path, PathBuf},
};

#[cfg(windows)]
mod windows;

pub struct SystemCommandResult {
    pub exit_code: i32,
    /// The command ran, but its final state could not be recovered. The last
    /// complete snapshot remains available for the next command.
    pub state_error: Option<io::Error>,
}

pub struct SystemCommandSession {
    environment: BTreeMap<OsString, OsString>,
    directory: PathBuf,
    #[cfg(unix)]
    initialized: bool,
}

impl SystemCommandSession {
    pub fn new() -> io::Result<Self> {
        Ok(Self {
            environment: std::env::vars_os().collect(),
            directory: std::env::current_dir()?,
            #[cfg(unix)]
            initialized: false,
        })
    }

    /// Absolute working directory used by the next system command.
    pub fn current_dir(&self) -> &Path {
        &self.directory
    }

    /// Run explicit shell input. This API is only for the advanced command
    /// line; ordinary system operations must continue using typed adapters.
    pub fn run(&mut self, command: &str) -> io::Result<SystemCommandResult> {
        #[cfg(unix)]
        {
            self.run_unix(command)
        }
        #[cfg(windows)]
        {
            self.run_windows(command)
        }
        #[cfg(not(any(unix, windows)))]
        {
            let _ = command;
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "system commands are unsupported on this platform",
            ))
        }
    }

    #[cfg(unix)]
    fn run_unix(&mut self, command: &str) -> io::Result<SystemCommandResult> {
        use std::io::{Read, Seek, SeekFrom};
        use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
        use std::os::unix::process::CommandExt;

        // Anonymous, private files avoid publishing environment values in
        // terminal output or leaving named state files behind. High source
        // descriptors prevent collisions while installing descriptors 8/9.
        let mut environment = tempfile::tempfile()?;
        let mut directory = tempfile::tempfile()?;
        let duplicate = |file: &std::fs::File| -> io::Result<OwnedFd> {
            // SAFETY: the source is live and fcntl creates a new owned fd.
            let fd = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_DUPFD_CLOEXEC, 10) };
            if fd < 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(unsafe { OwnedFd::from_raw_fd(fd) })
        };
        let environment_fd = duplicate(&environment)?;
        let directory_fd = duplicate(&directory)?;
        let mut child = Command::new("/bin/sh");
        // Load login defaults once, rather than resetting PATH after every
        // command. The input is a separate argument, not interpolated into
        // the snapshot protocol. EXIT also captures state after `exit N`.
        child
            .args([
                if self.initialized { "-c" } else { "-lc" },
                UNIX_COMMAND,
                "tundra-system-command",
                command,
            ])
            // Some /bin/sh implementations discard inherited OLDPWD at
            // startup. Restore it explicitly so `cd -` works across calls.
            .arg(
                self.environment
                    .get(&OsString::from("OLDPWD"))
                    .cloned()
                    .unwrap_or_default(),
            )
            .arg(
                if self.environment.contains_key(&OsString::from("OLDPWD")) {
                    "1"
                } else {
                    "0"
                },
            )
            .current_dir(&self.directory)
            .env_clear()
            .envs(&self.environment);
        // SAFETY: only async-signal-safe dup2 calls run between fork/exec.
        // Both captured descriptors remain owned until status() completes.
        unsafe {
            child.pre_exec(move || {
                if libc::dup2(environment_fd.as_raw_fd(), 8) < 0
                    || libc::dup2(directory_fd.as_raw_fd(), 9) < 0
                {
                    return Err(io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let status = child.status()?;
        self.initialized = true;
        let snapshot = (|| {
            let mut environment_bytes = Vec::new();
            let mut directory_bytes = Vec::new();
            environment.seek(SeekFrom::Start(0))?;
            directory.seek(SeekFrom::Start(0))?;
            environment.read_to_end(&mut environment_bytes)?;
            directory.read_to_end(&mut directory_bytes)?;
            parse_unix_snapshot(&environment_bytes, &directory_bytes)
        })();
        let state_error = match snapshot {
            Ok((environment, directory)) => {
                self.environment = environment;
                self.directory = directory;
                None
            }
            Err(error) => Some(error),
        };
        Ok(SystemCommandResult {
            exit_code: status.code().unwrap_or(1),
            state_error,
        })
    }
}

// Use an absolute env path: `command -p env` itself replaces the child's
// PATH on some shells. cwd has its own stream so filename newlines survive.
// The extra NUL is a completion marker; partial snapshots are never applied.
#[cfg(unix)]
const UNIX_COMMAND: &str = r#"trap 'command -p pwd -P >&9 && /usr/bin/env -0 >&8 && command -p printf "\0" >&8' EXIT
export OLDPWD
if [ "$3" = 1 ]; then OLDPWD=$2; fi
eval "$1"
"#;

#[cfg(unix)]
fn parse_unix_snapshot(
    environment: &[u8],
    directory: &[u8],
) -> io::Result<(BTreeMap<OsString, OsString>, PathBuf)> {
    use std::os::unix::ffi::OsStringExt;

    let invalid = || {
        io::Error::other(
            "the command did not leave a complete environment/directory snapshot (it may have used exec, replaced the EXIT trap, or been interrupted)",
        )
    };
    let environment = environment.strip_suffix(&[0]).ok_or_else(invalid)?;
    let directory = directory.strip_suffix(b"\n").ok_or_else(invalid)?;
    if !directory.starts_with(b"/") || directory.contains(&0) {
        return Err(invalid());
    }
    let mut values = BTreeMap::new();
    for record in environment.split_inclusive(|byte| *byte == 0) {
        let record = record.strip_suffix(&[0]).ok_or_else(invalid)?;
        let separator = record
            .iter()
            .position(|byte| *byte == b'=')
            .ok_or_else(invalid)?;
        if separator == 0 {
            return Err(invalid());
        }
        values.insert(
            OsString::from_vec(record[..separator].to_vec()),
            OsString::from_vec(record[separator + 1..].to_vec()),
        );
    }
    Ok((
        values,
        PathBuf::from(OsString::from_vec(directory.to_vec())),
    ))
}

#[cfg(all(test, any(unix, windows)))]
#[path = "../tests/unit/command_session/tests.rs"]
mod tests;
