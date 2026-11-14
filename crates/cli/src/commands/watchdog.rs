use std::io::{self, Write};
use std::time::Duration;

use i18n::LocalizedMessage;
use platform::shell_control::{watchdog_directory, watchdog_exchange, watchdog_sessions};
use serde_json::{Value, json};
use shell_control::SessionInfo as Guardian;
use workspace_protocol::{
    PROTOCOL_VERSION, ProcessState, RunId, SessionId, SessionInfo, WatchdogAction, WatchdogError,
    WatchdogReply, WatchdogRequest, WatchdogResponse, WorkerRole,
};

use crate::CliError;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WatchdogCommand {
    Help,
    List,
    Kill {
        run: RunId,
        session: Option<SessionId>,
    },
}

pub(crate) fn parse_watchdog(args: &[String]) -> Result<WatchdogCommand, CliError> {
    let invalid = CliError::InvalidWatchdogArgument;
    let mut options = match args {
        [] => return Ok(WatchdogCommand::Help),
        [verb] if verb == "list" => return Ok(WatchdogCommand::List),
        [verb, options @ ..] if verb == "kill" => options.iter(),
        _ => return Err(invalid),
    };
    let mut run = None;
    let mut target = None;
    while let Some(option) = options.next() {
        match option.as_str() {
            "--run" if run.is_none() => {
                run = Some(
                    options
                        .next()
                        .filter(|value| value.len() == 32)
                        .and_then(|value| value.parse().ok())
                        .ok_or(invalid.clone())?,
                );
            }
            "--session" if target.is_none() => {
                target = Some(Some(
                    options
                        .next()
                        .filter(|value| value.bytes().all(|byte| byte.is_ascii_digit()))
                        .and_then(|value| value.parse().ok())
                        .ok_or(invalid.clone())?,
                ));
            }
            "--shell" if target.is_none() => target = Some(None),
            _ => return Err(invalid),
        }
    }
    match (run, target) {
        (Some(run), Some(session)) => Ok(WatchdogCommand::Kill { run, session }),
        _ => Err(invalid),
    }
}

pub(crate) fn help(output: &mut impl Write) -> io::Result<()> {
    writeln!(output, "{}", i18n::tr!("watchdog-cli-help"))
}

pub(crate) fn run_watchdog(
    platform: &dyn platform::Platform,
    stdout: &mut impl Write,
    stderr: &mut impl Write,
    command: WatchdogCommand,
) -> i32 {
    if command == WatchdogCommand::Help {
        return i32::from(help(stdout).is_err());
    }
    let result = platform
        .app_paths()
        .map_err(|error| unavailable(error.to_string()))
        .and_then(|paths| {
            watchdog_sessions(&watchdog_directory(&paths))
                .map_err(|error| unavailable(error.to_string()))
        })
        .and_then(|guardians| match command {
            WatchdogCommand::List => list(&guardians),
            WatchdogCommand::Kill { run, session } => kill(&guardians, run, session),
            WatchdogCommand::Help => unreachable!(),
        });
    let result = match result {
        Ok(value) => serde_json::to_writer(&mut *stdout, &value)
            .and_then(|_| writeln!(stdout).map_err(serde_json::Error::io))
            .map_err(|error| i18n::msg!("watchdog-cli-output-error", reason = error.to_string())),
        Err(error) => Err(error),
    };
    match result {
        Ok(()) => 0,
        Err(error) => {
            let _ = writeln!(stderr, "{}", i18n::render_current(&error));
            1
        }
    }
}

fn list(guardians: &[Guardian]) -> Result<Value, LocalizedMessage> {
    let mut runs = Vec::new();
    for guardian in guardians {
        let run = guardian
            .session
            .parse()
            .map_err(|_| i18n::msg!("watchdog-cli-protocol"))?;
        let workers = list_workers(guardian, run)?;
        runs.push(json!({ "run": run, "workers": workers }));
    }
    Ok(json!({ "protocol": PROTOCOL_VERSION, "runs": runs }))
}

fn kill(
    guardians: &[Guardian],
    run: RunId,
    session: Option<SessionId>,
) -> Result<Value, LocalizedMessage> {
    let run_key = run.to_string();
    let guardian = guardians
        .iter()
        .find(|guardian| guardian.session == run_key)
        .ok_or_else(|| i18n::msg!("watchdog-cli-run-missing", run = run_key))?;
    let role = session.map_or(WorkerRole::Shell, WorkerRole::App);
    let worker = list_workers(guardian, run)?
        .into_iter()
        .find(|entry| {
            entry.worker.role == role && !matches!(entry.state, ProcessState::Exited { .. })
        })
        .map(|entry| entry.worker)
        .ok_or_else(|| i18n::msg!("watchdog-cli-target-missing"))?;
    match request(guardian, run, WatchdogAction::Kill { worker })? {
        WatchdogReply::Killed {
            worker: reply,
            code,
        } if reply == worker => Ok(
            json!({ "protocol": PROTOCOL_VERSION, "status": "killed", "worker": worker, "code": code }),
        ),
        WatchdogReply::KillRequested { worker: reply } if reply == worker => Ok(
            json!({ "protocol": PROTOCOL_VERSION, "status": "kill_requested", "worker": worker }),
        ),
        _ => Err(i18n::msg!("watchdog-cli-protocol")),
    }
}

fn list_workers(guardian: &Guardian, run: RunId) -> Result<Vec<SessionInfo>, LocalizedMessage> {
    match request(guardian, run, WatchdogAction::List)? {
        WatchdogReply::List(workers) if workers.iter().all(|entry| entry.worker.run == run) => {
            Ok(workers)
        }
        _ => Err(i18n::msg!("watchdog-cli-protocol")),
    }
}

fn request(
    guardian: &Guardian,
    run: RunId,
    action: WatchdogAction,
) -> Result<WatchdogReply, LocalizedMessage> {
    let request = WatchdogRequest {
        run,
        pid: guardian.pid,
        protocol: PROTOCOL_VERSION,
        action,
    };
    let response: WatchdogResponse = watchdog_exchange(guardian, &request, REQUEST_TIMEOUT)
        .map_err(|error| match action {
            WatchdogAction::List => unavailable(error.to_string()),
            WatchdogAction::Kill { .. } => {
                i18n::msg!("watchdog-cli-kill-unconfirmed", reason = error.to_string())
            }
        })?;
    response.map_err(|error| match error {
        WatchdogError::WrongRun => i18n::msg!("watchdog-cli-run-missing", run = run.to_string()),
        WatchdogError::Version => i18n::msg!("watchdog-cli-protocol"),
        WatchdogError::NotFound => i18n::msg!("watchdog-cli-target-missing"),
        WatchdogError::Unavailable => i18n::msg!("watchdog-cli-request-refused"),
    })
}

fn unavailable(reason: String) -> LocalizedMessage {
    i18n::msg!("watchdog-cli-unavailable", reason = reason)
}
