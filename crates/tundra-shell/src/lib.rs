use tundra_apps::explorer::{ExplorerCommand, ExplorerController, ExplorerState};
use tundra_core::{
    AuditOutcome, AuditService, AuthSession, CoreError, DebugPolicy, PASSWORD_MAX_LEN,
    PASSWORD_MIN_LEN, PermissionAction, PermissionService, SessionService, UserAccount, UserRole,
    UserService,
};
use tundra_storage::{
    CONFIG_DESCRIPTOR, SCHEMA_VERSION, StorageError, StorageLoadReport, StorageManager, UserRecord,
};

use chrono::{DateTime, Timelike, Utc};
use crossterm::cursor::{Hide, Show};
use crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyEventKind,
    KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::Rect;
use std::io::{self, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tundra_platform::{
    AppPaths, CapabilityStatus, FileAttributes, Platform, PlatformCapabilities, PlatformError,
    PlatformKind, TerminalControlHandler,
};
use tundra_weathr::network_clock::{NetworkClock, TimeSyncResult};

pub use tundra_platform::{ENTER_FULLSCREEN_SEQUENCE, EXIT_FULLSCREEN_SEQUENCE};
pub use tundra_weathr::network_clock::TIME_SYNC_INTERVAL;

pub const BANNER_DISPLAY_DURATION: Duration = Duration::from_secs(2);
const BANNER_ASSET_KEY: &str = "tundraux3";

static PANIC_RESTORE_HOOK_INSTALLED: AtomicBool = AtomicBool::new(false);

pub fn banner_lines() -> Result<Vec<String>, tundra_ui::AssetError> {
    let ascii_assets = tundra_ui::RuntimeAsciiAssets::load_default()?;
    Ok(ascii_assets.banner_lines(BANNER_ASSET_KEY)?.to_vec())
}

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShellLaunchConfig {
    pub terminal_mode: ShellTerminalMode,
    pub home_mode_override: HomeModeOverride,
}

impl Default for ShellLaunchConfig {
    fn default() -> Self {
        Self {
            terminal_mode: ShellTerminalMode::Fullscreen,
            home_mode_override: HomeModeOverride::BuildDefault,
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
    Explorer,
    UserManagement,
    ExitConfirm,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ShellAppConfig {
    pub home_mode: Option<ShellHomeMode>,
}

impl ShellAppConfig {
    fn from_storage_config(_config: &tundra_storage::StorageConfig) -> Self {
        Self::default()
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
    fn from_record(record: &UserRecord) -> Self {
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

    fn current_process_defaults() -> Self {
        let platform = tundra_platform::native_platform();
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
    let platform_kind = platform.kind();
    let platform_capabilities = platform.capabilities();
    let storage_open = StorageManager::open_from_platform(platform)?;
    let app_paths = app_paths_from_storage_layout(storage_open.manager.layout())?;
    let storage_config = storage_open.manager.load_config()?;
    let users = storage_open.manager.load_users()?;
    let sessions = storage_open.manager.load_sessions()?;
    let storage_report =
        ShellStorageReport::from_storage_load_report(Some(app_paths), storage_open.report);
    let debug_policy = DebugPolicy::current_build(storage_config.security.allow_release_debug);
    let login_users = users
        .users
        .iter()
        .map(ShellLoginUser::from_record)
        .collect::<Vec<_>>();

    Ok(ShellStartupState {
        app_config: ShellAppConfig::from_storage_config(&storage_config),
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

fn app_paths_from_storage_layout(
    layout: &tundra_storage::StorageLayout,
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
    _sessions: &tundra_storage::SessionsDocument,
) -> Option<ShellRestoredSession> {
    None
}

pub const DOUBLE_CLICK_INTERVAL: Duration = Duration::from_millis(500);
const DOUBLE_CLICK_CELL_TOLERANCE: u16 = 1;

pub type CellPosition = (u16, u16);
pub type ShellInput = InputEvent;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct InputModifiers {
    pub shift: bool,
    pub control: bool,
    pub alt: bool,
}

impl InputModifiers {
    pub const fn none() -> Self {
        Self {
            shift: false,
            control: false,
            alt: false,
        }
    }
}

impl From<KeyModifiers> for InputModifiers {
    fn from(modifiers: KeyModifiers) -> Self {
        Self {
            shift: modifiers.contains(KeyModifiers::SHIFT),
            control: modifiers.contains(KeyModifiers::CONTROL),
            alt: modifiers.contains(KeyModifiers::ALT),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum InputKey {
    Character(char),
    Enter,
    Escape,
    Backspace,
    Tab,
    BackTab,
    Left,
    Right,
    Up,
    Down,
    Delete,
    Insert,
    Home,
    End,
    PageUp,
    PageDown,
    Function(u8),
    Other(String),
}

impl InputKey {
    fn label(&self) -> String {
        match self {
            Self::Character(character) => character.to_string(),
            Self::Enter => "Enter".to_string(),
            Self::Escape => "Esc".to_string(),
            Self::Backspace => "Backspace".to_string(),
            Self::Tab => "Tab".to_string(),
            Self::BackTab => "Shift+Tab".to_string(),
            Self::Left => "Left".to_string(),
            Self::Right => "Right".to_string(),
            Self::Up => "Up".to_string(),
            Self::Down => "Down".to_string(),
            Self::Delete => "Delete".to_string(),
            Self::Insert => "Insert".to_string(),
            Self::Home => "Home".to_string(),
            Self::End => "End".to_string(),
            Self::PageUp => "PageUp".to_string(),
            Self::PageDown => "PageDown".to_string(),
            Self::Function(number) => format!("F({number})"),
            Self::Other(label) => label.clone(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum InputPhase {
    Press,
    Repeat,
    Release,
}

impl InputPhase {
    const fn is_press_like(self) -> bool {
        matches!(self, Self::Press | Self::Repeat)
    }
}

impl From<KeyEventKind> for InputPhase {
    fn from(kind: KeyEventKind) -> Self {
        match kind {
            KeyEventKind::Press => Self::Press,
            KeyEventKind::Repeat => Self::Repeat,
            KeyEventKind::Release => Self::Release,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct KeyInput {
    pub key: InputKey,
    pub modifiers: InputModifiers,
    pub phase: InputPhase,
}

impl KeyInput {
    pub fn new(key: InputKey, modifiers: InputModifiers, phase: InputPhase) -> Self {
        Self {
            key,
            modifiers,
            phase,
        }
    }

    pub fn from_label(label: impl AsRef<str>) -> Self {
        let label = label.as_ref();
        let (key, modifiers) = match label {
            "Ctrl+C" => (
                InputKey::Character('c'),
                InputModifiers {
                    control: true,
                    ..InputModifiers::none()
                },
            ),
            "Enter" => (InputKey::Enter, InputModifiers::none()),
            "Esc" => (InputKey::Escape, InputModifiers::none()),
            "Backspace" => (InputKey::Backspace, InputModifiers::none()),
            "Tab" => (InputKey::Tab, InputModifiers::none()),
            "Shift+Tab" => (
                InputKey::BackTab,
                InputModifiers {
                    shift: true,
                    ..InputModifiers::none()
                },
            ),
            "Left" => (InputKey::Left, InputModifiers::none()),
            "Right" => (InputKey::Right, InputModifiers::none()),
            "Up" => (InputKey::Up, InputModifiers::none()),
            "Down" => (InputKey::Down, InputModifiers::none()),
            "Delete" => (InputKey::Delete, InputModifiers::none()),
            "Home" => (InputKey::Home, InputModifiers::none()),
            "End" => (InputKey::End, InputModifiers::none()),
            "PageUp" => (InputKey::PageUp, InputModifiers::none()),
            "PageDown" => (InputKey::PageDown, InputModifiers::none()),
            single if single.chars().count() == 1 => (
                InputKey::Character(single.chars().next().expect("single char")),
                InputModifiers::none(),
            ),
            other => (InputKey::Other(other.to_string()), InputModifiers::none()),
        };

        Self::new(key, modifiers, InputPhase::Press)
    }

    pub fn label(&self) -> String {
        if matches!(&self.key, InputKey::BackTab) {
            return "Shift+Tab".to_string();
        }

        if self.modifiers.control {
            if let InputKey::Character(character) = &self.key {
                return format!("Ctrl+{}", character.to_ascii_uppercase());
            }
        }

        let mut parts = Vec::new();
        if self.modifiers.control {
            parts.push("Ctrl");
        }
        if self.modifiers.alt {
            parts.push("Alt");
        }
        if self.modifiers.shift {
            parts.push("Shift");
        }

        let key = self.key.label();
        if parts.is_empty() {
            key
        } else {
            parts.push(key.as_str());
            parts.join("+")
        }
    }

    fn is_ctrl_c(&self) -> bool {
        matches!(&self.key, InputKey::Character('c' | 'C')) && self.modifiers.control
    }

    fn is_character(&self, expected: char) -> bool {
        matches!(&self.key, InputKey::Character(character) if *character == expected)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PointerButton {
    Left,
    Right,
    Middle,
}

impl PointerButton {
    const fn label(self) -> &'static str {
        match self {
            Self::Left => "Left",
            Self::Right => "Right",
            Self::Middle => "Middle",
        }
    }
}

impl From<MouseButton> for PointerButton {
    fn from(button: MouseButton) -> Self {
        match button {
            MouseButton::Left => Self::Left,
            MouseButton::Right => Self::Right,
            MouseButton::Middle => Self::Middle,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ScrollDirection {
    Down,
    Up,
    Left,
    Right,
}

impl ScrollDirection {
    const fn label(self) -> &'static str {
        match self {
            Self::Down => "Down",
            Self::Up => "Up",
            Self::Left => "Left",
            Self::Right => "Right",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DragDirection {
    Up,
    Down,
    Left,
    Right,
}

impl DragDirection {
    const fn label(self) -> &'static str {
        match self {
            Self::Up => "Up",
            Self::Down => "Down",
            Self::Left => "Left",
            Self::Right => "Right",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MouseInput {
    Down {
        button: PointerButton,
        coordinates: CellPosition,
        modifiers: InputModifiers,
    },
    Up {
        button: PointerButton,
        coordinates: CellPosition,
        modifiers: InputModifiers,
    },
    Drag {
        button: PointerButton,
        coordinates: CellPosition,
        modifiers: InputModifiers,
    },
    Moved {
        coordinates: CellPosition,
        modifiers: InputModifiers,
    },
    Scroll {
        direction: ScrollDirection,
        coordinates: CellPosition,
        modifiers: InputModifiers,
    },
}

impl MouseInput {
    pub fn coordinates(self) -> CellPosition {
        match self {
            Self::Down { coordinates, .. }
            | Self::Up { coordinates, .. }
            | Self::Drag { coordinates, .. }
            | Self::Moved { coordinates, .. }
            | Self::Scroll { coordinates, .. } => coordinates,
        }
    }

    pub fn scroll_direction(self) -> Option<ScrollDirection> {
        match self {
            Self::Scroll { direction, .. } => Some(direction),
            _ => None,
        }
    }

    pub fn summary(self) -> String {
        match self {
            Self::Down { button, .. } => format!("Mouse Down {}", button.label()),
            Self::Up { button, .. } => format!("Mouse Up {}", button.label()),
            Self::Drag { button, .. } => format!("Mouse Drag {}", button.label()),
            Self::Moved { .. } => "Mouse Moved".to_string(),
            Self::Scroll { direction, .. } => format!("Mouse Scroll {}", direction.label()),
        }
    }

    fn down_button(self) -> Option<PointerButton> {
        match self {
            Self::Down { button, .. } => Some(button),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputEvent {
    Key(KeyInput),
    Mouse(MouseInput),
    Resize { width: u16, height: u16 },
    FocusGained,
    FocusLost,
    Paste(String),
    Tick,
    Shutdown,
}

impl InputEvent {
    pub fn from_key_label(label: impl AsRef<str>) -> Self {
        Self::Key(KeyInput::from_label(label))
    }

    pub fn mouse_down(button: PointerButton, coordinates: CellPosition) -> Self {
        Self::Mouse(MouseInput::Down {
            button,
            coordinates,
            modifiers: InputModifiers::none(),
        })
    }

    pub fn mouse_up(button: PointerButton, coordinates: CellPosition) -> Self {
        Self::Mouse(MouseInput::Up {
            button,
            coordinates,
            modifiers: InputModifiers::none(),
        })
    }

    pub fn mouse_drag(button: PointerButton, coordinates: CellPosition) -> Self {
        Self::Mouse(MouseInput::Drag {
            button,
            coordinates,
            modifiers: InputModifiers::none(),
        })
    }

    pub fn mouse_moved(coordinates: CellPosition) -> Self {
        Self::Mouse(MouseInput::Moved {
            coordinates,
            modifiers: InputModifiers::none(),
        })
    }

    pub fn mouse_scroll(direction: ScrollDirection, coordinates: CellPosition) -> Self {
        Self::Mouse(MouseInput::Scroll {
            direction,
            coordinates,
            modifiers: InputModifiers::none(),
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ShellComponent {
    CompactHome,
    TopBar,
    Home,
    ClockButton,
    Clock,
    LoginUserList,
    LoginUsername,
    LoginPassword,
    SetupLanguage,
    SetupTimezone,
    SetupAdminUsername,
    SetupAdminPassword,
    SetupAdminPasswordConfirm,
    SetupAdminHint,
    SetupSubmit,
    BootstrapUsername,
    BootstrapPassword,
    Explorer,
    UserManagement,
    StatusBar,
    ExitDialog,
    TimeSyncDialog,
    ContextMenu,
}

impl ShellComponent {
    const fn label(self) -> &'static str {
        match self {
            Self::CompactHome => "CompactHome",
            Self::TopBar => "TopBar",
            Self::Home => "Home",
            Self::ClockButton => "ClockButton",
            Self::Clock => "Clock",
            Self::LoginUserList => "LoginUserList",
            Self::LoginUsername => "LoginUsername",
            Self::LoginPassword => "LoginPassword",
            Self::SetupLanguage => "SetupLanguage",
            Self::SetupTimezone => "SetupTimezone",
            Self::SetupAdminUsername => "SetupAdminUsername",
            Self::SetupAdminPassword => "SetupAdminPassword",
            Self::SetupAdminPasswordConfirm => "SetupAdminPasswordConfirm",
            Self::SetupAdminHint => "SetupAdminHint",
            Self::SetupSubmit => "SetupSubmit",
            Self::BootstrapUsername => "BootstrapUsername",
            Self::BootstrapPassword => "BootstrapPassword",
            Self::Explorer => "Explorer",
            Self::UserManagement => "UserManagement",
            Self::StatusBar => "StatusBar",
            Self::ExitDialog => "ExitDialog",
            Self::TimeSyncDialog => "TimeSyncDialog",
            Self::ContextMenu => "ContextMenu",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClickKind {
    Single,
    Double,
}

// Shell-local stand-in until the UI foundation exports hit testing and overlay
// ownership. Expected future imports: tundra_ui::{ComponentId, HitMap,
// HitRegion, OverlayCapture, OverlayLayer}.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShellHitRegion {
    pub component: ShellComponent,
    pub area: Rect,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellHitMap {
    terminal_size: CellPosition,
    generation: u64,
    regions: Vec<ShellHitRegion>,
}

impl ShellHitMap {
    fn new(terminal_size: CellPosition, generation: u64, regions: Vec<ShellHitRegion>) -> Self {
        Self {
            terminal_size,
            generation,
            regions,
        }
    }

    fn empty(terminal_size: CellPosition) -> Self {
        Self::new(terminal_size, 0, Vec::new())
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn terminal_size(&self) -> CellPosition {
        self.terminal_size
    }

    pub fn regions(&self) -> &[ShellHitRegion] {
        &self.regions
    }

    pub fn target_at(&self, coordinates: CellPosition) -> Option<ShellComponent> {
        self.regions
            .iter()
            .rev()
            .find(|region| rect_contains(region.area, coordinates))
            .map(|region| region.component)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShellPopup {
    pub owner: Option<ShellComponent>,
    pub anchor: CellPosition,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct DragTracker {
    button: PointerButton,
    last_coordinates: CellPosition,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UserManagementFormField {
    Username,
    DisplayName,
    Password,
}

impl UserManagementFormField {
    fn next(self) -> Self {
        match self {
            Self::Username => Self::DisplayName,
            Self::DisplayName => Self::Password,
            Self::Password => Self::Username,
        }
    }

    fn previous(self) -> Self {
        match self {
            Self::Username => Self::Password,
            Self::DisplayName => Self::Username,
            Self::Password => Self::DisplayName,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct UserManagementCreateForm {
    username: String,
    display_name: String,
    password: String,
    role: UserRole,
    focused_field: UserManagementFormField,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct UserManagementInfoForm {
    username: String,
    display_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct UserManagementPasswordForm {
    username: String,
    password: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum UserManagementMode {
    Browse,
    Create(UserManagementCreateForm),
    EditInfo(UserManagementInfoForm),
    Password(UserManagementPasswordForm),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExplorerInputMode {
    Browse,
    Search,
    NewFolder,
    NewTextFile,
    Rename,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShellCommand {
    Noop,
    Tick,
    Shutdown,
    RequestExit,
    ConfirmExit,
    CancelExit,
    FocusNext,
    FocusPrevious,
    AppendAuthChar(char),
    AuthBackspace,
    LoginPreviousUser,
    LoginNextUser,
    LoginPageUserUp,
    LoginPageUserDown,
    LoginFirstUser,
    LoginLastUser,
    LoginFocusUserList,
    LoginFocusPassword,
    SubmitLogin,
    SubmitBootstrapAdmin,
    SetupPreviousLanguage,
    SetupNextLanguage,
    SetupContinue,
    SetupPreviousTimezone,
    SetupNextTimezone,
    SetupPageTimezoneUp,
    SetupPageTimezoneDown,
    SetupFirstTimezone,
    SetupLastTimezone,
    SetupFocusNext,
    SetupFocusPrevious,
    AppendSetupAdminChar(char),
    SetupAdminBackspace,
    SubmitSetup,
    ActivateSetup {
        target: ShellComponent,
        coordinates: CellPosition,
    },
    ActivateLogin {
        target: ShellComponent,
        coordinates: CellPosition,
    },
    HomeEntryLeft,
    HomeEntryRight,
    HomeEntryUp,
    HomeEntryDown,
    HomeFirstEntry,
    HomeLastEntry,
    ActivateSelectedHomeEntry,
    SelectHomeEntryAt(CellPosition),
    ActivateHomeEntryAt(CellPosition, ClickKind),
    OpenExplorer,
    CloseExplorer,
    ExplorerNext,
    ExplorerPrevious,
    ExplorerOpenSelected,
    ExplorerOpenParent,
    ExplorerToggleHidden,
    ExplorerCopy,
    ExplorerCut,
    ExplorerPaste,
    ExplorerDelete,
    ExplorerConfirmDelete,
    ExplorerSelectAt(CellPosition, ClickKind),
    BeginExplorerSearch,
    BeginExplorerNewFolder,
    BeginExplorerNewTextFile,
    BeginExplorerRename,
    AppendExplorerChar(char),
    ExplorerBackspace,
    SubmitExplorerInput,
    CancelExplorerInput,
    OpenUserManagement,
    CloseUserManagement,
    OpenClock,
    CloseClock,
    UserManagementNext,
    UserManagementPrevious,
    CreateManagedUser(UserRole),
    EditManagedUserInfo,
    DisableManagedUser,
    UnlockManagedUser,
    ResetManagedPassword,
    CycleManagedRole,
    DeleteManagedUser,
    AppendUserManagementChar(char),
    UserManagementBackspace,
    UserManagementFocusNext,
    UserManagementFocusPrevious,
    SubmitUserManagementForm,
    CancelUserManagementForm,
    Hover(Option<ShellComponent>),
    Activate {
        target: ShellComponent,
        coordinates: CellPosition,
        click: ClickKind,
    },
    OpenContextMenu {
        target: Option<ShellComponent>,
        coordinates: CellPosition,
    },
    ClosePopup,
    CloseTimeSyncDialog,
    CaptureOverlayInput,
    RefreshHitMap {
        width: u16,
        height: u16,
    },
    RecordInput,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoutedTarget {
    Global,
    Component(ShellComponent),
    Modal(ShellComponent),
    Popup(ShellComponent),
    OutsidePopup,
    None,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoutedEvent {
    pub input: InputEvent,
    pub target: RoutedTarget,
    pub command: ShellCommand,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShortcutScope {
    Global,
    Screen(ShellScreen),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyBinding {
    pub key: InputKey,
    pub modifiers: InputModifiers,
}

impl From<&KeyInput> for KeyBinding {
    fn from(input: &KeyInput) -> Self {
        Self {
            key: input.key.clone(),
            modifiers: input.modifiers,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellShortcut {
    pub scope: ShortcutScope,
    pub binding: KeyBinding,
    pub command: ShellCommand,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShortcutConflict {
    pub scope: ShortcutScope,
    pub binding: KeyBinding,
    pub first: ShellCommand,
    pub second: ShellCommand,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellAction {
    Redraw,
    Exit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShellTerminalFlags {
    pub raw_mode: bool,
    pub alternate_screen: bool,
    pub mouse_capture: bool,
    pub cursor_restore_enabled: bool,
}

impl ShellTerminalFlags {
    const fn enabled() -> Self {
        Self {
            raw_mode: true,
            alternate_screen: true,
            mouse_capture: true,
            cursor_restore_enabled: true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TimedClick {
    target: Option<ShellComponent>,
    coordinates: CellPosition,
    at: Instant,
}

pub struct TerminalGuard<W: Write> {
    terminal: Terminal<CrosstermBackend<W>>,
    restored: bool,
}

impl<W: Write> TerminalGuard<W> {
    pub fn enter(mut output: W) -> io::Result<Self> {
        install_panic_restore_hook();

        enable_raw_mode()?;
        if let Err(error) = execute!(output, EnterAlternateScreen, EnableMouseCapture, Hide) {
            let _ = disable_raw_mode();
            return Err(error);
        }

        let terminal = Terminal::new(CrosstermBackend::new(output))?;

        Ok(Self {
            terminal,
            restored: false,
        })
    }

    pub fn terminal_mut(&mut self) -> &mut Terminal<CrosstermBackend<W>> {
        &mut self.terminal
    }

    pub fn restore(&mut self) -> io::Result<()> {
        if self.restored {
            return Ok(());
        }

        execute!(
            self.terminal.backend_mut(),
            Show,
            DisableMouseCapture,
            LeaveAlternateScreen
        )?;
        disable_raw_mode()?;
        self.restored = true;

        Ok(())
    }
}

impl<W: Write> Drop for TerminalGuard<W> {
    fn drop(&mut self) {
        let _ = self.restore();
    }
}

fn install_panic_restore_hook() {
    if PANIC_RESTORE_HOOK_INSTALLED
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return;
    }

    let previous_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic_info| {
        let _ = disable_raw_mode();
        let mut stderr = io::stderr();
        let _ = execute!(stderr, Show, DisableMouseCapture, LeaveAlternateScreen);
        previous_hook(panic_info);
    }));
}

#[derive(Debug, Clone)]
struct ShellNetworkClock(NetworkClock);

impl ShellNetworkClock {
    fn new(timezone_id: Option<String>) -> Self {
        Self(NetworkClock::new(timezone_id))
    }

    fn apply_sync(&mut self, result: TimeSyncResult) {
        self.0.apply_sync(result);
    }

    fn current(&self) -> tundra_weathr::network_clock::ClockDisplay {
        self.0.current()
    }
}

impl PartialEq for ShellNetworkClock {
    fn eq(&self, _other: &Self) -> bool {
        true
    }
}

impl Eq for ShellNetworkClock {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellState {
    home_mode: ShellHomeMode,
    ascii_assets: tundra_ui::RuntimeAsciiAssets,
    screen_stack: Vec<ShellScreen>,
    storage_manager: Option<StorageManager>,
    network_clock: ShellNetworkClock,
    clock_timezone_id: Option<String>,
    last_time_sync_utc: Option<DateTime<Utc>>,
    time_sync_dialog_visible: bool,
    time_sync_failure_message: Option<String>,
    auth_session: Option<AuthSession>,
    requested_debug_mode: bool,
    debug_policy: DebugPolicy,
    login_users: Vec<ShellLoginUser>,
    login_selected_user: usize,
    login_user_window_start: usize,
    login_username: String,
    login_password: String,
    setup_step: tundra_ui::SetupStep,
    setup_selected_language_index: usize,
    setup_selected_timezone_index: usize,
    setup_admin_username: String,
    setup_admin_password: String,
    setup_admin_password_confirm: String,
    setup_admin_password_hint: String,
    setup_focused_field: tundra_ui::SetupField,
    setup_timezone_window_start: usize,
    bootstrap_username: String,
    bootstrap_password: String,
    user_management_users: Vec<UserAccount>,
    user_management_selected: usize,
    user_management_message: Option<String>,
    user_management_mode: UserManagementMode,
    selected_home_entry_index: usize,
    explorer_state: Option<ExplorerState>,
    explorer_input_mode: ExplorerInputMode,
    explorer_input: String,
    terminal_size: (u16, u16),
    terminal_flags: ShellTerminalFlags,
    focused_component: ShellComponent,
    hovered_component: Option<ShellComponent>,
    active_popup: Option<ShellPopup>,
    hit_map: ShellHitMap,
    hit_map_generation: u64,
    tick_count: u64,
    status_message: String,
    toast_message: Option<String>,
    error_message: Option<String>,
    shutdown_requested: bool,
    last_command: Option<ShellCommand>,
    last_routed_target: Option<RoutedTarget>,
    last_key_event: Option<String>,
    last_mouse_event: Option<String>,
    last_resize_event: Option<String>,
    mouse_coordinates: Option<(u16, u16)>,
    mouse_scroll_direction: Option<String>,
    mouse_drag_direction: Option<String>,
    platform_capability_summary: String,
    last_click: Option<TimedClick>,
    drag_tracker: Option<DragTracker>,
}

impl ShellState {
    pub fn new(launch_config: ShellLaunchConfig, terminal_size: (u16, u16)) -> Self {
        Self::new_with_startup(
            launch_config,
            terminal_size,
            ShellStartupState::current_process_defaults(),
        )
    }

    pub fn try_new(
        launch_config: ShellLaunchConfig,
        terminal_size: (u16, u16),
    ) -> Result<Self, tundra_ui::AssetError> {
        Self::try_new_with_startup(
            launch_config,
            terminal_size,
            ShellStartupState::current_process_defaults(),
        )
    }

    pub fn new_with_startup(
        launch_config: ShellLaunchConfig,
        terminal_size: (u16, u16),
        startup: ShellStartupState,
    ) -> Self {
        let ascii_assets =
            tundra_ui::RuntimeAsciiAssets::load_default().expect("default ASCII assets must load");
        Self::new_with_startup_and_assets(launch_config, terminal_size, startup, ascii_assets)
    }

    pub fn try_new_with_startup(
        launch_config: ShellLaunchConfig,
        terminal_size: (u16, u16),
        startup: ShellStartupState,
    ) -> Result<Self, tundra_ui::AssetError> {
        let ascii_assets = tundra_ui::RuntimeAsciiAssets::load_default()?;
        Ok(Self::new_with_startup_and_assets(
            launch_config,
            terminal_size,
            startup,
            ascii_assets,
        ))
    }

    pub fn new_with_startup_and_assets(
        launch_config: ShellLaunchConfig,
        terminal_size: (u16, u16),
        startup: ShellStartupState,
        ascii_assets: tundra_ui::RuntimeAsciiAssets,
    ) -> Self {
        let home_mode = resolved_home_mode(launch_config, &startup);
        let auth_gate_enabled = startup.storage_manager.is_some();
        let initial_screen = if auth_gate_enabled {
            if startup.auth_bootstrap_required {
                ShellScreen::FirstRunSetup
            } else {
                ShellScreen::Login
            }
        } else {
            ShellScreen::Home
        };
        let initial_focus = match initial_screen {
            ShellScreen::FirstRunSetup => ShellComponent::SetupLanguage,
            ShellScreen::BootstrapAdmin => ShellComponent::BootstrapUsername,
            ShellScreen::Login => ShellComponent::LoginUserList,
            _ => ShellComponent::Home,
        };
        let login_users = startup.login_users.clone();
        let login_selected_user = default_login_user_index(&login_users);
        let login_username = login_users
            .get(login_selected_user)
            .map(|user| user.username.clone())
            .unwrap_or_default();
        let clock_timezone_id = startup_clock_timezone_id(&startup);
        let network_clock = ShellNetworkClock::new(clock_timezone_id.clone());

        let mut state = Self {
            home_mode,
            ascii_assets,
            screen_stack: vec![initial_screen],
            storage_manager: startup.storage_manager.clone(),
            network_clock,
            clock_timezone_id,
            last_time_sync_utc: None,
            time_sync_dialog_visible: false,
            time_sync_failure_message: None,
            auth_session: None,
            requested_debug_mode: launch_config.home_mode_override == HomeModeOverride::Debug,
            debug_policy: startup.debug_policy,
            login_users,
            login_selected_user,
            login_user_window_start: 0,
            login_username,
            login_password: String::new(),
            setup_step: tundra_ui::SetupStep::Language,
            setup_selected_language_index: 0,
            setup_selected_timezone_index: 0,
            setup_admin_username: String::new(),
            setup_admin_password: String::new(),
            setup_admin_password_confirm: String::new(),
            setup_admin_password_hint: String::new(),
            setup_focused_field: tundra_ui::SetupField::LanguageList,
            setup_timezone_window_start: 0,
            bootstrap_username: String::new(),
            bootstrap_password: String::new(),
            user_management_users: Vec::new(),
            user_management_selected: 0,
            user_management_message: None,
            user_management_mode: UserManagementMode::Browse,
            selected_home_entry_index: 0,
            explorer_state: None,
            explorer_input_mode: ExplorerInputMode::Browse,
            explorer_input: String::new(),
            terminal_size,
            terminal_flags: ShellTerminalFlags::enabled(),
            focused_component: initial_focus,
            hovered_component: None,
            active_popup: None,
            hit_map: ShellHitMap::empty(terminal_size),
            hit_map_generation: 0,
            tick_count: 0,
            status_message: "Ready".to_string(),
            toast_message: startup
                .storage_report
                .has_recovery_warnings()
                .then(|| "Storage recovered defaults".to_string()),
            error_message: None,
            shutdown_requested: false,
            last_command: None,
            last_routed_target: None,
            last_key_event: None,
            last_mouse_event: None,
            last_resize_event: None,
            mouse_coordinates: None,
            mouse_scroll_direction: None,
            mouse_drag_direction: None,
            platform_capability_summary: platform_capability_summary(
                startup.platform_kind,
                &startup.platform_capabilities,
            ),
            last_click: None,
            drag_tracker: None,
        };
        state.refresh_hit_map();
        if !auth_gate_enabled && let Some(restored_session) = startup.restored_session.as_ref() {
            state.apply_restored_session(restored_session);
        }
        state
    }

    pub fn sanitized_session_state(&self) -> ShellRestoredSession {
        let focused_component = if self.focus_order().contains(&self.focused_component) {
            self.focused_component
        } else {
            self.focus_order()
                .first()
                .copied()
                .unwrap_or(ShellComponent::Home)
        };

        ShellRestoredSession {
            active_screen: ShellScreen::Home,
            focused_component,
            display_mode: self.home_mode,
            active_popup: None,
        }
    }

    fn legacy_default_home_mode(launch_config: ShellLaunchConfig) -> ShellHomeMode {
        match launch_config.home_mode_override {
            HomeModeOverride::Debug => ShellHomeMode::Debug,
            HomeModeOverride::BuildDefault => {
                if cfg!(debug_assertions) {
                    ShellHomeMode::Debug
                } else {
                    ShellHomeMode::User
                }
            }
        }
    }

    pub fn new_for_home_mode(
        launch_config: ShellLaunchConfig,
        terminal_size: (u16, u16),
        home_mode: ShellHomeMode,
    ) -> Self {
        let mut state = Self::new(launch_config, terminal_size);
        state.home_mode = home_mode;
        state
    }

    pub fn to_home_view_model(&self) -> tundra_ui::HomeViewModel {
        match self.home_mode {
            ShellHomeMode::Debug => {
                tundra_ui::HomeViewModel::debug(tundra_ui::DebugDiagnosticsViewModel {
                    tick_count: self.tick_count,
                    last_key_event: self.last_key_event.clone(),
                    last_mouse_event: self.last_mouse_event.clone(),
                    last_resize_event: self.last_resize_event.clone(),
                    mouse_coordinates: self.mouse_coordinates,
                    scroll_direction: self.mouse_scroll_direction.clone(),
                    drag_direction: self.mouse_drag_direction.clone(),
                    terminal_flags: terminal_flag_labels(self.terminal_flags),
                    platform_capability_summary: self.platform_capability_summary.clone(),
                })
            }
            ShellHomeMode::User => {
                let user = self
                    .auth_session
                    .as_ref()
                    .map(|session| session.username.as_str())
                    .unwrap_or("Guest");
                tundra_ui::HomeViewModel::user_with_selection_and_icon_assets(
                    user,
                    self.current_time_label(),
                    self.user_home_entries(),
                    self.selected_home_entry_index(),
                    self.ascii_assets.clone(),
                )
            }
        }
    }

    pub fn to_clock_view_model(&self) -> tundra_ui::ClockViewModel {
        tundra_ui::ClockViewModel {
            current_time: self.current_time_label(),
        }
    }

    pub fn to_time_sync_dialog_view_model(&self) -> Option<tundra_ui::TimeSyncDialogViewModel> {
        self.time_sync_dialog_visible
            .then(tundra_ui::TimeSyncDialogViewModel::new)
    }

    pub fn to_login_view_model(&self) -> tundra_ui::LoginViewModel {
        tundra_ui::LoginViewModel::new(
            self.login_users
                .iter()
                .map(|user| tundra_ui::LoginUserOptionViewModel {
                    username: user.username.clone(),
                    display_name: user.display_name.clone(),
                    role: user.role.clone(),
                    enabled: user.enabled,
                    locked: user
                        .locked_until_epoch_ms
                        .map(|locked_until| locked_until > unix_millis())
                        .unwrap_or(false),
                })
                .collect(),
            self.login_selected_user,
            self.login_user_window_start,
            self.login_password.chars().count(),
            match self.focused_component {
                ShellComponent::LoginPassword => tundra_ui::LoginField::Password,
                _ => tundra_ui::LoginField::UserList,
            },
            self.error_message.clone(),
        )
    }

    pub fn to_bootstrap_admin_view_model(&self) -> tundra_ui::BootstrapAdminViewModel {
        tundra_ui::BootstrapAdminViewModel::new(
            self.bootstrap_username.clone(),
            self.bootstrap_password.chars().count(),
            match self.focused_component {
                ShellComponent::BootstrapPassword => tundra_ui::AuthField::Password,
                _ => tundra_ui::AuthField::Username,
            },
            self.error_message.clone(),
        )
    }

    pub fn to_setup_view_model(&self) -> tundra_ui::SetupViewModel {
        let password_requirements = setup_password_requirements(
            &self.setup_admin_username,
            &self.setup_admin_password,
            &self.setup_admin_password_confirm,
        );
        let can_submit = !self.setup_admin_username.trim().is_empty()
            && password_requirements
                .iter()
                .all(|requirement| requirement.met);

        tundra_ui::SetupViewModel {
            step: self.setup_step.clone(),
            languages: tundra_ui::setup_language_options(),
            timezones: tundra_ui::setup_timezone_options(),
            selected_language_index: self.setup_selected_language_index,
            selected_timezone_index: self.setup_selected_timezone_index,
            timezone_window_start: self.setup_timezone_window_start,
            admin_username: self.setup_admin_username.clone(),
            admin_password_len: self.setup_admin_password.chars().count(),
            admin_password_confirm_len: self.setup_admin_password_confirm.chars().count(),
            password_requirements,
            password_hint: self.setup_admin_password_hint.clone(),
            focused_field: self.setup_focused_field.clone(),
            can_submit,
            error: self.error_message.clone(),
        }
    }

    pub fn to_user_management_view_model(&self) -> tundra_ui::UserManagementViewModel {
        tundra_ui::UserManagementViewModel::new(
            self.auth_session
                .as_ref()
                .map(|session| session.username.clone())
                .unwrap_or_else(|| "Guest".to_string()),
            self.user_management_users
                .iter()
                .map(|user| tundra_ui::UserManagementUserViewModel {
                    username: user.username.clone(),
                    display_name: user.display_name.clone(),
                    role: user.role.as_str().to_string(),
                    enabled: user.enabled,
                    locked: user
                        .locked_until_epoch_ms
                        .map(|locked_until| locked_until > unix_millis())
                        .unwrap_or(false),
                })
                .collect(),
            self.user_management_selected,
            self.user_management_message.clone(),
            self.can_manage_all_users(),
            self.user_management_form_view_model(),
        )
    }

    pub fn to_explorer_view_model(&self) -> tundra_ui::ExplorerViewModel {
        let Some(state) = self.explorer_state.as_ref() else {
            return tundra_ui::ExplorerViewModel::new("Explorer unavailable", Vec::new(), None);
        };

        let entries = state
            .entries
            .iter()
            .enumerate()
            .map(|(index, entry)| tundra_ui::ExplorerEntryViewModel {
                name: entry.name.clone(),
                kind: entry.kind.label().to_string(),
                size: (entry.kind == tundra_apps::explorer::ExplorerEntryKind::File)
                    .then(|| entry.size.to_string()),
                modified: entry.modified.map(system_time_label),
                attributes: explorer_attribute_labels(&entry.attributes),
                selected: index == state.selected_index,
            })
            .collect::<Vec<_>>();
        let selected_index = (!entries.is_empty()).then_some(state.selected_index);
        let mut model = tundra_ui::ExplorerViewModel::new(
            state.current_path.display().to_string(),
            entries,
            selected_index,
        );
        model.show_hidden = state.show_hidden;
        model.message = state.message.clone();
        model.error = state.error.clone();
        model.search = if self.explorer_input_mode == ExplorerInputMode::Search {
            Some(tundra_ui::ExplorerSearchViewModel::new(
                self.explorer_input.clone(),
                true,
                Some(state.entries.len()),
            ))
        } else if !state.query.is_empty() {
            Some(tundra_ui::ExplorerSearchViewModel::new(
                state.query.clone(),
                false,
                Some(state.entries.len()),
            ))
        } else {
            None
        };
        model.pending_dialog = state.pending_dialog.as_ref().map(|dialog| {
            tundra_ui::ExplorerDialogViewModel::new(
                dialog.title.clone(),
                dialog.message.clone(),
                "Y / Enter: move",
                "N / Esc: cancel",
            )
        });

        if self.explorer_input_mode != ExplorerInputMode::Browse
            && self.explorer_input_mode != ExplorerInputMode::Search
        {
            model.message = Some(format!(
                "{}: {}",
                explorer_input_prompt(self.explorer_input_mode),
                self.explorer_input
            ));
        }

        model
    }

    fn can_manage_all_users(&self) -> bool {
        matches!(
            self.auth_session.as_ref().map(|session| session.role),
            Some(UserRole::Admin | UserRole::Debug)
        )
    }

    fn user_management_form_view_model(&self) -> Option<tundra_ui::UserManagementFormViewModel> {
        match &self.user_management_mode {
            UserManagementMode::Browse => None,
            UserManagementMode::Create(form) => Some(tundra_ui::UserManagementFormViewModel {
                kind: tundra_ui::UserManagementFormKind::Create,
                title: "Create user".to_string(),
                username: form.username.clone(),
                display_name: form.display_name.clone(),
                role: form.role.as_str().to_string(),
                password_len: form.password.chars().count(),
                focused_field: to_ui_user_management_field(form.focused_field),
            }),
            UserManagementMode::EditInfo(form) => Some(tundra_ui::UserManagementFormViewModel {
                kind: tundra_ui::UserManagementFormKind::EditInfo,
                title: "Edit user info".to_string(),
                username: form.username.clone(),
                display_name: form.display_name.clone(),
                role: String::new(),
                password_len: 0,
                focused_field: tundra_ui::UserManagementField::DisplayName,
            }),
            UserManagementMode::Password(form) => Some(tundra_ui::UserManagementFormViewModel {
                kind: tundra_ui::UserManagementFormKind::Password,
                title: "Set password".to_string(),
                username: form.username.clone(),
                display_name: String::new(),
                role: String::new(),
                password_len: form.password.chars().count(),
                focused_field: tundra_ui::UserManagementField::Password,
            }),
        }
    }

    pub fn to_shell_chrome_view_model(&self) -> tundra_ui::ShellChromeViewModel {
        let status = if self.home_mode == ShellHomeMode::Debug
            && self.active_screen() == ShellScreen::Home
        {
            format!(
                "{} | Key: {} | Mouse: {} | Resize: {}",
                self.status_message,
                self.last_key_event.as_deref().unwrap_or("none"),
                self.last_mouse_event.as_deref().unwrap_or("none"),
                self.last_resize_event.as_deref().unwrap_or("none")
            )
        } else {
            self.status_message.clone()
        };
        tundra_ui::ShellChromeViewModel {
            app_name: "TundraUX 3".to_string(),
            build_mode: build_mode_label().to_string(),
            display_mode: self.home_display_mode(),
            terminal_size: self.terminal_size,
            screen_stack: self
                .screen_stack
                .iter()
                .map(|screen| format!("{screen:?}"))
                .collect(),
            status: tundra_ui::StatusViewModel {
                status,
                toast: self.toast_message.clone(),
                error: self.error_message.clone(),
                time_button_label: self.status_time_button_label(),
                time_button_selected: self.time_button_selected(),
            },
        }
    }

    fn append_auth_char(&mut self, character: char) {
        match self.focused_component {
            ShellComponent::LoginPassword => self.login_password.push(character),
            ShellComponent::BootstrapUsername => self.bootstrap_username.push(character),
            ShellComponent::BootstrapPassword => self.bootstrap_password.push(character),
            _ => {}
        }
        self.error_message = None;
    }

    fn auth_backspace(&mut self) {
        match self.focused_component {
            ShellComponent::LoginPassword => {
                self.login_password.pop();
            }
            ShellComponent::BootstrapUsername => {
                self.bootstrap_username.pop();
            }
            ShellComponent::BootstrapPassword => {
                self.bootstrap_password.pop();
            }
            _ => {}
        }
        self.error_message = None;
    }

    fn selected_login_username(&self) -> Option<&str> {
        self.login_users
            .get(self.login_selected_user)
            .map(|user| user.username.as_str())
    }

    fn selected_login_password_hint(&self) -> Option<&str> {
        self.login_users
            .get(self.login_selected_user)?
            .password_hint
            .as_deref()
    }

    fn sync_login_selection(&mut self) {
        if self.login_users.is_empty() {
            self.login_selected_user = 0;
            self.login_user_window_start = 0;
            self.login_username.clear();
            return;
        }

        self.login_selected_user = self.login_selected_user.min(self.login_users.len() - 1);
        self.login_username = self.login_users[self.login_selected_user].username.clone();
        self.sync_login_user_window();
    }

    fn sync_login_user_window(&mut self) {
        let count = self.login_users.len();
        if count == 0 {
            self.login_user_window_start = 0;
            return;
        }

        let visible_rows = self.login_user_visible_row_count().min(count).max(1);
        let max_start = count.saturating_sub(visible_rows);
        self.login_user_window_start = self.login_user_window_start.min(max_start);

        if self.login_selected_user < self.login_user_window_start {
            self.login_user_window_start = self.login_selected_user;
        }

        let window_end = self.login_user_window_start.saturating_add(visible_rows);
        if self.login_selected_user >= window_end {
            self.login_user_window_start = self
                .login_selected_user
                .saturating_add(1)
                .saturating_sub(visible_rows)
                .min(max_start);
        }
    }

    fn login_user_visible_row_count(&self) -> usize {
        login_user_visible_row_count(self.terminal_size).max(1)
    }

    fn select_login_user_delta(&mut self, delta: isize) {
        if self.login_users.is_empty() {
            self.sync_login_selection();
            return;
        }

        let current = self.login_selected_user.min(self.login_users.len() - 1) as isize;
        self.login_selected_user =
            (current + delta).clamp(0, self.login_users.len() as isize - 1) as usize;
        self.login_password.clear();
        self.error_message = None;
        self.sync_login_selection();
    }

    fn select_first_login_user(&mut self) {
        self.login_selected_user = 0;
        self.login_password.clear();
        self.error_message = None;
        self.sync_login_selection();
    }

    fn select_last_login_user(&mut self) {
        if self.login_users.is_empty() {
            self.sync_login_selection();
            return;
        }

        self.login_selected_user = self.login_users.len() - 1;
        self.login_password.clear();
        self.error_message = None;
        self.sync_login_selection();
    }

    fn select_login_user_at(&mut self, index: usize) {
        if index >= self.login_users.len() {
            return;
        }

        self.login_selected_user = index;
        self.login_password.clear();
        self.error_message = None;
        self.sync_login_selection();
    }

    fn refresh_login_users_from_storage(&mut self) -> Result<(), StorageError> {
        let Some(storage) = self.storage_manager.clone() else {
            return Ok(());
        };
        let previous_username = self.selected_login_username().map(str::to_string);
        let users = storage.load_users()?;
        self.login_users = users
            .users
            .iter()
            .map(ShellLoginUser::from_record)
            .collect();
        self.login_selected_user = previous_username
            .as_deref()
            .and_then(|username| {
                self.login_users
                    .iter()
                    .position(|user| user.username.eq_ignore_ascii_case(username))
            })
            .unwrap_or_else(|| default_login_user_index(&self.login_users));
        self.sync_login_selection();
        Ok(())
    }

    fn setup_next_language(&mut self) {
        let count = tundra_ui::setup_language_options().len();
        if count == 0 {
            return;
        }
        self.setup_selected_language_index = (self.setup_selected_language_index + 1) % count;
        self.error_message = None;
    }

    fn setup_previous_language(&mut self) {
        let count = tundra_ui::setup_language_options().len();
        if count == 0 {
            return;
        }
        self.setup_selected_language_index =
            (self.setup_selected_language_index + count - 1) % count;
        self.error_message = None;
    }

    fn setup_select_timezone_delta(&mut self, delta: isize) {
        let count = tundra_ui::setup_timezone_options().len();
        if count == 0 {
            self.setup_timezone_window_start = 0;
            return;
        }

        let current = self.setup_selected_timezone_index.min(count - 1) as isize;
        let next = (current + delta).clamp(0, count as isize - 1) as usize;
        self.setup_selected_timezone_index = next;
        self.sync_setup_timezone_window();
        self.error_message = None;
    }

    fn setup_select_first_timezone(&mut self) {
        self.setup_selected_timezone_index = 0;
        self.sync_setup_timezone_window();
        self.error_message = None;
    }

    fn setup_select_last_timezone(&mut self) {
        let count = tundra_ui::setup_timezone_options().len();
        if count == 0 {
            return;
        }
        self.setup_selected_timezone_index = count - 1;
        self.sync_setup_timezone_window();
        self.error_message = None;
    }

    fn sync_setup_timezone_window(&mut self) {
        let count = tundra_ui::setup_timezone_options().len();
        if count == 0 {
            self.setup_selected_timezone_index = 0;
            self.setup_timezone_window_start = 0;
            return;
        }

        self.setup_selected_timezone_index = self.setup_selected_timezone_index.min(count - 1);
        let visible_rows = self.setup_timezone_visible_row_count().min(count).max(1);
        let max_start = count.saturating_sub(visible_rows);
        self.setup_timezone_window_start = self.setup_timezone_window_start.min(max_start);

        if self.setup_selected_timezone_index < self.setup_timezone_window_start {
            self.setup_timezone_window_start = self.setup_selected_timezone_index;
        }

        let window_end = self
            .setup_timezone_window_start
            .saturating_add(visible_rows);
        if self.setup_selected_timezone_index >= window_end {
            self.setup_timezone_window_start = self
                .setup_selected_timezone_index
                .saturating_add(1)
                .saturating_sub(visible_rows)
                .min(max_start);
        }
    }

    fn setup_timezone_visible_row_count(&self) -> usize {
        setup_timezone_visible_row_count(self.terminal_size).max(1)
    }

    fn setup_continue(&mut self) {
        match self.setup_step {
            tundra_ui::SetupStep::Language => {
                self.setup_step = tundra_ui::SetupStep::Timezone;
                self.setup_focused_field = tundra_ui::SetupField::TimezoneList;
                self.focused_component = ShellComponent::SetupTimezone;
                self.sync_setup_timezone_window();
            }
            tundra_ui::SetupStep::Timezone => {
                self.setup_step = tundra_ui::SetupStep::Admin;
                self.setup_focused_field = tundra_ui::SetupField::AdminUsername;
                self.focused_component = ShellComponent::SetupAdminUsername;
            }
            tundra_ui::SetupStep::Admin => {}
        }
        self.error_message = None;
        self.refresh_hit_map();
    }

    fn move_setup_admin_focus(&mut self, direction: i8) {
        let order = [
            (
                tundra_ui::SetupField::AdminUsername,
                ShellComponent::SetupAdminUsername,
            ),
            (
                tundra_ui::SetupField::AdminPassword,
                ShellComponent::SetupAdminPassword,
            ),
            (
                tundra_ui::SetupField::AdminPasswordConfirm,
                ShellComponent::SetupAdminPasswordConfirm,
            ),
            (
                tundra_ui::SetupField::PasswordHint,
                ShellComponent::SetupAdminHint,
            ),
            (tundra_ui::SetupField::Submit, ShellComponent::SetupSubmit),
        ];
        let next = match order
            .iter()
            .position(|(field, _)| *field == self.setup_focused_field)
        {
            Some(current) => {
                (current as isize + direction as isize).rem_euclid(order.len() as isize) as usize
            }
            None if direction < 0 => order.len().saturating_sub(1),
            None => 0,
        };
        let (field, component) = order[next as usize];
        self.setup_focused_field = field;
        self.focused_component = component;
        self.error_message = None;
    }

    fn focus_setup_component(&mut self, component: ShellComponent) {
        if !setup_component_active_for_step(component, self.setup_step) {
            return;
        }

        let Some(field) = setup_field_for_component(component) else {
            return;
        };

        self.setup_focused_field = field;
        self.focused_component = component;
    }

    fn setup_active_key_component(&self) -> ShellComponent {
        match self.setup_step {
            tundra_ui::SetupStep::Language => ShellComponent::SetupLanguage,
            tundra_ui::SetupStep::Timezone => ShellComponent::SetupTimezone,
            tundra_ui::SetupStep::Admin => {
                let component = setup_component_for_field(self.setup_focused_field);
                if setup_component_active_for_step(component, self.setup_step) {
                    component
                } else {
                    ShellComponent::SetupAdminUsername
                }
            }
        }
    }

    fn append_setup_admin_char(&mut self, character: char) {
        match self.setup_focused_field {
            tundra_ui::SetupField::AdminUsername => self.setup_admin_username.push(character),
            tundra_ui::SetupField::AdminPassword => self.setup_admin_password.push(character),
            tundra_ui::SetupField::AdminPasswordConfirm => {
                self.setup_admin_password_confirm.push(character);
            }
            tundra_ui::SetupField::PasswordHint => self.setup_admin_password_hint.push(character),
            _ => {}
        }
        self.error_message = None;
    }

    fn setup_admin_backspace(&mut self) {
        match self.setup_focused_field {
            tundra_ui::SetupField::AdminUsername => {
                self.setup_admin_username.pop();
            }
            tundra_ui::SetupField::AdminPassword => {
                self.setup_admin_password.pop();
            }
            tundra_ui::SetupField::AdminPasswordConfirm => {
                self.setup_admin_password_confirm.pop();
            }
            tundra_ui::SetupField::PasswordHint => {
                self.setup_admin_password_hint.pop();
            }
            _ => {}
        }
        self.error_message = None;
    }

    fn activate_setup(&mut self, target: ShellComponent, coordinates: CellPosition) {
        match self.setup_step {
            tundra_ui::SetupStep::Language => {
                if target == ShellComponent::SetupLanguage
                    && let Some(index) = self.setup_language_index_at(coordinates)
                {
                    self.setup_selected_language_index = index;
                    self.error_message = None;
                }
                self.focus_setup_component(ShellComponent::SetupLanguage);
            }
            tundra_ui::SetupStep::Timezone => {
                if target == ShellComponent::SetupTimezone
                    && let Some(index) = self.setup_timezone_index_at(coordinates)
                {
                    self.setup_selected_timezone_index = index;
                    self.sync_setup_timezone_window();
                    self.error_message = None;
                }
                self.focus_setup_component(ShellComponent::SetupTimezone);
            }
            tundra_ui::SetupStep::Admin => {
                self.focus_setup_component(target);
                if target == ShellComponent::SetupSubmit {
                    self.submit_first_run_setup();
                }
            }
        }
    }

    fn activate_login(&mut self, target: ShellComponent, coordinates: CellPosition) {
        match target {
            ShellComponent::LoginUserList => {
                if let Some(index) = self.login_user_index_at(coordinates) {
                    self.select_login_user_at(index);
                }
                self.focused_component = ShellComponent::LoginPassword;
            }
            ShellComponent::LoginUsername => {
                self.focused_component = ShellComponent::LoginUserList;
            }
            ShellComponent::LoginPassword => {
                self.focused_component = ShellComponent::LoginPassword;
            }
            _ => {}
        }
    }

    fn setup_language_index_at(&self, coordinates: CellPosition) -> Option<usize> {
        let row = setup_language_list_row_at(self.terminal_size, coordinates)?;
        (row < tundra_ui::setup_language_options().len()).then_some(row)
    }

    fn setup_timezone_index_at(&self, coordinates: CellPosition) -> Option<usize> {
        let row = setup_timezone_list_row_at(self.terminal_size, coordinates)?;
        let count = tundra_ui::setup_timezone_options().len();
        if count == 0 {
            return None;
        }

        let visible_rows = self.setup_timezone_visible_row_count().min(count);
        let start = self
            .setup_timezone_window_start
            .min(count.saturating_sub(visible_rows));
        let index = start.saturating_add(row);
        (row < visible_rows && index < count).then_some(index)
    }

    fn login_user_index_at(&self, coordinates: CellPosition) -> Option<usize> {
        let row = login_user_list_row_at(self.terminal_size, coordinates)?;
        let count = self.login_users.len();
        if count == 0 {
            return None;
        }

        let visible_rows = self.login_user_visible_row_count().min(count);
        let start = self
            .login_user_window_start
            .min(count.saturating_sub(visible_rows));
        let index = start.saturating_add(row);
        (row < visible_rows && index < count).then_some(index)
    }

    fn selected_setup_language_value(&self) -> String {
        let options = tundra_ui::setup_language_options();
        setup_language_code_at(&options, self.setup_selected_language_index)
            .unwrap_or_else(|| "en-US".to_string())
    }

    fn selected_setup_timezone_value(&self) -> String {
        let options = tundra_ui::setup_timezone_options();
        setup_timezone_id_at(&options, self.setup_selected_timezone_index)
            .unwrap_or_else(|| "UTC".to_string())
    }

    fn submit_login(&mut self) {
        let Some(storage) = self.storage_manager.clone() else {
            self.error_message = Some("Storage unavailable".to_string());
            return;
        };
        let Some(username) = self.selected_login_username().map(str::to_string) else {
            self.error_message = Some("No user selected".to_string());
            self.status_message = "Login failed".to_string();
            return;
        };
        let password_hint = self.selected_login_password_hint().map(str::to_string);
        let mut sessions = SessionService::new(storage);
        match sessions.login(&username, &self.login_password) {
            Ok(session) => self.complete_login(session),
            Err(error) => {
                self.error_message = Some(login_error_message(&error, password_hint.as_deref()));
                self.status_message = "Login failed".to_string();
                let _ = self.refresh_login_users_from_storage();
            }
        }
    }

    fn submit_bootstrap_admin(&mut self) {
        let Some(storage) = self.storage_manager.clone() else {
            self.error_message = Some("Storage unavailable".to_string());
            return;
        };
        let users = UserService::with_debug_policy(storage.clone(), self.debug_policy);
        match users.bootstrap_admin(&self.bootstrap_username, &self.bootstrap_password) {
            Ok(_) => {
                self.login_username = self.bootstrap_username.clone();
                self.login_password = self.bootstrap_password.clone();
                let mut sessions = SessionService::new(storage);
                match sessions.login(&self.login_username, &self.login_password) {
                    Ok(session) => self.complete_login(session),
                    Err(error) => {
                        self.error_message = Some(format_core_error(&error));
                        self.status_message = "Login failed".to_string();
                    }
                }
            }
            Err(error) => {
                self.error_message = Some(format_core_error(&error));
                self.status_message = "Admin bootstrap failed".to_string();
            }
        }
    }

    fn submit_first_run_setup(&mut self) {
        let Some(storage) = self.storage_manager.clone() else {
            self.error_message = Some("Storage unavailable".to_string());
            return;
        };

        let username = self.setup_admin_username.trim().to_string();
        let password = self.setup_admin_password.clone();
        if password != self.setup_admin_password_confirm {
            self.setup_focused_field = tundra_ui::SetupField::AdminPasswordConfirm;
            self.focused_component = ShellComponent::SetupAdminPasswordConfirm;
            self.error_message = Some("Passwords do not match".to_string());
            self.status_message = "Setup incomplete".to_string();
            return;
        }

        let hint = self.setup_admin_password_hint.trim().to_string();
        let hint = (!hint.is_empty()).then_some(hint);

        let mut config = match storage.load_config() {
            Ok(config) => config,
            Err(error) => {
                self.error_message = Some(error.to_string());
                self.status_message = "Setup failed".to_string();
                return;
            }
        };
        config.language = self.selected_setup_language_value();
        config.timezone = self.selected_setup_timezone_value();
        let selected_timezone = config.timezone.clone();
        if let Err(error) = storage.save_config(&config) {
            self.error_message = Some(error.to_string());
            self.status_message = "Setup failed".to_string();
            return;
        }
        self.set_clock_timezone(Some(selected_timezone));

        let users = UserService::with_debug_policy(storage.clone(), self.debug_policy);
        match users.bootstrap_admin_with_hint(&username, &password, hint.as_deref()) {
            Ok(account) => {
                let mut sessions = SessionService::new(storage);
                match sessions.login(&account.username, &password) {
                    Ok(session) => {
                        self.setup_admin_password.clear();
                        self.setup_admin_password_confirm.clear();
                        self.complete_login(session);
                    }
                    Err(error) => {
                        self.setup_admin_password.clear();
                        self.setup_admin_password_confirm.clear();
                        self.error_message = Some(format_core_error(&error));
                        self.status_message = "Login failed".to_string();
                    }
                }
            }
            Err(error) => {
                self.error_message = Some(format_core_error(&error));
                self.status_message = "Setup failed".to_string();
            }
        }
    }

    fn complete_login(&mut self, session: AuthSession) {
        self.auth_session = Some(session.clone());
        self.login_username = session.username.clone();
        self.login_password.clear();
        self.bootstrap_password.clear();
        self.setup_admin_password.clear();
        self.setup_admin_password_confirm.clear();
        self.error_message = None;
        self.status_message = format!("Signed in as {}", session.username);
        self.home_mode = ShellHomeMode::User;

        if self.requested_debug_mode {
            let permission = PermissionService::new(self.debug_policy).authorize(
                Some(&session),
                PermissionAction::EnterDebugMode,
                None,
            );
            if let Some(storage) = self.storage_manager.clone() {
                let audit = AuditService::new(storage);
                if permission.allowed {
                    self.home_mode = ShellHomeMode::Debug;
                    let _ = audit.record(
                        Some(&session),
                        PermissionAction::EnterDebugMode,
                        None,
                        AuditOutcome::Success,
                        Some("debug_entered"),
                    );
                } else {
                    let reason = permission
                        .reason
                        .as_deref()
                        .unwrap_or("debug_policy_denied");
                    let _ = audit.record(
                        Some(&session),
                        PermissionAction::EnterDebugMode,
                        None,
                        AuditOutcome::Denied,
                        Some(reason),
                    );
                    self.toast_message = Some("Debug mode denied".to_string());
                }
            }
        }

        self.screen_stack = vec![ShellScreen::Home];
        self.focused_component = ShellComponent::Home;
        self.active_popup = None;
        self.refresh_hit_map();
    }

    fn open_user_management(&mut self) {
        if self.auth_session.is_none() {
            self.error_message = Some("Login required".to_string());
            return;
        };

        if self.refresh_user_management().is_ok() {
            self.screen_stack.push(ShellScreen::UserManagement);
            self.focused_component = ShellComponent::UserManagement;
            self.status_message = if self.can_manage_all_users() {
                "User Management".to_string()
            } else {
                "User Profile".to_string()
            };
            self.refresh_hit_map();
        }
    }

    fn open_clock(&mut self) {
        if self.active_screen() != ShellScreen::Clock {
            self.screen_stack.push(ShellScreen::Clock);
        }
        self.active_popup = None;
        self.focused_component = ShellComponent::Clock;
        self.status_message = "Clock".to_string();
        self.refresh_hit_map();
    }

    fn close_clock(&mut self) {
        if self.active_screen() == ShellScreen::Clock {
            self.screen_stack.pop();
        }
        if self.screen_stack.is_empty() {
            self.screen_stack.push(ShellScreen::Home);
        }
        self.status_message = "Ready".to_string();
        self.refresh_hit_map();
    }

    fn open_explorer(&mut self, platform: &dyn Platform) {
        if self.auth_session.is_none() {
            self.error_message = Some("Login required".to_string());
            return;
        }
        let Some(storage) = self.storage_manager.clone() else {
            self.error_message = Some("Storage unavailable".to_string());
            return;
        };

        let show_hidden = storage
            .load_config()
            .map(|config| config.explorer.show_hidden)
            .unwrap_or(false);
        let start_path = platform
            .user_dirs()
            .map(|dirs| dirs.documents().to_path_buf())
            .unwrap_or_else(|_| storage.layout().data_path.clone());
        let start_path = if start_path.exists() {
            start_path
        } else {
            storage.layout().data_path.clone()
        };

        self.explorer_state = Some(ExplorerState::new(start_path, show_hidden));
        self.explorer_input_mode = ExplorerInputMode::Browse;
        self.explorer_input.clear();
        self.screen_stack.push(ShellScreen::Explorer);
        self.focused_component = ShellComponent::Explorer;
        self.status_message = "Explorer".to_string();
        self.apply_explorer_command(ExplorerCommand::Refresh, platform);
        self.refresh_hit_map();
    }

    fn close_explorer(&mut self) {
        self.explorer_input_mode = ExplorerInputMode::Browse;
        self.explorer_input.clear();
        self.pop_to_home();
        self.status_message = "Ready".to_string();
    }

    fn apply_explorer_command(&mut self, command: ExplorerCommand, platform: &dyn Platform) {
        let Some(storage) = self.storage_manager.clone() else {
            self.error_message = Some("Storage unavailable".to_string());
            return;
        };
        let session = self.auth_session.clone();
        let Some(state) = self.explorer_state.as_mut() else {
            self.error_message = Some("Explorer unavailable".to_string());
            return;
        };

        ExplorerController::default().apply(state, command, session.as_ref(), platform, &storage);
        if let Some(error) = state.error.clone() {
            self.error_message = Some(error);
            self.status_message = "Explorer error".to_string();
        } else {
            self.error_message = None;
            if let Some(message) = state.message.clone() {
                self.status_message = message;
            }
        }
    }

    fn begin_explorer_input(&mut self, mode: ExplorerInputMode) {
        self.explorer_input_mode = mode;
        self.explorer_input = if mode == ExplorerInputMode::Rename {
            self.explorer_state
                .as_ref()
                .and_then(|state| state.selected_entry())
                .map(|entry| entry.name.clone())
                .unwrap_or_default()
        } else {
            String::new()
        };
        self.status_message = explorer_input_prompt(mode).to_string();
    }

    fn append_explorer_char(&mut self, character: char) {
        self.explorer_input.push(character);
    }

    fn explorer_backspace(&mut self) {
        self.explorer_input.pop();
    }

    fn submit_explorer_input(&mut self, platform: &dyn Platform) {
        let value = self.explorer_input.trim().to_string();
        let command = match self.explorer_input_mode {
            ExplorerInputMode::Browse => return,
            ExplorerInputMode::Search => ExplorerCommand::Search(value),
            ExplorerInputMode::NewFolder => ExplorerCommand::NewFolder(value),
            ExplorerInputMode::NewTextFile => ExplorerCommand::NewTextFile(value),
            ExplorerInputMode::Rename => ExplorerCommand::Rename(value),
        };

        self.explorer_input_mode = ExplorerInputMode::Browse;
        self.explorer_input.clear();
        self.apply_explorer_command(command, platform);
    }

    fn cancel_explorer_input(&mut self) {
        if let Some(state) = self.explorer_state.as_mut()
            && state.pending_dialog.is_some()
        {
            state.pending_dialog = None;
            state.message = Some("Cancelled".to_string());
            self.status_message = "Cancelled".to_string();
            return;
        }
        self.explorer_input_mode = ExplorerInputMode::Browse;
        self.explorer_input.clear();
        self.status_message = "Explorer".to_string();
    }

    fn select_explorer_at(
        &mut self,
        coordinates: CellPosition,
        click: ClickKind,
        platform: &dyn Platform,
    ) {
        let Some(index) = self.explorer_index_at(coordinates) else {
            return;
        };
        self.apply_explorer_command(ExplorerCommand::SelectIndex(index), platform);
        if click == ClickKind::Double {
            self.apply_explorer_command(ExplorerCommand::OpenSelected, platform);
        }
    }

    fn explorer_index_at(&self, coordinates: CellPosition) -> Option<usize> {
        let area = Rect::new(0, 0, self.terminal_size.0, self.terminal_size.1);
        let tundra_ui::ShellLayout::Full { main, .. } = tundra_ui::compute_shell_layout(area)
        else {
            return None;
        };
        if !rect_contains(main, coordinates) {
            return None;
        }
        let content_line = coordinates.1.checked_sub(main.y.saturating_add(1))? as usize;
        let explorer = self.to_explorer_view_model();
        let content_width = main.width.saturating_sub(2);
        let first_entry_line =
            tundra_ui::explorer_first_entry_content_line(&explorer, content_width);
        let index = content_line.checked_sub(first_entry_line)?;
        self.explorer_state
            .as_ref()
            .filter(|state| index < state.entries.len())
            .map(|_| index)
    }

    fn refresh_user_management(&mut self) -> Result<(), CoreError> {
        let Some(storage) = self.storage_manager.clone() else {
            self.error_message = Some("Storage unavailable".to_string());
            return Ok(());
        };
        let Some(session) = self.auth_session.as_ref() else {
            self.error_message = Some("Login required".to_string());
            return Ok(());
        };
        let users = UserService::with_debug_policy(storage, self.debug_policy)
            .list_accessible_users(session)?;
        self.user_management_users = users;
        if self.user_management_users.is_empty() {
            self.user_management_selected = 0;
        } else {
            self.user_management_selected = self
                .user_management_selected
                .min(self.user_management_users.len() - 1);
        }
        Ok(())
    }

    fn select_user_management_row(&mut self, direction: i8) {
        if self.user_management_users.is_empty() {
            return;
        }
        let len = self.user_management_users.len() as isize;
        let next = (self.user_management_selected as isize + direction as isize).rem_euclid(len);
        self.user_management_selected = next as usize;
    }

    fn begin_create_managed_user(&mut self, role: UserRole) {
        self.user_management_mode = UserManagementMode::Create(UserManagementCreateForm {
            username: String::new(),
            display_name: String::new(),
            password: String::new(),
            role,
            focused_field: UserManagementFormField::Username,
        });
        self.user_management_message = None;
    }

    fn begin_edit_selected_user_info(&mut self) {
        if let Some(user) = self
            .user_management_users
            .get(self.user_management_selected)
            .cloned()
        {
            self.user_management_mode = UserManagementMode::EditInfo(UserManagementInfoForm {
                username: user.username,
                display_name: user.display_name,
            });
            self.user_management_message = None;
        }
    }

    fn begin_set_selected_password(&mut self) {
        if let Some(username) = self.selected_managed_username() {
            self.user_management_mode = UserManagementMode::Password(UserManagementPasswordForm {
                username,
                password: String::new(),
            });
            self.user_management_message = None;
        }
    }

    fn disable_selected_user(&mut self) {
        if let Some(username) = self.selected_managed_username() {
            let current_user = self.is_current_username(&username);
            let disabled = self.run_selected_user_operation("Disabled", |service, session| {
                service.disable_user(session, &username)
            });
            if disabled && current_user {
                self.return_to_login("Account disabled");
            }
        }
    }

    fn unlock_selected_user(&mut self) {
        if let Some(username) = self.selected_managed_username() {
            self.run_selected_user_operation("Enabled/unlocked", |service, session| {
                service.enable_user(session, &username)
            });
        }
    }

    fn reset_selected_password(&mut self) {
        self.begin_set_selected_password();
    }

    fn cycle_selected_role(&mut self) {
        if let Some(username) = self.selected_managed_username() {
            let next_role = self
                .user_management_users
                .get(self.user_management_selected)
                .map(|user| match user.role {
                    UserRole::User | UserRole::Guest => UserRole::Admin,
                    UserRole::Admin => UserRole::Debug,
                    UserRole::Debug => UserRole::User,
                })
                .unwrap_or(UserRole::User);
            let changed = self
                .run_selected_user_operation("Changed role for", |service, session| {
                    service.change_role(session, &username, next_role)
                });
            if changed {
                self.sync_current_session_role();
                let _ = self.refresh_user_management();
            }
        }
    }

    fn run_selected_user_operation(
        &mut self,
        success_prefix: &'static str,
        operation: impl FnOnce(UserService, &AuthSession) -> Result<(), CoreError>,
    ) -> bool {
        let Some(storage) = self.storage_manager.clone() else {
            return false;
        };
        let Some(session) = self.auth_session.as_ref() else {
            return false;
        };
        let username = self
            .selected_managed_username()
            .unwrap_or_else(|| "user".to_string());
        let service = UserService::with_debug_policy(storage, self.debug_policy);
        let succeeded = match operation(service, session) {
            Ok(()) => {
                self.user_management_message = Some(format!("{success_prefix} {username}"));
                true
            }
            Err(error) => {
                self.user_management_message = Some(format_core_error(&error));
                false
            }
        };
        let _ = self.refresh_user_management();
        succeeded
    }

    fn submit_user_management_form(&mut self) {
        let Some(storage) = self.storage_manager.clone() else {
            return;
        };
        let Some(session) = self.auth_session.as_ref() else {
            return;
        };
        let service = UserService::with_debug_policy(storage, self.debug_policy);
        match self.user_management_mode.clone() {
            UserManagementMode::Browse => {}
            UserManagementMode::Create(form) => {
                let username = form.username.trim().to_string();
                let result = service.create_user(
                    session,
                    &form.username,
                    &form.display_name,
                    form.role,
                    &form.password,
                );
                self.user_management_message = Some(match result {
                    Ok(account) => {
                        self.user_management_mode = UserManagementMode::Browse;
                        format!("Created {}", account.username)
                    }
                    Err(error) => format_core_error(&error),
                });
                let _ = self.refresh_user_management();
                if !username.is_empty() {
                    self.select_managed_username(&username);
                }
                return;
            }
            UserManagementMode::EditInfo(form) => {
                let result = service.update_user_info(session, &form.username, &form.display_name);
                self.user_management_message = Some(match result {
                    Ok(account) => {
                        self.user_management_mode = UserManagementMode::Browse;
                        format!("Updated {}", account.username)
                    }
                    Err(error) => format_core_error(&error),
                });
            }
            UserManagementMode::Password(form) => {
                let result = service.set_user_password(session, &form.username, &form.password);
                self.user_management_message = Some(match result {
                    Ok(()) => {
                        self.user_management_mode = UserManagementMode::Browse;
                        format!("Updated password for {}", form.username)
                    }
                    Err(error) => format_core_error(&error),
                });
            }
        }
        let _ = self.refresh_user_management();
    }

    fn delete_selected_user(&mut self) {
        let Some(username) = self.selected_managed_username() else {
            return;
        };
        let Some(storage) = self.storage_manager.clone() else {
            return;
        };
        let Some(session) = self.auth_session.as_ref() else {
            return;
        };
        let deleting_current_user = self.is_current_username(&username);
        let deleted = match UserService::with_debug_policy(storage, self.debug_policy)
            .delete_user(session, &username)
        {
            Ok(()) => {
                self.user_management_message = Some(format!("Deleted {username}"));
                true
            }
            Err(error) => {
                self.user_management_message = Some(format_core_error(&error));
                false
            }
        };
        if deleted && deleting_current_user {
            self.return_to_login("Account deleted");
            return;
        }
        let _ = self.refresh_user_management();
    }

    fn append_user_management_char(&mut self, character: char) {
        match &mut self.user_management_mode {
            UserManagementMode::Create(form) => match form.focused_field {
                UserManagementFormField::Username => form.username.push(character),
                UserManagementFormField::DisplayName => form.display_name.push(character),
                UserManagementFormField::Password => form.password.push(character),
            },
            UserManagementMode::EditInfo(form) => form.display_name.push(character),
            UserManagementMode::Password(form) => form.password.push(character),
            UserManagementMode::Browse => {}
        }
    }

    fn user_management_backspace(&mut self) {
        match &mut self.user_management_mode {
            UserManagementMode::Create(form) => match form.focused_field {
                UserManagementFormField::Username => {
                    form.username.pop();
                }
                UserManagementFormField::DisplayName => {
                    form.display_name.pop();
                }
                UserManagementFormField::Password => {
                    form.password.pop();
                }
            },
            UserManagementMode::EditInfo(form) => {
                form.display_name.pop();
            }
            UserManagementMode::Password(form) => {
                form.password.pop();
            }
            UserManagementMode::Browse => {}
        }
    }

    fn move_user_management_form_focus(&mut self, direction: i8) {
        if let UserManagementMode::Create(form) = &mut self.user_management_mode {
            form.focused_field = if direction < 0 {
                form.focused_field.previous()
            } else {
                form.focused_field.next()
            };
        }
    }

    fn cancel_user_management_form(&mut self) {
        if self.user_management_mode != UserManagementMode::Browse {
            self.user_management_mode = UserManagementMode::Browse;
            self.user_management_message = Some("Cancelled".to_string());
        }
    }

    fn selected_managed_username(&self) -> Option<String> {
        self.user_management_users
            .get(self.user_management_selected)
            .map(|user| user.username.clone())
    }

    fn select_managed_username(&mut self, username: &str) {
        if let Some(index) = self
            .user_management_users
            .iter()
            .position(|user| user.username.eq_ignore_ascii_case(username))
        {
            self.user_management_selected = index;
        }
    }

    fn is_current_username(&self, username: &str) -> bool {
        self.auth_session
            .as_ref()
            .map(|session| session.username.eq_ignore_ascii_case(username))
            .unwrap_or(false)
    }

    fn sync_current_session_role(&mut self) {
        let Some(session) = self.auth_session.as_mut() else {
            return;
        };
        if let Some(user) = self
            .user_management_users
            .iter()
            .find(|user| user.username.eq_ignore_ascii_case(&session.username))
        {
            session.role = user.role;
        }
    }

    fn return_to_login(&mut self, status: &str) {
        self.auth_session = None;
        self.user_management_users.clear();
        self.user_management_selected = 0;
        self.user_management_mode = UserManagementMode::Browse;
        self.login_password.clear();
        let _ = self.refresh_login_users_from_storage();
        self.screen_stack = vec![ShellScreen::Login];
        self.focused_component = ShellComponent::LoginUserList;
        self.status_message = status.to_string();
        self.refresh_hit_map();
    }

    fn user_home_entries(&self) -> Vec<tundra_ui::ShellEntry> {
        let mut entries = user_home_entries();
        if self.can_manage_all_users() {
            entries.push(tundra_ui::ShellEntry::new(
                "User Management",
                "Manage local TundraUX users",
            ));
        } else if self.auth_session.is_some() {
            entries.push(tundra_ui::ShellEntry::new(
                "User Profile",
                "Manage your local TundraUX account",
            ));
        }
        entries
    }

    fn sync_home_entry_selection(&mut self) {
        let count = self.user_home_entries().len();
        self.selected_home_entry_index = if count == 0 {
            0
        } else {
            self.selected_home_entry_index.min(count - 1)
        };
    }

    fn select_home_entry(&mut self, index: usize) {
        let entries = self.user_home_entries();
        if entries.is_empty() {
            self.selected_home_entry_index = 0;
            return;
        }

        self.selected_home_entry_index = index.min(entries.len() - 1);
        self.status_message = format!("Home: {}", entries[self.selected_home_entry_index].label);
    }

    fn select_home_entry_delta(&mut self, delta: isize) {
        let count = self.user_home_entries().len();
        if count == 0 {
            self.selected_home_entry_index = 0;
            return;
        }

        let current = self.selected_home_entry_index().min(count - 1) as isize;
        let next = (current + delta).clamp(0, count.saturating_sub(1) as isize);
        self.select_home_entry(next as usize);
    }

    fn select_home_entry_row_delta(&mut self, direction: isize) {
        let columns = self.visible_home_entry_columns().max(1) as isize;
        self.select_home_entry_delta(direction.saturating_mul(columns));
    }

    fn activate_selected_home_entry(&mut self, platform: &dyn Platform) {
        self.activate_home_entry(self.selected_home_entry_index(), platform);
    }

    fn activate_home_entry(&mut self, index: usize, platform: &dyn Platform) {
        let entries = self.user_home_entries();
        let Some(entry) = entries.get(index) else {
            return;
        };

        self.selected_home_entry_index = index;
        match entry.label.as_str() {
            "Explorer" => self.open_explorer(platform),
            "User Management" | "User Profile" => self.open_user_management(),
            label => {
                self.error_message = None;
                self.status_message = format!("{label} is not implemented yet");
            }
        }
    }

    fn visible_home_entry_columns(&self) -> usize {
        let area = Rect::new(0, 0, self.terminal_size.0, self.terminal_size.1);
        let tundra_ui::ShellLayout::Full { main, .. } = tundra_ui::compute_shell_layout(area)
        else {
            return 1;
        };
        let areas = tundra_ui::home_entry_tile_areas(main, self.user_home_entries().len());
        let Some(first) = areas.first() else {
            return 1;
        };

        areas.iter().take_while(|area| area.y == first.y).count()
    }

    fn home_entry_index_at(&self, coordinates: CellPosition) -> Option<usize> {
        let area = Rect::new(0, 0, self.terminal_size.0, self.terminal_size.1);
        let tundra_ui::ShellLayout::Full { main, .. } = tundra_ui::compute_shell_layout(area)
        else {
            return None;
        };

        tundra_ui::home_entry_index_at(main, self.user_home_entries().len(), coordinates)
    }

    pub fn apply_input(&mut self, input: InputEvent) -> ShellAction {
        let platform = tundra_platform::native_platform();
        self.apply_input_with_platform(input, platform.as_ref())
    }

    pub fn apply_input_with_platform(
        &mut self,
        input: InputEvent,
        platform: &dyn Platform,
    ) -> ShellAction {
        let routed = self.route_input_at(input, Instant::now());
        self.apply_routed_event(routed, platform)
    }

    pub fn route_input_at(&mut self, input: InputEvent, received_at: Instant) -> RoutedEvent {
        let (target, command) = match &input {
            InputEvent::Shutdown => (RoutedTarget::Global, ShellCommand::Shutdown),
            InputEvent::Tick => (RoutedTarget::Global, ShellCommand::Tick),
            InputEvent::Resize { width, height } => (
                RoutedTarget::Global,
                ShellCommand::RefreshHitMap {
                    width: *width,
                    height: *height,
                },
            ),
            InputEvent::Key(key) => {
                let (target, command) = self.route_key_input(key);
                (target, command)
            }
            InputEvent::Mouse(mouse) => {
                let (target, command) = self.route_mouse_input(*mouse, received_at);
                (target, command)
            }
            InputEvent::FocusGained | InputEvent::FocusLost | InputEvent::Paste(_) => {
                (RoutedTarget::Global, ShellCommand::RecordInput)
            }
        };

        RoutedEvent {
            input,
            target,
            command,
        }
    }

    fn apply_routed_event(&mut self, routed: RoutedEvent, platform: &dyn Platform) -> ShellAction {
        self.record_input_diagnostics(&routed);
        self.last_routed_target = Some(routed.target);
        self.last_command = Some(routed.command.clone());

        match routed.command {
            ShellCommand::Shutdown => {
                self.shutdown_requested = true;
                ShellAction::Exit
            }
            ShellCommand::Tick => {
                self.tick_count = self.tick_count.saturating_add(1);
                ShellAction::Redraw
            }
            ShellCommand::RefreshHitMap { width, height } => {
                self.terminal_size = (width, height);
                self.last_resize_event = Some(format!("{width}x{height}"));
                if self.active_screen() == ShellScreen::FirstRunSetup {
                    self.sync_setup_timezone_window();
                }
                if self.active_screen() == ShellScreen::Login {
                    self.sync_login_user_window();
                }
                self.refresh_hit_map();
                ShellAction::Redraw
            }
            ShellCommand::RequestExit => {
                if self.active_screen() != ShellScreen::ExitConfirm {
                    self.screen_stack.push(ShellScreen::ExitConfirm);
                }
                self.active_popup = None;
                self.focused_component = ShellComponent::ExitDialog;
                self.status_message = "Confirm exit".to_string();
                self.refresh_hit_map();
                ShellAction::Redraw
            }
            ShellCommand::ConfirmExit => {
                self.shutdown_requested = true;
                ShellAction::Exit
            }
            ShellCommand::CancelExit => {
                self.cancel_exit_confirmation();
                self.active_popup = None;
                self.status_message = "Ready".to_string();
                self.refresh_hit_map();
                ShellAction::Redraw
            }
            ShellCommand::FocusNext => {
                self.move_focus(1);
                self.status_message = format!("Focus: {}", self.focused_component.label());
                ShellAction::Redraw
            }
            ShellCommand::FocusPrevious => {
                self.move_focus(-1);
                self.status_message = format!("Focus: {}", self.focused_component.label());
                ShellAction::Redraw
            }
            ShellCommand::AppendAuthChar(character) => {
                self.append_auth_char(character);
                ShellAction::Redraw
            }
            ShellCommand::AuthBackspace => {
                self.auth_backspace();
                ShellAction::Redraw
            }
            ShellCommand::LoginPreviousUser => {
                self.select_login_user_delta(-1);
                ShellAction::Redraw
            }
            ShellCommand::LoginNextUser => {
                self.select_login_user_delta(1);
                ShellAction::Redraw
            }
            ShellCommand::LoginPageUserUp => {
                self.select_login_user_delta(-(self.login_user_visible_row_count() as isize));
                ShellAction::Redraw
            }
            ShellCommand::LoginPageUserDown => {
                self.select_login_user_delta(self.login_user_visible_row_count() as isize);
                ShellAction::Redraw
            }
            ShellCommand::LoginFirstUser => {
                self.select_first_login_user();
                ShellAction::Redraw
            }
            ShellCommand::LoginLastUser => {
                self.select_last_login_user();
                ShellAction::Redraw
            }
            ShellCommand::LoginFocusUserList => {
                self.focused_component = ShellComponent::LoginUserList;
                self.error_message = None;
                ShellAction::Redraw
            }
            ShellCommand::LoginFocusPassword => {
                self.focused_component = ShellComponent::LoginPassword;
                self.error_message = None;
                ShellAction::Redraw
            }
            ShellCommand::SubmitLogin => {
                self.submit_login();
                ShellAction::Redraw
            }
            ShellCommand::SubmitBootstrapAdmin => {
                self.submit_bootstrap_admin();
                ShellAction::Redraw
            }
            ShellCommand::SetupPreviousLanguage => {
                self.setup_previous_language();
                ShellAction::Redraw
            }
            ShellCommand::SetupNextLanguage => {
                self.setup_next_language();
                ShellAction::Redraw
            }
            ShellCommand::SetupContinue => {
                self.setup_continue();
                ShellAction::Redraw
            }
            ShellCommand::SetupPreviousTimezone => {
                self.setup_select_timezone_delta(-1);
                ShellAction::Redraw
            }
            ShellCommand::SetupNextTimezone => {
                self.setup_select_timezone_delta(1);
                ShellAction::Redraw
            }
            ShellCommand::SetupPageTimezoneUp => {
                self.setup_select_timezone_delta(
                    -(self.setup_timezone_visible_row_count() as isize),
                );
                ShellAction::Redraw
            }
            ShellCommand::SetupPageTimezoneDown => {
                self.setup_select_timezone_delta(self.setup_timezone_visible_row_count() as isize);
                ShellAction::Redraw
            }
            ShellCommand::SetupFirstTimezone => {
                self.setup_select_first_timezone();
                ShellAction::Redraw
            }
            ShellCommand::SetupLastTimezone => {
                self.setup_select_last_timezone();
                ShellAction::Redraw
            }
            ShellCommand::SetupFocusNext => {
                self.move_setup_admin_focus(1);
                ShellAction::Redraw
            }
            ShellCommand::SetupFocusPrevious => {
                self.move_setup_admin_focus(-1);
                ShellAction::Redraw
            }
            ShellCommand::AppendSetupAdminChar(character) => {
                self.append_setup_admin_char(character);
                ShellAction::Redraw
            }
            ShellCommand::SetupAdminBackspace => {
                self.setup_admin_backspace();
                ShellAction::Redraw
            }
            ShellCommand::SubmitSetup => {
                self.submit_first_run_setup();
                ShellAction::Redraw
            }
            ShellCommand::ActivateSetup {
                target,
                coordinates,
            } => {
                self.activate_setup(target, coordinates);
                ShellAction::Redraw
            }
            ShellCommand::ActivateLogin {
                target,
                coordinates,
            } => {
                self.activate_login(target, coordinates);
                ShellAction::Redraw
            }
            ShellCommand::HomeEntryLeft => {
                self.select_home_entry_delta(-1);
                ShellAction::Redraw
            }
            ShellCommand::HomeEntryRight => {
                self.select_home_entry_delta(1);
                ShellAction::Redraw
            }
            ShellCommand::HomeEntryUp => {
                self.select_home_entry_row_delta(-1);
                ShellAction::Redraw
            }
            ShellCommand::HomeEntryDown => {
                self.select_home_entry_row_delta(1);
                ShellAction::Redraw
            }
            ShellCommand::HomeFirstEntry => {
                self.select_home_entry(0);
                ShellAction::Redraw
            }
            ShellCommand::HomeLastEntry => {
                self.select_home_entry(self.user_home_entries().len().saturating_sub(1));
                ShellAction::Redraw
            }
            ShellCommand::ActivateSelectedHomeEntry => {
                self.activate_selected_home_entry(platform);
                ShellAction::Redraw
            }
            ShellCommand::SelectHomeEntryAt(coordinates) => {
                if let Some(index) = self.home_entry_index_at(coordinates) {
                    self.select_home_entry(index);
                }
                ShellAction::Redraw
            }
            ShellCommand::ActivateHomeEntryAt(coordinates, click) => {
                if let Some(index) = self.home_entry_index_at(coordinates) {
                    self.select_home_entry(index);
                    if click == ClickKind::Double {
                        self.activate_home_entry(index, platform);
                    }
                }
                ShellAction::Redraw
            }
            ShellCommand::OpenExplorer => {
                self.open_explorer(platform);
                ShellAction::Redraw
            }
            ShellCommand::CloseExplorer => {
                self.close_explorer();
                ShellAction::Redraw
            }
            ShellCommand::ExplorerNext => {
                self.apply_explorer_command(ExplorerCommand::SelectNext, platform);
                ShellAction::Redraw
            }
            ShellCommand::ExplorerPrevious => {
                self.apply_explorer_command(ExplorerCommand::SelectPrevious, platform);
                ShellAction::Redraw
            }
            ShellCommand::ExplorerOpenSelected => {
                self.apply_explorer_command(ExplorerCommand::OpenSelected, platform);
                ShellAction::Redraw
            }
            ShellCommand::ExplorerOpenParent => {
                self.apply_explorer_command(ExplorerCommand::OpenParent, platform);
                ShellAction::Redraw
            }
            ShellCommand::ExplorerToggleHidden => {
                self.apply_explorer_command(ExplorerCommand::ToggleHidden, platform);
                ShellAction::Redraw
            }
            ShellCommand::ExplorerCopy => {
                self.apply_explorer_command(ExplorerCommand::Copy, platform);
                ShellAction::Redraw
            }
            ShellCommand::ExplorerCut => {
                self.apply_explorer_command(ExplorerCommand::Cut, platform);
                ShellAction::Redraw
            }
            ShellCommand::ExplorerPaste => {
                self.apply_explorer_command(ExplorerCommand::Paste, platform);
                ShellAction::Redraw
            }
            ShellCommand::ExplorerDelete => {
                self.apply_explorer_command(ExplorerCommand::DeleteToTrash, platform);
                ShellAction::Redraw
            }
            ShellCommand::ExplorerConfirmDelete => {
                self.apply_explorer_command(ExplorerCommand::ConfirmDelete, platform);
                ShellAction::Redraw
            }
            ShellCommand::ExplorerSelectAt(coordinates, click) => {
                self.select_explorer_at(coordinates, click, platform);
                ShellAction::Redraw
            }
            ShellCommand::BeginExplorerSearch => {
                self.begin_explorer_input(ExplorerInputMode::Search);
                ShellAction::Redraw
            }
            ShellCommand::BeginExplorerNewFolder => {
                self.begin_explorer_input(ExplorerInputMode::NewFolder);
                ShellAction::Redraw
            }
            ShellCommand::BeginExplorerNewTextFile => {
                self.begin_explorer_input(ExplorerInputMode::NewTextFile);
                ShellAction::Redraw
            }
            ShellCommand::BeginExplorerRename => {
                self.begin_explorer_input(ExplorerInputMode::Rename);
                ShellAction::Redraw
            }
            ShellCommand::AppendExplorerChar(character) => {
                self.append_explorer_char(character);
                ShellAction::Redraw
            }
            ShellCommand::ExplorerBackspace => {
                self.explorer_backspace();
                ShellAction::Redraw
            }
            ShellCommand::SubmitExplorerInput => {
                self.submit_explorer_input(platform);
                ShellAction::Redraw
            }
            ShellCommand::CancelExplorerInput => {
                self.cancel_explorer_input();
                ShellAction::Redraw
            }
            ShellCommand::OpenUserManagement => {
                self.open_user_management();
                ShellAction::Redraw
            }
            ShellCommand::CloseUserManagement => {
                self.user_management_mode = UserManagementMode::Browse;
                self.pop_to_home();
                self.status_message = "Ready".to_string();
                self.refresh_hit_map();
                ShellAction::Redraw
            }
            ShellCommand::OpenClock => {
                self.open_clock();
                ShellAction::Redraw
            }
            ShellCommand::CloseClock => {
                self.close_clock();
                ShellAction::Redraw
            }
            ShellCommand::UserManagementNext => {
                self.select_user_management_row(1);
                ShellAction::Redraw
            }
            ShellCommand::UserManagementPrevious => {
                self.select_user_management_row(-1);
                ShellAction::Redraw
            }
            ShellCommand::CreateManagedUser(role) => {
                self.begin_create_managed_user(role);
                ShellAction::Redraw
            }
            ShellCommand::EditManagedUserInfo => {
                self.begin_edit_selected_user_info();
                ShellAction::Redraw
            }
            ShellCommand::DisableManagedUser => {
                self.disable_selected_user();
                ShellAction::Redraw
            }
            ShellCommand::UnlockManagedUser => {
                self.unlock_selected_user();
                ShellAction::Redraw
            }
            ShellCommand::ResetManagedPassword => {
                self.reset_selected_password();
                ShellAction::Redraw
            }
            ShellCommand::CycleManagedRole => {
                self.cycle_selected_role();
                ShellAction::Redraw
            }
            ShellCommand::DeleteManagedUser => {
                self.delete_selected_user();
                ShellAction::Redraw
            }
            ShellCommand::AppendUserManagementChar(character) => {
                self.append_user_management_char(character);
                ShellAction::Redraw
            }
            ShellCommand::UserManagementBackspace => {
                self.user_management_backspace();
                ShellAction::Redraw
            }
            ShellCommand::UserManagementFocusNext => {
                self.move_user_management_form_focus(1);
                ShellAction::Redraw
            }
            ShellCommand::UserManagementFocusPrevious => {
                self.move_user_management_form_focus(-1);
                ShellAction::Redraw
            }
            ShellCommand::SubmitUserManagementForm => {
                self.submit_user_management_form();
                ShellAction::Redraw
            }
            ShellCommand::CancelUserManagementForm => {
                self.cancel_user_management_form();
                ShellAction::Redraw
            }
            ShellCommand::Hover(target) => {
                self.hovered_component = target;
                ShellAction::Redraw
            }
            ShellCommand::Activate {
                target,
                coordinates,
                click,
            } => {
                if target == ShellComponent::Explorer {
                    self.focus_component(target);
                    self.select_explorer_at(coordinates, click, platform);
                    return ShellAction::Redraw;
                }
                self.focus_component(target);
                let click_label = match click {
                    ClickKind::Single => "single click",
                    ClickKind::Double => "double click",
                };
                self.status_message = format!("{} activated by {click_label}", target.label());
                ShellAction::Redraw
            }
            ShellCommand::OpenContextMenu {
                target,
                coordinates,
            } => {
                if target == Some(ShellComponent::Explorer)
                    && let Some(index) = self.explorer_index_at(coordinates)
                {
                    self.apply_explorer_command(ExplorerCommand::SelectIndex(index), platform);
                }
                self.active_popup = Some(ShellPopup {
                    owner: target,
                    anchor: coordinates,
                });
                self.focused_component = ShellComponent::ContextMenu;
                self.status_message = match target {
                    Some(target) => format!("Context menu: {}", target.label()),
                    None => "Context menu".to_string(),
                };
                self.refresh_hit_map();
                ShellAction::Redraw
            }
            ShellCommand::ClosePopup => {
                self.active_popup = None;
                self.status_message = "Ready".to_string();
                self.refresh_hit_map();
                ShellAction::Redraw
            }
            ShellCommand::CloseTimeSyncDialog => {
                self.close_time_sync_dialog();
                ShellAction::Redraw
            }
            ShellCommand::CaptureOverlayInput => {
                self.status_message = "Overlay captured input".to_string();
                ShellAction::Redraw
            }
            ShellCommand::RecordInput | ShellCommand::Noop => ShellAction::Redraw,
        }
    }

    pub fn active_screen(&self) -> ShellScreen {
        self.screen_stack
            .last()
            .copied()
            .unwrap_or(ShellScreen::Home)
    }

    pub fn home_mode(&self) -> ShellHomeMode {
        self.home_mode
    }

    pub fn screen_stack(&self) -> &[ShellScreen] {
        &self.screen_stack
    }

    pub fn terminal_size(&self) -> (u16, u16) {
        self.terminal_size
    }

    pub fn tick_count(&self) -> u64 {
        self.tick_count
    }

    pub fn last_key_event(&self) -> Option<&str> {
        self.last_key_event.as_deref()
    }

    pub fn last_mouse_event(&self) -> Option<&str> {
        self.last_mouse_event.as_deref()
    }

    pub fn last_resize_event(&self) -> Option<&str> {
        self.last_resize_event.as_deref()
    }

    pub fn mouse_coordinates(&self) -> Option<(u16, u16)> {
        self.mouse_coordinates
    }

    pub fn shutdown_requested(&self) -> bool {
        self.shutdown_requested
    }

    pub fn status(&self) -> &str {
        &self.status_message
    }

    pub fn current_time_label(&self) -> String {
        clock_display_label(self.network_clock.current())
    }

    pub fn time_sync_failure_dialog_visible(&self) -> bool {
        self.time_sync_dialog_visible
    }

    pub fn time_sync_failure_message(&self) -> Option<&str> {
        self.time_sync_failure_message.as_deref()
    }

    pub fn apply_time_sync_result(&mut self, result: TimeSyncResult) {
        match result {
            Ok(utc) => self.apply_time_sync_success_utc(utc),
            Err(error) => {
                self.last_time_sync_utc = None;
                self.network_clock.apply_sync(Err(error));
                self.show_time_sync_failure_dialog("联网校准时间失败".to_string());
            }
        }
    }

    #[doc(hidden)]
    pub fn apply_time_sync_utc_for_test(&mut self, utc: DateTime<Utc>) {
        self.apply_time_sync_success_utc(utc);
    }

    #[doc(hidden)]
    pub fn apply_time_sync_failure_for_test(&mut self, message: &str) {
        self.last_time_sync_utc = None;
        self.network_clock = ShellNetworkClock::new(self.clock_timezone_id.clone());
        self.show_time_sync_failure_dialog(message.to_string());
    }

    pub fn terminal_flags(&self) -> ShellTerminalFlags {
        self.terminal_flags
    }

    pub fn mouse_scroll_direction(&self) -> Option<&str> {
        self.mouse_scroll_direction.as_deref()
    }

    pub fn mouse_drag_direction(&self) -> Option<&str> {
        self.mouse_drag_direction.as_deref()
    }

    pub fn platform_capability_summary(&self) -> &str {
        &self.platform_capability_summary
    }

    pub fn focused_component(&self) -> ShellComponent {
        self.focused_component
    }

    pub fn selected_home_entry_index(&self) -> usize {
        let count = self.user_home_entries().len();
        if count == 0 {
            0
        } else {
            self.selected_home_entry_index.min(count - 1)
        }
    }

    pub fn hovered_component(&self) -> Option<ShellComponent> {
        self.hovered_component
    }

    pub fn active_popup(&self) -> Option<ShellPopup> {
        self.active_popup
    }

    pub fn hit_map(&self) -> &ShellHitMap {
        &self.hit_map
    }

    pub fn hit_map_generation(&self) -> u64 {
        self.hit_map.generation()
    }

    pub fn hit_target_at(&self, coordinates: CellPosition) -> Option<ShellComponent> {
        self.hit_map.target_at(coordinates)
    }

    pub fn last_command(&self) -> Option<&ShellCommand> {
        self.last_command.as_ref()
    }

    pub fn last_routed_target(&self) -> Option<RoutedTarget> {
        self.last_routed_target
    }

    fn home_display_mode(&self) -> tundra_ui::HomeDisplayMode {
        if matches!(
            self.active_screen(),
            ShellScreen::FirstRunSetup | ShellScreen::Login | ShellScreen::BootstrapAdmin
        ) {
            return tundra_ui::HomeDisplayMode::Auth;
        }

        match self.home_mode {
            ShellHomeMode::Debug => tundra_ui::HomeDisplayMode::Debug,
            ShellHomeMode::User => tundra_ui::HomeDisplayMode::User,
        }
    }

    pub fn auth_session(&self) -> Option<&AuthSession> {
        self.auth_session.as_ref()
    }

    fn apply_time_sync_success_utc(&mut self, utc: DateTime<Utc>) {
        self.last_time_sync_utc = Some(utc);
        self.network_clock.apply_sync(Ok(utc));

        if self.time_sync_dialog_visible {
            self.time_sync_dialog_visible = false;
            self.time_sync_failure_message = None;
            self.status_message = "Ready".to_string();
        }

        self.refresh_hit_map();
    }

    fn show_time_sync_failure_dialog(&mut self, message: String) {
        self.time_sync_dialog_visible = true;
        self.time_sync_failure_message = Some(message.clone());
        self.active_popup = None;
        self.focused_component = ShellComponent::TimeSyncDialog;
        self.status_message = message;
        self.refresh_hit_map();
    }

    fn close_time_sync_dialog(&mut self) {
        self.time_sync_dialog_visible = false;
        self.time_sync_failure_message = None;
        self.status_message = "Ready".to_string();
        self.refresh_hit_map();
    }

    fn status_time_button_label(&self) -> Option<String> {
        clock_button_active_for_screen(self.active_screen()).then(|| self.current_time_label())
    }

    fn time_button_selected(&self) -> bool {
        self.focused_component == ShellComponent::ClockButton
            || self.active_screen() == ShellScreen::Clock
    }

    fn set_clock_timezone(&mut self, timezone_id: Option<String>) {
        self.clock_timezone_id = timezone_id;
        self.network_clock = ShellNetworkClock::new(self.clock_timezone_id.clone());
        if let Some(utc) = self.last_time_sync_utc {
            self.network_clock.apply_sync(Ok(utc));
        }
    }

    fn route_key_input(&self, key: &KeyInput) -> (RoutedTarget, ShellCommand) {
        if !key.phase.is_press_like() {
            return (RoutedTarget::Global, ShellCommand::Noop);
        }

        if key.is_ctrl_c() {
            return (RoutedTarget::Global, ShellCommand::Shutdown);
        }

        if self.time_sync_dialog_visible {
            return self.route_time_sync_dialog_key(key);
        }

        if self.active_screen() == ShellScreen::ExitConfirm {
            return self.route_exit_confirm_key(key);
        }

        if self.active_screen() == ShellScreen::Clock {
            return self.route_clock_key(key);
        }

        if self.active_screen() == ShellScreen::FirstRunSetup {
            return self.route_setup_key(key);
        }

        if self.active_screen() == ShellScreen::Login {
            return self.route_login_key(key);
        }

        if self.active_screen() == ShellScreen::BootstrapAdmin {
            return self.route_auth_key(key);
        }

        if self.active_screen() == ShellScreen::UserManagement {
            return self.route_user_management_key(key);
        }

        if self.active_popup.is_some() {
            return self.route_popup_key(key);
        }

        if self.active_screen() == ShellScreen::Explorer {
            return self.route_explorer_key(key);
        }

        if matches!(&key.key, InputKey::BackTab)
            || (matches!(&key.key, InputKey::Tab) && key.modifiers.shift)
        {
            return (RoutedTarget::Global, ShellCommand::FocusPrevious);
        }
        if matches!(&key.key, InputKey::Tab) {
            return (RoutedTarget::Global, ShellCommand::FocusNext);
        }

        match self.active_screen() {
            _ if self.focused_component == ShellComponent::ClockButton
                && matches!(&key.key, InputKey::Enter | InputKey::Character(' ')) =>
            {
                (
                    RoutedTarget::Component(ShellComponent::ClockButton),
                    self.clock_button_activation_command(),
                )
            }
            ShellScreen::Home if matches!(&key.key, InputKey::Left) => (
                RoutedTarget::Component(ShellComponent::Home),
                ShellCommand::HomeEntryLeft,
            ),
            ShellScreen::Home if matches!(&key.key, InputKey::Right) => (
                RoutedTarget::Component(ShellComponent::Home),
                ShellCommand::HomeEntryRight,
            ),
            ShellScreen::Home if matches!(&key.key, InputKey::Up) => (
                RoutedTarget::Component(ShellComponent::Home),
                ShellCommand::HomeEntryUp,
            ),
            ShellScreen::Home if matches!(&key.key, InputKey::Down) => (
                RoutedTarget::Component(ShellComponent::Home),
                ShellCommand::HomeEntryDown,
            ),
            ShellScreen::Home if matches!(&key.key, InputKey::Home) => (
                RoutedTarget::Component(ShellComponent::Home),
                ShellCommand::HomeFirstEntry,
            ),
            ShellScreen::Home if matches!(&key.key, InputKey::End) => (
                RoutedTarget::Component(ShellComponent::Home),
                ShellCommand::HomeLastEntry,
            ),
            ShellScreen::Home if matches!(&key.key, InputKey::Enter | InputKey::Character(' ')) => {
                (
                    RoutedTarget::Component(ShellComponent::Home),
                    ShellCommand::ActivateSelectedHomeEntry,
                )
            }
            ShellScreen::Home if key.is_character('e') || key.is_character('E') => {
                (RoutedTarget::Global, ShellCommand::OpenExplorer)
            }
            ShellScreen::Home if key.is_character('u') || key.is_character('U') => {
                (RoutedTarget::Global, ShellCommand::OpenUserManagement)
            }
            ShellScreen::Home if key.is_character('q') || matches!(&key.key, InputKey::Escape) => {
                (RoutedTarget::Global, ShellCommand::RequestExit)
            }
            _ => (
                RoutedTarget::Component(self.focused_component),
                ShellCommand::RecordInput,
            ),
        }
    }

    fn route_login_key(&self, key: &KeyInput) -> (RoutedTarget, ShellCommand) {
        let target = RoutedTarget::Component(self.focused_component);
        if matches!(&key.key, InputKey::Escape) {
            return (RoutedTarget::Global, ShellCommand::RequestExit);
        }

        match self.focused_component {
            ShellComponent::LoginPassword => match &key.key {
                InputKey::BackTab => (target, ShellCommand::LoginFocusUserList),
                InputKey::Tab if key.modifiers.shift => (target, ShellCommand::LoginFocusUserList),
                InputKey::Tab => (target, ShellCommand::LoginFocusUserList),
                InputKey::Up => (target, ShellCommand::LoginFocusUserList),
                InputKey::Enter => (target, ShellCommand::SubmitLogin),
                InputKey::Backspace => (target, ShellCommand::AuthBackspace),
                InputKey::Character(character) => {
                    (target, ShellCommand::AppendAuthChar(*character))
                }
                _ => (target, ShellCommand::RecordInput),
            },
            _ => match &key.key {
                InputKey::BackTab => (target, ShellCommand::LoginFocusPassword),
                InputKey::Tab => (target, ShellCommand::LoginFocusPassword),
                InputKey::Enter => (target, ShellCommand::LoginFocusPassword),
                InputKey::Up => (target, ShellCommand::LoginPreviousUser),
                InputKey::Down => (target, ShellCommand::LoginNextUser),
                InputKey::PageUp => (target, ShellCommand::LoginPageUserUp),
                InputKey::PageDown => (target, ShellCommand::LoginPageUserDown),
                InputKey::Home => (target, ShellCommand::LoginFirstUser),
                InputKey::End => (target, ShellCommand::LoginLastUser),
                _ => (target, ShellCommand::RecordInput),
            },
        }
    }

    fn route_auth_key(&self, key: &KeyInput) -> (RoutedTarget, ShellCommand) {
        let target = RoutedTarget::Component(self.focused_component);
        if matches!(&key.key, InputKey::BackTab)
            || (matches!(&key.key, InputKey::Tab) && key.modifiers.shift)
        {
            return (target, ShellCommand::FocusPrevious);
        }
        if matches!(&key.key, InputKey::Tab | InputKey::Down) {
            return (target, ShellCommand::FocusNext);
        }
        if matches!(&key.key, InputKey::Up) {
            return (target, ShellCommand::FocusPrevious);
        }
        if matches!(&key.key, InputKey::Escape) {
            return (RoutedTarget::Global, ShellCommand::RequestExit);
        }
        if matches!(&key.key, InputKey::Enter) {
            if matches!(
                self.focused_component,
                ShellComponent::LoginUsername | ShellComponent::BootstrapUsername
            ) {
                return (target, ShellCommand::FocusNext);
            }
            return (
                target,
                match self.active_screen() {
                    ShellScreen::BootstrapAdmin => ShellCommand::SubmitBootstrapAdmin,
                    _ => ShellCommand::SubmitLogin,
                },
            );
        }
        if matches!(&key.key, InputKey::Backspace) {
            return (target, ShellCommand::AuthBackspace);
        }
        if let InputKey::Character(character) = &key.key {
            return (target, ShellCommand::AppendAuthChar(*character));
        }

        (target, ShellCommand::RecordInput)
    }

    fn route_clock_key(&self, key: &KeyInput) -> (RoutedTarget, ShellCommand) {
        let target = RoutedTarget::Component(ShellComponent::Clock);
        match &key.key {
            InputKey::Escape => (RoutedTarget::Global, ShellCommand::CloseClock),
            _ => (target, ShellCommand::RecordInput),
        }
    }

    fn clock_button_activation_command(&self) -> ShellCommand {
        if self.active_screen() == ShellScreen::Clock {
            ShellCommand::CloseClock
        } else {
            ShellCommand::OpenClock
        }
    }

    fn route_time_sync_dialog_key(&self, key: &KeyInput) -> (RoutedTarget, ShellCommand) {
        let target = RoutedTarget::Modal(ShellComponent::TimeSyncDialog);
        match &key.key {
            InputKey::Escape | InputKey::Enter | InputKey::Character(' ') => {
                (target, ShellCommand::CloseTimeSyncDialog)
            }
            _ => (target, ShellCommand::CaptureOverlayInput),
        }
    }

    fn route_setup_key(&self, key: &KeyInput) -> (RoutedTarget, ShellCommand) {
        let target_component = self.setup_active_key_component();
        let target = RoutedTarget::Component(target_component);

        if matches!(&key.key, InputKey::Escape) {
            return (RoutedTarget::Global, ShellCommand::RequestExit);
        }

        match self.setup_step {
            tundra_ui::SetupStep::Language => match &key.key {
                InputKey::Up | InputKey::Left => (target, ShellCommand::SetupPreviousLanguage),
                InputKey::Down | InputKey::Right => (target, ShellCommand::SetupNextLanguage),
                InputKey::Enter | InputKey::Character(' ') => (target, ShellCommand::SetupContinue),
                _ => (target, ShellCommand::RecordInput),
            },
            tundra_ui::SetupStep::Timezone => match &key.key {
                InputKey::Up => (target, ShellCommand::SetupPreviousTimezone),
                InputKey::Down => (target, ShellCommand::SetupNextTimezone),
                InputKey::PageUp => (target, ShellCommand::SetupPageTimezoneUp),
                InputKey::PageDown => (target, ShellCommand::SetupPageTimezoneDown),
                InputKey::Home => (target, ShellCommand::SetupFirstTimezone),
                InputKey::End => (target, ShellCommand::SetupLastTimezone),
                InputKey::Enter => (target, ShellCommand::SetupContinue),
                _ => (target, ShellCommand::RecordInput),
            },
            tundra_ui::SetupStep::Admin => match &key.key {
                InputKey::BackTab => (target, ShellCommand::SetupFocusPrevious),
                InputKey::Tab if key.modifiers.shift => (target, ShellCommand::SetupFocusPrevious),
                InputKey::Tab => (target, ShellCommand::SetupFocusNext),
                InputKey::Up => (target, ShellCommand::SetupFocusPrevious),
                InputKey::Down => (target, ShellCommand::SetupFocusNext),
                InputKey::Backspace if setup_admin_text_field(self.setup_focused_field) => {
                    (target, ShellCommand::SetupAdminBackspace)
                }
                InputKey::Enter if self.setup_focused_field == tundra_ui::SetupField::Submit => {
                    (target, ShellCommand::SubmitSetup)
                }
                InputKey::Enter => (target, ShellCommand::SetupFocusNext),
                InputKey::Character(character)
                    if setup_admin_text_field(self.setup_focused_field) =>
                {
                    (target, ShellCommand::AppendSetupAdminChar(*character))
                }
                _ => (target, ShellCommand::RecordInput),
            },
        }
    }

    fn route_explorer_key(&self, key: &KeyInput) -> (RoutedTarget, ShellCommand) {
        let target = RoutedTarget::Component(ShellComponent::Explorer);

        if self
            .explorer_state
            .as_ref()
            .and_then(|state| state.pending_dialog.as_ref())
            .is_some()
        {
            return match &key.key {
                InputKey::Enter => (target, ShellCommand::ExplorerConfirmDelete),
                InputKey::Escape => (target, ShellCommand::CancelExplorerInput),
                InputKey::Character(character) if matches!(character, 'y' | 'Y') => {
                    (target, ShellCommand::ExplorerConfirmDelete)
                }
                InputKey::Character(character) if matches!(character, 'n' | 'N') => {
                    (target, ShellCommand::CancelExplorerInput)
                }
                _ => (target, ShellCommand::CaptureOverlayInput),
            };
        }

        if self.explorer_input_mode != ExplorerInputMode::Browse {
            return match &key.key {
                InputKey::Escape => (target, ShellCommand::CancelExplorerInput),
                InputKey::Enter => (target, ShellCommand::SubmitExplorerInput),
                InputKey::Backspace => (target, ShellCommand::ExplorerBackspace),
                InputKey::Character(character) => {
                    (target, ShellCommand::AppendExplorerChar(*character))
                }
                _ => (target, ShellCommand::RecordInput),
            };
        }

        match &key.key {
            InputKey::Escape => (RoutedTarget::Global, ShellCommand::CloseExplorer),
            InputKey::Up => (target, ShellCommand::ExplorerPrevious),
            InputKey::Down => (target, ShellCommand::ExplorerNext),
            InputKey::Enter => (target, ShellCommand::ExplorerOpenSelected),
            InputKey::Backspace => (target, ShellCommand::ExplorerOpenParent),
            InputKey::Delete => (target, ShellCommand::ExplorerDelete),
            InputKey::Character(character) if matches!(character, 'h' | 'H') => {
                (target, ShellCommand::ExplorerToggleHidden)
            }
            InputKey::Character(character) if matches!(character, 'c' | 'C') => {
                (target, ShellCommand::ExplorerCopy)
            }
            InputKey::Character(character) if matches!(character, 'x' | 'X') => {
                (target, ShellCommand::ExplorerCut)
            }
            InputKey::Character(character) if matches!(character, 'v' | 'V') => {
                (target, ShellCommand::ExplorerPaste)
            }
            InputKey::Character(character) if matches!(character, 'd' | 'D') => {
                (target, ShellCommand::ExplorerDelete)
            }
            InputKey::Character(character) if matches!(character, 'n' | 'N' | 'f' | 'F') => {
                (target, ShellCommand::BeginExplorerNewFolder)
            }
            InputKey::Character(character) if matches!(character, 't' | 'T') => {
                (target, ShellCommand::BeginExplorerNewTextFile)
            }
            InputKey::Character(character) if matches!(character, 'r' | 'R') => {
                (target, ShellCommand::BeginExplorerRename)
            }
            InputKey::Character('/') => (target, ShellCommand::BeginExplorerSearch),
            _ => (target, ShellCommand::RecordInput),
        }
    }

    fn route_user_management_key(&self, key: &KeyInput) -> (RoutedTarget, ShellCommand) {
        let target = RoutedTarget::Component(ShellComponent::UserManagement);
        if self.user_management_mode != UserManagementMode::Browse {
            return match &key.key {
                InputKey::Escape => (target, ShellCommand::CancelUserManagementForm),
                InputKey::BackTab => (target, ShellCommand::UserManagementFocusPrevious),
                InputKey::Tab if key.modifiers.shift => {
                    (target, ShellCommand::UserManagementFocusPrevious)
                }
                InputKey::Tab | InputKey::Down => (target, ShellCommand::UserManagementFocusNext),
                InputKey::Up => (target, ShellCommand::UserManagementFocusPrevious),
                InputKey::Enter => (target, ShellCommand::SubmitUserManagementForm),
                InputKey::Backspace => (target, ShellCommand::UserManagementBackspace),
                InputKey::Character(character) => {
                    (target, ShellCommand::AppendUserManagementChar(*character))
                }
                _ => (target, ShellCommand::RecordInput),
            };
        }

        if !self.can_manage_all_users() {
            return match &key.key {
                InputKey::Escape => (RoutedTarget::Global, ShellCommand::CloseUserManagement),
                InputKey::Up => (target, ShellCommand::UserManagementPrevious),
                InputKey::Down => (target, ShellCommand::UserManagementNext),
                InputKey::Character('e') | InputKey::Character('E') => {
                    (target, ShellCommand::EditManagedUserInfo)
                }
                InputKey::Character('r') | InputKey::Character('R') => {
                    (target, ShellCommand::ResetManagedPassword)
                }
                InputKey::Character('x') | InputKey::Character('X') | InputKey::Delete => {
                    (target, ShellCommand::DeleteManagedUser)
                }
                _ => (target, ShellCommand::RecordInput),
            };
        }

        match &key.key {
            InputKey::Escape => (RoutedTarget::Global, ShellCommand::CloseUserManagement),
            InputKey::Up => (target, ShellCommand::UserManagementPrevious),
            InputKey::Down => (target, ShellCommand::UserManagementNext),
            InputKey::Character('n') | InputKey::Character('N') => {
                (target, ShellCommand::CreateManagedUser(UserRole::User))
            }
            InputKey::Character('e') | InputKey::Character('E') => {
                (target, ShellCommand::EditManagedUserInfo)
            }
            InputKey::Character('a') | InputKey::Character('A') => {
                (target, ShellCommand::CreateManagedUser(UserRole::Admin))
            }
            InputKey::Character('g') | InputKey::Character('G') => {
                (target, ShellCommand::CreateManagedUser(UserRole::Debug))
            }
            InputKey::Character('d') | InputKey::Character('D') => {
                (target, ShellCommand::DisableManagedUser)
            }
            InputKey::Character('u') | InputKey::Character('U') => {
                (target, ShellCommand::UnlockManagedUser)
            }
            InputKey::Character('r') | InputKey::Character('R') => {
                (target, ShellCommand::ResetManagedPassword)
            }
            InputKey::Character('c') | InputKey::Character('C') => {
                (target, ShellCommand::CycleManagedRole)
            }
            InputKey::Character('x') | InputKey::Character('X') | InputKey::Delete => {
                (target, ShellCommand::DeleteManagedUser)
            }
            _ => (target, ShellCommand::RecordInput),
        }
    }

    fn route_exit_confirm_key(&self, key: &KeyInput) -> (RoutedTarget, ShellCommand) {
        let target = RoutedTarget::Modal(ShellComponent::ExitDialog);

        if matches!(&key.key, InputKey::BackTab)
            || (matches!(&key.key, InputKey::Tab) && key.modifiers.shift)
        {
            return (target, ShellCommand::FocusPrevious);
        }
        if matches!(&key.key, InputKey::Tab) {
            return (target, ShellCommand::FocusNext);
        }

        if key.is_character('y') || key.is_character('Y') || matches!(&key.key, InputKey::Enter) {
            return (target, ShellCommand::ConfirmExit);
        }

        if key.is_character('n') || key.is_character('N') || matches!(&key.key, InputKey::Escape) {
            return (target, ShellCommand::CancelExit);
        }

        (target, ShellCommand::CaptureOverlayInput)
    }

    fn route_popup_key(&self, key: &KeyInput) -> (RoutedTarget, ShellCommand) {
        let target = RoutedTarget::Popup(ShellComponent::ContextMenu);

        if matches!(&key.key, InputKey::Escape) {
            return (target, ShellCommand::ClosePopup);
        }
        if matches!(&key.key, InputKey::BackTab)
            || (matches!(&key.key, InputKey::Tab) && key.modifiers.shift)
        {
            return (target, ShellCommand::FocusPrevious);
        }
        if matches!(&key.key, InputKey::Tab) {
            return (target, ShellCommand::FocusNext);
        }

        (target, ShellCommand::CaptureOverlayInput)
    }

    fn route_mouse_input(
        &mut self,
        mouse: MouseInput,
        received_at: Instant,
    ) -> (RoutedTarget, ShellCommand) {
        let coordinates = mouse.coordinates();
        let hit_target = self.hit_map.target_at(coordinates);

        if self.time_sync_dialog_visible {
            return self.route_time_sync_dialog_mouse(mouse, hit_target);
        }

        if self.active_screen() == ShellScreen::ExitConfirm {
            let routed_target = if hit_target == Some(ShellComponent::ExitDialog) {
                RoutedTarget::Modal(ShellComponent::ExitDialog)
            } else {
                RoutedTarget::Modal(ShellComponent::ExitDialog)
            };
            return (routed_target, ShellCommand::CaptureOverlayInput);
        }

        if self.active_screen() == ShellScreen::FirstRunSetup {
            return self.route_setup_mouse(mouse, hit_target);
        }

        if self.active_screen() == ShellScreen::Login {
            return self.route_login_mouse(mouse, hit_target);
        }

        if self.active_popup.is_some() {
            return self.route_popup_mouse(mouse, hit_target, received_at);
        }

        match mouse {
            MouseInput::Moved { .. } => (target_route(hit_target), ShellCommand::Hover(hit_target)),
            MouseInput::Down {
                button: PointerButton::Right,
                ..
            } => {
                self.last_click = None;
                (
                    target_route(hit_target),
                    ShellCommand::OpenContextMenu {
                        target: hit_target,
                        coordinates,
                    },
                )
            }
            MouseInput::Down { button, .. } => {
                if let Some(target) = hit_target {
                    let click = self.register_click(hit_target, coordinates, button, received_at);
                    if target == ShellComponent::ClockButton {
                        return (
                            RoutedTarget::Component(target),
                            if button == PointerButton::Left {
                                self.clock_button_activation_command()
                            } else {
                                ShellCommand::Activate {
                                    target,
                                    coordinates,
                                    click,
                                }
                            },
                        );
                    }
                    if self.active_screen() == ShellScreen::Home && target == ShellComponent::Home {
                        return (
                            RoutedTarget::Component(target),
                            ShellCommand::ActivateHomeEntryAt(coordinates, click),
                        );
                    }

                    (
                        RoutedTarget::Component(target),
                        ShellCommand::Activate {
                            target,
                            coordinates,
                            click,
                        },
                    )
                } else {
                    (RoutedTarget::None, ShellCommand::RecordInput)
                }
            }
            _ => (target_route(hit_target), ShellCommand::RecordInput),
        }
    }

    fn route_time_sync_dialog_mouse(
        &mut self,
        mouse: MouseInput,
        hit_target: Option<ShellComponent>,
    ) -> (RoutedTarget, ShellCommand) {
        match mouse {
            MouseInput::Moved { .. } => (
                RoutedTarget::Modal(ShellComponent::TimeSyncDialog),
                ShellCommand::Hover(hit_target),
            ),
            _ if mouse.down_button().is_some() => (
                RoutedTarget::Modal(ShellComponent::TimeSyncDialog),
                ShellCommand::CloseTimeSyncDialog,
            ),
            _ => (
                RoutedTarget::Modal(ShellComponent::TimeSyncDialog),
                ShellCommand::CaptureOverlayInput,
            ),
        }
    }

    fn route_setup_mouse(
        &mut self,
        mouse: MouseInput,
        hit_target: Option<ShellComponent>,
    ) -> (RoutedTarget, ShellCommand) {
        let coordinates = mouse.coordinates();

        match mouse {
            MouseInput::Moved { .. } => (target_route(hit_target), ShellCommand::Hover(hit_target)),
            MouseInput::Scroll {
                direction: ScrollDirection::Up,
                ..
            } if hit_target == Some(ShellComponent::SetupLanguage)
                && self.setup_step == tundra_ui::SetupStep::Language =>
            {
                (
                    RoutedTarget::Component(ShellComponent::SetupLanguage),
                    ShellCommand::SetupPreviousLanguage,
                )
            }
            MouseInput::Scroll {
                direction: ScrollDirection::Down,
                ..
            } if hit_target == Some(ShellComponent::SetupLanguage)
                && self.setup_step == tundra_ui::SetupStep::Language =>
            {
                (
                    RoutedTarget::Component(ShellComponent::SetupLanguage),
                    ShellCommand::SetupNextLanguage,
                )
            }
            MouseInput::Scroll {
                direction: ScrollDirection::Up,
                ..
            } if hit_target == Some(ShellComponent::SetupTimezone)
                && self.setup_step == tundra_ui::SetupStep::Timezone =>
            {
                (
                    RoutedTarget::Component(ShellComponent::SetupTimezone),
                    ShellCommand::SetupPreviousTimezone,
                )
            }
            MouseInput::Scroll {
                direction: ScrollDirection::Down,
                ..
            } if hit_target == Some(ShellComponent::SetupTimezone)
                && self.setup_step == tundra_ui::SetupStep::Timezone =>
            {
                (
                    RoutedTarget::Component(ShellComponent::SetupTimezone),
                    ShellCommand::SetupNextTimezone,
                )
            }
            MouseInput::Down {
                button: PointerButton::Left,
                ..
            } => {
                if let Some(target) = hit_target
                    && setup_field_for_component(target).is_some()
                    && setup_component_active_for_step(target, self.setup_step)
                {
                    return (
                        RoutedTarget::Component(target),
                        ShellCommand::ActivateSetup {
                            target,
                            coordinates,
                        },
                    );
                }

                (RoutedTarget::None, ShellCommand::RecordInput)
            }
            MouseInput::Down {
                button: PointerButton::Right,
                ..
            } => {
                self.last_click = None;
                (target_route(hit_target), ShellCommand::CaptureOverlayInput)
            }
            _ => (target_route(hit_target), ShellCommand::RecordInput),
        }
    }

    fn route_login_mouse(
        &mut self,
        mouse: MouseInput,
        hit_target: Option<ShellComponent>,
    ) -> (RoutedTarget, ShellCommand) {
        let coordinates = mouse.coordinates();

        match mouse {
            MouseInput::Moved { .. } => (target_route(hit_target), ShellCommand::Hover(hit_target)),
            MouseInput::Scroll {
                direction: ScrollDirection::Up,
                ..
            } if hit_target == Some(ShellComponent::LoginUserList) => (
                RoutedTarget::Component(ShellComponent::LoginUserList),
                ShellCommand::LoginPreviousUser,
            ),
            MouseInput::Scroll {
                direction: ScrollDirection::Down,
                ..
            } if hit_target == Some(ShellComponent::LoginUserList) => (
                RoutedTarget::Component(ShellComponent::LoginUserList),
                ShellCommand::LoginNextUser,
            ),
            MouseInput::Down {
                button: PointerButton::Left,
                ..
            } => {
                if let Some(
                    target @ (ShellComponent::LoginUserList
                    | ShellComponent::LoginUsername
                    | ShellComponent::LoginPassword),
                ) = hit_target
                {
                    return (
                        RoutedTarget::Component(target),
                        ShellCommand::ActivateLogin {
                            target,
                            coordinates,
                        },
                    );
                }

                (RoutedTarget::None, ShellCommand::RecordInput)
            }
            MouseInput::Down {
                button: PointerButton::Right,
                ..
            } => {
                self.last_click = None;
                (target_route(hit_target), ShellCommand::CaptureOverlayInput)
            }
            _ => (target_route(hit_target), ShellCommand::RecordInput),
        }
    }

    fn route_popup_mouse(
        &mut self,
        mouse: MouseInput,
        hit_target: Option<ShellComponent>,
        received_at: Instant,
    ) -> (RoutedTarget, ShellCommand) {
        let coordinates = mouse.coordinates();

        if hit_target != Some(ShellComponent::ContextMenu) {
            if mouse.down_button().is_some() {
                return (RoutedTarget::OutsidePopup, ShellCommand::ClosePopup);
            }

            return (
                RoutedTarget::Popup(ShellComponent::ContextMenu),
                ShellCommand::CaptureOverlayInput,
            );
        }

        match mouse {
            MouseInput::Moved { .. } => (
                RoutedTarget::Popup(ShellComponent::ContextMenu),
                ShellCommand::Hover(Some(ShellComponent::ContextMenu)),
            ),
            MouseInput::Down { button, .. } => {
                let click = self.register_click(
                    Some(ShellComponent::ContextMenu),
                    coordinates,
                    button,
                    received_at,
                );
                (
                    RoutedTarget::Popup(ShellComponent::ContextMenu),
                    ShellCommand::Activate {
                        target: ShellComponent::ContextMenu,
                        coordinates,
                        click,
                    },
                )
            }
            _ => (
                RoutedTarget::Popup(ShellComponent::ContextMenu),
                ShellCommand::CaptureOverlayInput,
            ),
        }
    }

    fn register_click(
        &mut self,
        target: Option<ShellComponent>,
        coordinates: CellPosition,
        button: PointerButton,
        received_at: Instant,
    ) -> ClickKind {
        if button != PointerButton::Left {
            self.last_click = None;
            return ClickKind::Single;
        }

        let is_double_click = self
            .last_click
            .map(|last_click| {
                last_click.target == target
                    && coordinates_within_tolerance(last_click.coordinates, coordinates)
                    && received_at
                        .checked_duration_since(last_click.at)
                        .map(|elapsed| elapsed <= DOUBLE_CLICK_INTERVAL)
                        .unwrap_or(false)
            })
            .unwrap_or(false);

        if is_double_click {
            self.last_click = None;
            ClickKind::Double
        } else {
            self.last_click = Some(TimedClick {
                target,
                coordinates,
                at: received_at,
            });
            ClickKind::Single
        }
    }

    fn record_input_diagnostics(&mut self, routed: &RoutedEvent) {
        match &routed.input {
            InputEvent::Key(key) => {
                self.last_key_event = Some(key.label());
            }
            InputEvent::Mouse(mouse) => {
                let mut summary = self.record_mouse_drag_diagnostics(*mouse);
                if matches!(
                    &routed.command,
                    ShellCommand::Activate {
                        click: ClickKind::Double,
                        ..
                    }
                ) {
                    if let MouseInput::Down { button, .. } = *mouse {
                        summary = format!("Mouse DoubleClick {}", button.label());
                    }
                }

                self.last_mouse_event = Some(summary);
                self.mouse_coordinates = Some(mouse.coordinates());
                self.mouse_scroll_direction = mouse
                    .scroll_direction()
                    .map(|direction| direction.label().to_string());
            }
            InputEvent::Resize { width, height } => {
                self.last_resize_event = Some(format!("{width}x{height}"));
            }
            InputEvent::FocusGained => {
                self.last_key_event = Some("FocusGained".to_string());
            }
            InputEvent::FocusLost => {
                self.last_key_event = Some("FocusLost".to_string());
            }
            InputEvent::Paste(value) => {
                self.last_key_event = Some(format!("Paste({} chars)", value.chars().count()));
            }
            InputEvent::Tick | InputEvent::Shutdown => {}
        }
    }

    fn record_mouse_drag_diagnostics(&mut self, mouse: MouseInput) -> String {
        match mouse {
            MouseInput::Down {
                button,
                coordinates,
                ..
            } => {
                self.drag_tracker = Some(DragTracker {
                    button,
                    last_coordinates: coordinates,
                });
                self.mouse_drag_direction = None;
                mouse.summary()
            }
            MouseInput::Drag {
                button,
                coordinates,
                ..
            } => {
                let previous = self
                    .drag_tracker
                    .filter(|tracker| tracker.button == button)
                    .map(|tracker| tracker.last_coordinates);
                let direction =
                    previous.and_then(|previous| drag_direction_between(previous, coordinates));

                self.drag_tracker = Some(DragTracker {
                    button,
                    last_coordinates: coordinates,
                });
                self.mouse_drag_direction =
                    direction.map(|direction| direction.label().to_string());

                if let Some(direction) = direction {
                    format!("Mouse Drag {} to {}", button.label(), direction.label())
                } else {
                    mouse.summary()
                }
            }
            MouseInput::Up { .. } | MouseInput::Moved { .. } | MouseInput::Scroll { .. } => {
                self.drag_tracker = None;
                self.mouse_drag_direction = None;
                mouse.summary()
            }
        }
    }

    fn refresh_hit_map(&mut self) {
        self.hit_map_generation = self.hit_map_generation.saturating_add(1);
        if self.active_screen() == ShellScreen::Login {
            self.sync_login_selection();
        }
        let time_button_label = self.status_time_button_label();
        self.hit_map = build_shell_hit_map(
            self.terminal_size,
            self.active_screen(),
            self.active_popup,
            self.setup_step.clone(),
            self.hit_map_generation,
            time_button_label.as_deref(),
            self.time_sync_dialog_visible,
        );
        self.sync_home_entry_selection();

        let focus_order = self.focus_order();
        if !focus_order.contains(&self.focused_component) {
            self.focused_component = focus_order.first().copied().unwrap_or(ShellComponent::Home);
            if let Some(field) = setup_field_for_component(self.focused_component) {
                self.setup_focused_field = field;
            }
        }
    }

    fn focus_order(&self) -> Vec<ShellComponent> {
        if self.time_sync_dialog_visible {
            return vec![ShellComponent::TimeSyncDialog];
        }
        if self.active_screen() == ShellScreen::ExitConfirm {
            return vec![ShellComponent::ExitDialog];
        }
        if self.active_screen() == ShellScreen::FirstRunSetup {
            return match self.setup_step {
                tundra_ui::SetupStep::Language => vec![ShellComponent::SetupLanguage],
                tundra_ui::SetupStep::Timezone => vec![ShellComponent::SetupTimezone],
                tundra_ui::SetupStep::Admin => vec![
                    ShellComponent::SetupAdminUsername,
                    ShellComponent::SetupAdminPassword,
                    ShellComponent::SetupAdminPasswordConfirm,
                    ShellComponent::SetupAdminHint,
                    ShellComponent::SetupSubmit,
                ],
            };
        }
        if self.active_screen() == ShellScreen::Login {
            return vec![ShellComponent::LoginUserList, ShellComponent::LoginPassword];
        }
        if self.active_screen() == ShellScreen::BootstrapAdmin {
            return vec![
                ShellComponent::BootstrapUsername,
                ShellComponent::BootstrapPassword,
            ];
        }
        if self.active_screen() == ShellScreen::UserManagement {
            return vec![ShellComponent::UserManagement];
        }
        if self.active_screen() == ShellScreen::Explorer {
            return vec![ShellComponent::Explorer];
        }
        if self.active_screen() == ShellScreen::Clock {
            return vec![ShellComponent::Clock];
        }
        if self.active_popup.is_some() {
            return vec![ShellComponent::ContextMenu];
        }
        if self
            .hit_map
            .regions()
            .iter()
            .any(|region| region.component == ShellComponent::CompactHome)
        {
            return vec![ShellComponent::CompactHome];
        }

        vec![
            ShellComponent::Home,
            ShellComponent::ClockButton,
            ShellComponent::StatusBar,
            ShellComponent::TopBar,
        ]
    }

    fn move_focus(&mut self, direction: i8) {
        let focus_order = self.focus_order();
        if focus_order.is_empty() {
            return;
        }

        let current_index = focus_order
            .iter()
            .position(|component| *component == self.focused_component)
            .unwrap_or(0);
        let len = focus_order.len() as isize;
        let next_index = (current_index as isize + direction as isize).rem_euclid(len) as usize;
        self.focused_component = focus_order[next_index];
    }

    fn focus_component(&mut self, component: ShellComponent) {
        if self.focus_order().contains(&component) {
            self.focused_component = component;
        }
    }

    fn apply_restored_session(&mut self, session: &ShellRestoredSession) {
        self.screen_stack = vec![ShellScreen::Home];
        self.active_popup = None;

        let focus_order = self.focus_order();
        self.focused_component = if focus_order.contains(&session.focused_component) {
            session.focused_component
        } else {
            focus_order.first().copied().unwrap_or(ShellComponent::Home)
        };
        self.refresh_hit_map();
    }

    fn pop_to_home(&mut self) {
        self.screen_stack.truncate(1);
        if self.screen_stack.is_empty() {
            self.screen_stack.push(ShellScreen::Home);
        }
        self.focused_component = ShellComponent::Home;
        self.refresh_hit_map();
    }

    fn cancel_exit_confirmation(&mut self) {
        if self.active_screen() == ShellScreen::ExitConfirm {
            self.screen_stack.pop();
        }
        if self.screen_stack.is_empty() {
            self.screen_stack.push(ShellScreen::Home);
        }
        self.refresh_hit_map();
    }
}

pub fn default_shell_shortcuts() -> Vec<ShellShortcut> {
    vec![
        ShellShortcut {
            scope: ShortcutScope::Global,
            binding: KeyBinding::from(&KeyInput::from_label("Ctrl+C")),
            command: ShellCommand::Shutdown,
        },
        ShellShortcut {
            scope: ShortcutScope::Global,
            binding: KeyBinding::from(&KeyInput::from_label("Tab")),
            command: ShellCommand::FocusNext,
        },
        ShellShortcut {
            scope: ShortcutScope::Global,
            binding: KeyBinding::from(&KeyInput::from_label("Shift+Tab")),
            command: ShellCommand::FocusPrevious,
        },
        ShellShortcut {
            scope: ShortcutScope::Screen(ShellScreen::Home),
            binding: KeyBinding::from(&KeyInput::from_label("q")),
            command: ShellCommand::RequestExit,
        },
        ShellShortcut {
            scope: ShortcutScope::Screen(ShellScreen::Home),
            binding: KeyBinding::from(&KeyInput::from_label("Esc")),
            command: ShellCommand::RequestExit,
        },
        ShellShortcut {
            scope: ShortcutScope::Screen(ShellScreen::ExitConfirm),
            binding: KeyBinding::from(&KeyInput::from_label("y")),
            command: ShellCommand::ConfirmExit,
        },
        ShellShortcut {
            scope: ShortcutScope::Screen(ShellScreen::ExitConfirm),
            binding: KeyBinding::from(&KeyInput::from_label("Y")),
            command: ShellCommand::ConfirmExit,
        },
        ShellShortcut {
            scope: ShortcutScope::Screen(ShellScreen::ExitConfirm),
            binding: KeyBinding::from(&KeyInput::from_label("Enter")),
            command: ShellCommand::ConfirmExit,
        },
        ShellShortcut {
            scope: ShortcutScope::Screen(ShellScreen::ExitConfirm),
            binding: KeyBinding::from(&KeyInput::from_label("n")),
            command: ShellCommand::CancelExit,
        },
        ShellShortcut {
            scope: ShortcutScope::Screen(ShellScreen::ExitConfirm),
            binding: KeyBinding::from(&KeyInput::from_label("N")),
            command: ShellCommand::CancelExit,
        },
        ShellShortcut {
            scope: ShortcutScope::Screen(ShellScreen::ExitConfirm),
            binding: KeyBinding::from(&KeyInput::from_label("Esc")),
            command: ShellCommand::CancelExit,
        },
    ]
}

pub fn detect_shortcut_conflicts(shortcuts: &[ShellShortcut]) -> Vec<ShortcutConflict> {
    let mut conflicts = Vec::new();

    for (index, first) in shortcuts.iter().enumerate() {
        for second in shortcuts.iter().skip(index + 1) {
            if first.scope == second.scope
                && first.binding == second.binding
                && first.command != second.command
            {
                conflicts.push(ShortcutConflict {
                    scope: first.scope.clone(),
                    binding: first.binding.clone(),
                    first: first.command.clone(),
                    second: second.command.clone(),
                });
            }
        }
    }

    conflicts
}

fn build_shell_hit_map(
    terminal_size: CellPosition,
    active_screen: ShellScreen,
    active_popup: Option<ShellPopup>,
    setup_step: tundra_ui::SetupStep,
    generation: u64,
    time_button_label: Option<&str>,
    time_sync_dialog_visible: bool,
) -> ShellHitMap {
    let (width, height) = terminal_size;
    let area = Rect::new(0, 0, width, height);
    let mut regions = Vec::new();

    match tundra_ui::compute_shell_layout(area) {
        tundra_ui::ShellLayout::Compact(compact) => {
            regions.push(ShellHitRegion {
                component: ShellComponent::CompactHome,
                area: compact,
            });
        }
        tundra_ui::ShellLayout::Full { top, main, status } => {
            regions.push(ShellHitRegion {
                component: ShellComponent::TopBar,
                area: top,
            });
            match active_screen {
                ShellScreen::FirstRunSetup => {
                    regions.extend(setup_hit_regions(main, setup_step));
                }
                ShellScreen::Login => {
                    let users = tundra_ui::login_user_list_area(main);
                    let username = tundra_ui::login_selected_username_area(main);
                    let password = tundra_ui::login_password_area(main);
                    regions.push(ShellHitRegion {
                        component: ShellComponent::LoginUserList,
                        area: users,
                    });
                    regions.push(ShellHitRegion {
                        component: ShellComponent::LoginUsername,
                        area: username,
                    });
                    regions.push(ShellHitRegion {
                        component: ShellComponent::LoginPassword,
                        area: password,
                    });
                }
                ShellScreen::BootstrapAdmin => {
                    let (username, password) = auth_field_rects(main);
                    regions.push(ShellHitRegion {
                        component: ShellComponent::BootstrapUsername,
                        area: username,
                    });
                    regions.push(ShellHitRegion {
                        component: ShellComponent::BootstrapPassword,
                        area: password,
                    });
                }
                ShellScreen::UserManagement => {
                    regions.push(ShellHitRegion {
                        component: ShellComponent::UserManagement,
                        area: main,
                    });
                }
                ShellScreen::Explorer => {
                    regions.push(ShellHitRegion {
                        component: ShellComponent::Explorer,
                        area: main,
                    });
                }
                ShellScreen::Clock => {
                    regions.push(ShellHitRegion {
                        component: ShellComponent::Clock,
                        area: main,
                    });
                }
                _ => {
                    regions.push(ShellHitRegion {
                        component: ShellComponent::Home,
                        area: main,
                    });
                }
            }
            regions.push(ShellHitRegion {
                component: ShellComponent::StatusBar,
                area: status,
            });
            if clock_button_active_for_screen(active_screen)
                && let Some(label) = time_button_label
            {
                let button = tundra_ui::status_time_button_area(status, label);
                if button.width > 0 && button.height > 0 {
                    regions.push(ShellHitRegion {
                        component: ShellComponent::ClockButton,
                        area: button,
                    });
                }
            }
        }
    }

    if let Some(popup) = active_popup {
        regions.push(ShellHitRegion {
            component: ShellComponent::ContextMenu,
            area: popup_rect(terminal_size, popup.anchor),
        });
    }

    if active_screen == ShellScreen::ExitConfirm {
        regions.push(ShellHitRegion {
            component: ShellComponent::ExitDialog,
            area: centered_rect(area, width.min(46), height.min(7)),
        });
    }

    if time_sync_dialog_visible {
        regions.push(ShellHitRegion {
            component: ShellComponent::TimeSyncDialog,
            area: centered_rect(area, width.min(34), height.min(5)),
        });
    }

    ShellHitMap::new(terminal_size, generation, regions)
}

fn auth_field_rects(main: Rect) -> (Rect, Rect) {
    let x = main.x.saturating_add(1);
    let width = main.width.saturating_sub(2);
    let username_y = main.y.saturating_add(3);
    let password_y = main.y.saturating_add(4);

    (
        Rect::new(x, username_y, width, 1),
        Rect::new(x, password_y, width, 1),
    )
}

fn setup_hit_regions(
    main: Rect,
    setup_step: tundra_ui::SetupStep,
) -> impl IntoIterator<Item = ShellHitRegion> {
    match setup_step {
        tundra_ui::SetupStep::Language => vec![ShellHitRegion {
            component: ShellComponent::SetupLanguage,
            area: setup_language_list_rect(main),
        }],
        tundra_ui::SetupStep::Timezone => vec![ShellHitRegion {
            component: ShellComponent::SetupTimezone,
            area: tundra_ui::setup_timezone_list_area(main),
        }],
        tundra_ui::SetupStep::Admin => vec![
            ShellHitRegion {
                component: ShellComponent::SetupAdminUsername,
                area: tundra_ui::setup_admin_field_area(main, tundra_ui::SetupField::AdminUsername),
            },
            ShellHitRegion {
                component: ShellComponent::SetupAdminPassword,
                area: tundra_ui::setup_admin_field_area(main, tundra_ui::SetupField::AdminPassword),
            },
            ShellHitRegion {
                component: ShellComponent::SetupAdminPasswordConfirm,
                area: tundra_ui::setup_admin_field_area(
                    main,
                    tundra_ui::SetupField::AdminPasswordConfirm,
                ),
            },
            ShellHitRegion {
                component: ShellComponent::SetupAdminHint,
                area: tundra_ui::setup_admin_field_area(main, tundra_ui::SetupField::PasswordHint),
            },
            ShellHitRegion {
                component: ShellComponent::SetupSubmit,
                area: tundra_ui::setup_admin_field_area(main, tundra_ui::SetupField::Submit),
            },
        ],
    }
}

fn setup_language_list_row_at(
    terminal_size: CellPosition,
    coordinates: CellPosition,
) -> Option<usize> {
    let main = setup_main_rect(terminal_size)?;
    setup_row_at(setup_language_list_rect(main), coordinates)
}

fn setup_timezone_list_row_at(
    terminal_size: CellPosition,
    coordinates: CellPosition,
) -> Option<usize> {
    let main = setup_main_rect(terminal_size)?;
    setup_row_at(tundra_ui::setup_timezone_list_area(main), coordinates)
}

fn setup_timezone_visible_row_count(terminal_size: CellPosition) -> usize {
    setup_main_rect(terminal_size)
        .map(tundra_ui::setup_timezone_visible_rows)
        .unwrap_or(0)
}

fn login_user_visible_row_count(terminal_size: CellPosition) -> usize {
    setup_main_rect(terminal_size)
        .map(tundra_ui::login_user_list_visible_rows)
        .unwrap_or(0)
}

fn setup_main_rect(terminal_size: CellPosition) -> Option<Rect> {
    let area = Rect::new(0, 0, terminal_size.0, terminal_size.1);
    let tundra_ui::ShellLayout::Full { main, .. } = tundra_ui::compute_shell_layout(area) else {
        return None;
    };

    Some(main)
}

fn setup_language_list_rect(main: Rect) -> Rect {
    tundra_ui::setup_language_list_area(main, tundra_ui::setup_language_options().len())
}

fn setup_row_at(rect: Rect, coordinates: CellPosition) -> Option<usize> {
    rect_contains(rect, coordinates).then(|| coordinates.1.saturating_sub(rect.y) as usize)
}

fn login_user_list_row_at(terminal_size: CellPosition, coordinates: CellPosition) -> Option<usize> {
    let main = setup_main_rect(terminal_size)?;
    let rect = tundra_ui::login_user_list_area(main);
    if rect.height <= 2 || !rect_contains(rect, coordinates) {
        return None;
    }

    let row = coordinates.1.checked_sub(rect.y.saturating_add(1))? as usize;
    (row < rect.height.saturating_sub(2) as usize).then_some(row)
}

fn default_login_user_index(users: &[ShellLoginUser]) -> usize {
    users
        .iter()
        .enumerate()
        .filter_map(|(index, user)| {
            user.last_login_at_epoch_ms
                .map(|last_login| (index, last_login))
        })
        .max_by_key(|(_, last_login)| *last_login)
        .map(|(index, _)| index)
        .unwrap_or(0)
}

fn setup_field_for_component(component: ShellComponent) -> Option<tundra_ui::SetupField> {
    match component {
        ShellComponent::SetupLanguage => Some(tundra_ui::SetupField::LanguageList),
        ShellComponent::SetupTimezone => Some(tundra_ui::SetupField::TimezoneList),
        ShellComponent::SetupAdminUsername => Some(tundra_ui::SetupField::AdminUsername),
        ShellComponent::SetupAdminPassword => Some(tundra_ui::SetupField::AdminPassword),
        ShellComponent::SetupAdminPasswordConfirm => {
            Some(tundra_ui::SetupField::AdminPasswordConfirm)
        }
        ShellComponent::SetupAdminHint => Some(tundra_ui::SetupField::PasswordHint),
        ShellComponent::SetupSubmit => Some(tundra_ui::SetupField::Submit),
        _ => None,
    }
}

fn setup_component_for_field(field: tundra_ui::SetupField) -> ShellComponent {
    match field {
        tundra_ui::SetupField::LanguageList => ShellComponent::SetupLanguage,
        tundra_ui::SetupField::TimezoneList => ShellComponent::SetupTimezone,
        tundra_ui::SetupField::AdminUsername => ShellComponent::SetupAdminUsername,
        tundra_ui::SetupField::AdminPassword => ShellComponent::SetupAdminPassword,
        tundra_ui::SetupField::AdminPasswordConfirm => ShellComponent::SetupAdminPasswordConfirm,
        tundra_ui::SetupField::PasswordHint => ShellComponent::SetupAdminHint,
        tundra_ui::SetupField::Submit => ShellComponent::SetupSubmit,
    }
}

fn setup_component_active_for_step(component: ShellComponent, step: tundra_ui::SetupStep) -> bool {
    matches!(
        (step, component),
        (
            tundra_ui::SetupStep::Language,
            ShellComponent::SetupLanguage
        ) | (
            tundra_ui::SetupStep::Timezone,
            ShellComponent::SetupTimezone
        ) | (
            tundra_ui::SetupStep::Admin,
            ShellComponent::SetupAdminUsername
                | ShellComponent::SetupAdminPassword
                | ShellComponent::SetupAdminPasswordConfirm
                | ShellComponent::SetupAdminHint
                | ShellComponent::SetupSubmit
        )
    )
}

fn setup_admin_text_field(field: tundra_ui::SetupField) -> bool {
    matches!(
        field,
        tundra_ui::SetupField::AdminUsername
            | tundra_ui::SetupField::AdminPassword
            | tundra_ui::SetupField::AdminPasswordConfirm
            | tundra_ui::SetupField::PasswordHint
    )
}

fn setup_password_requirements(
    username: &str,
    password: &str,
    password_confirm: &str,
) -> Vec<tundra_ui::SetupPasswordRequirementViewModel> {
    let normalized_username = username.trim().to_ascii_lowercase();
    let normalized_password = password.trim().to_ascii_lowercase();

    vec![
        tundra_ui::SetupPasswordRequirementViewModel::new(
            format!("At least {PASSWORD_MIN_LEN} characters"),
            password.len() >= PASSWORD_MIN_LEN,
        ),
        tundra_ui::SetupPasswordRequirementViewModel::new(
            format!("At most {PASSWORD_MAX_LEN} characters"),
            password.len() <= PASSWORD_MAX_LEN,
        ),
        tundra_ui::SetupPasswordRequirementViewModel::new("Not blank", !password.trim().is_empty()),
        tundra_ui::SetupPasswordRequirementViewModel::new(
            "Different from username",
            normalized_username != normalized_password,
        ),
        tundra_ui::SetupPasswordRequirementViewModel::new(
            "Passwords match",
            !password.is_empty() && password == password_confirm,
        ),
    ]
}

fn setup_language_code_at(
    options: &[tundra_ui::SetupLanguageOption],
    index: usize,
) -> Option<String> {
    options
        .get(index)
        .or_else(|| options.first())
        .map(|option| option.code.clone())
}

fn setup_timezone_id_at(
    options: &[tundra_ui::SetupTimezoneOption],
    index: usize,
) -> Option<String> {
    options
        .get(index)
        .or_else(|| options.first())
        .map(|option| option.id.clone())
}

fn popup_rect(terminal_size: CellPosition, anchor: CellPosition) -> Rect {
    let width = terminal_size.0.min(24);
    let height = terminal_size.1.min(5);
    let x = anchor.0.min(terminal_size.0.saturating_sub(width));
    let y = anchor.1.min(terminal_size.1.saturating_sub(height));

    Rect::new(x, y, width, height)
}

fn target_route(target: Option<ShellComponent>) -> RoutedTarget {
    target.map_or(RoutedTarget::None, RoutedTarget::Component)
}

fn rect_contains(rect: Rect, coordinates: CellPosition) -> bool {
    let (x, y) = coordinates;
    x >= rect.x
        && x < rect.x.saturating_add(rect.width)
        && y >= rect.y
        && y < rect.y.saturating_add(rect.height)
}

fn coordinates_within_tolerance(first: CellPosition, second: CellPosition) -> bool {
    first.0.abs_diff(second.0) <= DOUBLE_CLICK_CELL_TOLERANCE
        && first.1.abs_diff(second.1) <= DOUBLE_CLICK_CELL_TOLERANCE
}

fn drag_direction_between(previous: CellPosition, current: CellPosition) -> Option<DragDirection> {
    let delta_x = current.0 as i32 - previous.0 as i32;
    let delta_y = current.1 as i32 - previous.1 as i32;

    if delta_x == 0 && delta_y == 0 {
        return None;
    }

    if delta_x.abs() >= delta_y.abs() {
        if delta_x > 0 {
            Some(DragDirection::Right)
        } else {
            Some(DragDirection::Left)
        }
    } else if delta_y > 0 {
        Some(DragDirection::Down)
    } else {
        Some(DragDirection::Up)
    }
}

fn centered_rect(area: Rect, width: u16, height: u16) -> Rect {
    Rect::new(
        area.x.saturating_add(area.width.saturating_sub(width) / 2),
        area.y
            .saturating_add(area.height.saturating_sub(height) / 2),
        width,
        height,
    )
}

fn clock_display_label(display: tundra_weathr::network_clock::ClockDisplay) -> String {
    format!(
        "{} {:02}:{:02}",
        display.date,
        display.time.hour(),
        display.time.minute()
    )
}

fn clock_button_active_for_screen(screen: ShellScreen) -> bool {
    matches!(
        screen,
        ShellScreen::Home
            | ShellScreen::Explorer
            | ShellScreen::UserManagement
            | ShellScreen::Clock
    )
}

fn startup_clock_timezone_id(startup: &ShellStartupState) -> Option<String> {
    startup
        .storage_manager
        .as_ref()
        .and_then(|storage| storage.load_config().ok())
        .map(|config| config.timezone)
        .or_else(|| Some("UTC".to_string()))
}

fn system_time_label(value: SystemTime) -> String {
    value
        .duration_since(UNIX_EPOCH)
        .map(|duration| format!("unix:{}", duration.as_secs()))
        .unwrap_or_else(|_| "unknown".to_string())
}

fn explorer_attribute_labels(attributes: &FileAttributes) -> Vec<String> {
    let mut labels = Vec::new();
    if attributes.readonly {
        labels.push("readonly".to_string());
    }
    if attributes.hidden {
        labels.push("hidden".to_string());
    }
    if attributes.system {
        labels.push("system".to_string());
    }
    if attributes.archive {
        labels.push("archive".to_string());
    }
    if attributes.symlink {
        labels.push("symlink".to_string());
    }
    if attributes.junction {
        labels.push("junction".to_string());
    }
    if attributes.reparse_point {
        labels.push("reparse".to_string());
    }
    if attributes.shortcut {
        labels.push("shortcut".to_string());
    }
    labels
}

fn explorer_input_prompt(mode: ExplorerInputMode) -> &'static str {
    match mode {
        ExplorerInputMode::Browse => "Explorer",
        ExplorerInputMode::Search => "Search",
        ExplorerInputMode::NewFolder => "New folder name",
        ExplorerInputMode::NewTextFile => "New text file name",
        ExplorerInputMode::Rename => "Rename to",
    }
}

fn unix_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .ok()
        .and_then(|millis| u64::try_from(millis).ok())
        .unwrap_or(0)
}

fn format_core_error(error: &CoreError) -> String {
    match error {
        CoreError::InvalidCredentials => "Invalid username or password".to_string(),
        CoreError::AccountDisabled => "Account disabled".to_string(),
        CoreError::AccountLocked { .. } => "Account locked".to_string(),
        CoreError::BootstrapAlreadyExists => "Admin already exists".to_string(),
        CoreError::BootstrapRequired => "Create the first admin account".to_string(),
        CoreError::DuplicateUsername => "Username already exists".to_string(),
        CoreError::InvalidUsername => "Invalid username".to_string(),
        CoreError::InvalidUserInfo(reason) => format!("Invalid user info: {reason}"),
        CoreError::InvalidPassword(reason) => format!("Invalid password: {reason}"),
        CoreError::LastPrivilegedUserRequired => {
            "At least one enabled admin or debug user is required".to_string()
        }
        CoreError::PermissionDenied { reason, .. } => format!("Permission denied: {reason}"),
        CoreError::UserNotFound => "User not found".to_string(),
        other => other.to_string(),
    }
}

fn login_error_message(error: &CoreError, password_hint: Option<&str>) -> String {
    if matches!(error, CoreError::InvalidCredentials)
        && let Some(hint) = password_hint.map(str::trim).filter(|hint| !hint.is_empty())
    {
        return format!("Password hint: {hint}");
    }

    format_core_error(error)
}

fn to_ui_user_management_field(field: UserManagementFormField) -> tundra_ui::UserManagementField {
    match field {
        UserManagementFormField::Username => tundra_ui::UserManagementField::Username,
        UserManagementFormField::DisplayName => tundra_ui::UserManagementField::DisplayName,
        UserManagementFormField::Password => tundra_ui::UserManagementField::Password,
    }
}

fn user_home_entries() -> Vec<tundra_ui::ShellEntry> {
    vec![
        tundra_ui::ShellEntry::new("Explorer", "Browse files"),
        tundra_ui::ShellEntry::new("Launcher", "Open apps and commands"),
        tundra_ui::ShellEntry::new("Editor", "Edit text files"),
        tundra_ui::ShellEntry::new("Settings", "Adjust TundraUX"),
        tundra_ui::ShellEntry::new("Diagnostics", "Inspect system readiness"),
    ]
}

fn terminal_flag_labels(flags: ShellTerminalFlags) -> Vec<String> {
    let mut labels = Vec::new();

    if flags.raw_mode {
        labels.push("raw mode: enabled".to_string());
    }
    if flags.alternate_screen {
        labels.push("alternate screen: enabled".to_string());
    }
    if flags.mouse_capture {
        labels.push("mouse capture: enabled".to_string());
    }
    if flags.cursor_restore_enabled {
        labels.push("cursor restore: enabled".to_string());
    }

    labels
}

fn resolved_home_mode(
    launch_config: ShellLaunchConfig,
    startup: &ShellStartupState,
) -> ShellHomeMode {
    match launch_config.home_mode_override {
        HomeModeOverride::Debug => ShellHomeMode::Debug,
        HomeModeOverride::BuildDefault => startup
            .restored_session
            .as_ref()
            .map(|session| session.display_mode)
            .or(startup.app_config.home_mode)
            .unwrap_or_else(|| ShellState::legacy_default_home_mode(launch_config)),
    }
}

fn should_show_startup_lockscreen(startup: &ShellStartupState) -> bool {
    startup.storage_manager.is_some()
        && !startup.auth_bootstrap_required
        && !startup.login_users.is_empty()
}

fn startup_lockscreen_launch_options(startup: &ShellStartupState) -> tundra_weathr::LaunchOptions {
    let Some(config) = startup
        .storage_manager
        .as_ref()
        .and_then(|storage| storage.load_config().ok())
    else {
        return tundra_weathr::LaunchOptions::default();
    };

    let mut options = tundra_weathr::LaunchOptions {
        timezone_id: Some(config.timezone.clone()),
        ..tundra_weathr::LaunchOptions::default()
    };

    if let Some(timezone) = tundra_ui::setup_timezone_options()
        .into_iter()
        .find(|timezone| timezone.id == config.timezone)
    {
        options.location_override = Some(tundra_weathr::LaunchLocation {
            latitude: timezone.latitude,
            longitude: timezone.longitude,
            city: Some(timezone.label),
        });
    }

    options
}

fn platform_capability_summary(kind: PlatformKind, capabilities: &PlatformCapabilities) -> String {
    let (mut supported, mut best_effort, mut unsupported) = (0, 0, 0);

    for (_, status) in capabilities.checks() {
        match status {
            CapabilityStatus::Supported => supported += 1,
            CapabilityStatus::BestEffort => best_effort += 1,
            CapabilityStatus::Unsupported => unsupported += 1,
        }
    }

    format!(
        "{}: {supported} supported, {best_effort} best-effort, {unsupported} unsupported",
        kind.as_str()
    )
}

fn build_mode_label() -> &'static str {
    if cfg!(debug_assertions) {
        "debug"
    } else {
        "release"
    }
}

pub fn crossterm_event_to_input(event: Event) -> InputEvent {
    match event {
        Event::Key(key_event) => InputEvent::Key(key_event_to_input(key_event)),
        Event::Mouse(mouse_event) => mouse_event_to_input(mouse_event),
        Event::Resize(width, height) => InputEvent::Resize { width, height },
        Event::FocusGained => InputEvent::FocusGained,
        Event::FocusLost => InputEvent::FocusLost,
        Event::Paste(value) => InputEvent::Paste(value),
    }
}

fn key_event_to_input(key_event: KeyEvent) -> KeyInput {
    let key = match key_event.code {
        KeyCode::Char(character) => InputKey::Character(character),
        KeyCode::Enter => InputKey::Enter,
        KeyCode::Esc => InputKey::Escape,
        KeyCode::Backspace => InputKey::Backspace,
        KeyCode::Tab => InputKey::Tab,
        KeyCode::BackTab => InputKey::BackTab,
        KeyCode::Left => InputKey::Left,
        KeyCode::Right => InputKey::Right,
        KeyCode::Up => InputKey::Up,
        KeyCode::Down => InputKey::Down,
        KeyCode::Delete => InputKey::Delete,
        KeyCode::Insert => InputKey::Insert,
        KeyCode::Home => InputKey::Home,
        KeyCode::End => InputKey::End,
        KeyCode::PageUp => InputKey::PageUp,
        KeyCode::PageDown => InputKey::PageDown,
        KeyCode::F(number) => InputKey::Function(number),
        other => InputKey::Other(format!("{other:?}")),
    };

    KeyInput::new(
        key,
        InputModifiers::from(key_event.modifiers),
        InputPhase::from(key_event.kind),
    )
}

#[cfg(test)]
fn key_event_to_label(key_event: KeyEvent) -> String {
    key_event_to_input(key_event).label()
}

fn mouse_event_to_input(mouse_event: MouseEvent) -> InputEvent {
    let coordinates = (mouse_event.column, mouse_event.row);
    let modifiers = InputModifiers::from(mouse_event.modifiers);
    let mouse = match mouse_event.kind {
        MouseEventKind::Down(button) => MouseInput::Down {
            button: PointerButton::from(button),
            coordinates,
            modifiers,
        },
        MouseEventKind::Up(button) => MouseInput::Up {
            button: PointerButton::from(button),
            coordinates,
            modifiers,
        },
        MouseEventKind::Drag(button) => MouseInput::Drag {
            button: PointerButton::from(button),
            coordinates,
            modifiers,
        },
        MouseEventKind::Moved => MouseInput::Moved {
            coordinates,
            modifiers,
        },
        MouseEventKind::ScrollDown => MouseInput::Scroll {
            direction: ScrollDirection::Down,
            coordinates,
            modifiers,
        },
        MouseEventKind::ScrollUp => MouseInput::Scroll {
            direction: ScrollDirection::Up,
            coordinates,
            modifiers,
        },
        MouseEventKind::ScrollLeft => MouseInput::Scroll {
            direction: ScrollDirection::Left,
            coordinates,
            modifiers,
        },
        MouseEventKind::ScrollRight => MouseInput::Scroll {
            direction: ScrollDirection::Right,
            coordinates,
            modifiers,
        },
    };

    InputEvent::Mouse(mouse)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShellArgError {
    UnknownArgument(String),
    DuplicateArgument(String),
}

impl std::fmt::Display for ShellArgError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownArgument(argument) => write!(formatter, "unknown argument: {argument}"),
            Self::DuplicateArgument(argument) => {
                write!(formatter, "duplicate argument: {argument}")
            }
        }
    }
}

impl std::error::Error for ShellArgError {}

pub fn parse_shell_args<I, S>(args: I) -> Result<ShellLaunchConfig, ShellArgError>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut config = ShellLaunchConfig::default();
    let mut seen_not_fullscreen = false;
    let mut seen_debug = false;

    for arg in args {
        match arg.as_ref() {
            "-notfullscreen" => {
                if seen_not_fullscreen {
                    return Err(ShellArgError::DuplicateArgument(arg.as_ref().to_string()));
                }
                seen_not_fullscreen = true;
                config.terminal_mode = ShellTerminalMode::NotFullscreen;
            }
            "-debug" => {
                if seen_debug {
                    return Err(ShellArgError::DuplicateArgument(arg.as_ref().to_string()));
                }
                seen_debug = true;
                config.home_mode_override = HomeModeOverride::Debug;
            }
            other => return Err(ShellArgError::UnknownArgument(other.to_string())),
        }
    }

    Ok(config)
}

pub fn startup_lines() -> Vec<String> {
    vec![
        "TundraUX3 shell - Phase 0 smoke".to_string(),
        "Supported OS: Windows and macOS".to_string(),
        "Target terminal: crossterm-compatible terminal".to_string(),
        format!(
            "Config format: {} (schema v{})",
            CONFIG_DESCRIPTOR.file_name, SCHEMA_VERSION
        ),
        "State data: users, state, recent-files, sessions use versioned JSON".to_string(),
    ]
}

pub fn render_static_banner(output: &mut impl Write) -> io::Result<()> {
    let ascii_assets = tundra_ui::RuntimeAsciiAssets::load_default().map_err(asset_io_error)?;
    render_static_banner_with_assets(output, &ascii_assets)
}

pub fn render_static_banner_with_assets(
    output: &mut impl Write,
    ascii_assets: &tundra_ui::RuntimeAsciiAssets,
) -> io::Result<()> {
    for line in ascii_assets
        .banner_lines(BANNER_ASSET_KEY)
        .map_err(asset_io_error)?
    {
        writeln!(output, "{line}")?;
    }

    Ok(())
}

pub fn display_banner(output: &mut impl Write) -> io::Result<()> {
    let ascii_assets = tundra_ui::RuntimeAsciiAssets::load_default().map_err(asset_io_error)?;
    display_animated_banner_with_assets(output, BANNER_DISPLAY_DURATION, &ascii_assets)
}

pub fn display_animated_banner(
    output: &mut impl Write,
    total_duration: Duration,
) -> io::Result<()> {
    let ascii_assets = tundra_ui::RuntimeAsciiAssets::load_default().map_err(asset_io_error)?;
    display_animated_banner_with_assets(output, total_duration, &ascii_assets)
}

pub fn display_animated_banner_with_assets(
    output: &mut impl Write,
    total_duration: Duration,
    ascii_assets: &tundra_ui::RuntimeAsciiAssets,
) -> io::Result<()> {
    let banner_lines = ascii_assets
        .banner_lines(BANNER_ASSET_KEY)
        .map_err(asset_io_error)?;
    if banner_lines.is_empty() {
        return Ok(());
    }

    let started_at = Instant::now();
    let frame_delay = total_duration / (banner_lines.len() as u32 + 1);

    for revealed_lines in 1..=banner_lines.len() {
        write!(output, "\x1B[2J\x1B[H")?;
        for line in banner_lines.iter().take(revealed_lines) {
            writeln!(output, "{line}")?;
        }
        output.flush()?;

        if !frame_delay.is_zero() {
            thread::sleep(frame_delay);
        }
    }

    let elapsed = started_at.elapsed();
    if elapsed < total_duration {
        thread::sleep(total_duration - elapsed);
    }

    Ok(())
}

fn asset_io_error(error: tundra_ui::AssetError) -> io::Error {
    io::Error::other(error.to_string())
}

pub fn run_without_animation(output: &mut impl Write) -> io::Result<()> {
    run_not_fullscreen_without_animation(output)
}

pub fn run_not_fullscreen_without_animation(output: &mut impl Write) -> io::Result<()> {
    let ascii_assets = tundra_ui::RuntimeAsciiAssets::load_default().map_err(asset_io_error)?;
    render_static_banner_with_assets(output, &ascii_assets)?;
    write_smoke_loop_message(output)
}

pub fn run_with_banner_animation(output: &mut impl Write) -> io::Result<()> {
    run_not_fullscreen(
        output,
        ShellLaunchConfig {
            terminal_mode: ShellTerminalMode::NotFullscreen,
            ..ShellLaunchConfig::default()
        },
    )
}

pub fn run_not_fullscreen(output: &mut impl Write, _config: ShellLaunchConfig) -> io::Result<()> {
    let ascii_assets = tundra_ui::RuntimeAsciiAssets::load_default().map_err(asset_io_error)?;
    display_animated_banner_with_assets(output, BANNER_DISPLAY_DURATION, &ascii_assets)?;
    write_smoke_loop_message(output)
}

pub fn run_fullscreen_once_without_animation(output: &mut impl Write) -> io::Result<()> {
    let ascii_assets = tundra_ui::RuntimeAsciiAssets::load_default().map_err(asset_io_error)?;
    with_fullscreen(output, |output| {
        render_static_banner_with_assets(output, &ascii_assets)?;
        write_smoke_loop_message(output)
    })
}

pub fn run_fullscreen_blocking(
    output: &mut impl Write,
    config: ShellLaunchConfig,
) -> io::Result<()> {
    let ascii_assets = tundra_ui::RuntimeAsciiAssets::load_default().map_err(asset_io_error)?;
    let platform = tundra_platform::native_platform();
    let startup = prepare_shell_startup(platform.as_ref(), config).map_err(io::Error::other)?;
    if should_show_startup_lockscreen(&startup) {
        let lockscreen_options = startup_lockscreen_launch_options(&startup);
        match tundra_weathr::run_shell_lockscreen_blocking_with_options(lockscreen_options)
            .map_err(io::Error::other)?
        {
            tundra_weathr::ShellLockscreenResult::Started => {}
            tundra_weathr::ShellLockscreenResult::Cancelled => return Ok(()),
        }
    }
    install_panic_restore_hook();
    let terminal_control = TerminalControlHandler::install();
    let mut guard = TerminalGuard::enter(output)?;
    let initial_size = crossterm::terminal::size().unwrap_or((80, 24));
    let mut state =
        ShellState::new_with_startup_and_assets(config, initial_size, startup, ascii_assets);
    let (time_sync_sender, time_sync_receiver) = mpsc::channel();
    let _time_sync_worker = spawn_time_sync_worker(time_sync_sender);
    let tick_rate = Duration::from_millis(250);
    let theme = tundra_ui::TundraTheme::default_dark();

    loop {
        drain_time_sync_results(&mut state, &time_sync_receiver);
        let chrome = state.to_shell_chrome_view_model();
        let home = state.to_home_view_model();
        let clock = state.to_clock_view_model();
        let time_sync_dialog = state.to_time_sync_dialog_view_model();
        let setup = state.to_setup_view_model();
        let login = state.to_login_view_model();
        let bootstrap_admin = state.to_bootstrap_admin_view_model();
        let user_management = state.to_user_management_view_model();
        let explorer = state.to_explorer_view_model();
        let active_screen = state.active_screen();
        let exit_confirmation = tundra_ui::ExitConfirmViewModel::new();

        guard.terminal_mut().draw(|frame| {
            let area = frame.area();
            match active_screen {
                ShellScreen::FirstRunSetup => {
                    tundra_ui::render_setup(frame, area, &chrome, &setup, &theme);
                }
                ShellScreen::Login => {
                    tundra_ui::render_login(frame, area, &chrome, &login, &theme);
                }
                ShellScreen::BootstrapAdmin => {
                    tundra_ui::render_bootstrap_admin(
                        frame,
                        area,
                        &chrome,
                        &bootstrap_admin,
                        &theme,
                    );
                }
                ShellScreen::UserManagement => {
                    tundra_ui::render_user_management(
                        frame,
                        area,
                        &chrome,
                        &user_management,
                        &theme,
                    );
                }
                ShellScreen::Explorer => {
                    tundra_ui::render_explorer(frame, area, &chrome, &explorer, &theme);
                }
                ShellScreen::Clock => {
                    tundra_ui::render_clock_placeholder(frame, area, &chrome, &clock, &theme);
                }
                ShellScreen::Home | ShellScreen::ExitConfirm => {
                    tundra_ui::render_home(frame, area, &chrome, &home, &theme);
                }
            }

            if active_screen == ShellScreen::ExitConfirm {
                tundra_ui::render_exit_confirmation(frame, area, &exit_confirmation, &theme);
            }
            if let Some(dialog) = time_sync_dialog.as_ref() {
                tundra_ui::render_time_sync_failure_dialog(frame, area, dialog, &theme);
            }
        })?;

        if terminal_control.shutdown_requested() {
            state.apply_input_with_platform(InputEvent::Shutdown, platform.as_ref());
        }
        if state.shutdown_requested() {
            break;
        }

        let action = if event::poll(tick_rate)? {
            state.apply_input_with_platform(
                crossterm_event_to_input(event::read()?),
                platform.as_ref(),
            )
        } else {
            state.apply_input_with_platform(InputEvent::Tick, platform.as_ref())
        };

        if action == ShellAction::Exit {
            break;
        }
    }

    guard.restore()
}

fn spawn_time_sync_worker(sender: mpsc::Sender<TimeSyncResult>) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let Ok(runtime) = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
        else {
            return;
        };

        runtime.block_on(async move {
            loop {
                let result = tundra_weathr::network_clock::fetch_standard_time().await;
                if sender.send(result).is_err() {
                    break;
                }
                tokio::time::sleep(TIME_SYNC_INTERVAL).await;
            }
        });
    })
}

fn drain_time_sync_results(state: &mut ShellState, receiver: &mpsc::Receiver<TimeSyncResult>) {
    loop {
        match receiver.try_recv() {
            Ok(result) => state.apply_time_sync_result(result),
            Err(mpsc::TryRecvError::Empty) => break,
            Err(mpsc::TryRecvError::Disconnected) => break,
        }
    }
}

fn with_fullscreen<W, T>(
    output: &mut W,
    body: impl FnOnce(&mut W) -> io::Result<T>,
) -> io::Result<T>
where
    W: Write,
{
    tundra_platform::with_terminal_fullscreen(output, body)
}

fn write_smoke_loop_message(output: &mut impl Write) -> io::Result<()> {
    for line in startup_lines() {
        writeln!(output, "{line}")?;
    }
    writeln!(output, "Entering smoke loop")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{
        KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
    };

    #[test]
    fn key_event_to_label_maps_requested_keys() {
        let cases = [
            (
                KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL),
                "Ctrl+C",
            ),
            (KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE), "x"),
            (KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE), "Enter"),
            (KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE), "Esc"),
            (
                KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE),
                "Backspace",
            ),
            (KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE), "Tab"),
            (
                KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT),
                "Shift+Tab",
            ),
            (KeyEvent::new(KeyCode::Left, KeyModifiers::NONE), "Left"),
            (KeyEvent::new(KeyCode::Right, KeyModifiers::NONE), "Right"),
            (KeyEvent::new(KeyCode::Up, KeyModifiers::NONE), "Up"),
            (KeyEvent::new(KeyCode::Down, KeyModifiers::NONE), "Down"),
            (KeyEvent::new(KeyCode::F(5), KeyModifiers::NONE), "F(5)"),
        ];

        for (event, expected) in cases {
            assert_eq!(key_event_to_label(event), expected);
        }
    }

    #[test]
    fn mouse_event_to_input_maps_button_motion_and_scroll_events() {
        let down = mouse_event_to_input(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 12,
            row: 7,
            modifiers: KeyModifiers::NONE,
        });
        let drag = mouse_event_to_input(MouseEvent {
            kind: MouseEventKind::Drag(MouseButton::Right),
            column: 13,
            row: 8,
            modifiers: KeyModifiers::NONE,
        });
        let moved = mouse_event_to_input(MouseEvent {
            kind: MouseEventKind::Moved,
            column: 14,
            row: 9,
            modifiers: KeyModifiers::NONE,
        });
        let scroll_up = mouse_event_to_input(MouseEvent {
            kind: MouseEventKind::ScrollUp,
            column: 15,
            row: 10,
            modifiers: KeyModifiers::NONE,
        });

        assert_eq!(
            down,
            InputEvent::Mouse(MouseInput::Down {
                button: PointerButton::Left,
                coordinates: (12, 7),
                modifiers: InputModifiers::none(),
            })
        );
        assert_eq!(
            drag,
            InputEvent::Mouse(MouseInput::Drag {
                button: PointerButton::Right,
                coordinates: (13, 8),
                modifiers: InputModifiers::none(),
            })
        );
        assert_eq!(
            moved,
            InputEvent::Mouse(MouseInput::Moved {
                coordinates: (14, 9),
                modifiers: InputModifiers::none(),
            })
        );
        assert_eq!(
            scroll_up,
            InputEvent::Mouse(MouseInput::Scroll {
                direction: ScrollDirection::Up,
                coordinates: (15, 10),
                modifiers: InputModifiers::none(),
            })
        );
    }

    #[test]
    fn mouse_event_to_input_uses_required_scroll_direction_labels() {
        let cases = [
            (MouseEventKind::ScrollDown, "Down"),
            (MouseEventKind::ScrollUp, "Up"),
            (MouseEventKind::ScrollLeft, "Left"),
            (MouseEventKind::ScrollRight, "Right"),
        ];

        for (kind, expected_direction) in cases {
            let input = mouse_event_to_input(MouseEvent {
                kind,
                column: 1,
                row: 2,
                modifiers: KeyModifiers::NONE,
            });

            assert_eq!(
                input,
                InputEvent::Mouse(MouseInput::Scroll {
                    direction: match expected_direction {
                        "Down" => ScrollDirection::Down,
                        "Up" => ScrollDirection::Up,
                        "Left" => ScrollDirection::Left,
                        "Right" => ScrollDirection::Right,
                        _ => unreachable!("test direction"),
                    },
                    coordinates: (1, 2),
                    modifiers: InputModifiers::none(),
                })
            );
        }
    }

    #[test]
    fn platform_capability_summary_counts_native_supported_capabilities() {
        let summary = platform_capability_summary(
            PlatformKind::Windows,
            &PlatformCapabilities::native_supported(),
        );

        assert_eq!(
            summary,
            "Windows: 10 supported, 0 best-effort, 3 unsupported"
        );
    }

    #[test]
    fn startup_lockscreen_launch_options_use_storage_timezone_and_location() {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time")
            .as_nanos();
        let base = std::env::temp_dir().join(format!(
            "tundra-shell-lockscreen-options-{}-{nanos}",
            std::process::id()
        ));
        let app_paths = tundra_platform::build_windows_app_paths(
            base.join("Roaming"),
            base.join("Local"),
            base.join("Temp"),
        )
        .expect("app paths");
        let opened = StorageManager::open(app_paths).expect("storage opens");
        let mut config = opened.manager.load_config().expect("config loads");
        config.timezone = "Asia/Shanghai".to_string();
        opened.manager.save_config(&config).expect("config saves");

        let mut startup = ShellStartupState::clean(
            PlatformKind::Windows,
            PlatformCapabilities::native_supported(),
        );
        startup.storage_manager = Some(opened.manager.clone());

        let options = startup_lockscreen_launch_options(&startup);

        assert_eq!(options.timezone_id.as_deref(), Some("Asia/Shanghai"));
        let location = options.location_override.expect("mapped location");
        assert_eq!(location.city.as_deref(), Some("Shanghai"));
        assert!((location.latitude - 31.2304).abs() < 0.001);
        assert!((location.longitude - 121.4737).abs() < 0.001);

        let _ = std::fs::remove_dir_all(base);
    }
}
