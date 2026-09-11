use std::fmt;
use std::io::Write;
use std::path::Path;

use platform::Platform;
use storage::{BorderColor, StorageConfig, StorageLayout, StorageManager};
use watchdog::{AppWatchdog, ProcessWatchdog};

use crate::arguments::{CliCommand, parse_args};
use crate::asset_command::run_asset;
use crate::config_command::run_config;
use crate::debug_command::{drain_watchdog_incidents, run_watchdog_test};
use crate::doctor::run_doctor;
use crate::help_text::{write_debug_help, write_explain, write_help, write_ui_style_help};
use crate::path_report::run_paths;
use crate::storage_reset::run_new;

const CLEAR_TERMINAL_SEQUENCE: &[u8] = b"\x1b[3J\x1b[2J\x1b[H";

pub fn run<I, S, Stdout, Stderr>(args: I, stdout: &mut Stdout, stderr: &mut Stderr) -> i32
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
    Stdout: Write,
    Stderr: Write,
{
    let platform = platform::native_platform();
    run_with_platform(args, platform.as_ref(), stdout, stderr)
}

pub fn run_managed<I, S, Stdout, Stderr>(
    args: I,
    process_watchdog: &ProcessWatchdog,
    cli_watchdog: AppWatchdog,
    stdout: &mut Stdout,
    stderr: &mut Stderr,
) -> i32
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
    Stdout: Write,
    Stderr: Write,
{
    let platform = platform::native_platform();
    run_with_platform_and_watchdog(
        args,
        platform.as_ref(),
        stdout,
        stderr,
        process_watchdog,
        cli_watchdog,
    )
}

pub fn run_with_platform_and_watchdog<I, S, Stdout, Stderr>(
    args: I,
    platform: &dyn Platform,
    stdout: &mut Stdout,
    stderr: &mut Stderr,
    process_watchdog: &ProcessWatchdog,
    cli_watchdog: AppWatchdog,
) -> i32
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
    Stdout: Write,
    Stderr: Write,
{
    let args = args
        .into_iter()
        .map(|arg| arg.as_ref().to_string())
        .collect::<Vec<_>>();
    let mut execute = |command: &[String]| {
        let code = dispatch(
            command,
            platform,
            stdout,
            stderr,
            None,
            Some((process_watchdog, &cli_watchdog)),
        );
        drain_watchdog_incidents(process_watchdog, stderr);
        code
    };
    if let Ok(CliCommand::Repl { embedded }) = parse_args(&args) {
        crate::repl::run_repl(embedded, execute)
    } else {
        execute(&args)
    }
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
    let args = args
        .into_iter()
        .map(|arg| arg.as_ref().to_string())
        .collect::<Vec<_>>();
    if let Ok(CliCommand::Repl { embedded }) = parse_args(&args) {
        return crate::repl::run_repl(embedded, |command| {
            dispatch(command, platform, stdout, stderr, None, None)
        });
    }
    dispatch(args, platform, stdout, stderr, None, None)
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
    dispatch(args, platform, stdout, stderr, Some(asset_root), None)
}

fn dispatch<I, S, Stdout, Stderr>(
    args: I,
    platform: &dyn Platform,
    stdout: &mut Stdout,
    stderr: &mut Stderr,
    asset_root: Option<&Path>,
    managed: Option<(&ProcessWatchdog, &AppWatchdog)>,
) -> i32
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
    Stdout: Write,
    Stderr: Write,
{
    match parse_args(args) {
        Ok(
            command @ (CliCommand::System(_)
            | CliCommand::Session(_)
            | CliCommand::MigrateLegacy(_)),
        ) => crate::system_command::run(command, stdout, stderr),
        Ok(CliCommand::Logs(action)) => {
            crate::logs_command::run_logs(platform, stdout, stderr, action)
        }
        Ok(CliCommand::Asset(action)) => run_asset(stdout, stderr, action, asset_root),
        Ok(CliCommand::Cls) => run_cls(stdout, stderr),
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
        Ok(CliCommand::Repl { .. }) => {
            let _ = writeln!(stderr, "ERROR: repl cannot be started from inside repl");
            2
        }
        Ok(CliCommand::Paths) => run_paths(platform, stdout, stderr),
        Ok(CliCommand::Doctor) => run_doctor(platform, stdout, stderr, asset_root),
        Ok(CliCommand::TestFrost) => {
            run_configured_animation_preview(platform, stderr, "frost", |color| {
                shell::run_frost_animation_preview_with_color(stdout, color)
            })
        }
        Ok(CliCommand::TestMatrix) => {
            run_configured_animation_preview(platform, stderr, "Matrix", |color| {
                shell::run_matrix_animation_preview_with_color(stdout, color)
            })
        }
        Ok(CliCommand::UiStyleHelp) => match write_ui_style_help(stdout) {
            Ok(()) => 0,
            Err(error) => {
                let _ = writeln!(stderr, "ERROR: could not print UI style catalogue: {error}");
                1
            }
        },
        Ok(CliCommand::ViewUiStyle(version)) => run_animation_preview(stderr, "UI style", || {
            let paths = platform.app_paths().map_err(std::io::Error::other)?;
            let storage = StorageManager::from_layout(StorageLayout::from_app_paths(&paths));
            let appearance = if storage.layout().config_path.exists() {
                storage
                    .load_config()
                    .map_err(std::io::Error::other)?
                    .appearance
            } else {
                StorageConfig::default().appearance
            };
            shell::run_ui_style_preview(stdout, version, &appearance)
        }),
        Ok(CliCommand::DebugHelp) => {
            let _ = write_debug_help(stdout);
            0
        }
        Ok(
            command @ (CliCommand::TestWatchdogError
            | CliCommand::TestWatchdogCritical
            | CliCommand::TestWatchdogPanic),
        ) => run_watchdog_test(command, managed, stdout, stderr),
        Ok(CliCommand::UpdateProbe) => write_update_probe(stdout),
        Ok(CliCommand::ApplyUpdate {
            manifest,
            parent_pid,
            recover_only,
        }) => run_update_helper(&manifest, parent_pid, recover_only, stderr),
        Err(error) => {
            let _ = writeln!(stderr, "ERROR: {error}");
            let _ = write_help(stderr);
            2
        }
    }
}

fn write_update_probe(output: &mut impl Write) -> i32 {
    let identity = app::update::current_build_identity();
    let commit = identity.commit_sha.as_deref().unwrap_or("unknown");
    let dirty = if identity.dirty { "dirty" } else { "clean" };
    let _ = writeln!(
        output,
        "protocol={}\nversion={}\ncommit={}\nstate={}",
        app::update::UPDATE_PROTOCOL_VERSION,
        identity.package_version,
        commit,
        dirty
    );
    0
}

fn run_update_helper(
    manifest: &Path,
    parent_pid: u32,
    recover_only: bool,
    stderr: &mut impl Write,
) -> i32 {
    match app::update::apply_update_transaction(manifest, parent_pid, recover_only) {
        Ok(()) => 0,
        Err(error) => {
            let _ = platform::native_platform()
                .show_critical_error("TundraUX update recovery failed", &error.to_string());
            let _ = writeln!(stderr, "ERROR: {error}");
            1
        }
    }
}

fn run_cls<Stdout: Write, Stderr: Write>(stdout: &mut Stdout, stderr: &mut Stderr) -> i32 {
    match stdout
        .write_all(CLEAR_TERMINAL_SEQUENCE)
        .and_then(|()| stdout.flush())
    {
        Ok(()) => 0,
        Err(error) => {
            let _ = writeln!(stderr, "ERROR: could not clear terminal screen: {error}");
            1
        }
    }
}

fn run_animation_preview<Stderr, Launcher, LaunchError>(
    stderr: &mut Stderr,
    name: &str,
    launcher: Launcher,
) -> i32
where
    Stderr: Write,
    Launcher: FnOnce() -> Result<(), LaunchError>,
    LaunchError: fmt::Display,
{
    match launcher() {
        Ok(()) => 0,
        Err(error) => {
            let _ = writeln!(stderr, "ERROR: could not play {name} animation: {error}");
            1
        }
    }
}

fn run_configured_animation_preview<Stderr, Launcher, LaunchError>(
    platform: &dyn Platform,
    stderr: &mut Stderr,
    name: &str,
    launcher: Launcher,
) -> i32
where
    Stderr: Write,
    Launcher: FnOnce(BorderColor) -> Result<(), LaunchError>,
    LaunchError: fmt::Display,
{
    let color = match configured_border_color(platform) {
        Ok(color) => color,
        Err(error) => {
            let _ = writeln!(
                stderr,
                "ERROR: could not load theme for {name} preview: {error}"
            );
            return 1;
        }
    };
    run_animation_preview(stderr, name, || launcher(color))
}

fn configured_border_color(platform: &dyn Platform) -> Result<BorderColor, String> {
    let paths = platform.app_paths().map_err(|error| error.to_string())?;
    let storage = StorageManager::from_layout(StorageLayout::from_app_paths(&paths));
    if !storage.layout().config_path.exists() {
        return Ok(StorageConfig::default().appearance.border_color);
    }
    storage
        .load_config()
        .map(|config| config.appearance.border_color)
        .map_err(|error| error.to_string())
}
