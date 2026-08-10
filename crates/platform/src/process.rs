use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::PlatformError;

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
