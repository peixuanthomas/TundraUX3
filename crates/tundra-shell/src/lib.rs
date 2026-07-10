mod clock_scheduler;

use clock_scheduler::{
    ClockEntryKind as ScheduledClockEntryKind, ClockScheduler, ClockSchedulerError, DueEvent,
};
use tundra_apps::explorer::{ExplorerCommand, ExplorerController, ExplorerState};
use tundra_core::{
    AuditOutcome, AuditService, AuthSession, CoreError, DebugPolicy, PASSWORD_MAX_LEN,
    PASSWORD_MIN_LEN, PermissionAction, PermissionService, SessionService, UserAccount, UserRole,
    UserService,
};
use tundra_storage::{
    CLOCK_DESCRIPTOR, CONFIG_DESCRIPTOR, ClockProfile, SCHEMA_VERSION, StorageError,
    StorageLoadReport, StorageManager, UserRecord,
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
use std::collections::VecDeque;
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
const DEFAULT_TOAST_DURATION: Duration = Duration::from_secs(4);
const MAX_ACTIVE_ALERTS: usize = 64;
const MAX_NOTIFICATION_RESPONSES: usize = 128;
const MAX_NOTIFICATION_FOLLOW_UP_STEPS: usize = 64;
const DEFAULT_ALERT_KEY: &str = "shell.default";
const NOTIFICATION_FOLLOW_UP_ALERT_KEY: &str = "shell.notification-follow-up";
const EXIT_CONFIRM_NOTIFICATION_KEY: &str = "shell.exit-confirm";
const TIME_SYNC_NOTIFICATION_KEY: &str = "shell.time-sync-failure";
const EXPLORER_DELETE_NOTIFICATION_KEY: &str = "explorer.delete-confirm";
const EXPLORER_ALERT_KEY: &str = "explorer.operation";
const USER_MANAGEMENT_REFRESH_ALERT_KEY: &str = "user-management.refresh";
const USER_MANAGEMENT_DELETE_NOTIFICATION_KEY: &str = "user-management.delete-confirm";
const CLOCK_STORAGE_ALERT_KEY: &str = "clock.storage";
const CLOCK_MANAGE_NOTIFICATION_KEY_PREFIX: &str = "clock.manage";
const CLOCK_DUE_NOTIFICATION_KEY_PREFIX: &str = "clock.due";

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
    pub super_key: bool,
    pub hyper: bool,
    pub meta: bool,
}

impl InputModifiers {
    pub const fn none() -> Self {
        Self {
            shift: false,
            control: false,
            alt: false,
            super_key: false,
            hyper: false,
            meta: false,
        }
    }
}

impl From<KeyModifiers> for InputModifiers {
    fn from(modifiers: KeyModifiers) -> Self {
        Self {
            shift: modifiers.contains(KeyModifiers::SHIFT),
            control: modifiers.contains(KeyModifiers::CONTROL),
            alt: modifiers.contains(KeyModifiers::ALT),
            super_key: modifiers.contains(KeyModifiers::SUPER),
            hyper: modifiers.contains(KeyModifiers::HYPER),
            meta: modifiers.contains(KeyModifiers::META),
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

        if self.modifiers.control
            && !self.modifiers.alt
            && !self.modifiers.shift
            && !self.modifiers.super_key
            && !self.modifiers.hyper
            && !self.modifiers.meta
            && let InputKey::Character(character) = &self.key
        {
            return format!("Ctrl+{}", character.to_ascii_uppercase());
        }

        let mut parts = Vec::new();
        if self.modifiers.control {
            parts.push("Ctrl");
        }
        if self.modifiers.alt {
            parts.push("Alt");
        }
        if self.modifiers.super_key {
            parts.push("Super");
        }
        if self.modifiers.hyper {
            parts.push("Hyper");
        }
        if self.modifiers.meta {
            parts.push("Meta");
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

    fn has_non_shift_modifier(&self) -> bool {
        self.modifiers.control
            || self.modifiers.alt
            || self.modifiers.super_key
            || self.modifiers.hyper
            || self.modifiers.meta
    }

    fn is_unmodified_action_key(&self) -> bool {
        !self.has_non_shift_modifier() && !self.modifiers.shift
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
    ClockNewButton,
    ClockEntryList,
    ClockCreateDialog,
    ClockCreateInput,
    ClockCreateAlarmButton,
    ClockCreateCountdownButton,
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
    NotificationDialog,
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
            Self::ClockNewButton => "ClockNewButton",
            Self::ClockEntryList => "ClockEntryList",
            Self::ClockCreateDialog => "ClockCreateDialog",
            Self::ClockCreateInput => "ClockCreateInput",
            Self::ClockCreateAlarmButton => "ClockCreateAlarmButton",
            Self::ClockCreateCountdownButton => "ClockCreateCountdownButton",
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
            Self::NotificationDialog => "NotificationDialog",
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellNotificationAction {
    pub id: String,
    pub label: String,
    pub shortcut: Option<InputKey>,
    pub cancel: bool,
    pub follow_up: Option<ShellCommand>,
}

impl ShellNotificationAction {
    pub fn new(id: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            shortcut: None,
            cancel: false,
            follow_up: None,
        }
    }

    pub fn with_shortcut(mut self, shortcut: InputKey) -> Self {
        self.shortcut = Some(shortcut);
        self
    }

    pub fn cancel(mut self) -> Self {
        self.cancel = true;
        self
    }

    pub fn with_follow_up(mut self, command: ShellCommand) -> Self {
        self.follow_up = Some(command);
        self
    }

    fn shortcut_label(&self) -> Option<String> {
        self.shortcut.as_ref().map(InputKey::label)
    }

    fn matches_shortcut(&self, input: &KeyInput) -> bool {
        if input.phase != InputPhase::Press || input.has_non_shift_modifier() {
            return false;
        }

        if input.modifiers.shift
            && !matches!(
                input.key,
                InputKey::Character(character) if character.is_ascii_alphabetic()
            )
        {
            return false;
        }

        match (&self.shortcut, &input.key) {
            (Some(InputKey::Character(expected)), InputKey::Character(actual)) => {
                expected.eq_ignore_ascii_case(actual)
            }
            (Some(expected), actual) => expected == actual,
            _ => false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellNotification {
    pub id: u64,
    pub key: Option<String>,
    pub level: tundra_ui::NotificationLevel,
    pub tone: tundra_ui::NotificationTone,
    pub component: ShellComponent,
    pub title: String,
    pub message: String,
    pub actions: Vec<ShellNotificationAction>,
    selected_action: usize,
}

impl ShellNotification {
    pub fn modal(
        title: impl Into<String>,
        message: impl Into<String>,
        tone: tundra_ui::NotificationTone,
        actions: Vec<ShellNotificationAction>,
    ) -> Self {
        Self {
            id: 0,
            key: None,
            level: tundra_ui::NotificationLevel::Modal,
            tone,
            component: ShellComponent::NotificationDialog,
            title: title.into(),
            message: message.into(),
            actions: non_empty_notification_actions(actions),
            selected_action: 0,
        }
    }

    pub fn with_key(mut self, key: impl Into<String>) -> Self {
        self.key = Some(key.into());
        self
    }

    pub fn with_component(mut self, component: ShellComponent) -> Self {
        self.component = component;
        self
    }

    fn with_selected_action(mut self, index: usize) -> Self {
        self.selected_action = index.min(self.actions.len().saturating_sub(1));
        self
    }

    fn to_view_model(&self) -> tundra_ui::NotificationViewModel {
        tundra_ui::NotificationViewModel::new(
            self.id.to_string(),
            self.level,
            self.tone,
            self.title.clone(),
            self.message.clone(),
            self.actions
                .iter()
                .enumerate()
                .map(|(index, action)| {
                    let mut view =
                        tundra_ui::NotificationActionViewModel::new(&action.id, &action.label);
                    if let Some(shortcut) = action.shortcut_label() {
                        view = view.with_shortcut(shortcut);
                    }
                    view.selected(index == self.selected_action)
                })
                .collect(),
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellNotificationResponse {
    pub notification_id: u64,
    pub action_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AlertState {
    key: String,
    message: String,
    tone: tundra_ui::NotificationTone,
    sequence: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotificationCenter {
    status: String,
    toast: Option<String>,
    toast_expires_at: Option<Instant>,
    alerts: VecDeque<AlertState>,
    active_modal: Option<ShellNotification>,
    modal_queue: VecDeque<ShellNotification>,
    responses: VecDeque<ShellNotificationResponse>,
    next_id: u64,
    next_alert_sequence: u64,
}

impl NotificationCenter {
    pub fn new(status: impl Into<String>) -> Self {
        Self {
            status: status.into(),
            toast: None,
            toast_expires_at: None,
            alerts: VecDeque::new(),
            active_modal: None,
            modal_queue: VecDeque::new(),
            responses: VecDeque::new(),
            next_id: 1,
            next_alert_sequence: 1,
        }
    }

    pub fn notify_status(&mut self, message: impl Into<String>) {
        self.status = message.into();
    }

    pub fn notify_toast(&mut self, message: impl Into<String>) {
        self.notify_toast_at(message, Instant::now());
    }

    fn notify_toast_at(&mut self, message: impl Into<String>, now: Instant) {
        self.toast = Some(message.into());
        self.toast_expires_at = now.checked_add(DEFAULT_TOAST_DURATION).or(Some(now));
    }

    pub fn notify_alert(&mut self, message: impl Into<String>, tone: tundra_ui::NotificationTone) {
        self.notify_alert_with_key(DEFAULT_ALERT_KEY, message, tone);
    }

    pub fn notify_alert_with_key(
        &mut self,
        key: impl Into<String>,
        message: impl Into<String>,
        tone: tundra_ui::NotificationTone,
    ) {
        let key = key.into();
        let sequence = self.next_alert_sequence;
        self.next_alert_sequence = self.next_alert_sequence.saturating_add(1).max(1);
        if let Some(alert) = self.alerts.iter_mut().find(|alert| alert.key == key) {
            alert.message = message.into();
            alert.tone = tone;
            alert.sequence = sequence;
            return;
        }

        if self.alerts.len() >= MAX_ACTIVE_ALERTS
            && let Some(oldest_index) = self
                .alerts
                .iter()
                .enumerate()
                .min_by_key(|(_, alert)| alert.sequence)
                .map(|(index, _)| index)
        {
            self.alerts.remove(oldest_index);
        }
        self.alerts.push_back(AlertState {
            key,
            message: message.into(),
            tone,
            sequence,
        });
    }

    pub fn resolve_alert(&mut self, key: &str) {
        let had_alerts = !self.alerts.is_empty();
        self.alerts.retain(|alert| alert.key != key);
        if had_alerts && self.alerts.is_empty() && self.toast.is_some() {
            let now = Instant::now();
            self.toast_expires_at = now.checked_add(DEFAULT_TOAST_DURATION).or(Some(now));
        }
    }

    pub fn clear_alert(&mut self) {
        let had_alerts = !self.alerts.is_empty();
        self.alerts.clear();
        if had_alerts && self.toast.is_some() {
            let now = Instant::now();
            self.toast_expires_at = now.checked_add(DEFAULT_TOAST_DURATION).or(Some(now));
        }
    }

    pub fn clear_toast(&mut self) {
        self.toast = None;
        self.toast_expires_at = None;
    }

    pub fn tick(&mut self) {
        self.expire(Instant::now());
    }

    pub fn expire(&mut self, now: Instant) {
        if self.alerts.is_empty()
            && self
                .toast_expires_at
                .is_some_and(|expires_at| now >= expires_at)
        {
            self.toast = None;
            self.toast_expires_at = None;
        }
    }

    pub fn poll_timeout(&self, now: Instant, maximum: Duration) -> Duration {
        if !self.alerts.is_empty() {
            return maximum;
        }
        match self.toast_expires_at {
            Some(expires_at) => expires_at
                .checked_duration_since(now)
                .unwrap_or(Duration::ZERO)
                .min(maximum),
            None => maximum,
        }
    }

    pub fn push_modal(&mut self, mut notification: ShellNotification) -> u64 {
        if let Some(key) = notification.key.clone()
            && let Some(existing) = self.modal_with_key_mut(&key)
        {
            notification.id = existing.id;
            *existing = notification;
            return existing.id;
        }

        notification.id = self.next_id;
        self.next_id = self.next_id.saturating_add(1).max(1);
        let id = notification.id;
        if self.active_modal.is_none() {
            self.active_modal = Some(notification);
        } else {
            self.modal_queue.push_back(notification);
        }
        id
    }

    pub fn dismiss_modal_by_key(&mut self, key: &str) {
        let active_matches = self
            .active_modal
            .as_ref()
            .and_then(|modal| modal.key.as_deref())
            == Some(key);
        if active_matches {
            self.active_modal = None;
            self.promote_next_modal();
        }
        self.modal_queue
            .retain(|modal| modal.key.as_deref() != Some(key));
    }

    fn dismiss_modals_by_key_prefix(&mut self, prefix: &str) {
        self.modal_queue.retain(|modal| {
            !modal
                .key
                .as_deref()
                .is_some_and(|key| key.starts_with(prefix))
        });
        while self
            .active_modal
            .as_ref()
            .and_then(|modal| modal.key.as_deref())
            .is_some_and(|key| key.starts_with(prefix))
        {
            self.active_modal = None;
            self.promote_next_modal();
        }
    }

    pub fn has_active_modal(&self) -> bool {
        self.active_modal.is_some()
    }

    pub fn active_modal_component(&self) -> Option<ShellComponent> {
        self.active_modal.as_ref().map(|modal| modal.component)
    }

    pub fn active_modal_view_model(&self) -> Option<tundra_ui::NotificationViewModel> {
        self.active_modal
            .as_ref()
            .map(ShellNotification::to_view_model)
    }

    pub fn active_modal_action_count(&self) -> usize {
        self.active_modal
            .as_ref()
            .map(|modal| modal.actions.len())
            .unwrap_or(0)
    }

    pub fn select_next_action(&mut self) {
        let Some(modal) = self.active_modal.as_mut() else {
            return;
        };
        if modal.actions.is_empty() {
            return;
        }
        modal.selected_action = (modal.selected_action + 1) % modal.actions.len();
    }

    pub fn select_previous_action(&mut self) {
        let Some(modal) = self.active_modal.as_mut() else {
            return;
        };
        if modal.actions.is_empty() {
            return;
        }
        modal.selected_action = if modal.selected_action == 0 {
            modal.actions.len().saturating_sub(1)
        } else {
            modal.selected_action.saturating_sub(1)
        };
    }

    pub fn action_index_for_key(&self, key: &InputKey) -> Option<usize> {
        let input = KeyInput::new(key.clone(), InputModifiers::none(), InputPhase::Press);
        self.action_index_for_input(&input)
    }

    pub fn action_index_for_input(&self, input: &KeyInput) -> Option<usize> {
        self.active_modal.as_ref().and_then(|modal| {
            modal
                .actions
                .iter()
                .position(|action| action.matches_shortcut(input))
        })
    }

    pub fn select_action(&mut self, index: usize) {
        let Some(modal) = self.active_modal.as_mut() else {
            return;
        };
        if index < modal.actions.len() {
            modal.selected_action = index;
        }
    }

    pub fn active_modal_id(&self) -> Option<u64> {
        self.active_modal.as_ref().map(|modal| modal.id)
    }

    pub fn cancel_action_index(&self) -> Option<usize> {
        self.active_modal.as_ref().and_then(|modal| {
            modal
                .actions
                .iter()
                .position(|action| action.cancel)
                .or_else(|| modal.actions.len().checked_sub(1))
        })
    }

    pub fn explicit_cancel_action_index(&self) -> Option<usize> {
        self.active_modal
            .as_ref()
            .and_then(|modal| modal.actions.iter().position(|action| action.cancel))
    }

    pub fn dismiss_active_modal_without_response(&mut self) -> bool {
        if self.active_modal.take().is_none() {
            return false;
        }
        self.promote_next_modal();
        true
    }

    pub fn activate_selected_action(&mut self) -> Option<ShellCommand> {
        let index = self
            .active_modal
            .as_ref()
            .map(|modal| modal.selected_action)?;
        self.activate_action(index)
    }

    pub fn activate_action(&mut self, index: usize) -> Option<ShellCommand> {
        let modal = self.active_modal.take()?;
        if index >= modal.actions.len() {
            self.active_modal = Some(modal);
            return None;
        }

        let action = modal.actions[index].clone();
        if self.responses.len() >= MAX_NOTIFICATION_RESPONSES {
            self.responses.pop_front();
        }
        self.responses.push_back(ShellNotificationResponse {
            notification_id: modal.id,
            action_id: action.id,
        });
        self.promote_next_modal();
        action.follow_up
    }

    pub fn take_response(&mut self) -> Option<ShellNotificationResponse> {
        self.responses.pop_front()
    }

    pub fn status(&self) -> &str {
        &self.status
    }

    pub fn toast(&self) -> Option<String> {
        self.toast.clone()
    }

    pub fn alert(&self) -> Option<String> {
        self.active_alert().map(|alert| alert.message.clone())
    }

    pub fn alert_tone(&self) -> Option<tundra_ui::NotificationTone> {
        self.active_alert().map(|alert| alert.tone)
    }

    fn alert_message_for_key(&self, key: &str) -> Option<&str> {
        self.alerts
            .iter()
            .find(|alert| alert.key == key)
            .map(|alert| alert.message.as_str())
    }

    fn modal_with_key_mut(&mut self, key: &str) -> Option<&mut ShellNotification> {
        if self
            .active_modal
            .as_ref()
            .and_then(|modal| modal.key.as_deref())
            == Some(key)
        {
            return self.active_modal.as_mut();
        }

        self.modal_queue
            .iter_mut()
            .find(|modal| modal.key.as_deref() == Some(key))
    }

    fn promote_next_modal(&mut self) {
        if self.active_modal.is_none() {
            self.active_modal = self.modal_queue.pop_front();
        }
    }

    fn active_alert(&self) -> Option<&AlertState> {
        self.alerts
            .iter()
            .max_by_key(|alert| (notification_tone_priority(alert.tone), alert.sequence))
    }
}

const fn notification_tone_priority(tone: tundra_ui::NotificationTone) -> u8 {
    match tone {
        tundra_ui::NotificationTone::Info => 0,
        tundra_ui::NotificationTone::Success => 1,
        tundra_ui::NotificationTone::Warning => 2,
        tundra_ui::NotificationTone::Error => 3,
        tundra_ui::NotificationTone::Critical => 4,
    }
}

fn non_empty_notification_actions(
    actions: Vec<ShellNotificationAction>,
) -> Vec<ShellNotificationAction> {
    if actions.is_empty() {
        vec![ShellNotificationAction::new("ok", "OK").cancel()]
    } else {
        actions
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ModalFocusContext {
    screen: ShellScreen,
    component: ShellComponent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct NotificationPointerCapture {
    notification_id: u64,
    action_index: usize,
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
    Role,
    Password,
    Submit,
    Cancel,
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
    focused_field: UserManagementFormField,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct UserManagementPasswordForm {
    username: String,
    password: String,
    focused_field: UserManagementFormField,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum UserManagementMode {
    Browse,
    Create(UserManagementCreateForm),
    EditInfo(UserManagementInfoForm),
    Password(UserManagementPasswordForm),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UserManagementPageFocus {
    UserList,
    Action(tundra_ui::UserManagementAction),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UserManagementFeedbackTone {
    Info,
    Success,
    Error,
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
struct ClockCreateState {
    input: String,
    error: Option<String>,
    focus: tundra_ui::ClockCreateDialogFocus,
}

impl Default for ClockCreateState {
    fn default() -> Self {
        Self {
            input: String::new(),
            error: None,
            focus: tundra_ui::ClockCreateDialogFocus::Input,
        }
    }
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
    ClockOpenCreate,
    ClockCloseCreate,
    ClockCreateFocusNext,
    ClockCreateFocusPrevious,
    ClockCreateSetFocus(tundra_ui::ClockCreateDialogFocus),
    ClockCreateAppend(char),
    ClockCreateBackspace,
    ClockCreateAlarm,
    ClockCreateCountdown,
    ClockSelectPrevious,
    ClockSelectNext,
    ClockSelectPageUp,
    ClockSelectPageDown,
    ClockSelectFirst,
    ClockSelectLast,
    ClockSelectEntry(u64),
    ClockActivateSelected,
    ClockManageEntry(u64),
    ClockDeleteEntry(u64),
    ClockToggleStrong(u64),
    ClockSnoozeFiveMinutes(u64),
    UserManagementNext,
    UserManagementPrevious,
    UserManagementPageUp,
    UserManagementPageDown,
    UserManagementFirst,
    UserManagementLast,
    UserManagementSelectRow(usize),
    UserManagementFocusAction(tundra_ui::UserManagementAction),
    UserManagementActivateFocused,
    UserManagementActivateAction(tundra_ui::UserManagementAction),
    UserManagementSetFormFocus(tundra_ui::UserManagementField),
    UserManagementActivateFormControl(tundra_ui::UserManagementField),
    UserManagementToggleFormRole,
    CreateManagedUser,
    EditManagedUserInfo,
    DisableManagedUser,
    UnlockManagedUser,
    ResetManagedPassword,
    CycleManagedRole,
    RequestDeleteManagedUser,
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
    NotificationNextAction,
    NotificationPreviousAction,
    NotificationActivateSelected,
    NotificationActivateAction(usize),
    NotificationCancel,
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

    fn snapshot(&self) -> tundra_weathr::network_clock::ClockSnapshot {
        self.0.snapshot()
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
    clock_scheduler: Option<ClockScheduler>,
    clock_selected_entry_id: Option<u64>,
    clock_entry_window_start: usize,
    clock_create_state: Option<ClockCreateState>,
    clock_persist_pending: bool,
    clock_pending_due_summary: Option<String>,
    clock_profile_pending_sync: Option<ClockProfile>,
    time_sync_attempted: bool,
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
    user_management_window_start: usize,
    user_management_focus: UserManagementPageFocus,
    user_management_message: Option<String>,
    user_management_feedback_tone: UserManagementFeedbackTone,
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
    notifications: NotificationCenter,
    modal_focus_context: Option<ModalFocusContext>,
    modal_focus_prepared_for_follow_up: bool,
    notification_pointer_capture: Option<NotificationPointerCapture>,
    pending_notification_commands: VecDeque<ShellCommand>,
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
        let mut notifications = NotificationCenter::new("Ready");
        if startup.storage_report.has_recovery_warnings() {
            notifications.notify_toast("Storage recovered defaults");
        }

        let mut state = Self {
            home_mode,
            ascii_assets,
            screen_stack: vec![initial_screen],
            storage_manager: startup.storage_manager.clone(),
            network_clock,
            clock_timezone_id,
            last_time_sync_utc: None,
            clock_scheduler: None,
            clock_selected_entry_id: None,
            clock_entry_window_start: 0,
            clock_create_state: None,
            clock_persist_pending: false,
            clock_pending_due_summary: None,
            clock_profile_pending_sync: None,
            time_sync_attempted: false,
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
            user_management_window_start: 0,
            user_management_focus: UserManagementPageFocus::UserList,
            user_management_message: None,
            user_management_feedback_tone: UserManagementFeedbackTone::Info,
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
            notifications,
            modal_focus_context: None,
            modal_focus_prepared_for_follow_up: false,
            notification_pointer_capture: None,
            pending_notification_commands: VecDeque::new(),
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
        let snapshot = self.network_clock.snapshot();
        self.to_clock_view_model_at(&snapshot, Instant::now())
    }

    fn to_clock_view_model_at(
        &self,
        snapshot: &tundra_weathr::network_clock::ClockSnapshot,
        now: Instant,
    ) -> tundra_ui::ClockViewModel {
        let mut alarms = Vec::new();
        let mut countdowns = Vec::new();
        if let Some(scheduler) = &self.clock_scheduler {
            for entry in scheduler.entries(now) {
                let label = match entry.kind {
                    ScheduledClockEntryKind::DailyAlarm => {
                        if entry.snoozed {
                            format!("{} Daily (snoozed)", entry.display_time)
                        } else {
                            format!("{} Daily", entry.display_time)
                        }
                    }
                    ScheduledClockEntryKind::Countdown => {
                        format!("{} left", entry.display_time)
                    }
                };
                let view = tundra_ui::ClockEntryViewModel::new(entry.id, label, entry.strong);
                match entry.kind {
                    ScheduledClockEntryKind::DailyAlarm => alarms.push(view),
                    ScheduledClockEntryKind::Countdown => countdowns.push(view),
                }
            }
        }

        let mut model = tundra_ui::ClockViewModel::at(
            snapshot.date.to_string(),
            snapshot.time.format("%H:%M:%S").to_string(),
            snapshot.time.hour() as u8,
            snapshot.time.minute() as u8,
            snapshot.time.second() as u8,
        )
        .with_ascii_assets(self.ascii_assets.clone());
        model.alarms = alarms;
        model.countdowns = countdowns;
        model.selected_entry_id = (self.focused_component == ShellComponent::ClockEntryList)
            .then_some(self.clock_selected_entry_id)
            .flatten();
        model.entry_window_start = self.clock_entry_window_start;
        model.create_dialog =
            self.clock_create_state
                .as_ref()
                .map(|state| tundra_ui::ClockCreateDialogViewModel {
                    input: state.input.clone(),
                    error: state.error.clone(),
                    focus: state.focus,
                });
        model
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
            step: self.setup_step,
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
            focused_field: self.setup_focused_field,
            can_submit,
            error: self.error_message.clone(),
        }
    }

    pub fn to_user_management_view_model(&self) -> tundra_ui::UserManagementViewModel {
        let current_user = self
            .auth_session
            .as_ref()
            .map(|session| session.username.clone())
            .unwrap_or_else(|| "Guest".to_string());
        let mut model = tundra_ui::UserManagementViewModel::new(
            current_user.clone(),
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
                    is_current: user.username.eq_ignore_ascii_case(&current_user),
                })
                .collect(),
            self.user_management_selected,
            self.user_management_message.clone(),
            self.can_manage_all_users(),
            self.user_management_form_view_model(),
        );
        model.user_window_start = self.user_management_window_start;
        model.focus = match self.user_management_focus {
            UserManagementPageFocus::UserList => tundra_ui::UserManagementFocus::UserList,
            UserManagementPageFocus::Action(action) => {
                tundra_ui::UserManagementFocus::Action(action)
            }
        };
        model.actions = self.user_management_action_view_models();
        model.feedback_tone = match self.user_management_feedback_tone {
            UserManagementFeedbackTone::Info => tundra_ui::UserManagementFeedbackTone::Info,
            UserManagementFeedbackTone::Success => tundra_ui::UserManagementFeedbackTone::Success,
            UserManagementFeedbackTone::Error => tundra_ui::UserManagementFeedbackTone::Error,
        };
        model
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
            Some(UserRole::Admin)
        )
    }

    fn user_management_action_view_models(&self) -> Vec<tundra_ui::UserManagementActionViewModel> {
        use tundra_ui::UserManagementAction;

        let selected = self
            .user_management_users
            .get(self.user_management_selected);
        let last_enabled_admin = self.selected_is_last_enabled_admin();
        let no_selection_reason = selected.is_none().then(|| "No user selected".to_string());
        let protected_reason =
            last_enabled_admin.then(|| "At least one enabled admin is required".to_string());
        let mut actions = Vec::new();

        if self.can_manage_all_users() {
            actions.push(user_management_action_model(
                UserManagementAction::NewUser,
                "New user",
                Some('N'),
                true,
                None,
                false,
            ));
        }

        actions.push(user_management_action_model(
            UserManagementAction::EditInfo,
            if self.can_manage_all_users() {
                "Edit"
            } else {
                "Edit profile"
            },
            Some('E'),
            selected.is_some(),
            no_selection_reason.clone(),
            false,
        ));
        actions.push(user_management_action_model(
            UserManagementAction::SetPassword,
            if self.can_manage_all_users() {
                "Password"
            } else {
                "Change password"
            },
            Some('R'),
            selected.is_some(),
            no_selection_reason.clone(),
            false,
        ));

        if self.can_manage_all_users() {
            let locked = selected.is_some_and(user_is_locked);
            let enabled = selected.is_some_and(|user| user.enabled);
            let (toggle_label, toggle_shortcut, disabling) = if !enabled {
                ("Enable", Some('U'), false)
            } else if locked {
                ("Unlock", Some('U'), false)
            } else {
                ("Disable", Some('D'), true)
            };
            actions.push(user_management_action_model(
                UserManagementAction::ToggleEnabled,
                toggle_label,
                toggle_shortcut,
                selected.is_some() && !(disabling && last_enabled_admin),
                no_selection_reason.clone().or_else(|| {
                    (disabling && last_enabled_admin)
                        .then(|| protected_reason.clone())
                        .flatten()
                }),
                disabling,
            ));

            let demoting = selected.is_some_and(|user| user.role == UserRole::Admin);
            actions.push(user_management_action_model(
                UserManagementAction::ToggleRole,
                if demoting { "Make user" } else { "Make admin" },
                Some('C'),
                selected.is_some() && !(demoting && last_enabled_admin),
                no_selection_reason.clone().or_else(|| {
                    (demoting && last_enabled_admin)
                        .then(|| protected_reason.clone())
                        .flatten()
                }),
                demoting,
            ));
        }

        actions.push(user_management_action_model(
            UserManagementAction::Delete,
            if self.can_manage_all_users() {
                "Delete"
            } else {
                "Delete account"
            },
            Some('X'),
            selected.is_some() && !last_enabled_admin,
            no_selection_reason.or(protected_reason),
            true,
        ));
        actions.push(user_management_action_model(
            UserManagementAction::Back,
            "Back",
            None,
            true,
            None,
            false,
        ));
        actions
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
                error: self.user_management_form_error(),
            }),
            UserManagementMode::EditInfo(form) => Some(tundra_ui::UserManagementFormViewModel {
                kind: tundra_ui::UserManagementFormKind::EditInfo,
                title: "Edit user info".to_string(),
                username: form.username.clone(),
                display_name: form.display_name.clone(),
                role: String::new(),
                password_len: 0,
                focused_field: to_ui_user_management_field(form.focused_field),
                error: self.user_management_form_error(),
            }),
            UserManagementMode::Password(form) => Some(tundra_ui::UserManagementFormViewModel {
                kind: tundra_ui::UserManagementFormKind::Password,
                title: "Set password".to_string(),
                username: form.username.clone(),
                display_name: String::new(),
                role: String::new(),
                password_len: form.password.chars().count(),
                focused_field: to_ui_user_management_field(form.focused_field),
                error: self.user_management_form_error(),
            }),
        }
    }

    fn user_management_form_error(&self) -> Option<String> {
        (self.user_management_feedback_tone == UserManagementFeedbackTone::Error)
            .then(|| self.user_management_message.clone())
            .flatten()
    }

    pub fn to_shell_chrome_view_model(&self) -> tundra_ui::ShellChromeViewModel {
        let status = if self.home_mode == ShellHomeMode::Debug
            && self.active_screen() == ShellScreen::Home
        {
            format!(
                "{} | Key: {} | Mouse: {} | Resize: {}",
                self.notifications.status(),
                self.last_key_event.as_deref().unwrap_or("none"),
                self.last_mouse_event.as_deref().unwrap_or("none"),
                self.last_resize_event.as_deref().unwrap_or("none")
            )
        } else {
            self.notifications.status().to_string()
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
                toast: self.notifications.toast(),
                error: self.notifications.alert(),
                alert_tone: self
                    .notifications
                    .alert_tone()
                    .unwrap_or(tundra_ui::NotificationTone::Info),
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
        let (field, component) = order[next];
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
            self.notify_status("Login failed");
            return;
        };
        let password_hint = self.selected_login_password_hint().map(str::to_string);
        let mut sessions = SessionService::new(storage);
        match sessions.login(&username, &self.login_password) {
            Ok(session) => self.complete_login(session),
            Err(error) => {
                self.error_message = Some(login_error_message(&error, password_hint.as_deref()));
                self.notify_status("Login failed");
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
                        self.notify_status("Login failed");
                    }
                }
            }
            Err(error) => {
                self.error_message = Some(format_core_error(&error));
                self.notify_status("Admin bootstrap failed");
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
            self.notify_status("Setup incomplete");
            return;
        }

        let hint = self.setup_admin_password_hint.trim().to_string();
        let hint = (!hint.is_empty()).then_some(hint);

        let mut config = match storage.load_config() {
            Ok(config) => config,
            Err(error) => {
                self.error_message = Some(error.to_string());
                self.notify_status("Setup failed");
                return;
            }
        };
        config.language = self.selected_setup_language_value();
        config.timezone = self.selected_setup_timezone_value();
        let selected_timezone = config.timezone.clone();
        if let Err(error) = storage.save_config(&config) {
            self.error_message = Some(error.to_string());
            self.notify_status("Setup failed");
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
                        self.notify_status("Login failed");
                    }
                }
            }
            Err(error) => {
                self.error_message = Some(format_core_error(&error));
                self.notify_status("Setup failed");
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
        self.notify_status(format!("Signed in as {}", session.username));
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
                    self.notify_toast("Debug mode denied");
                }
            }
        }

        self.screen_stack = vec![ShellScreen::Home];
        self.focused_component = ShellComponent::Home;
        self.active_popup = None;
        self.load_clock_for_session(&session);
        self.refresh_hit_map();
    }

    fn open_user_management(&mut self) {
        if self.auth_session.is_none() {
            self.error_message = Some("Login required".to_string());
            return;
        };

        if self.refresh_user_management() {
            self.screen_stack.push(ShellScreen::UserManagement);
            self.focused_component = ShellComponent::UserManagement;
            self.user_management_mode = UserManagementMode::Browse;
            self.user_management_focus = UserManagementPageFocus::UserList;
            self.ensure_user_management_selection_visible();
            let status = if self.can_manage_all_users() {
                "User Management"
            } else {
                "User Profile"
            };
            self.notify_status(status);
            self.refresh_hit_map();
        }
    }

    fn open_clock(&mut self) {
        if self.active_screen() != ShellScreen::Clock {
            self.screen_stack.push(ShellScreen::Clock);
        }
        self.active_popup = None;
        self.clock_create_state = None;
        self.focused_component = ShellComponent::ClockNewButton;
        self.sync_clock_selection();
        self.notify_status("Clock");
        self.refresh_hit_map();
    }

    fn close_clock(&mut self) {
        if self.active_screen() == ShellScreen::Clock {
            self.screen_stack.pop();
        }
        if self.screen_stack.is_empty() {
            self.screen_stack.push(ShellScreen::Home);
        }
        self.clock_create_state = None;
        self.notify_status("Ready");
        self.refresh_hit_map();
    }

    fn load_clock_for_session(&mut self, session: &AuthSession) {
        self.clock_scheduler = None;
        self.clock_selected_entry_id = None;
        self.clock_entry_window_start = 0;
        self.clock_create_state = None;
        self.clock_persist_pending = false;
        self.clock_pending_due_summary = None;
        self.clock_profile_pending_sync = None;

        let Some(storage) = self.storage_manager.clone() else {
            return;
        };
        let document = match storage.load_clock() {
            Ok(document) => document,
            Err(error) => {
                self.report_clock_storage_error(error.to_string());
                return;
            }
        };
        let profile = document
            .profiles
            .get(&session.user_id)
            .cloned()
            .unwrap_or_default();
        if !self.time_sync_attempted && !profile.entries.is_empty() {
            self.clock_profile_pending_sync = Some(profile);
            self.notify_toast("Waiting for initial time sync to restore reminders");
            return;
        }
        self.restore_clock_profile(profile);
    }

    fn restore_clock_profile(&mut self, profile: ClockProfile) {
        let snapshot = self.network_clock.snapshot();
        let now = Instant::now();
        let (scheduler, due) = ClockScheduler::restore(profile, &snapshot, now);
        self.clock_scheduler = Some(scheduler);
        self.sync_clock_selection_at(now);
        let ordinary_due = self.handle_clock_due_events(due);
        if let Some(summary) = ordinary_due {
            self.remember_clock_due_summary(summary);
        }

        if let Err(error) = self.persist_clock_scheduler_at(&snapshot, now) {
            self.clock_persist_pending = true;
            self.report_clock_storage_error(error);
        } else {
            self.clock_pending_due_summary = None;
            self.notifications.resolve_alert(CLOCK_STORAGE_ALERT_KEY);
        }
    }

    fn restore_clock_profile_after_initial_sync(&mut self) {
        if self.auth_session.is_none() {
            self.clock_profile_pending_sync = None;
            return;
        }
        if let Some(profile) = self.clock_profile_pending_sync.take() {
            self.restore_clock_profile(profile);
            self.refresh_hit_map();
        }
    }

    fn persist_clock_scheduler_at(
        &self,
        snapshot: &tundra_weathr::network_clock::ClockSnapshot,
        now: Instant,
    ) -> Result<(), String> {
        let storage = self
            .storage_manager
            .as_ref()
            .ok_or_else(|| "Clock storage is unavailable".to_string())?;
        let user_id = self
            .auth_session
            .as_ref()
            .map(|session| session.user_id.as_str())
            .ok_or_else(|| "Sign in to save alarms and countdowns".to_string())?;
        let scheduler = self
            .clock_scheduler
            .as_ref()
            .ok_or_else(|| "Clock scheduler is unavailable".to_string())?;
        let mut document = storage.load_clock().map_err(|error| error.to_string())?;
        document
            .profiles
            .insert(user_id.to_string(), scheduler.export_profile(snapshot, now));
        storage
            .save_clock(&document)
            .map_err(|error| error.to_string())
    }

    fn report_clock_storage_error(&mut self, message: impl Into<String>) {
        let ordinary_due = self.clock_pending_due_summary.clone();
        self.report_clock_storage_error_with_due(message, ordinary_due.as_deref());
    }

    fn remember_clock_due_summary(&mut self, summary: String) {
        self.clock_pending_due_summary = Some(match self.clock_pending_due_summary.take() {
            None => summary,
            Some(previous) if previous == summary => previous,
            Some(_) => "Multiple reminders are due".to_string(),
        });
    }

    fn report_clock_storage_error_with_due(
        &mut self,
        message: impl Into<String>,
        ordinary_due: Option<&str>,
    ) {
        let storage_error = format!("Clock data could not be saved: {}", message.into());
        let message = ordinary_due
            .map(|due| format!("{due}. {storage_error}"))
            .unwrap_or(storage_error);
        self.notifications.notify_alert_with_key(
            CLOCK_STORAGE_ALERT_KEY,
            message,
            tundra_ui::NotificationTone::Error,
        );
    }

    fn commit_clock_mutation(
        &mut self,
        previous: ClockScheduler,
        snapshot: &tundra_weathr::network_clock::ClockSnapshot,
        now: Instant,
    ) -> Result<(), String> {
        match self.persist_clock_scheduler_at(snapshot, now) {
            Ok(()) => {
                self.clock_persist_pending = false;
                self.clock_pending_due_summary = None;
                self.notifications.resolve_alert(CLOCK_STORAGE_ALERT_KEY);
                Ok(())
            }
            Err(error) => {
                self.clock_scheduler = Some(previous);
                self.report_clock_storage_error(error.clone());
                Err(error)
            }
        }
    }

    fn advance_clock_background(&mut self) {
        let snapshot = self.network_clock.snapshot();
        self.advance_clock_background_at(&snapshot, Instant::now());
    }

    fn advance_clock_background_at(
        &mut self,
        snapshot: &tundra_weathr::network_clock::ClockSnapshot,
        now: Instant,
    ) {
        self.notifications.expire(now);
        let due = self
            .clock_scheduler
            .as_mut()
            .map(|scheduler| scheduler.advance(snapshot, now))
            .unwrap_or_default();
        let has_due = !due.is_empty();
        let ordinary_due = if has_due {
            self.sync_clock_selection_at(now);
            let ordinary_due = self.handle_clock_due_events(due);
            self.refresh_hit_map();
            ordinary_due
        } else {
            None
        };
        if let Some(summary) = ordinary_due {
            self.remember_clock_due_summary(summary);
        }
        if has_due || self.clock_persist_pending {
            match self.persist_clock_scheduler_at(snapshot, now) {
                Ok(()) => {
                    self.clock_persist_pending = false;
                    self.clock_pending_due_summary = None;
                    self.notifications.resolve_alert(CLOCK_STORAGE_ALERT_KEY);
                }
                Err(error) => {
                    self.clock_persist_pending = true;
                    self.report_clock_storage_error(error);
                }
            }
        }
    }

    fn handle_clock_due_events(&mut self, due: Vec<DueEvent>) -> Option<String> {
        let mut ordinary = Vec::new();
        for event in due {
            let message = match event.kind {
                ScheduledClockEntryKind::DailyAlarm => {
                    format!("Alarm {} is due", event.display_time)
                }
                ScheduledClockEntryKind::Countdown => "Countdown finished".to_string(),
            };
            if !event.strong {
                ordinary.push(message);
                continue;
            }

            let user_id = self
                .auth_session
                .as_ref()
                .map(|session| session.user_id.as_str())
                .unwrap_or("unknown");
            let key = format!("{CLOCK_DUE_NOTIFICATION_KEY_PREFIX}.{user_id}.{}", event.id);
            let (title, actions) = match event.kind {
                ScheduledClockEntryKind::DailyAlarm => (
                    "Alarm",
                    vec![
                        ShellNotificationAction::new("snooze", "Snooze 5 min")
                            .with_shortcut(InputKey::Character('s'))
                            .with_follow_up(ShellCommand::ClockSnoozeFiveMinutes(event.id)),
                        ShellNotificationAction::new("dismiss", "Dismiss")
                            .with_shortcut(InputKey::Escape)
                            .cancel(),
                    ],
                ),
                ScheduledClockEntryKind::Countdown => (
                    "Countdown",
                    vec![
                        ShellNotificationAction::new("dismiss", "Dismiss")
                            .with_shortcut(InputKey::Escape)
                            .cancel(),
                    ],
                ),
            };
            self.notify_modal_with_options(
                ShellNotification::modal(
                    title,
                    message,
                    tundra_ui::NotificationTone::Critical,
                    actions,
                )
                .with_key(key)
                .with_component(ShellComponent::NotificationDialog),
            );
        }

        let message = match ordinary.len() {
            0 => None,
            1 => ordinary.pop(),
            count => Some(format!("{count} reminders are due")),
        };
        if let Some(message) = &message {
            self.notify_toast(message.clone());
        }
        message
    }

    fn open_clock_create_dialog(&mut self) {
        if self.clock_scheduler.is_none() {
            if self.clock_profile_pending_sync.is_some() {
                self.notify_toast("Waiting for initial time sync to restore reminders");
            } else {
                self.notify_toast("Sign in to create alarms and countdowns");
            }
            return;
        }
        self.clock_create_state = Some(ClockCreateState::default());
        self.focused_component = ShellComponent::ClockCreateInput;
        self.refresh_hit_map();
    }

    fn close_clock_create_dialog(&mut self) {
        self.clock_create_state = None;
        self.focused_component = ShellComponent::ClockNewButton;
        self.refresh_hit_map();
    }

    fn move_clock_create_focus(&mut self, direction: i8) {
        let Some(state) = self.clock_create_state.as_mut() else {
            return;
        };
        let order = [
            tundra_ui::ClockCreateDialogFocus::Input,
            tundra_ui::ClockCreateDialogFocus::CreateAlarm,
            tundra_ui::ClockCreateDialogFocus::CreateCountdown,
        ];
        let current = order
            .iter()
            .position(|focus| *focus == state.focus)
            .unwrap_or(0);
        let next =
            (current as isize + direction as isize).rem_euclid(order.len() as isize) as usize;
        self.set_clock_create_focus(order[next]);
    }

    fn set_clock_create_focus(&mut self, focus: tundra_ui::ClockCreateDialogFocus) {
        let Some(state) = self.clock_create_state.as_mut() else {
            return;
        };
        state.focus = focus;
        self.focused_component = match focus {
            tundra_ui::ClockCreateDialogFocus::Input => ShellComponent::ClockCreateInput,
            tundra_ui::ClockCreateDialogFocus::CreateAlarm => {
                ShellComponent::ClockCreateAlarmButton
            }
            tundra_ui::ClockCreateDialogFocus::CreateCountdown => {
                ShellComponent::ClockCreateCountdownButton
            }
        };
    }

    fn append_clock_create_char(&mut self, character: char) {
        let Some(state) = self.clock_create_state.as_mut() else {
            return;
        };
        if state.focus != tundra_ui::ClockCreateDialogFocus::Input
            || state.input.len() >= 8
            || !(character.is_ascii_digit() || character == ' ')
        {
            return;
        }
        state.input.push(character);
        state.error = None;
    }

    fn clock_create_backspace(&mut self) {
        let Some(state) = self.clock_create_state.as_mut() else {
            return;
        };
        if state.focus == tundra_ui::ClockCreateDialogFocus::Input {
            state.input.pop();
            state.error = None;
        }
    }

    fn create_clock_entry(&mut self, kind: ScheduledClockEntryKind) {
        let Some(input) = self
            .clock_create_state
            .as_ref()
            .map(|state| state.input.clone())
        else {
            return;
        };
        let snapshot = self.network_clock.snapshot();
        let now = Instant::now();
        let Some(previous) = self.clock_scheduler.clone() else {
            if let Some(state) = self.clock_create_state.as_mut() {
                state.error = Some("Sign in to create clock entries".to_string());
            }
            return;
        };
        let result = match (kind, self.clock_scheduler.as_mut()) {
            (ScheduledClockEntryKind::DailyAlarm, Some(scheduler)) => {
                scheduler.create_daily_alarm(&input, &snapshot)
            }
            (ScheduledClockEntryKind::Countdown, Some(scheduler)) => {
                scheduler.create_countdown(&input, &snapshot, now)
            }
            (_, None) => Err(ClockSchedulerError::EntryNotFound),
        };
        let id = match result {
            Ok(id) => id,
            Err(error) => {
                if let Some(state) = self.clock_create_state.as_mut() {
                    state.error = Some(error.to_string());
                }
                return;
            }
        };
        if let Err(error) = self.commit_clock_mutation(previous, &snapshot, now) {
            if let Some(state) = self.clock_create_state.as_mut() {
                state.error = Some(format!("Could not save: {error}"));
            }
            return;
        }

        self.clock_create_state = None;
        self.clock_selected_entry_id = Some(id);
        self.focused_component = ShellComponent::ClockEntryList;
        self.sync_clock_window_at(now);
        self.notify_toast(match kind {
            ScheduledClockEntryKind::DailyAlarm => "Daily alarm created",
            ScheduledClockEntryKind::Countdown => "Countdown created",
        });
        self.refresh_hit_map();
    }

    fn ordered_clock_entry_ids_at(&self, now: Instant) -> Vec<u64> {
        let Some(scheduler) = &self.clock_scheduler else {
            return Vec::new();
        };
        let entries = scheduler.entries(now);
        entries
            .iter()
            .filter(|entry| entry.kind == ScheduledClockEntryKind::DailyAlarm)
            .chain(
                entries
                    .iter()
                    .filter(|entry| entry.kind == ScheduledClockEntryKind::Countdown),
            )
            .map(|entry| entry.id)
            .collect()
    }

    fn sync_clock_selection(&mut self) {
        self.sync_clock_selection_at(Instant::now());
    }

    fn sync_clock_selection_at(&mut self, now: Instant) {
        let ids = self.ordered_clock_entry_ids_at(now);
        if !self
            .clock_selected_entry_id
            .is_some_and(|selected| ids.contains(&selected))
        {
            self.clock_selected_entry_id = ids.first().copied();
        }
        self.sync_clock_window_at(now);
    }

    fn clock_entry_capacity_at(&self, now: Instant) -> usize {
        let (width, height) = self.terminal_size;
        let area = Rect::new(0, 0, width, height);
        let tundra_ui::ShellLayout::Full { main, .. } = tundra_ui::compute_shell_layout(area)
        else {
            return 1;
        };
        let snapshot = self.network_clock.snapshot();
        let model = self.to_clock_view_model_at(&snapshot, now);
        tundra_ui::clock_page_layout(main, &model)
            .entry_capacity
            .max(1)
    }

    fn sync_clock_window_at(&mut self, now: Instant) {
        let ids = self.ordered_clock_entry_ids_at(now);
        let capacity = self.clock_entry_capacity_at(now);
        let max_start = ids.len().saturating_sub(capacity);
        self.clock_entry_window_start = self.clock_entry_window_start.min(max_start);
        let Some(index) = self
            .clock_selected_entry_id
            .and_then(|selected| ids.iter().position(|id| *id == selected))
        else {
            self.clock_entry_window_start = 0;
            return;
        };
        if index < self.clock_entry_window_start {
            self.clock_entry_window_start = index;
        } else if index >= self.clock_entry_window_start.saturating_add(capacity) {
            self.clock_entry_window_start = index.saturating_add(1).saturating_sub(capacity);
        }
    }

    fn select_clock_entry_delta(&mut self, delta: isize) {
        let now = Instant::now();
        let ids = self.ordered_clock_entry_ids_at(now);
        if ids.is_empty() {
            self.clock_selected_entry_id = None;
            self.focused_component = ShellComponent::ClockNewButton;
            return;
        }
        let current = self
            .clock_selected_entry_id
            .and_then(|selected| ids.iter().position(|id| *id == selected))
            .unwrap_or(0);
        let next =
            (current as isize + delta).clamp(0, ids.len().saturating_sub(1) as isize) as usize;
        self.clock_selected_entry_id = Some(ids[next]);
        self.focused_component = ShellComponent::ClockEntryList;
        self.sync_clock_window_at(now);
    }

    fn select_clock_entry_edge(&mut self, last: bool) {
        let now = Instant::now();
        let ids = self.ordered_clock_entry_ids_at(now);
        self.clock_selected_entry_id = if last {
            ids.last().copied()
        } else {
            ids.first().copied()
        };
        if self.clock_selected_entry_id.is_some() {
            self.focused_component = ShellComponent::ClockEntryList;
        }
        self.sync_clock_window_at(now);
    }

    fn select_clock_entry(&mut self, id: u64) {
        if self
            .ordered_clock_entry_ids_at(Instant::now())
            .contains(&id)
        {
            self.clock_selected_entry_id = Some(id);
            self.focused_component = ShellComponent::ClockEntryList;
            self.sync_clock_window_at(Instant::now());
        }
    }

    fn show_clock_manage_dialog(&mut self, id: u64) {
        let Some(entry) = self.clock_scheduler.as_ref().and_then(|scheduler| {
            scheduler
                .entries(Instant::now())
                .into_iter()
                .find(|entry| entry.id == id)
        }) else {
            self.notify_toast("Clock entry no longer exists");
            return;
        };
        self.clock_selected_entry_id = Some(id);
        let (title, kind_label) = match entry.kind {
            ScheduledClockEntryKind::DailyAlarm => ("Manage Alarm", "Daily alarm"),
            ScheduledClockEntryKind::Countdown => ("Manage Countdown", "Countdown"),
        };
        let toggle_label = if entry.strong {
            "Turn Strong Off"
        } else {
            "Turn Strong On"
        };
        let user_id = self
            .auth_session
            .as_ref()
            .map(|session| session.user_id.as_str())
            .unwrap_or("unknown");
        self.notify_modal_with_options(
            ShellNotification::modal(
                title,
                format!("{kind_label} {}", entry.display_time),
                tundra_ui::NotificationTone::Info,
                vec![
                    ShellNotificationAction::new("delete", "Delete")
                        .with_shortcut(InputKey::Character('x'))
                        .with_follow_up(ShellCommand::ClockDeleteEntry(id)),
                    ShellNotificationAction::new("toggle-strong", toggle_label)
                        .with_shortcut(InputKey::Character('t'))
                        .with_follow_up(ShellCommand::ClockToggleStrong(id)),
                    ShellNotificationAction::new("cancel", "Cancel")
                        .with_shortcut(InputKey::Escape)
                        .cancel(),
                ],
            )
            .with_key(format!(
                "{CLOCK_MANAGE_NOTIFICATION_KEY_PREFIX}.{user_id}.{id}"
            ))
            .with_component(ShellComponent::NotificationDialog),
        );
    }

    fn delete_clock_entry(&mut self, id: u64) {
        let snapshot = self.network_clock.snapshot();
        let now = Instant::now();
        let Some(previous) = self.clock_scheduler.clone() else {
            return;
        };
        if !self
            .clock_scheduler
            .as_mut()
            .is_some_and(|scheduler| scheduler.delete(id))
        {
            self.notify_toast("Clock entry no longer exists");
            return;
        }
        if self.commit_clock_mutation(previous, &snapshot, now).is_ok() {
            if let Some(user_id) = self
                .auth_session
                .as_ref()
                .map(|session| session.user_id.as_str())
            {
                self.notifications.dismiss_modal_by_key(&format!(
                    "{CLOCK_DUE_NOTIFICATION_KEY_PREFIX}.{user_id}.{id}"
                ));
            }
            self.sync_clock_selection_at(now);
            self.notify_toast("Clock entry deleted");
            self.refresh_hit_map();
        }
    }

    fn toggle_clock_entry_strong(&mut self, id: u64) {
        let snapshot = self.network_clock.snapshot();
        let now = Instant::now();
        let Some(previous) = self.clock_scheduler.clone() else {
            return;
        };
        let Some(enabled) = self
            .clock_scheduler
            .as_mut()
            .and_then(|scheduler| scheduler.toggle_strong(id))
        else {
            self.notify_toast("Clock entry no longer exists");
            return;
        };
        if self.commit_clock_mutation(previous, &snapshot, now).is_ok() {
            self.notify_toast(if enabled {
                "Strong notification enabled"
            } else {
                "Strong notification disabled"
            });
            self.refresh_hit_map();
        }
    }

    fn snooze_clock_alarm(&mut self, id: u64) {
        let snapshot = self.network_clock.snapshot();
        let now = Instant::now();
        let Some(previous) = self.clock_scheduler.clone() else {
            return;
        };
        let retry_event = previous
            .entries(now)
            .into_iter()
            .find(|entry| {
                entry.id == id && entry.kind == ScheduledClockEntryKind::DailyAlarm && entry.strong
            })
            .map(|entry| DueEvent {
                id: entry.id,
                kind: entry.kind,
                strong: true,
                display_time: entry.display_time,
            });
        let result = self
            .clock_scheduler
            .as_mut()
            .ok_or(ClockSchedulerError::EntryNotFound)
            .and_then(|scheduler| scheduler.snooze_five_minutes(id, &snapshot, now));
        match result {
            Ok(()) => {
                if self.commit_clock_mutation(previous, &snapshot, now).is_ok() {
                    self.notify_toast("Alarm snoozed for 5 minutes");
                    self.refresh_hit_map();
                } else if let Some(event) = retry_event {
                    let _ = self.handle_clock_due_events(vec![event]);
                    self.refresh_hit_map();
                }
            }
            Err(error) => self.notify_toast(error.to_string()),
        }
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
        self.notify_status("Explorer");
        self.apply_explorer_command(ExplorerCommand::Refresh, platform);
        self.refresh_hit_map();
    }

    fn close_explorer(&mut self) {
        self.explorer_input_mode = ExplorerInputMode::Browse;
        self.explorer_input.clear();
        self.resolve_explorer_alert();
        self.pop_to_home();
        self.notify_status("Ready");
    }

    fn resolve_explorer_alert(&mut self) {
        let resolved_message = self
            .notifications
            .alert_message_for_key(EXPLORER_ALERT_KEY)
            .map(str::to_string);
        if self.error_message.as_ref() == resolved_message.as_ref() {
            self.error_message = None;
        }
        self.resolve_notification_alert(EXPLORER_ALERT_KEY);
    }

    fn apply_explorer_command(&mut self, command: ExplorerCommand, platform: &dyn Platform) {
        let command_kind = command.clone();
        let Some(storage) = self.storage_manager.clone() else {
            let message = "Storage unavailable".to_string();
            self.error_message = Some(message.clone());
            self.notify_alert_with_key(
                EXPLORER_ALERT_KEY,
                message,
                tundra_ui::NotificationTone::Error,
            );
            return;
        };
        let session = self.auth_session.clone();
        let Some(state) = self.explorer_state.as_mut() else {
            let message = "Explorer unavailable".to_string();
            self.error_message = Some(message.clone());
            self.notify_alert_with_key(
                EXPLORER_ALERT_KEY,
                message,
                tundra_ui::NotificationTone::Error,
            );
            return;
        };

        ExplorerController::default().apply(state, command, session.as_ref(), platform, &storage);
        let pending_dialog = state.pending_dialog.clone();
        let explorer_error = state.error.clone();
        let explorer_message = state.message.clone();
        if let Some(error) = explorer_error {
            self.error_message = Some(error.clone());
            self.notify_alert_with_key(
                EXPLORER_ALERT_KEY,
                error,
                tundra_ui::NotificationTone::Error,
            );
            self.notify_status("Explorer error");
        } else {
            self.error_message = None;
            self.resolve_explorer_alert();
            if let Some(message) = explorer_message {
                self.notify_status(message);
            }
        }

        if matches!(
            command_kind,
            ExplorerCommand::DeleteToTrash | ExplorerCommand::ConfirmDelete
        ) && let Some(dialog) = pending_dialog
        {
            self.notify_modal_with_options(
                ShellNotification::modal(
                    dialog.title,
                    dialog.message,
                    tundra_ui::NotificationTone::Warning,
                    vec![
                        ShellNotificationAction::new("confirm", "Move")
                            .with_shortcut(InputKey::Character('y'))
                            .with_follow_up(ShellCommand::ExplorerConfirmDelete),
                        ShellNotificationAction::new("cancel", "Cancel")
                            .with_shortcut(InputKey::Character('n'))
                            .cancel()
                            .with_follow_up(ShellCommand::CancelExplorerInput),
                    ],
                )
                .with_key(EXPLORER_DELETE_NOTIFICATION_KEY),
            );
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
        self.notify_status(explorer_input_prompt(mode));
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
            self.notifications
                .dismiss_modal_by_key(EXPLORER_DELETE_NOTIFICATION_KEY);
            self.notify_status("Cancelled");
            return;
        }
        self.explorer_input_mode = ExplorerInputMode::Browse;
        self.explorer_input.clear();
        self.notify_status("Explorer");
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

    fn refresh_user_management(&mut self) -> bool {
        let Some(storage) = self.storage_manager.clone() else {
            self.report_user_management_refresh_error("Storage unavailable".to_string());
            return false;
        };
        let Some(session) = self.auth_session.clone() else {
            self.report_user_management_refresh_error("Login required".to_string());
            return false;
        };
        let users = match UserService::with_debug_policy(storage, self.debug_policy)
            .list_accessible_users(&session)
        {
            Ok(users) => users,
            Err(error) => {
                self.report_user_management_refresh_error(format_core_error(&error));
                return false;
            }
        };
        let selected_username = self.selected_managed_username();
        self.user_management_users = users;
        if self.user_management_users.is_empty() {
            self.user_management_selected = 0;
            self.user_management_window_start = 0;
            self.user_management_focus = UserManagementPageFocus::UserList;
        } else if let Some(username) = selected_username {
            self.user_management_selected = self
                .user_management_users
                .iter()
                .position(|user| user.username.eq_ignore_ascii_case(&username))
                .unwrap_or_else(|| {
                    self.user_management_selected
                        .min(self.user_management_users.len() - 1)
                });
        } else {
            self.user_management_selected = self
                .user_management_selected
                .min(self.user_management_users.len() - 1);
        }
        self.ensure_user_management_selection_visible();
        self.normalize_user_management_focus();
        self.resolve_user_management_refresh_alert();
        true
    }

    fn resolve_user_management_refresh_alert(&mut self) {
        let resolved_message = self
            .notifications
            .alert_message_for_key(USER_MANAGEMENT_REFRESH_ALERT_KEY)
            .map(str::to_string);
        if self.user_management_message.as_ref() == resolved_message.as_ref() {
            self.user_management_message = None;
        }
        if self.error_message.as_ref() == resolved_message.as_ref() {
            self.error_message = None;
        }
        self.resolve_notification_alert(USER_MANAGEMENT_REFRESH_ALERT_KEY);
    }

    fn report_user_management_refresh_error(&mut self, message: String) {
        self.error_message = Some(message.clone());
        self.user_management_message = Some(message.clone());
        self.user_management_feedback_tone = UserManagementFeedbackTone::Error;
        self.notify_alert_with_key(
            USER_MANAGEMENT_REFRESH_ALERT_KEY,
            message,
            tundra_ui::NotificationTone::Error,
        );
    }

    fn select_user_management_row(&mut self, direction: isize) {
        if self.user_management_users.is_empty() {
            return;
        }
        let last = self.user_management_users.len().saturating_sub(1) as isize;
        let next = (self.user_management_selected as isize + direction).clamp(0, last);
        self.user_management_selected = next as usize;
        self.user_management_focus = UserManagementPageFocus::UserList;
        self.ensure_user_management_selection_visible();
    }

    fn select_user_management_edge(&mut self, last: bool) {
        if self.user_management_users.is_empty() {
            return;
        }
        self.user_management_selected = if last {
            self.user_management_users.len().saturating_sub(1)
        } else {
            0
        };
        self.user_management_focus = UserManagementPageFocus::UserList;
        self.ensure_user_management_selection_visible();
    }

    fn select_user_management_page(&mut self, direction: isize) {
        let page = self.user_management_visible_row_count().max(1) as isize;
        self.select_user_management_row(direction.saturating_mul(page));
    }

    fn begin_create_managed_user(&mut self) {
        if !self.can_manage_all_users() {
            return;
        }
        self.user_management_mode = UserManagementMode::Create(UserManagementCreateForm {
            username: String::new(),
            display_name: String::new(),
            password: String::new(),
            role: UserRole::User,
            focused_field: UserManagementFormField::Username,
        });
        self.user_management_message = None;
        self.user_management_feedback_tone = UserManagementFeedbackTone::Info;
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
                focused_field: UserManagementFormField::DisplayName,
            });
            self.user_management_message = None;
            self.user_management_feedback_tone = UserManagementFeedbackTone::Info;
        }
    }

    fn begin_set_selected_password(&mut self) {
        if let Some(username) = self.selected_managed_username() {
            self.user_management_mode = UserManagementMode::Password(UserManagementPasswordForm {
                username,
                password: String::new(),
                focused_field: UserManagementFormField::Password,
            });
            self.user_management_message = None;
            self.user_management_feedback_tone = UserManagementFeedbackTone::Info;
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
                    UserRole::Admin => UserRole::User,
                })
                .unwrap_or(UserRole::User);
            let changed = self
                .run_selected_user_operation("Changed role for", |service, session| {
                    service.change_role(session, &username, next_role)
                });
            if changed {
                self.sync_current_session_role();
                let _refresh_succeeded = self.refresh_user_management();
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
                self.user_management_feedback_tone = UserManagementFeedbackTone::Success;
                true
            }
            Err(error) => {
                self.user_management_message = Some(format_core_error(&error));
                self.user_management_feedback_tone = UserManagementFeedbackTone::Error;
                false
            }
        };
        let _refresh_succeeded = self.refresh_user_management();
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
                        self.user_management_feedback_tone = UserManagementFeedbackTone::Success;
                        format!("Created {}", account.username)
                    }
                    Err(error) => {
                        self.user_management_feedback_tone = UserManagementFeedbackTone::Error;
                        format_core_error(&error)
                    }
                });
                if !self.refresh_user_management() {
                    return;
                }
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
                        self.user_management_feedback_tone = UserManagementFeedbackTone::Success;
                        format!("Updated {}", account.username)
                    }
                    Err(error) => {
                        self.user_management_feedback_tone = UserManagementFeedbackTone::Error;
                        format_core_error(&error)
                    }
                });
            }
            UserManagementMode::Password(form) => {
                let result = service.set_user_password(session, &form.username, &form.password);
                self.user_management_message = Some(match result {
                    Ok(()) => {
                        self.user_management_mode = UserManagementMode::Browse;
                        self.user_management_feedback_tone = UserManagementFeedbackTone::Success;
                        format!("Updated password for {}", form.username)
                    }
                    Err(error) => {
                        self.user_management_feedback_tone = UserManagementFeedbackTone::Error;
                        format_core_error(&error)
                    }
                });
            }
        }
        let _refresh_succeeded = self.refresh_user_management();
    }

    fn delete_selected_user(&mut self) {
        let Some(username) = self.selected_managed_username() else {
            return;
        };
        let deleted_user_id = self
            .user_management_users
            .iter()
            .find(|user| user.username.eq_ignore_ascii_case(&username))
            .map(|user| user.id.clone());
        let Some(storage) = self.storage_manager.clone() else {
            return;
        };
        let Some(session) = self.auth_session.as_ref() else {
            return;
        };
        let deleting_current_user = self.is_current_username(&username);
        let deleted = match UserService::with_debug_policy(storage.clone(), self.debug_policy)
            .delete_user(session, &username)
        {
            Ok(()) => {
                self.user_management_message = Some(format!("Deleted {username}"));
                self.user_management_feedback_tone = UserManagementFeedbackTone::Success;
                true
            }
            Err(error) => {
                self.user_management_message = Some(format_core_error(&error));
                self.user_management_feedback_tone = UserManagementFeedbackTone::Error;
                false
            }
        };
        if deleted && let Some(user_id) = deleted_user_id {
            match storage.load_clock() {
                Ok(mut document) => {
                    document.profiles.remove(&user_id);
                    if let Err(error) = storage.save_clock(&document) {
                        self.report_clock_storage_error(error.to_string());
                    }
                }
                Err(error) => self.report_clock_storage_error(error.to_string()),
            }
        }
        if deleted && deleting_current_user {
            self.return_to_login("Account deleted");
            return;
        }
        let _refresh_succeeded = self.refresh_user_management();
    }

    fn append_user_management_char(&mut self, character: char) {
        match &mut self.user_management_mode {
            UserManagementMode::Create(form) => match form.focused_field {
                UserManagementFormField::Username => form.username.push(character),
                UserManagementFormField::DisplayName => form.display_name.push(character),
                UserManagementFormField::Password => form.password.push(character),
                UserManagementFormField::Role
                | UserManagementFormField::Submit
                | UserManagementFormField::Cancel => {}
            },
            UserManagementMode::EditInfo(form)
                if form.focused_field == UserManagementFormField::DisplayName =>
            {
                form.display_name.push(character);
            }
            UserManagementMode::Password(form)
                if form.focused_field == UserManagementFormField::Password =>
            {
                form.password.push(character);
            }
            UserManagementMode::EditInfo(_) | UserManagementMode::Password(_) => {}
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
                UserManagementFormField::Role
                | UserManagementFormField::Submit
                | UserManagementFormField::Cancel => {}
            },
            UserManagementMode::EditInfo(form)
                if form.focused_field == UserManagementFormField::DisplayName =>
            {
                form.display_name.pop();
            }
            UserManagementMode::Password(form)
                if form.focused_field == UserManagementFormField::Password =>
            {
                form.password.pop();
            }
            UserManagementMode::EditInfo(_) | UserManagementMode::Password(_) => {}
            UserManagementMode::Browse => {}
        }
    }

    fn move_user_management_form_focus(&mut self, direction: i8) {
        let fields: &[UserManagementFormField] = match self.user_management_mode {
            UserManagementMode::Create(_) => &[
                UserManagementFormField::Username,
                UserManagementFormField::DisplayName,
                UserManagementFormField::Role,
                UserManagementFormField::Password,
                UserManagementFormField::Submit,
                UserManagementFormField::Cancel,
            ],
            UserManagementMode::EditInfo(_) => &[
                UserManagementFormField::DisplayName,
                UserManagementFormField::Submit,
                UserManagementFormField::Cancel,
            ],
            UserManagementMode::Password(_) => &[
                UserManagementFormField::Password,
                UserManagementFormField::Submit,
                UserManagementFormField::Cancel,
            ],
            UserManagementMode::Browse => return,
        };
        let current = self.user_management_form_field();
        let index = fields
            .iter()
            .position(|field| Some(*field) == current)
            .unwrap_or(0);
        let next = (index as isize + direction as isize).rem_euclid(fields.len() as isize) as usize;
        self.set_user_management_form_field(fields[next]);
    }

    fn cancel_user_management_form(&mut self) {
        if self.user_management_mode != UserManagementMode::Browse {
            self.user_management_mode = UserManagementMode::Browse;
            self.user_management_message = Some("Cancelled".to_string());
            self.user_management_feedback_tone = UserManagementFeedbackTone::Info;
            self.ensure_user_management_selection_visible();
        }
    }

    fn user_management_form_field(&self) -> Option<UserManagementFormField> {
        match &self.user_management_mode {
            UserManagementMode::Browse => None,
            UserManagementMode::Create(form) => Some(form.focused_field),
            UserManagementMode::EditInfo(form) => Some(form.focused_field),
            UserManagementMode::Password(form) => Some(form.focused_field),
        }
    }

    fn set_user_management_form_field(&mut self, field: UserManagementFormField) {
        match &mut self.user_management_mode {
            UserManagementMode::Browse => {}
            UserManagementMode::Create(form) => form.focused_field = field,
            UserManagementMode::EditInfo(form) => form.focused_field = field,
            UserManagementMode::Password(form) => form.focused_field = field,
        }
    }

    fn set_user_management_form_focus(&mut self, field: tundra_ui::UserManagementField) {
        let field = from_ui_user_management_field(field);
        let valid = match self.user_management_mode {
            UserManagementMode::Browse => false,
            UserManagementMode::Create(_) => true,
            UserManagementMode::EditInfo(_) => matches!(
                field,
                UserManagementFormField::DisplayName
                    | UserManagementFormField::Submit
                    | UserManagementFormField::Cancel
            ),
            UserManagementMode::Password(_) => matches!(
                field,
                UserManagementFormField::Password
                    | UserManagementFormField::Submit
                    | UserManagementFormField::Cancel
            ),
        };
        if valid {
            self.set_user_management_form_field(field);
        }
    }

    fn toggle_user_management_form_role(&mut self) {
        if let UserManagementMode::Create(form) = &mut self.user_management_mode {
            form.role = if form.role == UserRole::Admin {
                UserRole::User
            } else {
                UserRole::Admin
            };
        }
    }

    fn move_user_management_page_focus(&mut self, direction: i8) {
        let order = self.user_management_focus_order();
        if order.is_empty() {
            self.user_management_focus = UserManagementPageFocus::UserList;
            return;
        }
        let current = order
            .iter()
            .position(|focus| *focus == self.user_management_focus)
            .unwrap_or(0);
        let next = (current as isize + direction as isize).rem_euclid(order.len() as isize);
        self.user_management_focus = order[next as usize];
    }

    fn user_management_focus_order(&self) -> Vec<UserManagementPageFocus> {
        let mut order = vec![UserManagementPageFocus::UserList];
        order.extend(
            self.user_management_action_view_models()
                .into_iter()
                .filter(|action| action.enabled)
                .map(|action| UserManagementPageFocus::Action(action.action)),
        );
        order
    }

    fn normalize_user_management_focus(&mut self) {
        if !self
            .user_management_focus_order()
            .contains(&self.user_management_focus)
        {
            self.user_management_focus = UserManagementPageFocus::UserList;
        }
    }

    fn focus_user_management_action(&mut self, action: tundra_ui::UserManagementAction) {
        if self.user_management_action_enabled(action) {
            self.user_management_focus = UserManagementPageFocus::Action(action);
        }
    }

    fn user_management_action_enabled(&self, action: tundra_ui::UserManagementAction) -> bool {
        self.user_management_action_view_models()
            .iter()
            .find(|model| model.action == action)
            .is_some_and(|model| model.enabled)
    }

    fn activate_focused_user_management_control(&mut self) {
        match self.user_management_focus {
            UserManagementPageFocus::UserList => {}
            UserManagementPageFocus::Action(action) => {
                self.activate_user_management_action(action);
            }
        }
    }

    fn activate_user_management_action(&mut self, action: tundra_ui::UserManagementAction) {
        use tundra_ui::UserManagementAction;

        let action_model = self
            .user_management_action_view_models()
            .into_iter()
            .find(|model| model.action == action);
        let Some(action_model) = action_model else {
            return;
        };
        if !action_model.enabled {
            if let Some(reason) = action_model.disabled_reason {
                self.user_management_message = Some(reason);
                self.user_management_feedback_tone = UserManagementFeedbackTone::Error;
                self.ensure_user_management_selection_visible();
            }
            return;
        }

        match action {
            UserManagementAction::NewUser => self.begin_create_managed_user(),
            UserManagementAction::EditInfo => self.begin_edit_selected_user_info(),
            UserManagementAction::SetPassword => self.begin_set_selected_password(),
            UserManagementAction::ToggleEnabled => {
                let should_disable = self
                    .user_management_users
                    .get(self.user_management_selected)
                    .is_some_and(|user| user.enabled && !user_is_locked(user));
                if should_disable {
                    self.disable_selected_user();
                } else {
                    self.unlock_selected_user();
                }
            }
            UserManagementAction::ToggleRole => self.cycle_selected_role(),
            UserManagementAction::Delete => self.request_delete_selected_user(),
            UserManagementAction::Back => self.close_user_management(),
        }
        self.normalize_user_management_focus();
    }

    fn request_delete_selected_user(&mut self) {
        use tundra_ui::UserManagementAction;

        if !self.user_management_action_enabled(UserManagementAction::Delete) {
            self.activate_user_management_action(UserManagementAction::Delete);
            return;
        }
        let Some(username) = self.selected_managed_username() else {
            return;
        };
        let deleting_current_user = self.is_current_username(&username);
        let title = if deleting_current_user {
            "Delete your account"
        } else {
            "Delete user"
        };
        let message = if deleting_current_user {
            format!("Delete {username}? You will be signed out immediately.")
        } else {
            format!("Delete {username}? This action cannot be undone.")
        };
        self.notify_modal_with_options(
            ShellNotification::modal(
                title,
                message,
                tundra_ui::NotificationTone::Warning,
                vec![
                    ShellNotificationAction::new("delete", "Delete")
                        .with_shortcut(InputKey::Character('x'))
                        .with_follow_up(ShellCommand::DeleteManagedUser),
                    ShellNotificationAction::new("cancel", "Cancel")
                        .with_shortcut(InputKey::Escape)
                        .cancel(),
                ],
            )
            .with_selected_action(1)
            .with_key(USER_MANAGEMENT_DELETE_NOTIFICATION_KEY)
            .with_component(ShellComponent::NotificationDialog),
        );
    }

    fn selected_is_last_enabled_admin(&self) -> bool {
        let Some(selected) = self
            .user_management_users
            .get(self.user_management_selected)
        else {
            return false;
        };
        selected.enabled
            && selected.role == UserRole::Admin
            && self
                .user_management_users
                .iter()
                .filter(|user| user.enabled && user.role == UserRole::Admin)
                .count()
                <= 1
    }

    fn user_management_visible_row_count(&self) -> usize {
        self.user_management_layout()
            .map(|layout| layout.visible_capacity)
            .unwrap_or(0)
    }

    fn ensure_user_management_selection_visible(&mut self) {
        let count = self.user_management_users.len();
        if count == 0 {
            self.user_management_selected = 0;
            self.user_management_window_start = 0;
            return;
        }
        self.user_management_selected = self.user_management_selected.min(count - 1);
        let capacity = self.user_management_visible_row_count().min(count);
        if capacity == 0 {
            self.user_management_window_start = 0;
            return;
        }
        let max_start = count.saturating_sub(capacity);
        self.user_management_window_start = self.user_management_window_start.min(max_start);
        if self.user_management_selected < self.user_management_window_start {
            self.user_management_window_start = self.user_management_selected;
        } else if self.user_management_selected
            >= self.user_management_window_start.saturating_add(capacity)
        {
            self.user_management_window_start = self
                .user_management_selected
                .saturating_add(1)
                .saturating_sub(capacity);
        }
    }

    fn user_management_layout(&self) -> Option<tundra_ui::UserManagementLayout> {
        let area = Rect::new(0, 0, self.terminal_size.0, self.terminal_size.1);
        let tundra_ui::ShellLayout::Full { main, .. } = tundra_ui::compute_shell_layout(area)
        else {
            return None;
        };
        Some(tundra_ui::user_management_layout(
            main,
            &self.to_user_management_view_model(),
        ))
    }

    fn close_user_management(&mut self) {
        self.user_management_mode = UserManagementMode::Browse;
        self.resolve_user_management_refresh_alert();
        self.pop_to_home();
        self.notify_status("Ready");
        self.refresh_hit_map();
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
        self.resolve_user_management_refresh_alert();
        self.notifications.dismiss_modals_by_key_prefix("clock.");
        self.notifications.resolve_alert(CLOCK_STORAGE_ALERT_KEY);
        self.notifications.clear_toast();
        self.modal_focus_context = None;
        self.modal_focus_prepared_for_follow_up = false;
        self.notification_pointer_capture = None;
        self.pending_notification_commands.clear();
        self.auth_session = None;
        self.clock_scheduler = None;
        self.clock_selected_entry_id = None;
        self.clock_entry_window_start = 0;
        self.clock_create_state = None;
        self.clock_persist_pending = false;
        self.clock_pending_due_summary = None;
        self.clock_profile_pending_sync = None;
        self.user_management_users.clear();
        self.user_management_selected = 0;
        self.user_management_window_start = 0;
        self.user_management_focus = UserManagementPageFocus::UserList;
        self.user_management_feedback_tone = UserManagementFeedbackTone::Info;
        self.user_management_mode = UserManagementMode::Browse;
        self.login_password.clear();
        let _ = self.refresh_login_users_from_storage();
        self.screen_stack = vec![ShellScreen::Login];
        self.focused_component = ShellComponent::LoginUserList;
        self.notify_status(status);
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
        self.notify_status(format!(
            "Home: {}",
            entries[self.selected_home_entry_index].label
        ));
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
                self.notify_status(format!("{label} is not implemented yet"));
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

    fn notification_action_index_at(&self, coordinates: CellPosition) -> Option<usize> {
        let model = self.notifications.active_modal_view_model()?;
        let area = Rect::new(0, 0, self.terminal_size.0, self.terminal_size.1);
        let tundra_ui::NotificationLayout::Dialog(layout) =
            tundra_ui::notification_layout(area, &model)
        else {
            return None;
        };

        layout
            .actions
            .iter()
            .find(|action| rect_contains(action.area, coordinates))
            .map(|action| action.index)
    }

    fn notification_can_render(&self) -> bool {
        let Some(model) = self.notifications.active_modal_view_model() else {
            return false;
        };
        let area = Rect::new(0, 0, self.terminal_size.0, self.terminal_size.1);
        matches!(
            tundra_ui::notification_layout(area, &model),
            tundra_ui::NotificationLayout::Dialog(_)
        )
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
        let received_at = Instant::now();
        self.notifications.expire(received_at);
        let routed = self.route_input_at(input, received_at);
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
        self.pending_notification_commands.clear();
        let follow_up_input = routed.input.clone();
        let follow_up_target = routed.target;
        let mut action = self.apply_routed_event_once(routed, platform);
        let mut steps = 0_usize;

        while action != ShellAction::Exit {
            let Some(command) = self.pending_notification_commands.pop_front() else {
                break;
            };
            if steps >= MAX_NOTIFICATION_FOLLOW_UP_STEPS {
                self.pending_notification_commands.clear();
                self.notify_alert_with_key(
                    NOTIFICATION_FOLLOW_UP_ALERT_KEY,
                    "Notification follow-up limit reached",
                    tundra_ui::NotificationTone::Critical,
                );
                break;
            }
            steps = steps.saturating_add(1);
            action = self.apply_routed_event_once(
                RoutedEvent {
                    input: follow_up_input.clone(),
                    target: follow_up_target,
                    command,
                },
                platform,
            );
        }

        if action == ShellAction::Exit {
            self.pending_notification_commands.clear();
        }
        self.finish_modal_focus_transition();
        action
    }

    fn apply_routed_event_once(
        &mut self,
        routed: RoutedEvent,
        platform: &dyn Platform,
    ) -> ShellAction {
        self.record_input_diagnostics(&routed);
        if !matches!(routed.input, InputEvent::Mouse(_)) {
            self.notification_pointer_capture = None;
        }
        self.last_routed_target = Some(routed.target);
        self.last_command = Some(routed.command.clone());

        match routed.command {
            ShellCommand::Shutdown => {
                self.shutdown_requested = true;
                ShellAction::Exit
            }
            ShellCommand::Tick => {
                self.tick_count = self.tick_count.saturating_add(1);
                self.notifications.tick();
                self.advance_clock_background();
                ShellAction::Redraw
            }
            ShellCommand::RefreshHitMap { width, height } => {
                self.terminal_size = (width, height);
                self.notification_pointer_capture = None;
                self.last_resize_event = Some(format!("{width}x{height}"));
                if self.active_screen() == ShellScreen::FirstRunSetup {
                    self.sync_setup_timezone_window();
                }
                if self.active_screen() == ShellScreen::Login {
                    self.sync_login_user_window();
                }
                if self.active_screen() == ShellScreen::UserManagement {
                    self.ensure_user_management_selection_visible();
                }
                self.refresh_hit_map();
                ShellAction::Redraw
            }
            ShellCommand::RequestExit => {
                self.capture_modal_focus_context();
                if self.active_screen() != ShellScreen::ExitConfirm {
                    self.screen_stack.push(ShellScreen::ExitConfirm);
                }
                self.active_popup = None;
                self.notify_status("Confirm exit");
                self.notify_modal_with_options(
                    ShellNotification::modal(
                        "Exit TundraUX 3",
                        "Leave the shell and restore the terminal?",
                        tundra_ui::NotificationTone::Warning,
                        vec![
                            ShellNotificationAction::new("confirm", "Exit")
                                .with_shortcut(InputKey::Character('y'))
                                .with_follow_up(ShellCommand::ConfirmExit),
                            ShellNotificationAction::new("cancel", "Cancel")
                                .with_shortcut(InputKey::Character('n'))
                                .cancel()
                                .with_follow_up(ShellCommand::CancelExit),
                        ],
                    )
                    .with_key(EXIT_CONFIRM_NOTIFICATION_KEY)
                    .with_component(ShellComponent::ExitDialog),
                );
                self.refresh_hit_map();
                ShellAction::Redraw
            }
            ShellCommand::ConfirmExit => {
                self.shutdown_requested = true;
                ShellAction::Exit
            }
            ShellCommand::CancelExit => {
                self.notifications
                    .dismiss_modal_by_key(EXIT_CONFIRM_NOTIFICATION_KEY);
                self.cancel_exit_confirmation();
                self.active_popup = None;
                self.notify_status("Ready");
                self.refresh_hit_map();
                ShellAction::Redraw
            }
            ShellCommand::FocusNext => {
                self.move_focus(1);
                self.notify_status(format!("Focus: {}", self.focused_component.label()));
                ShellAction::Redraw
            }
            ShellCommand::FocusPrevious => {
                self.move_focus(-1);
                self.notify_status(format!("Focus: {}", self.focused_component.label()));
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
                self.close_user_management();
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
            ShellCommand::ClockOpenCreate => {
                self.open_clock_create_dialog();
                ShellAction::Redraw
            }
            ShellCommand::ClockCloseCreate => {
                self.close_clock_create_dialog();
                ShellAction::Redraw
            }
            ShellCommand::ClockCreateFocusNext => {
                self.move_clock_create_focus(1);
                ShellAction::Redraw
            }
            ShellCommand::ClockCreateFocusPrevious => {
                self.move_clock_create_focus(-1);
                ShellAction::Redraw
            }
            ShellCommand::ClockCreateSetFocus(focus) => {
                self.set_clock_create_focus(focus);
                ShellAction::Redraw
            }
            ShellCommand::ClockCreateAppend(character) => {
                self.append_clock_create_char(character);
                ShellAction::Redraw
            }
            ShellCommand::ClockCreateBackspace => {
                self.clock_create_backspace();
                ShellAction::Redraw
            }
            ShellCommand::ClockCreateAlarm => {
                self.create_clock_entry(ScheduledClockEntryKind::DailyAlarm);
                ShellAction::Redraw
            }
            ShellCommand::ClockCreateCountdown => {
                self.create_clock_entry(ScheduledClockEntryKind::Countdown);
                ShellAction::Redraw
            }
            ShellCommand::ClockSelectPrevious => {
                self.select_clock_entry_delta(-1);
                ShellAction::Redraw
            }
            ShellCommand::ClockSelectNext => {
                self.select_clock_entry_delta(1);
                ShellAction::Redraw
            }
            ShellCommand::ClockSelectPageUp => {
                let page = self.clock_entry_capacity_at(Instant::now()) as isize;
                self.select_clock_entry_delta(-page.max(1));
                ShellAction::Redraw
            }
            ShellCommand::ClockSelectPageDown => {
                let page = self.clock_entry_capacity_at(Instant::now()) as isize;
                self.select_clock_entry_delta(page.max(1));
                ShellAction::Redraw
            }
            ShellCommand::ClockSelectFirst => {
                self.select_clock_entry_edge(false);
                ShellAction::Redraw
            }
            ShellCommand::ClockSelectLast => {
                self.select_clock_entry_edge(true);
                ShellAction::Redraw
            }
            ShellCommand::ClockSelectEntry(id) => {
                self.select_clock_entry(id);
                ShellAction::Redraw
            }
            ShellCommand::ClockActivateSelected => {
                if let Some(id) = self.clock_selected_entry_id {
                    self.show_clock_manage_dialog(id);
                }
                ShellAction::Redraw
            }
            ShellCommand::ClockManageEntry(id) => {
                self.select_clock_entry(id);
                self.show_clock_manage_dialog(id);
                ShellAction::Redraw
            }
            ShellCommand::ClockDeleteEntry(id) => {
                self.delete_clock_entry(id);
                ShellAction::Redraw
            }
            ShellCommand::ClockToggleStrong(id) => {
                self.toggle_clock_entry_strong(id);
                ShellAction::Redraw
            }
            ShellCommand::ClockSnoozeFiveMinutes(id) => {
                self.snooze_clock_alarm(id);
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
            ShellCommand::UserManagementPageUp => {
                self.select_user_management_page(-1);
                ShellAction::Redraw
            }
            ShellCommand::UserManagementPageDown => {
                self.select_user_management_page(1);
                ShellAction::Redraw
            }
            ShellCommand::UserManagementFirst => {
                self.select_user_management_edge(false);
                ShellAction::Redraw
            }
            ShellCommand::UserManagementLast => {
                self.select_user_management_edge(true);
                ShellAction::Redraw
            }
            ShellCommand::UserManagementSelectRow(index) => {
                if index < self.user_management_users.len() {
                    self.user_management_selected = index;
                    self.user_management_focus = UserManagementPageFocus::UserList;
                    self.ensure_user_management_selection_visible();
                }
                ShellAction::Redraw
            }
            ShellCommand::UserManagementFocusAction(action) => {
                self.focus_user_management_action(action);
                ShellAction::Redraw
            }
            ShellCommand::UserManagementActivateFocused => {
                self.activate_focused_user_management_control();
                self.refresh_hit_map();
                ShellAction::Redraw
            }
            ShellCommand::UserManagementActivateAction(action) => {
                self.focus_user_management_action(action);
                self.activate_user_management_action(action);
                self.refresh_hit_map();
                ShellAction::Redraw
            }
            ShellCommand::UserManagementSetFormFocus(field) => {
                self.set_user_management_form_focus(field);
                ShellAction::Redraw
            }
            ShellCommand::UserManagementActivateFormControl(field) => {
                self.set_user_management_form_focus(field);
                match field {
                    tundra_ui::UserManagementField::Role => {
                        self.toggle_user_management_form_role();
                    }
                    tundra_ui::UserManagementField::Submit => {
                        self.submit_user_management_form();
                    }
                    tundra_ui::UserManagementField::Cancel => {
                        self.cancel_user_management_form();
                    }
                    tundra_ui::UserManagementField::Username
                    | tundra_ui::UserManagementField::DisplayName
                    | tundra_ui::UserManagementField::Password => {}
                }
                self.refresh_hit_map();
                ShellAction::Redraw
            }
            ShellCommand::UserManagementToggleFormRole => {
                self.toggle_user_management_form_role();
                ShellAction::Redraw
            }
            ShellCommand::CreateManagedUser => {
                self.begin_create_managed_user();
                self.refresh_hit_map();
                ShellAction::Redraw
            }
            ShellCommand::EditManagedUserInfo => {
                self.begin_edit_selected_user_info();
                self.refresh_hit_map();
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
                self.refresh_hit_map();
                ShellAction::Redraw
            }
            ShellCommand::CycleManagedRole => {
                self.cycle_selected_role();
                self.normalize_user_management_focus();
                ShellAction::Redraw
            }
            ShellCommand::RequestDeleteManagedUser => {
                self.request_delete_selected_user();
                ShellAction::Redraw
            }
            ShellCommand::DeleteManagedUser => {
                self.delete_selected_user();
                self.normalize_user_management_focus();
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
                if self.user_management_mode == UserManagementMode::Browse {
                    self.move_user_management_page_focus(1);
                } else {
                    self.move_user_management_form_focus(1);
                }
                ShellAction::Redraw
            }
            ShellCommand::UserManagementFocusPrevious => {
                if self.user_management_mode == UserManagementMode::Browse {
                    self.move_user_management_page_focus(-1);
                } else {
                    self.move_user_management_form_focus(-1);
                }
                ShellAction::Redraw
            }
            ShellCommand::SubmitUserManagementForm => {
                self.submit_user_management_form();
                self.refresh_hit_map();
                ShellAction::Redraw
            }
            ShellCommand::CancelUserManagementForm => {
                self.cancel_user_management_form();
                self.refresh_hit_map();
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
                self.notify_status(format!("{} activated by {click_label}", target.label()));
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
                let status = match target {
                    Some(target) => format!("Context menu: {}", target.label()),
                    None => "Context menu".to_string(),
                };
                self.notify_status(status);
                self.refresh_hit_map();
                ShellAction::Redraw
            }
            ShellCommand::ClosePopup => {
                self.active_popup = None;
                self.notify_status("Ready");
                self.refresh_hit_map();
                ShellAction::Redraw
            }
            ShellCommand::CloseTimeSyncDialog => {
                self.close_time_sync_dialog();
                ShellAction::Redraw
            }
            ShellCommand::NotificationNextAction => {
                self.notifications.select_next_action();
                ShellAction::Redraw
            }
            ShellCommand::NotificationPreviousAction => {
                self.notifications.select_previous_action();
                ShellAction::Redraw
            }
            ShellCommand::NotificationActivateSelected => self.activate_notification_selected(),
            ShellCommand::NotificationActivateAction(index) => {
                self.activate_notification_action(index)
            }
            ShellCommand::NotificationCancel => {
                if let Some(index) = self.notifications.explicit_cancel_action_index() {
                    self.activate_notification_action(index)
                } else if !self.notification_can_render()
                    && self.notifications.dismiss_active_modal_without_response()
                {
                    self.apply_notification_follow_up(None)
                } else if let Some(index) = self.notifications.cancel_action_index() {
                    self.activate_notification_action(index)
                } else {
                    ShellAction::Redraw
                }
            }
            ShellCommand::CaptureOverlayInput => ShellAction::Redraw,
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
        self.notifications.status()
    }

    pub fn notify_status(&mut self, message: impl Into<String>) {
        self.notifications.notify_status(message);
    }

    pub fn notify_toast(&mut self, message: impl Into<String>) {
        self.notifications.notify_toast(message);
    }

    pub fn notify_alert(&mut self, message: impl Into<String>) {
        self.notifications
            .notify_alert(message, tundra_ui::NotificationTone::Warning);
    }

    pub fn notify_alert_with_tone(
        &mut self,
        message: impl Into<String>,
        tone: tundra_ui::NotificationTone,
    ) {
        self.notifications.notify_alert(message, tone);
    }

    pub fn notify_alert_with_key(
        &mut self,
        key: impl Into<String>,
        message: impl Into<String>,
        tone: tundra_ui::NotificationTone,
    ) {
        self.notifications.notify_alert_with_key(key, message, tone);
    }

    pub fn resolve_notification_alert(&mut self, key: &str) {
        self.notifications.resolve_alert(key);
    }

    pub fn clear_notification_alert(&mut self) {
        self.notifications.clear_alert();
    }

    pub fn notify_modal(
        &mut self,
        title: impl Into<String>,
        message: impl Into<String>,
        tone: tundra_ui::NotificationTone,
        actions: Vec<ShellNotificationAction>,
    ) -> u64 {
        self.notify_modal_with_options(
            ShellNotification::modal(title, message, tone, actions)
                .with_component(ShellComponent::NotificationDialog),
        )
    }

    pub fn take_notification_response(&mut self) -> Option<ShellNotificationResponse> {
        self.notifications.take_response()
    }

    pub fn to_notification_view_model(&self) -> Option<tundra_ui::NotificationViewModel> {
        self.notifications.active_modal_view_model()
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
        self.time_sync_attempted = true;
        match result {
            Ok(utc) => self.apply_time_sync_success_utc(utc),
            Err(error) => {
                self.network_clock.apply_sync(Err(error));
                self.show_time_sync_failure_dialog("联网校准时间失败".to_string());
            }
        }
        self.restore_clock_profile_after_initial_sync();
    }

    #[doc(hidden)]
    pub fn apply_time_sync_utc_for_test(&mut self, utc: DateTime<Utc>) {
        self.time_sync_attempted = true;
        self.apply_time_sync_success_utc(utc);
        self.restore_clock_profile_after_initial_sync();
    }

    #[doc(hidden)]
    pub fn apply_time_sync_failure_for_test(&mut self, message: &str) {
        self.time_sync_attempted = true;
        self.last_time_sync_utc = None;
        self.network_clock = ShellNetworkClock::new(self.clock_timezone_id.clone());
        self.show_time_sync_failure_dialog(message.to_string());
        self.restore_clock_profile_after_initial_sync();
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

    fn capture_modal_focus_context(&mut self) {
        if self.modal_focus_context.is_none() && !self.notifications.has_active_modal() {
            self.modal_focus_context = Some(ModalFocusContext {
                screen: self.active_screen(),
                component: self.focused_component,
            });
            self.modal_focus_prepared_for_follow_up = false;
        }
    }

    fn notify_modal_with_options(&mut self, notification: ShellNotification) -> u64 {
        self.capture_modal_focus_context();
        if !self.notifications.has_active_modal() {
            self.modal_focus_prepared_for_follow_up = false;
        }
        let id = self.notifications.push_modal(notification);
        self.active_popup = None;
        self.notification_pointer_capture = None;
        if let Some(component) = self.notifications.active_modal_component() {
            self.focused_component = component;
        }
        self.refresh_hit_map();
        id
    }

    fn activate_notification_selected(&mut self) -> ShellAction {
        self.notification_pointer_capture = None;
        let follow_up = self.notifications.activate_selected_action();
        self.apply_notification_follow_up(follow_up)
    }

    fn activate_notification_action(&mut self, index: usize) -> ShellAction {
        self.notification_pointer_capture = None;
        let follow_up = self.notifications.activate_action(index);
        self.apply_notification_follow_up(follow_up)
    }

    fn apply_notification_follow_up(&mut self, follow_up: Option<ShellCommand>) -> ShellAction {
        if let Some(component) = self.notifications.active_modal_component() {
            self.focused_component = component;
            self.refresh_hit_map();
        } else {
            self.prepare_modal_focus_for_follow_up();
        }

        if let Some(command) = follow_up {
            self.pending_notification_commands.push_back(command);
        }
        ShellAction::Redraw
    }

    fn prepare_modal_focus_for_follow_up(&mut self) {
        if self.modal_focus_prepared_for_follow_up {
            return;
        }
        let Some(context) = self.modal_focus_context else {
            return;
        };
        if self.active_screen() != context.screen {
            return;
        }

        self.focused_component = context.component;
        if let Some(field) = setup_field_for_component(context.component) {
            self.setup_focused_field = field;
        }
        self.modal_focus_prepared_for_follow_up = true;
    }

    fn finish_modal_focus_transition(&mut self) {
        if let Some(component) = self.notifications.active_modal_component() {
            self.focused_component = component;
            self.refresh_hit_map();
            return;
        }

        self.notification_pointer_capture = None;
        let Some(context) = self.modal_focus_context.take() else {
            self.modal_focus_prepared_for_follow_up = false;
            return;
        };
        let focus_was_prepared = self.modal_focus_prepared_for_follow_up;
        self.modal_focus_prepared_for_follow_up = false;
        if self.active_screen() == context.screen && !focus_was_prepared {
            self.focused_component = context.component;
            if let Some(field) = setup_field_for_component(context.component) {
                self.setup_focused_field = field;
            }
        }
        self.refresh_hit_map();
    }

    fn apply_time_sync_success_utc(&mut self, utc: DateTime<Utc>) {
        self.last_time_sync_utc = Some(utc);
        self.network_clock.apply_sync(Ok(utc));

        if self.clock_scheduler.is_some() && self.auth_session.is_some() {
            let snapshot = self.network_clock.snapshot();
            match self.persist_clock_scheduler_at(&snapshot, Instant::now()) {
                Ok(()) => {
                    self.clock_persist_pending = false;
                    self.clock_pending_due_summary = None;
                    self.notifications.resolve_alert(CLOCK_STORAGE_ALERT_KEY);
                }
                Err(error) => {
                    self.clock_persist_pending = true;
                    self.report_clock_storage_error(error);
                }
            }
        }

        if self.time_sync_dialog_visible {
            self.time_sync_dialog_visible = false;
            self.time_sync_failure_message = None;
            self.notifications
                .dismiss_modal_by_key(TIME_SYNC_NOTIFICATION_KEY);
            self.notify_status("Ready");
        }

        self.finish_modal_focus_transition();
        if self.modal_focus_context.is_none() {
            self.refresh_hit_map();
        }
    }

    fn show_time_sync_failure_dialog(&mut self, message: String) {
        self.time_sync_dialog_visible = true;
        self.time_sync_failure_message = Some(message.clone());
        self.active_popup = None;
        self.notify_status(message.clone());
        self.notify_modal_with_options(
            ShellNotification::modal(
                "Time Sync",
                message,
                tundra_ui::NotificationTone::Error,
                vec![
                    ShellNotificationAction::new("ok", "OK")
                        .with_shortcut(InputKey::Escape)
                        .cancel()
                        .with_follow_up(ShellCommand::CloseTimeSyncDialog),
                ],
            )
            .with_key(TIME_SYNC_NOTIFICATION_KEY)
            .with_component(ShellComponent::TimeSyncDialog),
        );
        self.refresh_hit_map();
    }

    fn close_time_sync_dialog(&mut self) {
        self.time_sync_dialog_visible = false;
        self.time_sync_failure_message = None;
        self.notifications
            .dismiss_modal_by_key(TIME_SYNC_NOTIFICATION_KEY);
        self.notify_status("Ready");
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

        if self.notifications.has_active_modal() {
            return self.route_notification_key(key);
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
        let area = Rect::new(0, 0, self.terminal_size.0, self.terminal_size.1);
        if matches!(
            tundra_ui::compute_shell_layout(area),
            tundra_ui::ShellLayout::Compact(_)
        ) {
            return match &key.key {
                InputKey::Escape if self.clock_create_state.is_some() => (
                    RoutedTarget::Modal(ShellComponent::ClockCreateDialog),
                    ShellCommand::ClockCloseCreate,
                ),
                InputKey::Escape => (RoutedTarget::Global, ShellCommand::CloseClock),
                _ => (
                    RoutedTarget::Component(ShellComponent::CompactHome),
                    ShellCommand::CaptureOverlayInput,
                ),
            };
        }

        if let Some(create) = &self.clock_create_state {
            let target = RoutedTarget::Modal(ShellComponent::ClockCreateDialog);
            return match &key.key {
                InputKey::Escape => (target, ShellCommand::ClockCloseCreate),
                InputKey::BackTab => (target, ShellCommand::ClockCreateFocusPrevious),
                InputKey::Tab if key.modifiers.shift => {
                    (target, ShellCommand::ClockCreateFocusPrevious)
                }
                InputKey::Tab => (target, ShellCommand::ClockCreateFocusNext),
                InputKey::Up | InputKey::Left => (target, ShellCommand::ClockCreateFocusPrevious),
                InputKey::Down | InputKey::Right => (target, ShellCommand::ClockCreateFocusNext),
                InputKey::Enter => match create.focus {
                    tundra_ui::ClockCreateDialogFocus::Input => {
                        (target, ShellCommand::ClockCreateFocusNext)
                    }
                    tundra_ui::ClockCreateDialogFocus::CreateAlarm => {
                        (target, ShellCommand::ClockCreateAlarm)
                    }
                    tundra_ui::ClockCreateDialogFocus::CreateCountdown => {
                        (target, ShellCommand::ClockCreateCountdown)
                    }
                },
                InputKey::Character(' ')
                    if create.focus == tundra_ui::ClockCreateDialogFocus::CreateAlarm =>
                {
                    (target, ShellCommand::ClockCreateAlarm)
                }
                InputKey::Character(' ')
                    if create.focus == tundra_ui::ClockCreateDialogFocus::CreateCountdown =>
                {
                    (target, ShellCommand::ClockCreateCountdown)
                }
                InputKey::Backspace if create.focus == tundra_ui::ClockCreateDialogFocus::Input => {
                    (target, ShellCommand::ClockCreateBackspace)
                }
                InputKey::Character(character)
                    if create.focus == tundra_ui::ClockCreateDialogFocus::Input =>
                {
                    (target, ShellCommand::ClockCreateAppend(*character))
                }
                _ => (target, ShellCommand::CaptureOverlayInput),
            };
        }

        let target = RoutedTarget::Component(self.focused_component);
        match &key.key {
            InputKey::Escape => (RoutedTarget::Global, ShellCommand::CloseClock),
            InputKey::BackTab => (target, ShellCommand::FocusPrevious),
            InputKey::Tab if key.modifiers.shift => (target, ShellCommand::FocusPrevious),
            InputKey::Tab => (target, ShellCommand::FocusNext),
            InputKey::Character('n' | 'N') => (target, ShellCommand::ClockOpenCreate),
            InputKey::Enter | InputKey::Character(' ')
                if self.focused_component == ShellComponent::ClockNewButton =>
            {
                (target, ShellCommand::ClockOpenCreate)
            }
            InputKey::Enter | InputKey::Character(' ')
                if self.focused_component == ShellComponent::ClockEntryList =>
            {
                (target, ShellCommand::ClockActivateSelected)
            }
            InputKey::Up if self.focused_component == ShellComponent::ClockEntryList => {
                (target, ShellCommand::ClockSelectPrevious)
            }
            InputKey::Down if self.focused_component == ShellComponent::ClockEntryList => {
                (target, ShellCommand::ClockSelectNext)
            }
            InputKey::PageUp if self.focused_component == ShellComponent::ClockEntryList => {
                (target, ShellCommand::ClockSelectPageUp)
            }
            InputKey::PageDown if self.focused_component == ShellComponent::ClockEntryList => {
                (target, ShellCommand::ClockSelectPageDown)
            }
            InputKey::Home if self.focused_component == ShellComponent::ClockEntryList => {
                (target, ShellCommand::ClockSelectFirst)
            }
            InputKey::End if self.focused_component == ShellComponent::ClockEntryList => {
                (target, ShellCommand::ClockSelectLast)
            }
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

    fn route_notification_key(&self, key: &KeyInput) -> (RoutedTarget, ShellCommand) {
        let target_component = self
            .notifications
            .active_modal_component()
            .unwrap_or(ShellComponent::NotificationDialog);
        let target = RoutedTarget::Modal(target_component);

        if !self.notification_can_render() {
            return if key.phase == InputPhase::Press
                && key.is_unmodified_action_key()
                && matches!(key.key, InputKey::Escape)
            {
                (target, ShellCommand::NotificationCancel)
            } else {
                (target, ShellCommand::CaptureOverlayInput)
            };
        }

        if let Some(index) = self.notifications.action_index_for_input(key) {
            return (target, ShellCommand::NotificationActivateAction(index));
        }

        match &key.key {
            InputKey::BackTab if !key.has_non_shift_modifier() => {
                (target, ShellCommand::NotificationPreviousAction)
            }
            InputKey::Tab if key.modifiers.shift && !key.has_non_shift_modifier() => {
                (target, ShellCommand::NotificationPreviousAction)
            }
            InputKey::Tab if !key.modifiers.shift && !key.has_non_shift_modifier() => {
                (target, ShellCommand::NotificationNextAction)
            }
            InputKey::Right | InputKey::Down if key.is_unmodified_action_key() => {
                (target, ShellCommand::NotificationNextAction)
            }
            InputKey::Left | InputKey::Up if key.is_unmodified_action_key() => {
                (target, ShellCommand::NotificationPreviousAction)
            }
            InputKey::Enter | InputKey::Character(' ') => {
                if key.phase == InputPhase::Press && key.is_unmodified_action_key() {
                    (target, ShellCommand::NotificationActivateSelected)
                } else {
                    (target, ShellCommand::CaptureOverlayInput)
                }
            }
            InputKey::Escape => {
                if key.phase == InputPhase::Press && key.is_unmodified_action_key() {
                    (target, ShellCommand::NotificationCancel)
                } else {
                    (target, ShellCommand::CaptureOverlayInput)
                }
            }
            _ => (target, ShellCommand::CaptureOverlayInput),
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
            if key.phase != InputPhase::Press || key.has_non_shift_modifier() {
                return (target, ShellCommand::CaptureOverlayInput);
            }
            return match &key.key {
                InputKey::Enter if key.is_unmodified_action_key() => {
                    (target, ShellCommand::ExplorerConfirmDelete)
                }
                InputKey::Escape if key.is_unmodified_action_key() => {
                    (target, ShellCommand::CancelExplorerInput)
                }
                InputKey::Character('y' | 'Y') => (target, ShellCommand::ExplorerConfirmDelete),
                InputKey::Character('n' | 'N') => (target, ShellCommand::CancelExplorerInput),
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
            InputKey::Character('h' | 'H') => (target, ShellCommand::ExplorerToggleHidden),
            InputKey::Character('c' | 'C') => (target, ShellCommand::ExplorerCopy),
            InputKey::Character('x' | 'X') => (target, ShellCommand::ExplorerCut),
            InputKey::Character('v' | 'V') => (target, ShellCommand::ExplorerPaste),
            InputKey::Character('d' | 'D') => (target, ShellCommand::ExplorerDelete),
            InputKey::Character('n' | 'N' | 'f' | 'F') => {
                (target, ShellCommand::BeginExplorerNewFolder)
            }
            InputKey::Character('t' | 'T') => (target, ShellCommand::BeginExplorerNewTextFile),
            InputKey::Character('r' | 'R') => (target, ShellCommand::BeginExplorerRename),
            InputKey::Character('/') => (target, ShellCommand::BeginExplorerSearch),
            _ => (target, ShellCommand::RecordInput),
        }
    }

    fn route_user_management_key(&self, key: &KeyInput) -> (RoutedTarget, ShellCommand) {
        let target = RoutedTarget::Component(ShellComponent::UserManagement);
        let area = Rect::new(0, 0, self.terminal_size.0, self.terminal_size.1);
        if matches!(
            tundra_ui::compute_shell_layout(area),
            tundra_ui::ShellLayout::Compact(_)
        ) {
            return match &key.key {
                InputKey::Escape => (RoutedTarget::Global, ShellCommand::CloseUserManagement),
                _ => (
                    RoutedTarget::Component(ShellComponent::CompactHome),
                    ShellCommand::CaptureOverlayInput,
                ),
            };
        }

        if self.user_management_mode != UserManagementMode::Browse {
            let field = self.user_management_form_field();
            return match &key.key {
                InputKey::Escape => (target, ShellCommand::CancelUserManagementForm),
                InputKey::BackTab => (target, ShellCommand::UserManagementFocusPrevious),
                InputKey::Tab if key.modifiers.shift => {
                    (target, ShellCommand::UserManagementFocusPrevious)
                }
                InputKey::Tab | InputKey::Down => (target, ShellCommand::UserManagementFocusNext),
                InputKey::Up => (target, ShellCommand::UserManagementFocusPrevious),
                InputKey::Left | InputKey::Right
                    if field == Some(UserManagementFormField::Role) =>
                {
                    (target, ShellCommand::UserManagementToggleFormRole)
                }
                InputKey::Enter | InputKey::Character(' ')
                    if field == Some(UserManagementFormField::Role) =>
                {
                    (target, ShellCommand::UserManagementToggleFormRole)
                }
                InputKey::Enter | InputKey::Character(' ')
                    if field == Some(UserManagementFormField::Cancel) =>
                {
                    (target, ShellCommand::CancelUserManagementForm)
                }
                InputKey::Enter
                    if field == Some(UserManagementFormField::Submit)
                        || matches!(
                            field,
                            Some(
                                UserManagementFormField::Username
                                    | UserManagementFormField::DisplayName
                                    | UserManagementFormField::Password
                            )
                        ) =>
                {
                    (target, ShellCommand::SubmitUserManagementForm)
                }
                InputKey::Character(' ') if field == Some(UserManagementFormField::Submit) => {
                    (target, ShellCommand::SubmitUserManagementForm)
                }
                InputKey::Backspace => (target, ShellCommand::UserManagementBackspace),
                InputKey::Character(character)
                    if matches!(character, 'c' | 'C')
                        && field == Some(UserManagementFormField::Role) =>
                {
                    (target, ShellCommand::UserManagementToggleFormRole)
                }
                InputKey::Character(character) => {
                    (target, ShellCommand::AppendUserManagementChar(*character))
                }
                _ => (target, ShellCommand::RecordInput),
            };
        }

        use tundra_ui::UserManagementAction;
        match &key.key {
            InputKey::Escape => (RoutedTarget::Global, ShellCommand::CloseUserManagement),
            InputKey::BackTab => (target, ShellCommand::UserManagementFocusPrevious),
            InputKey::Tab if key.modifiers.shift => {
                (target, ShellCommand::UserManagementFocusPrevious)
            }
            InputKey::Tab => (target, ShellCommand::UserManagementFocusNext),
            InputKey::Up => (target, ShellCommand::UserManagementPrevious),
            InputKey::Down => (target, ShellCommand::UserManagementNext),
            InputKey::PageUp => (target, ShellCommand::UserManagementPageUp),
            InputKey::PageDown => (target, ShellCommand::UserManagementPageDown),
            InputKey::Home => (target, ShellCommand::UserManagementFirst),
            InputKey::End => (target, ShellCommand::UserManagementLast),
            InputKey::Enter | InputKey::Character(' ') => {
                (target, ShellCommand::UserManagementActivateFocused)
            }
            InputKey::Character('n') | InputKey::Character('N') if self.can_manage_all_users() => (
                target,
                ShellCommand::UserManagementActivateAction(UserManagementAction::NewUser),
            ),
            InputKey::Character('e') | InputKey::Character('E') => (
                target,
                ShellCommand::UserManagementActivateAction(UserManagementAction::EditInfo),
            ),
            InputKey::Character('d') | InputKey::Character('D')
                if self
                    .user_management_users
                    .get(self.user_management_selected)
                    .is_some_and(|user| user.enabled && !user_is_locked(user)) =>
            {
                (
                    target,
                    ShellCommand::UserManagementActivateAction(UserManagementAction::ToggleEnabled),
                )
            }
            InputKey::Character('u') | InputKey::Character('U')
                if self
                    .user_management_users
                    .get(self.user_management_selected)
                    .is_some_and(|user| !user.enabled || user_is_locked(user)) =>
            {
                (
                    target,
                    ShellCommand::UserManagementActivateAction(UserManagementAction::ToggleEnabled),
                )
            }
            InputKey::Character('r') | InputKey::Character('R') => (
                target,
                ShellCommand::UserManagementActivateAction(UserManagementAction::SetPassword),
            ),
            InputKey::Character('c') | InputKey::Character('C') if self.can_manage_all_users() => (
                target,
                ShellCommand::UserManagementActivateAction(UserManagementAction::ToggleRole),
            ),
            InputKey::Character('x') | InputKey::Character('X') | InputKey::Delete => (
                target,
                ShellCommand::UserManagementActivateAction(UserManagementAction::Delete),
            ),
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

        if self.notifications.has_active_modal() {
            return self.route_notification_mouse(mouse, hit_target);
        }

        if self.time_sync_dialog_visible {
            return self.route_time_sync_dialog_mouse(mouse, hit_target);
        }

        if self.active_screen() == ShellScreen::ExitConfirm {
            return (
                RoutedTarget::Modal(ShellComponent::ExitDialog),
                ShellCommand::CaptureOverlayInput,
            );
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

        if self.active_screen() == ShellScreen::Clock {
            return self.route_clock_mouse(mouse, hit_target);
        }

        if self.active_screen() == ShellScreen::UserManagement {
            return self.route_user_management_mouse(mouse, hit_target);
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

    fn route_clock_mouse(
        &mut self,
        mouse: MouseInput,
        hit_target: Option<ShellComponent>,
    ) -> (RoutedTarget, ShellCommand) {
        let coordinates = mouse.coordinates();
        let modal_target = RoutedTarget::Modal(ShellComponent::ClockCreateDialog);

        if self.clock_create_state.is_some() {
            return match mouse {
                MouseInput::Moved { .. } => (modal_target, ShellCommand::Hover(hit_target)),
                MouseInput::Down {
                    button: PointerButton::Left,
                    ..
                } => match hit_target {
                    Some(ShellComponent::ClockCreateInput) => (
                        modal_target,
                        ShellCommand::ClockCreateSetFocus(tundra_ui::ClockCreateDialogFocus::Input),
                    ),
                    Some(ShellComponent::ClockCreateAlarmButton) => {
                        (modal_target, ShellCommand::ClockCreateAlarm)
                    }
                    Some(ShellComponent::ClockCreateCountdownButton) => {
                        (modal_target, ShellCommand::ClockCreateCountdown)
                    }
                    _ => (modal_target, ShellCommand::CaptureOverlayInput),
                },
                _ => (modal_target, ShellCommand::CaptureOverlayInput),
            };
        }

        let target = target_route(hit_target);
        match mouse {
            MouseInput::Moved { .. } => (target, ShellCommand::Hover(hit_target)),
            MouseInput::Scroll {
                direction: ScrollDirection::Up,
                ..
            } if hit_target == Some(ShellComponent::ClockEntryList) => {
                (target, ShellCommand::ClockSelectPrevious)
            }
            MouseInput::Scroll {
                direction: ScrollDirection::Down,
                ..
            } if hit_target == Some(ShellComponent::ClockEntryList) => {
                (target, ShellCommand::ClockSelectNext)
            }
            MouseInput::Down {
                button: PointerButton::Left,
                ..
            } => match hit_target {
                Some(ShellComponent::ClockButton) => (target, ShellCommand::CloseClock),
                Some(ShellComponent::ClockNewButton) => (target, ShellCommand::ClockOpenCreate),
                Some(ShellComponent::ClockEntryList) => self
                    .clock_entry_id_at(coordinates)
                    .map(|id| (target, ShellCommand::ClockManageEntry(id)))
                    .unwrap_or((target, ShellCommand::RecordInput)),
                _ => (target, ShellCommand::RecordInput),
            },
            MouseInput::Down {
                button: PointerButton::Right,
                ..
            } => (target, ShellCommand::CaptureOverlayInput),
            _ => (target, ShellCommand::RecordInput),
        }
    }

    fn route_user_management_mouse(
        &mut self,
        mouse: MouseInput,
        hit_target: Option<ShellComponent>,
    ) -> (RoutedTarget, ShellCommand) {
        let target = RoutedTarget::Component(ShellComponent::UserManagement);
        let area = Rect::new(0, 0, self.terminal_size.0, self.terminal_size.1);
        if matches!(
            tundra_ui::compute_shell_layout(area),
            tundra_ui::ShellLayout::Compact(_)
        ) {
            return (
                RoutedTarget::Component(ShellComponent::CompactHome),
                ShellCommand::CaptureOverlayInput,
            );
        }

        let Some(layout) = self.user_management_layout() else {
            return (target, ShellCommand::CaptureOverlayInput);
        };
        let coordinates = mouse.coordinates();

        if self.user_management_mode != UserManagementMode::Browse {
            return match mouse {
                MouseInput::Moved { .. } => (target, ShellCommand::Hover(hit_target)),
                MouseInput::Down {
                    button: PointerButton::Left,
                    ..
                } => layout
                    .form_control_at(coordinates.0, coordinates.1)
                    .map(|field| {
                        let command = match field {
                            tundra_ui::UserManagementField::Role
                            | tundra_ui::UserManagementField::Submit
                            | tundra_ui::UserManagementField::Cancel => {
                                ShellCommand::UserManagementActivateFormControl(field)
                            }
                            _ => ShellCommand::UserManagementSetFormFocus(field),
                        };
                        (target, command)
                    })
                    .unwrap_or((target, ShellCommand::CaptureOverlayInput)),
                _ => (target, ShellCommand::CaptureOverlayInput),
            };
        }

        match mouse {
            MouseInput::Moved { .. } => (target, ShellCommand::Hover(hit_target)),
            MouseInput::Scroll {
                direction: ScrollDirection::Up,
                ..
            } if rect_contains(layout.rows_area, coordinates) => {
                (target, ShellCommand::UserManagementPrevious)
            }
            MouseInput::Scroll {
                direction: ScrollDirection::Down,
                ..
            } if rect_contains(layout.rows_area, coordinates) => {
                (target, ShellCommand::UserManagementNext)
            }
            MouseInput::Down {
                button: PointerButton::Left,
                ..
            } => {
                if let Some(index) = layout.row_index_at(coordinates.0, coordinates.1) {
                    return (target, ShellCommand::UserManagementSelectRow(index));
                }
                if let Some(action) = layout.action_at(coordinates.0, coordinates.1) {
                    return (target, ShellCommand::UserManagementActivateAction(action));
                }
                (target, ShellCommand::RecordInput)
            }
            _ => (target, ShellCommand::CaptureOverlayInput),
        }
    }

    fn clock_entry_id_at(&self, coordinates: CellPosition) -> Option<u64> {
        let (width, height) = self.terminal_size;
        let area = Rect::new(0, 0, width, height);
        let tundra_ui::ShellLayout::Full { main, .. } = tundra_ui::compute_shell_layout(area)
        else {
            return None;
        };
        let snapshot = self.network_clock.snapshot();
        let model = self.to_clock_view_model_at(&snapshot, Instant::now());
        tundra_ui::clock_page_layout(main, &model)
            .entry_rows
            .into_iter()
            .find(|row| rect_contains(row.area, coordinates))
            .map(|row| row.id)
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

    fn route_notification_mouse(
        &mut self,
        mouse: MouseInput,
        hit_target: Option<ShellComponent>,
    ) -> (RoutedTarget, ShellCommand) {
        let target_component = self
            .notifications
            .active_modal_component()
            .unwrap_or(ShellComponent::NotificationDialog);
        let target = RoutedTarget::Modal(target_component);

        if !self.notification_can_render() {
            self.notification_pointer_capture = None;
            return (target, ShellCommand::CaptureOverlayInput);
        }

        match mouse {
            MouseInput::Moved { .. } => (target, ShellCommand::Hover(hit_target)),
            MouseInput::Down {
                button: PointerButton::Left,
                ..
            } => {
                let action_index = self.notification_action_index_at(mouse.coordinates());
                self.notification_pointer_capture = action_index.and_then(|action_index| {
                    self.notifications.active_modal_id().map(|notification_id| {
                        NotificationPointerCapture {
                            notification_id,
                            action_index,
                        }
                    })
                });
                if let Some(action_index) = action_index {
                    self.notifications.select_action(action_index);
                }
                (target, ShellCommand::CaptureOverlayInput)
            }
            MouseInput::Up {
                button: PointerButton::Left,
                ..
            } => {
                let pressed = self.notification_pointer_capture.take();
                let released_index = self.notification_action_index_at(mouse.coordinates());
                let current_id = self.notifications.active_modal_id();
                match (pressed, current_id, released_index) {
                    (Some(pressed), Some(current_id), Some(released_index))
                        if pressed.notification_id == current_id
                            && pressed.action_index == released_index =>
                    {
                        (
                            target,
                            ShellCommand::NotificationActivateAction(released_index),
                        )
                    }
                    _ => (target, ShellCommand::CaptureOverlayInput),
                }
            }
            MouseInput::Drag {
                button: PointerButton::Left,
                ..
            } => {
                self.notification_pointer_capture = None;
                (target, ShellCommand::CaptureOverlayInput)
            }
            MouseInput::Down { .. }
            | MouseInput::Up { .. }
            | MouseInput::Drag { .. }
            | MouseInput::Scroll { .. } => {
                self.notification_pointer_capture = None;
                (target, ShellCommand::CaptureOverlayInput)
            }
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
                ) && let MouseInput::Down { button, .. } = *mouse
                {
                    summary = format!("Mouse DoubleClick {}", button.label());
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
        let notification_model = self.notifications.active_modal_view_model();
        let clock_model =
            (self.active_screen() == ShellScreen::Clock).then(|| self.to_clock_view_model());
        self.hit_map = build_shell_hit_map(
            self.terminal_size,
            self.active_screen(),
            self.active_popup,
            self.setup_step,
            self.hit_map_generation,
            time_button_label.as_deref(),
            self.time_sync_dialog_visible,
            self.notifications.active_modal_component(),
            notification_model.as_ref(),
            clock_model.as_ref(),
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
        if let Some(component) = self.notifications.active_modal_component() {
            return vec![component];
        }
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
            let area = Rect::new(0, 0, self.terminal_size.0, self.terminal_size.1);
            if matches!(
                tundra_ui::compute_shell_layout(area),
                tundra_ui::ShellLayout::Compact(_)
            ) {
                return vec![ShellComponent::CompactHome];
            }
            if self.clock_create_state.is_some() {
                return vec![
                    ShellComponent::ClockCreateInput,
                    ShellComponent::ClockCreateAlarmButton,
                    ShellComponent::ClockCreateCountdownButton,
                ];
            }
            let mut order = vec![ShellComponent::ClockNewButton];
            if !self.ordered_clock_entry_ids_at(Instant::now()).is_empty() {
                order.push(ShellComponent::ClockEntryList);
            }
            return order;
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

#[allow(clippy::too_many_arguments)]
fn build_shell_hit_map(
    terminal_size: CellPosition,
    active_screen: ShellScreen,
    active_popup: Option<ShellPopup>,
    setup_step: tundra_ui::SetupStep,
    generation: u64,
    time_button_label: Option<&str>,
    time_sync_dialog_visible: bool,
    notification_modal_component: Option<ShellComponent>,
    notification_model: Option<&tundra_ui::NotificationViewModel>,
    clock_model: Option<&tundra_ui::ClockViewModel>,
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
                    if let Some(model) = clock_model {
                        let layout = tundra_ui::clock_page_layout(main, model);
                        if layout.panel.width > 0 && layout.panel.height > 0 {
                            regions.push(ShellHitRegion {
                                component: ShellComponent::ClockEntryList,
                                area: layout.panel,
                            });
                        }
                        if layout.new_button.width > 0 && layout.new_button.height > 0 {
                            regions.push(ShellHitRegion {
                                component: ShellComponent::ClockNewButton,
                                area: layout.new_button,
                            });
                        }
                        regions.extend(layout.entry_rows.iter().map(|row| ShellHitRegion {
                            component: ShellComponent::ClockEntryList,
                            area: row.area,
                        }));
                        if let Some(dialog) = layout.create_dialog {
                            regions.push(ShellHitRegion {
                                component: ShellComponent::ClockCreateDialog,
                                area: dialog.dialog,
                            });
                            regions.push(ShellHitRegion {
                                component: ShellComponent::ClockCreateInput,
                                area: dialog.input,
                            });
                            regions.push(ShellHitRegion {
                                component: ShellComponent::ClockCreateAlarmButton,
                                area: dialog.create_alarm,
                            });
                            regions.push(ShellHitRegion {
                                component: ShellComponent::ClockCreateCountdownButton,
                                area: dialog.create_countdown,
                            });
                        }
                    }
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

    if let (Some(component), Some(model)) = (notification_modal_component, notification_model)
        && let tundra_ui::NotificationLayout::Dialog(layout) =
            tundra_ui::notification_layout(area, model)
    {
        regions.push(ShellHitRegion {
            component,
            area: layout.dialog,
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
            "At least one enabled admin is required".to_string()
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
        UserManagementFormField::Role => tundra_ui::UserManagementField::Role,
        UserManagementFormField::Password => tundra_ui::UserManagementField::Password,
        UserManagementFormField::Submit => tundra_ui::UserManagementField::Submit,
        UserManagementFormField::Cancel => tundra_ui::UserManagementField::Cancel,
    }
}

fn from_ui_user_management_field(field: tundra_ui::UserManagementField) -> UserManagementFormField {
    match field {
        tundra_ui::UserManagementField::Username => UserManagementFormField::Username,
        tundra_ui::UserManagementField::DisplayName => UserManagementFormField::DisplayName,
        tundra_ui::UserManagementField::Role => UserManagementFormField::Role,
        tundra_ui::UserManagementField::Password => UserManagementFormField::Password,
        tundra_ui::UserManagementField::Submit => UserManagementFormField::Submit,
        tundra_ui::UserManagementField::Cancel => UserManagementFormField::Cancel,
    }
}

fn user_management_action_model(
    action: tundra_ui::UserManagementAction,
    label: &str,
    shortcut: Option<char>,
    enabled: bool,
    disabled_reason: Option<String>,
    dangerous: bool,
) -> tundra_ui::UserManagementActionViewModel {
    tundra_ui::UserManagementActionViewModel {
        action,
        label: label.to_string(),
        shortcut,
        enabled,
        disabled_reason: (!enabled).then_some(disabled_reason).flatten(),
        dangerous,
    }
}

fn user_is_locked(user: &UserAccount) -> bool {
    user.locked_until_epoch_ms
        .is_some_and(|locked_until| locked_until > unix_millis())
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
        format!(
            "State data: users, state, recent-files, sessions, {} use versioned JSON",
            CLOCK_DESCRIPTOR.name
        ),
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
        let frame_now = Instant::now();
        let clock_snapshot = state.network_clock.snapshot();
        state.advance_clock_background_at(&clock_snapshot, frame_now);
        let chrome = state.to_shell_chrome_view_model();
        let home = state.to_home_view_model();
        let clock = state.to_clock_view_model_at(&clock_snapshot, frame_now);
        let time_sync_dialog = state.to_time_sync_dialog_view_model();
        let setup = state.to_setup_view_model();
        let login = state.to_login_view_model();
        let bootstrap_admin = state.to_bootstrap_admin_view_model();
        let user_management = state.to_user_management_view_model();
        let explorer = state.to_explorer_view_model();
        let notification = state.to_notification_view_model();
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
                    tundra_ui::render_clock(frame, area, &chrome, &clock, &theme);
                }
                ShellScreen::Home | ShellScreen::ExitConfirm => {
                    tundra_ui::render_home(frame, area, &chrome, &home, &theme);
                }
            }

            if notification.is_none() && active_screen == ShellScreen::ExitConfirm {
                tundra_ui::render_exit_confirmation(frame, area, &exit_confirmation, &theme);
            }
            if notification.is_none()
                && let Some(dialog) = time_sync_dialog.as_ref()
            {
                tundra_ui::render_time_sync_failure_dialog(frame, area, dialog, &theme);
            }
            if let Some(notification) = notification.as_ref() {
                tundra_ui::render_notification_overlay(frame, area, notification, &theme);
            }
        })?;

        if terminal_control.shutdown_requested() {
            state.apply_input_with_platform(InputEvent::Shutdown, platform.as_ref());
        }
        if state.shutdown_requested() {
            break;
        }

        let poll_timeout = state.notifications.poll_timeout(Instant::now(), tick_rate);
        let action = if event::poll(poll_timeout)? {
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
    fn notification_toast_expires_at_wall_clock_deadline() {
        let started_at = Instant::now();
        let mut notifications = NotificationCenter::new("Ready");

        notifications.notify_toast_at("Saved", started_at);
        assert_eq!(
            notifications.poll_timeout(started_at, Duration::from_millis(250)),
            Duration::from_millis(250)
        );
        assert_eq!(
            notifications.poll_timeout(
                started_at + DEFAULT_TOAST_DURATION - Duration::from_millis(100),
                Duration::from_millis(250),
            ),
            Duration::from_millis(100)
        );
        assert_eq!(
            notifications.poll_timeout(
                started_at + DEFAULT_TOAST_DURATION,
                Duration::from_millis(250),
            ),
            Duration::ZERO
        );
        notifications.expire(started_at + DEFAULT_TOAST_DURATION - Duration::from_millis(1));
        assert_eq!(notifications.toast().as_deref(), Some("Saved"));

        notifications.expire(started_at + DEFAULT_TOAST_DURATION);
        assert_eq!(notifications.toast(), None);

        let replacement_at = started_at + Duration::from_secs(10);
        notifications.notify_toast_at("First", replacement_at);
        notifications.notify_toast_at("Saved again", replacement_at + Duration::from_secs(3));
        notifications.expire(replacement_at + DEFAULT_TOAST_DURATION);
        assert_eq!(notifications.toast().as_deref(), Some("Saved again"));

        notifications.expire(replacement_at + Duration::from_secs(3) + DEFAULT_TOAST_DURATION);
        assert_eq!(notifications.toast(), None);
    }

    #[test]
    fn notification_toast_waits_behind_an_active_alert() {
        let started_at = Instant::now();
        let mut notifications = NotificationCenter::new("Ready");
        notifications.notify_alert_with_key(
            "storage",
            "Storage unavailable",
            tundra_ui::NotificationTone::Error,
        );
        notifications.notify_toast_at("Countdown finished", started_at);

        notifications.expire(started_at + DEFAULT_TOAST_DURATION + Duration::from_secs(1));

        assert_eq!(notifications.toast().as_deref(), Some("Countdown finished"));
        assert_eq!(
            notifications.poll_timeout(started_at, Duration::from_millis(250)),
            Duration::from_millis(250)
        );
        notifications.resolve_alert("storage");
        assert_eq!(notifications.toast().as_deref(), Some("Countdown finished"));
    }

    #[test]
    fn clock_storage_retry_keeps_the_due_summary_visible() {
        let mut state = ShellState::new(ShellLaunchConfig::default(), (80, 24));
        state.remember_clock_due_summary("Countdown finished".to_string());

        state.report_clock_storage_error("first failure");
        state.report_clock_storage_error("retry failure");

        assert!(
            state
                .to_shell_chrome_view_model()
                .status
                .error
                .as_deref()
                .is_some_and(|message| {
                    message.contains("Countdown finished") && message.contains("retry failure")
                })
        );
    }

    #[test]
    fn compact_clock_routes_only_escape_and_does_not_open_hidden_controls() {
        let mut state = ShellState::new(ShellLaunchConfig::default(), (49, 11));
        state.screen_stack = vec![ShellScreen::Clock];

        assert_eq!(
            state.route_clock_key(&KeyInput::from_label("n")).1,
            ShellCommand::CaptureOverlayInput
        );
        assert_eq!(state.focus_order(), vec![ShellComponent::CompactHome]);

        state.clock_create_state = Some(ClockCreateState::default());
        assert_eq!(
            state.route_clock_key(&KeyInput::from_label("Esc")).1,
            ShellCommand::ClockCloseCreate
        );
    }

    #[test]
    fn notification_alerts_resolve_by_key_and_preserve_other_sources() {
        let mut notifications = NotificationCenter::new("Ready");
        notifications.notify_alert_with_key(
            "settings",
            "Settings warning",
            tundra_ui::NotificationTone::Warning,
        );
        notifications.notify_alert_with_key(
            "explorer.operation",
            "Explorer failed",
            tundra_ui::NotificationTone::Error,
        );

        assert_eq!(notifications.alert().as_deref(), Some("Explorer failed"));
        assert_eq!(
            notifications.alert_tone(),
            Some(tundra_ui::NotificationTone::Error)
        );

        notifications.resolve_alert("explorer.operation");
        assert_eq!(notifications.alert().as_deref(), Some("Settings warning"));
        assert_eq!(
            notifications.alert_tone(),
            Some(tundra_ui::NotificationTone::Warning)
        );
    }

    #[test]
    fn notification_response_queue_is_bounded() {
        let mut notifications = NotificationCenter::new("Ready");
        let total = MAX_NOTIFICATION_RESPONSES + 5;

        for index in 0..total {
            notifications.push_modal(ShellNotification::modal(
                "Notice",
                "Continue?",
                tundra_ui::NotificationTone::Info,
                vec![ShellNotificationAction::new(format!("ok-{index}"), "OK")],
            ));
            let _follow_up = notifications.activate_selected_action();
        }

        assert_eq!(notifications.responses.len(), MAX_NOTIFICATION_RESPONSES);
        assert_eq!(
            notifications
                .responses
                .front()
                .map(|response| response.notification_id),
            Some(6)
        );
    }

    #[test]
    fn notification_follow_up_activation_is_iterative_and_bounded() {
        let mut state = ShellState::new(ShellLaunchConfig::default(), (80, 24));
        for index in 0..(MAX_NOTIFICATION_FOLLOW_UP_STEPS + 3) {
            state.notify_modal(
                format!("Notice {index}"),
                "Continue?",
                tundra_ui::NotificationTone::Info,
                vec![
                    ShellNotificationAction::new(format!("ok-{index}"), "OK")
                        .with_follow_up(ShellCommand::NotificationActivateSelected),
                ],
            );
        }

        let action = state.apply_input(InputEvent::from_key_label("Enter"));

        assert_eq!(action, ShellAction::Redraw);
        assert!(state.to_notification_view_model().is_some());
        assert_eq!(
            state.to_shell_chrome_view_model().status.error.as_deref(),
            Some("Notification follow-up limit reached")
        );
        assert_eq!(
            state.to_shell_chrome_view_model().status.alert_tone,
            tundra_ui::NotificationTone::Critical
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
