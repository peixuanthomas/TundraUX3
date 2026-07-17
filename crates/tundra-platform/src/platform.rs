use std::fmt;
use std::fs;
use std::io::Read;
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
    pub directory_listing: CapabilityStatus,
    pub local_volumes: CapabilityStatus,
    pub trash: CapabilityStatus,
    pub critical_dialog: CapabilityStatus,
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
            directory_listing: CapabilityStatus::Supported,
            local_volumes: CapabilityStatus::Supported,
            trash: CapabilityStatus::Supported,
            critical_dialog: CapabilityStatus::Supported,
            power: CapabilityStatus::Supported,
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
            directory_listing: CapabilityStatus::Unsupported,
            local_volumes: CapabilityStatus::Unsupported,
            trash: CapabilityStatus::Unsupported,
            critical_dialog: CapabilityStatus::Unsupported,
            power: CapabilityStatus::Unsupported,
        }
    }

    pub fn checks(&self) -> [(&'static str, CapabilityStatus); 15] {
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
            ("directory_listing", self.directory_listing),
            ("local_volumes", self.local_volumes),
            ("trash", self.trash),
            ("critical_dialog", self.critical_dialog),
            ("power", self.power),
        ]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VolumeKind {
    Fixed,
    Removable,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalVolume {
    pub root: PathBuf,
    pub label: Option<String>,
    pub kind: VolumeKind,
    pub total_bytes: Option<u64>,
    pub available_bytes: Option<u64>,
}

/// A platform-owned identifier for an item currently in the system Trash.
/// Callers must not infer paths or other platform details from its value.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TrashEntryId(String);

impl TrashEntryId {
    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub(crate) fn from_native(value: impl Into<String>) -> Self {
        Self(value.into())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrashEntry {
    pub id: TrashEntryId,
    pub display_name: String,
    pub original_path: Option<PathBuf>,
    pub deleted_at: Option<SystemTime>,
    pub size: u64,
    pub is_directory: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TrashStats {
    pub item_count: u64,
    pub total_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum TrashRestoreTarget {
    OriginalLocation,
    /// The complete absolute destination path, including the restored name.
    DestinationPath(PathBuf),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartupPermissionStatus {
    Ready,
    ActionRequired { name: String, message: String },
}

impl StartupPermissionStatus {
    pub fn action_required(name: impl Into<String>, message: impl Into<String>) -> Self {
        Self::ActionRequired {
            name: name.into(),
            message: message.into(),
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
    pub hidden: bool,
    pub system: bool,
    pub archive: bool,
    pub symlink: bool,
    pub junction: bool,
    pub reparse_point: bool,
    pub shortcut: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutableKind {
    NativeBinary,
    Installer,
    Script,
    Shortcut,
    ApplicationBundle,
}

impl ExecutableKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::NativeBinary => "executable",
            Self::Installer => "installer",
            Self::Script => "script",
            Self::Shortcut => "shortcut",
            Self::ApplicationBundle => "application bundle",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileOpenPolicy {
    SystemDefault,
    LauncherRequired {
        kind: ExecutableKind,
        reason: String,
    },
    Blocked {
        reason: String,
    },
}

impl FileOpenPolicy {
    pub fn system_default() -> Self {
        Self::SystemDefault
    }

    pub fn launcher_required(kind: ExecutableKind, reason: impl Into<String>) -> Self {
        Self::LauncherRequired {
            kind,
            reason: reason.into(),
        }
    }

    pub fn blocked(reason: impl Into<String>) -> Self {
        Self::Blocked {
            reason: reason.into(),
        }
    }

    pub fn is_system_default(&self) -> bool {
        matches!(self, Self::SystemDefault)
    }

    pub fn requires_launcher(&self) -> bool {
        matches!(self, Self::LauncherRequired { .. })
    }

    pub fn reason(&self) -> Option<&str> {
        match self {
            Self::SystemDefault => None,
            Self::LauncherRequired { reason, .. } | Self::Blocked { reason } => Some(reason),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectoryEntryMetadata {
    pub path: PathBuf,
    pub name: String,
    pub attributes: Option<FileAttributes>,
    pub open_policy: FileOpenPolicy,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectoryListingWarning {
    pub path: PathBuf,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectoryListing {
    pub path: PathBuf,
    pub entries: Vec<DirectoryEntryMetadata>,
    pub warnings: Vec<DirectoryListingWarning>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExternalOpenPolicy {
    blocked_reason: Option<String>,
}

impl ExternalOpenPolicy {
    pub fn allowed() -> Self {
        Self {
            blocked_reason: None,
        }
    }

    pub fn blocked(reason: impl Into<String>) -> Self {
        Self {
            blocked_reason: Some(reason.into()),
        }
    }

    pub fn is_allowed(&self) -> bool {
        self.blocked_reason.is_none()
    }

    pub fn blocked_reason(&self) -> Option<&str> {
        self.blocked_reason.as_deref()
    }

    pub fn from_file_open_policy(policy: FileOpenPolicy) -> Self {
        match policy {
            FileOpenPolicy::SystemDefault => Self::allowed(),
            FileOpenPolicy::LauncherRequired { reason, .. }
            | FileOpenPolicy::Blocked { reason } => Self::blocked(reason),
        }
    }
}

pub trait Platform: Send + Sync {
    fn kind(&self) -> PlatformKind;
    fn capabilities(&self) -> PlatformCapabilities;

    /// Whether this value is the operating system's real platform backend.
    /// Test doubles must keep the default even when they emulate a native
    /// `PlatformKind`, so callers never infer permission for native side
    /// effects from `kind()` alone.
    fn is_native_backend(&self) -> bool {
        false
    }

    /// Checks permissions that must be granted before the application starts.
    /// This method must be read-only so diagnostics can call it without
    /// opening operating-system settings or displaying prompts.
    fn startup_permission_status(&self) -> Result<StartupPermissionStatus, PlatformError> {
        Ok(StartupPermissionStatus::Ready)
    }

    /// Opens the operating system's permission flow for a status returned by
    /// [`Platform::startup_permission_status`].
    fn request_startup_permissions(&self) -> Result<(), PlatformError> {
        Ok(())
    }

    fn user_dirs(&self) -> Result<UserDirs, PlatformError>;
    fn app_paths(&self) -> Result<AppPaths, PlatformError>;
    fn open_path(&self, path: &Path) -> Result<(), PlatformError>;
    fn open_with(&self, path: &Path, application: &Path) -> Result<(), PlatformError>;
    fn open_uri(&self, uri: &str) -> Result<(), PlatformError>;
    fn spawn_detached(&self, spec: &ProcessSpec) -> Result<(), PlatformError>;
    fn spawn_wait(&self, spec: &ProcessSpec) -> Result<ProcessExit, PlatformError>;
    fn read_clipboard_text(&self) -> Result<String, PlatformError>;
    fn write_clipboard_text(&self, text: &str) -> Result<(), PlatformError>;

    fn local_volumes(&self) -> Result<Vec<LocalVolume>, PlatformError> {
        Err(PlatformError::Unsupported {
            capability: "local_volumes",
        })
    }

    fn list_trash(&self) -> Result<Vec<TrashEntry>, PlatformError> {
        Err(PlatformError::Unsupported {
            capability: "trash.list",
        })
    }

    fn trash_stats(&self) -> Result<TrashStats, PlatformError> {
        Err(PlatformError::Unsupported {
            capability: "trash.stats",
        })
    }

    fn move_to_trash(&self, _paths: &[PathBuf]) -> Result<(), PlatformError> {
        Err(PlatformError::Unsupported {
            capability: "trash.move",
        })
    }

    fn empty_trash(&self) -> Result<(), PlatformError> {
        Err(PlatformError::Unsupported {
            capability: "trash.empty",
        })
    }

    fn restore_trash_item(
        &self,
        _id: &TrashEntryId,
        _target: TrashRestoreTarget,
    ) -> Result<PathBuf, PlatformError> {
        Err(PlatformError::Unsupported {
            capability: "trash.restore",
        })
    }

    fn file_attributes(&self, path: &Path) -> Result<FileAttributes, PlatformError> {
        default_file_attributes(path)
    }

    fn read_directory(&self, path: &Path) -> Result<DirectoryListing, PlatformError> {
        default_read_directory(self, path)
    }

    fn file_open_policy(&self, path: &Path, attributes: &FileAttributes) -> FileOpenPolicy {
        default_file_open_policy(self.kind(), path, attributes)
    }

    fn external_open_policy(&self, path: &Path, attributes: &FileAttributes) -> ExternalOpenPolicy {
        ExternalOpenPolicy::from_file_open_policy(self.file_open_policy(path, attributes))
    }

    fn rename_path(&self, source: &Path, target: &Path) -> Result<(), PlatformError> {
        default_rename_path(source, target)
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

    /// Displays a process-level critical error after an interactive terminal
    /// can no longer provide a reliable error surface.
    fn show_critical_error(&self, _title: &str, _body: &str) -> Result<(), PlatformError> {
        Err(PlatformError::Unsupported {
            capability: "critical_dialog",
        })
    }

    /// Returns whether an operating-system process is still alive.
    ///
    /// Backends treat an access-denied probe as alive because the process is
    /// known to exist even though its state cannot be queried in detail.
    fn is_process_alive(&self, _pid: u32) -> Result<bool, PlatformError> {
        Err(PlatformError::Unsupported {
            capability: "process_liveness",
        })
    }

    fn poweroff(&self) -> Result<(), PlatformError> {
        Err(PlatformError::Unsupported {
            capability: "power.poweroff",
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
    CrossDevice {
        source: PathBuf,
        target: PathBuf,
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
            Self::CrossDevice {
                source,
                target,
                message,
            } => write!(
                formatter,
                "cannot rename {} to {} across filesystems: {message}",
                source.display(),
                target.display()
            ),
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
    let link_metadata = fs::symlink_metadata(path).map_err(|error| PlatformError::Io {
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
        hidden: is_dotfile(path),
        system: false,
        archive: false,
        symlink: link_metadata.file_type().is_symlink(),
        junction: false,
        reparse_point: false,
        shortcut: false,
    })
}

pub fn default_read_directory<P: Platform + ?Sized>(
    platform: &P,
    path: &Path,
) -> Result<DirectoryListing, PlatformError> {
    let directory = fs::read_dir(path).map_err(|error| PlatformError::Io {
        operation: "read directory",
        path: Some(path.to_path_buf()),
        message: error.to_string(),
    })?;
    let mut entries = Vec::new();
    let mut warnings = Vec::new();

    for result in directory {
        let entry = match result {
            Ok(entry) => entry,
            Err(error) => {
                warnings.push(DirectoryListingWarning {
                    path: path.to_path_buf(),
                    message: error.to_string(),
                });
                continue;
            }
        };
        let entry_path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        match platform.file_attributes(&entry_path) {
            Ok(attributes) => {
                let open_policy = platform.file_open_policy(&entry_path, &attributes);
                entries.push(DirectoryEntryMetadata {
                    path: entry_path,
                    name,
                    attributes: Some(attributes),
                    open_policy,
                });
            }
            Err(error) => {
                warnings.push(DirectoryListingWarning {
                    path: entry_path.clone(),
                    message: error.to_string(),
                });
                entries.push(DirectoryEntryMetadata {
                    path: entry_path,
                    name,
                    attributes: None,
                    open_policy: FileOpenPolicy::blocked(
                        "metadata is unavailable; opening this entry is blocked",
                    ),
                });
            }
        }
    }

    Ok(DirectoryListing {
        path: path.to_path_buf(),
        entries,
        warnings,
    })
}

pub fn default_rename_path(source: &Path, target: &Path) -> Result<(), PlatformError> {
    fs::rename(source, target).map_err(|error| {
        if is_cross_device_error(&error) {
            PlatformError::CrossDevice {
                source: source.to_path_buf(),
                target: target.to_path_buf(),
                message: error.to_string(),
            }
        } else {
            PlatformError::Io {
                operation: "rename path",
                path: Some(source.to_path_buf()),
                message: error.to_string(),
            }
        }
    })
}

fn is_cross_device_error(error: &std::io::Error) -> bool {
    matches!(error.raw_os_error(), Some(17 | 18))
}

pub fn default_file_open_policy(
    kind: PlatformKind,
    path: &Path,
    attributes: &FileAttributes,
) -> FileOpenPolicy {
    if attributes.symlink || attributes.junction || attributes.reparse_point {
        return FileOpenPolicy::blocked(
            "symbolic links and reparse points are blocked until safe path traversal is available",
        );
    }

    if kind != PlatformKind::Windows && attributes.is_file && extension_is(path, &["exe"]) {
        return FileOpenPolicy::launcher_required(
            ExecutableKind::NativeBinary,
            "executable files must be opened through Launcher",
        );
    }

    match kind {
        PlatformKind::Windows => windows_file_open_policy(path, attributes),
        PlatformKind::Macos => macos_file_open_policy(path, attributes),
        PlatformKind::Unsupported => unsupported_file_open_policy(path, attributes),
    }
}

pub fn default_external_open_policy(attributes: &FileAttributes) -> ExternalOpenPolicy {
    if attributes.shortcut {
        return ExternalOpenPolicy::blocked(
            "shortcut files are blocked until Launcher/Open With policy is available",
        );
    }

    if attributes.reparse_point {
        return ExternalOpenPolicy::blocked(
            "reparse point external opens are blocked until path policy is available",
        );
    }

    ExternalOpenPolicy::allowed()
}

pub(crate) fn windows_file_open_policy(path: &Path, attributes: &FileAttributes) -> FileOpenPolicy {
    const NATIVE: &[&str] = &["exe", "com", "scr", "cpl", "pif"];
    const INSTALLERS: &[&str] = &["msi", "msp", "msix", "msixbundle", "appx", "appxbundle"];
    const SCRIPTS: &[&str] = &[
        "bat", "cmd", "ps1", "psm1", "vbs", "vbe", "js", "jse", "wsf", "wsh", "hta", "jar", "py",
        "pyw", "rb", "pl",
    ];
    const SHORTCUTS: &[&str] = &["lnk", "url", "reg", "scf"];

    let classified = if attributes.shortcut {
        Some(ExecutableKind::Shortcut)
    } else if !attributes.is_file {
        None
    } else if extension_is(path, NATIVE) {
        Some(ExecutableKind::NativeBinary)
    } else if extension_is(path, INSTALLERS) {
        Some(ExecutableKind::Installer)
    } else if extension_is(path, SCRIPTS) {
        Some(ExecutableKind::Script)
    } else if extension_is(path, SHORTCUTS) {
        Some(ExecutableKind::Shortcut)
    } else {
        None
    };

    classified.map_or_else(FileOpenPolicy::system_default, |kind| {
        FileOpenPolicy::launcher_required(
            kind,
            format!(
                "Windows {} files must be opened through Launcher",
                kind.label()
            ),
        )
    })
}

pub(crate) fn macos_file_open_policy(path: &Path, attributes: &FileAttributes) -> FileOpenPolicy {
    if attributes.is_dir && extension_is(path, &["app"]) {
        return FileOpenPolicy::launcher_required(
            ExecutableKind::ApplicationBundle,
            "macOS application bundles must be opened through Launcher",
        );
    }
    if extension_is(path, &["pkg", "mpkg"]) {
        return FileOpenPolicy::launcher_required(
            ExecutableKind::Installer,
            "macOS installer packages must be opened through Launcher",
        );
    }
    let mach_o = attributes.is_file && is_mach_o(path);
    if attributes.is_file && (mach_o || native_executable_bit(path)) {
        return FileOpenPolicy::launcher_required(
            if !mach_o && extension_is(path, &["command", "tool", "sh", "bash", "zsh"]) {
                ExecutableKind::Script
            } else {
                ExecutableKind::NativeBinary
            },
            "executable files must be opened through Launcher",
        );
    }
    FileOpenPolicy::system_default()
}

pub(crate) fn unsupported_file_open_policy(
    path: &Path,
    attributes: &FileAttributes,
) -> FileOpenPolicy {
    if attributes.is_file
        && (extension_is(path, &["desktop", "appimage", "run"]) || native_executable_bit(path))
    {
        return FileOpenPolicy::launcher_required(
            ExecutableKind::NativeBinary,
            "executable files must be opened through Launcher",
        );
    }
    FileOpenPolicy::system_default()
}

pub(crate) fn windows_external_open_policy(
    path: &Path,
    attributes: &FileAttributes,
) -> ExternalOpenPolicy {
    ExternalOpenPolicy::from_file_open_policy(default_file_open_policy(
        PlatformKind::Windows,
        path,
        attributes,
    ))
}

fn extension_is(path: &Path, candidates: &[&str]) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            candidates
                .iter()
                .any(|candidate| extension.eq_ignore_ascii_case(candidate))
        })
}

fn is_mach_o(path: &Path) -> bool {
    const MACH_O_MAGICS: [[u8; 4]; 8] = [
        [0xfe, 0xed, 0xfa, 0xce],
        [0xce, 0xfa, 0xed, 0xfe],
        [0xfe, 0xed, 0xfa, 0xcf],
        [0xcf, 0xfa, 0xed, 0xfe],
        [0xca, 0xfe, 0xba, 0xbe],
        [0xbe, 0xba, 0xfe, 0xca],
        [0xca, 0xfe, 0xba, 0xbf],
        [0xbf, 0xba, 0xfe, 0xca],
    ];

    let mut magic = [0_u8; 4];
    fs::File::open(path)
        .and_then(|mut file| file.read_exact(&mut magic))
        .is_ok()
        && MACH_O_MAGICS.contains(&magic)
}

#[cfg(unix)]
fn native_executable_bit(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;

    fs::symlink_metadata(path)
        .map(|metadata| metadata.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn native_executable_bit(_path: &Path) -> bool {
    false
}

fn is_dotfile(path: &Path) -> bool {
    path.file_name()
        .map(|file_name| {
            let file_name = file_name.to_string_lossy();
            file_name.starts_with('.') && file_name != "." && file_name != ".."
        })
        .unwrap_or(false)
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
