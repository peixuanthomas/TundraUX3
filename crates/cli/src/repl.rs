use std::{borrow::Cow, io::Write};

use platform::SystemCommandSession;
use rustyline::error::ReadlineError;
use rustyline::history::MemHistory;
use rustyline::{
    Config, Editor, Helper, completion::Completer, highlight::Highlighter, hint::Hinter,
    validate::Validator,
};

struct PromptDisplay {
    accent: String,
}

impl PromptDisplay {
    fn new() -> Self {
        Self {
            accent: std::env::var("TUNDRA_COMMAND_LINE_ACCENT")
                .ok()
                .and_then(|value| validated_foreground(&value))
                .unwrap_or_else(|| "\x1b[38;2;99;211;229m".to_string()),
        }
    }
}

fn validated_foreground(value: &str) -> Option<String> {
    let parameters = value.strip_prefix("\x1b[")?.strip_suffix('m')?;
    let values = parameters
        .split(';')
        .map(str::parse::<u8>)
        .collect::<Result<Vec<_>, _>>()
        .ok()?;
    matches!(
        values.as_slice(),
        [30..=37 | 39 | 90..=97] | [38, 5, _] | [38, 2, _, _, _]
    )
    .then(|| value.to_string())
}

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
        if default
            && let Some(identity) = prompt
                .strip_prefix("○ ")
                .and_then(|rest| rest.strip_suffix(" >> "))
        {
            // Only decorate the rendered prompt. Windows rustyline counts CSI
            // bytes as visible text when calculating the undecorated layout.
            Cow::Owned(format!(
                "○\x1b[777;0z {}{identity}\x1b[39m >> ",
                self.accent
            ))
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
                // The helper carries both prompt colors and host status metadata.
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
        editor.set_helper(Some(PromptDisplay::new()));
    }
    let mut system_session = None;
    println!("System commands run by default. Use /help for UX commands; exit to leave.");
    let mut separate_next_prompt = false;

    loop {
        if separate_next_prompt {
            println!();
            separate_next_prompt = false;
        }
        let prompt = repl_prompt(embedded, system_session.as_ref());
        // Keep layout input free of terminal controls on every platform.
        // PromptDisplay adds colors and marker metadata only during rendering.
        let prompt = if embedded {
            format!("○ {prompt}")
        } else {
            prompt
        };
        let line = match editor.readline(&prompt) {
            Ok(line) => line,
            Err(ReadlineError::Interrupted) => {
                report_command_status(embedded, 1);
                separate_next_prompt = true;
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
        separate_next_prompt = true;
        if is_exit_line(&line) {
            report_command_status(embedded, 0);
            return 0;
        }
        let _ = editor.add_history_entry(line);

        let Some(ux_command) = ux_command_input(line) else {
            let code = run_system_command(&mut system_session, line, embedded);
            if code != 0 && is_likely_ux_command(line) {
                eprintln!(
                    "This looks like a UX command. Prefix it with '/': /{}",
                    visible_command(line)
                );
            }
            report_command_status(embedded, code);
            continue;
        };
        if ux_command.trim().is_empty() {
            eprintln!("ERROR: '/' must be followed by a UX command; use /help");
            report_command_status(embedded, 2);
            continue;
        }

        let mut arguments = match shlex::split(ux_command) {
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
            Ok(CliCommand::ScreenKeyboard) if embedded => {
                eprintln!(
                    "ERROR: screen keyboard requires an external terminal. Run: tundra-cli debug screen-keyboard"
                );
                1
            }
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
                    eprintln!(
                        "This looks like a system command. Remove the '/' prefix: {}",
                        visible_command(ux_command)
                    );
                } else {
                    let _ = crate::help_text::write_error_help(&mut std::io::stderr(), &arguments);
                }
                2
            }
        };
        report_command_status(embedded, code);
    }
}

fn ux_command_input(line: &str) -> Option<&str> {
    let command = line.strip_prefix('/')?;
    let name = command.split_whitespace().next().unwrap_or_default();
    // An absolute executable path such as /usr/bin/ls is ordinary system
    // input. A single leading slash such as /help selects a UX command.
    (!name.contains('/')).then_some(command)
}

fn is_likely_ux_command(line: &str) -> bool {
    shlex::split(line).is_some_and(|arguments| {
        !arguments.is_empty()
            && !matches!(
                parse_args(arguments),
                Err(crate::CliError::UnknownCommand(_))
            )
    })
}

fn visible_command(command: &str) -> String {
    let mut visible = String::new();
    for character in command.chars() {
        if character.is_control() {
            visible.extend(character.escape_default());
        } else {
            visible.push(character);
        }
    }
    visible
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

/// Executes system input unchanged and returns the operating
/// system command's status. The REPL intentionally remains open afterwards.
fn run_system_command(
    session: &mut Option<SystemCommandSession>,
    command: &str,
    embedded: bool,
) -> i32 {
    if command.trim().is_empty() {
        eprintln!("ERROR: an operating-system command is required");
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
            if embedded {
                println!("\x1b[90m[system exit code: {exit_code}]\x1b[39m");
            } else {
                println!("[system exit code: {exit_code}]");
            }
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
