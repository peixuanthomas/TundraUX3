use super::*;

struct NoInput;
impl OperationInteraction for NoInput {
    fn emit(&mut self, _: OperationEvent) {}
    fn ask(&mut self, _: &str, _: &str, _: &[String], _: bool) -> Result<String, ManagementError> {
        Err(ManagementError::Cancelled)
    }
}

fn query() -> ManagementQuery {
    let mut query = ManagementQuery::new(ManagementKind::Backups);
    query.options.insert("page_size".into(), "50".into());
    query
}

fn record(root: &Path, source: &str) -> BackupSession {
    let record = begin_at(root, source, &[]).unwrap();
    record.finish("saved").unwrap();
    record
}

fn command(
    action: &str,
    target: Option<&str>,
    values: BTreeMap<String, String>,
) -> ManagementCommand {
    ManagementCommand {
        kind: ManagementKind::Backups,
        action: action.into(),
        target: target.map(String::from),
        values,
        identity: BTreeMap::new(),
    }
}

fn preview(root: &Path, mut query: ManagementQuery) -> ManagementCommand {
    query.scope = "cleanup-preview".into();
    let snapshot = query_at(root, &query, &AtomicBool::new(false)).unwrap();
    let action = snapshot
        .actions
        .iter()
        .find(|action| action.id == "cleanup_backups")
        .unwrap();
    command(&action.id, None, action.values.clone())
}

#[test]
fn pagination_filters_and_lazy_details_do_not_read_every_record_again() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let ids = (0..6)
        .map(|index| {
            record(root, if index < 3 { "ssh" } else { "test" })
                .id()
                .to_owned()
        })
        .collect::<Vec<_>>();
    let mut query = query();
    query.options.insert("page_size".into(), "2".into());
    let first = query_at(root, &query, &AtomicBool::new(false)).unwrap();
    assert_eq!(first.rows.len(), 2);
    assert!(first.rows.iter().all(|row| row.detail.is_empty()));
    assert!(!first.details_complete);
    let next = first
        .actions
        .iter()
        .find(|action| action.id == "backups_next_page")
        .unwrap();
    let next = ManagementQuery::from_inspection(&command(&next.id, None, next.values.clone()));
    let second = query_at(root, &next, &AtomicBool::new(false)).unwrap();
    let mut direct = next.clone();
    direct.target = Some(ids[5].clone());
    assert_eq!(
        query_at(root, &direct, &AtomicBool::new(false))
            .unwrap()
            .rows[0]
            .id,
        ids[5]
    );
    assert!(
        first
            .rows
            .iter()
            .all(|first| second.rows.iter().all(|second| first.id != second.id))
    );
    query.options.insert("source".into(), "ssh".into());
    let filtered = query_at(root, &query, &AtomicBool::new(false)).unwrap();
    assert!(
        filtered
            .rows
            .iter()
            .all(|row| row.identity["source"] == "ssh")
    );
    query.options.remove("source");
    query.options.insert("from_timestamp".into(), "1".into());
    query.options.insert("to_timestamp".into(), "2".into());
    assert!(
        query_at(root, &query, &AtomicBool::new(false))
            .unwrap()
            .rows
            .is_empty()
    );
    query.options.remove("from_timestamp");
    query.options.remove("to_timestamp");
    fs::write(record_path(root, &ids[0]).unwrap(), b"damaged body").unwrap();
    assert_eq!(
        query_at(root, &query, &AtomicBool::new(false))
            .unwrap()
            .rows
            .len(),
        2
    );
    query.target = Some(ids[5].clone());
    let detail = query_at(root, &query, &AtomicBool::new(false)).unwrap();
    assert_eq!(detail.rows.len(), 1);
    assert!(detail.details_complete);
    assert!(!detail.rows[0].detail.is_empty());
    query.target = Some(ids[0].clone());
    assert!(query_at(root, &query, &AtomicBool::new(false)).is_err());
}

#[test]
fn retained_flags_survive_writer_updates_and_rebuilding_the_index() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let session = record(root, "test");
    let stale = read_record(root, session.id()).unwrap();
    query_at(root, &query(), &AtomicBool::new(false)).unwrap();
    execute_at(
        root,
        &command("retain_backup", Some(session.id()), BTreeMap::new()),
        &mut NoInput,
        &AtomicBool::new(false),
    )
    .unwrap();
    write_record(root, &stale).unwrap();
    session
        .set_payload(serde_json::json!({"new":"payload"}))
        .unwrap();
    session.finish("saved again").unwrap();
    assert!(read_record(root, session.id()).unwrap().retained);
    fs::remove_file(root.join("index/entries.jsonl")).unwrap();
    let snapshot = query_at(root, &query(), &AtomicBool::new(false)).unwrap();
    assert_eq!(snapshot.rows[0].identity["retained"], "true");
    let cleanup = preview(root, query());
    execute_at(root, &cleanup, &mut NoInput, &AtomicBool::new(false)).unwrap();
    assert!(record_path(root, session.id()).unwrap().exists());
}

#[test]
fn legacy_rows_filter_and_keep_flags_survive_index_rebuild() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("history");
    fs::create_dir(&root).unwrap();
    let source = directory.path().join("example.conf");
    fs::write(&source, "before").unwrap();
    let stored = system_config::read_stored(&source).unwrap();
    let id = "1700000000000000000-1";
    let path = root.join(format!("{id}.json"));
    fs::write(&path, serde_json::to_vec(&stored).unwrap()).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    let mut query = query();
    query.options.insert("source".into(), "legacy".into());
    let snapshot = query_at(&root, &query, &AtomicBool::new(false)).unwrap();
    assert_eq!(snapshot.rows.len(), 1);
    assert_eq!(snapshot.rows[0].identity["source"], "legacy");
    assert!(snapshot.rows[0].detail.is_empty());
    index::set_legacy_retained(&root, id, true).unwrap();
    assert_eq!(
        query_at(&root, &query, &AtomicBool::new(false))
            .unwrap()
            .rows[0]
            .identity["retained"],
        "true"
    );
    execute_at(
        &root,
        &command("retain_backup", Some(id), BTreeMap::new()),
        &mut NoInput,
        &AtomicBool::new(false),
    )
    .unwrap();
    fs::remove_file(root.join("index/entries.jsonl")).unwrap();
    query.target = Some(id.into());
    let snapshot = query_at(&root, &query, &AtomicBool::new(false)).unwrap();
    assert_eq!(snapshot.rows[0].identity["retained"], "true");
    assert!(
        snapshot.rows[0]
            .actions
            .iter()
            .any(|action| action.id == "restore_legacy")
    );
}

#[test]
fn cleanup_excludes_kept_unfinished_active_and_referenced_records_and_removes_duplicates() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let referenced = record(root, "test");
    let kept = record(root, "test");
    kept.set_payload(serde_json::json!({"restore_record":referenced.id()}))
        .unwrap();
    execute_at(
        root,
        &command("retain_backup", Some(kept.id()), BTreeMap::new()),
        &mut NoInput,
        &AtomicBool::new(false),
    )
    .unwrap();
    let active = record(root, "test");
    let lease = index::restore_lease(root, active.id()).unwrap();
    let pending = begin_at(root, "network", &[]).unwrap();
    let awaiting = record(root, "ssh");
    awaiting
        .finish("awaiting SSH connection confirmation")
        .unwrap();
    let remove = record(root, "test");
    let legacy = root.join(format!("{}.json", remove.id()));
    fs::write(&legacy, "redundant old copy").unwrap();
    fs::set_permissions(&legacy, fs::Permissions::from_mode(0o600)).unwrap();
    let cleanup = preview(root, query());
    assert!(matches!(
        execute_at(root, &cleanup, &mut NoInput, &AtomicBool::new(false)),
        Err(ManagementError::Conflict(_))
    ));
    drop(lease);
    let mut query = query();
    query.filter = remove.id().into();
    let cleanup = preview(root, query);
    execute_at(root, &cleanup, &mut NoInput, &AtomicBool::new(false)).unwrap();
    assert!(!record_path(root, remove.id()).unwrap().exists());
    assert!(!legacy.exists());
    for session in [&referenced, &kept, &active, &pending, &awaiting] {
        assert!(record_path(root, session.id()).unwrap().exists());
    }
}

#[test]
fn cleanup_rejects_changes_after_preview_and_cancel_does_not_delete_records() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let first = record(root, "test");
    let cleanup = preview(root, query());
    let second = record(root, "test");
    assert!(matches!(
        execute_at(root, &cleanup, &mut NoInput, &AtomicBool::new(false)),
        Err(ManagementError::Conflict(_))
    ));
    let cleanup = preview(root, query());
    assert!(matches!(
        execute_at(root, &cleanup, &mut NoInput, &AtomicBool::new(true)),
        Err(ManagementError::Cancelled)
    ));
    fs::write(
        record_path(root, first.id()).unwrap(),
        b"modified after review",
    )
    .unwrap();
    assert!(matches!(
        execute_at(root, &cleanup, &mut NoInput, &AtomicBool::new(false)),
        Err(ManagementError::Conflict(_))
    ));
    assert!(record_path(root, first.id()).unwrap().exists());
    assert!(record_path(root, second.id()).unwrap().exists());
}

#[test]
fn unsafe_history_directories_are_rejected_before_creating_index_files() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("history");
    fs::create_dir(&root).unwrap();
    fs::set_permissions(&root, fs::Permissions::from_mode(0o777)).unwrap();
    assert!(matches!(
        index::lock(&root),
        Err(ManagementError::PermissionDenied(_))
    ));
    assert!(!root.join("index").exists());
}

#[test]
fn global_sorting_uses_timestamp_numbers_and_stable_ids_and_anchor_keeps_the_current_record() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let mut rows = Vec::new();
    for (index, timestamp) in [100, 20, 20, 2, 200, 10, 20].into_iter().enumerate() {
        let session = record(root, if index % 2 == 0 { "B" } else { "a" });
        let mut stored = read_record(root, session.id()).unwrap();
        stored.timestamp = timestamp;
        write_record(root, &stored).unwrap();
        rows.push((timestamp, session.id().to_owned()));
    }
    execute_at(
        root,
        &command("retain_backup", Some(&rows[0].1), BTreeMap::new()),
        &mut NoInput,
        &AtomicBool::new(false),
    )
    .unwrap();
    for (column, descending) in [(0, true), (0, false), (1, false), (1, true)] {
        let mut query = query();
        query.options.insert("page_size".into(), "2".into());
        query
            .options
            .insert("sort_column".into(), column.to_string());
        query
            .options
            .insert("sort_descending".into(), descending.to_string());
        let mut all = Vec::new();
        loop {
            let snapshot = query_at(root, &query, &AtomicBool::new(false)).unwrap();
            all.extend(snapshot.rows.iter().map(|row| row.id.clone()));
            let Some(next) = snapshot
                .actions
                .iter()
                .find(|action| action.id == "backups_next_page")
            else {
                break;
            };
            query = ManagementQuery::from_inspection(&command(&next.id, None, next.values.clone()));
            assert!(all.len() < 10);
        }
        let mut expected = rows.clone();
        expected.sort_by(|left, right| {
            let order = if column == 1 {
                left.0.cmp(&right.0).then_with(|| left.1.cmp(&right.1))
            } else {
                left.1.cmp(&right.1)
            };
            if descending { order.reverse() } else { order }
        });
        assert_eq!(
            all,
            expected.iter().map(|row| row.1.clone()).collect::<Vec<_>>()
        );
    }
    let mut query = query();
    query.options.insert("page_size".into(), "2".into());
    query.options.insert("sort_column".into(), "1".into());
    query
        .options
        .insert("sort_descending".into(), "false".into());
    let anchor = rows.iter().max().unwrap().1.clone();
    query.options.insert("anchor".into(), anchor.clone());
    let anchored = query_at(root, &query, &AtomicBool::new(false)).unwrap();
    assert!(anchored.rows.iter().any(|row| row.id == anchor));
    let first = anchored
        .actions
        .iter()
        .find(|action| action.id == "backups_first_page")
        .unwrap();
    let first = ManagementQuery::from_inspection(&command(&first.id, None, first.values.clone()));
    assert!(!first.options.contains_key("anchor"));
    assert!(!first.options.contains_key("cursor"));
    assert_ne!(
        query_at(root, &first, &AtomicBool::new(false))
            .unwrap()
            .rows[0]
            .id,
        anchor
    );
}

#[test]
#[ignore = "explicit history scale measurement"]
fn scale_measurement_100_1000_10000() {
    for count in [100, 1_000, 10_000] {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        fs::create_dir(root.join("operations")).unwrap();
        let padding = "x".repeat(32 * 1024);
        for index in 0..count {
            let id = format!("{}-1", 1_700_000_000_000_000_000u128 + index as u128);
            let record = Record {
                format_version: 1,
                id: id.clone(),
                source: "test".into(),
                timestamp: 1_700_000_000,
                before: Vec::new(),
                after: Some(Vec::new()),
                result: "saved".into(),
                retained: false,
                payload: serde_json::json!({"padding":padding}),
            };
            let path = record_path(root, &id).unwrap();
            fs::write(&path, serde_json::to_vec(&record).unwrap()).unwrap();
            fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
        }
        let start = std::time::Instant::now();
        let first = query_at(root, &query(), &AtomicBool::new(false)).unwrap();
        let cold = start.elapsed();
        let start = std::time::Instant::now();
        let second = query_at(root, &query(), &AtomicBool::new(false)).unwrap();
        let warm = start.elapsed();
        assert_eq!(first.rows.len(), 50);
        assert_eq!(second.rows.len(), 50);
        assert!(second.rows.iter().all(|row| row.detail.is_empty()));
        let cleanup = preview(root, query());
        assert!(serde_json::to_vec(&cleanup).unwrap().len() < 4096);
        println!(
            "backup history count={count}, body=32KiB, cold={cold:?}, indexed={warm:?}, rows=50"
        );
    }
}
