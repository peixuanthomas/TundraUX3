use super::*;
#[test]
fn historical_admin_credentials_and_lock_state_are_not_identity() {
    let mut record: UserRecord = serde_json_record();
    let mut saved = record.clone();
    saved.role = "Admin".into();
    saved.password_hash = "secret".into();
    saved.password_hint = Some("secret hint".into());
    saved.enabled = false;
    saved.failed_login_attempts = 12;
    saved.locked_until_epoch_ms = Some(u64::MAX);
    saved.personalization_pending = false;
    attach_preferences(&mut record, &[saved]);
    assert_eq!(record.role, "User");
    assert!(record.password_hash.is_empty());
    assert!(record.password_hint.is_none());
    assert!(record.enabled);
    assert_eq!(record.failed_login_attempts, 0);
    assert_eq!(record.locked_until_epoch_ms, None);
    assert!(!record.personalization_pending);
}
fn serde_json_record() -> UserRecord {
    UserRecord {
        id: "linux-uid-42".into(),
        username: "user".into(),
        display_name: "user".into(),
        role: "User".into(),
        password_hash: String::new(),
        password_hint: None,
        appearance: Default::default(),
        personalization_pending: true,
        system_status_dashboard: Default::default(),
        enabled: true,
        failed_login_attempts: 0,
        locked_until_epoch_ms: None,
        created_at_epoch_ms: 0,
        updated_at_epoch_ms: 0,
        last_login_at_epoch_ms: None,
    }
}
