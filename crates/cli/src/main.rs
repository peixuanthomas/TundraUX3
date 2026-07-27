use std::panic::AssertUnwindSafe;
use std::sync::Arc;
use watchdog::{
    AppCriticality, AppDescriptor, AppId, BoundaryKind, BoundarySpec, ProcessWatchdog,
    RecoveryOutcome, WatchdogConfig, WatchdogRuntime,
};

fn main() {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let parent_managed = is_parent_managed_command_line(&args);
    let (watchdog_runtime, process_watchdog) = match start_watchdog(parent_managed) {
        Ok(value) => value,
        Err(error) => {
            eprintln!("tundra-cli watchdog failed to start: {error}");
            std::process::exit(3);
        }
    };
    let _ =
        process_watchdog.register_emergency_cleanup(Arc::new(weathr::restore_terminal_best_effort));
    let cli_watchdog = match process_watchdog.register_app(AppDescriptor::new(
        AppId::from_static("cli"),
        "Tundra CLI",
        env!("CARGO_PKG_VERSION"),
        AppCriticality::ProcessCritical,
    )) {
        Ok(watchdog) => watchdog,
        Err(error) => {
            eprintln!("tundra-cli watchdog registration failed: {error}");
            let _ = watchdog_runtime.shutdown();
            std::process::exit(3);
        }
    };
    let weathr_watchdog = match process_watchdog.register_app(weathr::weathr_watchdog_descriptor())
    {
        Ok(watchdog) => watchdog,
        Err(error) => {
            eprintln!("tundra-cli Weathr watchdog registration failed: {error}");
            let _ = watchdog_runtime.shutdown();
            std::process::exit(3);
        }
    };

    let mut stdout = std::io::stdout();
    let mut stderr = std::io::stderr();
    let result = cli_watchdog.run_boundary(
        BoundarySpec::new("cli.command", BoundaryKind::Process).terminal_owner(),
        AssertUnwindSafe(|| {
            cli::run_managed(
                args,
                &process_watchdog,
                weathr_watchdog,
                &mut stdout,
                &mut stderr,
            )
        }),
    );
    let exit_code = match result {
        Ok(exit_code) => exit_code,
        Err(caught) => {
            let reason = caught.payload().to_string();
            let receipt = caught
                .finalize(RecoveryOutcome::Unrecoverable(
                    "CLI commands are never replayed after panic".to_string(),
                ))
                .ok();
            let report = receipt
                .as_ref()
                .and_then(|receipt| receipt.text_report_path.as_ref())
                .map(|path| path.display().to_string())
                .unwrap_or_else(|| "report path unavailable".to_string());
            eprintln!("tundra-cli panicked: {reason}\nCrash report: {report}");
            1
        }
    };

    let _ = watchdog_runtime.shutdown();
    std::process::exit(exit_code);
}

fn is_parent_managed_command_line(args: &[String]) -> bool {
    matches!(
        cli::parse_args(args),
        Ok(cli::CliCommand::Repl { embedded: true })
    )
}

fn start_watchdog(
    parent_managed: bool,
) -> Result<(WatchdogRuntime, ProcessWatchdog), watchdog::WatchdogError> {
    let fallback = std::env::temp_dir().join("TundraUX3").join("watchdog");
    let platform = platform::native_platform();
    let config = match platform.app_paths() {
        Ok(paths) => WatchdogConfig::new(
            paths.logs_path().join("crashes"),
            fallback.join("crashes"),
            paths.data_path(),
            "tundra-cli",
            env!("CARGO_PKG_VERSION"),
        ),
        Err(_) => WatchdogConfig::new(
            fallback.join("crashes"),
            fallback.join("fallback"),
            fallback.join("state"),
            "tundra-cli",
            env!("CARGO_PKG_VERSION"),
        ),
    }
    .with_unclean_exit_tracking(!parent_managed);
    let (runtime, process) = WatchdogRuntime::start(config)?;
    let process = process.install_global()?;
    let _ = process.report_stale_runs(|pid| platform.is_process_alive(pid).unwrap_or(true));
    Ok((runtime, process))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_string()).collect()
    }

    #[test]
    fn only_the_embedded_repl_uses_parent_managed_lifecycle() {
        assert!(is_parent_managed_command_line(&args(&[
            "repl",
            "--embedded"
        ])));
        assert!(!is_parent_managed_command_line(&args(&["repl"])));
        assert!(!is_parent_managed_command_line(&args(&["help"])));
        assert!(!is_parent_managed_command_line(&args(&["--embedded"])));
    }
}
