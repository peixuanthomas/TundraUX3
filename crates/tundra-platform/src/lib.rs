#[cfg(not(windows))]
compile_error!("TundraUX3 phase 0 supports Windows 11 only.");

use std::env;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::process;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppPaths {
    config_path: PathBuf,
    data_path: PathBuf,
    cache_path: PathBuf,
}

impl AppPaths {
    pub const CONFIG_TEMPLATE: &'static str = r"%APPDATA%\TundraUX3\config.toml";
    pub const DATA_TEMPLATE: &'static str = r"%LOCALAPPDATA%\TundraUX3\state\";
    pub const CACHE_TEMPLATE: &'static str = r"%LOCALAPPDATA%\TundraUX3\cache\";

    pub fn from_environment() -> Result<Self, PathResolutionError> {
        let appdata = required_env_path("APPDATA")?;
        let localappdata = required_env_path("LOCALAPPDATA")?;

        Self::from_roots(appdata, localappdata)
    }

    pub fn from_roots(
        appdata: impl Into<PathBuf>,
        localappdata: impl Into<PathBuf>,
    ) -> Result<Self, PathResolutionError> {
        let appdata = require_absolute("APPDATA", appdata.into())?;
        let localappdata = require_absolute("LOCALAPPDATA", localappdata.into())?;

        Ok(Self {
            config_path: appdata.join("TundraUX3").join("config.toml"),
            data_path: localappdata.join("TundraUX3").join("state"),
            cache_path: localappdata.join("TundraUX3").join("cache"),
        })
    }

    pub fn config_path(&self) -> &Path {
        &self.config_path
    }

    pub fn data_path(&self) -> &Path {
        &self.data_path
    }

    pub fn cache_path(&self) -> &Path {
        &self.cache_path
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PathResolutionError {
    MissingEnv { name: &'static str },
    EmptyEnv { name: &'static str },
    RelativePath { name: &'static str, value: PathBuf },
}

impl fmt::Display for PathResolutionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingEnv { name } => write!(formatter, "%{name}% is not set"),
            Self::EmptyEnv { name } => write!(formatter, "%{name}% is empty"),
            Self::RelativePath { name, value } => {
                write!(formatter, "%{name}% is not absolute: {}", value.display())
            }
        }
    }
}

impl std::error::Error for PathResolutionError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowsBuildClass {
    UnsupportedWindows,
    Windows11OrNewer,
}

pub fn classify_windows_build(build: u32) -> WindowsBuildClass {
    if build >= 22_000 {
        WindowsBuildClass::Windows11OrNewer
    } else {
        WindowsBuildClass::UnsupportedWindows
    }
}

pub fn current_windows_build() -> Result<u32, String> {
    let mut version: RtlOsVersionInfoW = unsafe { std::mem::zeroed() };
    version.dw_os_version_info_size = std::mem::size_of::<RtlOsVersionInfoW>() as u32;

    let status = unsafe { RtlGetVersion(&mut version) };
    if status >= 0 {
        Ok(version.dw_build_number)
    } else {
        Err(format!("RtlGetVersion failed with NTSTATUS {status}"))
    }
}

pub fn is_windows_terminal_session(wt_session: Option<&str>) -> bool {
    wt_session
        .map(|value| !value.trim().is_empty())
        .unwrap_or(false)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckStatus {
    Pass,
    Warning,
    Fail,
}

impl CheckStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pass => "PASS",
            Self::Warning => "WARN",
            Self::Fail => "FAIL",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnvironmentCheck {
    pub label: String,
    pub status: CheckStatus,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathCheck {
    pub label: String,
    pub path: PathBuf,
    pub status: CheckStatus,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DoctorReport {
    pub app_paths: AppPaths,
    pub environment_checks: Vec<EnvironmentCheck>,
    pub path_checks: Vec<PathCheck>,
}

impl DoctorReport {
    pub fn has_failures(&self) -> bool {
        self.environment_checks
            .iter()
            .any(|check| check.status == CheckStatus::Fail)
            || self
                .path_checks
                .iter()
                .any(|check| check.status == CheckStatus::Fail)
    }
}

pub fn run_doctor() -> Result<DoctorReport, PathResolutionError> {
    let app_paths = AppPaths::from_environment()?;
    let environment_checks = vec![check_windows_version(), check_terminal()];
    let path_checks = vec![
        check_file_parent_read_write("Config parent", app_paths.config_path()),
        check_directory_read_write("Data path", app_paths.data_path()),
        check_directory_read_write("Cache path", app_paths.cache_path()),
    ];

    Ok(DoctorReport {
        app_paths,
        environment_checks,
        path_checks,
    })
}

pub fn check_directory_read_write(label: impl Into<String>, directory: &Path) -> PathCheck {
    let label = label.into();
    let cleanup = CreatedDirectoryCleanup::capture(directory);

    if directory.exists() {
        match fs::metadata(directory) {
            Ok(metadata) if metadata.is_dir() => {}
            Ok(_) => {
                return finish_with_cleanup(
                    failed_path_check(label, directory, "path exists but is not a directory"),
                    cleanup,
                );
            }
            Err(error) => {
                return finish_with_cleanup(
                    failed_path_check(label, directory, format!("cannot read metadata: {error}")),
                    cleanup,
                );
            }
        }
    }

    if let Err(error) = fs::create_dir_all(directory) {
        return finish_with_cleanup(
            failed_path_check(
                label,
                directory,
                format!("cannot create directory: {error}"),
            ),
            cleanup,
        );
    }

    match fs::metadata(directory) {
        Ok(metadata) if metadata.is_dir() => {}
        Ok(_) => {
            return finish_with_cleanup(
                failed_path_check(label, directory, "path exists but is not a directory"),
                cleanup,
            );
        }
        Err(error) => {
            return finish_with_cleanup(
                failed_path_check(label, directory, format!("cannot read metadata: {error}")),
                cleanup,
            );
        }
    }

    let probe_path = directory.join(format!(
        ".tundraux3-doctor-probe-{}-{}.tmp",
        process::id(),
        timestamp_nanos()
    ));

    if let Err(error) = fs::write(&probe_path, b"probe") {
        return finish_with_cleanup(
            failed_path_check(
                label,
                directory,
                format!("cannot write probe file: {error}"),
            ),
            cleanup,
        );
    }

    match fs::read(&probe_path) {
        Ok(bytes) if bytes == b"probe" => {}
        Ok(_) => {
            let _ = fs::remove_file(&probe_path);
            return finish_with_cleanup(
                failed_path_check(label, directory, "probe file content changed"),
                cleanup,
            );
        }
        Err(error) => {
            let _ = fs::remove_file(&probe_path);
            return finish_with_cleanup(
                failed_path_check(label, directory, format!("cannot read probe file: {error}")),
                cleanup,
            );
        }
    }

    if let Err(error) = fs::remove_file(&probe_path) {
        return finish_with_cleanup(
            failed_path_check(
                label,
                directory,
                format!("cannot remove probe file: {error}"),
            ),
            cleanup,
        );
    }

    let message = if cleanup.will_remove_directories() {
        format!("{} can be created, read, and written", directory.display())
    } else {
        format!("{} is readable and writable", directory.display())
    };

    let mut check = PathCheck {
        label,
        path: directory.to_path_buf(),
        status: CheckStatus::Pass,
        message,
    };

    if let Err(error) = cleanup.remove_created_directories() {
        check.status = CheckStatus::Warning;
        check.message = format!(
            "{}; cleanup warning: could not remove temporary directory {error}",
            check.message
        );
    }

    check
}

fn check_file_parent_read_write(label: impl Into<String>, file_path: &Path) -> PathCheck {
    match file_path.parent() {
        Some(parent) => check_directory_read_write(label, parent),
        None => failed_path_check(label.into(), file_path, "file path has no parent directory"),
    }
}

fn check_windows_version() -> EnvironmentCheck {
    match current_windows_build() {
        Ok(build) if classify_windows_build(build) == WindowsBuildClass::Windows11OrNewer => {
            EnvironmentCheck {
                label: "Platform".to_string(),
                status: CheckStatus::Pass,
                message: format!("Windows build {build} meets Windows 11 requirement"),
            }
        }
        Ok(build) => EnvironmentCheck {
            label: "Platform".to_string(),
            status: CheckStatus::Fail,
            message: format!("Windows build {build} is below Windows 11 build 22000"),
        },
        Err(error) => EnvironmentCheck {
            label: "Platform".to_string(),
            status: CheckStatus::Fail,
            message: error,
        },
    }
}

fn check_terminal() -> EnvironmentCheck {
    if is_windows_terminal_session(env::var("WT_SESSION").ok().as_deref()) {
        EnvironmentCheck {
            label: "Terminal".to_string(),
            status: CheckStatus::Pass,
            message: "Windows Terminal detected".to_string(),
        }
    } else {
        EnvironmentCheck {
            label: "Terminal".to_string(),
            status: CheckStatus::Warning,
            message: "Windows Terminal not detected; conhost is best-effort only".to_string(),
        }
    }
}

fn required_env_path(name: &'static str) -> Result<PathBuf, PathResolutionError> {
    match env::var_os(name) {
        Some(value) if value.as_os_str().is_empty() => Err(PathResolutionError::EmptyEnv { name }),
        Some(value) => require_absolute(name, PathBuf::from(value)),
        None => Err(PathResolutionError::MissingEnv { name }),
    }
}

fn require_absolute(name: &'static str, path: PathBuf) -> Result<PathBuf, PathResolutionError> {
    if path.is_absolute() {
        Ok(path)
    } else {
        Err(PathResolutionError::RelativePath { name, value: path })
    }
}

fn failed_path_check(label: String, path: &Path, message: impl Into<String>) -> PathCheck {
    PathCheck {
        label,
        path: path.to_path_buf(),
        status: CheckStatus::Fail,
        message: message.into(),
    }
}

fn finish_with_cleanup(mut check: PathCheck, cleanup: CreatedDirectoryCleanup) -> PathCheck {
    if let Err(error) = cleanup.remove_created_directories() {
        check.message = format!(
            "{}; cleanup warning: could not remove temporary directory {error}",
            check.message
        );
    }

    check
}

#[derive(Debug)]
struct CreatedDirectoryCleanup {
    directories_to_remove: Vec<PathBuf>,
}

impl CreatedDirectoryCleanup {
    fn capture(directory: &Path) -> Self {
        let mut directories_to_remove = Vec::new();
        let mut cursor = Some(directory);

        while let Some(path) = cursor {
            if path.exists() {
                break;
            }

            directories_to_remove.push(path.to_path_buf());
            cursor = path.parent();
        }

        Self {
            directories_to_remove,
        }
    }

    fn will_remove_directories(&self) -> bool {
        !self.directories_to_remove.is_empty()
    }

    fn remove_created_directories(self) -> Result<(), String> {
        for directory in self.directories_to_remove {
            if directory.exists() {
                fs::remove_dir(&directory)
                    .map_err(|error| format!("{}: {error}", directory.display()))?;
            }
        }

        Ok(())
    }
}

fn timestamp_nanos() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0)
}

#[repr(C)]
struct RtlOsVersionInfoW {
    dw_os_version_info_size: u32,
    dw_major_version: u32,
    dw_minor_version: u32,
    dw_build_number: u32,
    dw_platform_id: u32,
    sz_csd_version: [u16; 128],
}

#[link(name = "ntdll")]
unsafe extern "system" {
    fn RtlGetVersion(version_information: *mut RtlOsVersionInfoW) -> i32;
}
