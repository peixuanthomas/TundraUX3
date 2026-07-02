use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::{
    AppPaths, PathResolutionError, ProcessExit, ProcessSpec, UserDirs, cleanup_temp_path,
    create_temp_dir, create_temp_file,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlatformKind {
    Windows,
    Macos,
    Unsupported,
}

impl PlatformKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Windows => "Windows",
            Self::Macos => "macOS",
            Self::Unsupported => "Unsupported",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CapabilityStatus {
    Supported,
    BestEffort,
    Unsupported,
}

impl CapabilityStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Supported => "supported",
            Self::BestEffort => "best-effort",
            Self::Unsupported => "unsupported",
        }
    }

    pub fn is_failure(self) -> bool {
        matches!(self, Self::Unsupported)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlatformCapabilities {
    pub open_path: CapabilityStatus,
    pub open_with: CapabilityStatus,
    pub open_uri: CapabilityStatus,
    pub spawn_detached: CapabilityStatus,
    pub spawn_wait: CapabilityStatus,
    pub clipboard_text: CapabilityStatus,
    pub user_dirs: CapabilityStatus,
    pub app_paths: CapabilityStatus,
    pub temp: CapabilityStatus,
    pub file_attributes: CapabilityStatus,
    pub notifications: CapabilityStatus,
    pub default_apps: CapabilityStatus,
    pub power: CapabilityStatus,
}

impl PlatformCapabilities {
    pub fn native_supported() -> Self {
        Self {
            open_path: CapabilityStatus::Supported,
            open_with: CapabilityStatus::Supported,
            open_uri: CapabilityStatus::Supported,
            spawn_detached: CapabilityStatus::Supported,
            spawn_wait: CapabilityStatus::Supported,
            clipboard_text: CapabilityStatus::Supported,
            user_dirs: CapabilityStatus::Supported,
            app_paths: CapabilityStatus::Supported,
            temp: CapabilityStatus::Supported,
            file_attributes: CapabilityStatus::Supported,
            notifications: CapabilityStatus::Unsupported,
            default_apps: CapabilityStatus::Unsupported,
            power: CapabilityStatus::Unsupported,
        }
    }

    pub fn unsupported() -> Self {
        Self {
            open_path: CapabilityStatus::Unsupported,
            open_with: CapabilityStatus::Unsupported,
            open_uri: CapabilityStatus::Unsupported,
            spawn_detached: CapabilityStatus::Unsupported,
            spawn_wait: CapabilityStatus::Unsupported,
            clipboard_text: CapabilityStatus::Unsupported,
            user_dirs: CapabilityStatus::Unsupported,
            app_paths: CapabilityStatus::Unsupported,
            temp: CapabilityStatus::Unsupported,
            file_attributes: CapabilityStatus::Unsupported,
            notifications: CapabilityStatus::Unsupported,
            default_apps: CapabilityStatus::Unsupported,
            power: CapabilityStatus::Unsupported,
        }
    }

    pub fn checks(&self) -> [(&'static str, CapabilityStatus); 13] {
        [
            ("open_path", self.open_path),
            ("open_with", self.open_with),
            ("open_uri", self.open_uri),
            ("spawn_detached", self.spawn_detached),
            ("spawn_wait", self.spawn_wait),
            ("clipboard_text", self.clipboard_text),
            ("user_dirs", self.user_dirs),
            ("app_paths", self.app_paths),
            ("temp", self.temp),
            ("file_attributes", self.file_attributes),
            ("notifications", self.notifications),
            ("default_apps", self.default_apps),
            ("power", self.power),
        ]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PowerAction {
    Lock,
    Sleep,
    Shutdown,
    Restart,
}

impl PowerAction {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Lock => "lock",
            Self::Sleep => "sleep",
            Self::Shutdown => "shutdown",
            Self::Restart => "restart",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileAttributes {
    pub path: PathBuf,
    pub is_file: bool,
    pub is_dir: bool,
    pub len: u64,
    pub readonly: bool,
    pub modified: Option<SystemTime>,
}

pub trait Platform: Send + Sync {
    fn kind(&self) -> PlatformKind;
    fn capabilities(&self) -> PlatformCapabilities;
    fn user_dirs(&self) -> Result<UserDirs, PlatformError>;
    fn app_paths(&self) -> Result<AppPaths, PlatformError>;
    fn open_path(&self, path: &Path) -> Result<(), PlatformError>;
    fn open_with(&self, path: &Path, application: &Path) -> Result<(), PlatformError>;
    fn open_uri(&self, uri: &str) -> Result<(), PlatformError>;
    fn spawn_detached(&self, spec: &ProcessSpec) -> Result<(), PlatformError>;
    fn spawn_wait(&self, spec: &ProcessSpec) -> Result<ProcessExit, PlatformError>;
    fn read_clipboard_text(&self) -> Result<String, PlatformError>;
    fn write_clipboard_text(&self, text: &str) -> Result<(), PlatformError>;

    fn file_attributes(&self, path: &Path) -> Result<FileAttributes, PlatformError> {
        default_file_attributes(path)
    }

    fn create_temp_file(&self, prefix: &str) -> Result<PathBuf, PlatformError> {
        let app_paths = self.app_paths()?;
        create_temp_file(app_paths.temp_path(), prefix).map_err(|error| PlatformError::Io {
            operation: "create temporary file",
            path: Some(app_paths.temp_path().to_path_buf()),
            message: error.to_string(),
        })
    }

    fn create_temp_dir(&self, prefix: &str) -> Result<PathBuf, PlatformError> {
        let app_paths = self.app_paths()?;
        create_temp_dir(app_paths.temp_path(), prefix).map_err(|error| PlatformError::Io {
            operation: "create temporary directory",
            path: Some(app_paths.temp_path().to_path_buf()),
            message: error.to_string(),
        })
    }

    fn cleanup_temp_path(&self, path: &Path) -> Result<(), PlatformError> {
        cleanup_temp_path(path).map_err(|error| PlatformError::Io {
            operation: "cleanup temporary path",
            path: Some(path.to_path_buf()),
            message: error.to_string(),
        })
    }

    fn show_notification(&self, _title: &str, _body: &str) -> Result<(), PlatformError> {
        Err(PlatformError::Unsupported {
            capability: "notifications",
        })
    }

    fn default_app_for_path(&self, _path: &Path) -> Result<Option<PathBuf>, PlatformError> {
        Err(PlatformError::Unsupported {
            capability: "default_apps",
        })
    }

    fn power_action(&self, action: PowerAction) -> Result<(), PlatformError> {
        Err(PlatformError::Unsupported {
            capability: match action {
                PowerAction::Lock => "power.lock",
                PowerAction::Sleep => "power.sleep",
                PowerAction::Shutdown => "power.shutdown",
                PowerAction::Restart => "power.restart",
            },
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlatformError {
    Unsupported {
        capability: &'static str,
    },
    InvalidInput {
        message: String,
    },
    PathResolution(PathResolutionError),
    Io {
        operation: &'static str,
        path: Option<PathBuf>,
        message: String,
    },
    ProcessPolicy {
        message: String,
    },
    CommandFailed {
        program: String,
        status: Option<i32>,
        stderr: String,
    },
    Native {
        operation: &'static str,
        message: String,
    },
}

impl fmt::Display for PlatformError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported { capability } => {
                write!(
                    formatter,
                    "platform capability is unsupported: {capability}"
                )
            }
            Self::InvalidInput { message } => formatter.write_str(message),
            Self::PathResolution(error) => error.fmt(formatter),
            Self::Io {
                operation,
                path,
                message,
            } => match path {
                Some(path) => write!(
                    formatter,
                    "{operation} failed for {}: {message}",
                    path.display()
                ),
                None => write!(formatter, "{operation} failed: {message}"),
            },
            Self::ProcessPolicy { message } => formatter.write_str(message),
            Self::CommandFailed {
                program,
                status,
                stderr,
            } => write!(
                formatter,
                "{program} failed with status {}: {stderr}",
                status
                    .map(|code| code.to_string())
                    .unwrap_or_else(|| "unknown".to_string())
            ),
            Self::Native { operation, message } => {
                write!(formatter, "{operation} failed: {message}")
            }
        }
    }
}

impl std::error::Error for PlatformError {}

impl From<PathResolutionError> for PlatformError {
    fn from(value: PathResolutionError) -> Self {
        Self::PathResolution(value)
    }
}

pub fn default_file_attributes(path: &Path) -> Result<FileAttributes, PlatformError> {
    let metadata = fs::metadata(path).map_err(|error| PlatformError::Io {
        operation: "read file attributes",
        path: Some(path.to_path_buf()),
        message: error.to_string(),
    })?;

    Ok(FileAttributes {
        path: path.to_path_buf(),
        is_file: metadata.is_file(),
        is_dir: metadata.is_dir(),
        len: metadata.len(),
        readonly: metadata.permissions().readonly(),
        modified: metadata.modified().ok(),
    })
}

pub fn native_platform() -> Box<dyn Platform> {
    #[cfg(windows)]
    {
        Box::new(crate::windows::WindowsPlatform)
    }

    #[cfg(target_os = "macos")]
    {
        Box::new(crate::macos::MacosPlatform)
    }

    #[cfg(not(any(windows, target_os = "macos")))]
    {
        Box::new(crate::mock::UnsupportedPlatform)
    }
}
