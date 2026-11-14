//! Holds the PTY session leader before it can execute the requested program.
use crate::shell_control::{LocalConnection, LocalListener};
use portable_pty::CommandBuilder;
use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::io::{self, Read, Write};
use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt;
use std::path::{Component, Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

const SHIM: &str = "__tundra-pty-exec";
const TIMEOUT: Duration = Duration::from_secs(5);
const INTERVAL: Duration = Duration::from_millis(5);
const MAX_ENVIRONMENT_BYTES: usize = 16 * 1024 * 1024;
static GATE_DIRECTORY: OnceLock<PathBuf> = OnceLock::new();

/// Sets the guardian-owned directory that is removed at the end of the run.
pub fn set_pty_exec_gate_directory(path: PathBuf) -> io::Result<()> {
    GATE_DIRECTORY
        .set(path)
        .map_err(|_| io::ErrorKind::AlreadyExists.into())
}

pub struct Gate {
    endpoint: String,
    listener: LocalListener,
    _directory: tempfile::TempDir,
    environment: Vec<u8>,
}

impl Gate {
    /// Replaces the target executable with the current executable's private PTY entry.
    pub fn wrap(
        command: &mut CommandBuilder,
        environment: BTreeMap<OsString, OsString>,
    ) -> io::Result<Self> {
        let directory = GATE_DIRECTORY
            .get()
            .ok_or_else(|| io::Error::other("managed PTY gate directory is unavailable"))?;
        Self::wrap_in(directory, command, environment)
    }

    fn wrap_in(
        directory: &Path,
        command: &mut CommandBuilder,
        mut environment: BTreeMap<OsString, OsString>,
    ) -> io::Result<Self> {
        let directory = tempfile::Builder::new().prefix("p").tempdir_in(directory)?;
        let endpoint = directory
            .path()
            .join("s")
            .into_os_string()
            .into_string()
            .map_err(|_| io::ErrorKind::InvalidInput)?;
        let listener = LocalListener::bind(&endpoint)?;
        environment
            .entry(OsString::from("SHELL"))
            .or_insert_with(|| command.get_shell().into());
        let environment = serde_json::to_vec(&environment.into_iter().collect::<Vec<_>>())
            .map_err(io::Error::other)?;
        if environment.len() > MAX_ENVIRONMENT_BYTES {
            return Err(io::ErrorKind::InvalidInput.into());
        }
        let mut argv = vec![
            std::env::current_exe()?.into_os_string(),
            SHIM.into(),
            endpoint.clone().into(),
            std::process::id().to_string().into(),
        ];
        argv.append(command.get_argv_mut());
        *command.get_argv_mut() = argv;
        let home = command.get_env("HOME").map(OsStr::to_os_string);
        // Pass the target environment over the gate so it cannot affect shim startup.
        command.env_clear();
        if let Some(home) = home {
            command.env("HOME", home);
        }
        Ok(Self {
            endpoint,
            listener,
            _directory: directory,
            environment,
        })
    }

    /// Releases the session leader after the guardian has registered its identity.
    pub fn release(self, child_pid: u32) -> io::Result<()> {
        let stream = self.accept(child_pid)?;
        self.start_target(stream)
    }

    fn start_target(self, mut stream: UnixStream) -> io::Result<()> {
        std::fs::remove_file(&self.endpoint)?;
        stream.write_all(&(self.environment.len() as u32).to_be_bytes())?;
        stream.write_all(&self.environment)?;
        let mut ready = [0];
        stream.read_exact(&mut ready)?;
        let mut error = [0; 4];
        if ready == [0] {
            stream.read_exact(&mut error)?;
        } else {
            // Successful exec closes the authenticated socket via close-on-exec.
            if stream.read(&mut error[..1])? == 0 {
                return Ok(());
            }
            stream.read_exact(&mut error[1..])?;
        }
        Err(io::Error::from_raw_os_error(i32::from_be_bytes(error)))
    }

    fn accept(&self, child_pid: u32) -> io::Result<UnixStream> {
        let deadline = Instant::now() + TIMEOUT;
        loop {
            match self.listener.accept() {
                Ok(connection) if connection.peer_pid()? == child_pid => {
                    return blocking(connection);
                }
                Ok(_) => {}
                Err(error)
                    if matches!(
                        error.kind(),
                        io::ErrorKind::WouldBlock | io::ErrorKind::PermissionDenied
                    ) => {}
                Err(error) => return Err(error),
            }
            if Instant::now() >= deadline {
                return Err(io::ErrorKind::TimedOut.into());
            }
            std::thread::sleep(INTERVAL);
        }
    }
}

/// Dispatches the private PTY command before the application entry point.
pub fn run_pty_exec_shim() -> Option<io::Error> {
    let mut args = std::env::args_os().skip(1);
    if args.next().as_deref() != Some(OsStr::new(SHIM)) {
        return None;
    }
    match run_shim(args) {
        Err(error) => Some(error),
        Ok(never) => match never {},
    }
}

fn run_shim(mut args: impl Iterator<Item = OsString>) -> io::Result<std::convert::Infallible> {
    let parent = unsafe { libc::getppid() } as u32;
    let (Some(endpoint), Some(claimed_parent), Some(program)) =
        (args.next(), args.next(), args.next())
    else {
        return Err(io::ErrorKind::InvalidInput.into());
    };
    if claimed_parent.to_str().and_then(|value| value.parse().ok()) != Some(parent)
        || parent <= 1
        || unsafe { libc::getsid(0) } != std::process::id() as libc::pid_t
    {
        return Err(io::ErrorKind::PermissionDenied.into());
    }
    let endpoint = endpoint.to_str().ok_or(io::ErrorKind::InvalidInput)?;
    let mut stream = blocking(LocalConnection::connect(endpoint, parent)?)?;
    let command = target_command(&mut stream, program, args);
    stream.write_all(&[u8::from(command.is_ok())])?;
    let error = match command {
        Ok(mut command) => command.exec(),
        Err(error) => error,
    };
    let _ = stream.write_all(&error.raw_os_error().unwrap_or(libc::EIO).to_be_bytes());
    Err(error)
}

fn blocking(connection: LocalConnection) -> io::Result<UnixStream> {
    let stream = connection.into_unix_stream();
    stream.set_nonblocking(false)?;
    stream.set_read_timeout(Some(TIMEOUT))?;
    stream.set_write_timeout(Some(TIMEOUT))?;
    Ok(stream)
}

fn target_command(
    stream: &mut UnixStream,
    program: OsString,
    args: impl Iterator<Item = OsString>,
) -> io::Result<Command> {
    let environment = read_environment(stream)?;
    let executable = resolve_executable(&program, &environment)?;
    let mut command = Command::new(executable);
    command
        .arg0(program)
        .args(args)
        .env_clear()
        .envs(environment);
    Ok(command)
}

fn read_environment(stream: &mut UnixStream) -> io::Result<BTreeMap<OsString, OsString>> {
    let mut size = [0; 4];
    stream.read_exact(&mut size)?;
    let size = u32::from_be_bytes(size) as usize;
    if size > MAX_ENVIRONMENT_BYTES {
        return Err(io::ErrorKind::InvalidData.into());
    }
    let mut environment = vec![0; size];
    stream.read_exact(&mut environment)?;
    let entries: Vec<(OsString, OsString)> =
        serde_json::from_slice(&environment).map_err(io::Error::other)?;
    Ok(entries.into_iter().collect())
}

fn resolve_executable(
    program: &OsStr,
    environment: &BTreeMap<OsString, OsString>,
) -> io::Result<PathBuf> {
    // Match portable-pty's relative-path and PATH lookup from the configured cwd.
    let program = Path::new(program);
    if program.is_absolute()
        || matches!(
            program.components().next(),
            Some(Component::CurDir | Component::ParentDir)
        )
    {
        return std::path::absolute(program);
    }
    let cwd = std::env::current_dir()?;
    let path = environment
        .get(OsStr::new("PATH"))
        .ok_or(io::ErrorKind::NotFound)?;
    for directory in std::env::split_paths(path) {
        let candidate = cwd.join(directory).join(program);
        if candidate.is_file() {
            use std::os::unix::ffi::OsStrExt;
            let name = std::ffi::CString::new(candidate.as_os_str().as_bytes())?;
            if unsafe { libc::access(name.as_ptr(), libc::X_OK) } == 0 {
                return Ok(candidate);
            }
        }
    }
    Err(io::ErrorKind::NotFound.into())
}

#[cfg(test)]
#[path = "../tests/unit/process/pty_exec_tests.rs"]
mod tests;
