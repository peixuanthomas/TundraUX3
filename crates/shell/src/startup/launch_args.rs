#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellLaunchCommand {
    Run,
    Help,
    Version,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShellArgError {
    ArgumentNotAllowed(String),
}

impl std::fmt::Display for ShellArgError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ArgumentNotAllowed(argument) => {
                write!(
                    formatter,
                    "unsupported tundra-shell argument: {argument}; use --help for usage"
                )
            }
        }
    }
}

impl std::error::Error for ShellArgError {}

/// Validates the `tundra-shell` process boundary.
///
/// Only help and version are public arguments; no arguments starts the UI.
pub fn parse_shell_args<I, S>(args: I) -> Result<ShellLaunchCommand, ShellArgError>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut args = args.into_iter();
    let Some(argument) = args.next() else {
        return Ok(ShellLaunchCommand::Run);
    };
    let command = match argument.as_ref() {
        "-h" | "--help" => ShellLaunchCommand::Help,
        "-V" | "--version" => ShellLaunchCommand::Version,
        other => return Err(ShellArgError::ArgumentNotAllowed(other.to_string())),
    };
    if let Some(extra) = args.next() {
        return Err(ShellArgError::ArgumentNotAllowed(
            extra.as_ref().to_string(),
        ));
    }
    Ok(command)
}
