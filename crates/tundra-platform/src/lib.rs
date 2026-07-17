mod diagnostics;
mod document;
pub mod macos;
pub mod mock;
mod paths;
mod platform;
mod process;
mod terminal;

#[cfg(windows)]
pub mod windows;

pub use diagnostics::{
    CheckStatus, DoctorReport, EnvironmentCheck, PathCheck, WindowsBuildClass,
    check_directory_read_write, classify_windows_build, run_doctor, run_doctor_with,
};
pub use document::{
    DocumentBytes, DocumentFingerprint, DocumentReadWindow, DocumentWriteError,
    atomic_write_document, atomic_write_document_if_unchanged, document_fingerprint,
    read_document_bytes, read_document_tail_bytes, validate_no_follow_path,
};
pub use paths::{
    AppPaths, PathResolutionError, UserDirs, build_binary_dir_app_paths, build_macos_app_paths,
    build_windows_app_paths, cleanup_temp_path, create_temp_dir, create_temp_file,
};
pub use platform::{
    CapabilityStatus, DirectoryEntryMetadata, DirectoryListing, DirectoryListingWarning,
    ExecutableKind, ExternalOpenPolicy, FileAttributes, FileOpenPolicy, LocalVolume, Platform,
    PlatformCapabilities, PlatformError, PlatformKind, StartupPermissionStatus, TrashEntry,
    TrashEntryId, TrashRestoreTarget, TrashStats, VolumeKind, default_external_open_policy,
    default_file_attributes, default_file_open_policy, default_read_directory, default_rename_path,
    native_platform,
};
pub use process::{ProcessExit, ProcessSpec, ProcessStream, validate_process_spec};
pub use terminal::{
    ENTER_FULLSCREEN_SEQUENCE, EXIT_FULLSCREEN_SEQUENCE, TerminalControlHandler,
    is_windows_terminal_session, terminal_environment_check, terminal_environment_check_with,
    with_terminal_fullscreen,
};

#[cfg(windows)]
pub use windows::current_windows_build;
