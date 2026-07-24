use std::ffi::OsString;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::SystemTime;

use crate::paths::home_dir_from_env;
use crate::{
    AppPaths, CapabilityStatus, Platform, PlatformCapabilities, PlatformError, PlatformKind,
    ProcessExit, ProcessSpec, UserDirs, build_linux_app_paths,
};

const XDG_OPEN: &str = "xdg-open";

#[derive(Debug, Clone, Copy, Default)]
pub struct LinuxPlatform;

impl Platform for LinuxPlatform {
    fn kind(&self) -> PlatformKind {
        PlatformKind::Linux
    }

    fn capabilities(&self) -> PlatformCapabilities {
        let mut capabilities = PlatformCapabilities::unsupported();
        capabilities.open_path = CapabilityStatus::BestEffort;
        capabilities.open_with = CapabilityStatus::BestEffort;
        capabilities.open_uri = CapabilityStatus::BestEffort;
        capabilities.spawn_detached = CapabilityStatus::Supported;
        capabilities.spawn_wait = CapabilityStatus::Supported;
        capabilities.user_dirs = CapabilityStatus::Supported;
        capabilities.app_paths = CapabilityStatus::Supported;
        capabilities.temp = CapabilityStatus::Supported;
        capabilities.file_attributes = CapabilityStatus::Supported;
        capabilities.directory_listing = CapabilityStatus::Supported;
        capabilities
    }

    fn is_native_backend(&self) -> bool {
        true
    }

    fn user_dirs(&self) -> Result<UserDirs, PlatformError> {
        let home = home_dir_from_env()?;
        let base_dirs = XdgBaseDirs::from_environment(&home);

        UserDirs::new(
            home.join("Desktop"),
            home.join("Documents"),
            home.join("Downloads"),
            home.join("Pictures"),
            home.join("Videos"),
            home.join("Music"),
            base_dirs.data,
        )
        .map_err(Into::into)
    }

    fn app_paths(&self) -> Result<AppPaths, PlatformError> {
        let home = home_dir_from_env()?;
        let base_dirs = XdgBaseDirs::from_environment(&home);
        build_linux_app_paths(
            base_dirs.config,
            base_dirs.data,
            base_dirs.cache,
            base_dirs.state,
            std::env::temp_dir(),
        )
        .map_err(Into::into)
    }

    fn system_time(&self) -> Result<SystemTime, PlatformError> {
        Ok(SystemTime::now())
    }

    fn open_path(&self, path: &Path) -> Result<(), PlatformError> {
        run_xdg_open(path.as_os_str().to_os_string())
    }

    fn open_with(&self, path: &Path, application: &Path) -> Result<(), PlatformError> {
        if application.as_os_str().is_empty() {
            return Err(PlatformError::InvalidInput {
                message: "application path must not be empty".to_string(),
            });
        }

        Command::new(application)
            .arg(path)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map(|_| ())
            .map_err(|error| PlatformError::Io {
                operation: "open path with Linux application",
                path: Some(application.to_path_buf()),
                message: error.to_string(),
            })
    }

    fn open_uri(&self, uri: &str) -> Result<(), PlatformError> {
        if uri.trim().is_empty() {
            return Err(PlatformError::InvalidInput {
                message: "URI must not be empty".to_string(),
            });
        }

        run_xdg_open(OsString::from(uri))
    }

    fn spawn_detached(&self, spec: &ProcessSpec) -> Result<(), PlatformError> {
        crate::process::spawn_detached_impl(spec, false)
    }

    fn spawn_wait(&self, spec: &ProcessSpec) -> Result<ProcessExit, PlatformError> {
        crate::process::spawn_wait_impl(spec, false)
    }

    fn read_clipboard_text(&self) -> Result<String, PlatformError> {
        Err(PlatformError::Unsupported {
            capability: "clipboard_text",
        })
    }

    fn write_clipboard_text(&self, _text: &str) -> Result<(), PlatformError> {
        Err(PlatformError::Unsupported {
            capability: "clipboard_text",
        })
    }

    fn is_process_alive(&self, pid: u32) -> Result<bool, PlatformError> {
        if pid == 0 {
            return Ok(false);
        }

        let process_path = PathBuf::from("/proc").join(pid.to_string());
        match fs::metadata(&process_path) {
            Ok(_) => Ok(true),
            Err(error) if error.kind() == ErrorKind::PermissionDenied => Ok(true),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(false),
            Err(error) => Err(PlatformError::Io {
                operation: "check Linux process liveness",
                path: Some(process_path),
                message: error.to_string(),
            }),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct XdgBaseDirs {
    config: PathBuf,
    data: PathBuf,
    cache: PathBuf,
    state: PathBuf,
}

impl XdgBaseDirs {
    fn from_environment(home: &Path) -> Self {
        Self::resolve(
            home,
            std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from),
            std::env::var_os("XDG_DATA_HOME").map(PathBuf::from),
            std::env::var_os("XDG_CACHE_HOME").map(PathBuf::from),
            std::env::var_os("XDG_STATE_HOME").map(PathBuf::from),
        )
    }

    fn resolve(
        home: &Path,
        config: Option<PathBuf>,
        data: Option<PathBuf>,
        cache: Option<PathBuf>,
        state: Option<PathBuf>,
    ) -> Self {
        Self {
            config: absolute_or(config, || home.join(".config")),
            data: absolute_or(data, || home.join(".local").join("share")),
            cache: absolute_or(cache, || home.join(".cache")),
            state: absolute_or(state, || home.join(".local").join("state")),
        }
    }
}

fn absolute_or(candidate: Option<PathBuf>, fallback: impl FnOnce() -> PathBuf) -> PathBuf {
    candidate
        .filter(|path| !path.as_os_str().is_empty() && path.is_absolute())
        .unwrap_or_else(fallback)
}

fn run_xdg_open(target: OsString) -> Result<(), PlatformError> {
    let mut command = Command::new(XDG_OPEN);
    command
        .arg(target)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    run_open_command(&mut command)
}

fn run_open_command(command: &mut Command) -> Result<(), PlatformError> {
    let program = command.get_program().to_string_lossy().into_owned();
    let program_path = PathBuf::from(command.get_program());
    let output = command.output().map_err(|error| PlatformError::Io {
        operation: "open with xdg-open",
        path: Some(program_path),
        message: error.to_string(),
    })?;

    if output.status.success() {
        return Ok(());
    }

    Err(PlatformError::CommandFailed {
        program,
        status: output.status.code(),
        stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::{LinuxPlatform, XdgBaseDirs, run_open_command};
    use crate::{Platform, PlatformError};
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use std::time::{Duration, Instant};

    #[test]
    fn xdg_base_dirs_use_absolute_overrides() {
        let home = Path::new("/home/tundra");
        let dirs = XdgBaseDirs::resolve(
            home,
            Some(PathBuf::from("/config")),
            Some(PathBuf::from("/data")),
            Some(PathBuf::from("/cache")),
            Some(PathBuf::from("/state")),
        );

        assert_eq!(dirs.config, Path::new("/config"));
        assert_eq!(dirs.data, Path::new("/data"));
        assert_eq!(dirs.cache, Path::new("/cache"));
        assert_eq!(dirs.state, Path::new("/state"));
    }

    #[test]
    fn xdg_base_dirs_fall_back_for_missing_empty_or_relative_values() {
        let home = Path::new("/home/tundra");
        let dirs = XdgBaseDirs::resolve(
            home,
            None,
            Some(PathBuf::new()),
            Some(PathBuf::from("relative-cache")),
            None,
        );

        assert_eq!(dirs.config, home.join(".config"));
        assert_eq!(dirs.data, home.join(".local").join("share"));
        assert_eq!(dirs.cache, home.join(".cache"));
        assert_eq!(dirs.state, home.join(".local").join("state"));
    }

    #[test]
    fn xdg_open_command_reports_missing_program_with_operation_context() {
        let missing = Path::new("/tundraux3-test-missing-xdg-open");
        let mut command = Command::new(missing);

        let error = run_open_command(&mut command).expect_err("missing opener should fail");

        assert!(matches!(
            error,
            PlatformError::Io {
                operation: "open with xdg-open",
                path: Some(path),
                ..
            } if path == missing
        ));
    }

    #[test]
    fn xdg_open_command_preserves_nonzero_status_and_stderr() {
        let mut command = Command::new("/bin/sh");
        command.args(["-c", "printf 'xdg-open fixture failed' >&2; exit 9"]);

        let error = run_open_command(&mut command).expect_err("nonzero opener should fail");

        assert!(matches!(
            error,
            PlatformError::CommandFailed {
                status: Some(9),
                ref stderr,
                ..
            } if stderr == "xdg-open fixture failed"
        ));
    }

    #[test]
    fn open_with_spawns_without_waiting_and_reports_application_errors() {
        let started = Instant::now();
        LinuxPlatform
            .open_with(Path::new("1"), Path::new("/bin/sleep"))
            .expect("Linux open_with should spawn the selected application");
        assert!(started.elapsed() < Duration::from_secs(1));

        let missing = Path::new("/tundraux3-test-missing-viewer");
        let error = LinuxPlatform
            .open_with(Path::new("/tmp/document.txt"), missing)
            .expect_err("missing application should fail");
        assert!(matches!(
            error,
            PlatformError::Io {
                operation: "open path with Linux application",
                path: Some(path),
                ..
            } if path == missing
        ));

        assert!(matches!(
            LinuxPlatform.open_uri("  "),
            Err(PlatformError::InvalidInput { .. })
        ));
    }
}
