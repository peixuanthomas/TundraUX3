use crate::CliError;
use app::runtime_logs::{LogAccess, export_diagnostics, query_snapshot};
use runtime_log::{LogLevel, LogQuery, LogSource, LogSourceState};
use std::{io::Write, path::PathBuf, sync::atomic::AtomicBool};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogsVerb {
    Query,
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
        "incidents" => LogsVerb::Incidents,
        "export" => LogsVerb::Export,
        _ => {
            return Err(CliError::InvalidLogsArgument(
                "expected query, incidents, or export".into(),
            ));
        }
    };
    let mut query = LogQuery::default();
    let mut format = LogsFormat::Text;
    let mut output = None;
    let mut seen = std::collections::HashSet::new();
    let mut options = args.iter();
    while let Some(flag) = options.next() {
        if !seen.insert(flag.as_str()) {
            return Err(CliError::InvalidLogsArgument(format!(
                "duplicate option {flag}"
            )));
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
                }
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
    match result.state {
        LogSourceState::Cancelled => 130,
        LogSourceState::Partial => 3,
        LogSourceState::Ready if result.truncated || result.damaged_records > 0 => 3,
        LogSourceState::Ready => 0,
        _ => 1,
    }
}
fn write_logs_help(output: &mut impl Write) -> std::io::Result<()> {
    writeln!(
        output,
        "Usage: tundra-cli logs <query|incidents|export> [options]"
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
        "Exit codes: 0 complete, 1 unavailable/access/output failure, 2 invalid arguments, 3 partial/truncated, 130 cancelled."
    )
}
