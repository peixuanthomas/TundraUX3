use crate::CliError;
use runtime_log::{LogClearAction, LogClearTarget, LogFileType};
use std::{io::Write, path::PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClearLogsAction {
    Help,
    Run {
        target: LogClearTarget,
        confirmed: bool,
    },
}

pub(crate) fn parse_clear_logs(args: &[String]) -> Result<ClearLogsAction, CliError> {
    if args.is_empty()
        || matches!(args, [help] if matches!(help.as_str(), "help" | "--help" | "-h"))
    {
        return Ok(ClearLogsAction::Help);
    }
    let mut target = None;
    let mut confirmed = false;
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        if arg == "--yes" {
            if confirmed {
                return Err(bad("duplicate --yes"));
            }
            confirmed = true;
            continue;
        }
        if target.is_some() {
            return Err(bad(
                "choose exactly one target: all, a type, or --file PATH",
            ));
        }
        target = Some(match arg.as_str() {
            "--file" => {
                let path = args
                    .next()
                    .filter(|p| !p.is_empty() && !p.starts_with("--"))
                    .ok_or_else(|| bad("--file requires a path"))?;
                LogClearTarget::File(PathBuf::from(path))
            }
            "--type" => parse_type(
                args.next()
                    .ok_or_else(|| bad("--type requires a log type"))?,
            )?,
            "all" | "--all" => LogClearTarget::All,
            value => parse_type(value)?,
        });
    }
    Ok(ClearLogsAction::Run {
        target: target.ok_or_else(|| bad("missing log target"))?,
        confirmed,
    })
}
fn bad(message: &str) -> CliError {
    CliError::InvalidLogsArgument(message.into())
}
fn parse_type(value: &str) -> Result<LogClearTarget, CliError> {
    Ok(LogClearTarget::Type(match value {
        "runtime" => LogFileType::Runtime,
        "incidents" => LogFileType::Incidents,
        "snapshots" => LogFileType::Snapshots,
        "legacy" => LogFileType::Legacy,
        _ => {
            return Err(bad(
                "expected all, runtime, incidents, snapshots, legacy, or --file PATH",
            ));
        }
    }))
}

pub(crate) fn run_clear_logs(
    platform: &dyn platform::Platform,
    stdout: &mut impl Write,
    stderr: &mut impl Write,
    action: ClearLogsAction,
) -> i32 {
    let ClearLogsAction::Run { target, confirmed } = action else {
        return if help(stdout).is_ok() { 0 } else { 1 };
    };
    let root = match platform.app_paths() {
        Ok(paths) => paths.logs_path().to_path_buf(),
        Err(error) => {
            let _ = writeln!(stderr, "ERROR: {error}");
            return 1;
        }
    };
    let label = match &target {
        LogClearTarget::All => "all".to_string(),
        LogClearTarget::File(path) => format!("file {}", path.display()),
        LogClearTarget::Type(kind) => match kind {
            LogFileType::Runtime => "runtime",
            LogFileType::Incidents => "incidents",
            LogFileType::Snapshots => "snapshots",
            LogFileType::Legacy => "legacy",
        }
        .to_string(),
    };
    if writeln!(
        stdout,
        "{} Tundra logs: {}\nTarget: {label}",
        if confirmed { "Clearing" } else { "Previewing" },
        root.display()
    )
    .and_then(|_| stdout.flush())
    .is_err()
    {
        return 1;
    }
    let report = match runtime_log::clear_logs(&root, &target, confirmed) {
        Ok(report) => report,
        Err(error) => {
            let _ = writeln!(stderr, "ERROR: {error}");
            return 1;
        }
    };
    let print = (|| -> std::io::Result<()> {
        for entry in &report.files {
            let action = match entry.action {
                LogClearAction::Preview => "Would clear",
                LogClearAction::Removed => "Removed",
                LogClearAction::Truncated => "Cleared active log",
            };
            writeln!(
                stdout,
                "{action}: {} ({} bytes)",
                entry.path.display(),
                entry.bytes
            )?;
        }
        writeln!(
            stdout,
            "{} file(s), {} bytes; {} failure(s).",
            report.files.len(),
            report.files.iter().map(|f| f.bytes).sum::<u64>(),
            report.failures.len()
        )?;
        if !confirmed {
            writeln!(
                stdout,
                "Preview only. Add --yes to permanently clear these logs."
            )?;
        } else {
            writeln!(
                stdout,
                "New events may create logs again; configuration and watchdog state are preserved."
            )?;
        }
        Ok(())
    })();
    for (path, error) in &report.failures {
        let _ = writeln!(stderr, "ERROR: {}: {error}", path.display());
    }
    if print.is_err() {
        1
    } else if report.failures.is_empty() {
        0
    } else if report.files.is_empty() {
        1
    } else {
        3
    }
}

fn help(output: &mut impl Write) -> std::io::Result<()> {
    writeln!(
        output,
        "Usage: tundra-cli debug clear-logs <all|TYPE|--type TYPE|--file PATH> [--yes]"
    )?;
    writeln!(
        output,
        "Types: runtime, incidents (crashes/*.json, *.txt, *.log), snapshots, legacy (root *.log)."
    )?;
    writeln!(
        output,
        "PATH is relative to the configured logs directory, or an absolute path inside it."
    )?;
    writeln!(
        output,
        "Without --yes, list files without changing them. --yes permanently clears the selected files."
    )?;
    writeln!(
        output,
        "Active runtime logs are emptied safely; subsequent events may appear. Incidents clears report files, not their runtime-event references."
    )?;
    writeln!(
        output,
        "Only configured Tundra logs are included; system journals, temporary fallback reports, exported bundles, configuration and watchdog state are excluded."
    )?;
    writeln!(
        output,
        "Exit codes: 0 complete, 1 failed, 2 invalid arguments, 3 partial failure."
    )
}
