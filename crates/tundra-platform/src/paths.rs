use std::env;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::process;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserDirs {
    desktop: PathBuf,
    documents: PathBuf,
    downloads: PathBuf,
    pictures: PathBuf,
    videos: PathBuf,
    music: PathBuf,
    app_data: PathBuf,
}

impl UserDirs {
    pub fn new(
        desktop: impl Into<PathBuf>,
        documents: impl Into<PathBuf>,
        downloads: impl Into<PathBuf>,
        pictures: impl Into<PathBuf>,
        videos: impl Into<PathBuf>,
        music: impl Into<PathBuf>,
        app_data: impl Into<PathBuf>,
    ) -> Result<Self, PathResolutionError> {
        Ok(Self {
            desktop: require_absolute("desktop directory", desktop.into())?,
            documents: require_absolute("documents directory", documents.into())?,
            downloads: require_absolute("downloads directory", downloads.into())?,
            pictures: require_absolute("pictures directory", pictures.into())?,
            videos: require_absolute("videos directory", videos.into())?,
            music: require_absolute("music directory", music.into())?,
            app_data: require_absolute("app data directory", app_data.into())?,
        })
    }

    pub fn desktop(&self) -> &Path {
        &self.desktop
    }

    pub fn documents(&self) -> &Path {
        &self.documents
    }

    pub fn downloads(&self) -> &Path {
        &self.downloads
    }

    pub fn pictures(&self) -> &Path {
        &self.pictures
    }

    pub fn videos(&self) -> &Path {
        &self.videos
    }

    pub fn music(&self) -> &Path {
        &self.music
    }

    pub fn app_data(&self) -> &Path {
        &self.app_data
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppPaths {
    config_path: PathBuf,
    data_path: PathBuf,
    cache_path: PathBuf,
    logs_path: PathBuf,
    temp_path: PathBuf,
}

impl AppPaths {
    #[cfg(windows)]
    pub const CONFIG_TEMPLATE: &'static str = "%APPDATA%\\TundraUX3\\config.toml";
    #[cfg(target_os = "macos")]
    pub const CONFIG_TEMPLATE: &'static str = "~/Library/Application Support/TundraUX3/config.toml";
    #[cfg(not(any(windows, target_os = "macos")))]
    pub const CONFIG_TEMPLATE: &'static str = "<unsupported>/TundraUX3/config.toml";

    #[cfg(windows)]
    pub const DATA_TEMPLATE: &'static str = "%LOCALAPPDATA%\\TundraUX3\\state";
    #[cfg(target_os = "macos")]
    pub const DATA_TEMPLATE: &'static str = "~/Library/Application Support/TundraUX3/state";
    #[cfg(not(any(windows, target_os = "macos")))]
    pub const DATA_TEMPLATE: &'static str = "<unsupported>/TundraUX3/state";

    #[cfg(windows)]
    pub const CACHE_TEMPLATE: &'static str = "%LOCALAPPDATA%\\TundraUX3\\cache";
    #[cfg(target_os = "macos")]
    pub const CACHE_TEMPLATE: &'static str = "~/Library/Caches/TundraUX3";
    #[cfg(not(any(windows, target_os = "macos")))]
    pub const CACHE_TEMPLATE: &'static str = "<unsupported>/TundraUX3/cache";

    #[cfg(windows)]
    pub const LOGS_TEMPLATE: &'static str = "%LOCALAPPDATA%\\TundraUX3\\logs";
    #[cfg(target_os = "macos")]
    pub const LOGS_TEMPLATE: &'static str = "~/Library/Logs/TundraUX3";
    #[cfg(not(any(windows, target_os = "macos")))]
    pub const LOGS_TEMPLATE: &'static str = "<unsupported>/TundraUX3/logs";

    #[cfg(windows)]
    pub const TEMP_TEMPLATE: &'static str = "%TEMP%\\TundraUX3";
    #[cfg(target_os = "macos")]
    pub const TEMP_TEMPLATE: &'static str = "<temp-dir>/TundraUX3";
    #[cfg(not(any(windows, target_os = "macos")))]
    pub const TEMP_TEMPLATE: &'static str = "<unsupported>/TundraUX3/temp";

    pub fn from_environment() -> Result<Self, PathResolutionError> {
        crate::native_platform()
            .app_paths()
            .map_err(|error| PathResolutionError::Platform {
                message: error.to_string(),
            })
    }

    pub fn from_current_exe() -> Result<Self, PathResolutionError> {
        let executable_path =
            env::current_exe().map_err(|error| PathResolutionError::CurrentExe {
                message: error.to_string(),
            })?;
        let binary_dir =
            executable_path
                .parent()
                .ok_or_else(|| PathResolutionError::MissingParent {
                    name: "current executable",
                    value: executable_path.clone(),
                })?;

        Self::from_binary_dir(binary_dir)
    }

    pub fn from_binary_dir(binary_dir: impl Into<PathBuf>) -> Result<Self, PathResolutionError> {
        build_binary_dir_app_paths(binary_dir)
    }

    pub fn from_parts(
        config_path: impl Into<PathBuf>,
        data_path: impl Into<PathBuf>,
        cache_path: impl Into<PathBuf>,
        logs_path: impl Into<PathBuf>,
        temp_path: impl Into<PathBuf>,
    ) -> Result<Self, PathResolutionError> {
        Ok(Self {
            config_path: require_absolute("config path", config_path.into())?,
            data_path: require_absolute("data path", data_path.into())?,
            cache_path: require_absolute("cache path", cache_path.into())?,
            logs_path: require_absolute("logs path", logs_path.into())?,
            temp_path: require_absolute("temp path", temp_path.into())?,
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

    pub fn logs_path(&self) -> &Path {
        &self.logs_path
    }

    pub fn temp_path(&self) -> &Path {
        &self.temp_path
    }
}

pub fn build_binary_dir_app_paths(
    binary_dir: impl Into<PathBuf>,
) -> Result<AppPaths, PathResolutionError> {
    let binary_dir = require_absolute("binary directory", binary_dir.into())?;
    let app_dir = binary_dir.join("TundraUX3");

    AppPaths::from_parts(
        app_dir.join("config.toml"),
        app_dir.join("state"),
        app_dir.join("cache"),
        app_dir.join("logs"),
        app_dir.join("temp"),
    )
}

pub fn build_windows_app_paths(
    roaming_app_data: impl Into<PathBuf>,
    local_app_data: impl Into<PathBuf>,
    temp_dir: impl Into<PathBuf>,
) -> Result<AppPaths, PathResolutionError> {
    let roaming_app_data = require_absolute("roaming app data directory", roaming_app_data.into())?;
    let local_app_data = require_absolute("local app data directory", local_app_data.into())?;
    let temp_dir = require_absolute("temporary directory", temp_dir.into())?;
    let roaming_app_dir = roaming_app_data.join("TundraUX3");
    let local_app_dir = local_app_data.join("TundraUX3");

    AppPaths::from_parts(
        roaming_app_dir.join("config.toml"),
        local_app_dir.join("state"),
        local_app_dir.join("cache"),
        local_app_dir.join("logs"),
        temp_dir.join("TundraUX3"),
    )
}

pub fn build_macos_app_paths(
    home_dir: impl Into<PathBuf>,
    temp_dir: impl Into<PathBuf>,
) -> Result<AppPaths, PathResolutionError> {
    let home_dir = require_absolute("home directory", home_dir.into())?;
    let temp_dir = require_absolute("temporary directory", temp_dir.into())?;
    let app_support = home_dir
        .join("Library")
        .join("Application Support")
        .join("TundraUX3");

    AppPaths::from_parts(
        app_support.join("config.toml"),
        app_support.join("state"),
        home_dir.join("Library").join("Caches").join("TundraUX3"),
        home_dir.join("Library").join("Logs").join("TundraUX3"),
        temp_dir.join("TundraUX3"),
    )
}

pub fn create_temp_file(
    temp_root: &Path,
    prefix: impl AsRef<str>,
) -> Result<PathBuf, std::io::Error> {
    fs::create_dir_all(temp_root)?;

    for _ in 0..64 {
        let path = temp_root.join(unique_temp_name(prefix.as_ref(), "tmp"));
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(_) => return Ok(path),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
    }

    Err(std::io::Error::new(
        std::io::ErrorKind::AlreadyExists,
        "could not create a unique temporary file",
    ))
}

pub fn create_temp_dir(
    temp_root: &Path,
    prefix: impl AsRef<str>,
) -> Result<PathBuf, std::io::Error> {
    fs::create_dir_all(temp_root)?;

    for _ in 0..64 {
        let path = temp_root.join(unique_temp_name(prefix.as_ref(), "dir"));
        match fs::create_dir(&path) {
            Ok(()) => return Ok(path),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
    }

    Err(std::io::Error::new(
        std::io::ErrorKind::AlreadyExists,
        "could not create a unique temporary directory",
    ))
}

pub fn cleanup_temp_path(path: &Path) -> Result<(), std::io::Error> {
    if !path.exists() {
        return Ok(());
    }

    if path.is_dir() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PathResolutionError {
    RelativePath { name: &'static str, value: PathBuf },
    CurrentExe { message: String },
    MissingParent { name: &'static str, value: PathBuf },
    MissingEnvironment { name: &'static str },
    Platform { message: String },
}

impl fmt::Display for PathResolutionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RelativePath { name, value } => {
                write!(formatter, "{name} is not absolute: {}", value.display())
            }
            Self::CurrentExe { message } => {
                write!(formatter, "cannot locate current executable: {message}")
            }
            Self::MissingParent { name, value } => {
                write!(
                    formatter,
                    "{name} has no parent directory: {}",
                    value.display()
                )
            }
            Self::MissingEnvironment { name } => {
                write!(formatter, "missing required environment variable {name}")
            }
            Self::Platform { message } => formatter.write_str(message),
        }
    }
}

impl std::error::Error for PathResolutionError {}

pub(crate) fn require_absolute(
    name: &'static str,
    path: PathBuf,
) -> Result<PathBuf, PathResolutionError> {
    if path.is_absolute() {
        Ok(path)
    } else {
        Err(PathResolutionError::RelativePath { name, value: path })
    }
}

pub(crate) fn home_dir_from_env() -> Result<PathBuf, PathResolutionError> {
    let home = env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or(PathResolutionError::MissingEnvironment { name: "HOME" })?;
    require_absolute("home directory", home)
}

fn unique_temp_name(prefix: &str, suffix: &str) -> String {
    format!(
        ".tundraux3-{prefix}-{}-{}-{suffix}",
        process::id(),
        timestamp_nanos()
    )
}

pub(crate) fn timestamp_nanos() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0)
}
