use super::{ManagementError, OperationInteraction, check_cancelled};
use crate::management::{OperationEvent, problem::OperationProblem};
use crate::{ManagedChild, ManagedCommand};
use portable_pty::{CommandBuilder, PtySize, native_pty_system};
use std::io::{Read, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

const ENVIRONMENT: &[(&str, &str)] = &[
    ("HOMEBREW_NO_AUTO_UPDATE", "1"),
    ("HOMEBREW_NO_INSTALL_CLEANUP", "1"),
    ("HOMEBREW_NO_AUTOREMOVE", "1"),
    ("HOMEBREW_NO_ANALYTICS", "1"),
    ("HOMEBREW_NO_ENV_HINTS", "1"),
    ("LC_ALL", "en_US.UTF-8"),
];

fn io_error(error: impl std::fmt::Display) -> ManagementError {
    ManagementError::Failed(i18n::tr!(
        "management-brew-process-error",
        detail = error.to_string()
    ))
}

pub(super) fn read(
    program: &Path,
    args: &[String],
    cancelled: &AtomicBool,
) -> Result<String, ManagementError> {
    check_cancelled(cancelled)?;
    let mut child = Command::new(program)
        .args(args)
        .envs(ENVIRONMENT.iter().copied())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn_managed()
        .map_err(io_error)?;
    let mut stdout = child.stdout.take().unwrap();
    let mut stderr = child.stderr.take().unwrap();
    crate::process::make_nonblocking(&stdout).map_err(io_error)?;
    crate::process::make_nonblocking(&stderr).map_err(io_error)?;
    let mut output = Vec::new();
    let mut diagnostics = Vec::new();
    let started = Instant::now();
    loop {
        check_cancelled(cancelled)?;
        if started.elapsed() > Duration::from_secs(45) {
            return Err(ManagementError::Failed(i18n::tr!(
                "management-brew-query-timeout"
            )));
        }
        let progress = drain(&mut stdout, &mut output)? | drain(&mut stderr, &mut diagnostics)?;
        if let Some(status) = child.try_wait().map_err(io_error)? {
            if progress || drain(&mut stdout, &mut output)? | drain(&mut stderr, &mut diagnostics)?
            {
                continue;
            }
            let diagnostics = String::from_utf8_lossy(&diagnostics);
            // Homebrew uses exit 1 for a successful search with no matches.
            let no_matches = args.first().is_some_and(|arg| arg == "search")
                && status.code() == Some(1)
                && output.is_empty()
                && diagnostics
                    .trim()
                    .starts_with("Error: No formulae or casks found for ");
            if status.success() || no_matches {
                return Ok(String::from_utf8_lossy(&output).into_owned());
            }
            return Err(ManagementError::Failed(i18n::tr!(
                "management-brew-command-failed",
                status = status.to_string(),
                detail = runtime_log::sanitize_text(&diagnostics)
            )));
        }
        if !progress {
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

fn drain(reader: &mut dyn Read, output: &mut Vec<u8>) -> Result<bool, ManagementError> {
    let mut bytes = [0; 16 * 1024];
    let mut progress = false;
    for _ in 0..16 {
        match reader.read(&mut bytes) {
            Ok(0) => break,
            Ok(count) => {
                if output.len() + count > 64 * 1024 * 1024 {
                    return Err(ManagementError::Failed(i18n::tr!(
                        "management-brew-query-size"
                    )));
                }
                output.extend_from_slice(&bytes[..count]);
                progress = true;
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => break,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(io_error(error)),
        }
    }
    Ok(progress)
}

fn size(columns: u16, rows: u16) -> PtySize {
    PtySize {
        rows: rows.max(1),
        cols: columns.max(1),
        pixel_width: 0,
        pixel_height: 0,
    }
}

fn write_input(writer: &mut dyn Write, bytes: &[u8]) -> std::io::Result<()> {
    let bytes: Vec<_> = bytes
        .iter()
        .copied()
        .filter(|byte| !matches!(byte, 3 | 4 | 26 | 28))
        .take(16 * 1024)
        .collect();
    writer.write_all(&bytes)?;
    writer.flush()
}

pub(super) fn execute(
    program: &Path,
    args: &[String],
    interaction: &mut dyn OperationInteraction,
    cancelled: &AtomicBool,
) -> Result<String, ManagementError> {
    check_cancelled(cancelled)?;
    let initial_input = interaction.terminal_input(Duration::ZERO)?;
    if interaction.stop_requested().is_some() {
        return Err(ManagementError::Cancelled);
    }
    let (columns, rows) = interaction.terminal_size().unwrap_or((108, 24));
    let pair = native_pty_system()
        .openpty(size(columns, rows))
        .map_err(io_error)?;
    let fd = pair
        .master
        .as_raw_fd()
        .ok_or_else(|| io_error("missing PTY descriptor"))?;
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
        return Err(io_error(std::io::Error::last_os_error()));
    }
    let mut writer = pair.master.take_writer().map_err(io_error)?;
    let mut command = CommandBuilder::new(program);
    command.args(args);
    for &(key, value) in ENVIRONMENT {
        command.env(key, value);
    }
    command.env("TERM", "xterm-256color");
    // Homebrew now enables native confirmation by default and rejects HOMEBREW_ASK.
    command.env_remove("HOMEBREW_ASK");
    command.env_remove("HOMEBREW_NO_ASK");
    let mut child = ManagedChild::spawn_pty(pair.slave.as_ref(), command).map_err(io_error)?;
    drop(pair.slave);
    if let Some(bytes) = initial_input {
        write_input(&mut writer, &bytes).map_err(io_error)?;
    }
    interaction.emit(OperationEvent::Connected {
        operation_id: format!("brew:{}", child.id()),
    });
    interaction.emit(OperationEvent::Progress {
        message: i18n::tr!("management-brew-running"),
        percent: None,
    });
    let mut tail = Vec::new();
    let mut status = None;
    let mut stopped = false;
    let mut output_error = None;
    loop {
        if let Some(bytes) = interaction.terminal_input(Duration::ZERO)? {
            if let Err(error) = write_input(&mut writer, &bytes) {
                output_error.get_or_insert(error.to_string());
            }
        }
        if let Some(force) = interaction.stop_requested() {
            stopped = true;
            if force {
                child.kill()
            } else {
                child.terminate()
            }
            .map_err(io_error)?;
        }
        if let Some((columns, rows)) = interaction.terminal_size() {
            pair.master.resize(size(columns, rows)).map_err(io_error)?;
        }
        let mut descriptor = libc::pollfd {
            fd,
            events: libc::POLLIN | libc::POLLHUP,
            revents: 0,
        };
        let ready = unsafe { libc::poll(&mut descriptor, 1, 50) };
        if ready < 0 && std::io::Error::last_os_error().kind() != std::io::ErrorKind::Interrupted {
            output_error.get_or_insert_with(|| std::io::Error::last_os_error().to_string());
        }
        let mut count = 0;
        if ready > 0 {
            let mut bytes = [0_u8; 8192];
            count = unsafe { libc::read(fd, bytes.as_mut_ptr().cast(), bytes.len()) };
            if count > 0 {
                let bytes = &bytes[..count as usize];
                interaction.emit(OperationEvent::TerminalOutput {
                    bytes: bytes.to_vec(),
                });
                tail.extend_from_slice(bytes);
                if tail.len() > 32 * 1024 {
                    tail.drain(..tail.len() - 32 * 1024);
                }
            } else if count < 0 {
                let error = std::io::Error::last_os_error();
                if !matches!(
                    error.raw_os_error(),
                    Some(libc::EIO) | Some(libc::EAGAIN) | Some(libc::EINTR)
                ) {
                    output_error.get_or_insert(error.to_string());
                }
            }
        }
        if status.is_none() {
            status = child.try_wait().map_err(io_error)?;
        } else if count <= 0 {
            break;
        }
    }
    if stopped {
        return Err(ManagementError::Failed(i18n::tr!(
            "management-brew-interrupted"
        )));
    }
    let status = status.unwrap();
    if !status.success() {
        let mut problem = OperationProblem::from_error(&ManagementError::Failed(i18n::tr!(
            "management-brew-command-failed",
            status = status.to_string(),
            detail = runtime_log::sanitize_text(&String::from_utf8_lossy(&tail))
        )));
        problem.native_exit_code = status.code();
        interaction.emit(OperationEvent::Problem {
            problem: problem.clone(),
        });
        return Err(ManagementError::Failed(problem.detail));
    }
    if let Some(error) = output_error {
        return Err(io_error(error));
    }
    Ok(i18n::tr!("management-brew-completed"))
}
