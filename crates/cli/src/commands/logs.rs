use crate::CliError;
use app::runtime_logs::{LogAccess, export_diagnostics, query_snapshot};
use runtime_log::{LogLevel, LogQuery, LogSource, LogSourceState};
use std::{io::Write, path::PathBuf, sync::atomic::AtomicBool};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogsVerb {
    Query,
    Follow,
    Incidents,
    Export,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogsFormat {
    Text,
    Jsonl,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LogsAction {
    Help,
    Run {
        verb: LogsVerb,
        query: LogQuery,
        format: LogsFormat,
        output: Option<PathBuf>,
    },
}

pub(crate) fn parse_logs(args: &[String]) -> Result<LogsAction, CliError> {
    let Some((verb, args)) = args.split_first() else {
        return Ok(LogsAction::Help);
    };
    if matches!(verb.as_str(), "help" | "-h" | "--help") && args.is_empty() {
        return Ok(LogsAction::Help);
    }
    let verb = match verb.as_str() {
        "query" => LogsVerb::Query,
        "follow" => LogsVerb::Follow,
        "incidents" => LogsVerb::Incidents,
        "export" => LogsVerb::Export,
        _ => {
            return Err(CliError::InvalidLogsArgument(
                "expected query, follow, incidents, or export".into(),
            ));
        }
    };
    let mut query = LogQuery::default();
    let mut format = LogsFormat::Text;
    let mut output = None;
    let mut seen = std::collections::HashSet::new();
    let mut explicit_source = None;
    let mut options = args.iter();
    while let Some(flag) = options.next() {
        if !seen.insert(flag.as_str()) {
            return Err(CliError::InvalidLogsArgument(format!(
                "duplicate option {flag}"
            )));
        }
        if flag == "--json" {
            format = LogsFormat::Jsonl;
            continue;
        }
        if matches!(flag.as_str(), "--yes" | "--non-interactive") {
            continue;
        }
        let value = options
            .next()
            .ok_or_else(|| CliError::InvalidLogsArgument(format!("missing value for {flag}")))?;
        let bad = || CliError::InvalidLogsArgument(format!("invalid {flag} value"));
        match flag.as_str() {
            "--source" => {
                query.source = match value.as_str() {
                    "ux" => LogSource::Ux,
                    "linux" => LogSource::Linux,
                    _ => return Err(bad()),
                };
                explicit_source = Some(query.source);
            }
            "--since" => {
                query.since = Some(
                    chrono::DateTime::parse_from_rfc3339(value)
                        .map_err(|_| bad())?
                        .with_timezone(&chrono::Utc),
                )
            }
            "--until" => {
                query.until = Some(
                    chrono::DateTime::parse_from_rfc3339(value)
                        .map_err(|_| bad())?
                        .with_timezone(&chrono::Utc),
                )
            }
            "--level" => {
                query.min_level = Some(match value.as_str() {
                    "trace" => LogLevel::Trace,
                    "debug" => LogLevel::Debug,
                    "info" => LogLevel::Info,
                    "warning" => LogLevel::Warning,
                    "error" => LogLevel::Error,
                    "critical" => LogLevel::Critical,
                    _ => return Err(bad()),
                })
            }
            "--module" => query.module = Some(identifier(value)?),
            "--unit" => {
                let unit = identifier(value)?;
                if !unit.ends_with(".service")
                    || unit.starts_with('-')
                    || unit.bytes().any(|byte| {
                        !(byte.is_ascii_alphanumeric()
                            || matches!(byte, b':' | b'_' | b'.' | b'@' | b'-' | b'\\'))
                    })
                {
                    return Err(bad());
                }
                query.systemd_unit = Some(unit);
                query.source = LogSource::Linux;
            }
            "--scope" => {
                if !matches!(value.as_str(), "system" | "user") {
                    return Err(bad());
                }
                query.systemd_scope = Some(value.into());
                query.source = LogSource::Linux;
            }
            "--boot" => {
                if !(value.len() == 32 && value.bytes().all(|byte| byte.is_ascii_hexdigit()))
                    && !value.parse::<i32>().is_ok_and(|number| {
                        (-10_000..=0).contains(&number) && number.to_string() == *value
                    })
                {
                    return Err(bad());
                }
                query.systemd_boot = Some(value.into());
                query.source = LogSource::Linux;
            }
            "--invocation" => {
                if value.len() != 32 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                    return Err(bad());
                }
                query.systemd_invocation = Some(value.into());
                query.source = LogSource::Linux;
            }
            "--file" => {
                let path = PathBuf::from(value);
                if !path.is_absolute() {
                    return Err(bad());
                }
                query.file_path = Some(path);
            }
            "--run-id" => query.run_id = Some(identifier(value)?),
            "--operation-id" => query.operation_id = Some(identifier(value)?),
            "--task-id" => query.task_id = Some(identifier(value)?),
            "--incident-id" => query.incident_id = Some(identifier(value)?),
            "--limit" => {
                query.limit = value.parse().map_err(|_| bad())?;
                if !(1..=10_000).contains(&query.limit) {
                    return Err(bad());
                }
            }
            "--format" => {
                format = match value.as_str() {
                    "text" => LogsFormat::Text,
                    "jsonl" => LogsFormat::Jsonl,
                    _ => return Err(bad()),
                }
            }
            "--output" if verb == LogsVerb::Export => output = Some(PathBuf::from(value)),
            _ => {
                return Err(CliError::InvalidLogsArgument(format!(
                    "unknown option {flag}"
                )));
            }
        }
    }
    if explicit_source == Some(LogSource::Ux)
        && (query.systemd_unit.is_some()
            || query.systemd_scope.is_some()
            || query.systemd_boot.is_some()
            || query.systemd_invocation.is_some())
    {
        return Err(CliError::InvalidLogsArgument(
            "Journal filters require --source linux".into(),
        ));
    }
    if query.file_path.is_some()
        && (query.systemd_unit.is_some()
            || query.systemd_boot.is_some()
            || query.systemd_scope.is_some()
            || query.systemd_invocation.is_some()
            || query.run_id.is_some()
            || query.operation_id.is_some()
            || query.task_id.is_some()
            || query.incident_id.is_some())
    {
        return Err(CliError::InvalidLogsArgument(
            "--file cannot be combined with journal or UX correlation filters".into(),
        ));
    }
    if verb == LogsVerb::Incidents && query.source != LogSource::Ux {
        return Err(CliError::InvalidLogsArgument(
            "Incidents belong to the UX source".into(),
        ));
    }
    if query
        .since
        .zip(query.until)
        .is_some_and(|(since, until)| since > until)
    {
        return Err(CliError::InvalidLogsArgument(
            "--since is after --until".into(),
        ));
    }
    if verb == LogsVerb::Export && output.as_ref().is_none_or(|p| p.as_os_str().is_empty()) {
        return Err(CliError::MissingArgument("--output NEW_DIRECTORY"));
    }
    Ok(LogsAction::Run {
        verb,
        query,
        format,
        output,
    })
}
fn identifier(value: &str) -> Result<String, CliError> {
    if value.is_empty() || value.len() > 2048 || value.chars().any(char::is_control) {
        Err(CliError::InvalidLogsArgument(
            "invalid empty, oversized, or control-containing filter".into(),
        ))
    } else {
        Ok(value.into())
    }
}

pub(crate) fn run_logs(
    platform: &dyn platform::Platform,
    stdout: &mut impl Write,
    stderr: &mut impl Write,
    action: LogsAction,
) -> i32 {
    let LogsAction::Run {
        verb,
        query,
        format,
        output,
    } = action
    else {
        return if write_logs_help(stdout).is_ok() {
            0
        } else {
            1
        };
    };
    let root = match platform.app_paths() {
        Ok(paths) => paths.logs_path().to_path_buf(),
        Err(_) => {
            let _ = writeln!(stderr, "Logs directory is unavailable");
            return 1;
        }
    };
    let cancelled = AtomicBool::new(false);
    if verb == LogsVerb::Follow {
        return follow_logs(&root, &query, format, platform, stdout, stderr);
    }
    if verb == LogsVerb::Export {
        match export_diagnostics(
            &root,
            &query,
            &LogAccess::OsUser,
            platform,
            &cancelled,
            output.as_deref().expect("validated export directory"),
        ) {
            Ok(result) => {
                let _ = writeln!(
                    stdout,
                    "Diagnostic package created: {}",
                    output.as_ref().unwrap().display()
                );
                return report_status(&result, stderr);
            }
            Err(error) => {
                let _ = writeln!(
                    stderr,
                    "Diagnostic export failed: {}",
                    runtime_log::sanitize_text(&error)
                );
                return 1;
            }
        }
    }
    let snapshot = query_snapshot(&root, &query, &LogAccess::OsUser, platform, &cancelled);
    let write = (|| -> std::io::Result<()> {
        if verb == LogsVerb::Incidents {
            for report in snapshot.incidents {
                if format == LogsFormat::Jsonl {
                    serde_json::to_writer(&mut *stdout, &report)?;
                    writeln!(stdout)?
                } else {
                    writeln!(
                        stdout,
                        "{} {:?} {} {} {}",
                        report.occurred_at.to_rfc3339(),
                        report.severity,
                        report.incident_id,
                        report.boundary,
                        report.summary
                    )?
                }
            }
        } else {
            for event in &snapshot.result.events {
                if format == LogsFormat::Jsonl {
                    serde_json::to_writer(&mut *stdout, event)?;
                    writeln!(stdout)?
                } else {
                    writeln!(
                        stdout,
                        "{} {:?} {} {} run={} operation={} task={} code={} {}",
                        event.timestamp.to_rfc3339(),
                        event.level,
                        event.context.module,
                        event.context.operation,
                        event.context.run_id.as_deref().unwrap_or("-"),
                        event.context.operation_id.as_deref().unwrap_or("-"),
                        event.context.task_id.as_deref().unwrap_or("-"),
                        event.error_code.as_deref().unwrap_or("-"),
                        event.message
                    )?
                }
            }
        }
        Ok(())
    })();
    if write.is_err() {
        let _ = writeln!(stderr, "Could not write log query output");
        return 1;
    }
    report_status(&snapshot.result, stderr)
}

fn follow_logs(
    root: &std::path::Path,
    query: &LogQuery,
    format: LogsFormat,
    platform: &dyn platform::Platform,
    stdout: &mut impl Write,
    stderr: &mut impl Write,
) -> i32 {
    let control = platform::TerminalControlHandler::install();
    let cancelled = control.shutdown_flag();
    let mut seen = std::collections::HashSet::new();
    let mut recent = std::collections::VecDeque::new();
    let mut last_status = None;
    let mut previous_file: Option<runtime_log::LogFileStatus> = None;
    let mut available = false;
    while !control.shutdown_requested() {
        let snapshot = query_snapshot(root, query, &LogAccess::OsUser, platform, &cancelled);
        if let Some(file) = &snapshot.result.file_status {
            if previous_file
                .as_ref()
                .is_some_and(|old| old.identity != file.identity || old.length > file.length)
            {
                let _ = writeln!(
                    stderr,
                    "The log file rotated or was truncated. Following the current file."
                );
                seen.clear();
                recent.clear();
            }
            previous_file = Some(file.clone());
        }
        let status = (
            snapshot.result.state,
            snapshot.result.notices.clone(),
            snapshot.result.truncated,
        );
        if last_status.as_ref() != Some(&status) {
            report_status(&snapshot.result, stderr);
            last_status = Some(status);
        }
        if matches!(
            snapshot.result.state,
            LogSourceState::Ready | LogSourceState::Partial
        ) {
            available = true;
        }
        if !available
            && !matches!(
                snapshot.result.state,
                LogSourceState::Ready | LogSourceState::Partial | LogSourceState::Cancelled
            )
        {
            return logs_exit_code(&snapshot.result);
        }
        for event in snapshot.result.events.iter().rev() {
            if !seen.insert(event.event_id.clone()) {
                continue;
            }
            recent.push_back(event.event_id.clone());
            if recent.len() > 20_000
                && let Some(old) = recent.pop_front()
            {
                seen.remove(&old);
            }
            let written = if format == LogsFormat::Jsonl {
                serde_json::to_writer(&mut *stdout, event)
                    .and_then(|_| writeln!(stdout).map_err(serde_json::Error::io))
                    .map_err(std::io::Error::other)
            } else {
                writeln!(
                    stdout,
                    "{} {:?} {} {}",
                    event.timestamp.to_rfc3339(),
                    event.level,
                    event.context.module,
                    event.message
                )
            };
            if let Err(error) = written {
                return if error.kind() == std::io::ErrorKind::BrokenPipe {
                    0
                } else {
                    1
                };
            }
        }
        if stdout.flush().is_err() {
            return 1;
        }
        for _ in 0..10 {
            if control.shutdown_requested() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    }
    130
}
fn report_status(result: &runtime_log::LogQueryResult, stderr: &mut impl Write) -> i32 {
    for notice in &result.notices {
        let _ = writeln!(stderr, "{}", runtime_log::sanitize_text(notice));
    }
    if result.truncated {
        let _ = writeln!(
            stderr,
            "Log results are truncated; narrow the filters or increase --limit"
        );
    }
    if result.state != LogSourceState::Ready {
        let _ = writeln!(stderr, "Log source status: {:?}", result.state);
    }
    logs_exit_code(result)
}
fn logs_exit_code(result: &runtime_log::LogQueryResult) -> i32 {
    match result.state {
        LogSourceState::Cancelled => 130,
        LogSourceState::PermissionDenied => 3,
        LogSourceState::Unsupported => 4,
        LogSourceState::Partial | LogSourceState::Unavailable => 1,
        LogSourceState::Ready if result.truncated || result.damaged_records > 0 => 1,
        LogSourceState::Ready => 0,
    }
}
pub(crate) fn write_logs_help(output: &mut impl Write) -> std::io::Result<()> {
    writeln!(
        output,
        "Usage: tundra-cli logs <query|follow|incidents|export> [options]"
    )?;
    writeln!(
        output,
        "  --source ux|linux          UX is default; Linux source requires Linux"
    )?;
    writeln!(
        output,
        "  --since RFC3339 --until RFC3339 --level trace|debug|info|warning|error|critical"
    )?;
    writeln!(
        output,
        "  --module NAME --run-id ID --operation-id ID --task-id ID --incident-id ID"
    )?;
    writeln!(
        output,
        "  --unit SERVICE --scope system|user --boot 0|-1|BOOT_ID --invocation ID"
    )?;
    writeln!(
        output,
        "  --file ABSOLUTE_PATH        Read/follow a selected text file, including rotation"
    )?;
    writeln!(
        output,
        "  follow --json              Stream JSONL; Ctrl+C stops following"
    )?;
    writeln!(
        output,
        "  --limit N                  Latest records, default 200, maximum 10000"
    )?;
    writeln!(
        output,
        "  --format text|jsonl        Query and incident output, default text"
    )?;
    writeln!(
        output,
        "  export --output NEW_DIRECTORY  Private diagnostic package; never overwrites"
    )?;
    writeln!(
        output,
        "Access follows the current OS identity and filesystem permissions."
    )?;
    writeln!(
        output,
        "Exit codes: 0 complete, 1 failed/partial/truncated, 2 invalid arguments, 3 permission denied, 4 unsupported, 130 cancelled."
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_and_follow_share_operation_exit_codes() {
        for (state, expected) in [
            (LogSourceState::Ready, 0),
            (LogSourceState::Partial, 1),
            (LogSourceState::Unavailable, 1),
            (LogSourceState::PermissionDenied, 3),
            (LogSourceState::Unsupported, 4),
            (LogSourceState::Cancelled, 130),
        ] {
            let result = runtime_log::LogQueryResult {
                state,
                ..Default::default()
            };
            assert_eq!(logs_exit_code(&result), expected);
            assert_eq!(report_status(&result, &mut Vec::new()), expected);
        }
        assert_eq!(
            logs_exit_code(&runtime_log::LogQueryResult {
                truncated: true,
                ..Default::default()
            }),
            1
        );
        assert_eq!(
            logs_exit_code(&runtime_log::LogQueryResult {
                damaged_records: 1,
                ..Default::default()
            }),
            1
        );
    }
}
