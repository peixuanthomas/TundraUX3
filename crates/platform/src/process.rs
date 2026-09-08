use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::PlatformError;

/// One stdout/stderr record, delivered while the child is still running.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessOutput {
    pub stderr: bool,
    pub text: String,
}

/// Resolve the account that invoked sudo without assuming a /home layout.
pub fn sudo_user_home() -> Option<PathBuf> {
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::ffi::OsStrExt;
        if unsafe { libc::geteuid() } != 0 {
            return None;
        }
        let uid = std::env::var("SUDO_UID")
            .ok()?
            .parse::<libc::uid_t>()
            .ok()?;
        let mut entry = std::mem::MaybeUninit::<libc::passwd>::uninit();
        let mut buffer = vec![0u8; 65536];
        let mut result = std::ptr::null_mut();
        let status = unsafe {
            libc::getpwuid_r(
                uid,
                entry.as_mut_ptr(),
                buffer.as_mut_ptr().cast(),
                buffer.len(),
                &mut result,
            )
        };
        if status != 0 || result.is_null() {
            return None;
        }
        let entry = unsafe { entry.assume_init() };
        if entry.pw_dir.is_null() {
            return None;
        }
        let bytes = unsafe { std::ffi::CStr::from_ptr(entry.pw_dir) }.to_bytes();
        let home = PathBuf::from(std::ffi::OsStr::from_bytes(bytes));
        home.is_absolute().then_some(home)
    }
    #[cfg(not(target_os = "linux"))]
    {
        None
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessSpec {
    program: PathBuf,
    args: Vec<String>,
    current_dir: Option<PathBuf>,
    env: BTreeMap<String, String>,
}

impl ProcessSpec {
    pub fn new(program: impl Into<PathBuf>) -> Self {
        Self {
            program: program.into(),
            args: Vec::new(),
            current_dir: None,
            env: BTreeMap::new(),
        }
    }

    pub fn arg(mut self, arg: impl Into<String>) -> Self {
        self.args.push(arg.into());
        self
    }

    pub fn args<I, S>(mut self, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.args.extend(args.into_iter().map(Into::into));
        self
    }

    pub fn current_dir(mut self, current_dir: impl Into<PathBuf>) -> Self {
        self.current_dir = Some(current_dir.into());
        self
    }

    pub fn env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.env.insert(key.into(), value.into());
        self
    }

    pub fn program(&self) -> &Path {
        &self.program
    }

    pub fn args_slice(&self) -> &[String] {
        &self.args
    }

    pub fn current_dir_path(&self) -> Option<&Path> {
        self.current_dir.as_deref()
    }

    pub fn env_map(&self) -> &BTreeMap<String, String> {
        &self.env
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessStream {
    bytes: Vec<u8>,
}

impl ProcessStream {
    pub fn from_bytes(bytes: Vec<u8>) -> Self {
        Self { bytes }
    }

    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub fn utf8_lossy(&self) -> String {
        String::from_utf8_lossy(&self.bytes).into_owned()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessExit {
    pub code: Option<i32>,
    pub stdout: ProcessStream,
    pub stderr: ProcessStream,
}

pub fn validate_process_spec(
    spec: &ProcessSpec,
    reject_windows_scripts: bool,
) -> Result<(), PlatformError> {
    if spec.program.as_os_str().is_empty() {
        return Err(PlatformError::InvalidInput {
            message: "process program must not be empty".to_string(),
        });
    }

    if reject_windows_scripts && is_blocked_windows_script(&spec.program) {
        return Err(PlatformError::ProcessPolicy {
            message: format!(
                "refusing to launch script file through platform process API: {}",
                spec.program.display()
            ),
        });
    }

    Ok(())
}

pub(crate) fn spawn_detached_impl(
    spec: &ProcessSpec,
    reject_windows_scripts: bool,
) -> Result<(), PlatformError> {
    validate_process_spec(spec, reject_windows_scripts)?;
    let mut command = command_from_spec(spec);
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    command.spawn().map_err(|error| PlatformError::Io {
        operation: "spawn detached process",
        path: Some(spec.program.clone()),
        message: error.to_string(),
    })?;

    Ok(())
}

pub(crate) fn spawn_wait_impl(
    spec: &ProcessSpec,
    reject_windows_scripts: bool,
) -> Result<ProcessExit, PlatformError> {
    validate_process_spec(spec, reject_windows_scripts)?;
    let output = command_from_spec(spec)
        .output()
        .map_err(|error| PlatformError::Io {
            operation: "spawn process and wait",
            path: Some(spec.program.clone()),
            message: error.to_string(),
        })?;

    Ok(ProcessExit {
        code: output.status.code(),
        stdout: ProcessStream::from_bytes(output.stdout),
        stderr: ProcessStream::from_bytes(output.stderr),
    })
}

fn command_from_spec(spec: &ProcessSpec) -> Command {
    let mut command = Command::new(&spec.program);
    command.args(&spec.args);

    if let Some(current_dir) = &spec.current_dir {
        command.current_dir(current_dir);
    }

    for (key, value) in &spec.env {
        command.env(key, value);
    }

    command
}

pub(crate) fn spawn_streaming_impl(
    spec: &ProcessSpec,
    reject_windows_scripts: bool,
    report: &mut dyn FnMut(ProcessOutput),
) -> Result<ProcessExit, PlatformError> {
    use std::io::Read;
    validate_process_spec(spec, reject_windows_scripts)?;
    let error = |error: std::io::Error| PlatformError::Io {
        operation: "stream process output",
        path: Some(spec.program.clone()),
        message: error.to_string(),
    };
    let mut child = command_from_spec(spec)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(error)?;
    let stdout = child.stdout.take().expect("piped stdout");
    let stderr = child.stderr.take().expect("piped stderr");
    let (tx, rx) = std::sync::mpsc::sync_channel(128);
    let read_stream =
        |mut stream: Box<dyn Read + Send>,
         is_stderr,
         tx: std::sync::mpsc::SyncSender<Result<ProcessOutput, std::io::Error>>| {
            let mut pending = Vec::new();
            let mut buffer = [0u8; 4096];
            loop {
                match stream.read(&mut buffer) {
                    Ok(0) => break,
                    Ok(count) => {
                        for byte in &buffer[..count] {
                            if matches!(*byte, b'\r' | b'\n') || pending.len() >= 8192 {
                                if !pending.is_empty() {
                                    let event = ProcessOutput {
                                        stderr: is_stderr,
                                        text: String::from_utf8_lossy(&pending).into_owned(),
                                    };
                                    if tx.send(Ok(event)).is_err() {
                                        return;
                                    }
                                    pending.clear();
                                }
                                if matches!(*byte, b'\r' | b'\n') {
                                    continue;
                                }
                            }
                            pending.push(*byte);
                        }
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(error) => {
                        let _ = tx.send(Err(error));
                        break;
                    }
                }
            }
            if !pending.is_empty() {
                let _ = tx.send(Ok(ProcessOutput {
                    stderr: is_stderr,
                    text: String::from_utf8_lossy(&pending).into_owned(),
                }));
            }
        };
    let result = std::thread::scope(|scope| {
        let out_tx = tx.clone();
        let err_tx = tx.clone();
        scope.spawn(move || read_stream(Box::new(stdout), false, out_tx));
        scope.spawn(move || read_stream(Box::new(stderr), true, err_tx));
        drop(tx);
        let mut out = Vec::new();
        let mut err = Vec::new();
        let mut read_error = None;
        for event in rx {
            match event {
                Ok(event) => {
                    let tail = if event.stderr { &mut err } else { &mut out };
                    tail.extend_from_slice(event.text.as_bytes());
                    tail.push(b'\n');
                    if tail.len() > 65536 {
                        tail.drain(..tail.len() - 65536);
                    }
                    report(event);
                }
                Err(error) => {
                    read_error = Some(error);
                    let _ = child.kill();
                }
            }
        }
        (out, err, read_error)
    });
    let status = child.wait().map_err(error)?;
    if let Some(read_error) = result.2 {
        return Err(error(read_error));
    }
    Ok(ProcessExit {
        code: status.code(),
        stdout: ProcessStream::from_bytes(result.0),
        stderr: ProcessStream::from_bytes(result.1),
    })
}

fn is_blocked_windows_script(program: &Path) -> bool {
    program
        .extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| {
            matches!(
                extension.to_ascii_lowercase().as_str(),
                "bat" | "cmd" | "ps1"
            )
        })
        .unwrap_or(false)
}

#[cfg(all(test, unix))]
mod streaming_tests {
    use super::*;

    #[test]
    fn streaming_delivers_stdout_before_exit_and_drains_stderr() {
        let root = std::env::temp_dir().join(format!("tundra-stream-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let ready = root.join("ready");
        let spec = ProcessSpec::new("/bin/sh").args(["-c", "printf 'first\\r'; i=0; while [ ! -f \"$1\" ] && [ $i -lt 100 ]; do sleep 0.02; i=$((i+1)); done; test -f \"$1\" || exit 7; printf 'warning\\n' >&2; printf 'last'; exit 3", "stream-test"]).arg(ready.to_string_lossy());
        let mut events = Vec::new();
        let result = spawn_streaming_impl(&spec, false, &mut |event| {
            if event.text == "first" {
                std::fs::write(&ready, "ready").unwrap();
            }
            events.push(event);
        })
        .unwrap();
        assert_eq!(result.code, Some(3));
        assert!(
            events
                .iter()
                .any(|event| event.stderr && event.text == "warning")
        );
        assert!(
            events
                .iter()
                .any(|event| !event.stderr && event.text == "last")
        );
        assert!(result.stdout.utf8_lossy().contains("last"));
        std::fs::remove_dir_all(root).unwrap();
    }
}
