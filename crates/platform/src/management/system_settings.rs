//! Operating-system settings; Tundra's display and clock preferences stay separate.
use super::*;
use crate::ManagedCommand;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::Ordering;
use zbus::blocking::{Connection, Proxy};
#[cfg(test)]
#[path = "../../tests/unit/management/system_settings_isolated.rs"]
mod isolated;
mod ntp;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SystemSettingsState {
    pub hostname: String,
    pub static_hostname: String,
    pub timezone: String,
    pub automatic_time: bool,
    pub synchronized: bool,
    pub time_utc: String,
}

fn error(error: impl std::fmt::Display) -> ManagementError {
    ManagementError::Failed(error.to_string())
}
fn invalid() -> ManagementError {
    ManagementError::InvalidInput(i18n::tr!("system-settings-invalid"))
}
fn check(flag: &AtomicBool) -> Result<(), ManagementError> {
    if flag.load(Ordering::Acquire) {
        Err(ManagementError::Cancelled)
    } else {
        Ok(())
    }
}
fn proxy<'a>(
    connection: &'a Connection,
    service: &'static str,
    path: &'static str,
    interface: &'static str,
) -> Result<Proxy<'a>, ManagementError> {
    Proxy::new(connection, service, path, interface).map_err(error)
}
fn timedate(connection: &Connection) -> Result<Proxy<'_>, ManagementError> {
    proxy(
        connection,
        "org.freedesktop.timedate1",
        "/org/freedesktop/timedate1",
        "org.freedesktop.timedate1",
    )
}
fn host(connection: &Connection) -> Result<Proxy<'_>, ManagementError> {
    proxy(
        connection,
        "org.freedesktop.hostname1",
        "/org/freedesktop/hostname1",
        "org.freedesktop.hostname1",
    )
}
fn systemd(connection: &Connection) -> Result<Proxy<'_>, ManagementError> {
    proxy(
        connection,
        "org.freedesktop.systemd1",
        "/org/freedesktop/systemd1",
        "org.freedesktop.systemd1.Manager",
    )
}
fn state(connection: &Connection) -> Result<SystemSettingsState, ManagementError> {
    let host = host(connection)?;
    let time = timedate(connection)?;
    Ok(SystemSettingsState {
        hostname: host.get_property("Hostname").map_err(error)?,
        static_hostname: host.get_property("StaticHostname").map_err(error)?,
        timezone: time.get_property("Timezone").map_err(error)?,
        automatic_time: time.get_property("NTP").map_err(error)?,
        synchronized: time.get_property("NTPSynchronized").map_err(error)?,
        time_utc: chrono::Utc::now().to_rfc3339(),
    })
}
fn native(program: &str, args: &[&str], flag: &AtomicBool) -> Result<String, ManagementError> {
    check(flag)?;
    let mut child = Command::new(program)
        .args(args)
        .env_clear()
        .env("PATH", "/usr/sbin:/usr/bin:/sbin:/bin")
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn_managed()
        .map_err(error)?;
    let (status, output, errors) = super::system_config::collect_command(
        &mut child,
        flag,
        std::time::Duration::from_secs(30),
    )?;
    check(flag)?;
    if !status.is_some_and(|status| status.success()) {
        return Err(ManagementError::Unavailable(i18n::tr!(
            "system-settings-tool-failed",
            tool = program,
            reason = if status.is_none() {
                i18n::tr!("system-settings-timeout")
            } else {
                runtime_log::sanitize_text(&String::from_utf8_lossy(&errors))
            }
        )));
    }
    String::from_utf8(output).map_err(error)
}
fn field(id: &str, label: &str, value: String) -> ManagementField {
    ManagementField {
        id: id.into(),
        label: i18n::tr!(label),
        value,
        required: true,
        ..Default::default()
    }
}
fn action(id: &str, label: &str, fields: Vec<ManagementField>) -> ManagementAction {
    ManagementAction {
        id: id.into(),
        label: i18n::tr!(label),
        fields,
        privileged: true,
        primary: true,
        ..Default::default()
    }
}
fn bool_label(value: bool) -> String {
    i18n::tr!(if value {
        "system-settings-yes"
    } else {
        "system-settings-no"
    })
}
pub fn query(
    query: &ManagementQuery,
    flag: &AtomicBool,
) -> Result<ManagementSnapshot, ManagementError> {
    check(flag)?;
    let connection = crate::linux::dbus::system().map_err(error)?;
    let current = state(&connection)?;
    let identity = BTreeMap::from([
        ("hostname".into(), current.static_hostname.clone()),
        ("timezone".into(), current.timezone.clone()),
        ("automatic_time".into(), current.automatic_time.to_string()),
    ]);
    let zones = native(
        "/usr/bin/timedatectl",
        &["--no-pager", "list-timezones"],
        flag,
    );
    let mut zone_field = field(
        "timezone",
        "system-settings-timezone",
        current.timezone.clone(),
    );
    zone_field.control = FieldControl::SearchableChoice;
    zone_field.choices = zones
        .as_ref()
        .map(|text| text.lines().map(String::from).collect())
        .unwrap_or_default();
    let mut zone_action = action(
        "set_system_timezone",
        "system-settings-edit-timezone",
        vec![zone_field],
    );
    zone_action.disabled_reason = zones.err().map(|e| e.to_string());
    let provider = ntp::detect(
        &connection,
        query.options.get("provider").map(String::as_str),
    );
    let mut ntp_actions = Vec::new();
    let mut ntp_details = Vec::new();
    let mut notices = vec![i18n::tr!("system-settings-help")];
    match provider {
        Ok(provider) => {
            ntp_details.push((i18n::tr!("system-settings-provider"), provider.unit.clone()));
            let enabled = provider.active || provider.enabled;
            let mut toggle = action(
                "set_system_ntp",
                "system-settings-edit-automatic",
                vec![ManagementField {
                    id: "enabled".into(),
                    label: i18n::tr!("system-settings-automatic"),
                    value: enabled.to_string(),
                    control: FieldControl::Checkbox,
                    ..Default::default()
                }],
            );
            toggle.values.insert("unit".into(), provider.unit.clone());
            toggle
                .values
                .insert("was_enabled".into(), provider.enabled.to_string());
            toggle
                .values
                .insert("was_active".into(), provider.active.to_string());
            ntp_actions.push(toggle);
            match ntp::configuration(&provider, &connection, flag) {
                Ok(configuration) => {
                    ntp_details.extend(configuration.details());
                    let mut configure = action(
                        "set_system_ntp_servers",
                        "system-settings-edit-servers",
                        configuration.fields(),
                    );
                    configure
                        .values
                        .insert("unit".into(), provider.unit.clone());
                    configure.values.insert(
                        "versions".into(),
                        serde_json::to_string(&configuration.versions()).map_err(error)?,
                    );
                    ntp_actions.push(configure);
                }
                Err(reason) => {
                    ntp_details.push((i18n::tr!("system-settings-status"), reason.to_string()));
                    notices.push(reason.to_string());
                    if matches!(reason, ManagementError::PermissionDenied(_)) {
                        ntp_actions.push(ManagementAction {
                            id: "inspect_system_settings".into(),
                            label: i18n::tr!("system-settings-authorize-read"),
                            values: query.inspection_values(),
                            privileged: true,
                            ..Default::default()
                        });
                    }
                }
            }
        }
        Err(reason) => {
            ntp_details.push((i18n::tr!("system-settings-status"), reason.to_string()));
            notices.push(reason.to_string());
        }
    }
    let local = native("/usr/bin/date", &["--iso-8601=seconds"], flag)
        .map(|time| time.trim().to_string())
        .unwrap_or_else(|_| current.time_utc.clone());
    let mut rows = vec![
        ManagementRow {
            id: "hostname".into(),
            cells: vec![i18n::tr!("system-settings-hostname"), current.hostname],
            detail: vec![(
                i18n::tr!("system-settings-hostname"),
                current.static_hostname.clone(),
            )],
            actions: vec![action(
                "set_system_hostname",
                "system-settings-edit-hostname",
                vec![field(
                    "hostname",
                    "system-settings-hostname",
                    current.static_hostname,
                )],
            )],
            identity: identity.clone(),
        },
        ManagementRow {
            id: "time".into(),
            cells: vec![
                i18n::tr!("system-settings-timezone"),
                current.timezone.clone(),
            ],
            detail: vec![
                (i18n::tr!("system-settings-timezone"), current.timezone),
                (i18n::tr!("system-settings-current-time"), local),
                (
                    i18n::tr!("system-settings-automatic"),
                    bool_label(current.automatic_time),
                ),
                (
                    i18n::tr!("system-settings-synchronized"),
                    bool_label(current.synchronized),
                ),
            ],
            actions: vec![zone_action],
            identity: identity.clone(),
        },
        ManagementRow {
            id: "ntp".into(),
            cells: vec![
                i18n::tr!("system-settings-servers"),
                bool_label(current.synchronized),
            ],
            detail: ntp_details,
            actions: ntp_actions,
            identity,
        },
    ];
    for row in &mut rows {
        row.detail.push((
            i18n::tr!("system-settings-help-label"),
            i18n::tr!("system-settings-help"),
        ));
    }
    Ok(ManagementSnapshot {
        columns: vec![
            i18n::tr!("system-settings-setting"),
            i18n::tr!("system-settings-value"),
        ],
        rows,
        notices,
        details_complete: true,
        backend: "systemd / chrony".into(),
        ..Default::default()
    })
}
fn valid_hostname(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value.split('.').all(|part| {
            !part.is_empty()
                && !part.starts_with('-')
                && !part.ends_with('-')
                && part
                    .bytes()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
        })
}
pub fn execute(
    command: &ManagementCommand,
    _: &ExecutionContext,
    io: &mut dyn OperationInteraction,
    flag: &AtomicBool,
) -> Result<String, ManagementError> {
    check(flag)?;
    if command.action == "inspect_system_settings" {
        let snapshot = query(&ManagementQuery::from_inspection(command), flag)?
            .with_authorized_refresh(command);
        io.emit(OperationEvent::Snapshot { snapshot });
        return Ok(i18n::tr!("system-settings-title"));
    }
    let connection = crate::linux::dbus::system().map_err(error)?;
    let before = state(&connection)?;
    match command.action.as_str() {
        "set_system_hostname" | "set_system_timezone" => {
            let hostname = command.action == "set_system_hostname";
            let key = if hostname { "hostname" } else { "timezone" };
            let value = command.values.get(key).ok_or_else(invalid)?;
            let old = if hostname {
                &before.static_hostname
            } else {
                &before.timezone
            };
            if command.identity.get(key) != Some(old) {
                return Err(ManagementError::Conflict(i18n::tr!(
                    "platform-management-review-again"
                )));
            }
            if hostname && !valid_hostname(value) {
                return Err(ManagementError::InvalidInput(i18n::tr!(
                    "system-settings-hostname-invalid"
                )));
            }
            if !hostname
                && !native(
                    "/usr/bin/timedatectl",
                    &["--no-pager", "list-timezones"],
                    flag,
                )?
                .lines()
                .any(|zone| zone == value)
            {
                return Err(invalid());
            }
            super::configuration::confirm_preview(
                "system-settings-apply",
                &i18n::tr!("system-settings-review"),
                &[format!(
                    "{}: {old} → {value}",
                    i18n::tr!(if hostname {
                        "system-settings-hostname"
                    } else {
                        "system-settings-timezone"
                    })
                )],
                io,
            )?;
            check(flag)?;
            let fresh = state(&connection)?;
            if (if hostname {
                &fresh.static_hostname
            } else {
                &fresh.timezone
            }) != old
            {
                return Err(ManagementError::Conflict(i18n::tr!(
                    "platform-management-review-again"
                )));
            }
            if hostname {
                host(&connection)?
                    .call::<_, _, ()>("SetStaticHostname", &(value, false))
                    .map_err(error)?;
            } else {
                timedate(&connection)?
                    .call::<_, _, ()>("SetTimezone", &(value, false))
                    .map_err(error)?;
            }
            let after = state(&connection)?;
            if (if hostname {
                &after.static_hostname
            } else {
                &after.timezone
            }) != value
            {
                return Err(error(i18n::tr!("system-settings-verify-failed")));
            }
        }
        "set_system_ntp" => ntp::set_enabled(command, &connection, io, flag)?,
        "set_system_ntp_servers" => ntp::save_servers(command, &connection, io, flag)?,
        _ => return Err(invalid()),
    }
    let mut refresh = ManagementQuery::new(ManagementKind::SystemSettings);
    if let Some(unit) = command.values.get("unit") {
        refresh.options.insert("provider".into(), unit.clone());
    }
    io.emit(OperationEvent::Snapshot {
        snapshot: query(&refresh, flag)?,
    });
    Ok(i18n::tr!("system-settings-completed"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hostname_validation_matches_persistent_host_names() {
        for name in ["tundra", "host-1", "tundra.example"] {
            assert!(valid_hostname(name));
        }
        for name in [
            "",
            "UPPER",
            "-host",
            "host-",
            "host..name",
            "host/name",
            "host\nname",
        ] {
            assert!(!valid_hostname(name));
        }
        assert!(!valid_hostname(&"x".repeat(65)));
    }
}
