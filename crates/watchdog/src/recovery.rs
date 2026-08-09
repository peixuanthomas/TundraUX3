//! Versioned, privacy-preserving data for an out-of-process recovery UI.
//!
//! This module deliberately does not deserialize a full incident report.  The
//! launcher projects only the small, already-safe set of values it needs into
//! `RecoveryHandoffV1`; the recovery UI can then read that handoff without
//! gaining access to environment variables, command lines, or raw stderr.

use crate::config::WatchdogConfig;
use crate::report::{IncidentRecord, REPORT_SCHEMA_VERSION};
use crate::{WatchdogError, durable};
use chrono::{DateTime, SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Schema used by the first recovery handoff contract.
pub const RECOVERY_HANDOFF_V1_SCHEMA_VERSION: u32 = 1;
/// Header of the offline text represented by the recovery QR code.
pub const PANIC_CAPSULE_V1_HEADER: &str = "TUNDRA-PANIC-CAPSULE/1";
/// Largest permitted UTF-8 payload for the recovery QR code.
pub const PANIC_CAPSULE_MAX_BYTES: usize = 1_200;
// These limits intentionally match the recovery program's defensive display
// limits. Keeping the projection no larger means both sides encode the same
// capsule after a JSON handoff.
const MAX_IDENTIFIER_BYTES: usize = 96;
const MAX_COMPONENT_VERSION_BYTES: usize = 64;
const MAX_SOURCE_BYTES: usize = 96;
const MAX_SUMMARY_BYTES: usize = 240;
const MAX_TRACEBACK_FRAME_BYTES: usize = 120;
const MAX_TRACEBACK_FRAMES: usize = 8;
const TRUNCATED_LINE: &str = "Trace: truncated\n";
const DETAILS_UNAVAILABLE: &str = "Detailed report unavailable";
const REDACTED: &str = "[redacted]";
const MAX_INCIDENT_REPORT_BYTES: u64 = 1024 * 1024;
const MAX_INCIDENT_REPORT_FILES: usize = 64;

/// Versions displayed by the launcher recovery screen and encoded in its QR
/// capsule. These are labels rather than executable paths.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecoveryComponentVersionsV1 {
    pub tundra: String,
    pub shell: String,
    pub wezterm: String,
}

impl RecoveryComponentVersionsV1 {
    pub fn new(tundra: impl AsRef<str>, shell: impl AsRef<str>, wezterm: impl AsRef<str>) -> Self {
        Self {
            tundra: sanitize_text(tundra.as_ref(), MAX_COMPONENT_VERSION_BYTES).0,
            shell: sanitize_text(shell.as_ref(), MAX_COMPONENT_VERSION_BYTES).0,
            wezterm: sanitize_text(wezterm.as_ref(), MAX_COMPONENT_VERSION_BYTES).0,
        }
    }
}

/// Exit information safe to expose outside the full incident report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecoveryProcessFailureV1 {
    /// Which supervised component failed, for example `tundra-shell`.
    pub source: String,
    /// Process exit status, when a status was available.
    pub exit_code: Option<i32>,
    /// Platform-neutral signal/status name, when available.
    pub signal: Option<String>,
}

impl RecoveryProcessFailureV1 {
    pub fn new(source: impl AsRef<str>, exit_code: Option<i32>, signal: Option<String>) -> Self {
        Self {
            source: sanitize_text(source.as_ref(), MAX_SOURCE_BYTES).0,
            exit_code,
            signal: signal.map(|value| sanitize_text(&value, MAX_SOURCE_BYTES).0),
        }
    }
}

/// Input accepted by the watchdog when creating a recovery handoff.
///
/// Do not put process argv, environment values, raw stderr, clipboard data, or
/// absolute paths in this input. They are not part of the recovery protocol.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecoveryHandoffInputV1 {
    pub incident_id: String,
    pub session_id: String,
    pub occurred_at: DateTime<Utc>,
    pub failure: RecoveryProcessFailureV1,
    pub components: RecoveryComponentVersionsV1,
    pub restart_count: u32,
    pub summary: String,
    pub traceback_frames: Vec<String>,
    pub report_available: bool,
}

/// Read-only, serializable projection consumed by the WezTerm recovery UI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecoveryHandoffV1 {
    pub schema_version: u32,
    pub incident_id: String,
    pub session_id: String,
    pub occurred_at: DateTime<Utc>,
    pub failure: RecoveryProcessFailureV1,
    pub components: RecoveryComponentVersionsV1,
    pub restart_count: u32,
    pub report_available: bool,
    pub summary: String,
    pub traceback_frames: Vec<String>,
}

impl RecoveryHandoffV1 {
    /// Creates a restricted handoff from data that has already been selected by
    /// the process watchdog. Text is sanitized again at this trust boundary.
    pub fn new(input: RecoveryHandoffInputV1) -> Self {
        let (summary, _) = sanitize_text(&input.summary, MAX_SUMMARY_BYTES);
        let traceback_frames = input
            .traceback_frames
            .iter()
            .filter_map(|frame| {
                let (frame, _) = normalize_traceback_frame(frame);
                (!frame.is_empty()).then_some(frame)
            })
            .take(MAX_TRACEBACK_FRAMES)
            .collect();
        let report_available = input.report_available;
        Self {
            schema_version: RECOVERY_HANDOFF_V1_SCHEMA_VERSION,
            incident_id: sanitize_identifier(&input.incident_id),
            session_id: sanitize_identifier(&input.session_id),
            occurred_at: input.occurred_at,
            failure: RecoveryProcessFailureV1::new(
                input.failure.source,
                input.failure.exit_code,
                input.failure.signal,
            ),
            components: RecoveryComponentVersionsV1::new(
                input.components.tundra,
                input.components.shell,
                input.components.wezterm,
            ),
            restart_count: input.restart_count,
            report_available,
            summary: if report_available {
                summary
            } else {
                DETAILS_UNAVAILABLE.to_string()
            },
            traceback_frames: if report_available {
                traceback_frames
            } else {
                Vec::new()
            },
        }
    }

    /// Creates the guaranteed minimal handoff used when a detailed report is
    /// missing, corrupt, or could not be persisted.
    pub fn missing_report(mut input: RecoveryHandoffInputV1) -> Self {
        input.report_available = false;
        input.summary = DETAILS_UNAVAILABLE.to_string();
        input.traceback_frames.clear();
        Self::new(input)
    }

    /// Projects the newest valid v1 incident at or after `not_before` into a
    /// restricted recovery handoff.
    ///
    /// `incident_id`, `occurred_at`, `summary`, `traceback_frames`, and
    /// `report_available` in `base_input` are replaced by report data. The
    /// session, supervised-process status, component versions, and restart
    /// count remain launcher-owned. Corrupt, oversized, unsupported, or absent
    /// reports return `None`; callers can then use [`Self::missing_report`].
    pub fn from_latest_incident(
        config: &WatchdogConfig,
        not_before: DateTime<Utc>,
        mut base_input: RecoveryHandoffInputV1,
    ) -> Option<Self> {
        let record = latest_incident(config, not_before, &base_input.session_id)?;
        let backtrace = record
            .panic
            .as_ref()
            .map(|panic| panic.backtrace.as_str())
            .or_else(|| record.error.as_ref().map(|error| error.backtrace.as_str()))
            .unwrap_or_default();
        base_input.incident_id = record.incident_id.clone();
        base_input.occurred_at = record.occurred_at;
        base_input.summary = record.summary();
        base_input.traceback_frames = backtrace
            .lines()
            .filter(|line| !line.trim().is_empty())
            .take(MAX_TRACEBACK_FRAMES)
            .map(str::to_string)
            .collect();
        base_input.report_available = true;
        Some(Self::new(base_input))
    }

    /// Atomically persists this handoff. Readers will see either the previous
    /// complete JSON document or this complete JSON document, never a partial
    /// write.
    pub fn write_atomic(&self, path: impl AsRef<Path>) -> Result<(), WatchdogError> {
        let path = path.as_ref();
        let bytes = serde_json::to_vec_pretty(self)?;
        durable::atomic_write(path, &bytes).map_err(|source| WatchdogError::Io {
            operation: "write recovery handoff",
            path: path.to_path_buf(),
            source,
        })
    }

    /// Reads a handoff persisted by [`Self::write_atomic`].
    pub fn read(path: impl AsRef<Path>) -> Result<Self, WatchdogError> {
        let path = path.as_ref();
        let bytes = fs::read(path).map_err(|source| WatchdogError::Io {
            operation: "read recovery handoff",
            path: path.to_path_buf(),
            source,
        })?;
        let handoff = serde_json::from_slice::<Self>(&bytes)?;
        if handoff.schema_version != RECOVERY_HANDOFF_V1_SCHEMA_VERSION {
            return Err(WatchdogError::InvalidTaskPolicy(format!(
                "unsupported recovery handoff schema {}",
                handoff.schema_version
            )));
        }
        Ok(handoff.sanitized())
    }

    /// Produces the offline plain-text payload for a recovery QR code.
    /// The result is deterministic, contains at most eight normalized frames,
    /// and is always at most [`PANIC_CAPSULE_MAX_BYTES`] UTF-8 bytes.
    pub fn panic_capsule(&self) -> String {
        let handoff = self.sanitized();
        let mut frames = handoff.traceback_frames.clone();
        let mut truncated = false;
        loop {
            let mut payload = String::new();
            push_line(&mut payload, PANIC_CAPSULE_V1_HEADER);
            push_field(&mut payload, "Incident", &handoff.incident_id);
            push_field(&mut payload, "Session", &handoff.session_id);
            push_field(
                &mut payload,
                "UTC",
                &handoff
                    .occurred_at
                    .to_rfc3339_opts(SecondsFormat::Secs, true),
            );
            push_field(&mut payload, "Source", &handoff.failure.source);
            push_field(&mut payload, "Exit", &failure_status(&handoff.failure));
            push_field(&mut payload, "Restarts", &handoff.restart_count.to_string());
            push_field(
                &mut payload,
                "Versions",
                &format!(
                    "tundra={} shell={} wezterm={}",
                    handoff.components.tundra, handoff.components.shell, handoff.components.wezterm
                ),
            );
            push_field(
                &mut payload,
                "Summary",
                if handoff.report_available {
                    &handoff.summary
                } else {
                    DETAILS_UNAVAILABLE
                },
            );
            if handoff.report_available {
                for (index, frame) in frames.iter().enumerate() {
                    push_field(&mut payload, &format!("Frame {}", index + 1), frame);
                }
            }
            if truncated {
                payload.push_str(TRUNCATED_LINE);
            }
            push_field(
                &mut payload,
                "Full details",
                &format!("Diagnostics > Logs > {}", handoff.incident_id),
            );
            if payload.len() <= PANIC_CAPSULE_MAX_BYTES {
                return payload;
            }
            if frames.pop().is_none() {
                // All public fields are individually bounded, so the base
                // capsule must fit. Keep a defensive UTF-8 boundary anyway.
                truncate_utf8_in_place(&mut payload, PANIC_CAPSULE_MAX_BYTES);
                return payload;
            }
            truncated = true;
        }
    }

    /// Returns a safe copy after reapplying the projection's trust boundary.
    /// Recovery UIs should call this after receiving a handoff through a source
    /// other than [`Self::read`].
    pub fn sanitized(&self) -> Self {
        Self::new(RecoveryHandoffInputV1 {
            incident_id: self.incident_id.clone(),
            session_id: self.session_id.clone(),
            occurred_at: self.occurred_at,
            failure: self.failure.clone(),
            components: self.components.clone(),
            restart_count: self.restart_count,
            summary: self.summary.clone(),
            traceback_frames: self.traceback_frames.clone(),
            report_available: self.report_available,
        })
    }
}

#[derive(Debug)]
struct IncidentFile {
    path: PathBuf,
    is_primary: bool,
    modified_at: SystemTime,
}

fn latest_incident(
    config: &WatchdogConfig,
    not_before: DateTime<Utc>,
    session_id: &str,
) -> Option<IncidentRecord> {
    let mut latest: Option<(IncidentRecord, bool, SystemTime)> = None;
    for file in bounded_incident_files(config) {
        let Some(bytes) = read_bounded_report(&file.path) else {
            continue;
        };
        let record = match serde_json::from_slice::<IncidentRecord>(&bytes) {
            Ok(record)
                if record.schema_version == REPORT_SCHEMA_VERSION
                    && record.occurred_at >= not_before
                    && record.session_id.as_deref() == Some(session_id) =>
            {
                record
            }
            _ => continue,
        };
        let preferred = latest
            .as_ref()
            .is_none_or(|(existing, existing_primary, modified_at)| {
                record.occurred_at > existing.occurred_at
                    || (record.occurred_at == existing.occurred_at
                        && ((file.is_primary && !*existing_primary)
                            || (file.is_primary == *existing_primary
                                && file.modified_at > *modified_at)))
            });
        if preferred {
            latest = Some((record, file.is_primary, file.modified_at));
        }
    }
    latest.map(|(record, _, _)| record)
}

fn bounded_incident_files(config: &WatchdogConfig) -> Vec<IncidentFile> {
    let mut files = Vec::with_capacity(MAX_INCIDENT_REPORT_FILES);
    collect_incident_files(&config.report_dir, true, &mut files);
    collect_incident_files(&config.fallback_dir, false, &mut files);
    sort_and_limit_incident_files(&mut files);
    files
}

fn sort_and_limit_incident_files(files: &mut Vec<IncidentFile>) {
    files.sort_by(|left, right| {
        right
            .modified_at
            .cmp(&left.modified_at)
            .then_with(|| right.is_primary.cmp(&left.is_primary))
            .then_with(|| left.path.cmp(&right.path))
    });
    files.truncate(MAX_INCIDENT_REPORT_FILES);
}

fn collect_incident_files(directory: &Path, is_primary: bool, output: &mut Vec<IncidentFile>) {
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|extension| extension.to_str()) != Some("json") {
            continue;
        }
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if !file_type.is_file() {
            continue;
        }
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        if !metadata.is_file() || metadata.len() > MAX_INCIDENT_REPORT_BYTES {
            continue;
        }
        let Ok(modified_at) = metadata.modified() else {
            continue;
        };
        output.push(IncidentFile {
            path,
            is_primary,
            modified_at,
        });
        if output.len() > MAX_INCIDENT_REPORT_FILES {
            sort_and_limit_incident_files(output);
        }
    }
}

fn read_bounded_report(path: &Path) -> Option<Vec<u8>> {
    let file = fs::File::open(path).ok()?;
    let mut bytes = Vec::new();
    file.take(MAX_INCIDENT_REPORT_BYTES + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    (bytes.len() as u64 <= MAX_INCIDENT_REPORT_BYTES).then_some(bytes)
}

fn push_line(output: &mut String, value: &str) {
    output.push_str(value);
    output.push('\n');
}

fn push_field(output: &mut String, name: &str, value: &str) {
    output.push_str(name);
    output.push_str(": ");
    output.push_str(value);
    output.push('\n');
}

fn sanitize_identifier(value: &str) -> String {
    sanitize_text(value, MAX_IDENTIFIER_BYTES).0
}

fn normalize_traceback_frame(value: &str) -> (String, bool) {
    let (value, truncated) = sanitize_text(value, MAX_TRACEBACK_FRAME_BYTES);
    if value == REDACTED {
        return (value, truncated);
    }
    (
        value
            .rsplit(['/', '\\'])
            .next()
            .unwrap_or(&value)
            .to_string(),
        truncated,
    )
}

fn failure_status(failure: &RecoveryProcessFailureV1) -> String {
    match (failure.exit_code, failure.signal.as_deref()) {
        (Some(code), Some(signal)) => format!("exit {code}; {signal}"),
        (Some(code), None) => format!("exit {code}"),
        (None, Some(signal)) => signal.to_string(),
        (None, None) => "unknown".to_string(),
    }
}

fn sanitize_text(value: &str, maximum_bytes: usize) -> (String, bool) {
    let collapsed = value
        .chars()
        .filter(|character| !character.is_control())
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let lower = collapsed.to_ascii_lowercase();
    if contains_sensitive_content(&lower) || contains_absolute_path(&lower) {
        return (REDACTED.to_string(), false);
    }
    let truncated = collapsed.len() > maximum_bytes;
    (truncate_utf8(&collapsed, maximum_bytes), truncated)
}

fn contains_sensitive_content(lower: &str) -> bool {
    [
        "password",
        "passwd",
        "token",
        "secret",
        "api_key",
        "apikey",
        "authorization",
        "bearer ",
        "clipboard",
        "paste",
        "argv",
        "command line",
        "environment",
        "env=",
        "username",
        "user=",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
}

fn contains_absolute_path(lower: &str) -> bool {
    lower.starts_with('/')
        || lower.contains("\\\\")
        || lower.contains(":\\")
        || ["/home/", "/users/", "/private/", "/var/", "/tmp/", "/root/"]
            .iter()
            .any(|needle| lower.contains(needle))
}

fn truncate_utf8(value: &str, maximum_bytes: usize) -> String {
    let mut output = value.to_string();
    truncate_utf8_in_place(&mut output, maximum_bytes);
    output
}

fn truncate_utf8_in_place(value: &mut String, maximum_bytes: usize) {
    if value.len() <= maximum_bytes {
        return;
    }
    let mut boundary = maximum_bytes;
    while boundary > 0 && !value.is_char_boundary(boundary) {
        boundary -= 1;
    }
    value.truncate(boundary);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::report::{ErrorDetails, PanicDetails};
    use crate::{BoundaryKind, IncidentKind, IncidentSeverity, RecoveryOutcome, RuntimeSnapshot};
    use chrono::TimeZone;
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_TEST: AtomicU64 = AtomicU64::new(1);

    fn input() -> RecoveryHandoffInputV1 {
        RecoveryHandoffInputV1 {
            incident_id: "incident-42".to_string(),
            session_id: "session-9".to_string(),
            occurred_at: Utc.with_ymd_and_hms(2026, 8, 9, 1, 2, 3).unwrap(),
            failure: RecoveryProcessFailureV1::new(
                "tundra-shell",
                Some(101),
                Some("SIGABRT".to_string()),
            ),
            components: RecoveryComponentVersionsV1::new("1.2.3", "1.2.3", "wezterm-fork"),
            restart_count: 3,
            summary: "renderer failed safely".to_string(),
            traceback_frames: vec!["tundra::render::draw (src/render.rs:42)".to_string()],
            report_available: true,
        }
    }

    fn temporary_path(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "watchdog-recovery-{name}-{}-{}",
            std::process::id(),
            NEXT_TEST.fetch_add(1, Ordering::Relaxed)
        ))
    }

    fn projection_config(root: &Path) -> WatchdogConfig {
        WatchdogConfig::new(
            root.join("reports"),
            root.join("fallback"),
            root.join("data"),
            "projection-test",
            "1.2.3",
        )
    }

    fn incident(
        incident_id: &str,
        occurred_at: DateTime<Utc>,
        summary: &str,
        backtrace: &str,
        is_panic: bool,
    ) -> IncidentRecord {
        IncidentRecord {
            schema_version: REPORT_SCHEMA_VERSION,
            incident_id: incident_id.to_string(),
            report_stem: incident_id.to_string(),
            kind: if is_panic {
                IncidentKind::Panic
            } else {
                IncidentKind::Error
            },
            severity: IncidentSeverity::Critical,
            occurred_at,
            process_name: "projection-test".to_string(),
            process_version: "1.2.3".to_string(),
            session_id: Some("session-9".to_string()),
            process_id: 42,
            run_id: "private-run-id".to_string(),
            app: None,
            component: None,
            task_id: None,
            task_group: None,
            boundary: "process.failure".to_string(),
            boundary_kind: BoundaryKind::Process,
            task_kind: None,
            replay_safety: None,
            operation_kind: None,
            operation_id: None,
            recovery_handler_version: None,
            panic_action: None,
            restart_policy: None,
            restart_attempt: 0,
            thread_name: None,
            thread_id: "private-thread-id".to_string(),
            panic: is_panic.then(|| PanicDetails {
                payload: summary.to_string(),
                source_file: Some("C:\\Users\\alice\\private.rs".to_string()),
                source_line: Some(7),
                source_column: Some(9),
                backtrace: backtrace.to_string(),
            }),
            error: (!is_panic).then(|| ErrorDetails {
                message: summary.to_string(),
                source_chain: vec!["raw private error detail".to_string()],
                backtrace: backtrace.to_string(),
            }),
            runtime: RuntimeSnapshot {
                screen: Some("raw screen content".to_string()),
                last_command: Some("argv --token=private".to_string()),
                terminal_size: Some((120, 40)),
                active_operation: None,
            },
            breadcrumbs: Vec::new(),
            recovery: RecoveryOutcome::Unrecoverable("private recovery detail".to_string()),
            secondary_errors: vec!["raw stderr".to_string()],
        }
    }

    fn write_incident(directory: &Path, name: &str, incident: &IncidentRecord) {
        fs::create_dir_all(directory).unwrap();
        fs::write(
            directory.join(format!("{name}.json")),
            serde_json::to_vec(incident).unwrap(),
        )
        .unwrap();
    }

    #[test]
    fn capsule_is_deterministic_and_bounded() {
        let mut input = input();
        input.summary = "x".repeat(10_000);
        input.traceback_frames = (0..32)
            .map(|number| format!("frame {number}: {}", "y".repeat(500)))
            .collect();
        let handoff = RecoveryHandoffV1::new(input);
        let first = handoff.panic_capsule();
        assert_eq!(first, handoff.panic_capsule());
        assert!(first.len() <= PANIC_CAPSULE_MAX_BYTES);
        assert!(first.starts_with("TUNDRA-PANIC-CAPSULE/1\n"));
        assert!(first.contains("Trace: truncated"));
        assert!(first.contains("Full details: Diagnostics > Logs > incident-42"));
        assert!(first.matches("Frame ").count() <= MAX_TRACEBACK_FRAMES);
    }

    #[test]
    fn paths_tokens_environment_and_argv_are_redacted() {
        let mut input = input();
        input.summary = "token=top-secret at C:\\Users\\alice\\secret.txt".to_string();
        input.traceback_frames = vec![
            "at /home/alice/project/src/main.rs:11".to_string(),
            "argv: --password hunter2".to_string(),
        ];
        let handoff = RecoveryHandoffV1::new(input);
        assert_eq!(handoff.summary, REDACTED);
        assert_eq!(handoff.traceback_frames, vec![REDACTED, REDACTED]);
        let payload = handoff.panic_capsule();
        assert!(!payload.contains("alice"));
        assert!(!payload.contains("hunter2"));
        assert!(!payload.contains("top-secret"));
    }

    #[test]
    fn missing_report_uses_safe_fallback() {
        let handoff = RecoveryHandoffV1::missing_report(input());
        assert!(!handoff.report_available);
        assert_eq!(handoff.summary, DETAILS_UNAVAILABLE);
        assert!(handoff.traceback_frames.is_empty());
        assert!(handoff.panic_capsule().contains(DETAILS_UNAVAILABLE));
    }

    #[test]
    fn normalizes_and_limits_traceback_frames() {
        let mut input = input();
        input.traceback_frames = (0..12)
            .map(|index| format!("  frame {index}\n with\tspaces  "))
            .collect();
        let handoff = RecoveryHandoffV1::new(input);
        assert_eq!(handoff.traceback_frames.len(), MAX_TRACEBACK_FRAMES);
        assert_eq!(handoff.traceback_frames[0], "frame 0 withspaces");
    }

    #[test]
    fn capsule_uses_the_recovery_programs_canonical_wire_format() {
        let capsule = RecoveryHandoffV1::new(input()).panic_capsule();
        assert_eq!(
            capsule,
            concat!(
                "TUNDRA-PANIC-CAPSULE/1\n",
                "Incident: incident-42\n",
                "Session: session-9\n",
                "UTC: 2026-08-09T01:02:03Z\n",
                "Source: tundra-shell\n",
                "Exit: exit 101; SIGABRT\n",
                "Restarts: 3\n",
                "Versions: tundra=1.2.3 shell=1.2.3 wezterm=wezterm-fork\n",
                "Summary: renderer failed safely\n",
                "Frame 1: render.rs:42)\n",
                "Full details: Diagnostics > Logs > incident-42\n",
            )
        );
    }

    #[test]
    fn serde_round_trip_and_atomic_write() {
        let handoff = RecoveryHandoffV1::new(input());
        let json = serde_json::to_string(&handoff).unwrap();
        assert_eq!(
            serde_json::from_str::<RecoveryHandoffV1>(&json).unwrap(),
            handoff
        );

        let root = temporary_path("atomic");
        let path = root.join("handoff.json");
        handoff.write_atomic(&path).unwrap();
        assert_eq!(RecoveryHandoffV1::read(&path).unwrap(), handoff);
        assert!(fs::read_to_string(&path).unwrap().contains("incident-42"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn read_reapplies_the_safety_boundary() {
        let mut unsafe_handoff = RecoveryHandoffV1::new(input());
        unsafe_handoff.summary = "C:\\Users\\alice\\token=unsafe".to_string();
        let root = temporary_path("unsafe-read");
        let path = root.join("handoff.json");
        fs::create_dir_all(&root).unwrap();
        fs::write(&path, serde_json::to_vec(&unsafe_handoff).unwrap()).unwrap();

        let recovered = RecoveryHandoffV1::read(&path).unwrap();
        assert_eq!(recovered.summary, REDACTED);
        assert!(!recovered.panic_capsule().contains("alice"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn latest_incident_projection_scans_both_directories_and_filters_by_time() {
        let root = temporary_path("latest-projection");
        let config = projection_config(&root);
        let now = Utc.with_ymd_and_hms(2026, 8, 9, 2, 0, 0).unwrap();
        write_incident(
            &config.report_dir,
            "old",
            &incident(
                "old",
                now - chrono::Duration::minutes(10),
                "old",
                "old",
                false,
            ),
        );
        write_incident(
            &config.fallback_dir,
            "fallback",
            &incident(
                "fallback",
                now - chrono::Duration::minutes(2),
                "fallback failure",
                "fallback::frame",
                false,
            ),
        );
        write_incident(
            &config.report_dir,
            "latest",
            &incident(
                "latest",
                now - chrono::Duration::minutes(1),
                "latest failure",
                "latest::frame",
                false,
            ),
        );

        let handoff = RecoveryHandoffV1::from_latest_incident(
            &config,
            now - chrono::Duration::minutes(5),
            input(),
        )
        .unwrap();
        assert_eq!(handoff.incident_id, "latest");
        assert_eq!(handoff.occurred_at, now - chrono::Duration::minutes(1));
        assert_eq!(handoff.summary, "latest failure");
        assert_eq!(handoff.traceback_frames, vec!["latest::frame"]);
        assert!(handoff.report_available);
        assert!(RecoveryHandoffV1::from_latest_incident(&config, now, input()).is_none());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn incident_projection_never_exposes_private_report_fields() {
        let root = temporary_path("private-projection");
        let config = projection_config(&root);
        let occurred_at = Utc.with_ymd_and_hms(2026, 8, 9, 3, 0, 0).unwrap();
        let backtrace = (0..12)
            .map(|index| match index {
                1 => "at /home/alice/private.rs:7".to_string(),
                2 => "argv: --password hunter2".to_string(),
                _ => format!("tundra::safe::frame_{index}"),
            })
            .collect::<Vec<_>>()
            .join("\n");
        write_incident(
            &config.report_dir,
            "private",
            &incident(
                "private",
                occurred_at,
                "token=top-secret at C:\\Users\\alice\\private.rs",
                &backtrace,
                true,
            ),
        );

        let handoff = RecoveryHandoffV1::from_latest_incident(
            &config,
            occurred_at - chrono::Duration::seconds(1),
            input(),
        )
        .unwrap();
        assert_eq!(handoff.summary, REDACTED);
        assert_eq!(handoff.traceback_frames.len(), MAX_TRACEBACK_FRAMES);
        let exposed = format!(
            "{}\n{}",
            serde_json::to_string(&handoff).unwrap(),
            handoff.panic_capsule()
        );
        for private in [
            "alice",
            "hunter2",
            "top-secret",
            "private-run-id",
            "private-thread-id",
            "raw screen content",
            "raw stderr",
        ] {
            assert!(!exposed.contains(private), "leaked {private}");
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn incident_projection_requires_the_exact_current_session() {
        let root = temporary_path("session-projection");
        let config = projection_config(&root);
        let now = Utc.with_ymd_and_hms(2026, 8, 9, 4, 0, 0).unwrap();
        let matching = incident(
            "matching",
            now - chrono::Duration::minutes(2),
            "matching session",
            "matching::frame",
            false,
        );
        let mut wrong = incident(
            "wrong",
            now - chrono::Duration::minutes(1),
            "wrong session",
            "wrong::frame",
            false,
        );
        wrong.session_id = Some("another-session".to_string());
        let legacy = incident("legacy", now, "legacy report", "legacy::frame", false);
        write_incident(&config.report_dir, "matching", &matching);
        write_incident(&config.report_dir, "wrong", &wrong);
        fs::create_dir_all(&config.fallback_dir).unwrap();
        let mut legacy_json = serde_json::to_value(legacy).unwrap();
        legacy_json.as_object_mut().unwrap().remove("session_id");
        fs::write(
            config.fallback_dir.join("legacy.json"),
            serde_json::to_vec(&legacy_json).unwrap(),
        )
        .unwrap();

        let handoff = RecoveryHandoffV1::from_latest_incident(
            &config,
            now - chrono::Duration::minutes(5),
            input(),
        )
        .unwrap();
        assert_eq!(handoff.incident_id, "matching");
        assert!(
            RecoveryHandoffV1::from_latest_incident(
                &config,
                now - chrono::Duration::seconds(90),
                input(),
            )
            .is_none()
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn incident_projection_ignores_corrupt_unsupported_and_oversized_reports() {
        let root = temporary_path("bounded-projection");
        let config = projection_config(&root);
        fs::create_dir_all(&config.report_dir).unwrap();
        fs::write(config.report_dir.join("corrupt.json"), b"{bad-json").unwrap();
        fs::write(
            config.report_dir.join("oversized.json"),
            vec![b'x'; MAX_INCIDENT_REPORT_BYTES as usize + 1],
        )
        .unwrap();
        let mut unsupported = incident("unsupported", Utc::now(), "future", "frame", false);
        unsupported.schema_version += 1;
        write_incident(&config.report_dir, "unsupported", &unsupported);
        assert!(
            RecoveryHandoffV1::from_latest_incident(
                &config,
                Utc::now() - chrono::Duration::minutes(1),
                input(),
            )
            .is_none()
        );

        for index in 0..(MAX_INCIDENT_REPORT_FILES + 20) {
            fs::write(
                config.report_dir.join(format!("invalid-{index:03}.json")),
                b"{}",
            )
            .unwrap();
        }
        assert_eq!(
            bounded_incident_files(&config).len(),
            MAX_INCIDENT_REPORT_FILES
        );
        fs::remove_dir_all(root).unwrap();
    }
}
