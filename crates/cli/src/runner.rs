use std::fmt;
use std::io::Write;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use platform::Platform;
use storage::{BorderColor, StorageConfig, StorageLayout, StorageManager};
use watchdog::{AppWatchdog, ProcessWatchdog};

use crate::arguments::{CliCommand, parse_args};
use crate::asset_command::run_asset;
use crate::config_command::run_config;
use crate::doctor::run_doctor;
use crate::help_text::{write_explain, write_help};
use crate::path_report::run_paths;
use crate::storage_reset::run_new;
use crate::weathr_command::{
    WeathrLaunchOptions, drain_watchdog_incidents, run_weathr, run_weathr_managed,
};

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
    let args = args
        .into_iter()
        .map(|argument| argument.as_ref().to_string())
        .collect::<Vec<_>>();
    if let Ok(CliCommand::Repl { embedded }) = parse_args(&args) {
        return crate::repl::run_repl(embedded, |command| {
            run_with_platform_and_managed_weathr_launcher(
                command,
                platform,
                stdout,
                stderr,
                process_watchdog,
                weathr_watchdog.clone(),
                launch_weathr_managed,
            )
        });
    }
    run_with_platform_and_managed_weathr_launcher(
        args,
        platform,
        stdout,
        stderr,
        process_watchdog,
        weathr_watchdog,
        launch_weathr_managed,
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
    let args = args
        .into_iter()
        .map(|argument| argument.as_ref().to_string())
        .collect::<Vec<_>>();
    if let Ok(CliCommand::Repl { embedded }) = parse_args(&args) {
        return crate::repl::run_repl(embedded, |command| {
            run_with_platform_and_weathr_launcher(command, platform, stdout, stderr, launch_weathr)
        });
    }
    run_with_platform_and_weathr_launcher(args, platform, stdout, stderr, launch_weathr)
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
    Launcher: FnOnce(WeathrLaunchOptions) -> Result<(), LaunchError>,
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
    Launcher: FnOnce(WeathrLaunchOptions, AppWatchdog) -> Result<(), weathr::WeathrRunError>,
{
    let mut routed_by_weathr = false;
    let exit_code = match parse_args(args) {
        Ok(CliCommand::Asset(action)) => run_asset(stdout, stderr, action, None),
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
        launch_weathr,
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
    Launcher: FnOnce(WeathrLaunchOptions) -> Result<(), LaunchError>,
    LaunchError: fmt::Display,
{
    match parse_args(args) {
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
        Ok(CliCommand::Weathr) => run_weathr(platform, stderr, weathr_launcher),
        Err(error) => {
            let _ = writeln!(stderr, "ERROR: {error}");
            let _ = write_help(stderr);
            2
        }
    }
}

fn launch_weathr(options: WeathrLaunchOptions) -> Result<(), weathr::WeathrRunError> {
    let watchdog = ProcessWatchdog::global()
        .ok_or(weathr::WeathrRunError::WatchdogUnavailable)?
        .register_app(weathr::weathr_watchdog_descriptor())
        .map_err(weathr::WeathrRunError::Watchdog)?;
    launch_weathr_managed(options, watchdog)
}

fn launch_weathr_managed(
    options: WeathrLaunchOptions,
    watchdog: AppWatchdog,
) -> Result<(), weathr::WeathrRunError> {
    let mut services_config = system_services::SystemServicesConfig::default();
    services_config.weather_location = options.location_query;
    if let Some(timezone_id) = options.timezone_id {
        services_config.timezone_id = timezone_id;
    }
    services_config.timezone_location =
        options
            .location_override
            .map(|location| system_services::GeoLocation {
                latitude: location.latitude,
                longitude: location.longitude,
                city: location.city,
            });

    let (services, snapshots) =
        system_services::SystemServicesRuntime::start(services_config, watchdog.clone());
    let input = weathr::WeathrDisplayInput {
        snapshots,
        clock_format: weathr::ClockFormat::TwentyFourHour,
        hide_hud: false,
        palette: weathr::theme::catalogue::DEFAULT_PALETTE,
        shutdown: Arc::new(AtomicBool::new(false)),
        minimum_terminal_size: options.minimum_terminal_size,
    };
    let result = weathr::run_display_blocking(input, watchdog).map(|_| ());
    let _ = services.shutdown();
    result
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
