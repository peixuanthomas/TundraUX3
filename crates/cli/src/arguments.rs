use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CliCommand {
    Logs(crate::logs_command::LogsAction),
    Asset(AssetAction),
    Cls,
    Config(ConfigAction),
    Doctor,
    Explain,
    New,
    Paths,
    /// Starts the interactive command line. `embedded` is reserved for the
    /// Launcher-hosted terminal and is intentionally not advertised in help.
    Repl {
        embedded: bool,
    },
    TestFrost,
    TestMatrix,
    ViewUiStyle(shell::UiStyleVersion),
    UiStyleHelp,
    DebugHelp,
    TestWatchdogError,
    TestWatchdogCritical,
    TestWatchdogPanic,
    Help,
    #[doc(hidden)]
    UpdateProbe,
    #[doc(hidden)]
    ApplyUpdate {
        manifest: std::path::PathBuf,
        parent_pid: u32,
        recover_only: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AssetAction {
    Help,
    Show { name: String, output: AssetOutput },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AssetOutput {
    RenderAll,
    Source,
    Item(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigAction {
    Get(Option<ConfigField>),
    Set(ConfigUpdate),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigField {
    Theme,
    BorderShape,
    BorderColor,
    AccentColor,
    Language,
    Timezone,
    Address,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigUpdate {
    BorderShape(String),
    BorderColor(String),
    AccentColor(String),
    Language(String),
    Timezone(String),
    Address(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CliError {
    InvalidLogsArgument(String),
    ForbiddenConfigField(String),
    MissingArgument(&'static str),
    ReadOnlyConfigField(String),
    UnknownCommand(String),
    UnknownDebugCommand(String),
    UnknownConfigCommand(String),
    UnsupportedConfigField(String),
    UnexpectedArgument(String),
    InvalidReplArgument(String),
    InvalidProcessId(String),
    InvalidUiStyle(String),
}

impl fmt::Display for CliError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidLogsArgument(message) => {
                write!(formatter, "invalid logs arguments: {message}")
            }
            Self::ForbiddenConfigField(field) => {
                write!(
                    formatter,
                    "config field {field:?} is not exposed; username and password changes must use authenticated user management"
                )
            }
            Self::MissingArgument(argument) => write!(formatter, "missing argument: {argument}"),
            Self::ReadOnlyConfigField(field) => write!(
                formatter,
                "config field {field:?} is a read-only summary; set border-shape, border-color, or accent-color instead"
            ),
            Self::UnknownCommand(command) => write!(formatter, "unknown command: {command}"),
            Self::UnknownDebugCommand(command) => write!(
                formatter,
                "unknown debug command: {command}; run debug help"
            ),
            Self::UnknownConfigCommand(command) => {
                write!(formatter, "unknown config command: {command}")
            }
            Self::UnsupportedConfigField(field) => {
                write!(formatter, "unsupported config field: {field}")
            }
            Self::UnexpectedArgument(argument) => {
                write!(formatter, "unexpected argument: {argument}")
            }
            Self::InvalidReplArgument(argument) => {
                write!(formatter, "unsupported repl argument: {argument}")
            }
            Self::InvalidProcessId(value) => write!(formatter, "invalid process id: {value}"),
            Self::InvalidUiStyle(value) => write!(
                formatter,
                "unknown UI style: {value}; use 1, 2, or 3 (debug view-ui-style for details)"
            ),
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
        "logs" => crate::logs_command::parse_logs(&args).map(CliCommand::Logs),
        "debug" => parse_debug_args(&args),
        "cls" => parse_no_extra_args(&args, CliCommand::Cls),
        "config" => parse_config_args(&args).map(CliCommand::Config),
        "new" => parse_no_extra_args(&args, CliCommand::New),
        "repl" => parse_repl_args(&args),
        "__update-probe" => parse_no_extra_args(&args, CliCommand::UpdateProbe),
        "__apply-update" => parse_internal_update_args(&args, false),
        "__recover-update" => parse_internal_update_args(&args, true),
        "-h" | "--help" | "help" => Ok(CliCommand::Help),
        other => Err(CliError::UnknownCommand(other.to_string())),
    }
}

fn parse_debug_args(args: &[String]) -> Result<CliCommand, CliError> {
    let Some((command, rest)) = args.split_first() else {
        return Ok(CliCommand::DebugHelp);
    };
    let command = match command.as_str() {
        "help" | "-h" | "--help" => CliCommand::DebugHelp,
        "asset" => return parse_asset_args(rest).map(CliCommand::Asset),
        "doctor" => CliCommand::Doctor,
        "paths" => CliCommand::Paths,
        "explain" => CliCommand::Explain,
        "test-frost" => CliCommand::TestFrost,
        "test-matrix" => CliCommand::TestMatrix,
        "view-ui-style" => return parse_ui_style_args(rest),
        "test-watchdog-error" => CliCommand::TestWatchdogError,
        "test-watchdog-critical" => CliCommand::TestWatchdogCritical,
        "test-watchdog-panic" => CliCommand::TestWatchdogPanic,
        other => return Err(CliError::UnknownDebugCommand(other.to_string())),
    };
    parse_no_extra_args(rest, command)
}

fn parse_ui_style_args(args: &[String]) -> Result<CliCommand, CliError> {
    match args {
        [] => Ok(CliCommand::UiStyleHelp),
        [help] if matches!(help.as_str(), "help" | "-h" | "--help") => Ok(CliCommand::UiStyleHelp),
        [version] => shell::UiStyleVersion::ALL
            .into_iter()
            .find(|style| style.number().to_string() == *version)
            .map(CliCommand::ViewUiStyle)
            .ok_or_else(|| CliError::InvalidUiStyle(version.clone())),
        [_, extra, ..] => Err(CliError::UnexpectedArgument(extra.clone())),
    }
}

fn parse_internal_update_args(args: &[String], recover_only: bool) -> Result<CliCommand, CliError> {
    let [manifest, parent_pid] = args else {
        return Err(CliError::MissingArgument(
            "update manifest and parent process id",
        ));
    };
    let parent_pid = parent_pid
        .parse::<u32>()
        .map_err(|_| CliError::InvalidProcessId(parent_pid.clone()))?;
    Ok(CliCommand::ApplyUpdate {
        manifest: std::path::PathBuf::from(manifest),
        parent_pid,
        recover_only,
    })
}

fn parse_asset_args(args: &[String]) -> Result<AssetAction, CliError> {
    match args {
        [] => Ok(AssetAction::Help),
        [help] if matches!(help.as_str(), "-h" | "--help" | "help") => Ok(AssetAction::Help),
        [option, ..] if option.starts_with('-') => Err(CliError::MissingArgument("asset name")),
        [name] => Ok(AssetAction::Show {
            name: name.clone(),
            output: AssetOutput::RenderAll,
        }),
        [name, option] if option == "-a" => Ok(AssetAction::Show {
            name: name.clone(),
            output: AssetOutput::Source,
        }),
        [name, option] if option.starts_with("--") && option.len() > 2 => Ok(AssetAction::Show {
            name: name.clone(),
            output: AssetOutput::Item(option[2..].to_string()),
        }),
        [_, option, unexpected, ..]
            if option == "-a" || (option.starts_with("--") && option.len() > 2) =>
        {
            Err(CliError::UnexpectedArgument(unexpected.clone()))
        }
        [_, unexpected, ..] => Err(CliError::UnexpectedArgument(unexpected.clone())),
    }
}

fn parse_repl_args(args: &[String]) -> Result<CliCommand, CliError> {
    match args {
        [] => Ok(CliCommand::Repl { embedded: false }),
        [flag] if flag == "--embedded" => Ok(CliCommand::Repl { embedded: true }),
        [argument, ..] => Err(CliError::InvalidReplArgument(argument.clone())),
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
        ConfigField::Theme => Err(CliError::ReadOnlyConfigField(field.clone())),
        ConfigField::BorderShape => Ok(ConfigAction::Set(ConfigUpdate::BorderShape(value))),
        ConfigField::BorderColor => Ok(ConfigAction::Set(ConfigUpdate::BorderColor(value))),
        ConfigField::AccentColor => Ok(ConfigAction::Set(ConfigUpdate::AccentColor(value))),
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
        "border-shape" | "border_shape" => Ok(ConfigField::BorderShape),
        "border-color" | "border_color" => Ok(ConfigField::BorderColor),
        "accent-color" | "accent_color" => Ok(ConfigField::AccentColor),
        "language" | "locale" => Ok(ConfigField::Language),
        "timezone" | "time-zone" | "tz" => Ok(ConfigField::Timezone),
        "address" | "location" => Ok(ConfigField::Address),
        "user" | "users" | "username" | "password" | "passwd" | "password_hint" => {
            Err(CliError::ForbiddenConfigField(field.to_string()))
        }
        other => Err(CliError::UnsupportedConfigField(other.to_string())),
    }
}
