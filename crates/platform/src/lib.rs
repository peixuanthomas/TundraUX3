mod diagnostics;
mod document;
#[cfg(target_os = "linux")]
pub mod linux;
pub mod macos;
pub mod mock;
mod paths;
mod platform;
mod process;
mod system_monitor;
mod terminal;

#[cfg(windows)]
pub mod windows;

pub use diagnostics::{
    CheckStatus, DoctorReport, EnvironmentCheck, PathCheck, WindowsBuildClass,
    check_directory_read_write, classify_windows_build, run_doctor, run_doctor_with,
};
pub use document::{
    DocumentBytes, DocumentFingerprint, DocumentReadWindow, DocumentWriteError, MAX_DOCUMENT_BYTES,
    atomic_write_document, atomic_write_document_if_unchanged,
    atomic_write_document_if_unchanged_with, atomic_write_document_with, document_fingerprint,
    read_document_bytes, read_document_bytes_limited, read_document_bytes_limited_with_progress,
    read_document_prefix_snapshot_limited, read_document_prefix_snapshot_limited_with_progress,
    read_document_tail_bytes, validate_no_follow_path,
};
pub use paths::{
    AppPaths, PathResolutionError, UserDirs, build_binary_dir_app_paths, build_linux_app_paths,
    build_macos_app_paths, build_windows_app_paths, cleanup_temp_path, create_temp_dir,
    create_temp_file,
};
pub use platform::{
    CapabilityStatus, DirectoryEntryMetadata, DirectoryListing, DirectoryListingWarning,
    ExecutableKind, ExternalOpenPolicy, FileAttributes, FileOpenPolicy, LocalVolume,
    NetworkInterface, NetworkInterfaceKind, NetworkLinkState, NetworkStatus, Platform,
    PlatformCapabilities, PlatformError, PlatformIcon, PlatformKind, PlatformLifecycleEvent,
    StartupPermissionStatus, TrashEntry, TrashEntryId, TrashRestoreTarget, TrashStats,
    VolumeAccess, VolumeKind, default_external_open_policy, default_file_attributes,
    default_file_open_policy, default_read_directory, default_rename_path, native_platform,
};
pub use process::{ProcessExit, ProcessSpec, ProcessStream, validate_process_spec};
pub use system_monitor::{
    BatterySample, BatterySampleState, CpuSample, FastSystemSample, LoadSample, MemorySample,
    NativeSystemMonitor, NetworkIoInterfaceSample, ProcessMetricSample, SlowSystemSample,
    SystemMonitor, ThermalSensorSample,
};
pub use terminal::{
    ENTER_FULLSCREEN_SEQUENCE, EXIT_FULLSCREEN_SEQUENCE, TerminalCellSize, TerminalControlHandler,
    TerminalGraphicsCapabilities, TerminalGraphicsProbeStatus, TerminalGraphicsProtocol,
    is_windows_terminal_session, probe_terminal_graphics_capabilities, terminal_environment_check,
    terminal_environment_check_with, terminal_environment_check_with_graphics_protocol,
    with_terminal_fullscreen,
};

#[cfg(windows)]
pub use windows::current_windows_build;
