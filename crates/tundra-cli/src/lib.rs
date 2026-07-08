use std::env;
use std::fmt;
use std::fs;
use std::io::Write;
use std::path::Path;

use tundra_platform::{
    AppPaths, CapabilityStatus, CheckStatus, EnvironmentCheck, PathCheck, Platform, PlatformKind,
};
use tundra_storage::{StorageLayout, StorageManager};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CliCommand {
    Doctor,
    Explain,
    New,
    Paths,
    Weathr,
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
        "new" => Ok(CliCommand::New),
        "paths" => Ok(CliCommand::Paths),
        "weathr" => Ok(CliCommand::Weathr),
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
    let platform = tundra_platform::native_platform();
    run_with_platform(args, platform.as_ref(), stdout, stderr)
}

pub fn run_with_platform<I, S, Stdout, Stderr>(
    args: I,
    platform: &dyn Platform,
    stdout: &mut Stdout,
    stderr: &mut Stderr,
) -> i32
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
    Stdout: Write,
    Stderr: Write,
{
    run_with_platform_and_weathr_launcher(
        args,
        platform,
        stdout,
        stderr,
        tundra_weathr::run_default_blocking,
    )
}

#[doc(hidden)]
pub fn run_with_platform_and_weathr_launcher<I, S, Stdout, Stderr, Launcher, LaunchError>(
    args: I,
    platform: &dyn Platform,
    stdout: &mut Stdout,
    stderr: &mut Stderr,
    weathr_launcher: Launcher,
) -> i32
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
    Stdout: Write,
    Stderr: Write,
    Launcher: FnOnce() -> Result<(), LaunchError>,
    LaunchError: fmt::Display,
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
        Ok(CliCommand::New) => run_new(platform, stdout, stderr),
        Ok(CliCommand::Paths) => run_paths(platform, stdout, stderr),
        Ok(CliCommand::Doctor) => run_doctor(platform, stdout, stderr),
        Ok(CliCommand::Weathr) => run_weathr(stderr, weathr_launcher),
        Err(error) => {
            let _ = writeln!(stderr, "ERROR: {error}");
            let _ = write_help(stderr);
            2
        }
    }
}

fn run_weathr<Stderr, Launcher, LaunchError>(stderr: &mut Stderr, weathr_launcher: Launcher) -> i32
where
    Stderr: Write,
    Launcher: FnOnce() -> Result<(), LaunchError>,
    LaunchError: fmt::Display,
{
    match weathr_launcher() {
        Ok(()) => 0,
        Err(error) => {
            let _ = writeln!(stderr, "ERROR: could not launch weathr: {error}");
            1
        }
    }
}

fn run_new<Stdout: Write, Stderr: Write>(
    platform: &dyn Platform,
    stdout: &mut Stdout,
    stderr: &mut Stderr,
) -> i32 {
    match platform.app_paths() {
        Ok(paths) => match reset_saved_content(&paths) {
            Ok(report) => {
                let _ = writeln!(stdout, "TundraUX3 storage reset");
                let _ = writeln!(stdout, "Removed paths:");
                for path in &report.removed_paths {
                    let _ = writeln!(stdout, "  {}", path.display());
                }
                let _ = writeln!(stdout);
                let _ = writeln!(stdout, "Recreated storage files:");
                let layout = StorageLayout::from_app_paths(&paths);
                write_storage_files(stdout, &layout);
                0
            }
            Err(error) => {
                let _ = writeln!(stderr, "ERROR: could not reset saved content: {error}");
                1
            }
        },
        Err(error) => {
            let _ = writeln!(stderr, "ERROR: {error}");
            1
        }
    }
}

fn run_paths<Stdout: Write, Stderr: Write>(
    platform: &dyn Platform,
    stdout: &mut Stdout,
    stderr: &mut Stderr,
) -> i32 {
    let _ = writeln!(stdout, "Path templates:");
    write_path_templates(stdout);

    match platform.app_paths() {
        Ok(paths) => {
            let _ = writeln!(stdout);
            let _ = writeln!(stdout, "Resolved paths:");
            write_resolved_paths(stdout, &paths);
            let _ = writeln!(stdout);
            let _ = writeln!(stdout, "Storage files:");
            write_storage_files(stdout, &StorageLayout::from_app_paths(&paths));
            0
        }
        Err(error) => {
            let _ = writeln!(stderr, "ERROR: {error}");
            1
        }
    }
}

fn run_doctor<Stdout: Write, Stderr: Write>(
    platform: &dyn Platform,
    stdout: &mut Stdout,
    stderr: &mut Stderr,
) -> i32 {
    let _ = writeln!(stdout, "TundraUX3 doctor");
    let _ = writeln!(stdout, "Platform kind: {}", platform.kind().as_str());
    let _ = writeln!(stdout);
    let _ = writeln!(stdout, "Path templates:");
    write_path_templates(stdout);

    match tundra_platform::run_doctor_with(platform) {
        Ok(report) => {
            let _ = writeln!(stdout);
            let _ = writeln!(stdout, "Resolved paths:");
            write_resolved_paths(stdout, &report.app_paths);
            write_doctor_checks(stdout, &report.environment_checks, &report.path_checks);

            let storage_check = run_storage_check(&report.app_paths);
            write_storage_check(stdout, &storage_check);

            if report.has_failures() || storage_check.status == CheckStatus::Fail {
                let _ = writeln!(stderr, "Doctor result: FAIL");
                1
            } else {
                let _ = writeln!(stdout, "Doctor result: PASS");
                0
            }
        }
        Err(error) => {
            write_fallback_doctor_checks(stdout, platform, &error);
            let _ = writeln!(stderr, "Doctor result: FAIL");
            1
        }
    }
}

fn write_help(output: &mut impl Write) -> std::io::Result<()> {
    writeln!(output, "TundraUX3 CLI")?;
    writeln!(
        output,
        "Usage: tundra-cli <doctor|explain|new|paths|weathr>"
    )?;
    writeln!(
        output,
        "  doctor  Check Windows/macOS, terminal, and app path readiness"
    )?;
    writeln!(
        output,
        "  explain Show CLI startup flow and kernel/UI boundaries"
    )?;
    writeln!(
        output,
        "  new     Clear saved TundraUX3 data and recreate initial storage"
    )?;
    writeln!(output, "  paths   Print configured and resolved app paths")?;
    writeln!(output, "  weathr  Launch the terminal weather scene")
}

fn write_explain(output: &mut impl Write) -> std::io::Result<()> {
    writeln!(output, "TundraUX3 startup and boundary model")?;
    writeln!(output)?;
    writeln!(output, "Startup flow:")?;
    writeln!(
        output,
        "  1. User starts tundra-cli or tundra-shell from a crossterm-compatible terminal."
    )?;
    writeln!(
        output,
        "  2. tundra-cli handles diagnostics, operator commands, and launchers: doctor, paths, explain, new, weathr."
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
        "  - tundra-platform is the platform boundary for OS facts, paths, terminal checks, and future platform API calls."
    )?;
    writeln!(
        output,
        "  - tundra-storage owns config/state format boundaries: TOML config and schema-v1 JSON state."
    )?;
    writeln!(
        output,
        "  - UI and app code must call these crates instead of touching platform APIs or storage paths directly."
    )?;
    writeln!(output)?;
    writeln!(output, "UI boundary:")?;
    writeln!(
        output,
        "  - tundra-shell owns startup visuals, shell lifecycle, and the future event/render loop."
    )?;
    writeln!(
        output,
        "  - UI code consumes view state and commands; it should not create platform-specific paths or call platform APIs directly."
    )
}

fn write_path_templates(output: &mut impl Write) {
    let _ = writeln!(output, "Config path: {}", AppPaths::CONFIG_TEMPLATE);
    let _ = writeln!(output, "Data path:   {}", AppPaths::DATA_TEMPLATE);
    let _ = writeln!(output, "Cache path:  {}", AppPaths::CACHE_TEMPLATE);
    let _ = writeln!(output, "Logs path:   {}", AppPaths::LOGS_TEMPLATE);
    let _ = writeln!(output, "Temp path:   {}", AppPaths::TEMP_TEMPLATE);
}

fn write_resolved_paths(output: &mut impl Write, paths: &AppPaths) {
    let _ = writeln!(output, "Config path: {}", paths.config_path().display());
    let _ = writeln!(output, "Data path:   {}", paths.data_path().display());
    let _ = writeln!(output, "Cache path:  {}", paths.cache_path().display());
    let _ = writeln!(output, "Logs path:   {}", paths.logs_path().display());
    let _ = writeln!(output, "Temp path:   {}", paths.temp_path().display());
}

fn write_storage_files(output: &mut impl Write, layout: &StorageLayout) {
    let _ = writeln!(output, "Config file:  {}", layout.config_path.display());
    let _ = writeln!(output, "State file:   {}", layout.state_path.display());
    let _ = writeln!(
        output,
        "Recent files: {}",
        layout.recent_files_path.display()
    );
    let _ = writeln!(output, "Sessions file: {}", layout.sessions_path.display());
    let _ = writeln!(output, "Users file:   {}", layout.users_path.display());
    let _ = writeln!(output, "Audit log:    {}", layout.audit_log_path.display());
}

fn write_doctor_checks(
    output: &mut impl Write,
    environment_checks: &[EnvironmentCheck],
    path_checks: &[PathCheck],
) {
    let _ = writeln!(output);
    let _ = writeln!(output, "Checks:");

    let _ = writeln!(output);
    let _ = writeln!(output, "Platform checks:");
    for check in environment_checks
        .iter()
        .filter(|check| is_platform_check(check))
    {
        write_environment_check(output, check);
    }

    let _ = writeln!(output);
    let _ = writeln!(output, "Terminal check:");
    for check in environment_checks
        .iter()
        .filter(|check| is_terminal_check(check))
    {
        write_environment_check(output, check);
    }

    let _ = writeln!(output);
    let _ = writeln!(output, "Capability checks:");
    for check in environment_checks
        .iter()
        .filter(|check| is_capability_check(check))
    {
        write_environment_check(output, check);
    }

    let _ = writeln!(output);
    let _ = writeln!(output, "Path checks:");
    for check in path_checks {
        write_path_check(output, check);
    }
}

fn write_storage_check(output: &mut impl Write, check: &StorageCheck) {
    let _ = writeln!(output);
    let _ = writeln!(output, "Storage checks:");
    let _ = writeln!(
        output,
        "[{}] {}: {}",
        check.status.as_str(),
        check.label,
        check.message
    );
}

fn write_environment_check(output: &mut impl Write, check: &EnvironmentCheck) {
    let _ = writeln!(
        output,
        "[{}] {}: {}",
        check.status.as_str(),
        check.label,
        check.message
    );
}

fn write_path_check(output: &mut impl Write, check: &PathCheck) {
    let _ = writeln!(
        output,
        "[{}] {}: {} - {}",
        check.status.as_str(),
        check.label,
        check.path.display(),
        check.message
    );
}

fn write_fallback_doctor_checks(
    output: &mut impl Write,
    platform: &dyn Platform,
    error: &tundra_platform::PlatformError,
) {
    let terminal_check = fallback_terminal_check(platform.kind());
    let capability_checks = fallback_capability_checks(platform);

    let _ = writeln!(output);
    let _ = writeln!(output, "Checks:");

    let _ = writeln!(output);
    let _ = writeln!(output, "Terminal check:");
    write_environment_check(output, &terminal_check);

    let _ = writeln!(output);
    let _ = writeln!(output, "Capability checks:");
    for check in &capability_checks {
        write_environment_check(output, check);
    }

    let _ = writeln!(output);
    let _ = writeln!(output, "Path checks:");
    let _ = writeln!(output, "[FAIL] App paths: {error}");
}

fn fallback_terminal_check(kind: PlatformKind) -> EnvironmentCheck {
    match kind {
        PlatformKind::Windows => {
            if tundra_platform::is_windows_terminal_session(env::var("WT_SESSION").ok().as_deref())
            {
                EnvironmentCheck {
                    label: "Terminal".to_string(),
                    status: CheckStatus::Pass,
                    message: "Windows Terminal detected".to_string(),
                }
            } else {
                EnvironmentCheck {
                    label: "Terminal".to_string(),
                    status: CheckStatus::Warning,
                    message: "Windows Terminal not detected; conhost is best-effort only"
                        .to_string(),
                }
            }
        }
        PlatformKind::Macos => EnvironmentCheck {
            label: "Terminal".to_string(),
            status: CheckStatus::Pass,
            message: "macOS terminal session supported".to_string(),
        },
        PlatformKind::Unsupported => EnvironmentCheck {
            label: "Terminal".to_string(),
            status: CheckStatus::Warning,
            message: "terminal support is unsupported on this platform".to_string(),
        },
    }
}

fn fallback_capability_checks(platform: &dyn Platform) -> Vec<EnvironmentCheck> {
    platform
        .capabilities()
        .checks()
        .into_iter()
        .map(|(name, status)| EnvironmentCheck {
            label: format!("Capability: {name}"),
            status: check_status_for_capability(status),
            message: status.as_str().to_string(),
        })
        .collect()
}

fn check_status_for_capability(status: CapabilityStatus) -> CheckStatus {
    match status {
        CapabilityStatus::Supported => CheckStatus::Pass,
        CapabilityStatus::BestEffort => CheckStatus::Warning,
        CapabilityStatus::Unsupported => CheckStatus::Warning,
    }
}

fn is_platform_check(check: &EnvironmentCheck) -> bool {
    !is_terminal_check(check) && !is_capability_check(check)
}

fn is_terminal_check(check: &EnvironmentCheck) -> bool {
    check.label == "Terminal"
}

fn is_capability_check(check: &EnvironmentCheck) -> bool {
    check.label.starts_with("Capability: ")
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct StorageCheck {
    label: &'static str,
    status: CheckStatus,
    message: String,
}

fn run_storage_check(paths: &AppPaths) -> StorageCheck {
    match StorageManager::open(paths.clone()) {
        Ok(opened)
            if opened.report.warnings.is_empty() && opened.report.migrated_files.is_empty() =>
        {
            StorageCheck {
                label: "Storage bootstrap",
                status: CheckStatus::Pass,
                message: "storage initialized and loaded cleanly".to_string(),
            }
        }
        Ok(opened) => StorageCheck {
            label: "Storage bootstrap",
            status: CheckStatus::Warning,
            message: storage_warning_message(&opened.report),
        },
        Err(error) => StorageCheck {
            label: "Storage bootstrap",
            status: CheckStatus::Fail,
            message: error.to_string(),
        },
    }
}

fn storage_warning_message(report: &tundra_storage::StorageLoadReport) -> String {
    let mut warnings = report.warnings.clone();
    if !report.migrated_files.is_empty() {
        warnings.push(format!(
            "migrated {} storage files",
            report.migrated_files.len()
        ));
    }

    if warnings.is_empty() {
        "storage initialized with warnings".to_string()
    } else {
        format!("storage initialized with warnings: {}", warnings.join("; "))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ResetReport {
    removed_paths: Vec<std::path::PathBuf>,
}

fn reset_saved_content(paths: &AppPaths) -> Result<ResetReport, std::io::Error> {
    let candidates = [
        paths.config_path(),
        paths.data_path(),
        paths.cache_path(),
        paths.logs_path(),
        paths.temp_path(),
    ];
    let mut removed_paths = Vec::new();

    for path in candidates {
        guard_reset_path(path)?;
        if path.exists() {
            remove_path(path)?;
            removed_paths.push(path.to_path_buf());
        }
    }

    StorageManager::open(paths.clone())
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::Other, error.to_string()))?;

    Ok(ResetReport { removed_paths })
}

fn guard_reset_path(path: &Path) -> Result<(), std::io::Error> {
    if !path.is_absolute() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("refusing to reset non-absolute path {}", path.display()),
        ));
    }

    if path.parent().is_none() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("refusing to reset root path {}", path.display()),
        ));
    }

    Ok(())
}

fn remove_path(path: &Path) -> Result<(), std::io::Error> {
    if path.is_dir() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    }
}
