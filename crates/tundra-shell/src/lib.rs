#[cfg(not(windows))]
compile_error!("TundraUX3 phase 0 supports Windows 11 only.");

use tundra_storage::{CONFIG_DESCRIPTOR, SCHEMA_VERSION};

use crossterm::cursor::{Hide, Show};
use crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyModifiers,
    MouseEvent, MouseEventKind,
};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShellInput {
    Key(String),
    Mouse {
        summary: String,
        coordinates: Option<(u16, u16)>,
        scroll_direction: Option<String>,
    },
    Resize {
        width: u16,
        height: u16,
    },
    Tick,
    Shutdown,
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
    tick_count: u64,
    status_message: String,
    toast_message: Option<String>,
    error_message: Option<String>,
    shutdown_requested: bool,
    last_key_event: Option<String>,
    last_mouse_event: Option<String>,
    last_resize_event: Option<String>,
    mouse_coordinates: Option<(u16, u16)>,
    mouse_scroll_direction: Option<String>,
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

        Self {
            home_mode,
            screen_stack: vec![ShellScreen::Home],
            terminal_size,
            terminal_flags: ShellTerminalFlags::enabled(),
            tick_count: 0,
            status_message: "Ready".to_string(),
            toast_message: None,
            error_message: None,
            shutdown_requested: false,
            last_key_event: None,
            last_mouse_event: None,
            last_resize_event: None,
            mouse_coordinates: None,
            mouse_scroll_direction: None,
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

    pub fn apply_input(&mut self, input: ShellInput) -> ShellAction {
        match input {
            ShellInput::Key(key) => self.apply_key(key),
            ShellInput::Mouse {
                summary,
                coordinates,
                scroll_direction,
            } => {
                self.last_mouse_event = Some(summary);
                self.mouse_coordinates = coordinates;
                self.mouse_scroll_direction = scroll_direction;
                ShellAction::Redraw
            }
            ShellInput::Resize { width, height } => {
                self.terminal_size = (width, height);
                self.last_resize_event = Some(format!("{width}x{height}"));
                ShellAction::Redraw
            }
            ShellInput::Tick => {
                self.tick_count = self.tick_count.saturating_add(1);
                ShellAction::Redraw
            }
            ShellInput::Shutdown => {
                self.shutdown_requested = true;
                ShellAction::Exit
            }
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

    fn home_display_mode(&self) -> tundra_ui::HomeDisplayMode {
        match self.home_mode {
            ShellHomeMode::Debug => tundra_ui::HomeDisplayMode::Debug,
            ShellHomeMode::User => tundra_ui::HomeDisplayMode::User,
        }
    }

    fn apply_key(&mut self, key: String) -> ShellAction {
        match self.active_screen() {
            ShellScreen::Home if key == "q" || key == "Esc" => {
                self.screen_stack.push(ShellScreen::ExitConfirm);
                self.status_message = "Confirm exit".to_string();
                ShellAction::Redraw
            }
            ShellScreen::ExitConfirm if key == "y" || key == "Y" || key == "Enter" => {
                self.shutdown_requested = true;
                ShellAction::Exit
            }
            ShellScreen::ExitConfirm if key == "n" || key == "N" || key == "Esc" => {
                self.pop_to_home();
                self.status_message = "Ready".to_string();
                ShellAction::Redraw
            }
            _ => {
                self.last_key_event = Some(key);
                ShellAction::Redraw
            }
        }
    }

    fn pop_to_home(&mut self) {
        self.screen_stack.truncate(1);
        if self.screen_stack.is_empty() {
            self.screen_stack.push(ShellScreen::Home);
        }
    }
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

fn key_event_to_label(key_event: KeyEvent) -> String {
    match key_event.code {
        KeyCode::Char('c' | 'C') if key_event.modifiers.contains(KeyModifiers::CONTROL) => {
            "Ctrl+C".to_string()
        }
        KeyCode::Char(character) => character.to_string(),
        KeyCode::Enter => "Enter".to_string(),
        KeyCode::Esc => "Esc".to_string(),
        KeyCode::Backspace => "Backspace".to_string(),
        KeyCode::Tab => "Tab".to_string(),
        KeyCode::BackTab => "Shift+Tab".to_string(),
        KeyCode::Left => "Left".to_string(),
        KeyCode::Right => "Right".to_string(),
        KeyCode::Up => "Up".to_string(),
        KeyCode::Down => "Down".to_string(),
        other => format!("{other:?}"),
    }
}

fn mouse_event_to_input(mouse_event: MouseEvent) -> ShellInput {
    let coordinates = Some((mouse_event.column, mouse_event.row));
    let (summary, scroll_direction) = match mouse_event.kind {
        MouseEventKind::Down(button) => (format!("Mouse Down {button:?}"), None),
        MouseEventKind::Up(button) => (format!("Mouse Up {button:?}"), None),
        MouseEventKind::Drag(button) => (format!("Mouse Drag {button:?}"), None),
        MouseEventKind::Moved => ("Mouse Moved".to_string(), None),
        MouseEventKind::ScrollDown => mouse_scroll_summary("Down"),
        MouseEventKind::ScrollUp => mouse_scroll_summary("Up"),
        MouseEventKind::ScrollLeft => mouse_scroll_summary("Left"),
        MouseEventKind::ScrollRight => mouse_scroll_summary("Right"),
    };

    ShellInput::Mouse {
        summary,
        coordinates,
        scroll_direction,
    }
}

fn mouse_scroll_summary(direction: &str) -> (String, Option<String>) {
    (
        format!("Mouse Scroll {direction}"),
        Some(direction.to_string()),
    )
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
        "Supported OS: Windows 11 only".to_string(),
        "Target terminal: Windows Terminal; conhost is best-effort only".to_string(),
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
            state.apply_input(ShellInput::Shutdown);
        }
        if state.shutdown_requested() {
            break;
        }

        let action = if event::poll(tick_rate)? {
            match event::read()? {
                Event::Key(key_event) => {
                    let label = key_event_to_label(key_event);
                    if label == "Ctrl+C" {
                        state.apply_input(ShellInput::Shutdown)
                    } else {
                        state.apply_input(ShellInput::Key(label))
                    }
                }
                Event::Mouse(mouse_event) => state.apply_input(mouse_event_to_input(mouse_event)),
                Event::Resize(width, height) => {
                    state.apply_input(ShellInput::Resize { width, height })
                }
                Event::FocusGained | Event::FocusLost | Event::Paste(_) => continue,
            }
        } else {
            state.apply_input(ShellInput::Tick)
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

struct ConsoleControlHandler {
    installed: bool,
}

impl ConsoleControlHandler {
    fn install() -> Self {
        let installed =
            unsafe { SetConsoleCtrlHandler(Some(handle_console_control), true.into()) != 0 };

        Self { installed }
    }
}

impl Drop for ConsoleControlHandler {
    fn drop(&mut self) {
        if self.installed {
            unsafe {
                SetConsoleCtrlHandler(Some(handle_console_control), false.into());
            }
        }
    }
}

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

const CTRL_C_EVENT: u32 = 0;
const CTRL_BREAK_EVENT: u32 = 1;
const CTRL_CLOSE_EVENT: u32 = 2;
const CTRL_LOGOFF_EVENT: u32 = 5;
const CTRL_SHUTDOWN_EVENT: u32 = 6;

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
            ShellInput::Mouse {
                summary: "Mouse Down Left".to_string(),
                coordinates: Some((12, 7)),
                scroll_direction: None,
            }
        );
        assert_eq!(
            drag,
            ShellInput::Mouse {
                summary: "Mouse Drag Right".to_string(),
                coordinates: Some((13, 8)),
                scroll_direction: None,
            }
        );
        assert_eq!(
            moved,
            ShellInput::Mouse {
                summary: "Mouse Moved".to_string(),
                coordinates: Some((14, 9)),
                scroll_direction: None,
            }
        );
        assert_eq!(
            scroll_up,
            ShellInput::Mouse {
                summary: "Mouse Scroll Up".to_string(),
                coordinates: Some((15, 10)),
                scroll_direction: Some("Up".to_string()),
            }
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
                ShellInput::Mouse {
                    summary: format!("Mouse Scroll {expected_direction}"),
                    coordinates: Some((1, 2)),
                    scroll_direction: Some(expected_direction.to_string()),
                }
            );
        }
    }
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn SetConsoleCtrlHandler(
        handler_routine: Option<unsafe extern "system" fn(u32) -> i32>,
        add: i32,
    ) -> i32;
}
