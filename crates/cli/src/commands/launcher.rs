use std::{io::Write, path::PathBuf};

use crate::{
    CliError,
    config_command::{config_storage, load_or_default_config},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LauncherAction {
    List,
    Pin(PathBuf),
    Unpin(String),
}

pub(crate) fn parse_launcher(args: &[String]) -> Result<LauncherAction, CliError> {
    match args {
        [] => Ok(LauncherAction::List),
        [verb] if verb == "list" => Ok(LauncherAction::List),
        [verb, path] if verb == "pin" && !path.is_empty() => Ok(LauncherAction::Pin(path.into())),
        [verb, id] if verb == "unpin" && !id.is_empty() => Ok(LauncherAction::Unpin(id.clone())),
        _ => Err(CliError::InvalidLauncherArgument(
            "expected launcher list, pin <path>, or unpin <id>".into(),
        )),
    }
}

pub(crate) fn run_launcher(
    platform: &dyn platform::Platform,
    stdout: &mut impl Write,
    stderr: &mut impl Write,
    action: LauncherAction,
) -> i32 {
    let result = (|| -> Result<(), String> {
        let storage = config_storage(platform)?;
        let mut config = load_or_default_config(&storage)?;
        match action {
            LauncherAction::List => {
                if config.launcher.entries.is_empty() {
                    let _ = writeln!(
                        stdout,
                        "No pinned applications. Use launcher pin <path> to add one."
                    );
                }
                for entry in &config.launcher.entries {
                    let status = app::launcher::verify_launcher_entry(entry, platform)
                        .map_err(|error| error.to_string())?;
                    let _ = writeln!(stdout, "{}  {status:?}  {:?}", entry.id, entry.path);
                }
                return Ok(());
            }
            LauncherAction::Pin(path) => {
                // Keep the last path component intact so the shared validator
                // can reject symlinks before canonicalizing the target.
                let path = if path.is_absolute() {
                    path
                } else {
                    std::env::current_dir()
                        .map_err(|error| error.to_string())?
                        .join(path)
                };
                let record =
                    app::launcher::prepare_launcher_entry(&config.launcher, &path, "cli", platform)
                        .map_err(|error| error.to_string())?;
                let Some(record) = record else {
                    let _ = writeln!(stdout, "Already pinned; no changes.");
                    return Ok(());
                };
                let id = record.id.clone();
                config.launcher.entries.push(record);
                storage
                    .save_config(&config)
                    .map_err(|error| error.to_string())?;
                let _ = writeln!(stdout, "Pinned application: {id}");
            }
            LauncherAction::Unpin(id) => {
                let index = config
                    .launcher
                    .entries
                    .iter()
                    .position(|entry| entry.id == id)
                    .ok_or_else(|| format!("unknown Launcher id {id:?}; run launcher list"))?;
                config.launcher.entries.remove(index);
                storage
                    .save_config(&config)
                    .map_err(|error| error.to_string())?;
                let _ = writeln!(
                    stdout,
                    "Removed pin: {id}. The application file was not deleted."
                );
            }
        }
        let _ = writeln!(
            stdout,
            "Restart TundraUX3 to refresh an already running Launcher."
        );
        Ok(())
    })();
    match result {
        Ok(()) => 0,
        Err(error) => {
            let _ = writeln!(stderr, "ERROR: {error}");
            1
        }
    }
}
