use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CliCommand {
    Config(ConfigAction),
    Doctor,
    Editor,
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
        "editor" => parse_no_extra_args(&args, CliCommand::Editor),
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
