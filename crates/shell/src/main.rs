use std::sync::Arc;
use watchdog::{ProcessWatchdog, WatchdogConfig, WatchdogRuntime};

fn main() {
    if let Err(error) = shell::parse_shell_args(std::env::args().skip(1)) {
        eprintln!("tundra-shell failed: {error}");
        std::process::exit(2);
    }

    let (watchdog_runtime, process_watchdog) = match start_watchdog() {
        Ok(value) => value,
        Err(error) => {
            eprintln!("tundra-shell watchdog failed to start: {error}");
            std::process::exit(3);
        }
    };
    let _ =
        process_watchdog.register_emergency_cleanup(Arc::new(shell::restore_terminal_best_effort));

    let mut stdout = std::io::stdout();
    let run_result = shell::run_shell_blocking_managed_with_outcome(&mut stdout, process_watchdog);
    drop(stdout);
    let watchdog_shutdown = watchdog_runtime.shutdown();

    let exit_code = match (run_result, watchdog_shutdown) {
        (Ok(shell::ShellRunOutcome::Exit), Ok(())) => 0,
        (Ok(shell::ShellRunOutcome::ResetRequested), Ok(())) => match reset_storage_and_restart() {
            Ok(()) => 0,
            Err(error) => {
                eprintln!("tundra-shell reset failed: {error}");
                4
            }
        },
        (_, Err(error)) => {
            eprintln!("tundra-shell watchdog shutdown failed: {error}");
            3
        }
        (Err(error), Ok(())) => {
            eprintln!("tundra-shell failed: {error}");
            1
        }
    };

    if exit_code != 0 {
        std::process::exit(exit_code);
    }
}

fn reset_storage_and_restart() -> Result<(), std::io::Error> {
    let platform = platform::native_platform();
    let paths = platform
        .app_paths()
        .map_err(|error| std::io::Error::other(error.to_string()))?;
    storage::reset_saved_content(&paths)?;

    let executable = std::env::current_exe()?;
    std::process::Command::new(&executable)
        .spawn()
        .map_err(|error| {
            std::io::Error::new(
                error.kind(),
                format!("could not restart {}: {error}", executable.display()),
            )
        })?;
    Ok(())
}

fn start_watchdog() -> Result<(WatchdogRuntime, ProcessWatchdog), watchdog::WatchdogError> {
    let fallback = std::env::temp_dir().join("TundraUX3").join("watchdog");
    let platform = platform::native_platform();
    let config = match platform.app_paths() {
        Ok(paths) => WatchdogConfig::new(
            paths.logs_path().join("crashes"),
            fallback.join("crashes"),
            paths.data_path(),
            "tundra-shell",
            env!("CARGO_PKG_VERSION"),
        ),
        Err(_) => WatchdogConfig::new(
            fallback.join("crashes"),
            fallback.join("fallback"),
            fallback.join("state"),
            "tundra-shell",
            env!("CARGO_PKG_VERSION"),
        ),
    };
    let (runtime, process) = WatchdogRuntime::start(config)?;
    let process = process.install_global()?;
    let _ = process.report_stale_runs(|pid| platform.is_process_alive(pid).unwrap_or(true));
    Ok((runtime, process))
}
