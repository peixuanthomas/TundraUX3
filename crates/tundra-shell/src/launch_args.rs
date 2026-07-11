use crate::{HomeModeOverride, ShellLaunchConfig, ShellTerminalMode};

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
