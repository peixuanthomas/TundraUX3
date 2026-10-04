use super::*;
use std::{collections::VecDeque, path::PathBuf};

#[derive(Default)]
struct Interaction {
    output: Vec<u8>,
    progress: Vec<String>,
    answers: VecDeque<(&'static str, Vec<u8>)>,
    calls: usize,
}
impl OperationInteraction for Interaction {
    fn emit(&mut self, event: OperationEvent) {
        match event {
            OperationEvent::TerminalOutput { bytes } => self.output.extend(bytes),
            OperationEvent::Progress { message, .. } => self.progress.push(message),
            _ => {}
        }
    }
    fn ask(&mut self, _: &str, _: &str, _: &[String], _: bool) -> Result<String, ManagementError> {
        panic!("package prompts must remain in the real terminal")
    }
    fn terminal_input(&mut self, _: Duration) -> Result<Option<Vec<u8>>, ManagementError> {
        self.calls += 1;
        // Simulate a disconnected viewer before it returns to answer the prompt.
        if self.calls < 5 {
            return Err(ManagementError::Failed("disconnected".into()));
        }
        if self
            .answers
            .front()
            .is_some_and(|(prompt, _)| String::from_utf8_lossy(&self.output).contains(prompt))
        {
            Ok(self.answers.pop_front().map(|(_, bytes)| bytes))
        } else {
            Ok(None)
        }
    }
}

#[path = "apt_dpkg_tests.rs"]
mod apt_dpkg_tests;

fn fixture(script: &str) -> PackageCommand {
    PackageCommand {
        program: PathBuf::from("/usr/bin/timeout"),
        args: vec!["8".into(), "/bin/sh".into(), "-c".into(), script.into()],
    }
}

#[test]
fn real_pty_answers_yn_conffile_and_unrecognized_questions_after_reconnect() {
    let mut io = Interaction {
        answers: VecDeque::from([
            ("[Y/n]", b"y\r".to_vec()),
            ("[Y/I/N/O/D/Z]", b"n\r".to_vec()),
            ("Custom question:", "中文\r".as_bytes().to_vec()),
        ]),
        ..Default::default()
    };
    let spec = fixture(
        r#"printf 'Do you want to continue? [Y/n] '; read a; test "$a" = y || exit 7; printf '\nConfiguration file /etc/demo.conf [Y/I/N/O/D/Z] '; read b; test "$b" = n || exit 8; printf '\nCustom question: '; read c; test "$c" = 中文 || exit 9; printf '\nCOMPLETED\n'"#,
    );
    let result = execute(spec, PackageBackend::Apt, &mut io, &AtomicBool::new(false)).unwrap();
    assert!(result.contains("exit code 0"));
    assert!(String::from_utf8_lossy(&io.output).contains("COMPLETED"));
    assert!(io.answers.is_empty());
}

#[test]
fn real_pty_no_rejects_the_actual_plan() {
    let mut io = Interaction {
        answers: VecDeque::from([("[y/N]", b"n\r".to_vec())]),
        ..Default::default()
    };
    let spec = fixture(
        r#"printf 'Is this ok [y/N]: '; read a; test "$a" = y || exit 1; printf UNEXPECTED-APPLY"#,
    );
    assert!(execute(spec, PackageBackend::Dnf4, &mut io, &AtomicBool::new(false)).is_err());
    assert!(!String::from_utf8_lossy(&io.output).contains("UNEXPECTED-APPLY"));
}

#[test]
fn real_pty_reports_exit_code_and_partial_failure() {
    let mut io = Interaction::default();
    let error = execute(
        fixture("printf 'maintainer script failed'; exit 23"),
        PackageBackend::Apt,
        &mut io,
        &AtomicBool::new(false),
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("23"));
    assert!(error.contains("partially applied"));
    assert!(error.contains("maintainer script failed"));
}

#[test]
fn input_does_not_interrupt_suspend_or_eof_active_package_writes() {
    assert_eq!(
        safe_terminal_input(b"hello\x03\x04\x1a\x1c\r\x1b[A"),
        b"hello\r\x1b[A"
    );
}
