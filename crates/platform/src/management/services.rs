//! systemd system and current-user .service management.
use super::*;
use std::collections::BTreeMap;
use std::io::{self, Read};
use std::os::fd::{AsRawFd, RawFd};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};
use zbus::blocking::{Connection, Proxy};
use zbus::zvariant::OwnedObjectPath;

type LoadedUnit = (
    String,
    String,
    String,
    String,
    String,
    String,
    OwnedObjectPath,
    u32,
    String,
    OwnedObjectPath,
);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Scope {
    System,
    User,
}
impl Scope {
    fn parse(value: &str) -> Result<Self, ManagementError> {
        match value {
            "" | "system" => Ok(Self::System),
            "user" => Ok(Self::User),
            _ => Err(ManagementError::InvalidInput(
                "Service scope must be system or user".into(),
            )),
        }
    }
    fn id(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::User => "user",
        }
    }
    fn argument(self) -> &'static str {
        match self {
            Self::System => "--system",
            Self::User => "--user",
        }
    }
}

#[derive(Debug, Default)]
struct ServiceRecord {
    name: String,
    description: String,
    load: String,
    active: String,
    sub: String,
    file_state: String,
    fragment: String,
    object_path: Option<OwnedObjectPath>,
}

fn check_cancelled(cancelled: &AtomicBool) -> Result<(), ManagementError> {
    if cancelled.load(Ordering::Relaxed) {
        Err(ManagementError::Cancelled)
    } else {
        Ok(())
    }
}

fn connection(scope: Scope) -> Result<Connection, ManagementError> {
    let result = match scope {
        Scope::System => crate::linux::dbus::system(),
        Scope::User => crate::linux::dbus::session(),
    };
    result.map_err(|error| {
        ManagementError::Unavailable(format!(
            "Cannot connect to the {} service manager: {error}",
            scope.id()
        ))
    })
}

pub fn query(
    query: &ManagementQuery,
    cancelled: &AtomicBool,
) -> Result<ManagementSnapshot, ManagementError> {
    check_cancelled(cancelled)?;
    let scope = Scope::parse(&query.scope)?;
    let connection = connection(scope)?;
    let proxy = Proxy::new(
        &connection,
        "org.freedesktop.systemd1",
        "/org/freedesktop/systemd1",
        "org.freedesktop.systemd1.Manager",
    )
    .map_err(dbus_error)?;
    let loaded: Vec<LoadedUnit> = proxy.call("ListUnits", &()).map_err(dbus_error)?;
    check_cancelled(cancelled)?;
    let files: Vec<(String, String)> = proxy.call("ListUnitFiles", &()).map_err(dbus_error)?;
    check_cancelled(cancelled)?;
    let mut records = merge_units(loaded, files);
    let needle = query.filter.to_lowercase();
    records.retain(|_, record| {
        needle.is_empty()
            || format!(
                "{} {} {} {} {} {}",
                record.name,
                record.description,
                record.load,
                record.active,
                record.sub,
                record.file_state
            )
            .to_lowercase()
            .contains(&needle)
    });
    let mut rows = Vec::with_capacity(records.len());
    for record in records.into_values() {
        check_cancelled(cancelled)?;
        let selected = query.target.as_deref() == Some(record.name.as_str());
        let mut row = service_row(&record, scope);
        if selected {
            if let Some(path) = &record.object_path {
                append_details(&connection, path, &mut row)?;
            }
        }
        rows.push(row);
    }
    Ok(ManagementSnapshot {
        columns: ["Service", "Load", "Active", "Substate", "Startup", "Description"].map(String::from).to_vec(),
        rows, backend: format!("systemd ({})", scope.id()),
        notices: vec!["Starting a service and enabling it at startup are separate operations. Static and masked services do not have a simple startup toggle.".into()],
        actions: vec![ManagementAction { id: "set_view".into(), label: "Choose service scope".into(), fields: vec![ManagementField { id: "scope".into(), label: "Service scope".into(), value: scope.id().into(), choices: vec!["system".into(), "user".into()], required: true, ..Default::default() }], ..Default::default() }],
    })
}

fn merge_units(
    loaded: Vec<LoadedUnit>,
    files: Vec<(String, String)>,
) -> BTreeMap<String, ServiceRecord> {
    let mut records = BTreeMap::new();
    for (path, file_state) in files {
        let Some(name) = Path::new(&path)
            .file_name()
            .and_then(|name| name.to_str())
            .filter(|name| valid_unit_name(name))
        else {
            continue;
        };
        records.entry(name.to_string()).or_insert(ServiceRecord {
            name: name.into(),
            file_state,
            fragment: path,
            load: "unloaded".into(),
            active: "inactive".into(),
            sub: "—".into(),
            ..Default::default()
        });
    }
    for (name, description, load, active, sub, _, path, _, _, _) in loaded {
        if !valid_unit_name(&name) {
            continue;
        }
        let record = records
            .entry(name.clone())
            .or_insert_with(|| ServiceRecord {
                name,
                file_state: "transient/unknown".into(),
                ..Default::default()
            });
        record.description = description;
        record.load = load;
        record.active = active;
        record.sub = sub;
        record.object_path = Some(path);
    }
    records
}

fn service_row(record: &ServiceRecord, scope: Scope) -> ManagementRow {
    let template = record.name.ends_with("@.service");
    let masked = matches!(record.file_state.as_str(), "masked" | "masked-runtime")
        || record.load == "masked";
    let writable_startup = matches!(
        record.file_state.as_str(),
        "enabled" | "enabled-runtime" | "disabled" | "linked" | "linked-runtime" | "indirect"
    );
    let mut actions = Vec::new();
    for (id, label) in [
        ("start", "Start"),
        ("stop", "Stop"),
        ("restart", "Restart"),
        ("enable", "Enable at startup"),
        ("disable", "Disable at startup"),
        ("view_logs", "View service logs"),
    ] {
        let disabled_reason = if id == "view_logs" {
            None
        } else if template {
            Some("A service template needs an instance name; choose an existing instance".into())
        } else if matches!(id, "start" | "restart") && masked {
            Some("This service is masked".into())
        } else if matches!(id, "enable" | "disable") && !writable_startup {
            Some(format!(
                "Startup cannot be toggled for unit-file state {}",
                record.file_state
            ))
        } else {
            None
        };
        actions.push(ManagementAction {
            id: id.into(),
            label: label.into(),
            confirm: id != "view_logs",
            privileged: scope == Scope::System && id != "view_logs",
            disabled_reason,
            ..Default::default()
        });
    }
    ManagementRow {
        id: record.name.clone(),
        cells: vec![
            record.name.clone(),
            record.load.clone(),
            record.active.clone(),
            record.sub.clone(),
            record.file_state.clone(),
            runtime_log::sanitize_text(&record.description),
        ],
        detail: vec![
            ("Scope".into(), scope.id().into()),
            ("Unit file".into(), record.fragment.clone()),
            (
                "Description".into(),
                runtime_log::sanitize_text(&record.description),
            ),
        ],
        actions,
        identity: BTreeMap::from([
            ("unit".into(), record.name.clone()),
            ("scope".into(), scope.id().into()),
        ]),
    }
}

fn append_details(
    connection: &Connection,
    path: &OwnedObjectPath,
    row: &mut ManagementRow,
) -> Result<(), ManagementError> {
    let unit = Proxy::new(
        connection,
        "org.freedesktop.systemd1",
        path.clone(),
        "org.freedesktop.systemd1.Unit",
    )
    .map_err(dbus_error)?;
    for property in [
        "FragmentPath",
        "SourcePath",
        "UnitFileState",
        "LoadState",
        "ActiveState",
        "SubState",
    ] {
        let value: String = unit.get_property(property).map_err(dbus_error)?;
        row.detail
            .push((property.into(), runtime_log::sanitize_text(&value)));
    }
    let service = Proxy::new(
        connection,
        "org.freedesktop.systemd1",
        path.clone(),
        "org.freedesktop.systemd1.Service",
    )
    .map_err(dbus_error)?;
    let pid: u32 = service.get_property("MainPID").map_err(dbus_error)?;
    row.detail.push(("Main PID".into(), pid.to_string()));
    let result: String = service.get_property("Result").map_err(dbus_error)?;
    row.detail.push(("Result".into(), result));
    Ok(())
}

fn valid_unit_name(name: &str) -> bool {
    name.ends_with(".service")
        && name.len() > ".service".len()
        && name.len() <= 255
        && !name.starts_with('-')
        && name.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b':' | b'_' | b'.' | b'@' | b'-' | b'\\')
        })
}

pub fn execute(
    command: &ManagementCommand,
    context: &ExecutionContext,
    interaction: &mut dyn OperationInteraction,
    cancelled: &AtomicBool,
) -> Result<String, ManagementError> {
    check_cancelled(cancelled)?;
    if !matches!(
        command.action.as_str(),
        "start" | "stop" | "restart" | "enable" | "disable"
    ) {
        return Err(ManagementError::InvalidInput(
            "Unknown service operation".into(),
        ));
    }
    let target = command
        .target
        .as_deref()
        .filter(|name| valid_unit_name(name))
        .ok_or_else(|| {
            ManagementError::InvalidInput(
                "Choose an exact .service unit name, not a path or pattern".into(),
            )
        })?;
    if target.ends_with("@.service") {
        return Err(ManagementError::InvalidInput(
            "Choose an existing service instance rather than a template".into(),
        ));
    }
    let scope = Scope::parse(
        command
            .identity
            .get("scope")
            .map(String::as_str)
            .unwrap_or(""),
    )?;
    if command.identity.get("unit").map(String::as_str) != Some(target) {
        return Err(ManagementError::Conflict(
            "The selected service changed; refresh before retrying".into(),
        ));
    }
    let current_uid = unsafe { libc::getuid() };
    if (current_uid != 0 && current_uid != context.actor_uid)
        || (scope == Scope::User && current_uid != context.actor_uid)
    {
        return Err(ManagementError::PermissionDenied(
            "Current-user services must be managed as the original Linux user".into(),
        ));
    }
    // Refresh the selected unit's actual state rather than trusting stale UI
    // booleans. This also rejects unknown unit names and non-toggleable files.
    let snapshot = query(
        &ManagementQuery {
            kind: ManagementKind::Services,
            scope: scope.id().into(),
            filter: target.into(),
            target: None,
            options: BTreeMap::new(),
        },
        cancelled,
    )?;
    let row = snapshot
        .rows
        .iter()
        .find(|row| row.id == target)
        .ok_or_else(|| ManagementError::Conflict("The selected service no longer exists".into()))?;
    if let Some(reason) = row
        .actions
        .iter()
        .find(|action| action.id == command.action)
        .and_then(|action| action.disabled_reason.as_ref())
    {
        return Err(ManagementError::InvalidInput(reason.clone()));
    }
    interaction.emit(OperationEvent::Progress {
        message: format!("{} {} service {target}", command.action, scope.id()),
        percent: None,
    });
    let output = match run_systemctl(scope, &command.action, target, cancelled) {
        Err(ManagementError::Cancelled) => {
            interaction.emit(OperationEvent::Output {
                text: "Stopped waiting for systemctl. The systemd job may still complete; refresh the service state before retrying.".into(),
            });
            return Err(ManagementError::Cancelled);
        }
        result => result?,
    };
    if !output.trim().is_empty() {
        interaction.emit(OperationEvent::Output {
            text: runtime_log::sanitize_text(&output),
        });
    }
    // systemctl normally waits for the job; no --no-block or shell expansion.
    // Enable/disable intentionally do not imply --now.
    Ok(format!(
        "{} completed for {} service {target}",
        command.action,
        scope.id()
    ))
}

fn run_systemctl(
    scope: Scope,
    action: &str,
    unit: &str,
    cancelled: &AtomicBool,
) -> Result<String, ManagementError> {
    let mut command = Command::new("/usr/bin/systemctl");
    command
        .args([
            scope.argument(),
            "--no-pager",
            "--no-ask-password",
            action,
            "--",
            unit,
        ])
        .env("LC_ALL", "C")
        .env("SYSTEMD_COLORS", "0")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().map_err(|error| {
        ManagementError::Unavailable(format!("Cannot launch systemctl: {error}"))
    })?;
    let result = (|| {
        let mut stdout = child
            .stdout
            .take()
            .ok_or_else(|| ManagementError::Failed("systemctl stdout is unavailable".into()))?;
        let mut stderr = child
            .stderr
            .take()
            .ok_or_else(|| ManagementError::Failed("systemctl stderr is unavailable".into()))?;
        nonblocking(stdout.as_raw_fd())?;
        nonblocking(stderr.as_raw_fd())?;
        let mut out = Vec::new();
        let mut err = Vec::new();
        let started = Instant::now();
        loop {
            check_cancelled(cancelled)?;
            drain(&mut stdout, &mut out)?;
            drain(&mut stderr, &mut err)?;
            if let Some(status) = child
                .try_wait()
                .map_err(|error| ManagementError::Failed(error.to_string()))?
            {
                drain(&mut stdout, &mut out)?;
                drain(&mut stderr, &mut err)?;
                let stdout = String::from_utf8_lossy(&out).into_owned();
                let stderr = String::from_utf8_lossy(&err).into_owned();
                if status.success() {
                    return Ok(format!("{stdout}{stderr}"));
                }
                let lower = stderr.to_lowercase();
                let message = format!(
                    "systemctl {action} failed: {}",
                    runtime_log::sanitize_text(stderr.trim())
                );
                return Err(
                    if [
                        "access denied",
                        "permission denied",
                        "authentication is required",
                        "interactive authentication required",
                    ]
                    .iter()
                    .any(|needle| lower.contains(needle))
                    {
                        ManagementError::PermissionDenied(message)
                    } else {
                        ManagementError::Failed(message)
                    },
                );
            }
            if started.elapsed() >= Duration::from_secs(120) {
                return Err(ManagementError::Failed("The service request timed out; refresh its state before retrying because the systemd job may still be running".into()));
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    })();
    if child.try_wait().is_ok_and(|status| status.is_none()) {
        let _ = child.kill();
    }
    let _ = child.wait();
    result
}

fn nonblocking(fd: RawFd) -> Result<(), ManagementError> {
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
        Err(ManagementError::Failed(
            io::Error::last_os_error().to_string(),
        ))
    } else {
        Ok(())
    }
}

fn drain(reader: &mut impl Read, bytes: &mut Vec<u8>) -> Result<(), ManagementError> {
    let mut buffer = [0_u8; 4096];
    loop {
        match reader.read(&mut buffer) {
            Ok(0) => return Ok(()),
            Ok(count) => {
                if bytes.len().saturating_add(count) > 65536 {
                    return Err(ManagementError::Failed("systemctl output exceeded 64 KiB; refresh the service state before retrying".into()));
                }
                bytes.extend_from_slice(&buffer[..count]);
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => return Ok(()),
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(ManagementError::Failed(error.to_string())),
        }
    }
}

fn dbus_error(error: zbus::Error) -> ManagementError {
    match &error {
        zbus::Error::MethodError(name, _, _)
            if matches!(
                name.as_str(),
                "org.freedesktop.DBus.Error.AccessDenied"
                    | "org.freedesktop.PolicyKit1.Error.NotAuthorized"
            ) =>
        {
            ManagementError::PermissionDenied(error.to_string())
        }
        _ => {
            ManagementError::Unavailable(format!("systemd service manager is unavailable: {error}"))
        }
    }
}

#[cfg(test)]
#[path = "../../tests/unit/management/services_tests.rs"]
mod tests;
