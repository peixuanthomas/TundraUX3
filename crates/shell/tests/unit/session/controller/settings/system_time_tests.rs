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

fn settings_state(time: SystemTimeSettings) -> SettingsState {
    SettingsState {
        form_selected: 0,
        form_interaction: Default::default(),
        application: Default::default(),
        system_time: time,
        animation_editor: None,
        number_editor: None,
        category: ui::SettingsCategory::RegionTime,
        selected_field: ui::SettingsField::TimeSyncSource,
        status: String::new().into(),
        scroll_offset: 0,
        picker: None,
        color_editor: None,
        weather_location_editor: None,
        file_extensions_editor: None,
        time_sync_server_editor: None,
        time_sync_validation_request_id: None,
    }
}

#[test]
fn automatic_time_status_distinguishes_loading_from_unavailable_results() {
    let automatic_item = |time: &SystemTimeSettings| {
        let state = settings_state(time.clone());
        super::super::view::settings_cards(
            &state,
            &storage::StorageConfig::default(),
            &storage::AppearanceConfig::default(),
            true,
            "glacier",
            false,
            &[],
            Some(time),
        )
        .into_iter()
        .flat_map(|card| card.items)
        .find(|item| item.field == ui::SettingsField::TimeSyncSource)
        .unwrap()
    };
    let mut time = SystemTimeSettings::default();
    assert_eq!(automatic_item(&time).value, i18n::tr!("management-loading"));

    time.error = Some("system time query failed".into());
    let failed = automatic_item(&time);
    assert_eq!(failed.value, i18n::tr!("settings-device-unavailable"));
    assert!(!failed.enabled);
    assert_eq!(
        failed.unavailable_reason.as_deref(),
        Some("system time query failed")
    );

    time.error = None;
    time.snapshot = Some(ManagementSnapshot::default());
    let unavailable = automatic_item(&time);
    assert_eq!(unavailable.value, i18n::tr!("settings-device-unavailable"));
    assert_eq!(
        unavailable.unavailable_reason,
        Some(i18n::tr!("settings-device-unavailable"))
    );
    time.job = Some(TimeJob::new(None));
    assert_eq!(automatic_item(&time).value, i18n::tr!("management-loading"));
    time.job = None;

    for enabled in [true, false] {
        time.snapshot.as_mut().unwrap().rows = vec![ManagementRow {
            actions: vec![ManagementAction {
                id: "set_system_ntp".into(),
                fields: vec![ManagementField {
                    id: "enabled".into(),
                    value: enabled.to_string(),
                    ..Default::default()
                }],
                ..Default::default()
            }],
            ..Default::default()
        }];
        let item = automatic_item(&time);
        assert_eq!(
            item.value,
            i18n::tr!(if enabled {
                "settings-on"
            } else {
                "settings-off"
            })
        );
        assert!(item.enabled);
    }
}

#[test]
fn failed_system_time_worker_start_preserves_its_error_in_the_settings_state() {
    let mut session = ShellSession::new_for_home_mode(
        ShellLaunchConfig::default(),
        (120, 40),
        ShellHomeMode::User,
    );
    session.pages.settings.settings_state = Some(settings_state(SystemTimeSettings::default()));
    let error = watchdog::WatchdogError::ThreadSpawn(std::io::Error::other("worker unavailable"));
    let expected = error.to_string();
    session.install_system_time_job(TimeJob::new(None), Err(error));
    let time = &session
        .pages
        .settings
        .settings_state
        .as_ref()
        .unwrap()
        .system_time;
    assert!(!time.busy());
    assert_eq!(time.error.as_ref().unwrap().render_current(), expected);
    assert!(time.command("set_system_ntp").is_none());
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
