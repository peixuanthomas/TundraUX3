//! Suggestions use known UX and system syntax, never execute commands or read secrets.
use rustyline::hint::Hint;
use std::{
    borrow::Cow,
    time::{Duration, Instant},
};

/// Merge and sort on the discovery worker, never once per typed character.
pub(super) fn system_candidates(mut names: Vec<String>) -> Vec<String> {
    names.extend(system_words(std::env::consts::OS, &[]));
    names.sort();
    names.dedup();
    names
}

const COMMANDS: &[&str] = &[
    "help",
    "config",
    "launcher",
    "services",
    "processes",
    "packages",
    "network",
    "disks",
    "users",
    "system-config",
    "operations",
    "firewall",
    "ssh",
    "backups",
    "scheduled-tasks",
    "logs",
    "debug",
    "cls",
    "new",
];
const CONFIG_FIELDS: &[&str] = &[
    "theme",
    "border-shape",
    "border-color",
    "accent-color",
    "language",
    "timezone",
    "address",
    "icon-mode",
    "motion",
    "animation-speed",
    "weather-location",
    "update-mode",
];

pub(super) struct CommandHint {
    display: String,
    suffix: Option<String>,
}

impl Hint for CommandHint {
    fn display(&self) -> &str {
        &self.display
    }

    fn completion(&self) -> Option<&str> {
        // Information on a separate row must never be inserted into the input.
        self.suffix.as_deref()
    }
}

fn completion_context<'a>(
    line: &'a str,
    pos: usize,
    system_commands: &'a [String],
) -> Option<(usize, &'a str, Cow<'a, [String]>)> {
    // Pasted scripts/long values are valid input, but need no expensive hints.
    if line.len() > 4096 {
        return None;
    }
    let prefix = line.get(..pos)?;
    let ux = prefix.starts_with('/');
    if ux {
        super::ux_command_input(prefix)?;
    }
    let start = prefix
        .rfind(char::is_whitespace)
        .map(|index| index + prefix[index..].chars().next().unwrap().len_utf8())
        .unwrap_or(0);
    // Do not interpret a quoted value or an unfinished quote as command syntax.
    let words = shlex::split(&prefix[..start])?;
    let partial = &prefix[start..];
    (!partial.contains(['\'', '"', '\\'])).then(|| {
        let candidates = if !ux && words.is_empty() {
            Cow::Borrowed(system_commands)
        } else {
            let mut candidates = if ux {
                next_words(&words)
            } else {
                system_words(std::env::consts::OS, &words)
            };
            candidates.sort();
            candidates.dedup();
            Cow::Owned(candidates)
        };
        (start, partial, candidates)
    })
}

fn prefix_choices<'a>(partial: &str, candidates: &'a [String]) -> &'a [String] {
    let start = candidates.partition_point(|candidate| candidate.as_str() < partial);
    let rest = &candidates[start..];
    &rest[..rest.partition_point(|candidate| candidate.starts_with(partial))]
}

pub(super) fn command_completions(
    line: &str,
    pos: usize,
    path_commands: &[String],
) -> (usize, Vec<String>) {
    let Some((start, partial, candidates)) = completion_context(line, pos, path_commands) else {
        return (pos, Vec::new());
    };
    let partial = matching_word(line, partial);
    let choices = prefix_choices(&partial, &candidates);
    if choices.is_empty() {
        (start, nearby_words(&partial, &candidates))
    } else {
        (start, choices.to_vec())
    }
}

pub(super) fn command_hint(
    line: &str,
    pos: usize,
    path_commands: &[String],
) -> Option<CommandHint> {
    if pos != line.len()
        || line.is_empty()
        || line.len() > 4096
        || line.chars().any(char::is_control)
    {
        return None;
    }
    let (start, partial, candidates) = completion_context(line, pos, path_commands)?;
    let partial = matching_word(line, partial);
    let choices = prefix_choices(&partial, &candidates);
    if choices
        .first()
        .is_some_and(|choice| choice == partial.as_ref())
    {
        // /debug (without a trailing space) should already show its subcommands.
        let words = shlex::split(line)?;
        let next = if line.starts_with('/') {
            next_words(&words)
        } else {
            system_words(std::env::consts::OS, &words)
        };
        return information("Suggestions", &next);
    }
    if choices.len() == 1 {
        let suffix = choices[0].get(partial.len()..)?.to_string();
        return Some(CommandHint {
            display: suffix.clone(),
            suffix: Some(suffix),
        });
    }
    if !choices.is_empty() {
        return information("Suggestions", choices);
    }
    let corrected = nearby_words(&partial, &candidates)
        .into_iter()
        .map(|word| format!("{}{word}", &line[..start]))
        .collect::<Vec<_>>();
    information("Did you mean", &corrected)
}

fn matching_word<'a>(line: &str, word: &'a str) -> std::borrow::Cow<'a, str> {
    if cfg!(windows) && !line.starts_with('/') {
        std::borrow::Cow::Owned(word.to_ascii_lowercase())
    } else {
        std::borrow::Cow::Borrowed(word)
    }
}

fn information(label: &str, choices: &[String]) -> Option<CommandHint> {
    if choices.is_empty() {
        return None;
    }
    // Bound the hint height even for groups with many operations.
    let mut display = format!(
        "\n{label}: {}",
        choices
            .iter()
            .take(5)
            .cloned()
            .collect::<Vec<_>>()
            .join(" | ")
    );
    if choices.len() > 5 {
        display.push_str(" | ... (Tab for more)");
    }
    Some(CommandHint {
        display,
        suffix: None,
    })
}

fn strings(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| value.to_string()).collect()
}

/// Windows uses cmd built-ins; Unix uses sh and common tools. These are
/// suggestions, not a claim that every optional program is installed.
pub(super) fn system_words(os: &str, words: &[String]) -> Vec<String> {
    if words.is_empty() {
        let mut commands = strings(&[
            "help", "cd", "echo", "exit", "mkdir", "rmdir", "whoami", "ping", "git", "cargo",
        ]);
        commands.extend(strings(match os {
            "windows" => &[
                "dir", "cls", "copy", "move", "del", "ren", "type", "set", "where", "find",
                "findstr", "tree", "ipconfig", "netstat", "tasklist", "taskkill", "hostname",
                "ver", "more",
            ],
            "linux" => &[
                "ls",
                "pwd",
                "clear",
                "cp",
                "mv",
                "rm",
                "cat",
                "less",
                "more",
                "head",
                "tail",
                "grep",
                "find",
                "touch",
                "chmod",
                "chown",
                "ps",
                "kill",
                "top",
                "df",
                "du",
                "free",
                "uname",
                "man",
                "which",
                "env",
                "export",
                "unset",
                "printf",
                "ssh",
                "scp",
                "curl",
                "tar",
                "systemctl",
                "journalctl",
                "ip",
                "lsblk",
            ],
            "macos" => &[
                "ls",
                "pwd",
                "clear",
                "cp",
                "mv",
                "rm",
                "cat",
                "less",
                "more",
                "head",
                "tail",
                "grep",
                "find",
                "touch",
                "chmod",
                "chown",
                "ps",
                "kill",
                "top",
                "df",
                "du",
                "uname",
                "man",
                "which",
                "env",
                "export",
                "unset",
                "printf",
                "ssh",
                "scp",
                "curl",
                "tar",
                "open",
                "sw_vers",
                "networksetup",
                "diskutil",
            ],
            _ => &[],
        }));
        commands.sort();
        commands.dedup();
        return commands;
    }
    let normalized = words
        .iter()
        .map(|word| {
            if os == "windows" {
                word.to_ascii_lowercase()
            } else {
                word.clone()
            }
        })
        .collect::<Vec<_>>();
    let words = normalized.iter().map(String::as_str).collect::<Vec<_>>();
    if words.len() == 1 {
        return strings(match words[0] {
            "git" => &[
                "status", "diff", "log", "show", "add", "commit", "branch", "switch", "restore",
                "fetch", "pull", "push", "clone", "help",
            ],
            "cargo" => &[
                "build", "check", "test", "run", "fmt", "clippy", "clean", "doc", "help",
            ],
            "systemctl" if os == "linux" => &[
                "status",
                "start",
                "stop",
                "restart",
                "reload",
                "enable",
                "disable",
                "list-units",
                "list-unit-files",
                "daemon-reload",
                "--help",
            ],
            "ip" if os == "linux" => &["address", "link", "route", "neighbour", "--help"],
            "diskutil" if os == "macos" => &["list", "info", "mount", "unmount", "help"],
            "dir" if os == "windows" => &["/a", "/b", "/s", "/w", "/?"],
            "copy" | "del" | "move" | "tasklist" | "ipconfig" if os == "windows" => &["/?"],
            "ls" if os != "windows" => &["-a", "-l", "-la", "-h", "-R"],
            "rm" if os != "windows" => &["-i", "-r", "-f"],
            "cp" if os != "windows" => &["-i", "-r", "-p"],
            "grep" if os != "windows" => &["-i", "-n", "-r", "-v"],
            "ps" if os != "windows" => &["-ef", "aux"],
            "journalctl" if os == "linux" => &[
                "--unit", "--follow", "--boot", "--since", "--until", "--lines", "--help",
            ],
            _ => &[],
        });
    }
    match words.as_slice() {
        ["git", "help"] => system_words(os, &["git".into()]),
        ["cargo", "help"] => system_words(os, &["cargo".into()]),
        ["cargo", "build" | "check" | "test" | "run" | "clippy"] => strings(&[
            "--release",
            "--locked",
            "--package",
            "--workspace",
            "--all-features",
            "--help",
        ]),
        _ => Vec::new(),
    }
}

fn next_words(words: &[String]) -> Vec<String> {
    let words = words.iter().map(String::as_str).collect::<Vec<_>>();
    let Some(group) = words.first().map(|word| word.trim_start_matches('/')) else {
        return COMMANDS.iter().map(|name| format!("/{name}")).collect();
    };
    if group == "help" {
        let mut topic = words[1..]
            .iter()
            .map(|word| word.to_string())
            .collect::<Vec<_>>();
        if topic.is_empty() {
            return strings(COMMANDS);
        }
        topic[0].insert(0, '/');
        return next_words(&topic);
    }
    if words.len() == 1 {
        return match group {
            "debug" => strings(&[
                "asset",
                "clear-logs",
                "doctor",
                "paths",
                "explain",
                "test-frost",
                "test-aa-style1",
                "test-aa-style2",
                "test-aa-style3",
                "screen-keyboard",
                "ui-test",
                "test-rainbow-text",
                "test-matrix",
                "view-ui-style",
                "test-watchdog-error",
                "test-watchdog-critical",
                "test-watchdog-panic",
                "help",
            ]),
            "logs" => strings(&["query", "follow", "incidents", "export", "help"]),
            "config" => strings(&["get", "set", "reset", "options", "help"]),
            "launcher" => strings(&["list", "pin", "unpin", "help"]),
            _ => {
                let mut verbs = strings(crate::management_command::verbs(group));
                if !verbs.is_empty() {
                    verbs.push("help".into());
                }
                verbs
            }
        };
    }
    match words.as_slice() {
        ["/config", verb] if matches!(*verb, "get" | "set" | "reset" | "options") => {
            return CONFIG_FIELDS
                .iter()
                .filter(|field| **field != "theme" || matches!(*verb, "get" | "options"))
                .map(|field| field.to_string())
                .collect();
        }
        ["/config", "set", field] => {
            return strings(match *field {
                "border-shape" | "border_shape" => &["rounded", "square"],
                "icon-mode" | "icon_mode" => &["ascii", "image"],
                "motion" => &["full", "reduced"],
                "animation-speed" | "animation_speed" => {
                    &["50", "75", "100", "125", "150", "175", "200"]
                }
                "update-mode" | "update_mode" => &["release", "beta"],
                _ => &[],
            });
        }
        ["/packages", "sources"] => return strings(&["add", "enable", "disable", "remove"]),
        ["/debug", "view-ui-style"] => return strings(&["1", "2", "3", "help"]),
        ["/debug", "asset"] => {
            return ascii_assets::required_assets()
                .into_iter()
                .map(|asset| asset.key.to_string())
                .collect();
        }
        ["/debug", "asset", _] => return strings(&["-a", "help"]),
        ["/debug", "clear-logs"] => {
            return strings(&[
                "all",
                "runtime",
                "incidents",
                "snapshots",
                "legacy",
                "--file",
                "--type",
                "help",
            ]);
        }
        _ => {}
    }
    match (group, words.last().copied().unwrap_or_default()) {
        ("logs", "--source") => return strings(&["ux", "linux"]),
        ("logs", "--level") => {
            return strings(&["trace", "debug", "info", "warning", "error", "critical"]);
        }
        ("logs", "--format") => return strings(&["text", "jsonl"]),
        ("logs" | "services", "--scope") => return strings(&["system", "user"]),
        ("network", "--decision") => return strings(&["keep", "revert"]),
        ("debug", "--type") if words.get(1) == Some(&"clear-logs") => {
            return strings(&["runtime", "incidents", "snapshots", "legacy"]);
        }
        _ => {}
    }
    if words.last().is_some_and(|word| {
        word.starts_with("--")
            && !matches!(
                *word,
                "--json"
                    | "--yes"
                    | "--non-interactive"
                    | "--apply"
                    | "--allow-unvalidated"
                    | "--reload-service"
            )
    }) {
        return Vec::new();
    }
    if group == "debug" && words.get(1) == Some(&"clear-logs") {
        return strings(&["--yes"]);
    }
    if group == "logs" {
        let mut flags = strings(&[
            "--source",
            "--since",
            "--until",
            "--level",
            "--module",
            "--unit",
            "--scope",
            "--boot",
            "--invocation",
            "--file",
            "--run-id",
            "--operation-id",
            "--task-id",
            "--incident-id",
            "--limit",
            "--format",
            "--json",
        ]);
        if words.get(1) == Some(&"export") {
            flags.extend(strings(&["--output", "--yes", "--non-interactive"]));
        }
        return flags;
    }
    if !crate::management_command::verbs(group).is_empty() {
        let verb = if group == "packages" && words.get(1) == Some(&"sources") {
            format!("source-{}", words.get(2).copied().unwrap_or_default())
        } else {
            words[1].to_string()
        };
        if !crate::management_command::verbs(group).contains(&verb.as_str()) {
            return Vec::new();
        }
        let mut flags = strings(&[
            "--json",
            "--yes",
            "--non-interactive",
            "--wait",
            "--password-fd",
            "--authorization-fd",
        ]);
        flags.extend(
            crate::management_command::allowed_values(group, &verb)
                .into_iter()
                .map(|value| format!("--{}", value.replace('_', "-"))),
        );
        if group == "system-config" {
            flags.extend(strings(&["--input", "--allow-unvalidated"]));
            if verb == "reload" {
                flags.push("--reload-service".into());
            }
        }
        if group == "backups" && verb == "restore" {
            flags.push("--allow-unvalidated".into());
        }
        if crate::management_command::is_draft(group, &verb) {
            flags.extend(strings(&["--apply", "--allow-unvalidated"]));
        }
        flags.sort();
        flags.dedup();
        return flags;
    }
    Vec::new()
}

fn nearby_words(partial: &str, candidates: &[String]) -> Vec<String> {
    let word = partial
        .trim_start_matches('/')
        .chars()
        .take(65)
        .collect::<Vec<_>>();
    if word.len() < 2 || word.len() > 64 {
        return Vec::new();
    }
    // Suggestions are optional: stop rather than delay the next key when PATH
    // contains an unusually large number of similarly spelled executables.
    let deadline = Instant::now() + Duration::from_millis(2);
    let mut best = 3;
    let mut matches = Vec::new();
    for (index, candidate) in candidates.iter().enumerate() {
        if index > 0 && index % 16 == 0 && Instant::now() >= deadline {
            break;
        }
        let candidate_word = candidate
            .trim_start_matches('/')
            .chars()
            .take(67)
            .collect::<Vec<_>>();
        if word.len().abs_diff(candidate_word.len()) > 2 || candidate_word.len() > 66 {
            continue;
        }
        let distance = edit_distance(&word, &candidate_word);
        if distance == 0 || distance > 2 || distance > best {
            continue;
        }
        if distance < best {
            matches.clear();
            best = distance;
        }
        if matches.len() < 128 {
            matches.push(candidate.clone());
        }
    }
    matches
}

fn edit_distance(left: &[char], right: &[char]) -> usize {
    // Only distances <= 2 matter. Keep three narrow rows for insertion,
    // deletion, replacement and adjacent transposition, without a heap matrix.
    const FAR: usize = 3;
    if left.len().abs_diff(right.len()) > 2 || left.len() > 64 || right.len() > 66 {
        return FAR;
    }
    let mut previous = [FAR; 67];
    let mut before_previous = [FAR; 67];
    for (j, value) in previous.iter_mut().enumerate().take(right.len() + 1) {
        *value = j.min(FAR);
    }
    let mut current = [FAR; 67];
    for i in 1..=left.len() {
        current.fill(FAR);
        current[0] = i.min(FAR);
        for j in i.saturating_sub(2).max(1)..=right.len().min(i + 2) {
            current[j] = (previous[j] + 1)
                .min(current[j - 1] + 1)
                .min(previous[j - 1] + usize::from(left[i - 1] != right[j - 1]));
            if i > 1 && j > 1 && left[i - 1] == right[j - 2] && left[i - 2] == right[j - 1] {
                current[j] = current[j].min(before_previous[j - 2] + 1);
            }
        }
        std::mem::swap(&mut before_previous, &mut previous);
        std::mem::swap(&mut previous, &mut current);
    }
    previous[right.len()].min(FAR)
}

#[cfg(test)]
#[path = "../../tests/unit/repl/completion.rs"]
mod tests;
