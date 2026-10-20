use super::*;
use platform::management::ManagementField;
use std::collections::BTreeMap;

fn settings() -> SystemTimeSettings {
    SystemTimeSettings {
        snapshot: Some(ManagementSnapshot {
            rows: vec![ManagementRow {
                id: "ntp".into(),
                identity: BTreeMap::from([("automatic_time".into(), "true".into())]),
                actions: vec![ManagementAction {
                    id: "set_system_ntp_servers".into(),
                    fields: vec![ManagementField {
                        id: "source_0".into(),
                        label: "pool".into(),
                        value: "pool.example".into(),
                        ..Default::default()
                    }],
                    values: BTreeMap::from([
                        ("unit".into(), "chrony.service".into()),
                        ("versions".into(), "configuration-version".into()),
                    ]),
                    ..Default::default()
                }],
                ..Default::default()
            }],
            ..Default::default()
        }),
        ..Default::default()
    }
}

#[test]
fn system_time_commands_keep_native_service_and_conflict_checks() {
    let settings = settings();
    let command = settings.command("set_system_ntp_servers").unwrap();
    assert_eq!(command.target.as_deref(), Some("ntp"));
    assert_eq!(command.identity["automatic_time"], "true");
    assert_eq!(command.values["source_0"], "pool.example");
    assert_eq!(command.values["unit"], "chrony.service");
    assert_eq!(command.values["versions"], "configuration-version");
}

#[test]
fn unavailable_and_running_system_time_actions_cannot_submit() {
    let mut settings = settings();
    settings.job = Some(TimeJob::new(None));
    assert!(settings.command("set_system_ntp_servers").is_none());
    settings.job = None;
    settings.snapshot.as_mut().unwrap().rows[0].actions[0].disabled_reason =
        Some("Unavailable".into());
    assert!(settings.command("set_system_ntp_servers").is_none());
}

#[test]
fn linux_defaults_to_system_time_without_saved_configuration() {
    let startup = ShellStartupState::clean(
        PlatformKind::Linux,
        PlatformCapabilities::native_supported(),
    );
    let config = crate::session::runtime::system_services_config_for_startup(&startup);
    assert_eq!(
        config.time_sync_mode,
        system_services::TimeSyncMode::OperatingSystem
    );
    assert!(config.time_server_url.is_none());
}
