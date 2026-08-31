use std::sync::Arc;
use watchdog::{ProcessWatchdog, WatchdogConfig, WatchdogRuntime};

fn main() {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.as_slice() == ["__update-probe"] {
        let identity = app::update::current_build_identity();
        println!("protocol={}", app::update::UPDATE_PROTOCOL_VERSION);
        println!("version={}", identity.package_version);
        println!(
            "commit={}",
            identity.commit_sha.as_deref().unwrap_or("unknown")
        );
        println!("dirty={}", identity.dirty);
        return;
    }
    if let Err(error) = shell::parse_shell_args(args) {
        eprintln!("tundra-shell failed: {error}");
        std::process::exit(2);
    }

    if std::env::var_os(app::update::UPDATE_READY_FILE_ENV).is_none() {
        match app::update::recover_interrupted_update_from_current_exe(std::process::id()) {
            Ok(true) => return,
            Ok(false) => {}
            Err(error) => {
                let platform = platform::native_platform();
                let _ = platform
                    .show_critical_error("TundraUX update recovery failed", &error.to_string());
                eprintln!("tundra-shell update recovery failed: {error}");
                std::process::exit(5);
            }
        }
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

    let run_result = {
        let mut stdout = std::io::stdout();
        shell::run_shell_blocking_managed_with_outcome(&mut stdout, process_watchdog)
    };
    let watchdog_shutdown = watchdog_runtime.shutdown();

    let exit_code = match (run_result, watchdog_shutdown) {
        (Ok(shell::ShellRunOutcome::Exit), Ok(())) => 0,
        (Ok(shell::ShellRunOutcome::RestartRequested), Ok(())) => {
            match restart_current_executable() {
                Ok(()) => 0,
                Err(error) => {
                    eprintln!("tundra-shell restart failed: {error}");
                    4
                }
            }
        }
        (Ok(shell::ShellRunOutcome::ResetRequested), Ok(())) => match reset_storage_and_restart() {
            Ok(()) => 0,
            Err(error) => {
                eprintln!("tundra-shell reset failed: {error}");
                4
            }
        },
        (Ok(shell::ShellRunOutcome::UpdatePrepared(manifest)), Ok(())) => {
            match app::update::launch_update_helper(&manifest, std::process::id()) {
                Ok(()) => 0,
                Err(error) => {
                    eprintln!("tundra-shell update helper failed to start: {error}");
                    4
                }
            }
        }
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

    restart_current_executable()
}

fn restart_current_executable() -> Result<(), std::io::Error> {
    let executable = std::env::current_exe()?;
    let mut command = std::process::Command::new(&executable);

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;

        // Replacing the process keeps the foreground process group that the
        // invoking shell is waiting on. Spawning and then returning lets the
        // invoking shell reclaim the terminal before the replacement enables
        // raw mode, which makes the restarted TUI fail with EIO/SIGTTOU.
        let error = command.exec();
        Err(restart_error(&executable, error))
    }

    #[cfg(not(unix))]
    {
        command
            .spawn()
            .map_err(|error| restart_error(&executable, error))?;
        Ok(())
    }
}

fn restart_error(executable: &std::path::Path, error: std::io::Error) -> std::io::Error {
    std::io::Error::new(
        error.kind(),
        format!("could not restart {}: {error}", executable.display()),
    )
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
