use super::*;
#[test]
fn grouped_history_keeps_original_properties_results_and_conflicts() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("history");
    let a = dir.path().join("a.conf");
    let b = dir.path().join("b.conf");
    fs::write(&a, "old a\n").unwrap();
    fs::write(&b, "old b\n").unwrap();
    fs::set_permissions(&a, fs::Permissions::from_mode(0o640)).unwrap();
    let session = begin_at(&root, "test", &[a.clone(), b.clone()]).unwrap();
    fs::write(&a, "new a\n").unwrap();
    fs::write(&b, "new b\n").unwrap();
    session.finish("saved").unwrap();
    let record = read_record(&root, session.id()).unwrap();
    assert_eq!(record.before.len(), 2);
    assert_eq!(record.result, "saved");
    assert_eq!(
        fs::metadata(record_path(&root, session.id()).unwrap())
            .unwrap()
            .mode()
            & 0o777,
        0o600
    );
    fs::write(&b, "external\n").unwrap();
    assert!(matches!(
        restore_record(&root, &record, None),
        Err(ManagementError::Conflict(_))
    ));
    assert_eq!(fs::read_to_string(&a).unwrap(), "new a\n");
}
#[test]
fn sensitive_differences_and_backup_ids_are_safe() {
    assert!(!safe_text(Path::new("/etc/shadow"), "alice:$6$secret:1:2:3").contains("secret"));
    assert!(record_path(Path::new("/tmp"), "../a").is_err());
    assert!(record_path(Path::new("/tmp"), "1-2").is_ok());
}
#[test]
fn pending_recovery_accepts_only_original_or_planned_content() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("backups");
    let file = dir.path().join("a.conf");
    fs::write(&file, "old").unwrap();
    let session = begin_at(&root, "ssh", &[file.clone()]).unwrap();
    let record = read_record(&root, session.id()).unwrap();
    let planned = BTreeMap::from([(file.clone(), ("candidate".into(), true))]);
    fs::write(&file, "candidate").unwrap();
    assert!(verify_current(&record, Some(&planned)).is_ok());
    fs::write(&file, "external").unwrap();
    assert!(verify_current(&record, Some(&planned)).is_err());
}
#[test]
fn restore_preserves_properties_removes_created_files_and_can_be_undone() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("backups");
    let old = dir.path().join("old.conf");
    let created = dir.path().join("created.conf");
    fs::write(&old, "original\n").unwrap();
    fs::set_permissions(&old, fs::Permissions::from_mode(0o640)).unwrap();
    let session = begin_at(&root, "test", &[old.clone(), created.clone()]).unwrap();
    fs::write(&old, "changed\n").unwrap();
    fs::set_permissions(&old, fs::Permissions::from_mode(0o600)).unwrap();
    fs::write(&created, "created\n").unwrap();
    session.finish("saved").unwrap();
    restore_record(&root, &read_record(&root, session.id()).unwrap(), None).unwrap();
    assert_eq!(fs::read_to_string(&old).unwrap(), "original\n");
    assert_eq!(fs::metadata(&old).unwrap().mode() & 0o777, 0o640);
    assert!(!created.exists());
    let history = query_at(
        &root,
        &ManagementQuery::new(ManagementKind::Backups),
        &AtomicBool::new(false),
    )
    .unwrap();
    let undo = history
        .rows
        .iter()
        .find(|row| row.cells[2] == "restore")
        .unwrap();
    restore_record(&root, &read_record(&root, &undo.id).unwrap(), None).unwrap();
    assert_eq!(fs::read_to_string(&old).unwrap(), "changed\n");
    assert_eq!(fs::metadata(&old).unwrap().mode() & 0o777, 0o600);
    assert_eq!(fs::read_to_string(&created).unwrap(), "created\n");
    assert_eq!(fs::metadata(&root).unwrap().mode() & 0o777, 0o700);
}
#[test]
fn pending_account_record_keeps_password_original_before_the_write() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("backups");
    let session = begin_at(&root, "users", &[]).unwrap();
    session.set_payload(serde_json::json!({"action":"user_password","target":"alice","changes":[{"database":"shadow","name":"alice","before":["alice","$6$original", "1"],"after":["alice","$6$original","1"],"fields":[1,2],"member":null}]})).unwrap();
    let record = read_record(&root, session.id()).unwrap();
    assert_eq!(record.payload["changes"][0]["before"][1], "$6$original");
    assert!(record.after.is_none());
    assert!(!differences(&record).contains("$6$original"));
}
#[test]
fn native_only_changes_are_visible_and_unconfirmed_operations_cannot_restore() {
    let dir = tempfile::tempdir().unwrap();
    let session = begin_at(dir.path(), "scheduled-tasks", &[]).unwrap();
    session.set_payload(serde_json::json!({"type":"crontab","user":"alice","before":"# keep\n* * * * * /usr/bin/true\n","after":"# keep\n# paused\n"})).unwrap();
    session.finish("completed").unwrap();
    let mut record = read_record(dir.path(), session.id()).unwrap();
    assert!(differences(&record).contains("# keep\n* * * * *"));
    assert_eq!(operation_subject(&record), "alice");
    assert!(can_restore(&record));
    record.result = "awaiting SSH connection confirmation".into();
    assert!(!can_restore(&record));
    record.source = "firewall".into();
    record.payload = serde_json::json!({"backend":"firewalld","changes":[{"zone":"public","scope":"runtime","kind":"port","value":"2222/tcp","before":false,"after":true}]});
    assert!(differences(&record).contains("public / runtime / port 2222/tcp: false -> true"));
}
