use super::*;
use crate::management::{OperationEvent, OperationInput, problem::OperationProblem};
use std::sync::mpsc;
use std::time::Duration;

struct Interaction<'a> {
    inputs: mpsc::Receiver<OperationInput>,
    emit: &'a dyn Fn(OperationEvent),
    size: Option<(u16, u16)>,
    stop: Option<bool>,
}

impl OperationInteraction for Interaction<'_> {
    fn emit(&mut self, event: OperationEvent) {
        (self.emit)(event);
    }

    fn ask(&mut self, _: &str, _: &str, _: &[String], _: bool) -> Result<String, ManagementError> {
        Err(ManagementError::InvalidInput(i18n::tr!(
            "management-brew-native-input"
        )))
    }

    fn terminal_input(&mut self, timeout: Duration) -> Result<Option<Vec<u8>>, ManagementError> {
        loop {
            match self.inputs.recv_timeout(timeout) {
                Ok(OperationInput::Terminal { bytes }) => return Ok(Some(bytes)),
                Ok(OperationInput::Resize { columns, rows }) => self.size = Some((columns, rows)),
                Ok(OperationInput::Cancel | OperationInput::Terminate) => {
                    self.stop.get_or_insert(false);
                }
                Ok(OperationInput::Kill) => self.stop = Some(true),
                Ok(OperationInput::Answer { .. }) => {}
                Err(_) => return Ok(None),
            }
        }
    }

    fn terminal_size(&mut self) -> Option<(u16, u16)> {
        self.size.take()
    }
    fn stop_requested(&mut self) -> Option<bool> {
        self.stop.take()
    }
}

pub fn run(
    command: Option<ManagementCommand>,
    _privileged: bool,
    socket: Option<PathBuf>,
    inputs: mpsc::Receiver<OperationInput>,
    detached: &AtomicBool,
    emit: &dyn Fn(OperationEvent),
) -> Result<(), ManagementError> {
    check_cancelled(detached)?;
    if socket.is_some() {
        return Err(ManagementError::UnsupportedEnvironment(i18n::tr!(
            "management-brew-no-attach"
        )));
    }
    let command = command.ok_or_else(|| {
        ManagementError::InvalidInput(i18n::tr!("management-brew-invalid-command"))
    })?;
    let context = ExecutionContext {
        actor_uid: unsafe { libc::getuid() },
        helper_path: PathBuf::new(),
        caller_pid: std::process::id(),
        caller_start_ticks: 0,
    };
    let mut interaction = Interaction {
        inputs,
        emit,
        size: None,
        stop: None,
    };
    emit(OperationEvent::Started {
        kind: command.kind,
        action: command.action.clone(),
        target: command.target.clone(),
    });
    // Closing a page must not interrupt package writes. Explicit AA stop
    // requests remain available through the operation's input channel.
    match super::execute(
        &command,
        &context,
        &mut interaction,
        &AtomicBool::new(false),
    ) {
        Ok(message) => emit(OperationEvent::Completed { message }),
        Err(error) => {
            emit(OperationEvent::Problem {
                problem: OperationProblem::from_error(&error),
            });
            emit(OperationEvent::Failed {
                message: error.to_string(),
            });
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "../../../tests/unit/management/homebrew_client_tests.rs"]
mod tests;
