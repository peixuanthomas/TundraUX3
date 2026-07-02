use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process;

use crate::paths::timestamp_nanos;
use crate::{AppPaths, Platform, PlatformError, PlatformKind};

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
    pub platform_kind: PlatformKind,
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

pub fn run_doctor() -> Result<DoctorReport, PlatformError> {
    let platform = crate::native_platform();
    run_doctor_with(platform.as_ref())
}

pub fn run_doctor_with(platform: &dyn Platform) -> Result<DoctorReport, PlatformError> {
    let app_paths = platform.app_paths()?;
    let mut environment_checks = Vec::new();
    environment_checks.push(platform_check(platform));
    environment_checks.push(terminal_check(platform.kind()));
    environment_checks.extend(capability_checks(platform));

    let path_checks = vec![
        check_file_parent_read_write("Config parent", app_paths.config_path()),
        check_directory_read_write("Data path", app_paths.data_path()),
        check_directory_read_write("Cache path", app_paths.cache_path()),
        check_directory_read_write("Logs path", app_paths.logs_path()),
        check_directory_read_write("Temp path", app_paths.temp_path()),
    ];

    Ok(DoctorReport {
        platform_kind: platform.kind(),
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

fn platform_check(platform: &dyn Platform) -> EnvironmentCheck {
    match platform.kind() {
        PlatformKind::Windows => windows_platform_check(),
        PlatformKind::Macos => EnvironmentCheck {
            label: "Platform".to_string(),
            status: CheckStatus::Pass,
            message: "macOS platform supported".to_string(),
        },
        PlatformKind::Unsupported => EnvironmentCheck {
            label: "Platform".to_string(),
            status: CheckStatus::Fail,
            message: "unsupported platform".to_string(),
        },
    }
}

#[cfg(windows)]
fn windows_platform_check() -> EnvironmentCheck {
    match crate::windows::current_windows_build() {
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

#[cfg(not(windows))]
fn windows_platform_check() -> EnvironmentCheck {
    EnvironmentCheck {
        label: "Platform".to_string(),
        status: CheckStatus::Fail,
        message: "Windows platform check is unavailable on this build".to_string(),
    }
}

fn terminal_check(kind: PlatformKind) -> EnvironmentCheck {
    match kind {
        PlatformKind::Windows => {
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
                    message: "Windows Terminal not detected; conhost is best-effort only"
                        .to_string(),
                }
            }
        }
        PlatformKind::Macos => EnvironmentCheck {
            label: "Terminal".to_string(),
            status: CheckStatus::Pass,
            message: "macOS terminal session supported".to_string(),
        },
        PlatformKind::Unsupported => EnvironmentCheck {
            label: "Terminal".to_string(),
            status: CheckStatus::Warning,
            message: "terminal support is unsupported on this platform".to_string(),
        },
    }
}

fn capability_checks(platform: &dyn Platform) -> Vec<EnvironmentCheck> {
    platform
        .capabilities()
        .checks()
        .into_iter()
        .map(|(name, status)| EnvironmentCheck {
            label: format!("Capability: {name}"),
            status: match status {
                crate::CapabilityStatus::Supported => CheckStatus::Pass,
                crate::CapabilityStatus::BestEffort => CheckStatus::Warning,
                crate::CapabilityStatus::Unsupported => CheckStatus::Warning,
            },
            message: status.as_str().to_string(),
        })
        .collect()
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
