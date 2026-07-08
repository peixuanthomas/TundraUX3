mod diagnostics;
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
pub use paths::{
    AppPaths, PathResolutionError, UserDirs, build_binary_dir_app_paths, build_macos_app_paths,
    build_windows_app_paths, cleanup_temp_path, create_temp_dir, create_temp_file,
};
pub use platform::{
    CapabilityStatus, ExternalOpenPolicy, FileAttributes, Platform, PlatformCapabilities,
    PlatformError, PlatformKind, PowerAction, default_external_open_policy,
    default_file_attributes, native_platform,
};
pub use process::{ProcessExit, ProcessSpec, ProcessStream, validate_process_spec};
pub use terminal::{
    ENTER_FULLSCREEN_SEQUENCE, EXIT_FULLSCREEN_SEQUENCE, TerminalControlHandler,
    is_windows_terminal_session, terminal_environment_check, terminal_environment_check_with,
    with_terminal_fullscreen,
};

#[cfg(windows)]
pub use windows::current_windows_build;
