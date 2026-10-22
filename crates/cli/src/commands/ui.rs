use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::time::{Duration, Instant};

use shell_control::{Action, Button, Phase, Request, Response, SessionInfo, Target};

use crate::CliError;

pub(crate) const VERBS: &[&str] = &[
    "sessions", "snapshot", "click", "move", "drag", "scroll", "key", "text", "wait",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UiCommand {
    Help,
    Sessions,
    Control(UiRequest),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UiRequest {
    pub session: Option<String>,
    pub snapshot: Option<u64>,
    pub timeout_ms: u64,
    pub action: UiAction,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UiAction {
    Input(Action),
    Stdin,
    Wait { id: String, state: WaitState },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WaitState {
    Present,
    Absent,
    Actionable,
}

pub(crate) fn parse_ui(args: &[String]) -> Result<UiCommand, CliError> {
    let Some((verb, args)) = args.split_first() else {
        return Ok(UiCommand::Help);
    };
    if args.is_empty() && matches!(verb.as_str(), "help" | "--help" | "-h") {
        return Ok(UiCommand::Help);
    }
    if !VERBS.contains(&verb.as_str()) {
        return Err(CliError::InvalidUiArgument);
    }
    if matches!(args, [help] if matches!(help.as_str(), "--help" | "-h")
        || (help == "help" && !matches!(verb.as_str(), "key" | "text")))
    {
        return Ok(UiCommand::Help);
    }
    if verb == "sessions" {
        return if args.is_empty() {
            Ok(UiCommand::Sessions)
        } else {
            Err(CliError::InvalidUiArgument)
        };
    }

    let mut options = BTreeMap::new();
    let mut value = None;
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        if arg == "--" {
            for arg in args.by_ref() {
                if value.replace(arg.clone()).is_some() {
                    return Err(CliError::InvalidUiArgument);
                }
            }
        } else if arg.starts_with('-') {
            if !allowed_options(verb).contains(&arg.as_str()) {
                return Err(CliError::InvalidUiArgument);
            }
            let option = if matches!(arg.as_str(), "--double" | "--stdin") {
                String::new()
            } else {
                args.next()
                    .filter(|value| !value.is_empty() && !value.starts_with("--"))
                    .ok_or(CliError::InvalidUiArgument)?
                    .clone()
            };
            if options.insert(arg.as_str(), option).is_some() {
                return Err(CliError::InvalidUiArgument);
            }
        } else if value.replace(arg.clone()).is_some() {
            return Err(CliError::InvalidUiArgument);
        }
    }
    if value.is_some() && !matches!(verb.as_str(), "key" | "text") {
        return Err(CliError::InvalidUiArgument);
    }
    let session = options.remove("--session");
    let snapshot = options
        .remove("--snapshot")
        .map(|value| number(&value))
        .transpose()?;
    let timeout_ms = option_number(&mut options, "--timeout", 5000)?;
    if timeout_ms == 0 {
        return Err(CliError::InvalidUiArgument);
    }
    let button = match options.remove("--button").as_deref().unwrap_or("left") {
        "left" => Button::Left,
        "right" => Button::Right,
        "middle" => Button::Middle,
        _ => return Err(CliError::InvalidUiArgument),
    };
    let action = match verb.as_str() {
        "snapshot" => UiAction::Input(Action::Snapshot),
        "click" => UiAction::Input(Action::Click {
            target: target(&mut options, "--id", "--x", "--y")?,
            button,
            double: options.remove("--double").is_some(),
        }),
        "move" => UiAction::Input(Action::Move {
            target: target(&mut options, "--id", "--x", "--y")?,
        }),
        "drag" => {
            let duration_ms = option_number(&mut options, "--duration", 300)?;
            if !(16..=10_000).contains(&duration_ms) {
                return Err(CliError::InvalidUiArgument);
            }
            UiAction::Input(Action::Drag {
                from: target(&mut options, "--from-id", "--from-x", "--from-y")?,
                to: target(&mut options, "--to-id", "--to-x", "--to-y")?,
                duration_ms,
                button,
            })
        }
        "scroll" => {
            let direction = options
                .remove("--direction")
                .ok_or(CliError::InvalidUiArgument)?;
            let count = option_number(&mut options, "--count", 1)?;
            if !matches!(direction.as_str(), "up" | "down" | "left" | "right")
                || !(1..=1000).contains(&count)
            {
                return Err(CliError::InvalidUiArgument);
            }
            UiAction::Input(Action::Scroll {
                target: target(&mut options, "--id", "--x", "--y")?,
                direction,
                count,
            })
        }
        "key" => {
            let key = value.ok_or(CliError::InvalidUiArgument)?;
            if !valid_key(&key) {
                return Err(CliError::InvalidUiArgument);
            }
            let mut modifiers = Vec::new();
            if let Some(value) = options.remove("--modifiers") {
                for modifier in value.split(',') {
                    let modifier = modifier.to_ascii_lowercase();
                    let modifier = if modifier == "control" {
                        "ctrl".into()
                    } else {
                        modifier
                    };
                    if !matches!(
                        modifier.as_str(),
                        "ctrl" | "alt" | "shift" | "super" | "hyper" | "meta"
                    ) || modifiers.contains(&modifier)
                    {
                        return Err(CliError::InvalidUiArgument);
                    }
                    modifiers.push(modifier);
                }
            }
            let phase = match options.remove("--phase").as_deref().unwrap_or("press") {
                "press" => Phase::Press,
                "repeat" => Phase::Repeat,
                "release" => Phase::Release,
                _ => return Err(CliError::InvalidUiArgument),
            };
            UiAction::Input(Action::Key {
                key,
                modifiers,
                phase,
            })
        }
        "text" => match (value, options.remove("--stdin").is_some()) {
            (Some(text), false) => UiAction::Input(Action::Text { text }),
            (None, true) => UiAction::Stdin,
            _ => return Err(CliError::InvalidUiArgument),
        },
        "wait" => {
            let id = options.remove("--id").ok_or(CliError::InvalidUiArgument)?;
            let state = match options.remove("--state").as_deref().unwrap_or("actionable") {
                "present" => WaitState::Present,
                "absent" => WaitState::Absent,
                "actionable" => WaitState::Actionable,
                _ => return Err(CliError::InvalidUiArgument),
            };
            UiAction::Wait { id, state }
        }
        _ => return Err(CliError::InvalidUiArgument),
    };
    Ok(UiCommand::Control(UiRequest {
        session,
        snapshot,
        timeout_ms,
        action,
    }))
}

pub(crate) fn allowed_options(verb: &str) -> Vec<&'static str> {
    if verb == "sessions" || !VERBS.contains(&verb) {
        return Vec::new();
    }
    let mut flags = vec!["--session", "--snapshot", "--timeout"];
    flags.extend_from_slice(match verb {
        "click" => &["--id", "--x", "--y", "--button", "--double"],
        "move" => &["--id", "--x", "--y"],
        "drag" => &[
            "--from-id",
            "--from-x",
            "--from-y",
            "--to-id",
            "--to-x",
            "--to-y",
            "--duration",
            "--button",
        ],
        "scroll" => &["--id", "--x", "--y", "--direction", "--count"],
        "key" => &["--modifiers", "--phase"],
        "text" => &["--stdin"],
        "wait" => &["--id", "--state"],
        _ => &[],
    });
    flags
}

fn number<T: std::str::FromStr>(value: &str) -> Result<T, CliError> {
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(CliError::InvalidUiArgument);
    }
    value.parse().map_err(|_| CliError::InvalidUiArgument)
}

fn option_number<T: std::str::FromStr>(
    options: &mut BTreeMap<&str, String>,
    name: &str,
    default: T,
) -> Result<T, CliError> {
    options
        .remove(name)
        .map_or(Ok(default), |value| number(&value))
}

fn target(
    options: &mut BTreeMap<&str, String>,
    id: &str,
    x: &str,
    y: &str,
) -> Result<Target, CliError> {
    match (options.remove(id), options.remove(x), options.remove(y)) {
        (Some(id), None, None) => Ok(Target::Id { id }),
        (None, Some(x), Some(y)) => Ok(Target::Position {
            x: number(&x)?,
            y: number(&y)?,
        }),
        _ => Err(CliError::InvalidUiArgument),
    }
}

fn valid_key(key: &str) -> bool {
    if key.chars().count() == 1 {
        return !key.chars().any(char::is_control);
    }
    let key = key.to_ascii_lowercase();
    matches!(
        key.as_str(),
        "enter"
            | "esc"
            | "escape"
            | "tab"
            | "backtab"
            | "backspace"
            | "space"
            | "delete"
            | "insert"
            | "left"
            | "right"
            | "up"
            | "down"
            | "home"
            | "end"
            | "pageup"
            | "pagedown"
    ) || key
        .strip_prefix('f')
        .is_some_and(|value| number::<u8>(value).is_ok_and(|value| (1..=24).contains(&value)))
}

pub(crate) fn help(output: &mut impl Write) -> std::io::Result<()> {
    writeln!(output, "{}", i18n::tr!("ui-control-help"))
}

pub(crate) fn invalid_arguments(output: &mut impl Write) -> i32 {
    write_response(
        output,
        &Response::failure(
            "invalid_arguments",
            i18n::tr!("ui-control-invalid-arguments"),
        ),
        2,
    )
}

pub(crate) fn run_ui(
    platform: &dyn platform::Platform,
    output: &mut impl Write,
    command: UiCommand,
) -> i32 {
    if command == UiCommand::Help {
        return i32::from(help(output).is_err());
    }
    let sessions = platform
        .app_paths()
        .map_err(|error| unavailable(error.to_string()))
        .and_then(|paths| {
            platform::shell_control::sessions(&platform::shell_control::directory(&paths))
                .map_err(io_error)
        });
    let sessions = match sessions {
        Ok(sessions) => sessions,
        Err(response) => return write_response(output, &response, 1),
    };
    let UiCommand::Control(command) = command else {
        let value = serde_json::json!({ "protocol": shell_control::PROTOCOL_VERSION, "ok": true, "sessions": sessions });
        return i32::from(
            serde_json::to_writer(&mut *output, &value)
                .and_then(|_| writeln!(output).map_err(serde_json::Error::io))
                .is_err(),
        );
    };
    let session = match choose_session(&sessions, command.session.as_deref()) {
        Ok(session) => session,
        Err(response) => return write_response(output, &response, 1),
    };
    let response = execute(session, command);
    let code = if response.ok { 0 } else { 1 };
    write_response(output, &response, code)
}

fn choose_session<'a>(
    sessions: &'a [SessionInfo],
    requested: Option<&str>,
) -> Result<&'a SessionInfo, Response> {
    if let Some(requested) = requested {
        return sessions
            .iter()
            .find(|session| session.session == requested)
            .ok_or_else(|| {
                Response::failure("session_not_found", i18n::tr!("ui-control-session-missing"))
            });
    }
    match sessions {
        [] => Err(Response::failure(
            "no_session",
            i18n::tr!("ui-control-no-session"),
        )),
        [session] => Ok(session),
        _ => Err(Response::failure(
            "session_required",
            i18n::tr!("ui-control-many-sessions"),
        )),
    }
}

fn execute(session: &SessionInfo, command: UiRequest) -> Response {
    let action = match command.action {
        UiAction::Input(action) => action,
        UiAction::Wait { id, state } => {
            return wait(
                session,
                &id,
                state,
                command.snapshot,
                Duration::from_millis(command.timeout_ms),
            );
        }
        UiAction::Stdin => {
            let text = match read_text(std::io::stdin()) {
                Ok(text) => text,
                Err(error) => return error,
            };
            Action::Text { text }
        }
    };
    let request = Request {
        protocol: shell_control::PROTOCOL_VERSION,
        snapshot: command.snapshot,
        action,
    };
    exchange(session, &request, Duration::from_millis(command.timeout_ms))
}

fn read_text(input: impl Read) -> Result<String, Response> {
    let mut text = String::new();
    input
        .take(shell_control::MAX_REQUEST_BYTES as u64 + 1)
        .read_to_string(&mut text)
        .map_err(|_| Response::failure("stdin_error", i18n::tr!("ui-control-input-failed")))?;
    if text.len() > shell_control::MAX_REQUEST_BYTES {
        return Err(Response::failure(
            "invalid_arguments",
            i18n::tr!("ui-control-invalid-arguments"),
        ));
    }
    Ok(text)
}

fn wait(
    session: &SessionInfo,
    id: &str,
    state: WaitState,
    snapshot: Option<u64>,
    timeout: Duration,
) -> Response {
    let start = Instant::now();
    let request = Request {
        protocol: shell_control::PROTOCOL_VERSION,
        snapshot,
        action: Action::Snapshot,
    };
    loop {
        let remaining = timeout.saturating_sub(start.elapsed());
        if remaining.is_zero() {
            return Response::failure("timeout", i18n::tr!("ui-control-timeout"));
        }
        let response = exchange(session, &request, remaining);
        if let Some(snapshot) = &response.snapshot {
            let node = snapshot.nodes.iter().find(|node| node.id == id);
            let ready = match state {
                WaitState::Present => node.is_some(),
                WaitState::Absent => node.is_none(),
                WaitState::Actionable => node.is_some_and(|node| node.enabled && node.actionable),
            };
            if ready {
                return response;
            }
        } else if response
            .error
            .as_ref()
            .is_none_or(|error| error.code != "not_ready")
        {
            return response;
        }
        std::thread::sleep(Duration::from_millis(20).min(timeout.saturating_sub(start.elapsed())));
    }
}

fn exchange(session: &SessionInfo, request: &Request, timeout: Duration) -> Response {
    match platform::shell_control::exchange(session, request, timeout) {
        Ok(response)
            if response.protocol == shell_control::PROTOCOL_VERSION
                && ((response.ok
                    && response.error.is_none()
                    && ((response.closed && response.snapshot.is_none())
                        || (!response.closed
                            && response.snapshot.as_ref().is_some_and(|snapshot| {
                                snapshot.session == session.session
                            }))))
                    || (!response.ok
                        && !response.closed
                        && response.snapshot.is_none()
                        && response.error.is_some())) =>
        {
            response
        }
        Ok(_) => Response::failure("protocol_error", i18n::tr!("ui-control-protocol")),
        Err(error) => io_error(error),
    }
}

fn unavailable(reason: String) -> Response {
    Response::failure(
        "unavailable",
        i18n::tr!("ui-control-unavailable", reason = reason),
    )
}

fn io_error(error: std::io::Error) -> Response {
    match error.kind() {
        std::io::ErrorKind::TimedOut => {
            Response::failure("timeout", i18n::tr!("ui-control-timeout"))
        }
        std::io::ErrorKind::InvalidData => {
            Response::failure("protocol_error", i18n::tr!("ui-control-protocol"))
        }
        std::io::ErrorKind::InvalidInput => Response::failure(
            "invalid_arguments",
            i18n::tr!("ui-control-invalid-arguments"),
        ),
        _ => unavailable(error.to_string()),
    }
}

fn write_response(output: &mut impl Write, response: &Response, code: i32) -> i32 {
    if serde_json::to_writer(&mut *output, response)
        .and_then(|_| writeln!(output).map_err(serde_json::Error::io))
        .is_err()
    {
        1
    } else {
        code
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stdin_preserves_utf8_newlines_and_rejects_invalid_or_oversized_input() {
        let text = "中文 😀\r\nnext line\n";
        assert_eq!(read_text(text.as_bytes()).unwrap(), text);
        assert_eq!(read_text(&[][..]).unwrap(), "");
        let response = read_text(&[0xff][..]).unwrap_err();
        assert_eq!(response.error.unwrap().code, "stdin_error");
        let response =
            read_text(&vec![b'a'; shell_control::MAX_REQUEST_BYTES + 1][..]).unwrap_err();
        assert_eq!(response.error.unwrap().code, "invalid_arguments");
    }
}
