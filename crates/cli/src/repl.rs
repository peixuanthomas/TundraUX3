use std::process::Command;

use rustyline::error::ReadlineError;
use rustyline::history::MemHistory;
use rustyline::{Config, Editor};

use crate::{CliCommand, parse_args};

/// Reserved process status used by the embedded REPL to ask its parent Shell
/// to perform the destructive storage reset and restart itself.
pub const EMBEDDED_RESET_EXIT_CODE: i32 = 75;

/// Private environment contract with the Shell's embedded Command Line host.
const COMMAND_LINE_USERNAME_ENV: &str = "TUNDRA_COMMAND_LINE_USERNAME";

/// Runs an interactive Tundra command loop. The callback is deliberately the
/// normal CLI dispatcher, so REPL input cannot drift from the regular CLI
/// command surface.
pub(crate) fn run_repl<F>(embedded: bool, mut execute_cli: F) -> i32
where
    F: FnMut(&[String]) -> i32,
{
    let config = match Config::builder().history_ignore_dups(true) {
        Ok(builder) => builder.build(),
        Err(error) => {
            eprintln!("ERROR: could not configure command line: {error}");
            return 1;
        }
    };
    let mut editor = match Editor::<(), MemHistory>::with_history(config, MemHistory::new()) {
        Ok(editor) => editor,
        Err(error) => {
            eprintln!("ERROR: could not start command line: {error}");
            return 1;
        }
    };
    let prompt = repl_prompt(embedded);

    loop {
        let line = match editor.readline(&prompt) {
            Ok(line) => line,
            Err(ReadlineError::Interrupted) => continue,
            Err(ReadlineError::Eof) => return 0,
            Err(error) => {
                eprintln!("ERROR: command line input failed: {error}");
                return 1;
            }
        };
        if line.is_empty() {
            continue;
        }
        if is_exit_line(&line) {
            return 0;
        }
        let _ = editor.add_history_entry(&line);

        if let Some(system_command) = line.strip_prefix('/') {
            let _ = run_system_command(system_command);
            continue;
        }

        let arguments = match shlex::split(&line) {
            Some(arguments) => arguments,
            None => {
                eprintln!("ERROR: could not parse command line: unmatched quote");
                continue;
            }
        };
        if arguments.is_empty() {
            continue;
        }

        match parse_args(&arguments) {
            Ok(CliCommand::Repl { .. }) => {
                eprintln!("ERROR: repl cannot be started from inside repl");
            }
            Ok(CliCommand::New) => {
                if confirm_reset(&mut editor) {
                    if embedded {
                        println!("TundraUX3 reset requested; returning control to Launcher.");
                        return EMBEDDED_RESET_EXIT_CODE;
                    }
                    let _ = execute_cli(&arguments);
                } else {
                    println!("Reset cancelled.");
                }
            }
            Ok(_) => {
                let _ = execute_cli(&arguments);
            }
            Err(error) => {
                eprintln!("ERROR: {error}");
            }
        }
    }
}

fn repl_prompt(embedded: bool) -> String {
    let username = if embedded {
        std::env::var(COMMAND_LINE_USERNAME_ENV).ok()
    } else {
        standalone_username()
    };
    prompt_for_username(username.as_deref())
}

fn standalone_username() -> Option<String> {
    ["USERNAME", "USER"]
        .into_iter()
        .find_map(|name| std::env::var(name).ok())
}

fn prompt_for_username(username: Option<&str>) -> String {
    let username = username
        .map(str::trim)
        .filter(|username| is_safe_prompt_username(username))
        .unwrap_or("tundra");
    format!("{username} >> ")
}

fn is_safe_prompt_username(username: &str) -> bool {
    !username.is_empty()
        && username.len() <= 64
        && username.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.')
        })
}

fn confirm_reset(editor: &mut Editor<(), MemHistory>) -> bool {
    match editor.readline("Type RESET to erase TundraUX3 data, or press Enter to cancel: ") {
        Ok(answer) => is_reset_confirmation(&answer),
        Err(ReadlineError::Interrupted | ReadlineError::Eof) => false,
        Err(error) => {
            eprintln!("ERROR: command line input failed: {error}");
            false
        }
    }
}

fn is_exit_line(line: &str) -> bool {
    line == "exit"
}

fn is_reset_confirmation(answer: &str) -> bool {
    answer == "RESET"
}

/// Executes the bytes following `/` unchanged and returns the operating
/// system command's status. The REPL intentionally remains open afterwards.
fn run_system_command(command: &str) -> i32 {
    if command.trim().is_empty() {
        eprintln!("ERROR: '/' must be followed by an operating-system command");
        return 2;
    }

    let result = if cfg!(windows) {
        Command::new("cmd.exe")
            .args(["/D", "/S", "/C", command])
            .status()
    } else {
        Command::new("/bin/sh").args(["-lc", command]).status()
    };
    match result {
        Ok(status) => {
            let exit_code = status.code().unwrap_or(1);
            println!("[system exit code: {exit_code}]");
            exit_code
        }
        Err(error) => {
            eprintln!("ERROR: could not run operating-system command: {error}");
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_repl_exit_code_is_reserved_for_the_parent_shell() {
        assert_eq!(EMBEDDED_RESET_EXIT_CODE, 75);
    }

    #[test]
    fn prompt_uses_the_username_and_new_chevrons() {
        assert_eq!(
            prompt_for_username(Some("AdminUser")),
            "AdminUser >> ".to_string()
        );
        assert_eq!(prompt_for_username(None), "tundra >> ".to_string());
    }

    #[test]
    fn prompt_rejects_untrusted_environment_values() {
        assert_eq!(
            prompt_for_username(Some("user\u{1b}[31m")),
            "tundra >> ".to_string()
        );
        assert_eq!(
            prompt_for_username(Some(" user.name ")),
            "user.name >> ".to_string()
        );
    }

    #[test]
    fn repl_parser_keeps_shell_escapes_out_of_cli_parsing() {
        assert_eq!(
            shlex::split("config set address 'New York'"),
            Some(vec![
                "config".to_string(),
                "set".to_string(),
                "address".to_string(),
                "New York".to_string(),
            ])
        );
        assert_eq!("/dir".strip_prefix('/'), Some("dir"));
    }

    #[test]
    fn exit_and_reset_words_are_exact() {
        assert!(is_exit_line("exit"));
        assert!(!is_exit_line(" exit"));
        assert!(!is_exit_line("exit "));
        assert!(is_reset_confirmation("RESET"));
        assert!(!is_reset_confirmation("reset"));
        assert!(!is_reset_confirmation("RESET "));
    }

    #[test]
    fn system_command_returns_its_exit_code() {
        let command = if cfg!(windows) { "exit /B 7" } else { "exit 7" };
        assert_eq!(run_system_command(command), 7);
    }
}
