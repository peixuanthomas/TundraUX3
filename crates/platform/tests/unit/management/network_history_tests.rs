use super::*;
fn plan() -> Plan {
    Plan {
        restore_record: None,
        restore_uuid: String::new(),
        remove_uuid: String::new(),
        defer_confirmation: false,
        operation: "wifi-connect".into(),
        identity: BTreeMap::new(),
        kind: OwnerKind::NetworkManager,
        interface: "wlan0".into(),
        uuid: "old".into(),
        new_uuid: "new".into(),
        files: vec![],
        arguments: vec![],
        preview: String::new(),
        wifi: Some(config::Wifi {
            ssid: "test".into(),
            ssid_bytes: b"test".to_vec(),
            security: "wpa2".into(),
            password: "private-password".into(),
            saved_uuid: "saved".into(),
            ap_path: String::new(),
            ipv4: "auto".into(),
            ipv6: "auto".into(),
            require_ipv4: true,
            require_ipv6: false,
        }),
    }
}
#[test]
fn saved_plan_keeps_native_identity_without_transient_password() {
    let plan = plan();
    let json = serde_json::to_string(&plan).unwrap();
    assert!(!json.contains("private-password"));
    assert_eq!(
        uuids(&plan),
        BTreeSet::from(["old".into(), "new".into(), "saved".into()])
    );
}

#[test]
fn automatic_recovery_record_targets_the_current_saved_connection() {
    let original = plan();
    let reverse = rollback_plan(
        &original,
        "new".into(),
        &BTreeMap::from([
            ("old".into(), Some("before".into())),
            ("new".into(), Some("candidate".into())),
        ]),
    );
    assert_eq!(reverse.uuid, "new");
    assert_eq!(reverse.new_uuid, "");
    assert_eq!(reverse.restore_uuid, "old");
    assert_eq!(reverse.remove_uuid, "new");
    assert!(reverse.wifi.is_none());
    assert_eq!(
        uuids(&reverse),
        BTreeSet::from(["old".into(), "new".into()])
    );
    assert!(
        !serde_json::to_string(&reverse)
            .unwrap()
            .contains("private-password")
    );
}

#[test]
fn undoing_a_restore_records_the_profile_that_automatic_recovery_recreates() {
    let mut original = plan();
    original.operation = "restore".into();
    original.restore_record = Some("source".into());
    original.uuid = "new".into();
    original.new_uuid.clear();
    original.restore_uuid = "old".into();
    original.remove_uuid = "new".into();
    original.wifi = None;
    let reverse = rollback_plan(
        &original,
        "old".into(),
        &BTreeMap::from([("old".into(), Some("saved".into())), ("new".into(), None)]),
    );
    assert_eq!(reverse.uuid, "old");
    assert_eq!(reverse.new_uuid, "new");
    assert!(reverse.restore_record.is_none());
    assert_eq!(
        uuids(&reverse),
        BTreeSet::from(["old".into(), "new".into()])
    );
}

#[test]
fn disconnected_state_remains_disconnected_when_restoring_an_automatic_backup() {
    let reverse = rollback_plan(
        &plan(),
        String::new(),
        &BTreeMap::from([("old".into(), Some("saved".into()))]),
    );
    assert!(reverse.uuid.is_empty());
    assert!(reverse.new_uuid.is_empty());
    assert_eq!(reverse.restore_uuid, "old");
}

#[test]
fn chinese_recovery_explanation_does_not_change_native_identifiers() {
    let language = i18n::LanguageSnapshotData {
        code: "zh-CN".into(),
        generation: 1,
        current: vec![
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../ascii-assets/assets/locales/zh-CN/modules/management-network-history.ftl"
            ))
            .into(),
        ],
        english: Vec::new(),
    }
    .restore()
    .unwrap();
    let _language = i18n::enter_snapshot(std::sync::Arc::new(language));
    let reverse = rollback_plan(&plan(), "new".into(), &BTreeMap::new());
    assert_eq!(reverse.operation, "rollback");
    assert_eq!(reverse.uuid, "new");
    assert!(reverse.preview.contains("120 秒"));
    let reason = i18n::tr!(
        "platform-network-history-an-affected-connection-had-temporary-or-unsaved-settings-before-automatic"
    );
    let payload = serde_json::json!({"restore_disabled_reason":reason});
    let context = ExecutionContext {
        actor_uid: 1000,
        helper_path: PathBuf::new(),
        caller_pid: 0,
        caller_start_ticks: 0,
    };
    assert!(
        restore_preflight(&payload, &context, &AtomicBool::new(false))
            .unwrap_err()
            .to_string()
            .contains("临时")
    );
}
