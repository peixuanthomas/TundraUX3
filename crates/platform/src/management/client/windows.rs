use crate::management::*;
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
};

/// Windows installers handle system elevation through UAC after AA approval.
pub fn run(
    command: Option<ManagementCommand>,
    _privileged: bool,
    socket: Option<PathBuf>,
    inputs: mpsc::Receiver<OperationInput>,
    detached: &AtomicBool,
    emit: &dyn Fn(OperationEvent),
) -> Result<(), ManagementError> {
    if socket.is_some() {
        return Err(ManagementError::UnsupportedEnvironment(i18n::tr!(
            "management-winget-no-reconnect"
        )));
    }
    let command = command.ok_or_else(|| {
        ManagementError::InvalidInput(i18n::tr!("management-winget-invalid-command"))
    })?;
    if command.kind != ManagementKind::Packages {
        return Err(ManagementError::Unavailable(i18n::tr!(
            "platform-management-unavailable"
        )));
    }
    if detached.load(Ordering::Acquire)
        || inputs.try_iter().any(|input| {
            matches!(
                input,
                OperationInput::Cancel | OperationInput::Terminate | OperationInput::Kill
            )
        })
    {
        return Err(ManagementError::Cancelled);
    }
    emit(OperationEvent::Started {
        kind: command.kind,
        action: command.action.clone(),
        target: command.target.clone(),
    });
    let context = ExecutionContext {
        actor_uid: 0,
        helper_path: PathBuf::new(),
        caller_pid: std::process::id(),
        caller_start_ticks: 0,
    };
    let mut interaction = Interaction { emit, inputs };
    match super::super::winget::execute(&command, &context, &mut interaction, detached) {
        Ok(message) => emit(OperationEvent::Completed { message }),
        Err(error) => {
            emit(OperationEvent::Problem {
                problem: problem::OperationProblem::from_error(&error),
            });
            emit(OperationEvent::Failed {
                message: error.to_string(),
            });
        }
    }
    Ok(())
}

struct Interaction<'a> {
    emit: &'a dyn Fn(OperationEvent),
    inputs: mpsc::Receiver<OperationInput>,
}

impl OperationInteraction for Interaction<'_> {
    fn emit(&mut self, event: OperationEvent) {
        (self.emit)(event);
    }

    fn ask(
        &mut self,
        _id: &str,
        _prompt: &str,
        _choices: &[String],
        _secret: bool,
    ) -> Result<String, ManagementError> {
        Err(ManagementError::UnsupportedEnvironment(i18n::tr!(
            "management-winget-noninteractive"
        )))
    }

    fn terminal_input(
        &mut self,
        _timeout: std::time::Duration,
    ) -> Result<Option<Vec<u8>>, ManagementError> {
        if self.inputs.try_iter().any(|input| {
            matches!(
                input,
                OperationInput::Cancel | OperationInput::Terminate | OperationInput::Kill
            )
        }) {
            return Err(ManagementError::Cancelled);
        }
        Ok(None)
    }
}

#[cfg(test)]
#[path = "../../../tests/unit/management/winget_client_tests.rs"]
mod winget_client_tests;
