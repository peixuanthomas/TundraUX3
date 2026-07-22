use std::fmt;
use std::io::Write;
use std::path::Path;

use platform::Platform;
use storage::{BorderColor, StorageConfig, StorageLayout, StorageManager};
use watchdog::{AppWatchdog, ProcessWatchdog};
use weathr::LaunchOptions;

use crate::arguments::{CliCommand, parse_args};
use crate::config_command::run_config;
use crate::doctor::run_doctor;
use crate::help_text::{write_explain, write_help};
use crate::path_report::run_paths;
use crate::storage_reset::run_new;
use crate::weathr_command::{drain_watchdog_incidents, run_weathr, run_weathr_managed};

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
    weathr_watchdog: AppWatchdog,
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
        weathr_watchdog,
    )
}

pub fn run_with_platform_and_watchdog<I, S, Stdout, Stderr>(
    args: I,
    platform: &dyn Platform,
    stdout: &mut Stdout,
    stderr: &mut Stderr,
    process_watchdog: &ProcessWatchdog,
    weathr_watchdog: AppWatchdog,
) -> i32
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
    Stdout: Write,
    Stderr: Write,
{
    run_with_platform_and_managed_weathr_launcher(
        args,
        platform,
        stdout,
        stderr,
        process_watchdog,
        weathr_watchdog,
        weathr::run_blocking_managed,
    )
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
        weathr::run_blocking_with_options,
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
pub fn run_with_platform_and_managed_weathr_launcher<I, S, Stdout, Stderr, Launcher>(
    args: I,
    platform: &dyn Platform,
    stdout: &mut Stdout,
    stderr: &mut Stderr,
    process_watchdog: &ProcessWatchdog,
    weathr_watchdog: AppWatchdog,
    weathr_launcher: Launcher,
) -> i32
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
    Stdout: Write,
    Stderr: Write,
    Launcher: FnOnce(LaunchOptions, AppWatchdog) -> Result<(), weathr::WeathrRunError>,
{
    let mut routed_by_weathr = false;
    let exit_code = match parse_args(args) {
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
        Ok(CliCommand::Doctor) => run_doctor(platform, stdout, stderr, None),
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
        Ok(CliCommand::Weathr) => {
            routed_by_weathr = true;
            run_weathr_managed(
                platform,
                stderr,
                process_watchdog,
                weathr_watchdog,
                weathr_launcher,
            )
        }
        Err(error) => {
            let _ = writeln!(stderr, "ERROR: {error}");
            let _ = write_help(stderr);
            2
        }
    };
    if !routed_by_weathr {
        let _ = drain_watchdog_incidents(process_watchdog, stderr);
    }
    exit_code
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
        weathr::run_blocking_with_options,
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
        Ok(CliCommand::Weathr) => run_weathr(platform, stderr, weathr_launcher),
        Err(error) => {
            let _ = writeln!(stderr, "ERROR: {error}");
            let _ = write_help(stderr);
            2
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
