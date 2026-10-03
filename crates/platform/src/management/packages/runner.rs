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
/// interrupt a package database write or a maintainer script.
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
    interaction.emit(OperationEvent::Output { text: "Package operations continue in the system helper when this page closes. Cancellation is available at the package manager's confirmation; active package writes are not forcibly stopped.".into() });
    let mut parser = PromptParser::default();
    let mut question_sequence = 0_u64;
    let mut tail = Vec::<u8>::new();
    let mut declined = false;
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
                for prompt in parser.push(bytes) {
                    match prompt {
                        ObservedPrompt::Progress { message, percent } => {
                            interaction.emit(OperationEvent::Progress { message, percent })
                        }
                        ObservedPrompt::Confirm { text } => {
                            question_sequence += 1;
                            let answer = ask_until_answer(
                                interaction,
                                &format!("package-confirm-{question_sequence}"),
                                &format!(
                                    "This is the running package manager's actual plan:\n\n{text}\nContinue?"
                                ),
                                &["Continue".into(), "Cancel".into()],
                                false,
                            );
                            declined |= answer == "Cancel";
                            // A client-side preview never supplies this answer.
                            if let Err(error) = write_input(
                                &mut writer,
                                if answer == "Continue" { b"y\n" } else { b"n\n" },
                            ) {
                                output_error.get_or_insert_with(|| error.to_string());
                            }
                        }
                        ObservedPrompt::Conffile { text } => {
                            question_sequence += 1;
                            let answer = ask_until_answer(
                                interaction,
                                &format!("package-config-{question_sequence}"),
                                &format!(
                                    "A locally changed configuration file conflicts with the package version:\n{text}\nChoose which file to keep."
                                ),
                                &["Keep current".into(), "Install package version".into()],
                                true,
                            );
                            if let Err(error) = write_input(
                                &mut writer,
                                if answer == "Keep current" {
                                    b"n\n"
                                } else {
                                    b"y\n"
                                },
                            ) {
                                output_error.get_or_insert_with(|| error.to_string());
                            }
                        }
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
            interaction.emit(OperationEvent::Progress { message: "The package task is still running. Open it again to answer pending questions; active package writes cannot be forcibly cancelled.".into(), percent: None });
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
    if declined {
        return Err(ManagementError::Cancelled);
    }
    if !status.success() {
        let output = runtime_log::sanitize_text(&String::from_utf8_lossy(&tail));
        return Err(ManagementError::Failed(format!(
            "{} failed with exit code {}. Changes may be partially applied; review the package output and database before retrying.\n{output}",
            backend.id(),
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

fn ask_until_answer(
    interaction: &mut dyn OperationInteraction,
    id: &str,
    prompt: &str,
    choices: &[String],
    applying: bool,
) -> String {
    loop {
        match interaction.ask(id, prompt, choices, false) {
            Ok(answer) if choices.contains(&answer) => return answer,
            Err(ManagementError::Cancelled) if !applying => return "Cancel".into(),
            Ok(_) => interaction.emit(OperationEvent::Output {
                text: "Choose one of the displayed package-manager answers".into(),
            }),
            Err(_) => {
                // In particular, a UI timeout must not close the PTY, kill dpkg,
                // or silently choose a configuration replacement.
                std::thread::sleep(Duration::from_millis(250));
            }
        }
    }
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
mod tests {
    use super::*;
    use std::collections::VecDeque;
    use std::path::PathBuf;

    #[derive(Default)]
    struct Interaction {
        output: Vec<u8>,
        questions: Vec<String>,
        disconnects: usize,
        answers: VecDeque<String>,
        terminal: VecDeque<Vec<u8>>,
    }

    impl OperationInteraction for Interaction {
        fn emit(&mut self, event: OperationEvent) {
            if let OperationEvent::TerminalOutput { bytes } = event {
                self.output.extend(bytes);
            }
        }
        fn ask(
            &mut self,
            _id: &str,
            prompt: &str,
            choices: &[String],
            _secret: bool,
        ) -> Result<String, ManagementError> {
            self.questions.push(prompt.into());
            if self.disconnects > 0 {
                self.disconnects -= 1;
                return Err(ManagementError::Failed("client disconnected".into()));
            }
            Ok(self
                .answers
                .pop_front()
                .unwrap_or_else(|| choices[0].clone()))
        }
        fn terminal_input(
            &mut self,
            _timeout: Duration,
        ) -> Result<Option<Vec<u8>>, ManagementError> {
            Ok(self.terminal.pop_front())
        }
    }

    fn fixture(script: &str) -> PackageCommand {
        // Fixed test-only scripts never install or modify a package.
        PackageCommand {
            program: PathBuf::from("/bin/sh"),
            args: vec!["-c".into(), script.into()],
        }
    }

    #[test]
    fn input_cannot_interrupt_suspend_or_eof_the_package_manager() {
        assert_eq!(safe_terminal_input(b"hello\x03\x04\x1a\x1c\r"), b"hello\r");
        assert_eq!(safe_terminal_input(b"\x1b[A"), b"\x1b[A");
    }

    #[test]
    fn real_pty_keeps_waiting_for_confirmation_after_client_disconnect() {
        let spec = fixture(
            r#"printf 'The following NEW packages will be installed:\n dependency\nDo you want to continue? [Y/n] '; read answer; test "$answer" = y || exit 7; printf '\nprocessing: configure: demo\n'; printf "status: /etc/demo.conf : conffile-prompt : '/etc/demo.conf' '/etc/demo.conf.dpkg-new' 1 1\n"; read config; test "$config" = n || exit 8; printf 'COMPLETED-FIXTURE\n'"#,
        );
        let mut interaction = Interaction {
            disconnects: 1,
            ..Default::default()
        };
        let result = execute(
            spec,
            PackageBackend::Apt,
            &mut interaction,
            &AtomicBool::new(false),
        )
        .unwrap();
        assert!(result.contains("exit code 0"));
        assert!(String::from_utf8_lossy(&interaction.output).contains("COMPLETED-FIXTURE"));
        assert!(
            interaction
                .questions
                .iter()
                .any(|question| question.contains("dependency"))
        );
        assert!(
            interaction
                .questions
                .iter()
                .any(|question| question.contains("/etc/demo.conf"))
        );
    }

    #[test]
    fn real_pty_rejects_current_plan_without_applying_changes() {
        let spec = fixture(
            "printf 'Do you want to continue? [Y/n] '; read answer; test \"$answer\" = y || exit 1; printf 'UNEXPECTED-APPLY\n'",
        );
        let mut interaction = Interaction {
            answers: VecDeque::from(["Cancel".into()]),
            ..Default::default()
        };
        assert_eq!(
            execute(
                spec,
                PackageBackend::Apt,
                &mut interaction,
                &AtomicBool::new(false)
            ),
            Err(ManagementError::Cancelled)
        );
        assert!(!String::from_utf8_lossy(&interaction.output).contains("UNEXPECTED-APPLY"));
    }

    #[test]
    fn real_pty_reports_actual_script_failure_and_partial_state() {
        let spec = fixture(
            "printf 'processing: configure: demo\n'; printf 'maintainer script failed\n'; exit 23",
        );
        let mut interaction = Interaction::default();
        let error = execute(
            spec,
            PackageBackend::Apt,
            &mut interaction,
            &AtomicBool::new(false),
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("23"));
        assert!(error.contains("partially applied"));
        assert!(error.contains("maintainer script failed"));
    }

    #[test]
    fn script_interaction_is_delivered_to_the_pty_without_signal_keys() {
        let spec = fixture(
            "printf 'processing: configure: demo\n'; read answer; test \"$answer\" = expected || exit 31; printf 'SCRIPT-ANSWERED\n'",
        );
        let mut interaction = Interaction {
            terminal: VecDeque::from([b"\x03\x1aexpected\r".to_vec()]),
            ..Default::default()
        };
        let result = execute(
            spec,
            PackageBackend::Apt,
            &mut interaction,
            &AtomicBool::new(false),
        )
        .unwrap();
        assert!(result.contains("exit code 0"));
        assert!(String::from_utf8_lossy(&interaction.output).contains("SCRIPT-ANSWERED"));
    }
}
