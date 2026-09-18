use std::sync::Arc;
use watchdog::{ProcessWatchdog, WatchdogConfig, WatchdogRuntime};

fn main() {
    #[cfg(target_os = "linux")]
    match platform::linux::identity::LinuxUserContext::current() {
        Ok(user) => {
            // SAFETY: no threads or runtime have been started at executable entry.
            unsafe {
                user.install_environment();
            }
        }
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    }

    // Entry and post-run failures remain renderable without loading or repairing assets.
    // Runtime language scopes override this fallback while the shell is running.
    let _language = i18n::enter_snapshot(Arc::new(i18n::LanguageSnapshot::embedded(0)));
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
        eprintln!(
            "{}",
            i18n::tr!("shell-entry-failed", error = error.to_string())
        );
        std::process::exit(2);
    }

    if std::env::var_os(app::update::UPDATE_READY_FILE_ENV).is_none() {
        match app::update::recover_interrupted_update_from_current_exe(std::process::id()) {
            Ok(true) => return,
            Ok(false) => {}
            Err(error) => {
                let platform = platform::native_platform();
                let _ = platform.show_critical_error(
                    &i18n::tr!("shell-entry-update-recovery-title"),
                    &error.to_string(),
                );
                eprintln!(
                    "{}",
                    i18n::tr!(
                        "shell-entry-update-recovery-failed",
                        error = error.to_string()
                    )
                );
                std::process::exit(5);
            }
        }
    }

    let (watchdog_runtime, process_watchdog) = match start_watchdog() {
        Ok(value) => value,
        Err(error) => {
            eprintln!(
                "{}",
                i18n::tr!(
                    "shell-entry-watchdog-start-failed",
                    error = error.to_string()
                )
            );
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
                Ok(code) => code,
                Err(error) => {
                    eprintln!(
                        "{}",
                        i18n::tr!("shell-entry-restart-failed", error = error.to_string())
                    );
                    4
                }
            }
        }
        (Ok(shell::ShellRunOutcome::ResetRequested), Ok(())) => match reset_storage_and_restart() {
            Ok(code) => code,
            Err(error) => {
                eprintln!(
                    "{}",
                    i18n::tr!("shell-entry-reset-failed", error = error.to_string())
                );
                4
            }
        },
        (Ok(shell::ShellRunOutcome::UpdatePrepared(manifest)), Ok(())) => {
            match app::update::launch_update_helper(&manifest, std::process::id()) {
                Ok(()) => 0,
                Err(error) => {
                    eprintln!(
                        "{}",
                        i18n::tr!(
                            "shell-entry-update-helper-failed",
                            error = error.to_string()
                        )
                    );
                    4
                }
            }
        }
        (_, Err(error)) => {
            eprintln!(
                "{}",
                i18n::tr!(
                    "shell-entry-watchdog-shutdown-failed",
                    error = error.to_string()
                )
            );
            3
        }
        (Err(error), Ok(())) => {
            eprintln!(
                "{}",
                i18n::tr!("shell-entry-failed", error = error.to_string())
            );
            1
        }
    };

    if exit_code != 0 {
        std::process::exit(exit_code);
    }
}

fn reset_storage_and_restart() -> Result<i32, std::io::Error> {
    let platform = platform::native_platform();
    let paths = platform
        .app_paths()
        .map_err(|error| std::io::Error::other(error.to_string()))?;
    storage::reset_saved_content(&paths)?;

    restart_current_executable()
}

fn restart_current_executable() -> Result<i32, std::io::Error> {
    let executable = std::env::current_exe()?;
    let command = std::process::Command::new(&executable);

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        let mut command = command;

        // Replacing the process keeps the foreground process group that the
        // invoking shell is waiting on. Spawning and then returning lets the
        // invoking shell reclaim the terminal before the replacement enables
        // raw mode, which makes the restarted TUI fail with EIO/SIGTTOU.
        let error = command.exec();
        Err(restart_error(&executable, error))
    }

    #[cfg(not(unix))]
    {
        wait_for_replacement(command).map_err(|error| restart_error(&executable, error))
    }
}

#[cfg(not(unix))]
fn wait_for_replacement(mut command: std::process::Command) -> std::io::Result<i32> {
    // Windows cannot replace this process with exec. Keep the process that
    // PowerShell is waiting for alive until the replacement exits; otherwise
    // PowerShell resumes reading keys and writing into the replacement's TUI.
    command.status().map(|status| status.code().unwrap_or(1))
}

fn restart_error(executable: &std::path::Path, error: std::io::Error) -> std::io::Error {
    std::io::Error::new(
        error.kind(),
        i18n::render_diagnostic(&i18n::msg!(
            "shell-entry-restart-cause",
            executable = executable.display().to_string(),
            error = error.to_string()
        )),
    )
}

fn start_watchdog() -> Result<(WatchdogRuntime, ProcessWatchdog), watchdog::WatchdogError> {
    let fallback = std::env::temp_dir().join("TundraUX3").join("watchdog");
    let platform = platform::native_platform();
    let paths_result = platform.app_paths();
    let path_failure = paths_result.as_ref().err().map(ToString::to_string);
    let mut config = match paths_result {
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
    let retention_result = platform.app_paths().ok().map(|paths| {
        storage::StorageManager::from_layout(storage::StorageLayout::from_app_paths(&paths))
            .load_config()
    });
    if let Some(Ok(stored)) = &retention_result {
        config.runtime_log_max_age_days = stored.runtime_logs.max_age_days;
        config.runtime_log_max_total_bytes = stored
            .runtime_logs
            .max_total_mib
            .saturating_mul(1024 * 1024);
        config.runtime_log_segment_bytes =
            stored.runtime_logs.segment_mib.saturating_mul(1024 * 1024);
    }
    let (runtime, process) = WatchdogRuntime::start(config)?;
    if let Some(Err(error)) = retention_result {
        let mut event = runtime_log::RuntimeLogEvent::new(
            process.log_context("ux.storage", "load_log_configuration"),
            runtime_log::LogLevel::Warning,
            runtime_log::LogPhase::Degraded,
            "Using default log retention configuration",
        );
        event.error_chain.push(error.to_string());
        event.error_code = Some("UX_LOG_CONFIG_DEFAULTS".into());
        process.record_log(event);
    }
    if let Some(error) = path_failure {
        let mut event = runtime_log::RuntimeLogEvent::new(
            process.log_context("ux.storage", "resolve_log_directory"),
            runtime_log::LogLevel::Warning,
            runtime_log::LogPhase::Degraded,
            "Using temporary log storage",
        );
        event.error_chain.push(error);
        event.error_code = Some("UX_LOG_STORAGE_FALLBACK".into());
        process.record_log(event);
    }
    let process = process.install_global()?;
    if let Err(error) = process.report_stale_runs(|pid| match platform.is_process_alive(pid) {
        Ok(alive) => alive,
        Err(error) => {
            let mut event = runtime_log::RuntimeLogEvent::new(
                process.log_context("ux.watchdog", "inspect_previous_process"),
                runtime_log::LogLevel::Warning,
                runtime_log::LogPhase::Degraded,
                "Previous process state unavailable; retaining run marker",
            );
            watchdog::capture_error(&mut event, &error);
            event.alert_key = Some("previous-process-inspection".into());
            process.record_log(event);
            true
        }
    }) {
        let mut event = runtime_log::RuntimeLogEvent::new(
            process.log_context("ux.watchdog", "inspect_previous_runs"),
            runtime_log::LogLevel::Warning,
            runtime_log::LogPhase::Failed,
            "Could not inspect previous runs",
        );
        watchdog::capture_error(&mut event, &error);
        process.record_log(event);
    }
    Ok((runtime, process))
}

#[cfg(test)]
#[cfg(windows)]
mod restart_tests {
    use super::wait_for_replacement;
    use std::io::{BufRead, Read, Write};
    use std::process::{Command, Stdio};
    use std::sync::mpsc;
    use std::time::Duration;

    const DEPTH_ENV: &str = "TUNDRA_RESTART_TEST_DEPTH";
    const EXIT_ENV: &str = "TUNDRA_RESTART_TEST_EXIT";

    fn fixture_command(depth: u32, exit_code: i32) -> Command {
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "--exact",
                "restart_tests::replacement_fixture",
                "--nocapture",
            ])
            .env(DEPTH_ENV, depth.to_string())
            .env(EXIT_ENV, exit_code.to_string());
        command
    }

    #[test]
    fn replacement_fixture() {
        let Ok(depth) = std::env::var(DEPTH_ENV) else {
            return;
        };
        let depth: u32 = depth.parse().unwrap();
        let exit_code: i32 = std::env::var(EXIT_ENV).unwrap().parse().unwrap();
        if depth > 0 {
            std::process::exit(
                wait_for_replacement(fixture_command(depth - 1, exit_code)).unwrap(),
            );
        }
        println!("REPLACEMENT_READY");
        std::io::stdout().flush().unwrap();
        // The outer test holds stdin open until it has checked that every
        // restarting ancestor is still alive. EOF then lets the new UI exit.
        std::io::stdin().read_to_end(&mut Vec::new()).unwrap();
        std::process::exit(exit_code);
    }

    #[test]
    fn windows_restart_keeps_caller_waiting_and_propagates_exit_code() {
        for (depth, exit_code) in [(1, 0), (3, 23)] {
            let mut child = fixture_command(depth, exit_code)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .spawn()
                .unwrap();
            let input = child.stdin.take().unwrap();
            let output = std::io::BufReader::new(child.stdout.take().unwrap());
            assert!(
                output
                    .lines()
                    .any(|line| line.unwrap() == "REPLACEMENT_READY")
            );
            let (sender, receiver) = mpsc::channel();
            let waiter = std::thread::spawn(move || sender.send(child.wait()).unwrap());
            let premature_exit = receiver.recv_timeout(Duration::from_millis(250));
            let stayed_alive = matches!(premature_exit, Err(mpsc::RecvTimeoutError::Timeout));
            drop(input);
            let status = match premature_exit {
                Ok(status) => status,
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    receiver.recv_timeout(Duration::from_secs(10)).unwrap()
                }
                Err(error) => panic!("replacement waiter disconnected: {error}"),
            }
            .unwrap();
            waiter.join().unwrap();
            assert!(stayed_alive, "restart released the invoking shell early");
            assert_eq!(status.code(), Some(exit_code));
        }
    }
}
