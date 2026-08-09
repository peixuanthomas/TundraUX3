use std::sync::Arc;
use watchdog::{ProcessWatchdog, WatchdogConfig, WatchdogRuntime};

fn main() {
    let managed_session = match shell::ManagedSession::from_environment() {
        Ok(session) => session,
        Err(error) => {
            eprintln!("tundra-shell managed session failed: {error}");
            std::process::exit(shell::MANAGED_PROTOCOL_ERROR_EXIT_CODE);
        }
    };

    if let Err(error) = shell::parse_shell_args(std::env::args().skip(1)) {
        eprintln!("tundra-shell failed: {error}");
        exit_with_outcome(managed_session.as_ref(), 2);
    }

    let (watchdog_runtime, process_watchdog) = match start_watchdog(managed_session.as_ref()) {
        Ok(value) => value,
        Err(error) => {
            eprintln!("tundra-shell watchdog failed to start: {error}");
            exit_with_outcome(managed_session.as_ref(), 3);
        }
    };
    let _ =
        process_watchdog.register_emergency_cleanup(Arc::new(shell::restore_terminal_best_effort));

    let mut stdout = std::io::stdout();
    let run_result = shell::run_shell_blocking_managed_with_outcome(&mut stdout, process_watchdog);
    drop(stdout);
    let watchdog_shutdown = watchdog_runtime.shutdown();

    let exit_code = if managed_session.is_some() {
        managed_exit_code(run_result, watchdog_shutdown)
    } else {
        match (run_result, watchdog_shutdown) {
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
            (Ok(shell::ShellRunOutcome::ResetRequested), Ok(())) => {
                match reset_storage_and_restart() {
                    Ok(()) => 0,
                    Err(error) => {
                        eprintln!("tundra-shell reset failed: {error}");
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
        }
    };

    exit_with_outcome(managed_session.as_ref(), exit_code);
}

fn managed_exit_code(
    run_result: std::io::Result<shell::ShellRunOutcome>,
    watchdog_shutdown: Result<(), watchdog::WatchdogError>,
) -> i32 {
    if let Err(error) = watchdog_shutdown {
        eprintln!("tundra-shell watchdog shutdown failed: {error}");
        return 3;
    }

    match run_result {
        Ok(shell::ShellRunOutcome::Exit) => 0,
        Ok(shell::ShellRunOutcome::RestartRequested) => shell::MANAGED_RESTART_EXIT_CODE,
        Ok(shell::ShellRunOutcome::ResetRequested) => shell::MANAGED_RESET_EXIT_CODE,
        Err(error) => {
            eprintln!("tundra-shell failed: {error}");
            1
        }
    }
}

fn exit_with_outcome(managed_session: Option<&shell::ManagedSession>, mut exit_code: i32) -> ! {
    if let Some(session) = managed_session
        && let Err(error) = session.write_exit(exit_code)
    {
        eprintln!("tundra-shell could not write managed outcome: {error}");
        if exit_code == 0 {
            exit_code = 3;
        }
    }

    // The kiosk WezTerm closes its managed window when the foreground
    // program exits successfully.  The logical result is carried by the
    // atomically-written outcome file; normalizing the process status lets
    // restart/reset/error outcomes reach the outer supervisor instead of
    // getting trapped in WezTerm's ordinary non-zero-exit overlay.
    std::process::exit(process_exit_code(managed_session.is_some(), exit_code))
}

fn process_exit_code(parent_managed: bool, logical_exit_code: i32) -> i32 {
    if parent_managed { 0 } else { logical_exit_code }
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

fn start_watchdog(
    managed_session: Option<&shell::ManagedSession>,
) -> Result<(WatchdogRuntime, ProcessWatchdog), watchdog::WatchdogError> {
    let parent_managed = managed_session.is_some();
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
    }
    .with_unclean_exit_tracking(!parent_managed);
    let config = if let Some(session) = managed_session {
        config.with_session_id(session.session_id())
    } else {
        config
    };
    let (runtime, process) = WatchdogRuntime::start(config)?;
    let process = process.install_global()?;
    if !parent_managed {
        let _ = process.report_stale_runs(|pid| platform.is_process_alive(pid).unwrap_or(true));
    }
    Ok((runtime, process))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn managed_outcomes_are_returned_to_the_outer_host() {
        assert_eq!(
            managed_exit_code(Ok(shell::ShellRunOutcome::Exit), Ok(())),
            0
        );
        assert_eq!(
            managed_exit_code(Ok(shell::ShellRunOutcome::RestartRequested), Ok(())),
            shell::MANAGED_RESTART_EXIT_CODE
        );
        assert_eq!(
            managed_exit_code(Ok(shell::ShellRunOutcome::ResetRequested), Ok(())),
            shell::MANAGED_RESET_EXIT_CODE
        );
        assert_eq!(process_exit_code(true, shell::MANAGED_RESTART_EXIT_CODE), 0);
        assert_eq!(process_exit_code(false, 4), 4);
    }
}
