use super::*;
use chrono::{DateTime, Local, Utc};
use croner::parser::CronParser;
use std::collections::{BTreeMap, BTreeSet};
use std::os::unix::fs::MetadataExt;

const PAUSED: &str = "# TUNDRAUX3-PAUSED ";

#[derive(Clone, Debug)]
struct Entry {
    line: usize,
    schedule: String,
    user: String,
    command: String,
    environment: BTreeMap<String, String>,
    paused: bool,
}

fn environment(line: &str) -> Option<(String, String)> {
    let (key, value) = line.split_once('=')?;
    let key = key.trim();
    if key.is_empty()
        || !key.bytes().enumerate().all(|(index, byte)| {
            byte.is_ascii_alphabetic() || byte == b'_' || index > 0 && byte.is_ascii_digit()
        })
    {
        return None;
    }
    let value = value.trim();
    let value = if value.len() >= 2
        && (value.starts_with('"') && value.ends_with('"')
            || value.starts_with('\'') && value.ends_with('\''))
    {
        &value[1..value.len() - 1]
    } else {
        value
    };
    Some((key.into(), value.into()))
}

fn word<'a>(line: &mut &'a str) -> Option<&'a str> {
    *line = line.trim_start_matches([' ', '\t']);
    if line.is_empty() {
        return None;
    }
    let end = line.find([' ', '\t']).unwrap_or(line.len());
    let value = &line[..end];
    *line = &line[end..];
    Some(value)
}

fn entries(text: &str, system: bool, owner: &str) -> Vec<Entry> {
    let mut env = BTreeMap::new();
    let mut records = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let (line, paused) = match line.strip_prefix(PAUSED) {
            Some(line) => (line, true),
            None => (line, false),
        };
        let mut tail = line.trim_start();
        if tail.is_empty() || tail.starts_with('#') {
            continue;
        }
        if let Some((key, value)) = environment(tail) {
            if !paused {
                env.insert(key, value);
            }
            continue;
        }
        let Some(first) = word(&mut tail) else {
            continue;
        };
        let schedule = if first.starts_with('@') {
            first.to_string()
        } else {
            let mut fields = vec![first];
            for _ in 0..4 {
                if let Some(field) = word(&mut tail) {
                    fields.push(field);
                }
            }
            if fields.len() != 5 {
                continue;
            }
            fields.join(" ")
        };
        let user = if system {
            let Some(user) = word(&mut tail) else {
                continue;
            };
            user
        } else {
            owner
        };
        let command = tail.trim_start_matches([' ', '\t']);
        if command.is_empty() {
            continue;
        }
        records.push(Entry {
            line: index,
            schedule,
            user: user.into(),
            command: command.into(),
            environment: env.clone(),
            paused,
        });
    }
    records
}

fn next_run(entry: &Entry, now: DateTime<Utc>) -> String {
    if entry.paused {
        return i18n::tr!("scheduled-task-paused").into();
    }
    let pattern = match entry.schedule.as_str() {
        "@yearly" | "@annually" => "0 0 1 1 *",
        "@monthly" => "0 0 1 * *",
        "@weekly" => "0 0 * * 0",
        "@daily" | "@midnight" => "0 0 * * *",
        "@hourly" => "0 * * * *",
        "@reboot" => return i18n::tr!("scheduled-task-cron-reboot").into(),
        pattern if pattern.starts_with('@') => {
            return i18n::tr!("scheduled-task-cron-alias").into();
        }
        pattern => pattern,
    };
    if pattern.split_whitespace().count() != 5 || !standard_fields(pattern) {
        return i18n::tr!("scheduled-task-cron-extension").into();
    }
    let cron = match CronParser::new().parse(pattern) {
        Ok(cron) => cron,
        Err(error) => {
            return i18n::tr!("scheduled-task-cannot-determine", error = error.to_string());
        }
    };
    if let Some(zone) = entry
        .environment
        .get("CRON_TZ")
        .filter(|zone| !zone.is_empty())
    {
        let zone: chrono_tz::Tz = match zone.parse() {
            Ok(zone) => zone,
            Err(_) => return i18n::tr!("scheduled-task-cron-time-zone").into(),
        };
        cron.find_next_occurrence(&now.with_timezone(&zone), false)
            .map(|time| time.to_rfc3339())
            .unwrap_or_else(|error| {
                i18n::tr!("scheduled-task-cannot-determine", error = error.to_string())
            })
    } else {
        cron.find_next_occurrence(&now.with_timezone(&Local), false)
            .map(|time| time.to_rfc3339())
            .unwrap_or_else(|error| {
                i18n::tr!("scheduled-task-cannot-determine", error = error.to_string())
            })
    }
}

fn standard_fields(pattern: &str) -> bool {
    let fields = pattern.split_whitespace().collect::<Vec<_>>();
    fields.iter().enumerate().all(|(index, field)| {
        let mut candidate = field.to_ascii_uppercase();
        if index == 3 {
            for name in [
                "JAN", "FEB", "MAR", "APR", "MAY", "JUN", "JUL", "AUG", "SEP", "OCT", "NOV", "DEC",
            ] {
                candidate = candidate.replace(name, "1");
            }
        }
        if index == 4 {
            for name in ["SUN", "MON", "TUE", "WED", "THU", "FRI", "SAT"] {
                candidate = candidate.replace(name, "1");
            }
        }
        candidate
            .bytes()
            .all(|byte| byte.is_ascii_digit() || b"*,-/".contains(&byte))
    })
}

fn crontab(user: &str, flag: &AtomicBool) -> Result<Option<String>, ManagementError> {
    let mut args = Vec::new();
    if unsafe { libc::geteuid() } == 0 {
        args.extend(["-u".into(), user.into()]);
    }
    args.push("-l".into());
    let output = native("/usr/bin/crontab", &args, None, flag)?;
    if output.status.success() {
        return String::from_utf8(output.stdout)
            .map(Some)
            .map_err(|_| invalid(i18n::tr!("scheduled-task-crontab-encoding")));
    }
    let error = String::from_utf8_lossy(&output.stderr);
    if error.to_lowercase().contains("no crontab for") {
        return Ok(None);
    }
    Err(ManagementError::Unavailable(i18n::tr!(
        "scheduled-task-crontab-read",
        user = user,
        error = error.trim()
    )))
}

fn file_version(path: &Path, text: &str) -> Result<String, ManagementError> {
    let metadata = fs::symlink_metadata(path).map_err(io_error)?;
    if !metadata.is_file() {
        return Err(invalid(i18n::tr!("scheduled-task-cron-not-file")));
    }
    Ok(digest(
        format!(
            "{}\0{}\0{}\0{}\0{}",
            metadata.dev(),
            metadata.ino(),
            metadata.mode(),
            metadata.uid(),
            metadata.gid()
        )
        .as_bytes(),
    ) + &digest(text.as_bytes()))
}

fn key(source: &str, location: &str, entry: &Entry) -> String {
    format!(
        "cron-{}",
        digest(
            format!(
                "{source}\0{location}\0{}\0{}\0{}",
                entry.line, entry.user, entry.command
            )
            .as_bytes()
        )
    )
}

fn rows(
    text: &str,
    location: &str,
    system: bool,
    owner: &str,
    version: &str,
) -> Vec<ManagementRow> {
    let source = if system { "cron-file" } else { "crontab" };
    entries(text, system, owner)
        .into_iter()
        .map(|entry| {
            let result_key = key(source, location, &entry);
            let identity = BTreeMap::from([
                ("source".into(), source.into()),
                ("location".into(), location.into()),
                ("line".into(), entry.line.to_string()),
                ("version".into(), version.into()),
                ("user".into(), entry.user.clone()),
                ("result_key".into(), result_key.clone()),
            ]);
            let next = next_run(&entry, Utc::now());
            let manual = local_identity(&entry.user)
                .map(|(uid, _)| result_detail(&result_key, uid))
                .unwrap_or_else(|| i18n::tr!("scheduled-task-no-result").into());
            ManagementRow {
                id: format!("{source}:{location}:{}", entry.line),
                cells: vec![
                    entry.command.clone(),
                    location.into(),
                    entry.user.clone(),
                    entry.schedule.clone(),
                    if entry.paused {
                        i18n::tr!("scheduled-task-paused")
                    } else {
                        i18n::tr!("scheduled-task-scheduled")
                    }
                    .into(),
                    i18n::tr!("scheduled-task-cron-not-recorded").into(),
                    next,
                ],
                detail: vec![
                    (
                        i18n::tr!("scheduled-task-column-source").into(),
                        format!("{location}:{}", entry.line + 1),
                    ),
                    (
                        i18n::tr!("scheduled-task-detail-command").into(),
                        entry.command.clone(),
                    ),
                    (
                        i18n::tr!("scheduled-task-column-schedule").into(),
                        entry.schedule.clone(),
                    ),
                    (
                        i18n::tr!("scheduled-task-column-user").into(),
                        entry.user.clone(),
                    ),
                    (
                        i18n::tr!("scheduled-task-detail-environment").into(),
                        entry
                            .environment
                            .iter()
                            .map(|(key, value)| format!("{key}={value}"))
                            .collect::<Vec<_>>()
                            .join("\n"),
                    ),
                    (
                        i18n::tr!("scheduled-task-scheduled-result").into(),
                        i18n::tr!("scheduled-task-cron-result-detail").into(),
                    ),
                    (i18n::tr!("scheduled-task-manual-result").into(), manual),
                ],
                actions: [
                    ("pause", i18n::tr!("scheduled-task-pause"), entry.paused),
                    ("resume", i18n::tr!("scheduled-task-resume"), !entry.paused),
                    ("run_now", i18n::tr!("scheduled-task-run-now"), false),
                ]
                .into_iter()
                .map(|(id, label, disabled)| ManagementAction {
                    id: id.into(),
                    label: label.into(),
                    privileged: true,
                    confirm: true,
                    primary: true,
                    disabled_reason: disabled
                        .then(|| i18n::tr!("scheduled-task-cron-operation-disabled").into()),
                    ..Default::default()
                })
                .collect(),
                identity,
            }
        })
        .collect()
}

pub(super) fn append_rows(
    snapshot: &mut ManagementSnapshot,
    uid: u32,
    flag: &AtomicBool,
) -> Result<(), ManagementError> {
    let current = actor(uid)?;
    match crontab(&current.username, flag) {
        Ok(Some(text)) => snapshot.rows.extend(rows(
            &text,
            &format!("user:{}", current.username),
            false,
            &current.username,
            &digest(text.as_bytes()),
        )),
        Ok(None) => {}
        Err(error) => snapshot.notices.push(error.to_string()),
    }
    let mut paths = vec![PathBuf::from("/etc/crontab")];
    match fs::read_dir("/etc/cron.d") {
        Ok(directory) => {
            for entry in directory {
                let entry = entry.map_err(io_error)?;
                if entry.file_name().to_str().is_some_and(|name| {
                    !name.is_empty()
                        && name
                            .bytes()
                            .all(|byte| byte.is_ascii_alphanumeric() || b"_-".contains(&byte))
                }) {
                    paths.push(entry.path());
                }
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => snapshot.notices.push(i18n::tr!(
            "scheduled-task-source-unavailable",
            path = "/etc/cron.d",
            error = error.to_string()
        )),
    }
    paths.sort();
    for path in paths {
        check_cancelled(flag)?;
        match text_file(&path) {
            Ok(text) => {
                let version = file_version(&path, &text)?;
                snapshot
                    .rows
                    .extend(rows(&text, &path.display().to_string(), true, "", &version));
            }
            Err(error) if !path.exists() => {
                let _ = error;
            }
            Err(error) => snapshot.notices.push(i18n::tr!(
                "scheduled-task-source-unavailable",
                path = path.display().to_string(),
                error = error.to_string()
            )),
        }
    }
    Ok(())
}

fn validate_file(location: &str) -> Result<PathBuf, ManagementError> {
    let path = PathBuf::from(location);
    let valid = path == Path::new("/etc/crontab")
        || path.parent() == Some(Path::new("/etc/cron.d"))
            && path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| {
                    !name.is_empty()
                        && name
                            .bytes()
                            .all(|byte| byte.is_ascii_alphanumeric() || b"_-".contains(&byte))
                });
    if !valid {
        return Err(invalid(i18n::tr!("scheduled-task-cron-path")));
    }
    Ok(path)
}

fn changed_text(text: &str, index: usize, pause: bool) -> Result<String, ManagementError> {
    let mut lines = text
        .split_inclusive('\n')
        .map(String::from)
        .collect::<Vec<_>>();
    let line = lines.get_mut(index).ok_or_else(|| {
        ManagementError::Conflict(i18n::tr!("scheduled-task-cron-line-missing").into())
    })?;
    if pause {
        if line.starts_with(PAUSED) {
            return Err(invalid(i18n::tr!("scheduled-task-cron-already-paused")));
        }
        line.insert_str(0, PAUSED);
    } else {
        *line = line
            .strip_prefix(PAUSED)
            .ok_or_else(|| invalid(i18n::tr!("scheduled-task-cron-resume-owned")))?
            .to_string();
    }
    Ok(lines.concat())
}

fn install_crontab(
    user: &str,
    text: Option<&str>,
    flag: &AtomicBool,
) -> Result<(), ManagementError> {
    let args = vec![
        "-u".into(),
        user.into(),
        if text.is_some() { "-" } else { "-r" }.into(),
    ];
    let output = native("/usr/bin/crontab", &args, text.map(str::as_bytes), flag)?;
    if !output.status.success() {
        return Err(ManagementError::Failed(i18n::tr!(
            "scheduled-task-tool-failed",
            program = "crontab",
            error = String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    Ok(())
}

fn replace_file(
    path: &Path,
    expected: &str,
    expected_version: &str,
    replacement: &str,
) -> Result<(), ManagementError> {
    let actual = super::super::system_config::read(path)?;
    if actual.content != expected || file_version(path, &actual.content)? != expected_version {
        return Err(ManagementError::Conflict(
            i18n::tr!("scheduled-task-cron-file-changed").into(),
        ));
    }
    super::super::system_config::install_reviewed(path, &actual.version, replacement).map(|_| ())
}

pub(super) fn execute(
    command: &ManagementCommand,
    context: &ExecutionContext,
    _interaction: &mut dyn OperationInteraction,
    flag: &AtomicBool,
) -> Result<String, ManagementError> {
    require_root()?;
    let source = command
        .identity
        .get("source")
        .map(String::as_str)
        .unwrap_or("");
    let system = source == "cron-file";
    let location = command
        .identity
        .get("location")
        .ok_or_else(|| invalid(i18n::tr!("scheduled-task-cron-source-missing")))?;
    let current = actor(context.actor_uid)?;
    let (text, path, version) = if system {
        let path = validate_file(location)?;
        let text = text_file(&path)?;
        let version = file_version(&path, &text)?;
        (text, Some(path), version)
    } else {
        if location != &format!("user:{}", current.username) {
            return Err(invalid(i18n::tr!("scheduled-task-crontab-other-user")));
        }
        let text = crontab(&current.username, flag)?.ok_or_else(|| {
            ManagementError::Conflict(i18n::tr!("scheduled-task-crontab-missing").into())
        })?;
        let version = digest(text.as_bytes());
        (text, None, version)
    };
    if command.identity.get("version") != Some(&version) {
        return Err(ManagementError::Conflict(
            i18n::tr!("scheduled-task-cron-source-changed").into(),
        ));
    }
    let index: usize = command
        .identity
        .get("line")
        .and_then(|value| value.parse().ok())
        .ok_or_else(|| invalid(i18n::tr!("scheduled-task-cron-line-identity")))?;
    if command.target.as_deref() != Some(&format!("{source}:{location}:{index}")) {
        return Err(invalid(i18n::tr!("scheduled-task-cron-identity")));
    }
    let entry = entries(&text, system, &current.username)
        .into_iter()
        .find(|entry| entry.line == index)
        .ok_or_else(|| {
            ManagementError::Conflict(i18n::tr!("scheduled-task-cron-task-missing").into())
        })?;
    if command.identity.get("user") != Some(&entry.user)
        || command.identity.get("result_key") != Some(&key(source, location, &entry))
    {
        return Err(ManagementError::Conflict(
            i18n::tr!("scheduled-task-cron-task-changed").into(),
        ));
    }
    if command.action == "run_now" {
        return run_now(&entry, &key(source, location, &entry), flag);
    }
    let after = changed_text(&text, index, command.action == "pause")?;
    let paths = path.iter().cloned().collect::<Vec<_>>();
    let backup = super::super::config_backups::begin("scheduled-tasks", &paths)?;
    if !system {
        backup.set_payload(serde_json::json!({"type":"crontab","user":current.username,"before":text,"after":after}))?;
    } else {
        backup.set_payload(serde_json::json!({"type":"cron-file","path":location}))?;
    }
    let result = if let Some(path) = path {
        replace_file(&path, &text, &version, &after)
    } else {
        if crontab(&current.username, flag)?.as_deref() != Some(&text) {
            Err(ManagementError::Conflict(
                i18n::tr!("scheduled-task-crontab-concurrent").into(),
            ))
        } else {
            install_crontab(&current.username, Some(&after), flag)
        }
    };
    backup.finish(if result.is_ok() {
        "completed"
    } else {
        "failed"
    })?;
    result?;
    if !system && crontab(&current.username, flag)?.as_deref() != Some(&after) {
        return Err(ManagementError::Failed(
            i18n::tr!("scheduled-task-crontab-save-unconfirmed").into(),
        ));
    }
    Ok(i18n::tr!(
        "scheduled-task-cron-completed",
        action = state_action_label(&command.action),
        backup = backup.id()
    ))
}

pub(super) fn shell_quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\\''"))
}

fn split_percent(command: &str) -> (String, Option<String>) {
    let mut shell = String::new();
    let mut input = String::new();
    let mut in_input = false;
    let mut chars = command.chars().peekable();
    while let Some(character) = chars.next() {
        let out = if in_input { &mut input } else { &mut shell };
        match character {
            '\\' if chars.peek() == Some(&'%') => {
                chars.next();
                out.push('%');
            }
            '\\' if chars.peek() == Some(&'\\') => {
                out.push('\\');
                out.push(chars.next().unwrap());
            }
            '%' if !in_input => in_input = true,
            '%' => out.push('\n'),
            _ => out.push(character),
        }
    }
    if in_input && !input.ends_with('\n') {
        input.push('\n');
    }
    (shell, in_input.then_some(input))
}

fn run_arguments(
    entry: &Entry,
    account: &users::UserAccount,
    result_key: &str,
    unit: &str,
    manager_environment: &str,
) -> Result<Vec<String>, ManagementError> {
    let mut env = BTreeMap::from([
        ("SHELL".into(), "/bin/sh".into()),
        ("HOME".into(), account.home.display().to_string()),
        ("PATH".into(), "/usr/bin:/bin".into()),
        ("LOGNAME".into(), account.username.clone()),
        ("USER".into(), account.username.clone()),
    ]);
    env.extend(entry.environment.clone());
    env.insert("LOGNAME".into(), account.username.clone());
    let home = env.get("HOME").unwrap();
    if !Path::new(home).is_absolute() || !Path::new(home).is_dir() {
        return Err(invalid(i18n::tr!("scheduled-task-cron-home")));
    }
    let shell = env.get("SHELL").unwrap();
    if !Path::new(shell).is_absolute() || !Path::new(shell).is_file() {
        return Err(invalid(i18n::tr!("scheduled-task-cron-shell")));
    }
    let (command, input) = split_percent(&entry.command);
    let script = if let Some(input) = input {
        format!(
            "/usr/bin/printf '%s' {} | {} -c {}",
            shell_quote(&input),
            shell_quote(shell),
            shell_quote(&command)
        )
    } else {
        format!("exec {} -c {}", shell_quote(shell), shell_quote(&command))
    };
    let mut args = vec![
        "--system".into(),
        "--no-ask-password".into(),
        "--collect".into(),
        "--service-type=exec".into(),
        "--expand-environment=no".into(),
        format!("--unit={unit}"),
        format!("--uid={}", account.uid),
        format!("--gid={}", account.gid),
        format!("--working-directory={home}"),
        // Transient commands go through D-Bus, which does not expand unit specifiers.
        format!(
            "--property=ExecStopPost={}",
            result_command(result_key, account.uid).replace("%%", "%")
        ),
        "--property=TimeoutStartSec=infinity".into(),
    ];
    // Remove the manager's variables so each manual run sees the cron environment.
    let unset = [
        "LANG",
        "LANGUAGE",
        "LC_ALL",
        "LC_CTYPE",
        "LC_MESSAGES",
        "DISPLAY",
        "WAYLAND_DISPLAY",
        "XAUTHORITY",
        "DBUS_SESSION_BUS_ADDRESS",
    ]
    .into_iter()
    .chain(
        manager_environment
            .lines()
            .filter_map(|line| line.split_once('=').map(|(key, _)| key)),
    )
    .filter(|key| !env.contains_key(*key))
    .collect::<BTreeSet<_>>()
    .into_iter()
    .collect::<Vec<_>>()
    .join(" ");
    if !unset.is_empty() {
        args.push(format!("--property=UnsetEnvironment={unset}"));
    }
    if let Some(pam) = ["cron", "crond"]
        .into_iter()
        .find(|name| Path::new("/etc/pam.d").join(name).is_file())
    {
        args.push(format!("--property=PAMName={pam}"));
    }
    for (key, value) in env {
        args.push(format!("--setenv={key}={value}"));
    }
    args.extend(["--".into(), "/bin/sh".into(), "-c".into(), script]);
    Ok(args)
}

fn run_now(entry: &Entry, result_key: &str, flag: &AtomicBool) -> Result<String, ManagementError> {
    let account = users::accounts(Some(&entry.user))?
        .into_iter()
        .find(|account| account.local && account.username == entry.user)
        .ok_or_else(|| invalid(i18n::tr!("scheduled-task-cron-user-missing")))?;
    prepare_results(account.uid, account.gid)?;
    let unit = format!(
        "tundra-manual-{}-{}",
        &result_key[5..21],
        chrono::Utc::now().timestamp_micros()
    );
    let manager_environment = run(
        "/usr/bin/systemctl",
        &[
            "--system".into(),
            "--no-pager".into(),
            "show-environment".into(),
        ],
        flag,
    )?;
    let args = run_arguments(entry, &account, result_key, &unit, &manager_environment)?;
    run("/usr/bin/systemd-run", &args, flag)?;
    Ok(i18n::tr!(
        "scheduled-task-cron-started",
        user = account.username,
        unit = unit
    ))
}

pub(super) fn restore(
    payload: &serde_json::Value,
    context: &ExecutionContext,
    _interaction: &mut dyn OperationInteraction,
    flag: &AtomicBool,
) -> Result<String, ManagementError> {
    restore_preflight(payload, context, flag)?;
    let user = payload
        .get("user")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| invalid(i18n::tr!("scheduled-task-crontab-backup-owner")))?;
    if actor(context.actor_uid)?.username != user {
        return Err(invalid(i18n::tr!("scheduled-task-crontab-backup-other")));
    }
    let after = payload.get("after").and_then(serde_json::Value::as_str);
    let before = payload.get("before").and_then(serde_json::Value::as_str);
    if crontab(user, flag)?.as_deref() != after {
        return Err(ManagementError::Conflict(
            i18n::tr!("scheduled-task-crontab-backup-conflict").into(),
        ));
    }
    install_crontab(user, before, flag)?;
    if crontab(user, flag)?.as_deref() != before {
        return Err(ManagementError::Failed(
            i18n::tr!("scheduled-task-crontab-recovery-unconfirmed").into(),
        ));
    }
    Ok(i18n::tr!("scheduled-task-crontab-restored", user = user))
}

pub(super) fn restore_preflight(
    payload: &serde_json::Value,
    context: &ExecutionContext,
    flag: &AtomicBool,
) -> Result<(), ManagementError> {
    let user = payload
        .get("user")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| invalid(i18n::tr!("scheduled-task-crontab-backup-owner")))?;
    if actor(context.actor_uid)?.username != user {
        return Err(invalid(i18n::tr!("scheduled-task-crontab-backup-other")));
    }
    if crontab(user, flag)?.as_deref() != payload.get("after").and_then(serde_json::Value::as_str) {
        return Err(ManagementError::Conflict(
            i18n::tr!("scheduled-task-crontab-backup-conflict").into(),
        ));
    }
    Ok(())
}

pub(super) fn recovery_after(
    payload: &serde_json::Value,
    context: &ExecutionContext,
    flag: &AtomicBool,
) -> Result<Option<serde_json::Value>, ManagementError> {
    if payload.get("type").and_then(serde_json::Value::as_str) != Some("crontab") {
        return Ok(None);
    }
    let account = actor(context.actor_uid)?;
    Ok(Some(
        crontab(&account.username, flag)?
            .map(serde_json::Value::String)
            .unwrap_or(serde_json::Value::Null),
    ))
}

#[cfg(test)]
#[path = "../../../tests/unit/management/scheduled_tasks_cron_tests.rs"]
mod tests;
