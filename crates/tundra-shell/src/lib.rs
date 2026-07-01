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
pub enum ShellLaunchMode {
    Fullscreen,
    NotFullscreen,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShellArgError {
    UnknownArgument(String),
    UnexpectedArgument(String),
}

impl std::fmt::Display for ShellArgError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownArgument(argument) => write!(formatter, "unknown argument: {argument}"),
            Self::UnexpectedArgument(argument) => {
                write!(formatter, "unexpected argument: {argument}")
            }
        }
    }
}

impl std::error::Error for ShellArgError {}

pub fn parse_shell_args<I, S>(args: I) -> Result<ShellLaunchMode, ShellArgError>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut args = args.into_iter();
    let Some(first) = args.next() else {
        return Ok(ShellLaunchMode::Fullscreen);
    };

    if let Some(extra) = args.next() {
        return Err(ShellArgError::UnexpectedArgument(
            extra.as_ref().to_string(),
        ));
    }

    match first.as_ref() {
        "-notfullscreen" => Ok(ShellLaunchMode::NotFullscreen),
        other => Err(ShellArgError::UnknownArgument(other.to_string())),
    }
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
    write_main_loop_placeholder(output)
}

pub fn run_with_banner_animation(output: &mut impl Write) -> io::Result<()> {
    run_not_fullscreen(output)
}

pub fn run_not_fullscreen(output: &mut impl Write) -> io::Result<()> {
    display_banner(output)?;
    write_main_loop_placeholder(output)
}

pub fn run_fullscreen_once_without_animation(output: &mut impl Write) -> io::Result<()> {
    with_fullscreen(output, |output| {
        render_static_banner(output)?;
        write_main_loop_placeholder(output)
    })
}

pub fn run_fullscreen_blocking(output: &mut impl Write) -> io::Result<()> {
    SHELL_RUNNING.store(true, Ordering::SeqCst);
    let _handler = ConsoleControlHandler::install();

    with_fullscreen(output, |output| {
        display_banner(output)?;
        write_main_loop_placeholder(output)?;
        writeln!(
            output,
            "Fullscreen main loop placeholder is active. Start with -notfullscreen for the non-fullscreen smoke path."
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

fn write_main_loop_placeholder(output: &mut impl Write) -> io::Result<()> {
    for line in startup_lines() {
        writeln!(output, "{line}")?;
    }
    writeln!(output, "Entering main loop placeholder")
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
