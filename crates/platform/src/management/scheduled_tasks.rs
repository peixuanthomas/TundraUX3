//! Existing Linux schedules and persistent system-level service/timer tasks.
use super::*;
use crate::ManagedCommand;
use sha2::{Digest, Sha256};
use std::fs;
use std::io::{Read, Write};
use std::os::fd::AsRawFd;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

mod cron;
#[cfg(test)]
#[path = "../../tests/unit/management/scheduled_tasks_isolated.rs"]
mod isolated;
#[cfg(test)]
#[path = "../../tests/unit/management/scheduled_tasks_query_tests.rs"]
mod query_tests;
mod systemd;

const RESULTS: &str = "/var/lib/tundraux3/scheduled-results";
const MAX_DATA: usize = 4 * 1024 * 1024;

fn check_cancelled(flag: &AtomicBool) -> Result<(), ManagementError> {
    if flag.load(Ordering::Relaxed) {
        Err(ManagementError::Cancelled)
    } else {
        Ok(())
    }
}

fn invalid(message: impl Into<String>) -> ManagementError {
    ManagementError::InvalidInput(message.into())
}

fn state_action_label(action: &str) -> String {
    match action {
        "pause" => i18n::tr!("scheduled-task-operation-pause"),
        "resume" => i18n::tr!("scheduled-task-operation-resume"),
        _ => action.into(),
    }
}

fn io_error(error: std::io::Error) -> ManagementError {
    if error.kind() == std::io::ErrorKind::PermissionDenied {
        ManagementError::PermissionDenied(error.to_string())
    } else {
        ManagementError::Failed(error.to_string())
    }
}

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn native(
    program: &str,
    arguments: &[String],
    input: Option<&[u8]>,
    flag: &AtomicBool,
) -> Result<std::process::Output, ManagementError> {
    check_cancelled(flag)?;
    let mut child = Command::new(program)
        .args(arguments)
        .env("LC_ALL", "C")
        .env("SYSTEMD_COLORS", "0")
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn_managed()
        .map_err(|error| {
            let detail = i18n::tr!(
                "scheduled-task-cannot-start",
                program = program,
                error = error.to_string()
            );
            match error.kind() {
                std::io::ErrorKind::NotFound => ManagementError::MissingDependency(detail),
                std::io::ErrorKind::PermissionDenied => ManagementError::PermissionDenied(detail),
                _ => ManagementError::Failed(detail),
            }
        })?;
    if let Some(input) = input {
        child
            .stdin
            .take()
            .ok_or_else(|| invalid(i18n::tr!("scheduled-task-missing-input")))?
            .write_all(input)
            .map_err(io_error)?;
    }
    let result = (|| {
        let mut stdout = child
            .stdout
            .take()
            .ok_or_else(|| invalid(i18n::tr!("scheduled-task-missing-output")))?;
        let mut stderr = child
            .stderr
            .take()
            .ok_or_else(|| invalid(i18n::tr!("scheduled-task-missing-errors")))?;
        for fd in [stdout.as_raw_fd(), stderr.as_raw_fd()] {
            let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
            if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0
            {
                return Err(io_error(std::io::Error::last_os_error()));
            }
        }
        let started = Instant::now();
        let mut out = Vec::new();
        let mut err = Vec::new();
        loop {
            check_cancelled(flag)?;
            drain(&mut stdout, &mut out)?;
            drain(&mut stderr, &mut err)?;
            if let Some(status) = child.try_wait().map_err(io_error)? {
                drain(&mut stdout, &mut out)?;
                drain(&mut stderr, &mut err)?;
                return Ok(std::process::Output {
                    status,
                    stdout: out,
                    stderr: err,
                });
            }
            if started.elapsed() > Duration::from_secs(30) {
                return Err(ManagementError::Failed(i18n::tr!(
                    "scheduled-task-timeout",
                    program = program
                )));
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    })();
    if child.try_wait().is_ok_and(|status| status.is_none()) {
        let _ = child.kill();
    }
    let _ = child.wait();
    result
}

fn drain(reader: &mut impl Read, bytes: &mut Vec<u8>) -> Result<(), ManagementError> {
    let mut buffer = [0u8; 8192];
    loop {
        match reader.read(&mut buffer) {
            Ok(0) => return Ok(()),
            Ok(count) => {
                bytes.extend_from_slice(&buffer[..count]);
                if bytes.len() > MAX_DATA {
                    return Err(invalid(i18n::tr!("scheduled-task-output-large")));
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => return Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(io_error(error)),
        }
    }
}

fn run(program: &str, arguments: &[String], flag: &AtomicBool) -> Result<String, ManagementError> {
    let output = native(program, arguments, None, flag)?;
    if !output.status.success() {
        let message = runtime_log::sanitize_text(String::from_utf8_lossy(&output.stderr).trim());
        let permission = message.to_lowercase();
        return Err(
            if permission.contains("permission denied")
                || permission.contains("access denied")
                || permission.contains("authentication is required")
            {
                ManagementError::PermissionDenied(message)
            } else {
                ManagementError::Failed(i18n::tr!(
                    "scheduled-task-tool-failed",
                    program = program,
                    error = message
                ))
            },
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn text_file(path: &Path) -> Result<String, ManagementError> {
    use std::os::unix::fs::OpenOptionsExt;
    let file = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)
        .map_err(io_error)?;
    let metadata = file.metadata().map_err(io_error)?;
    if !metadata.is_file() || metadata.len() > MAX_DATA as u64 {
        return Err(invalid(i18n::tr!(
            "scheduled-task-file-invalid",
            path = path.display().to_string()
        )));
    }
    let mut text = String::new();
    file.take(MAX_DATA as u64 + 1)
        .read_to_string(&mut text)
        .map_err(io_error)?;
    if text.len() > MAX_DATA {
        return Err(invalid(i18n::tr!("scheduled-task-source-grew")));
    }
    Ok(text)
}

fn actor(uid: u32) -> Result<users::UserAccount, ManagementError> {
    users::accounts(Some(&uid.to_string()))?
        .into_iter()
        .find(|account| account.uid == uid && account.local)
        .ok_or_else(|| invalid(i18n::tr!("scheduled-task-actor-missing")))
}

fn require_root() -> Result<(), ManagementError> {
    if unsafe { libc::geteuid() } != 0 {
        Err(ManagementError::PermissionDenied(
            i18n::tr!("scheduled-task-authorization").into(),
        ))
    } else {
        Ok(())
    }
}

fn local_identity(user: &str) -> Option<(u32, u32)> {
    fs::read_to_string("/etc/passwd")
        .ok()?
        .lines()
        .find_map(|line| {
            let fields = line.split(':').collect::<Vec<_>>();
            if fields.len() != 7 || fields[0] != user && fields[2] != user {
                return None;
            }
            Some((fields[2].parse().ok()?, fields[3].parse().ok()?))
        })
}

fn result_detail(key: &str, uid: u32) -> String {
    match text_file(
        &Path::new(RESULTS)
            .join(uid.to_string())
            .join(format!("{key}.result")),
    ) {
        Ok(value) => value
            .lines()
            .filter(|line| !line.is_empty())
            .collect::<Vec<_>>()
            .join("; "),
        Err(_) => i18n::tr!("scheduled-task-no-result").into(),
    }
}

// Keep the recorder under the task's identity; cron can set loader/shell variables.
fn result_command(key: &str, uid: u32) -> String {
    let path = format!("{RESULTS}/{uid}/{key}.result");
    let script = format!(
        "umask 022; /usr/bin/printf '%s\\n' \"$(/usr/bin/date --iso-8601=seconds)\" \"$SERVICE_RESULT\" \"$EXIT_CODE\" \"$EXIT_STATUS\" > {path}.tmp.$$ && /usr/bin/mv -f -- {path}.tmp.$$ {path}"
    );
    format!(":/bin/sh -c {}", systemd::quote(&script))
}

fn prepare_results(uid: u32, gid: u32) -> Result<(), ManagementError> {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    for path in [Path::new("/var/lib/tundraux3"), Path::new(RESULTS)] {
        match fs::create_dir(path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(io_error(error)),
        }
        let metadata = fs::symlink_metadata(path).map_err(io_error)?;
        if !metadata.is_dir() || metadata.uid() != 0 || metadata.mode() & 0o022 != 0 {
            return Err(invalid(i18n::tr!("scheduled-task-unsafe-result-directory")));
        }
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).map_err(io_error)?;
    }
    let path = Path::new(RESULTS).join(uid.to_string());
    match fs::create_dir(&path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(io_error(error)),
    }
    let metadata = fs::symlink_metadata(&path).map_err(io_error)?;
    if !metadata.is_dir() || metadata.uid() != uid && metadata.uid() != 0 {
        return Err(invalid(i18n::tr!("scheduled-task-result-owner")));
    }
    let path_c = std::ffi::CString::new(path.as_os_str().as_encoded_bytes())
        .map_err(|_| invalid(i18n::tr!("scheduled-task-result-path")))?;
    if unsafe { libc::chown(path_c.as_ptr(), uid, gid) } != 0 {
        return Err(io_error(std::io::Error::last_os_error()));
    }
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).map_err(io_error)
}

pub fn query(
    query: &ManagementQuery,
    flag: &AtomicBool,
) -> Result<ManagementSnapshot, ManagementError> {
    query_as_actor(query, unsafe { libc::getuid() }, flag)
}

fn append_query_problem(
    snapshot: &mut ManagementSnapshot,
    source: String,
    error: ManagementError,
) -> Result<(), ManagementError> {
    if matches!(error, ManagementError::Cancelled) {
        return Err(error);
    }
    let problem = problem::OperationProblem::from_error(&error);
    snapshot.notices.push(i18n::tr!(
        "management-source-unavailable",
        source = source.clone(),
        reason = i18n::tr!(problem.summary_key)
    ));
    snapshot.details.push((source, problem.detail));
    Ok(())
}

fn query_as_actor(
    query: &ManagementQuery,
    uid: u32,
    flag: &AtomicBool,
) -> Result<ManagementSnapshot, ManagementError> {
    check_cancelled(flag)?;
    let mut snapshot = ManagementSnapshot {
        columns: [
            i18n::tr!("scheduled-task-column-task"),
            i18n::tr!("scheduled-task-column-source"),
            i18n::tr!("scheduled-task-column-user"),
            i18n::tr!("scheduled-task-column-schedule"),
            i18n::tr!("scheduled-task-column-state"),
            i18n::tr!("scheduled-task-column-last"),
            i18n::tr!("scheduled-task-column-next"),
        ]
        .map(String::from)
        .to_vec(),
        backend: i18n::tr!("scheduled-task-backend").into(),
        details_complete: true,
        ..Default::default()
    };
    for scope in [
        "system".to_string(),
        if unsafe { libc::geteuid() } == 0 {
            format!("user:{uid}")
        } else {
            "user".into()
        },
    ] {
        match systemd::rows(&scope, flag) {
            Ok(rows) => snapshot.rows.extend(rows),
            Err(error) => {
                let label = i18n::tr!(if scope == "system" {
                    "scheduled-task-scope-system"
                } else {
                    "scheduled-task-scope-user"
                });
                append_query_problem(
                    &mut snapshot,
                    i18n::tr!("management-schedule-source", scope = label),
                    error,
                )?;
            }
        }
    }
    cron::append_rows(&mut snapshot, uid, flag)?;
    let needle = query.filter.to_lowercase();
    snapshot
        .rows
        .retain(|row| needle.is_empty() || row.cells.join(" ").to_lowercase().contains(&needle));
    snapshot.rows.sort_by(|a, b| a.id.cmp(&b.id));
    snapshot.actions = systemd::create_actions(uid)?;
    if let Err(error) = super::capability::system_manager() {
        for action in &mut snapshot.actions {
            if action.id == "create_task" {
                action.disabled_reason = Some(error.to_string());
            }
        }
    }
    snapshot.actions.push(ManagementAction {
        id: "inspect_scheduled_tasks".into(),
        label: i18n::tr!("scheduled-task-inspect").into(),
        values: query.inspection_values(),
        privileged: true,
        confirm: true,
        ..Default::default()
    });
    snapshot.details.push((
        i18n::tr!("management-schedule-limitations"),
        format!(
            "{}\n{}",
            i18n::tr!("scheduled-task-cron-results-notice"),
            i18n::tr!("scheduled-task-pause-notice")
        ),
    ));
    Ok(snapshot)
}

pub fn execute(
    command: &ManagementCommand,
    context: &ExecutionContext,
    interaction: &mut dyn OperationInteraction,
    flag: &AtomicBool,
) -> Result<String, ManagementError> {
    check_cancelled(flag)?;
    if command.action == "inspect_scheduled_tasks" {
        require_root()?;
        interaction.emit(OperationEvent::Snapshot {
            snapshot: query_as_actor(
                &ManagementQuery::from_inspection(command),
                context.actor_uid,
                flag,
            )?
            .with_authorized_refresh(command),
        });
        return Ok(i18n::tr!("scheduled-task-inspected").into());
    }
    if matches!(command.action.as_str(), "preview_task" | "create_task") {
        return systemd::create(command, context, interaction, flag);
    }
    if !matches!(command.action.as_str(), "pause" | "resume" | "run_now") {
        return Err(invalid(i18n::tr!("scheduled-task-unknown-operation")));
    }
    match command.identity.get("source").map(String::as_str) {
        Some("systemd") => systemd::execute(command, context, interaction, flag),
        Some("crontab" | "cron-file") => cron::execute(command, context, interaction, flag),
        _ => Err(invalid(i18n::tr!("scheduled-task-select-task"))),
    }
}

pub(crate) fn restore_backup(
    backup_id: &str,
    payload: &serde_json::Value,
    context: &ExecutionContext,
    interaction: &mut dyn OperationInteraction,
    flag: &AtomicBool,
) -> Result<String, ManagementError> {
    require_root()?;
    restore_preflight(payload, context, flag)?;
    let paths = super::config_backups::snapshots(backup_id)?
        .into_iter()
        .map(|(before, _)| before.path)
        .collect::<Vec<_>>();
    let session = super::config_backups::begin("scheduled-tasks", &paths)?;
    let mut inverse = payload.clone();
    inverse["before"] = payload.get("after").cloned().unwrap_or_default();
    inverse["after"] = payload.get("before").cloned().unwrap_or_default();
    if payload.get("type").and_then(serde_json::Value::as_str) == Some("created-timer") {
        inverse["type"] = "timer-state".into();
        inverse.as_object_mut().unwrap().remove("version");
    }
    session.set_payload(inverse.clone())?;
    let result = (|| {
        systemd::before_restore(payload, context, flag)?;
        super::config_backups::restore_files_unrecorded(backup_id)?;
        match payload.get("type").and_then(serde_json::Value::as_str) {
            Some("crontab") => cron::restore(payload, context, interaction, flag),
            Some("timer-state" | "created-timer") => {
                systemd::restore(payload, context, interaction, flag)
            }
            Some("cron-file") => Ok(i18n::tr!("scheduled-task-cron-file-restored").into()),
            _ => Err(invalid(i18n::tr!("scheduled-task-unsupported-backup"))),
        }
    })();
    let actual = systemd::recovery_after(payload, context, &AtomicBool::new(false));
    if let Ok(Some(actual)) = &actual {
        inverse["after"] = actual.0.clone();
        if let Some(version) = &actual.1 {
            inverse["version"] = version.clone().into();
        } else {
            inverse.as_object_mut().unwrap().remove("version");
        }
        session.set_payload(inverse.clone())?;
    }
    let cron_after = cron::recovery_after(payload, context, &AtomicBool::new(false));
    if let Ok(Some(after)) = &cron_after {
        inverse["after"] = after.clone();
        session.set_payload(inverse)?;
    }
    session.finish(if result.is_ok() && actual.is_ok() && cron_after.is_ok() {
        "completed"
    } else {
        "failed"
    })?;
    actual?;
    cron_after?;
    result.map(|message| {
        i18n::tr!(
            "scheduled-task-recovery-backup",
            message = message,
            backup = session.id()
        )
    })
}

pub(crate) fn restore_preflight(
    payload: &serde_json::Value,
    context: &ExecutionContext,
    flag: &AtomicBool,
) -> Result<(), ManagementError> {
    require_root()?;
    match payload.get("type").and_then(serde_json::Value::as_str) {
        Some("timer-state" | "created-timer") => systemd::restore_preflight(payload, context, flag),
        Some("crontab") => cron::restore_preflight(payload, context, flag),
        Some("cron-file") => Ok(()),
        _ => Err(invalid(i18n::tr!("scheduled-task-unsupported-backup"))),
    }
}
