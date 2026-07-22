use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CliCommand {
    Config(ConfigAction),
    Doctor,
    Explain,
    New,
    Paths,
    TestFrost,
    TestMatrix,
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
    ForbiddenConfigField(String),
    MissingArgument(&'static str),
    ReadOnlyConfigField(String),
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
            Self::ReadOnlyConfigField(field) => write!(
                formatter,
                "config field {field:?} is a read-only summary; set border-shape, border-color, or accent-color instead"
            ),
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
        "test-frost" => parse_no_extra_args(&args, CliCommand::TestFrost),
        "test-matrix" => parse_no_extra_args(&args, CliCommand::TestMatrix),
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
