#[cfg(not(any(windows, target_os = "macos")))]
compile_error!("TundraUX3 phase 0 supports Windows and macOS only; Linux is unsupported.");

use tundra_storage::{CONFIG_DESCRIPTOR, SCHEMA_VERSION};

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
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub const BANNER_DISPLAY_DURATION: Duration = Duration::from_secs(2);
pub const ENTER_FULLSCREEN_SEQUENCE: &str = "\x1B[?1049h\x1B[?25l\x1B[2J\x1B[H";
pub const EXIT_FULLSCREEN_SEQUENCE: &str = "\x1B[?25h\x1B[?1049l";

static SHELL_RUNNING: AtomicBool = AtomicBool::new(true);
static PANIC_RESTORE_HOOK_INSTALLED: AtomicBool = AtomicBool::new(false);

const BANNER_LINES: &[&str] = &[
    r#"ooooooooooooo                               .o8                     ooooo     ooo ooooooo  ooooo   .oooo.   "#,
    r#"8'   888   `8                              "888                     `888'     `8'  `8888    d8'  .dP""Y88b  "#,
    r#"     888      oooo  oooo  ooo. .oo.    .oooo888  oooo d8b  .oooo.    888       8     Y888..8P          ]8P' "#,
    r#"     888      `888  `888  `888P"Y88b  d88' `888  `888""8P `P  )88b   888       8      `8888'         <88b.  "#,
    r#"     888       888   888   888   888  888   888   888      .oP"888   888       8     .8PY888.         `88b. "#,
    r#"     888       888   888   888   888  888   888   888     d8(  888   `88.    .8'    d8'  `888b   o.   .88P  "#,
    r#"    o888o      `V88V"V8P' o888o o888o `Y8bod88P" d888b    `Y888""8o    `YbodP'    o888o  o88888o `8bd88P'   "#,
    r#"                                                                                                            "#,
    r#"                                                                                                            "#,
    r#"                                                                                                            "#,
];

pub fn banner_lines() -> &'static [&'static str] {
    BANNER_LINES
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
    Home,
    ExitConfirm,
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
    StatusBar,
    ExitDialog,
    ContextMenu,
}

impl ShellComponent {
    const fn label(self) -> &'static str {
        match self {
            Self::CompactHome => "CompactHome",
            Self::TopBar => "TopBar",
            Self::Home => "Home",
            Self::StatusBar => "StatusBar",
            Self::ExitDialog => "ExitDialog",
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellState {
    home_mode: ShellHomeMode,
    screen_stack: Vec<ShellScreen>,
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
    last_click: Option<TimedClick>,
    drag_tracker: Option<DragTracker>,
}

impl ShellState {
    pub fn new(launch_config: ShellLaunchConfig, terminal_size: (u16, u16)) -> Self {
        let home_mode = match launch_config.home_mode_override {
            HomeModeOverride::Debug => ShellHomeMode::Debug,
            HomeModeOverride::BuildDefault => {
                if cfg!(debug_assertions) {
                    ShellHomeMode::Debug
                } else {
                    ShellHomeMode::User
                }
            }
        };

        let mut state = Self {
            home_mode,
            screen_stack: vec![ShellScreen::Home],
            terminal_size,
            terminal_flags: ShellTerminalFlags::enabled(),
            focused_component: ShellComponent::Home,
            hovered_component: None,
            active_popup: None,
            hit_map: ShellHitMap::empty(terminal_size),
            hit_map_generation: 0,
            tick_count: 0,
            status_message: "Ready".to_string(),
            toast_message: None,
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
            last_click: None,
            drag_tracker: None,
        };
        state.refresh_hit_map();
        state
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
                })
            }
            ShellHomeMode::User => {
                tundra_ui::HomeViewModel::user("Guest", current_time_label(), user_home_entries())
            }
        }
    }

    pub fn to_shell_chrome_view_model(&self) -> tundra_ui::ShellChromeViewModel {
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
                status: self.status_message.clone(),
                toast: self.toast_message.clone(),
                error: self.error_message.clone(),
            },
        }
    }

    pub fn apply_input(&mut self, input: InputEvent) -> ShellAction {
        let routed = self.route_input_at(input, Instant::now());
        self.apply_routed_event(routed)
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

    fn apply_routed_event(&mut self, routed: RoutedEvent) -> ShellAction {
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
                self.pop_to_home();
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
            ShellCommand::Hover(target) => {
                self.hovered_component = target;
                ShellAction::Redraw
            }
            ShellCommand::Activate {
                target,
                coordinates: _,
                click,
            } => {
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

    pub fn terminal_flags(&self) -> ShellTerminalFlags {
        self.terminal_flags
    }

    pub fn mouse_scroll_direction(&self) -> Option<&str> {
        self.mouse_scroll_direction.as_deref()
    }

    pub fn mouse_drag_direction(&self) -> Option<&str> {
        self.mouse_drag_direction.as_deref()
    }

    pub fn focused_component(&self) -> ShellComponent {
        self.focused_component
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
        match self.home_mode {
            ShellHomeMode::Debug => tundra_ui::HomeDisplayMode::Debug,
            ShellHomeMode::User => tundra_ui::HomeDisplayMode::User,
        }
    }

    fn route_key_input(&self, key: &KeyInput) -> (RoutedTarget, ShellCommand) {
        if !key.phase.is_press_like() {
            return (RoutedTarget::Global, ShellCommand::Noop);
        }

        if key.is_ctrl_c() {
            return (RoutedTarget::Global, ShellCommand::Shutdown);
        }

        if self.active_screen() == ShellScreen::ExitConfirm {
            return self.route_exit_confirm_key(key);
        }

        if self.active_popup.is_some() {
            return self.route_popup_key(key);
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
            ShellScreen::Home if key.is_character('q') || matches!(&key.key, InputKey::Escape) => {
                (RoutedTarget::Global, ShellCommand::RequestExit)
            }
            _ => (
                RoutedTarget::Component(self.focused_component),
                ShellCommand::RecordInput,
            ),
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

        if self.active_screen() == ShellScreen::ExitConfirm {
            let routed_target = if hit_target == Some(ShellComponent::ExitDialog) {
                RoutedTarget::Modal(ShellComponent::ExitDialog)
            } else {
                RoutedTarget::Modal(ShellComponent::ExitDialog)
            };
            return (routed_target, ShellCommand::CaptureOverlayInput);
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
        self.hit_map = build_shell_hit_map(
            self.terminal_size,
            self.active_screen(),
            self.active_popup,
            self.hit_map_generation,
        );

        let focus_order = self.focus_order();
        if !focus_order.contains(&self.focused_component) {
            self.focused_component = focus_order.first().copied().unwrap_or(ShellComponent::Home);
        }
    }

    fn focus_order(&self) -> Vec<ShellComponent> {
        if self.active_screen() == ShellScreen::ExitConfirm {
            return vec![ShellComponent::ExitDialog];
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

    fn pop_to_home(&mut self) {
        self.screen_stack.truncate(1);
        if self.screen_stack.is_empty() {
            self.screen_stack.push(ShellScreen::Home);
        }
        self.focused_component = ShellComponent::Home;
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
    generation: u64,
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
            regions.push(ShellHitRegion {
                component: ShellComponent::Home,
                area: main,
            });
            regions.push(ShellHitRegion {
                component: ShellComponent::StatusBar,
                area: status,
            });
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

    ShellHitMap::new(terminal_size, generation, regions)
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

fn current_time_label() -> String {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0);

    format!("unix:{seconds}")
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
    for line in BANNER_LINES {
        writeln!(output, "{line}")?;
    }

    Ok(())
}

pub fn display_banner(output: &mut impl Write) -> io::Result<()> {
    display_animated_banner(output, BANNER_DISPLAY_DURATION)
}

pub fn display_animated_banner(
    output: &mut impl Write,
    total_duration: Duration,
) -> io::Result<()> {
    let started_at = Instant::now();
    let frame_delay = total_duration / (BANNER_LINES.len() as u32 + 1);

    for revealed_lines in 1..=BANNER_LINES.len() {
        write!(output, "\x1B[2J\x1B[H")?;
        for line in BANNER_LINES.iter().take(revealed_lines) {
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

pub fn run_without_animation(output: &mut impl Write) -> io::Result<()> {
    run_not_fullscreen_without_animation(output)
}

pub fn run_not_fullscreen_without_animation(output: &mut impl Write) -> io::Result<()> {
    render_static_banner(output)?;
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
    display_banner(output)?;
    write_smoke_loop_message(output)
}

pub fn run_fullscreen_once_without_animation(output: &mut impl Write) -> io::Result<()> {
    with_fullscreen(output, |output| {
        render_static_banner(output)?;
        write_smoke_loop_message(output)
    })
}

pub fn run_fullscreen_blocking(
    output: &mut impl Write,
    config: ShellLaunchConfig,
) -> io::Result<()> {
    SHELL_RUNNING.store(true, Ordering::SeqCst);
    install_panic_restore_hook();
    let _handler = ConsoleControlHandler::install();
    let mut guard = TerminalGuard::enter(output)?;
    let initial_size = crossterm::terminal::size().unwrap_or((80, 24));
    let mut state = ShellState::new(config, initial_size);
    let tick_rate = Duration::from_millis(250);
    let theme = tundra_ui::TundraTheme::default_dark();

    loop {
        let chrome = state.to_shell_chrome_view_model();
        let home = state.to_home_view_model();
        let active_screen = state.active_screen();
        let exit_confirmation = tundra_ui::ExitConfirmViewModel::new();

        guard.terminal_mut().draw(|frame| {
            let area = frame.area();
            tundra_ui::render_home(frame, area, &chrome, &home, &theme);

            if active_screen == ShellScreen::ExitConfirm {
                tundra_ui::render_exit_confirmation(frame, area, &exit_confirmation, &theme);
            }
        })?;

        if !SHELL_RUNNING.load(Ordering::SeqCst) {
            state.apply_input(InputEvent::Shutdown);
        }
        if state.shutdown_requested() {
            break;
        }

        let action = if event::poll(tick_rate)? {
            state.apply_input(crossterm_event_to_input(event::read()?))
        } else {
            state.apply_input(InputEvent::Tick)
        };

        if action == ShellAction::Exit {
            break;
        }
    }

    guard.restore()
}

fn with_fullscreen<W, T>(
    output: &mut W,
    body: impl FnOnce(&mut W) -> io::Result<T>,
) -> io::Result<T>
where
    W: Write,
{
    write!(output, "{ENTER_FULLSCREEN_SEQUENCE}")?;
    output.flush()?;

    let body_result = body(output);
    let exit_result = write!(output, "{EXIT_FULLSCREEN_SEQUENCE}").and_then(|_| output.flush());

    match (body_result, exit_result) {
        (Ok(value), Ok(())) => Ok(value),
        (Err(error), _) => Err(error),
        (Ok(_), Err(error)) => Err(error),
    }
}

fn write_smoke_loop_message(output: &mut impl Write) -> io::Result<()> {
    for line in startup_lines() {
        writeln!(output, "{line}")?;
    }
    writeln!(output, "Entering smoke loop")
}

#[cfg(windows)]
struct ConsoleControlHandler {
    installed: bool,
}

#[cfg(windows)]
impl ConsoleControlHandler {
    fn install() -> Self {
        let installed =
            unsafe { SetConsoleCtrlHandler(Some(handle_console_control), true.into()) != 0 };

        Self { installed }
    }
}

#[cfg(windows)]
impl Drop for ConsoleControlHandler {
    fn drop(&mut self) {
        if self.installed {
            unsafe {
                SetConsoleCtrlHandler(Some(handle_console_control), false.into());
            }
        }
    }
}

#[cfg(windows)]
unsafe extern "system" fn handle_console_control(control_type: u32) -> i32 {
    match control_type {
        CTRL_C_EVENT | CTRL_BREAK_EVENT | CTRL_CLOSE_EVENT | CTRL_LOGOFF_EVENT
        | CTRL_SHUTDOWN_EVENT => {
            SHELL_RUNNING.store(false, Ordering::SeqCst);
            true.into()
        }
        _ => false.into(),
    }
}

#[cfg(windows)]
const CTRL_C_EVENT: u32 = 0;
#[cfg(windows)]
const CTRL_BREAK_EVENT: u32 = 1;
#[cfg(windows)]
const CTRL_CLOSE_EVENT: u32 = 2;
#[cfg(windows)]
const CTRL_LOGOFF_EVENT: u32 = 5;
#[cfg(windows)]
const CTRL_SHUTDOWN_EVENT: u32 = 6;

#[cfg(not(windows))]
struct ConsoleControlHandler;

#[cfg(not(windows))]
impl ConsoleControlHandler {
    fn install() -> Self {
        Self
    }
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
}

#[cfg(windows)]
#[link(name = "kernel32")]
unsafe extern "system" {
    fn SetConsoleCtrlHandler(
        handler_routine: Option<unsafe extern "system" fn(u32) -> i32>,
        add: i32,
    ) -> i32;
}
