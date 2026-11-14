#[cfg(not(unix))]
use platform::ManagedCommand;
use std::sync::Arc;
use watchdog::{ProcessWatchdog, WatchdogConfig, WatchdogRuntime};
use workspace_protocol::{ShellCompletion, WorkerRole};

fn main() {
    if let Some(error) = terminal_runtime::run_pty_exec_shim() {
        exit_with_error("shell-entry-failed", error, 1);
    }
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let command = shell::parse_shell_args(&args);
    // Help and version do not need an interactive terminal or application state.
    match &command {
        Ok(shell::ShellLaunchCommand::Help) => {
            println!(
                "TundraUX3 Shell\n\
                 Usage: tundra-shell [OPTIONS]\n\n\
                 Start the fullscreen UI when no options are supplied.\n\n\
                 Options:\n  -h, --help     Show help\n  -V, --version  Show version\n\n\
                 Use tundra-cli for commands and configuration.\n\n{}",
                i18n::tr!("startup-shell-ui-automation-help")
            );
            return;
        }
        Ok(shell::ShellLaunchCommand::Version) => {
            println!("tundra-shell {}", env!("CARGO_PKG_VERSION"));
            return;
        }
        _ => {}
    }

    let _language = i18n::enter_snapshot(Arc::new(i18n::LanguageSnapshot::embedded(0)));

    match shell::process_host::worker_from_args(&args) {
        Ok(Some(connection)) => std::process::exit(run_worker(connection)),
        Ok(None) => {}
        Err(error) => exit_with_error("shell-entry-failed", error, 2),
    }

    #[cfg(target_os = "linux")]
    match shell::confirm_linux_startup() {
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
    if let Err(error) = command {
        eprintln!(
            "{}",
            i18n::tr!("shell-entry-failed", error = error.to_string())
        );
        std::process::exit(2);
    }

    // RPM replaces the running inode during an update. On Linux current_exe()
    // then ends in " (deleted)"; retain the original path to exec the new file.
    let restart_executable = std::env::current_exe();

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

    let restart_path = restart_executable
        .as_ref()
        .unwrap_or_else(|error| exit_with_error("shell-entry-failed", error, 1));
    let guardian = shell::process_host::run_guardian(restart_path)
        .unwrap_or_else(|error| exit_with_error("shell-entry-failed", error, 1));
    if guardian.code != 0 {
        std::process::exit(guardian.code);
    }

    let exit_code = match guardian.completion {
        Some(ShellCompletion::Restart) => match restart_current_executable(restart_executable) {
            Ok(code) => code,
            Err(error) => {
                eprintln!(
                    "{}",
                    i18n::tr!("shell-entry-restart-failed", error = error.to_string())
                );
                4
            }
        },
        Some(ShellCompletion::Reset) => match reset_storage_and_restart(restart_executable) {
            Ok(code) => code,
            Err(error) => {
                eprintln!(
                    "{}",
                    i18n::tr!("shell-entry-reset-failed", error = error.to_string())
                );
                4
            }
        },
        Some(ShellCompletion::UpdatePrepared(manifest)) => {
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
        Some(ShellCompletion::Exit) | None => 0,
    };

    if exit_code != 0 {
        std::process::exit(exit_code);
    }
}

fn run_worker(connection: &'static shell::process_host::WorkerConnection) -> i32 {
    #[cfg(unix)]
    if let Some(directory) = connection.private_directory()
        && let Err(error) = terminal_runtime::set_pty_exec_gate_directory(directory.to_path_buf())
    {
        exit_with_error("shell-entry-failed", error, 1);
    }
    let role = connection.worker().role;
    let (watchdog_runtime, process_watchdog) = match start_watchdog(role) {
        Ok(value) => value,
        Err(error) => exit_with_error("shell-entry-watchdog-start-failed", error, 3),
    };
    if role == WorkerRole::Shell {
        let _ = process_watchdog
            .register_emergency_cleanup(Arc::new(shell::restore_terminal_best_effort));
    }
    let result = shell::process_host::run_worker(connection, process_watchdog);
    if let Err(error) = watchdog_runtime.shutdown() {
        exit_with_error("shell-entry-watchdog-shutdown-failed", error, 3);
    }
    match result {
        Ok(completion) if role == WorkerRole::Shell => match connection.finish(completion) {
            Ok(()) => 0,
            Err(error) => exit_with_error("shell-entry-failed", error, 1),
        },
        Ok(_) => 0,
        Err(error) => exit_with_error("shell-entry-failed", error, 1),
    }
}

fn exit_with_error(message: &str, error: impl std::fmt::Display, code: i32) -> ! {
    eprintln!(
        "{}",
        i18n::render_diagnostic(
            &i18n::LocalizedMessage::new(message).with_arg("error", error.to_string())
        )
    );
    std::process::exit(code);
}

fn reset_storage_and_restart(
    executable: std::io::Result<std::path::PathBuf>,
) -> Result<i32, std::io::Error> {
    let platform = platform::native_platform();
    let paths = platform
        .app_paths()
        .map_err(|error| std::io::Error::other(error.to_string()))?;
    storage::reset_saved_content(&paths)?;

    restart_current_executable(executable)
}

fn restart_current_executable(
    executable: std::io::Result<std::path::PathBuf>,
) -> Result<i32, std::io::Error> {
    let executable = executable?;
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

#[cfg(all(test, target_os = "linux"))]
#[path = "../tests/unit/main_tests.rs"]
mod tests;

#[cfg(not(unix))]
fn wait_for_replacement(mut command: std::process::Command) -> std::io::Result<i32> {
    // Windows cannot replace this process with exec. Keep the process that
    // PowerShell is waiting for alive until the replacement exits; otherwise
    // PowerShell resumes reading keys and writing into the replacement's TUI.
    command
        .spawn_successor()?
        .wait()
        .map(|status| status.code().unwrap_or(1))
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

fn start_watchdog(
    role: WorkerRole,
) -> Result<(WatchdogRuntime, ProcessWatchdog), watchdog::WatchdogError> {
    let fallback = std::env::temp_dir().join("TundraUX3").join("watchdog");
    let platform = platform::native_platform();
    let paths_result = platform.app_paths();
    let path_failure = paths_result.as_ref().err().map(ToString::to_string);
    let process_name = match role {
        WorkerRole::Shell => "tundra-shell",
        WorkerRole::App(_) => "tundra-app-host",
        WorkerRole::Services => "tundra-session-services",
    };
    let mut config = match paths_result {
        Ok(paths) => WatchdogConfig::new(
            paths.logs_path().join("crashes"),
            fallback.join("crashes"),
            paths.data_path(),
            process_name,
            env!("CARGO_PKG_VERSION"),
        ),
        Err(_) => WatchdogConfig::new(
            fallback.join("crashes"),
            fallback.join("fallback"),
            fallback.join("state"),
            process_name,
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
    if role == WorkerRole::Shell
        && let Err(error) = process.report_stale_runs(|pid| match platform.is_process_alive(pid) {
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
        })
    {
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
#[path = "../tests/unit/binary/restart_tests.rs"]
mod restart_tests;
