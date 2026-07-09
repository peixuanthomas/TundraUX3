use std::fmt;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use tundra_platform::{
    AppPaths, CapabilityStatus, CheckStatus, EnvironmentCheck, PathCheck, Platform, PlatformKind,
};
use tundra_storage::{StorageConfig, StorageLayout, StorageManager};
use tundra_weathr::{LaunchLocation, LaunchOptions};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CliCommand {
    Config(ConfigAction),
    Doctor,
    Explain,
    New,
    Paths,
    Weathr,
    Help,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigAction {
    Get(Option<ConfigField>),
    Set(ConfigUpdate),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigField {
    Theme,
    Language,
    Timezone,
    Address,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigUpdate {
    Theme(String),
    Language(String),
    Timezone(String),
    Address(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CliError {
    ForbiddenConfigField(String),
    MissingArgument(&'static str),
    UnknownCommand(String),
    UnknownConfigCommand(String),
    UnsupportedConfigField(String),
    UnexpectedArgument(String),
}

impl fmt::Display for CliError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ForbiddenConfigField(field) => {
                write!(
                    formatter,
                    "config field {field:?} is not exposed; username and password changes must use authenticated user management"
                )
            }
            Self::MissingArgument(argument) => write!(formatter, "missing argument: {argument}"),
            Self::UnknownCommand(command) => write!(formatter, "unknown command: {command}"),
            Self::UnknownConfigCommand(command) => {
                write!(formatter, "unknown config command: {command}")
            }
            Self::UnsupportedConfigField(field) => {
                write!(formatter, "unsupported config field: {field}")
            }
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
    let mut args = args
        .into_iter()
        .map(|arg| arg.as_ref().to_string())
        .collect::<Vec<_>>();
    if args.is_empty() {
        return Ok(CliCommand::Help);
    };
    let command = args.remove(0);

    match command.as_str() {
        "config" => parse_config_args(&args).map(CliCommand::Config),
        "doctor" => parse_no_extra_args(&args, CliCommand::Doctor),
        "explain" => parse_no_extra_args(&args, CliCommand::Explain),
        "new" => parse_no_extra_args(&args, CliCommand::New),
        "paths" => parse_no_extra_args(&args, CliCommand::Paths),
        "weathr" => parse_no_extra_args(&args, CliCommand::Weathr),
        "-h" | "--help" | "help" => Ok(CliCommand::Help),
        other => Err(CliError::UnknownCommand(other.to_string())),
    }
}

fn parse_no_extra_args(args: &[String], command: CliCommand) -> Result<CliCommand, CliError> {
    if let Some(extra) = args.first() {
        return Err(CliError::UnexpectedArgument(extra.clone()));
    }

    Ok(command)
}

fn parse_config_args(args: &[String]) -> Result<ConfigAction, CliError> {
    let Some(command) = args.first().map(String::as_str) else {
        return Ok(ConfigAction::Get(None));
    };

    match command {
        "get" => parse_config_get(&args[1..]),
        "set" => parse_config_set(&args[1..]),
        other => Err(CliError::UnknownConfigCommand(other.to_string())),
    }
}

fn parse_config_get(args: &[String]) -> Result<ConfigAction, CliError> {
    match args {
        [] => Ok(ConfigAction::Get(None)),
        [field] => parse_config_field(field).map(|field| ConfigAction::Get(Some(field))),
        [_, extra, ..] => Err(CliError::UnexpectedArgument(extra.clone())),
    }
}

fn parse_config_set(args: &[String]) -> Result<ConfigAction, CliError> {
    let field = args
        .first()
        .ok_or(CliError::MissingArgument("config field"))?;
    let value = joined_config_value(&args[1..]).ok_or(CliError::MissingArgument("config value"))?;

    match parse_config_field(field)? {
        ConfigField::Theme => Ok(ConfigAction::Set(ConfigUpdate::Theme(value))),
        ConfigField::Language => Ok(ConfigAction::Set(ConfigUpdate::Language(value))),
        ConfigField::Timezone => Ok(ConfigAction::Set(ConfigUpdate::Timezone(value))),
        ConfigField::Address => Ok(ConfigAction::Set(ConfigUpdate::Address(value))),
    }
}

fn joined_config_value(args: &[String]) -> Option<String> {
    if args.is_empty() {
        return None;
    }

    let value = args.join(" ");
    if value.trim().is_empty() {
        None
    } else {
        Some(value)
    }
}

fn parse_config_field(field: &str) -> Result<ConfigField, CliError> {
    match field {
        "theme" => Ok(ConfigField::Theme),
        "language" | "locale" => Ok(ConfigField::Language),
        "timezone" | "time-zone" | "tz" => Ok(ConfigField::Timezone),
        "address" | "location" => Ok(ConfigField::Address),
        "user" | "users" | "username" | "password" | "passwd" | "password_hint" => {
            Err(CliError::ForbiddenConfigField(field.to_string()))
        }
        other => Err(CliError::UnsupportedConfigField(other.to_string())),
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
        tundra_weathr::run_blocking_with_options,
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
    Launcher: FnOnce(LaunchOptions) -> Result<(), LaunchError>,
    LaunchError: fmt::Display,
{
    run_with_platform_and_weathr_launcher_and_asset_root(
        args,
        platform,
        stdout,
        stderr,
        weathr_launcher,
        None,
    )
}

#[doc(hidden)]
pub fn run_with_platform_and_asset_root<I, S, Stdout, Stderr>(
    args: I,
    platform: &dyn Platform,
    stdout: &mut Stdout,
    stderr: &mut Stderr,
    asset_root: &Path,
) -> i32
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
    Stdout: Write,
    Stderr: Write,
{
    run_with_platform_and_weathr_launcher_and_asset_root(
        args,
        platform,
        stdout,
        stderr,
        tundra_weathr::run_blocking_with_options,
        Some(asset_root),
    )
}

fn run_with_platform_and_weathr_launcher_and_asset_root<
    I,
    S,
    Stdout,
    Stderr,
    Launcher,
    LaunchError,
>(
    args: I,
    platform: &dyn Platform,
    stdout: &mut Stdout,
    stderr: &mut Stderr,
    weathr_launcher: Launcher,
    asset_root: Option<&Path>,
) -> i32
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
    Stdout: Write,
    Stderr: Write,
    Launcher: FnOnce(LaunchOptions) -> Result<(), LaunchError>,
    LaunchError: fmt::Display,
{
    match parse_args(args) {
        Ok(CliCommand::Config(action)) => run_config(platform, stdout, stderr, action),
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
        Ok(CliCommand::Doctor) => run_doctor(platform, stdout, stderr, asset_root),
        Ok(CliCommand::Weathr) => run_weathr(platform, stderr, weathr_launcher),
        Err(error) => {
            let _ = writeln!(stderr, "ERROR: {error}");
            let _ = write_help(stderr);
            2
        }
    }
}

fn run_weathr<Stderr, Launcher, LaunchError>(
    platform: &dyn Platform,
    stderr: &mut Stderr,
    weathr_launcher: Launcher,
) -> i32
where
    Stderr: Write,
    Launcher: FnOnce(LaunchOptions) -> Result<(), LaunchError>,
    LaunchError: fmt::Display,
{
    match weathr_launcher(weathr_launch_options(platform)) {
        Ok(()) => 0,
        Err(error) => {
            let _ = writeln!(stderr, "ERROR: could not launch weathr: {error}");
            1
        }
    }
}

fn weathr_launch_options(platform: &dyn Platform) -> LaunchOptions {
    let Some(config) = platform
        .app_paths()
        .ok()
        .map(|paths| StorageLayout::from_app_paths(&paths))
        .map(StorageManager::from_layout)
        .and_then(|storage| storage.load_config().ok())
    else {
        return LaunchOptions::default();
    };

    let mut options = LaunchOptions {
        timezone_id: Some(config.timezone.clone()),
        ..LaunchOptions::default()
    };

    if let Some(timezone) = tundra_ui::setup_timezone_options()
        .into_iter()
        .find(|timezone| timezone.id == config.timezone)
    {
        options.location_override = Some(LaunchLocation {
            latitude: timezone.latitude,
            longitude: timezone.longitude,
            city: Some(timezone.label),
        });
    }

    options
}

fn run_config<Stdout: Write, Stderr: Write>(
    platform: &dyn Platform,
    stdout: &mut Stdout,
    stderr: &mut Stderr,
    action: ConfigAction,
) -> i32 {
    let storage = match config_storage(platform) {
        Ok(storage) => storage,
        Err(error) => {
            let _ = writeln!(stderr, "ERROR: {error}");
            return 1;
        }
    };

    let mut config = match load_or_default_config(&storage) {
        Ok(config) => config,
        Err(error) => {
            let _ = writeln!(stderr, "ERROR: could not load config: {error}");
            return 1;
        }
    };

    match action {
        ConfigAction::Get(field) => {
            write_config_value(stdout, &config, field);
            0
        }
        ConfigAction::Set(update) => match apply_config_update(&mut config, update) {
            Ok(message) => match storage.save_config(&config) {
                Ok(()) => {
                    let _ = writeln!(stdout, "{message}");
                    0
                }
                Err(error) => {
                    let _ = writeln!(stderr, "ERROR: could not save config: {error}");
                    1
                }
            },
            Err(error) => {
                let _ = writeln!(stderr, "ERROR: {error}");
                1
            }
        },
    }
}

fn config_storage(platform: &dyn Platform) -> Result<StorageManager, String> {
    platform
        .app_paths()
        .map(|paths| StorageManager::from_layout(StorageLayout::from_app_paths(&paths)))
        .map_err(|error| error.to_string())
}

fn load_or_default_config(storage: &StorageManager) -> Result<StorageConfig, String> {
    if storage.layout().config_path.exists() {
        storage.load_config().map_err(|error| error.to_string())
    } else {
        Ok(StorageConfig::default())
    }
}

fn write_config_value(output: &mut impl Write, config: &StorageConfig, field: Option<ConfigField>) {
    match field {
        Some(ConfigField::Theme) => {
            let _ = writeln!(output, "theme = {}", config.theme);
        }
        Some(ConfigField::Language) => {
            let _ = writeln!(output, "language = {}", config.language);
        }
        Some(ConfigField::Timezone) => {
            let _ = writeln!(output, "timezone = {}", config.timezone);
        }
        Some(ConfigField::Address) => {
            let _ = writeln!(output, "address = {}", config_address_summary(config));
        }
        None => {
            let _ = writeln!(output, "theme = {}", config.theme);
            let _ = writeln!(output, "language = {}", config.language);
            let _ = writeln!(output, "timezone = {}", config.timezone);
            let _ = writeln!(output, "address = {}", config_address_summary(config));
        }
    }
}

fn apply_config_update(config: &mut StorageConfig, update: ConfigUpdate) -> Result<String, String> {
    match update {
        ConfigUpdate::Theme(value) => {
            let value = clean_config_value("theme", value)?;
            config.theme = value.clone();
            Ok(format!("Updated theme: {value}"))
        }
        ConfigUpdate::Language(value) => {
            let language = resolve_language(&value)?;
            config.language = language.code.clone();
            Ok(format!(
                "Updated language: {} ({})",
                language.label, language.code
            ))
        }
        ConfigUpdate::Timezone(value) => {
            let timezone = resolve_timezone(&value)?;
            config.timezone = timezone.id.clone();
            Ok(format!(
                "Updated timezone: {} ({})",
                timezone.label, timezone.id
            ))
        }
        ConfigUpdate::Address(value) => {
            let timezone = resolve_timezone(&value)?;
            config.timezone = timezone.id.clone();
            Ok(format!("Updated address: {}", timezone_summary(&timezone)))
        }
    }
}

fn clean_config_value(name: &str, value: String) -> Result<String, String> {
    let value = value.trim().to_string();
    if value.is_empty() {
        return Err(format!("{name} cannot be empty"));
    }
    if value.chars().any(char::is_control) {
        return Err(format!("{name} cannot contain control characters"));
    }

    Ok(value)
}

fn resolve_language(value: &str) -> Result<tundra_ui::SetupLanguageOption, String> {
    let value = clean_config_value("language", value.to_string())?;
    tundra_ui::setup_language_options()
        .into_iter()
        .find(|language| {
            language.code == value || language.label.eq_ignore_ascii_case(value.as_str())
        })
        .ok_or_else(|| {
            format!(
                "unsupported language {value:?}; available values: {}",
                tundra_ui::setup_language_options()
                    .into_iter()
                    .map(|language| language.code)
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        })
}

fn resolve_timezone(value: &str) -> Result<tundra_ui::SetupTimezoneOption, String> {
    let value = clean_config_value("address", value.to_string())?;
    tundra_ui::setup_timezone_options()
        .into_iter()
        .find(|timezone| {
            timezone.id == value || timezone.label.eq_ignore_ascii_case(value.as_str())
        })
        .ok_or_else(|| {
            format!(
                "unsupported address/timezone {value:?}; available values: {}",
                tundra_ui::setup_timezone_options()
                    .into_iter()
                    .map(|timezone| timezone.id)
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        })
}

fn config_address_summary(config: &StorageConfig) -> String {
    tundra_ui::setup_timezone_options()
        .into_iter()
        .find(|timezone| timezone.id == config.timezone)
        .map(|timezone| timezone_summary(&timezone))
        .unwrap_or_else(|| format!("unmapped timezone ({})", config.timezone))
}

fn timezone_summary(timezone: &tundra_ui::SetupTimezoneOption) -> String {
    format!(
        "{} ({}, {:.4}, {:.4})",
        timezone.label, timezone.id, timezone.latitude, timezone.longitude
    )
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
    asset_root: Option<&Path>,
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
            let asset_theme_id = asset_theme_id_from_storage(storage_check.theme_id.as_deref());
            let asset_check = run_asset_check(asset_root, &asset_theme_id);
            write_asset_check(stdout, &asset_check);

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
            let asset_check = run_asset_check(asset_root, tundra_ascii_assets::DEFAULT_THEME_ID);
            write_asset_check(stdout, &asset_check);
            let _ = writeln!(stderr, "Doctor result: FAIL");
            1
        }
    }
}

fn write_help(output: &mut impl Write) -> std::io::Result<()> {
    writeln!(output, "TundraUX3 CLI")?;
    writeln!(
        output,
        "Usage: tundra-cli <config|doctor|explain|new|paths|weathr>"
    )?;
    writeln!(
        output,
        "  config  View or update user config: get [field], set <theme|language|timezone|address> <value>"
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
        "  2. tundra-cli handles diagnostics, operator commands, config, and launchers: doctor, paths, explain, new, weathr."
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

fn write_asset_check(output: &mut impl Write, check: &AsciiAssetCheck) {
    let _ = writeln!(output);
    let _ = writeln!(output, "Asset checks:");
    let _ = writeln!(
        output,
        "[{}] Required ASCII assets (theme {}): {}",
        check.status.as_str(),
        check.theme_id,
        check.message
    );
    for detail in &check.details {
        let _ = writeln!(output, "  {detail}");
    }
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
    tundra_platform::terminal_environment_check(kind)
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
    theme_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AsciiAssetCheck {
    status: CheckStatus,
    theme_id: String,
    message: String,
    details: Vec<String>,
}

fn run_storage_check(paths: &AppPaths) -> StorageCheck {
    match StorageManager::open(paths.clone()) {
        Ok(opened) => {
            let theme_id = opened.manager.load_config().ok().map(|config| config.theme);
            if opened.report.warnings.is_empty() && opened.report.migrated_files.is_empty() {
                StorageCheck {
                    label: "Storage bootstrap",
                    status: CheckStatus::Pass,
                    message: "storage initialized and loaded cleanly".to_string(),
                    theme_id,
                }
            } else {
                StorageCheck {
                    label: "Storage bootstrap",
                    status: CheckStatus::Warning,
                    message: storage_warning_message(&opened.report),
                    theme_id,
                }
            }
        }
        Err(error) => StorageCheck {
            label: "Storage bootstrap",
            status: CheckStatus::Fail,
            message: error.to_string(),
            theme_id: None,
        },
    }
}

fn run_asset_check(asset_root: Option<&Path>, theme_id: &str) -> AsciiAssetCheck {
    let theme_id = normalized_asset_theme_id(theme_id);
    let root = match asset_root {
        Some(root) => Ok(root.to_path_buf()),
        None => tundra_ascii_assets::asset_root_from_env_or_current_exe(),
    };

    let root = match root {
        Ok(root) => root,
        Err(error) => {
            return AsciiAssetCheck {
                status: CheckStatus::Warning,
                theme_id,
                message: format!("could not resolve asset root: {error}"),
                details: Vec::new(),
            };
        }
    };

    let report = tundra_ascii_assets::check_required_assets(&root, &theme_id);
    if report.is_ok() {
        return AsciiAssetCheck {
            status: CheckStatus::Pass,
            theme_id,
            message: format!(
                "{} assets present and valid at {}",
                report.checks.len(),
                root.display()
            ),
            details: Vec::new(),
        };
    }

    let missing = report.missing_assets();
    let unreadable = report.unreadable_assets();
    let invalid = report.invalid_assets();
    let mut details = Vec::new();
    for check in &missing {
        details.push(format!("missing: {} ({})", check.key, check.path.display()));
    }
    for check in &unreadable {
        details.push(format!(
            "unreadable: {} ({})",
            check.key,
            check.path.display()
        ));
    }
    for check in &invalid {
        details.push(format!(
            "invalid: {} ({}) - {}",
            check.key,
            check.path.display(),
            check.message
        ));
    }

    AsciiAssetCheck {
        status: CheckStatus::Warning,
        theme_id,
        message: format!(
            "{}; {}; {} at {}",
            asset_count_message(missing.len(), "missing"),
            asset_count_message(unreadable.len(), "unreadable"),
            asset_count_message(invalid.len(), "invalid"),
            root.display()
        ),
        details,
    }
}

fn asset_theme_id_from_storage(theme_id: Option<&str>) -> String {
    normalized_asset_theme_id(theme_id.unwrap_or(tundra_ascii_assets::DEFAULT_THEME_ID))
}

fn normalized_asset_theme_id(theme_id: &str) -> String {
    match theme_id.trim() {
        "" | "dark" | "light" => tundra_ascii_assets::DEFAULT_THEME_ID.to_string(),
        other => other.to_string(),
    }
}

fn asset_count_message(count: usize, label: &str) -> String {
    let suffix = if count == 1 { "" } else { "s" };
    format!("{count} {label} asset{suffix}")
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
    removed_paths: Vec<PathBuf>,
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
        .map_err(|error| std::io::Error::other(error.to_string()))?;

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
