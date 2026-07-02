use std::ffi::{OsStr, OsString};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::paths::home_dir_from_env;
use crate::{
    AppPaths, Platform, PlatformCapabilities, PlatformError, PlatformKind, ProcessExit,
    ProcessSpec, UserDirs, build_macos_app_paths,
};

const OPEN: &str = "/usr/bin/open";
const PBCOPY: &str = "/usr/bin/pbcopy";
const PBPASTE: &str = "/usr/bin/pbpaste";

#[derive(Debug, Clone, Copy, Default)]
pub struct MacosPlatform;

impl Platform for MacosPlatform {
    fn kind(&self) -> PlatformKind {
        PlatformKind::Macos
    }

    fn capabilities(&self) -> PlatformCapabilities {
        PlatformCapabilities::native_supported()
    }

    fn user_dirs(&self) -> Result<UserDirs, PlatformError> {
        let home = home_dir_from_env()?;
        UserDirs::new(
            home.join("Desktop"),
            home.join("Documents"),
            home.join("Downloads"),
            home.join("Pictures"),
            home.join("Movies"),
            home.join("Music"),
            home.join("Library").join("Application Support"),
        )
        .map_err(Into::into)
    }

    fn app_paths(&self) -> Result<AppPaths, PlatformError> {
        build_macos_app_paths(home_dir_from_env()?, std::env::temp_dir()).map_err(Into::into)
    }

    fn open_path(&self, path: &Path) -> Result<(), PlatformError> {
        run_open([OsString::from("--"), path.as_os_str().to_os_string()])
    }

    fn open_with(&self, path: &Path, application: &Path) -> Result<(), PlatformError> {
        run_open([
            OsString::from("-a"),
            application.as_os_str().to_os_string(),
            OsString::from("--"),
            path.as_os_str().to_os_string(),
        ])
    }

    fn open_uri(&self, uri: &str) -> Result<(), PlatformError> {
        if uri.trim().is_empty() {
            return Err(PlatformError::InvalidInput {
                message: "URI must not be empty".to_string(),
            });
        }

        run_open([OsString::from(uri)])
    }

    fn spawn_detached(&self, spec: &ProcessSpec) -> Result<(), PlatformError> {
        crate::process::spawn_detached_impl(spec, false)
    }

    fn spawn_wait(&self, spec: &ProcessSpec) -> Result<ProcessExit, PlatformError> {
        crate::process::spawn_wait_impl(spec, false)
    }

    fn read_clipboard_text(&self) -> Result<String, PlatformError> {
        let output = Command::new(PBPASTE)
            .output()
            .map_err(|error| PlatformError::Io {
                operation: "read clipboard",
                path: Some(PathBuf::from(PBPASTE)),
                message: error.to_string(),
            })?;

        if !output.status.success() {
            return Err(PlatformError::CommandFailed {
                program: PBPASTE.to_string(),
                status: output.status.code(),
                stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
            });
        }

        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    }

    fn write_clipboard_text(&self, text: &str) -> Result<(), PlatformError> {
        let mut child = Command::new(PBCOPY)
            .stdin(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| PlatformError::Io {
                operation: "write clipboard",
                path: Some(PathBuf::from(PBCOPY)),
                message: error.to_string(),
            })?;

        child
            .stdin
            .take()
            .ok_or_else(|| PlatformError::Native {
                operation: "write clipboard",
                message: "pbcopy stdin is unavailable".to_string(),
            })?
            .write_all(text.as_bytes())
            .map_err(|error| PlatformError::Io {
                operation: "write clipboard",
                path: Some(PathBuf::from(PBCOPY)),
                message: error.to_string(),
            })?;

        let output = child
            .wait_with_output()
            .map_err(|error| PlatformError::Io {
                operation: "write clipboard",
                path: Some(PathBuf::from(PBCOPY)),
                message: error.to_string(),
            })?;

        if output.status.success() {
            Ok(())
        } else {
            Err(PlatformError::CommandFailed {
                program: PBCOPY.to_string(),
                status: output.status.code(),
                stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
            })
        }
    }
}

fn run_open<I, S>(args: I) -> Result<(), PlatformError>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let output = Command::new(OPEN)
        .args(args)
        .output()
        .map_err(|error| PlatformError::Io {
            operation: "open with macOS open",
            path: Some(PathBuf::from(OPEN)),
            message: error.to_string(),
        })?;

    if output.status.success() {
        Ok(())
    } else {
        Err(PlatformError::CommandFailed {
            program: OPEN.to_string(),
            status: output.status.code(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        })
    }
}
