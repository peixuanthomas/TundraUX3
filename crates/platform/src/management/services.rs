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

#[path = "services_config.rs"]
mod config;
pub use config::prepare_config_draft;

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
            let path = match record.object_path.as_ref() {
                Some(path) => Ok(path.clone()),
                // Load metadata for an installed inactive unit without starting it.
                None => proxy
                    .call::<_, _, OwnedObjectPath>("LoadUnit", &(record.name.as_str(),))
                    .map_err(dbus_error),
            };
            match path {
                Ok(path) => append_details(&connection, &path, &mut row, cancelled)?,
                Err(error) => row.detail.push((
                    "Service details".into(),
                    format!("{error}. Refresh or inspect the unit file"),
                )),
            }
        }
        rows.push(row);
    }
    Ok(ManagementSnapshot {
        columns: [
            "Service",
            "Load",
            "Active",
            "Substate",
            "Startup",
            "Description",
        ]
        .map(String::from)
        .to_vec(),
        rows,
        backend: format!("systemd ({})", scope.id()),
        notices: Vec::new(),
        actions: vec![
            ManagementAction {
                id: "set_view".into(),
                label: "Service scope".into(),
                fields: vec![ManagementField {
                    id: "scope".into(),
                    label: "Service scope".into(),
                    value: scope.id().into(),
                    choices: vec!["system".into(), "user".into()],
                    required: true,
                    ..Default::default()
                }],
                ..Default::default()
            },
            ManagementAction {
                id: "daemon_reload".into(),
                label: "Reload service definitions".into(),
                privileged: scope == Scope::System,
                confirm: true,
                values: BTreeMap::from([("scope".into(), scope.id().into())]),
                group: "configuration".into(),
                ..Default::default()
            },
            config::create_action(scope),
        ],
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
        ("reload", "Reload service configuration"),
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
            primary: matches!(id, "start" | "stop"),
            group: if matches!(id, "enable" | "disable") {
                "startup"
            } else {
                "service"
            }
            .into(),
            values: if id == "view_logs" {
                BTreeMap::from([
                    ("service".into(), record.name.clone()),
                    ("scope".into(), scope.id().into()),
                ])
            } else {
                BTreeMap::new()
            },
            ..Default::default()
        });
    }
    if template {
        actions.push(config::instance_action(scope));
    }
    actions.push(config::edit_action(&record.name, scope));
    actions.push(ManagementAction {
        id: "show_dependencies".into(),
        label: "View dependencies".into(),
        group: "inspect".into(),
        ..Default::default()
    });
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
    cancelled: &AtomicBool,
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
    for (property, label) in [
        ("Requires", "Required services"),
        ("Wants", "Wanted services"),
        ("BindsTo", "Bound services"),
        ("RequiredBy", "Required by"),
        ("WantedBy", "Wanted by"),
        ("After", "Start after"),
        ("Before", "Start before"),
    ] {
        match unit.get_property::<Vec<String>>(property) {
            Ok(values) => row.detail.push((label.into(), values.join(", "))),
            Err(error) => row
                .detail
                .push((label.into(), format!("Unavailable: {error}"))),
        }
    }
    if let Ok(can_reload) = unit.get_property::<bool>("CanReload") {
        if !can_reload {
            if let Some(action) = row.actions.iter_mut().find(|action| action.id == "reload") {
                action.disabled_reason =
                    Some("This service does not support reload; edit it or restart it".into());
            }
        }
    }
    if let Ok(invocation) = unit.get_property::<Vec<u8>>("InvocationID") {
        if invocation.len() == 16 && invocation.iter().any(|byte| *byte != 0) {
            let id = invocation
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>();
            row.identity.insert("invocation_id".into(), id.clone());
            if let Some(action) = row
                .actions
                .iter_mut()
                .find(|action| action.id == "view_logs")
            {
                action.values.insert("invocation_id".into(), id);
            }
        }
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
    row.detail.push(("Result".into(), result.clone()));
    for property in ["ExecMainCode", "ExecMainStatus"] {
        if let Ok(value) = service.get_property::<i32>(property) {
            row.detail.push((property.into(), value.to_string()));
        }
    }
    for property in ["ExecMainStartTimestamp", "ExecMainExitTimestamp"] {
        if let Ok(value) = service.get_property::<u64>(property) {
            row.detail.push((property.into(), value.to_string()));
            if property == "ExecMainStartTimestamp" && value > 0 {
                row.identity.insert("since_usec".into(), value.to_string());
                if let Some(action) = row
                    .actions
                    .iter_mut()
                    .find(|action| action.id == "view_logs")
                {
                    action.values.insert("since_usec".into(), value.to_string());
                }
            }
        }
    }
    if result != "success" {
        row.detail.push((
            "Failure guidance".into(),
            "Review the exit status and logs, then edit configuration or retry".into(),
        ));
        let logs = recent_logs(row, cancelled);
        row.detail.push(("Related logs".into(), logs));
    }
    Ok(())
}

fn recent_logs(row: &ManagementRow, cancelled: &AtomicBool) -> String {
    let mut args = vec![
        "--no-pager".into(),
        "--output=short-iso".into(),
        "--lines=60".into(),
        "--boot=0".into(),
        format!("--unit={}", row.id),
    ];
    if row
        .identity
        .get("scope")
        .is_some_and(|scope| scope == "user")
    {
        args.push("--user".into());
    }
    if let Some(id) = row.identity.get("invocation_id") {
        args.push(format!("_SYSTEMD_INVOCATION_ID={id}"));
    } else if let Some(since) = row
        .identity
        .get("since_usec")
        .and_then(|value| value.parse::<u64>().ok())
    {
        args.push(format!("--since=@{}", since / 1_000_000));
    } else {
        return "No recent invocation is available; open service logs and choose a boot".into();
    }
    match run_read_tool("/usr/bin/journalctl", &args, cancelled) {
        Ok(text) => runtime_log::sanitize_text(&text),
        Err(error) => {
            format!("Cannot read related logs: {error}. Open service logs with permission")
        }
    }
}

pub(crate) fn valid_unit_name(name: &str) -> bool {
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
        "start" | "stop" | "restart" | "reload" | "enable" | "disable" | "daemon_reload"
    ) {
        return Err(ManagementError::InvalidInput(
            "Unknown service operation".into(),
        ));
    }
    let scope = Scope::parse(
        command
            .identity
            .get("scope")
            .or_else(|| command.values.get("scope"))
            .map(String::as_str)
            .unwrap_or(""),
    )?;
    if command.action == "daemon_reload" {
        if command.target.is_some() {
            return Err(ManagementError::InvalidInput(
                "Reloading service definitions does not take a unit".into(),
            ));
        }
        if scope == Scope::User && unsafe { libc::getuid() } != context.actor_uid {
            return Err(ManagementError::PermissionDenied(
                "Run user-service reload as the original user".into(),
            ));
        }
        run_systemctl(scope, "daemon-reload", None, cancelled)?;
        return Ok("Service definitions reloaded; refresh service details".into());
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
    let output = match run_systemctl(scope, &command.action, Some(target), cancelled) {
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
    unit: Option<&str>,
    cancelled: &AtomicBool,
) -> Result<String, ManagementError> {
    let mut command = Command::new("/usr/bin/systemctl");
    command
        .args([scope.argument(), "--no-pager", "--no-ask-password", action])
        .env("LC_ALL", "C")
        .env("SYSTEMD_COLORS", "0")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(unit) = unit {
        command.args(["--", unit]);
    }
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

fn run_read_tool(
    program: &str,
    args: &[String],
    cancelled: &AtomicBool,
) -> Result<String, ManagementError> {
    let mut child = Command::new(program)
        .args(args)
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| ManagementError::Unavailable(error.to_string()))?;
    let result = (|| {
        let mut stdout = child
            .stdout
            .take()
            .ok_or_else(|| ManagementError::Failed("Missing stdout".into()))?;
        let mut stderr = child
            .stderr
            .take()
            .ok_or_else(|| ManagementError::Failed("Missing stderr".into()))?;
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
                if !status.success() {
                    return Err(ManagementError::Failed(
                        String::from_utf8_lossy(&err).into_owned(),
                    ));
                }
                return Ok(String::from_utf8_lossy(&out).into_owned());
            }
            if started.elapsed() > Duration::from_secs(10) {
                return Err(ManagementError::Failed("Log query timed out".into()));
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
