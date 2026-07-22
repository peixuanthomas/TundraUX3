use crate::{ShellComponent, ShellPopup};
use identity::DebugPolicy;
use platform::{
    AppPaths, Platform, PlatformCapabilities, PlatformError, PlatformKind, StartupPermissionStatus,
};
use std::path::PathBuf;
use storage::{StorageError, StorageLoadReport, StorageManager, UserRecord};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellTerminalMode {
    Fullscreen,
    NotFullscreen,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HomeModeOverride {
    BuildDefault,
    Debug,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ShellLaunchTarget {
    #[default]
    Home,
    Editor,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShellLaunchConfig {
    pub terminal_mode: ShellTerminalMode,
    pub home_mode_override: HomeModeOverride,
    pub launch_target: ShellLaunchTarget,
}

impl Default for ShellLaunchConfig {
    fn default() -> Self {
        Self {
            terminal_mode: ShellTerminalMode::Fullscreen,
            home_mode_override: Self::profile_home_mode_override(),
            launch_target: ShellLaunchTarget::Home,
        }
    }
}

impl ShellLaunchConfig {
    const fn profile_home_mode_override() -> HomeModeOverride {
        if cfg!(debug_assertions) {
            HomeModeOverride::Debug
        } else {
            HomeModeOverride::BuildDefault
        }
    }

    pub const fn editor() -> Self {
        Self {
            terminal_mode: ShellTerminalMode::Fullscreen,
            home_mode_override: Self::profile_home_mode_override(),
            launch_target: ShellLaunchTarget::Editor,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellHomeMode {
    Debug,
    User,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellScreen {
    FirstRunSetup,
    BootstrapAdmin,
    Login,
    Home,
    Clock,
    Diagnostics,
    Explorer,
    Launcher,
    Editor,
    Settings,
    UserManagement,
    ExitConfirm,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellAppConfig {
    pub home_mode: Option<ShellHomeMode>,
    pub border_shape: ui::BorderShape,
    pub border_color: ratatui::style::Color,
    pub accent_color: ratatui::style::Color,
}

impl Default for ShellAppConfig {
    fn default() -> Self {
        Self {
            home_mode: None,
            border_shape: ui::BorderShape::Rounded,
            border_color: ratatui::style::Color::White,
            accent_color: ratatui::style::Color::Cyan,
        }
    }
}

impl ShellAppConfig {
    pub(crate) fn from_appearance(appearance: &storage::AppearanceConfig) -> Self {
        let border_shape = match appearance.border_shape {
            storage::BorderShape::Rounded => ui::BorderShape::Rounded,
            storage::BorderShape::Square => ui::BorderShape::Square,
        };
        let border_color = ui_theme_color(appearance.border_color);
        let accent_color = ui_theme_color(appearance.accent_color);
        Self {
            border_shape,
            border_color,
            accent_color,
            ..Self::default()
        }
    }
}

pub(crate) const fn ui_theme_color(color: storage::BorderColor) -> ratatui::style::Color {
    use ratatui::style::Color;
    use storage::BorderColor;

    match color {
        BorderColor::Black => Color::Black,
        BorderColor::Red => Color::Red,
        BorderColor::Green => Color::Green,
        BorderColor::Yellow => Color::Yellow,
        BorderColor::Blue => Color::Blue,
        BorderColor::Magenta => Color::Magenta,
        BorderColor::Cyan => Color::Cyan,
        BorderColor::Gray => Color::Gray,
        BorderColor::DarkGray => Color::DarkGray,
        BorderColor::LightRed => Color::LightRed,
        BorderColor::LightGreen => Color::LightGreen,
        BorderColor::LightYellow => Color::LightYellow,
        BorderColor::LightBlue => Color::LightBlue,
        BorderColor::LightMagenta => Color::LightMagenta,
        BorderColor::LightCyan => Color::LightCyan,
        BorderColor::White => Color::White,
        BorderColor::Rgb(red, green, blue) => Color::Rgb(red, green, blue),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShellStorageWarning {
    RecoveredDefaults(String),
}

impl ShellStorageWarning {
    pub fn recovered_defaults(message: impl Into<String>) -> Self {
        Self::RecoveredDefaults(message.into())
    }

    fn is_recovery_warning(&self) -> bool {
        matches!(self, Self::RecoveredDefaults(_))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ShellStorageReport {
    pub app_paths: Option<AppPaths>,
    pub warnings: Vec<ShellStorageWarning>,
    pub created_files: Vec<PathBuf>,
    pub migrated_files: Vec<PathBuf>,
    pub recovered_files: Vec<ShellRecoveredStorageFile>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellRecoveredStorageFile {
    pub original_path: PathBuf,
    pub recovered_path: PathBuf,
}

impl ShellStorageReport {
    pub fn clean(app_paths: Option<AppPaths>) -> Self {
        Self {
            app_paths,
            warnings: Vec::new(),
            created_files: Vec::new(),
            migrated_files: Vec::new(),
            recovered_files: Vec::new(),
        }
    }

    pub fn recovered_defaults(app_paths: Option<AppPaths>) -> Self {
        Self {
            app_paths,
            warnings: vec![ShellStorageWarning::recovered_defaults(
                "storage recovered defaults",
            )],
            created_files: Vec::new(),
            migrated_files: Vec::new(),
            recovered_files: Vec::new(),
        }
    }

    pub fn has_recovery_warnings(&self) -> bool {
        self.warnings
            .iter()
            .any(ShellStorageWarning::is_recovery_warning)
    }

    fn from_storage_load_report(app_paths: Option<AppPaths>, report: StorageLoadReport) -> Self {
        Self {
            app_paths,
            warnings: report
                .warnings
                .into_iter()
                .map(ShellStorageWarning::RecoveredDefaults)
                .collect(),
            created_files: report.created_files,
            migrated_files: report.migrated_files,
            recovered_files: report
                .recovered_files
                .into_iter()
                .map(|recovered| ShellRecoveredStorageFile {
                    original_path: recovered.original_path,
                    recovered_path: recovered.recovered_path,
                })
                .collect(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellRestoredSession {
    pub active_screen: ShellScreen,
    pub focused_component: ShellComponent,
    pub display_mode: ShellHomeMode,
    pub active_popup: Option<ShellPopup>,
}

impl ShellRestoredSession {
    pub fn new(display_mode: ShellHomeMode, focused_component: ShellComponent) -> Self {
        Self {
            active_screen: ShellScreen::Home,
            focused_component,
            display_mode,
            active_popup: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellLoginUser {
    pub username: String,
    pub display_name: String,
    pub role: String,
    pub enabled: bool,
    pub password_hint: Option<String>,
    pub locked_until_epoch_ms: Option<u64>,
    pub last_login_at_epoch_ms: Option<u64>,
}

impl ShellLoginUser {
    pub(crate) fn from_record(record: &UserRecord) -> Self {
        Self {
            username: record.username.clone(),
            display_name: record.display_name.clone(),
            role: record.role.clone(),
            enabled: record.enabled,
            password_hint: record.password_hint.clone(),
            locked_until_epoch_ms: record.locked_until_epoch_ms,
            last_login_at_epoch_ms: record.last_login_at_epoch_ms,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellStartupState {
    pub app_config: ShellAppConfig,
    pub storage_report: ShellStorageReport,
    pub platform_kind: PlatformKind,
    pub platform_capabilities: PlatformCapabilities,
    pub restored_session: Option<ShellRestoredSession>,
    pub storage_manager: Option<StorageManager>,
    pub auth_bootstrap_required: bool,
    pub login_users: Vec<ShellLoginUser>,
    pub debug_policy: DebugPolicy,
}

impl ShellStartupState {
    pub fn clean(platform_kind: PlatformKind, platform_capabilities: PlatformCapabilities) -> Self {
        Self {
            app_config: ShellAppConfig::default(),
            storage_report: ShellStorageReport::default(),
            platform_kind,
            platform_capabilities,
            restored_session: None,
            storage_manager: None,
            auth_bootstrap_required: false,
            login_users: Vec::new(),
            debug_policy: DebugPolicy::default(),
        }
    }

    pub(crate) fn current_process_defaults() -> Self {
        let platform = platform::native_platform();
        Self::clean(platform.kind(), platform.capabilities())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShellStartupError {
    Platform(PlatformError),
    Storage(StorageError),
}

impl std::fmt::Display for ShellStartupError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Platform(error) => write!(formatter, "startup platform error: {error}"),
            Self::Storage(error) => write!(formatter, "startup storage error: {error}"),
        }
    }
}

impl std::error::Error for ShellStartupError {}

impl From<PlatformError> for ShellStartupError {
    fn from(value: PlatformError) -> Self {
        Self::Platform(value)
    }
}

impl From<StorageError> for ShellStartupError {
    fn from(value: StorageError) -> Self {
        Self::Storage(value)
    }
}

pub fn prepare_shell_startup(
    platform: &dyn Platform,
    launch_config: ShellLaunchConfig,
) -> Result<ShellStartupState, ShellStartupError> {
    let _ = launch_config;
    ensure_startup_permissions(platform)?;
    let platform_kind = platform.kind();
    let platform_capabilities = platform.capabilities();
    let storage_open = StorageManager::open_from_platform(platform)?;
    let app_paths = app_paths_from_storage_layout(storage_open.manager.layout())?;
    let users = storage_open.manager.load_users()?;
    let sessions = storage_open.manager.load_sessions()?;
    let storage_report =
        ShellStorageReport::from_storage_load_report(Some(app_paths), storage_open.report);
    let debug_policy = DebugPolicy::current_build();
    let login_users = users
        .users
        .iter()
        .map(ShellLoginUser::from_record)
        .collect::<Vec<_>>();

    Ok(ShellStartupState {
        app_config: ShellAppConfig::default(),
        storage_report,
        platform_kind,
        platform_capabilities,
        restored_session: restored_session_from_storage(&sessions),
        storage_manager: Some(storage_open.manager),
        auth_bootstrap_required: login_users.is_empty(),
        login_users,
        debug_policy,
    })
}

fn ensure_startup_permissions(platform: &dyn Platform) -> Result<(), ShellStartupError> {
    let StartupPermissionStatus::ActionRequired { name, message } =
        platform.startup_permission_status()?
    else {
        return Ok(());
    };

    let request_error = platform.request_startup_permissions().err();
    let request_detail = request_error
        .map(|error| {
            format!(" The operating-system permission screen could not be opened: {error}")
        })
        .unwrap_or_default();
    Err(ShellStartupError::Platform(PlatformError::Native {
        operation: "startup permission check",
        message: format!("{name} is required. {message}{request_detail}"),
    }))
}

pub(crate) fn app_paths_from_storage_layout(
    layout: &storage::StorageLayout,
) -> Result<AppPaths, ShellStartupError> {
    AppPaths::from_parts(
        layout.config_path.clone(),
        layout.data_path.clone(),
        layout.cache_path.clone(),
        layout.logs_path.clone(),
        layout.temp_path.clone(),
    )
    .map_err(|error| ShellStartupError::Platform(PlatformError::PathResolution(error)))
}

fn restored_session_from_storage(
    _sessions: &storage::SessionsDocument,
) -> Option<ShellRestoredSession> {
    None
}
