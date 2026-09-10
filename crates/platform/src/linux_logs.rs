//! Bounded, on-demand Linux journal and kernel log access.
//!
//! Call this on a worker: the subprocess deadline is five seconds. No shell,
//! elevation, pager, persistent collector, or traditional log-file scan is used.
use runtime_log::{LogQuery, LogQueryResult, LogSourceState};
use std::sync::atomic::AtomicBool;

pub fn query_linux_logs(query: &LogQuery, cancelled: &AtomicBool) -> LogQueryResult {
    #[cfg(target_os = "linux")]
    {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        implementation::query_with(query, cancelled, |program, args| {
            implementation::capture(program, args, cancelled, deadline, 8 * 1024 * 1024)
        })
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = query;
        LogQueryResult {
            state: if cancelled.load(std::sync::atomic::Ordering::Relaxed) {
                LogSourceState::Cancelled
            } else {
                LogSourceState::Unsupported
            },
            notices: vec!["Linux logs are unavailable on this platform".into()],
            ..Default::default()
        }
    }
}

#[cfg(any(target_os = "linux", test))]
mod implementation {
    use super::*;
    use chrono::{DateTime, Utc};
    use runtime_log::{LogContext, LogLevel, LogPhase, LogSource, RuntimeLogEvent};
    use serde_json::Value;
    use std::sync::atomic::Ordering;

    const MAX_RECORDS: usize = 10_000;
    const MAX_TEXT: usize = 2048;

    #[derive(Default)]
    pub(super) struct Capture {
        stdout: Vec<u8>,
        stderr: Vec<u8>,
        success: bool,
        stop: Option<LogSourceState>,
        limited: bool,
    }

    #[cfg(unix)]
    pub(super) fn capture(
        program: &str,
        args: &[String],
        cancelled: &AtomicBool,
        deadline: std::time::Instant,
        max_bytes: usize,
    ) -> Capture {
        use std::io::{ErrorKind, Read};
        use std::os::fd::AsRawFd;
        use std::process::{Command, Stdio};
        use std::time::{Duration, Instant};
        let mut out = Capture::default();
        if cancelled.load(Ordering::Relaxed) || Instant::now() >= deadline {
            out.stop = Some(if cancelled.load(Ordering::Relaxed) {
                LogSourceState::Cancelled
            } else {
                LogSourceState::Partial
            });
            return out;
        }
        let mut child = match Command::new(program)
            .args(args)
            .env("LC_ALL", "C")
            .env("SYSTEMD_COLORS", "0")
            .env("SYSTEMD_PAGER", "cat")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
        {
            Ok(child) => child,
            Err(error) => {
                out.stop = Some(if error.kind() == ErrorKind::PermissionDenied {
                    LogSourceState::PermissionDenied
                } else {
                    LogSourceState::Unavailable
                });
                return out;
            }
        };
        let mut stdout = child.stdout.take().expect("piped stdout");
        let mut stderr = child.stderr.take().expect("piped stderr");
        // Nonblocking reads avoid reader threads stranded by an inherited pipe.
        // Each pipe is drained only once per pass so noisy output cannot starve
        // cancellation, the deadline, or the other pipe.
        for fd in [stdout.as_raw_fd(), stderr.as_raw_fd()] {
            let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
            if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0
            {
                let _ = child.kill();
                let _ = child.wait();
                out.stop = Some(LogSourceState::Unavailable);
                return out;
            }
        }
        let mut eof = [false; 2];
        let mut status = None;
        loop {
            if cancelled.load(Ordering::Relaxed) {
                out.stop = Some(LogSourceState::Cancelled);
                break;
            }
            if Instant::now() >= deadline {
                out.stop = Some(LogSourceState::Partial);
                break;
            }
            let mut buffer = [0; 32 * 1024];
            for index in 0..2 {
                if eof[index] {
                    continue;
                }
                let remaining = max_bytes.saturating_sub(out.stdout.len() + out.stderr.len());
                if remaining == 0 {
                    out.limited = true;
                    out.stop = Some(LogSourceState::Partial);
                    break;
                }
                let length = remaining.min(buffer.len());
                let read = if index == 0 {
                    stdout.read(&mut buffer[..length])
                } else {
                    stderr.read(&mut buffer[..length])
                };
                match read {
                    Ok(0) => eof[index] = true,
                    Ok(count) => {
                        if index == 0 {
                            &mut out.stdout
                        } else {
                            &mut out.stderr
                        }
                        .extend_from_slice(&buffer[..count]);
                    }
                    Err(error)
                        if matches!(
                            error.kind(),
                            ErrorKind::WouldBlock | ErrorKind::Interrupted
                        ) => {}
                    Err(_) => {
                        out.stop = Some(LogSourceState::Partial);
                        break;
                    }
                }
            }
            if out.stop.is_some() {
                break;
            }
            if status.is_none() {
                match child.try_wait() {
                    Ok(value) => status = value,
                    Err(_) => {
                        out.stop = Some(LogSourceState::Unavailable);
                        break;
                    }
                }
            }
            if status.is_some() && eof.iter().all(|value| *value) {
                out.success = status.is_some_and(|status| status.success());
                return out;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        // kill/wait also applies when EOF was withheld by a child; there are no
        // detached threads or stderr writes to interfere with terminal drawing.
        let _ = child.kill();
        let _ = child.wait();
        out
    }

    pub(super) fn query_with(
        query: &LogQuery,
        cancelled: &AtomicBool,
        mut run: impl FnMut(&str, &[String]) -> Capture,
    ) -> LogQueryResult {
        if cancelled.load(Ordering::Relaxed) {
            return state(LogSourceState::Cancelled, "Linux log query cancelled");
        }
        if query.source != LogSource::Linux
            || query.run_id.is_some()
            || query.operation_id.is_some()
            || query.task_id.is_some()
            || query.incident_id.is_some()
            || query.owner_id.is_some()
        {
            return state(
                LogSourceState::Ready,
                "Linux events have no UX run, task, operation, incident, or owner identifiers",
            );
        }
        if query.limit == 0 {
            return LogQueryResult::default();
        }
        let mut args: Vec<String> = ["--no-pager", "--output=json", "--reverse", "--all", "--output-fields=__CURSOR,__REALTIME_TIMESTAMP,_TRANSPORT,_SYSTEMD_UNIT,_SYSTEMD_USER_UNIT,_KERNEL_DEVICE,_PID,PRIORITY,MESSAGE"].into_iter().map(String::from).collect();
        args.push(format!("--lines={}", MAX_RECORDS + 1));
        if let Some(since) = query.since {
            args.push(format!(
                "--since=@{}.{:06}",
                since.timestamp(),
                since.timestamp_subsec_micros()
            ));
        }
        if let Some(until) = query.until {
            args.push(format!(
                "--until=@{}.{:06}",
                until.timestamp(),
                until.timestamp_subsec_micros()
            ));
        }
        if let Some(level) = query.min_level {
            let priority = match level {
                LogLevel::Critical => 2,
                LogLevel::Error => 3,
                LogLevel::Warning => 4,
                LogLevel::Info => 6,
                LogLevel::Debug | LogLevel::Trace => 7,
            };
            args.push(format!("--priority=0..{priority}"));
        }
        if query.module.as_deref() == Some("linux.kernel") {
            args.push("_TRANSPORT=kernel".into());
        }
        let journal = run("journalctl", &args);
        let journal_state = capture_state(&journal);
        if matches!(
            journal_state,
            LogSourceState::Ready | LogSourceState::Partial | LogSourceState::Cancelled
        ) || !journal.stdout.is_empty()
        {
            let mut result = parse_journal(&journal.stdout, query);
            apply_capture(&mut result, &journal, "journal");
            return result;
        }
        // Permission denial is explicit: don't silently turn denied system
        // journal access into apparently complete kernel-only results.
        if journal_state == LogSourceState::PermissionDenied {
            return state(
                journal_state,
                "Permission denied reading the system journal",
            );
        }
        let mut dmesg = run(
            "dmesg",
            &["--json".into(), "--kernel".into(), "--color=never".into()],
        );
        let mut raw = false;
        let error = String::from_utf8_lossy(&dmesg.stderr).to_ascii_lowercase();
        if dmesg.stop.is_none()
            && !dmesg.success
            && (error.contains("unrecognized option")
                || error.contains("unknown option")
                || error.contains("invalid option"))
        {
            raw = true;
            dmesg = run(
                "dmesg",
                &["--raw".into(), "--kernel".into(), "--color=never".into()],
            );
        }
        let mut result = parse_dmesg(&dmesg.stdout, query, raw);
        apply_capture(&mut result, &dmesg, "dmesg");
        result.notices.insert(0, "System journal unavailable; dmesg fallback covers kernel messages only, not external services".into());
        if result.state == LogSourceState::Ready {
            result.state = LogSourceState::Partial;
        }
        result
    }

    fn state(state: LogSourceState, notice: &str) -> LogQueryResult {
        LogQueryResult {
            state,
            notices: vec![notice.into()],
            ..Default::default()
        }
    }

    fn capture_state(output: &Capture) -> LogSourceState {
        if let Some(state) = output.stop {
            return state;
        }
        let stderr = String::from_utf8_lossy(&output.stderr).to_ascii_lowercase();
        if [
            "permission denied",
            "operation not permitted",
            "insufficient permissions",
            "not seeing messages from other users",
            "not seeing messages from other users and the system",
        ]
        .iter()
        .any(|needle| stderr.contains(needle))
        {
            if !output.stdout.is_empty() {
                LogSourceState::Partial
            } else {
                LogSourceState::PermissionDenied
            }
        } else if !output.success || stderr.contains("no journal files were found") {
            LogSourceState::Unavailable
        } else if !output.stderr.is_empty() {
            LogSourceState::Partial
        } else {
            LogSourceState::Ready
        }
    }

    fn apply_capture(result: &mut LogQueryResult, output: &Capture, source: &str) {
        let captured = capture_state(output);
        if captured != LogSourceState::Ready {
            result.state = captured;
            result
                .notices
                .push(format!("{source} source status: {captured:?}"));
        }
        if output.limited {
            result.truncated = true;
            result
                .notices
                .push("Combined command output reached the 8 MiB limit".into());
        } else if output.stop == Some(LogSourceState::Partial) {
            result.truncated = true;
            result
                .notices
                .push("Command deadline reached or output read interrupted".into());
        }
        if !output.stderr.is_empty() {
            // Do not copy arbitrary program stderr: it can contain secrets or
            // terminal escapes. The structured state is the diagnostic result.
            result.notices.push(format!(
                "{source} reported a source warning; access may be incomplete"
            ));
        }
    }

    fn scalar(record: &Value, key: &str) -> Option<String> {
        match record.get(key)? {
            Value::String(value) => Some(value.clone()),
            Value::Number(value) => Some(value.to_string()),
            _ => None,
        }
    }

    fn priority(value: Option<u8>) -> LogLevel {
        match value {
            Some(0..=2) => LogLevel::Critical,
            Some(3) => LogLevel::Error,
            Some(4) => LogLevel::Warning,
            Some(7) => LogLevel::Debug,
            _ => LogLevel::Info,
        }
    }

    // Imported MESSAGE is untrusted free text. This deliberately omits the
    // whole record message when it declares sensitive payloads instead of
    // attempting to identify where a password, body or clipboard value ends.
    fn clean(value: &str) -> String {
        let lower = value.to_ascii_lowercase();
        if [
            "password",
            "passwd",
            "authorization",
            "bearer ",
            "token",
            "secret",
            "clipboard",
            "cookie",
            "api_key",
            "api-key",
            "private key",
            "file_content",
            "file content",
            "file body",
            "request body",
            "response body",
        ]
        .iter()
        .any(|word| lower.contains(word))
        {
            return "[sensitive message omitted]".into();
        }
        let filtered: String = value
            .chars()
            .filter(|ch| {
                !ch.is_control()
                    && !matches!(*ch, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
            })
            .take(MAX_TEXT)
            .collect();
        runtime_log::sanitize_text(&filtered)
    }

    fn event(
        timestamp: DateTime<Utc>,
        event_id: String,
        module: &str,
        message: String,
        native_priority: Option<u8>,
    ) -> RuntimeLogEvent {
        RuntimeLogEvent {
            schema_version: 1,
            event_id,
            timestamp,
            process_id: 0,
            source: LogSource::Linux,
            level: priority(native_priority),
            phase: LogPhase::Observed,
            context: LogContext {
                app: "linux".into(),
                module: module.into(),
                operation: "observe".into(),
                ..Default::default()
            },
            message,
            error_code: None,
            os_error_code: None,
            error_chain: vec![],
            source_path: None,
            target_path: None,
            incident_id: None,
            alert_key: None,
            repeat_count: 1,
            retry_count: 0,
            first_seen: None,
            last_seen: None,
            native_priority,
            native_source: None,
            timestamp_note: None,
        }
    }

    fn accepts(event: &RuntimeLogEvent, query: &LogQuery) -> bool {
        query.min_level.is_none_or(|level| event.level >= level)
            && query
                .module
                .as_deref()
                .is_none_or(|module| module == event.context.module)
            && query.since.is_none_or(|time| event.timestamp >= time)
            && query.until.is_none_or(|time| event.timestamp <= time)
    }

    fn finish(result: &mut LogQueryResult, query: &LogQuery) {
        result.events.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
        let limit = query.limit.min(MAX_RECORDS);
        if result.events.len() > limit {
            result.events.truncate(limit);
            result.truncated = true;
        }
        if result.damaged_records > 0 || result.truncated {
            result.state = LogSourceState::Partial;
        }
        if result.damaged_records > 0 {
            result.notices.push(format!(
                "Skipped {} malformed or unsupported records",
                result.damaged_records
            ));
        }
    }

    fn parse_journal(bytes: &[u8], query: &LogQuery) -> LogQueryResult {
        let mut result = LogQueryResult::default();
        for (index, line) in bytes
            .split(|byte| *byte == b'\n')
            .filter(|line| !line.is_empty())
            .enumerate()
        {
            if index >= MAX_RECORDS {
                result.truncated = true;
                break;
            }
            let parsed = (|| {
                let record: Value = serde_json::from_slice(line).ok()?;
                let cursor = scalar(&record, "__CURSOR")?;
                let micros = scalar(&record, "__REALTIME_TIMESTAMP")?
                    .parse::<i64>()
                    .ok()?;
                let timestamp = DateTime::from_timestamp_micros(micros)?;
                let message = record.get("MESSAGE")?.as_str()?;
                let kernel = scalar(&record, "_TRANSPORT").as_deref() == Some("kernel");
                let native_priority = scalar(&record, "PRIORITY")
                    .and_then(|p| p.parse::<u8>().ok())
                    .filter(|p| *p <= 7);
                let mut entry = event(
                    timestamp,
                    format!("journal:{}", clean(&cursor)),
                    if kernel {
                        "linux.kernel"
                    } else {
                        "linux.service"
                    },
                    clean(message),
                    native_priority,
                );
                entry.process_id = scalar(&record, "_PID")
                    .and_then(|pid| pid.parse().ok())
                    .unwrap_or(0);
                entry.native_source = Some(if kernel {
                    scalar(&record, "_KERNEL_DEVICE")
                        .map(|device| format!("journal; device={}", clean(&device)))
                        .unwrap_or_else(|| "journal; transport=kernel".into())
                } else {
                    scalar(&record, "_SYSTEMD_UNIT")
                        .or_else(|| scalar(&record, "_SYSTEMD_USER_UNIT"))
                        .map(|unit| format!("journal; unit={}", clean(&unit)))
                        .unwrap_or_else(|| "journal; external service".into())
                });
                Some(entry)
            })();
            match parsed {
                Some(event) if accepts(&event, query) => result.events.push(event),
                Some(_) => {}
                None => result.damaged_records += 1,
            }
        }
        finish(&mut result, query);
        result
    }

    fn parse_dmesg(bytes: &[u8], query: &LogQuery, raw: bool) -> LogQueryResult {
        let mut result = LogQueryResult::default();
        if bytes.is_empty() {
            return result;
        }
        let observed = Utc::now();
        let records: Vec<Option<(String, String, Option<u8>)>> = if raw {
            String::from_utf8_lossy(bytes)
                .lines()
                .rev()
                .map(|line| {
                    let (pri, rest) = line.strip_prefix('<')?.split_once('>')?;
                    let pri = pri.parse::<u8>().ok()?;
                    if pri > 7 {
                        return None;
                    } // --kernel must exclude userspace facilities.
                    let (time, message) = rest.trim_start().strip_prefix('[')?.split_once(']')?;
                    Some((
                        time.trim().to_owned(),
                        message.trim_start().into(),
                        Some(pri),
                    ))
                })
                .take(MAX_RECORDS + 1)
                .collect()
        } else {
            match serde_json::from_slice::<Value>(bytes)
                .ok()
                .and_then(|value| value.get("dmesg").and_then(Value::as_array).cloned())
            {
                Some(items) => items
                    .into_iter()
                    .rev()
                    .take(MAX_RECORDS + 1)
                    .map(|item| {
                        let pri = scalar(&item, "pri").and_then(|value| value.parse::<u8>().ok());
                        if pri.is_some_and(|pri| pri > 7) {
                            return None;
                        }
                        Some((
                            scalar(&item, "time")?,
                            item.get("msg")?.as_str()?.into(),
                            pri,
                        ))
                    })
                    .collect(),
                None => {
                    result.damaged_records = 1;
                    vec![]
                }
            }
        };
        // dmesg time is seconds since boot, not a reliable wall clock after
        // suspend. Store capture time only with an explicit precision note.
        // Wall-clock filters cannot truthfully match these records.
        let time_filtered = query.since.is_some() || query.until.is_some();
        for (index, record) in records.into_iter().enumerate() {
            if index >= MAX_RECORDS {
                result.truncated = true;
                continue;
            }
            let Some((monotonic, message, pri)) = record else {
                result.damaged_records += 1;
                continue;
            };
            if !monotonic
                .parse::<f64>()
                .is_ok_and(|value| value.is_finite() && value >= 0.0)
            {
                result.damaged_records += 1;
                continue;
            }
            let mut entry = event(
                observed,
                format!("dmesg:{}:{index}", clean(&monotonic)),
                "linux.kernel",
                clean(&message),
                pri,
            );
            entry.native_source = Some("dmesg; kernel ring buffer".into());
            entry.timestamp_note = Some(format!("Observed at timestamp; event occurred {} seconds since boot; wall-clock event time unavailable", clean(&monotonic)));
            if !time_filtered && accepts(&entry, query) {
                result.events.push(entry);
            }
        }
        if time_filtered {
            result.notices.push("dmesg records excluded: boot-relative timestamps cannot satisfy a wall-clock time filter".into());
            result.state = LogSourceState::Partial;
        }
        finish(&mut result, query);
        result
    }

    #[cfg(test)]
    mod tests;
}
