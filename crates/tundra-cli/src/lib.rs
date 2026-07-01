#[cfg(not(windows))]
compile_error!("TundraUX3 phase 0 supports Windows 11 only.");

use std::fmt;
use std::io::Write;

use tundra_platform::AppPaths;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CliCommand {
    Doctor,
    Explain,
    Paths,
    Help,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CliError {
    UnknownCommand(String),
    UnexpectedArgument(String),
}

impl fmt::Display for CliError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownCommand(command) => write!(formatter, "unknown command: {command}"),
            Self::UnexpectedArgument(argument) => {
                write!(formatter, "unexpected argument: {argument}")
            }
        }
    }
}

impl std::error::Error for CliError {}

pub fn parse_args<I, S>(args: I) -> Result<CliCommand, CliError>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut args = args.into_iter();
    let Some(command) = args.next() else {
        return Ok(CliCommand::Help);
    };

    if let Some(extra) = args.next() {
        return Err(CliError::UnexpectedArgument(extra.as_ref().to_string()));
    }

    match command.as_ref() {
        "doctor" => Ok(CliCommand::Doctor),
        "explain" => Ok(CliCommand::Explain),
        "paths" => Ok(CliCommand::Paths),
        "-h" | "--help" | "help" => Ok(CliCommand::Help),
        other => Err(CliError::UnknownCommand(other.to_string())),
    }
}

pub fn run<I, S, Stdout, Stderr>(args: I, stdout: &mut Stdout, stderr: &mut Stderr) -> i32
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
    Stdout: Write,
    Stderr: Write,
{
    match parse_args(args) {
        Ok(CliCommand::Help) => {
            let _ = write_help(stdout);
            0
        }
        Ok(CliCommand::Explain) => {
            let _ = write_explain(stdout);
            0
        }
        Ok(CliCommand::Paths) => run_paths(stdout, stderr),
        Ok(CliCommand::Doctor) => run_doctor(stdout, stderr),
        Err(error) => {
            let _ = writeln!(stderr, "ERROR: {error}");
            let _ = write_help(stderr);
            2
        }
    }
}

fn run_paths<Stdout: Write, Stderr: Write>(stdout: &mut Stdout, stderr: &mut Stderr) -> i32 {
    write_path_templates(stdout);

    match AppPaths::from_environment() {
        Ok(paths) => {
            let _ = writeln!(stdout);
            let _ = writeln!(stdout, "Resolved:");
            write_resolved_paths(stdout, &paths);
            0
        }
        Err(error) => {
            let _ = writeln!(stderr, "ERROR: {error}");
            1
        }
    }
}

fn run_doctor<Stdout: Write, Stderr: Write>(stdout: &mut Stdout, stderr: &mut Stderr) -> i32 {
    let _ = writeln!(stdout, "TundraUX3 doctor");
    write_path_templates(stdout);

    match tundra_platform::run_doctor() {
        Ok(report) => {
            let _ = writeln!(stdout);
            let _ = writeln!(stdout, "Resolved:");
            write_resolved_paths(stdout, &report.app_paths);
            let _ = writeln!(stdout);
            let _ = writeln!(stdout, "Checks:");

            for check in &report.environment_checks {
                let _ = writeln!(
                    stdout,
                    "[{}] {}: {}",
                    check.status.as_str(),
                    check.label,
                    check.message
                );
            }

            for check in &report.path_checks {
                let _ = writeln!(
                    stdout,
                    "[{}] {}: {}",
                    check.status.as_str(),
                    check.label,
                    check.message
                );
            }

            if report.has_failures() {
                let _ = writeln!(stderr, "Doctor result: FAIL");
                1
            } else {
                let _ = writeln!(stdout, "Doctor result: PASS");
                0
            }
        }
        Err(error) => {
            let _ = writeln!(stdout);
            let _ = writeln!(stdout, "Checks:");
            let _ = writeln!(stdout, "[FAIL] Paths: {error}");
            let _ = writeln!(stderr, "Doctor result: FAIL");
            1
        }
    }
}

fn write_help(output: &mut impl Write) -> std::io::Result<()> {
    writeln!(output, "TundraUX3 CLI")?;
    writeln!(output, "Usage: tundra-cli <doctor|explain|paths>")?;
    writeln!(
        output,
        "  doctor  Check Windows 11, terminal, and app path readiness"
    )?;
    writeln!(
        output,
        "  explain Show CLI startup flow and kernel/UI boundaries"
    )?;
    writeln!(output, "  paths   Print configured and resolved app paths")
}

fn write_explain(output: &mut impl Write) -> std::io::Result<()> {
    writeln!(output, "TundraUX3 startup and boundary model")?;
    writeln!(output)?;
    writeln!(output, "Startup flow:")?;
    writeln!(
        output,
        "  1. User starts tundra-cli or tundra-shell from Windows Terminal."
    )?;
    writeln!(
        output,
        "  2. tundra-cli handles diagnostics and operator commands: doctor, paths, explain."
    )?;
    writeln!(
        output,
        "  3. tundra-shell shows the banner, initializes the UX shell, then enters the main loop."
    )?;
    writeln!(
        output,
        "  4. The main loop will route input to UI controllers; Phase 0 uses a placeholder loop."
    )?;
    writeln!(output)?;
    writeln!(output, "Kernel boundary:")?;
    writeln!(
        output,
        "  - tundra-platform is the Windows boundary for OS facts, paths, terminal checks, and future Win32 calls."
    )?;
    writeln!(
        output,
        "  - tundra-storage owns config/state format boundaries: TOML config and schema-v1 JSON state."
    )?;
    writeln!(
        output,
        "  - UI and app code must call these crates instead of touching Windows APIs or storage paths directly."
    )?;
    writeln!(output)?;
    writeln!(output, "UI boundary:")?;
    writeln!(
        output,
        "  - tundra-shell owns startup visuals, shell lifecycle, and the future event/render loop."
    )?;
    writeln!(
        output,
        "  - UI code consumes view state and commands; it should not create AppData paths or call Win32 directly."
    )
}

fn write_path_templates(output: &mut impl Write) {
    let _ = writeln!(output, "Config path: {}", AppPaths::CONFIG_TEMPLATE);
    let _ = writeln!(output, "Data path:   {}", AppPaths::DATA_TEMPLATE);
    let _ = writeln!(output, "Cache path:  {}", AppPaths::CACHE_TEMPLATE);
}

fn write_resolved_paths(output: &mut impl Write, paths: &AppPaths) {
    let _ = writeln!(output, "Config path: {}", paths.config_path().display());
    let _ = writeln!(output, "Data path:   {}", paths.data_path().display());
    let _ = writeln!(output, "Cache path:  {}", paths.cache_path().display());
}
