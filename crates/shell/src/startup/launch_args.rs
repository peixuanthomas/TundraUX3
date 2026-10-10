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
                    "tundra-shell does not accept arguments: {argument}"
                )
            }
        }
    }
}

impl std::error::Error for ShellArgError {}

/// Validates the `tundra-shell` process boundary.
///
/// Shell functionality is exposed through its UI, so every command-line
/// argument—including help and former launch flags—is rejected.
pub fn parse_shell_args<I, S>(args: I) -> Result<(), ShellArgError>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    if let Some(argument) = args.into_iter().next() {
        return Err(ShellArgError::ArgumentNotAllowed(
            argument.as_ref().to_string(),
        ));
    }
    Ok(())
}
