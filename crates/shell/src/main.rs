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
    let exit_code = match shell::run_shell_blocking_managed(&mut stdout, process_watchdog) {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("tundra-shell failed: {error}");
            1
        }
    };

    let _ = watchdog_runtime.shutdown();
    if exit_code != 0 {
        std::process::exit(exit_code);
    }
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
