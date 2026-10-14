//! Suggestions use known UX and system syntax, never execute commands or read secrets.
use rustyline::hint::Hint;

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

fn completion_context(line: &str, pos: usize) -> Option<(usize, &str, Vec<String>)> {
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
        let candidates = if ux {
            next_words(&words)
        } else {
            system_words(std::env::consts::OS, &words)
        };
        (start, partial, candidates)
    })
}

pub(super) fn command_completions(line: &str, pos: usize) -> (usize, Vec<String>) {
    let Some((start, partial, candidates)) = completion_context(line, pos) else {
        return (pos, Vec::new());
    };
    let choices = candidates
        .iter()
        .filter(|candidate| candidate.starts_with(partial))
        .cloned()
        .collect::<Vec<_>>();
    if choices.is_empty() {
        (start, nearby_words(partial, &candidates))
    } else {
        (start, choices)
    }
}

pub(super) fn command_hint(line: &str, pos: usize) -> Option<CommandHint> {
    if pos != line.len() || line.is_empty() || line.chars().any(char::is_control) {
        return None;
    }
    let (start, partial, candidates) = completion_context(line, pos)?;
    let choices = candidates
        .iter()
        .filter(|candidate| candidate.starts_with(partial))
        .cloned()
        .collect::<Vec<_>>();
    if choices.iter().any(|choice| choice == partial) {
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
        let suffix = choices[0].strip_prefix(partial)?.to_string();
        return Some(CommandHint {
            display: suffix.clone(),
            suffix: Some(suffix),
        });
    }
    if !choices.is_empty() {
        return information("Suggestions", &choices);
    }
    let corrected = nearby_words(partial, &candidates)
        .into_iter()
        .map(|word| format!("{}{word}", &line[..start]))
        .collect::<Vec<_>>();
    information("Did you mean", &corrected)
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
    let words = words.iter().map(String::as_str).collect::<Vec<_>>();
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
    let word = partial.trim_start_matches('/');
    if word.len() < 2 || word.len() > 64 || !word.is_ascii() {
        return Vec::new();
    }
    let limit = 2;
    let mut matches = candidates
        .iter()
        .filter_map(|candidate| {
            let distance = edit_distance(
                word.as_bytes(),
                candidate.trim_start_matches('/').as_bytes(),
            );
            (distance > 0 && distance <= limit).then_some((distance, candidate.clone()))
        })
        .collect::<Vec<_>>();
    matches.sort();
    let best = matches.first().map(|item| item.0);
    matches
        .into_iter()
        .filter(|item| Some(item.0) == best)
        .map(|item| item.1)
        .collect()
}

fn edit_distance(left: &[u8], right: &[u8]) -> usize {
    let mut rows = vec![vec![0; right.len() + 1]; left.len() + 1];
    for (index, row) in rows.iter_mut().enumerate() {
        row[0] = index;
    }
    for index in 0..=right.len() {
        rows[0][index] = index;
    }
    for i in 1..=left.len() {
        for j in 1..=right.len() {
            rows[i][j] = (rows[i - 1][j] + 1)
                .min(rows[i][j - 1] + 1)
                .min(rows[i - 1][j - 1] + usize::from(left[i - 1] != right[j - 1]));
            if i > 1 && j > 1 && left[i - 1] == right[j - 2] && left[i - 2] == right[j - 1] {
                rows[i][j] = rows[i][j].min(rows[i - 2][j - 2] + 1);
            }
        }
    }
    rows[left.len()][right.len()]
}
