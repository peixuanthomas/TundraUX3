use std::fmt;
use std::io::Write;
use std::path::Path;

use tundra_platform::Platform;
use tundra_watchdog::{AppWatchdog, ProcessWatchdog};
use tundra_weathr::LaunchOptions;

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
    let platform = tundra_platform::native_platform();
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
    let platform = tundra_platform::native_platform();
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
        tundra_weathr::run_blocking_managed,
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
    Launcher: FnOnce(LaunchOptions, AppWatchdog) -> Result<(), tundra_weathr::WeathrRunError>,
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
        Ok(CliCommand::Editor) => run_editor(stderr, || {
            tundra_shell::run_shell_blocking_managed(
                stdout,
                tundra_shell::ShellLaunchConfig::editor(),
                process_watchdog.clone(),
            )
        }),
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
        Ok(CliCommand::Editor) => run_editor(stderr, || {
            tundra_shell::run_shell_blocking(stdout, tundra_shell::ShellLaunchConfig::editor())
        }),
        Ok(CliCommand::Weathr) => run_weathr(platform, stderr, weathr_launcher),
        Err(error) => {
            let _ = writeln!(stderr, "ERROR: {error}");
            let _ = write_help(stderr);
            2
        }
    }
}

fn run_editor<Stderr, Launcher, LaunchError>(stderr: &mut Stderr, launcher: Launcher) -> i32
where
    Stderr: Write,
    Launcher: FnOnce() -> Result<(), LaunchError>,
    LaunchError: fmt::Display,
{
    match launcher() {
        Ok(()) => 0,
        Err(error) => {
            let _ = writeln!(stderr, "ERROR: could not launch editor: {error}");
            1
        }
    }
}
