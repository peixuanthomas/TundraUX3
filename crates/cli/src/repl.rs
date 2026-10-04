use std::{borrow::Cow, io::Write};

use platform::SystemCommandSession;
use rustyline::error::ReadlineError;
use rustyline::history::MemHistory;
use rustyline::{
    Config, Editor, Helper, completion::Completer, highlight::Highlighter, hint::Hinter,
    validate::Validator,
};

struct PromptDisplay;

impl Completer for PromptDisplay {
    type Candidate = String;
}
impl Hinter for PromptDisplay {
    type Hint = String;
}
impl Validator for PromptDisplay {}
impl Helper for PromptDisplay {}

impl Highlighter for PromptDisplay {
    fn highlight_prompt<'b, 's: 'b, 'p: 'b>(
        &'s self,
        prompt: &'p str,
        default: bool,
    ) -> Cow<'b, str> {
        if default && let Some(rest) = prompt.strip_prefix('○') {
            // Only decorate the rendered prompt. Windows rustyline counts CSI
            // bytes as visible text when calculating the undecorated layout.
            Cow::Owned(format!("○\x1b[777;0z{rest}"))
        } else {
            Cow::Borrowed(prompt)
        }
    }
}

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
        // Emacs mode otherwise waits indefinitely after Escape and consumes
        // the next ordinary character as an Alt binding (e.g. the e in exit).
        // The embedded host writes complete escape sequences atomically.
        Ok(builder) => builder
            .keyseq_timeout(Some(100))
            .color_mode(if embedded {
                // This hook carries status metadata, not optional prompt colors.
                rustyline::ColorMode::Forced
            } else {
                rustyline::ColorMode::Enabled
            })
            .build(),
        Err(error) => {
            eprintln!("ERROR: could not configure command line: {error}");
            return 1;
        }
    };
    let mut editor =
        match Editor::<PromptDisplay, MemHistory>::with_history(config, MemHistory::new()) {
            Ok(editor) => editor,
            Err(error) => {
                eprintln!("ERROR: could not start command line: {error}");
                return 1;
            }
        };
    if embedded {
        editor.set_helper(Some(PromptDisplay));
    }
    let mut system_session = None;

    loop {
        let prompt = repl_prompt(embedded, system_session.as_ref());
        // Keep layout input free of terminal controls on every platform.
        // PromptDisplay adds the host's marker metadata only during rendering.
        let prompt = if embedded {
            format!("○ {prompt}")
        } else {
            prompt
        };
        let line = match editor.readline(&prompt) {
            Ok(line) => line,
            Err(ReadlineError::Interrupted) => {
                report_command_status(embedded, 1);
                continue;
            }
            Err(ReadlineError::Eof) => return 0,
            Err(error) => {
                eprintln!("ERROR: command line input failed: {error}");
                return 1;
            }
        };
        let line = line.trim_start();
        if line.is_empty() {
            continue;
        }
        if is_exit_line(&line) {
            report_command_status(embedded, 0);
            return 0;
        }
        let _ = editor.add_history_entry(line);

        if let Some(system_command) = line.strip_prefix('/') {
            let code = run_system_command(&mut system_session, system_command);
            report_command_status(embedded, code);
            continue;
        }

        let mut arguments = match shlex::split(line) {
            Some(arguments) => arguments,
            None => {
                eprintln!("ERROR: could not parse command line: unmatched quote");
                report_command_status(embedded, 1);
                continue;
            }
        };
        if arguments.is_empty() {
            continue;
        }

        if matches!(
            parse_args(&arguments),
            Ok(CliCommand::Launcher(crate::LauncherAction::Pin(_)))
        ) {
            if let Some(session) = system_session.as_ref() {
                let path = std::path::Path::new(&arguments[2]);
                if path.is_relative() {
                    arguments[2] = session
                        .current_dir()
                        .join(path)
                        .to_string_lossy()
                        .into_owned();
                }
            }
        }

        let code = match parse_args(&arguments) {
            Ok(CliCommand::TestWatchdogPanic) if embedded => {
                println!("Triggering a real Shell panic; the current session will be discarded.");
                return shell::COMMAND_LINE_PANIC_EXIT_CODE as i32;
            }
            Ok(CliCommand::Repl { .. }) => {
                eprintln!("ERROR: repl cannot be started from inside repl");
                1
            }
            Ok(CliCommand::New) => {
                if confirm_reset(&mut editor) {
                    if embedded {
                        println!("TundraUX3 reset requested; returning control to Launcher.");
                        return EMBEDDED_RESET_EXIT_CODE;
                    }
                    execute_cli(&arguments)
                } else {
                    println!("Reset cancelled.");
                    1
                }
            }
            Ok(_) => execute_cli(&arguments),
            Err(error) => {
                eprintln!("ERROR: {error}");
                let likely_system_command = matches!(error, crate::CliError::UnknownCommand(_))
                    && system_session
                        .as_ref()
                        .map(|session| session.is_likely_command(&arguments[0]))
                        .unwrap_or_else(|| {
                            SystemCommandSession::new()
                                .is_ok_and(|session| session.is_likely_command(&arguments[0]))
                        });
                if likely_system_command {
                    let example = line
                        .chars()
                        .map(|character| {
                            if character.is_control() {
                                character.escape_default().to_string()
                            } else {
                                character.to_string()
                            }
                        })
                        .collect::<String>();
                    eprintln!(
                        "This looks like a system command. Prefix it with '/': /{}",
                        example
                    );
                    eprintln!("这可能是系统命令；执行系统命令需要前缀“/”。本次未执行。");
                } else {
                    let _ = crate::help_text::write_error_help(&mut std::io::stderr(), &arguments);
                }
                2
            }
        };
        report_command_status(embedded, code);
    }
}

/// Only the embedded terminal interprets this private, bounded CSI protocol.
fn report_command_status(embedded: bool, code: i32) {
    if embedded {
        print!("\x1b[777;1;{}z", u8::from(code != 0));
        let _ = std::io::stdout().flush();
    }
}

fn repl_prompt(embedded: bool, system_session: Option<&SystemCommandSession>) -> String {
    let username = if embedded {
        std::env::var(COMMAND_LINE_USERNAME_ENV).ok()
    } else {
        standalone_username()
    };
    let directory = system_session
        .map(|session| session.current_dir().to_path_buf())
        .or_else(|| std::env::current_dir().ok());
    prompt_for_username(username.as_deref(), directory.as_deref())
}

fn standalone_username() -> Option<String> {
    ["USERNAME", "USER"]
        .into_iter()
        .find_map(|name| std::env::var(name).ok())
}

fn prompt_for_username(username: Option<&str>, directory: Option<&std::path::Path>) -> String {
    let username = username
        .map(str::trim)
        .filter(|username| is_safe_prompt_username(username))
        .unwrap_or("tundra");
    let mut path = String::new();
    if let Some(directory) = directory {
        for character in directory.to_string_lossy().chars() {
            // Filenames can contain terminal controls or line breaks. Display
            // those visibly without letting them move the cursor or hide text.
            if character.is_control() {
                path.extend(character.escape_default());
            } else {
                path.push(character);
            }
        }
    } else {
        path.push('?');
    }
    format!("{username}@{path} >> ")
}

fn is_safe_prompt_username(username: &str) -> bool {
    !username.is_empty()
        && username.len() <= 64
        && username.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.')
        })
}

fn confirm_reset(editor: &mut Editor<PromptDisplay, MemHistory>) -> bool {
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
fn run_system_command(session: &mut Option<SystemCommandSession>, command: &str) -> i32 {
    if command.trim().is_empty() {
        eprintln!("ERROR: '/' must be followed by an operating-system command");
        return 2;
    }

    let result = (|| {
        if session.is_none() {
            *session = Some(SystemCommandSession::new()?);
        }
        session
            .as_mut()
            .expect("initialized system session")
            .run(command)
    })();
    match result {
        Ok(result) => {
            let exit_code = result.exit_code;
            println!("[system exit code: {exit_code}]");
            if let Some(error) = result.state_error {
                eprintln!(
                    "WARNING: could not retain command state; the next system command will use the last saved environment and directory: {error}"
                );
            }
            exit_code
        }
        Err(error) => {
            eprintln!("ERROR: could not run operating-system command: {error}");
            1
        }
    }
}

#[cfg(test)]
#[path = "../tests/unit/repl/tests.rs"]
mod tests;
