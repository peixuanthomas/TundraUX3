use std::io;
use std::path::Path;
use std::time::Duration;
use watchdog::ProcessWatchdog;
use workspace_protocol::{ShellCompletion, WorkerRole};

mod client;
#[cfg(target_os = "linux")]
mod delegated;
#[cfg(windows)]
mod desktop;
mod guardian;
mod terminal;

pub use client::{WindowConnection, WorkerConnection};
#[cfg(target_os = "linux")]
pub use delegated::install_delegated_worker_resources;
pub use guardian::GuardianExit;

const WORKER_ARGUMENT: &str = "__workspace-worker";
const POLL_INTERVAL: Duration = Duration::from_millis(10);
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(2);

pub fn run_guardian(executable: &Path) -> io::Result<GuardianExit> {
    guardian::run(executable)
}

pub fn worker_from_args(args: &[String]) -> io::Result<Option<&'static WorkerConnection>> {
    if args.first().is_some_and(|arg| arg == WORKER_ARGUMENT) {
        return client::connect_worker(&args[1..]).map(Some);
    }
    Ok(None)
}

pub fn run_worker(
    connection: &'static WorkerConnection,
    process: ProcessWatchdog,
) -> io::Result<ShellCompletion> {
    #[cfg(windows)]
    if connection.worker().role != WorkerRole::Services {
        desktop::install()?;
    }
    let completion = match connection.worker().role {
        WorkerRole::Shell => {
            connection.send(workspace_protocol::WorkerToSupervisor::Ready)?;
            let result = crate::run_shell_blocking_managed_with_outcome(&mut io::stdout(), process);
            match result {
                Ok(crate::ShellRunOutcome::Exit) => ShellCompletion::Exit,
                Ok(crate::ShellRunOutcome::RestartRequested) => ShellCompletion::Restart,
                Ok(crate::ShellRunOutcome::ResetRequested) => ShellCompletion::Reset,
                Ok(crate::ShellRunOutcome::UpdatePrepared(path)) => {
                    ShellCompletion::UpdatePrepared(path)
                }
                Err(error)
                    if error.kind() == io::ErrorKind::Interrupted
                        && connection.shutdown_requested() =>
                {
                    ShellCompletion::Exit
                }
                Err(error) => return Err(error),
            }
        }
        WorkerRole::App(_) => {
            crate::run_app_host(connection, process)?;
            ShellCompletion::Exit
        }
        WorkerRole::Services => {
            crate::session_services::run_session_services(connection, process)?;
            ShellCompletion::Exit
        }
    };
    Ok(completion)
}
