use tundra_storage::{
    CONFIG_DESCRIPTOR, SCHEMA_VERSION, StorageFormat, USERS_SCHEMA_VERSION,
    VERSIONED_JSON_DESCRIPTORS,
};

#[test]
fn config_format_is_toml() {
    assert_eq!(CONFIG_DESCRIPTOR.name, "config");
    assert_eq!(CONFIG_DESCRIPTOR.file_name, "config.toml");
    assert_eq!(CONFIG_DESCRIPTOR.format, StorageFormat::Toml);
    assert_eq!(CONFIG_DESCRIPTOR.schema_version, SCHEMA_VERSION);
}

#[test]
fn stateful_data_uses_versioned_json() {
    let names: Vec<_> = VERSIONED_JSON_DESCRIPTORS
        .iter()
        .map(|descriptor| descriptor.name)
        .collect();

    assert_eq!(names, vec!["users", "state", "recent-files", "sessions"]);
    for descriptor in VERSIONED_JSON_DESCRIPTORS {
        assert_eq!(descriptor.format, StorageFormat::VersionedJson);
        let expected_schema = if descriptor.name == "users" {
            USERS_SCHEMA_VERSION
        } else {
            SCHEMA_VERSION
        };
        assert_eq!(descriptor.schema_version, expected_schema);
        assert!(
            descriptor.file_name.ends_with(".json"),
            "{} should be a JSON file",
            descriptor.name
        );
    }
}

#[test]
fn schema_version_starts_at_one() {
    assert_eq!(SCHEMA_VERSION, 1);
}
