use super::*;

fn app_paths(root: &std::path::Path) -> AppPaths {
    AppPaths::from_parts(
        root.join("config.toml"),
        root.join("state"),
        root.join("cache"),
        root.join("logs"),
        root.join("temp"),
    )
    .unwrap()
}

#[test]
fn recovery_round_trip_is_scoped_by_user() {
    let root = std::env::temp_dir().join(format!(
        "tundra-editor-recovery-{}-{}",
        std::process::id(),
        unix_millis()
    ));
    fs::create_dir_all(&root).unwrap();
    let root = fs::canonicalize(root).unwrap();
    let paths = app_paths(&root);
    let mut record = EditorRecoveryRecord::new("# recovered\n");
    record.cursor = 4;
    record.path = Some(PathBuf::from("C:/notes/example.md"));
    write_editor_recovery(&paths, "alice", &record).unwrap();
    assert_eq!(read_editor_recovery(&paths, "alice").unwrap(), Some(record));
    assert_eq!(read_editor_recovery(&paths, "bob").unwrap(), None);
    clear_editor_recovery(&paths, "alice").unwrap();
    assert_eq!(read_editor_recovery(&paths, "alice").unwrap(), None);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn schema_two_round_trips_a_rich_document_and_keeps_schema_one_readable() {
    let root = std::env::temp_dir().join(format!(
        "tundra-editor-recovery-v2-{}-{}",
        std::process::id(),
        unix_millis()
    ));
    fs::create_dir_all(&root).unwrap();
    let root = fs::canonicalize(root).unwrap();
    let paths = app_paths(&root);
    let mut record = EditorRecoveryRecordV2::rich(RichDocument::new(), "# fallback\n");
    record.path = Some(PathBuf::from("notes/example.md"));
    record.metadata.utf8_bom = true;
    record.saved_content_hash = Some(42);

    write_editor_recovery_v2(&paths, "alice", &record).unwrap();
    assert_eq!(
        read_versioned_editor_recovery(&paths, "alice").unwrap(),
        Some(VersionedEditorRecovery::V2(record))
    );

    let legacy = EditorRecoveryRecord::new("legacy source");
    write_editor_recovery(&paths, "alice", &legacy).unwrap();
    assert_eq!(
        read_versioned_editor_recovery(&paths, "alice").unwrap(),
        Some(VersionedEditorRecovery::V1(legacy))
    );

    clear_editor_recovery(&paths, "alice").unwrap();
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn damaged_rich_payload_recovers_the_private_markdown_fallback_as_source() {
    let root = std::env::temp_dir().join(format!(
        "tundra-editor-recovery-fallback-{}-{}",
        std::process::id(),
        unix_millis()
    ));
    fs::create_dir_all(&root).unwrap();
    let root = fs::canonicalize(root).unwrap();
    let paths = app_paths(&root);
    let record = EditorRecoveryRecordV2::rich(RichDocument::new(), "# safe fallback\n");
    let mut value = serde_json::to_value(record).unwrap();
    value["schema"] = Value::from(2);
    value["payload"]["document"]["blocks"] = Value::String("corrupt".to_string());
    let bytes = serde_json::to_vec_pretty(&value).unwrap();
    atomic_write_document(&editor_recovery_path(&paths, "alice"), &bytes).unwrap();

    let Some(VersionedEditorRecovery::V2Fallback { record, warning }) =
        read_versioned_editor_recovery(&paths, "alice").unwrap()
    else {
        panic!("expected Source fallback")
    };
    assert!(warning.contains("Source mode"));
    assert!(matches!(
        record.payload,
        EditorRecoveryPayload::Source { ref text, .. } if text == "# safe fallback\n"
    ));

    clear_editor_recovery(&paths, "alice").unwrap();
    fs::remove_dir_all(root).unwrap();
}
