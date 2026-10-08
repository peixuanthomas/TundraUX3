use super::{
    PackageBackend, PackageCommand,
    parsing::{ObservedPrompt, PromptParser},
};
use crate::management::{ManagementError, OperationEvent, OperationInteraction};
use portable_pty::{CommandBuilder, PtySize, native_pty_system};
use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

/// Runs inside the independent privileged helper. No process killer or
/// kill-on-drop guard is installed. Client cancellation/disconnection must not
/// interrupt a package database write or a maintainer script. Explicit AA
/// termination/kill requests are handled by the helper's independent control loop.
pub(super) fn execute(
    spec: PackageCommand,
    backend: PackageBackend,
    interaction: &mut dyn OperationInteraction,
    cancelled: &AtomicBool,
) -> Result<String, ManagementError> {
    if cancelled.load(Ordering::Acquire) {
        return Err(ManagementError::Cancelled);
    }
    let (columns, rows) = interaction.terminal_size().unwrap_or((108, 24));
    let pair = native_pty_system()
        .openpty(size(columns, rows))
        .map_err(pty_error)?;
    let fd = pair.master.as_raw_fd().ok_or_else(|| {
        ManagementError::Unavailable(
            "The package helper requires a Linux PTY file descriptor".into(),
        )
    })?;
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
        return Err(pty_error(std::io::Error::last_os_error()));
    }
    let mut writer = pair.master.take_writer().map_err(pty_error)?;
    let mut command = CommandBuilder::new(&spec.program);
    command.args(&spec.args);
    // Do not allow request/client environment variables to select arbitrary APT
    // configuration, preload a library, change RPM configuration, or inject a shell.
    command.env_clear();
    for (key, value) in [
        ("PATH", "/usr/sbin:/usr/bin:/sbin:/bin"),
        ("HOME", "/root"),
        ("USER", "root"),
        ("LOGNAME", "root"),
        ("LANG", "C.UTF-8"),
        ("LC_ALL", "C.UTF-8"),
        ("TERM", "xterm-256color"),
        ("DEBIAN_FRONTEND", "readline"),
        ("PAGER", "cat"),
    ] {
        command.env(key, value);
    }
    if spec.non_interactive {
        command.env("DEBIAN_FRONTEND", "noninteractive");
        command.env("DEBIAN_PRIORITY", "critical");
    }
    command.cwd("/");
    let mut child = pair.slave.spawn_command(command).map_err(pty_error)?;
    drop(pair.slave);
    interaction.emit(OperationEvent::Progress {
        message: format!(
            "{} started; waiting for the package manager's locks and actual plan",
            backend.id()
        ),
        percent: None,
    });
    interaction.emit(OperationEvent::Output { text: "Package operations survive a disconnected client. Answer n at the package manager's confirmation to cancel; AA can explicitly request termination, but interrupting writes may damage the system.".into() });
    let mut parser = PromptParser::default();
    let mut tail = Vec::<u8>::new();
    let mut cancel_noticed = false;
    let mut connection_error_noticed = false;
    let mut output_error = None;
    let mut exit_status = None;
    let mut exited_at = None;
    let mut last_size = (columns, rows);
    loop {
        let mut descriptor = libc::pollfd {
            fd,
            events: libc::POLLIN | libc::POLLHUP,
            revents: 0,
        };
        let ready = unsafe { libc::poll(&mut descriptor, 1, 50) };
        if ready < 0 && std::io::Error::last_os_error().kind() != std::io::ErrorKind::Interrupted {
            output_error.get_or_insert_with(|| std::io::Error::last_os_error().to_string());
        }
        if ready > 0 && descriptor.revents & (libc::POLLIN | libc::POLLHUP) != 0 {
            let mut bytes = [0_u8; 8_192];
            let count = unsafe { libc::read(fd, bytes.as_mut_ptr().cast(), bytes.len()) };
            if count > 0 {
                let bytes = &bytes[..count as usize];
                interaction.emit(OperationEvent::TerminalOutput {
                    bytes: bytes.to_vec(),
                });
                tail.extend_from_slice(bytes);
                if tail.len() > 32 * 1024 {
                    tail.drain(..tail.len() - 32 * 1024);
                }
                // Observe progress only. The live package process owns every prompt;
                // all answers go directly to its PTY, including unfamiliar/localized
                // maintainer-script questions and configuration-file choices.
                for prompt in parser.push(bytes) {
                    if let ObservedPrompt::Progress { message, percent } = prompt {
                        interaction.emit(OperationEvent::Progress { message, percent });
                    }
                }
            } else if count < 0 {
                let error = std::io::Error::last_os_error();
                if !matches!(
                    error.raw_os_error(),
                    Some(libc::EIO) | Some(libc::EAGAIN) | Some(libc::EINTR)
                ) {
                    output_error.get_or_insert_with(|| error.to_string());
                }
            }
        }
        if let Some(current) = interaction.terminal_size() {
            if current != last_size {
                if let Err(error) = pair.master.resize(size(current.0, current.1)) {
                    output_error.get_or_insert_with(|| error.to_string());
                }
                last_size = current;
            }
        }
        // The helper transports these bytes only; it never logs terminal input.
        match interaction.terminal_input(Duration::ZERO) {
            Ok(Some(bytes)) => {
                let safe = safe_terminal_input(&bytes);
                if !safe.is_empty() {
                    if let Err(error) = write_input(&mut writer, &safe) {
                        output_error.get_or_insert_with(|| error.to_string());
                    }
                }
            }
            Ok(None) => {}
            Err(_) => {
                if !connection_error_noticed {
                    interaction.emit(OperationEvent::Progress { message: "Terminal client disconnected; the package task remains running and can be reopened".into(), percent: None });
                    connection_error_noticed = true;
                }
            }
        }
        if cancelled.load(Ordering::Acquire) && !cancel_noticed {
            interaction.emit(OperationEvent::Progress { message: "The package task is still running. Reopen it to answer pending questions or use AA's explicit termination controls.".into(), percent: None });
            cancel_noticed = true;
        }
        if exit_status.is_none() {
            match child.try_wait() {
                Ok(Some(status)) => {
                    exit_status = Some(status);
                    exited_at = Some(Instant::now());
                }
                Ok(None) => {}
                Err(error) => {
                    // Losing an observation is not authorization to kill/replay
                    // installation. Preserve the PTY and continue trying to wait.
                    output_error.get_or_insert_with(|| error.to_string());
                }
            }
        }
        if let Some(exited_at) = exited_at {
            // Keep the final bytes and close only after the direct package
            // manager reports a real exit. A client exit never reaches this path.
            if exited_at.elapsed() >= Duration::from_millis(150) {
                break;
            }
        }
    }
    let status = exit_status.ok_or_else(|| ManagementError::Failed("Package manager exit status is unavailable; check the system package database before retrying".into()))?;
    if !status.success() {
        let output = runtime_log::sanitize_text(&String::from_utf8_lossy(&tail));
        let failure = super::classify_failure(backend, &output);
        interaction.emit(OperationEvent::Problem {
            problem: crate::management::problem::OperationProblem {
                code: failure.code.into(),
                summary_key: failure.code.replace('_', "-").replacen(
                    "package-",
                    "management-package-",
                    1,
                ),
                next_action: failure
                    .repair_actions
                    .first()
                    .copied()
                    .unwrap_or("check_database")
                    .into(),
                detail: output.clone(),
                exit_code: if failure.code == "package_busy" { 5 } else { 1 },
                native_exit_code: Some(status.exit_code() as i32),
                service: None,
                boot_id: None,
            },
        });
        interaction.emit(OperationEvent::Output {
            text: format!(
                "{}\n{}\nNative exit code: {}\n{output}",
                failure.summary,
                failure.next_step,
                status.exit_code()
            ),
        });
        return Err(ManagementError::Failed(format!(
            "{} {} ({}; exit code {}). Changes may be partially applied; review the task output before retrying.\n{output}",
            failure.summary,
            failure.next_step,
            failure.code,
            status.exit_code()
        )));
    }
    if let Some(error) = output_error {
        return Ok(format!(
            "{} completed (exit code 0). Terminal output could not be fully observed: {error}",
            backend.id()
        ));
    }
    Ok(format!(
        "{} completed (exit code 0); refresh the package list to read the resulting state",
        backend.id()
    ))
}

fn size(columns: u16, rows: u16) -> PtySize {
    PtySize {
        cols: columns.clamp(20, 500),
        rows: rows.clamp(5, 200),
        pixel_width: 0,
        pixel_height: 0,
    }
}
fn pty_error(error: impl std::fmt::Display) -> ManagementError {
    ManagementError::Failed(format!("Package PTY: {error}"))
}

fn write_input(writer: &mut dyn Write, bytes: &[u8]) -> std::io::Result<()> {
    writer.write_all(bytes)?;
    writer.flush()
}

/// Unix terminal control keys must not signal/EOF the active package manager.
/// Ordinary text, navigation and script form input remain usable.
fn safe_terminal_input(bytes: &[u8]) -> Vec<u8> {
    bytes
        .iter()
        .copied()
        .filter(|byte| !matches!(*byte, 0x03 | 0x04 | 0x1a | 0x1c))
        .take(16 * 1024)
        .collect()
}

#[cfg(test)]
#[path = "../../../tests/unit/management/packages/runner_tests.rs"]
mod tests;
