//! State belonging to one REPL's operating-system commands, never the host
//! process. Child commands retain their inherited terminal streams.

use std::io;
use std::process::Command;

use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::{OsStr, OsString},
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
    #[cfg(target_os = "linux")]
    shell: Option<PathBuf>,
}

/// An owned snapshot of command lookup settings, without the rest of the
/// session environment. Creating it does no filesystem work.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExecutableSearchPath {
    directory: PathBuf,
    path: Option<OsString>,
    #[cfg(windows)]
    extensions: Option<OsString>,
}

fn is_executable_file(path: &Path) -> bool {
    let Ok(metadata) = std::fs::metadata(path) else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

impl SystemCommandSession {
    pub fn new() -> io::Result<Self> {
        Ok(Self {
            environment: std::env::vars_os().collect(),
            directory: std::env::current_dir()?,
            #[cfg(unix)]
            initialized: false,
            #[cfg(target_os = "linux")]
            shell: None,
        })
    }

    /// Absolute working directory used by the next system command.
    pub fn current_dir(&self) -> &Path {
        &self.directory
    }

    fn environment_value(&self, name: &str) -> Option<&OsStr> {
        self.environment.iter().find_map(|(key, value)| {
            let matches = if cfg!(windows) {
                key.to_string_lossy().eq_ignore_ascii_case(name)
            } else {
                key == name
            };
            matches.then_some(value.as_os_str())
        })
    }

    pub fn executable_search_path(&self) -> ExecutableSearchPath {
        ExecutableSearchPath {
            directory: self.directory.clone(),
            path: self.environment_value("PATH").map(OsStr::to_os_string),
            #[cfg(windows)]
            extensions: self.environment_value("PATHEXT").map(OsStr::to_os_string),
        }
    }
}

impl ExecutableSearchPath {
    /// Read executable names without running them. Call from a worker: a
    /// filesystem operation may wait on a slow or unavailable PATH directory.
    /// Cancellation is checked between reads; it cannot interrupt an OS call.
    pub fn executable_names(&self, cancelled: impl Fn() -> bool) -> Vec<String> {
        let mut directories = BTreeSet::new();
        if cfg!(windows) {
            directories.insert(self.directory.clone());
        }
        if let Some(path) = &self.path {
            directories.extend(std::env::split_paths(path).map(|dir| self.directory.join(dir)));
        }
        #[cfg(windows)]
        let extensions = self
            .extensions
            .as_deref()
            .unwrap_or_else(|| OsStr::new(".COM;.EXE;.BAT;.CMD"))
            .to_string_lossy()
            .split(';')
            .filter(|ext| ext.starts_with('.') && !ext.contains(['/', '\\']))
            .map(str::to_ascii_lowercase)
            .collect::<Vec<_>>();
        let mut names = BTreeSet::new();
        for directory in directories {
            if cancelled() {
                return Vec::new();
            }
            let Ok(entries) = std::fs::read_dir(directory) else {
                continue;
            };
            for entry in entries.flatten() {
                if cancelled() {
                    return Vec::new();
                }
                let filename = entry.file_name();
                let Some(name) = filename.to_str() else {
                    continue;
                };
                #[cfg(windows)]
                let name = {
                    let lower = name.to_ascii_lowercase();
                    let Some(extension) =
                        extensions.iter().find(|ext| lower.ends_with(ext.as_str()))
                    else {
                        continue;
                    };
                    lower[..lower.len() - extension.len()].to_string()
                };
                // Suggestions are inserted as a shell word, so skip names that
                // need quoting or could introduce shell operators/controls.
                if !name.is_empty()
                    && name
                        .chars()
                        .all(|c| c.is_alphanumeric() || matches!(c, '-' | '_' | '.' | '+'))
                    && is_executable_file(&entry.path())
                {
                    names.insert(name.to_string());
                }
            }
        }
        names.into_iter().collect()
    }
}

impl SystemCommandSession {
    /// Best-effort hint only: inspect shell builtins and executable paths in
    /// this session without spawning a shell or executing the supplied text.
    pub fn is_likely_command(&self, word: &str) -> bool {
        #[cfg(windows)]
        let builtin = matches!(
            word.to_ascii_lowercase().as_str(),
            "cd" | "chdir"
                | "dir"
                | "echo"
                | "set"
                | "setlocal"
                | "endlocal"
                | "type"
                | "copy"
                | "move"
                | "del"
                | "erase"
                | "ren"
                | "rename"
                | "md"
                | "mkdir"
                | "rd"
                | "rmdir"
                | "pushd"
                | "popd"
                | "if"
                | "for"
                | "call"
                | "start"
                | "ver"
                | "vol"
                | "where"
                | "pause"
        );
        #[cfg(not(windows))]
        let builtin = matches!(
            word,
            "cd" | "pwd"
                | "echo"
                | "printf"
                | "export"
                | "unset"
                | "set"
                | "read"
                | "umask"
                | "ulimit"
                | "test"
                | "["
                | "command"
                | "type"
                | "exec"
                | "eval"
                | "alias"
                | "unalias"
                | "wait"
                | "true"
                | "false"
                | "source"
                | "."
        );
        if builtin {
            return true;
        }
        #[cfg(unix)]
        if let Some((name, _)) = word.split_once('=') {
            if !name.is_empty()
                && name.chars().enumerate().all(|(i, c)| {
                    c == '_' || c.is_ascii_alphabetic() || (i > 0 && c.is_ascii_digit())
                })
            {
                return true;
            }
        }
        let candidate_exists = |path: &Path| {
            #[cfg(windows)]
            {
                let extensions = self
                    .environment_value("PATHEXT")
                    .map(|v| v.to_string_lossy().into_owned())
                    .unwrap_or_else(|| ".COM;.EXE;.BAT;.CMD".into());
                extensions
                    .split(';')
                    .filter(|ext| ext.starts_with('.') && !ext.contains(['/', '\\']))
                    .any(|ext| {
                        if path
                            .to_string_lossy()
                            .to_ascii_lowercase()
                            .ends_with(&ext.to_ascii_lowercase())
                            && is_executable_file(path)
                        {
                            return true;
                        }
                        let mut candidate = path.as_os_str().to_os_string();
                        candidate.push(ext);
                        is_executable_file(Path::new(&candidate))
                    })
            }
            #[cfg(not(windows))]
            {
                is_executable_file(path)
            }
        };
        if word.contains('/') || (cfg!(windows) && word.contains(['\\', ':'])) {
            return candidate_exists(&self.directory.join(word));
        }
        if cfg!(windows) && candidate_exists(&self.directory.join(word)) {
            return true;
        }
        self.environment_value("PATH").is_some_and(|path| {
            std::env::split_paths(path)
                .any(|dir| candidate_exists(&self.directory.join(dir).join(word)))
        })
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
        // Resolve only when executing, never on the prompt/input path. Keep
        // the interpreter for this session even if a command changes SHELL.
        #[cfg(target_os = "linux")]
        let shell = self.shell.get_or_insert_with(|| {
            select_linux_shell(self.environment.get(OsStr::new("SHELL")), || {
                crate::linux::identity::LinuxUserContext::current()
                    .ok()
                    .map(|user| user.shell)
            })
        });
        #[cfg(not(target_os = "linux"))]
        let shell = Path::new("/bin/sh");
        let fish = shell.file_name() == Some(OsStr::new("fish"));
        let mut child = Command::new(&shell);
        // Load login defaults once, rather than resetting PATH after every
        // command. The input is a separate argument, not interpolated into
        // the snapshot protocol. EXIT also captures state after `exit N`.
        if fish && self.initialized {
            child.arg("--no-config");
        }
        child
            .arg(if self.initialized { "-c" } else { "-lc" })
            .arg(if fish { FISH_COMMAND } else { UNIX_COMMAND });
        // POSIX shells consume the next argument as $0; fish starts $argv
        // immediately. Both receive the command as data, without quoting it.
        if !fish {
            child.arg("tundra-system-command");
        }
        child
            .arg(command)
            // Some shells discard inherited OLDPWD at
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

#[cfg(target_os = "linux")]
fn select_linux_shell(
    configured: Option<&OsString>,
    account_shell: impl FnOnce() -> Option<PathBuf>,
) -> PathBuf {
    let usable = |path: &PathBuf| path.is_absolute() && is_executable_file(path);
    configured
        .map(PathBuf::from)
        .filter(usable)
        .or_else(|| account_shell().filter(usable))
        .unwrap_or_else(|| PathBuf::from("/bin/sh"))
}

// Use an absolute env path: `command -p env` itself replaces the child's
// PATH on some shells. cwd has its own stream so filename newlines survive.
// The extra NUL is a completion marker; partial snapshots are never applied.
// zsh's builtin `cd -` reads an internal previous-directory value rather than
// the restored OLDPWD. Use that exported value unless a user cd function exists.
#[cfg(unix)]
const UNIX_COMMAND: &str = r#"trap 'command -p pwd -P >&9 && /usr/bin/env -0 >&8 && command -p printf "\0" >&8' EXIT
export OLDPWD
if [ "$3" = 1 ]; then OLDPWD=$2; fi
if [ -n "${ZSH_VERSION-}" ] && ! typeset -f cd >/dev/null; then
    eval '\cd() {
        if [ "$#" = 1 ] && [ "$1" = - ]; then
            builtin cd -- "$OLDPWD"
        else
            builtin cd "$@"
        fi
    }'
fi
eval "$1"
"#;

#[cfg(unix)]
const FISH_COMMAND: &str = r#"set -g __tundra_previous_pwd "$PWD"
function __tundra_directory_changed --on-variable PWD
    if test "$PWD" != "$__tundra_previous_pwd"
        set -gx OLDPWD "$__tundra_previous_pwd"
        set -g __tundra_previous_pwd "$PWD"
    end
end
function __tundra_snapshot --on-event fish_exit
    builtin pwd -P >&9; and /usr/bin/env -0 >&8; and builtin printf '\0' >&8
end
if test "$argv[3]" = 1
    set -gx OLDPWD "$argv[2]"
    set -g dirprev "$argv[2]"
end
eval "$argv[1]"
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
