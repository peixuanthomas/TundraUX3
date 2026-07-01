#[cfg(not(windows))]
compile_error!("TundraUX3 phase 0 supports Windows 11 only.");

use tundra_storage::{CONFIG_DESCRIPTOR, SCHEMA_VERSION};

use std::io::{self, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

pub const BANNER_DISPLAY_DURATION: Duration = Duration::from_secs(2);
pub const ENTER_FULLSCREEN_SEQUENCE: &str = "\x1B[?1049h\x1B[?25l\x1B[2J\x1B[H";
pub const EXIT_FULLSCREEN_SEQUENCE: &str = "\x1B[?25h\x1B[?1049l";

static SHELL_RUNNING: AtomicBool = AtomicBool::new(true);

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellState {
    home_mode: ShellHomeMode,
    screen_stack: Vec<ShellScreen>,
    terminal_size: (u16, u16),
    terminal_flags: ShellTerminalFlags,
    tick_count: u64,
    status: String,
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
            status: "Ready".to_string(),
            shutdown_requested: false,
            last_key_event: None,
            last_mouse_event: None,
            last_resize_event: None,
            mouse_coordinates: None,
            mouse_scroll_direction: None,
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
        &self.status
    }

    pub fn terminal_flags(&self) -> ShellTerminalFlags {
        self.terminal_flags
    }

    pub fn mouse_scroll_direction(&self) -> Option<&str> {
        self.mouse_scroll_direction.as_deref()
    }

    fn apply_key(&mut self, key: String) -> ShellAction {
        match self.active_screen() {
            ShellScreen::Home if key == "q" || key == "Esc" => {
                self.screen_stack.push(ShellScreen::ExitConfirm);
                self.status = "Confirm exit".to_string();
                ShellAction::Redraw
            }
            ShellScreen::ExitConfirm if key == "y" || key == "Y" || key == "Enter" => {
                self.shutdown_requested = true;
                ShellAction::Exit
            }
            ShellScreen::ExitConfirm if key == "n" || key == "N" || key == "Esc" => {
                self.pop_to_home();
                self.status = "Ready".to_string();
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
    _config: ShellLaunchConfig,
) -> io::Result<()> {
    SHELL_RUNNING.store(true, Ordering::SeqCst);
    let _handler = ConsoleControlHandler::install();

    with_fullscreen(output, |output| {
        display_banner(output)?;
        write_smoke_loop_message(output)?;
        writeln!(
            output,
            "Fullscreen smoke loop is active. Start with -notfullscreen for the non-fullscreen smoke path."
        )?;
        output.flush()?;

        while SHELL_RUNNING.load(Ordering::SeqCst) {
            thread::sleep(Duration::from_millis(250));
        }

        Ok(())
    })
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

#[link(name = "kernel32")]
unsafe extern "system" {
    fn SetConsoleCtrlHandler(
        handler_routine: Option<unsafe extern "system" fn(u32) -> i32>,
        add: i32,
    ) -> i32;
}
