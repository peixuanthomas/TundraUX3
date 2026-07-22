use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use platform::AppPaths;
use storage::{
    SCHEMA_VERSION, StorageDocumentKind, StorageDocumentStatus, StorageError, StorageLayout,
    StorageManager,
};

#[test]
fn missing_document_is_reported_and_created_with_its_default() {
    let fixture = Fixture::new("missing");
    let (manager, layout) = fixture.open();
    fs::remove_dir_all(&layout.trash_path).expect("trash directory should be removable");

    let checks = manager.check_documents().expect("checks should complete");
    let check = check_for(&checks, StorageDocumentKind::TrashManifest);
    assert_eq!(check.path, layout.trash_manifest_path);
    assert_eq!(check.status, StorageDocumentStatus::Missing);
    assert!(
        !layout.trash_path.exists(),
        "checking must not create directories"
    );

    let report = manager
        .repair_document(StorageDocumentKind::TrashManifest)
        .expect("missing trash manifest should be repairable");
    assert!(report.created);
    assert!(!report.rebuilt);
    assert_eq!(report.backup_path, None);
    assert!(layout.trash_manifest_path.is_file());
    assert!(
        manager
            .load_trash()
            .expect("default trash should load")
            .records
            .is_empty()
    );
}

#[test]
fn corrupt_document_is_backed_up_and_rebuilt() {
    let fixture = Fixture::new("corrupt");
    let (manager, layout) = fixture.open();
    let corrupt_contents = b"{ not-json";
    fs::write(&layout.sessions_path, corrupt_contents).expect("corrupt fixture should be written");

    let checks = manager.check_documents().expect("checks should complete");
    assert_eq!(
        check_for(&checks, StorageDocumentKind::Sessions).status,
        StorageDocumentStatus::Corrupt
    );
    assert_eq!(
        fs::read(&layout.sessions_path).expect("fixture should remain readable"),
        corrupt_contents,
        "checking must not modify corrupt documents"
    );

    let report = manager
        .repair_document(StorageDocumentKind::Sessions)
        .expect("corrupt sessions should be repairable");
    assert!(!report.created);
    assert!(report.rebuilt);
    let backup_path = report.backup_path.expect("repair should report its backup");
    assert_eq!(
        fs::read(&backup_path).expect("backup should be readable"),
        corrupt_contents
    );
    assert!(
        backup_path
            .file_name()
            .expect("backup file name")
            .to_string_lossy()
            .contains(".corrupt.")
    );
    assert!(
        manager
            .load_sessions()
            .expect("rebuilt sessions should load")
            .sessions
            .is_empty()
    );
}

#[test]
fn unsupported_schema_is_reported_and_repair_refuses_without_mutation() {
    let fixture = Fixture::new("unsupported");
    let (manager, layout) = fixture.open();
    let original = format!(
        "{{\n  \"schema_version\": {},\n  \"values\": {{}}\n}}\n",
        SCHEMA_VERSION + 1
    );
    fs::write(&layout.state_path, &original).expect("future schema fixture should be written");

    let checks = manager.check_documents().expect("checks should complete");
    assert_eq!(
        check_for(&checks, StorageDocumentKind::State).status,
        StorageDocumentStatus::UnsupportedSchema
    );

    let error = manager
        .repair_document(StorageDocumentKind::State)
        .expect_err("future schemas must not be repaired");
    assert!(matches!(
        error,
        StorageError::UnsupportedSchema {
            document: "state",
            found,
            supported: SCHEMA_VERSION,
            ..
        } if found == SCHEMA_VERSION + 1
    ));
    assert_eq!(
        fs::read_to_string(&layout.state_path).expect("future document should remain readable"),
        original
    );
    assert_no_corrupt_backups(&layout.data_path, "state.v1.json");
}

#[test]
fn healthy_document_repair_is_a_no_op() {
    let fixture = Fixture::new("healthy");
    let (manager, layout) = fixture.open();
    let original = fs::read(&layout.config_path).expect("config should be readable");

    let checks = manager.check_documents().expect("checks should complete");
    assert_eq!(checks.len(), 7);
    assert!(
        checks
            .iter()
            .all(|check| check.status == StorageDocumentStatus::Healthy)
    );

    let report = manager
        .repair_document(StorageDocumentKind::Config)
        .expect("healthy repair should succeed");
    assert!(!report.created);
    assert!(!report.rebuilt);
    assert_eq!(report.backup_path, None);
    assert_eq!(
        fs::read(&layout.config_path).expect("config should remain readable"),
        original
    );
    assert_no_corrupt_backups(
        layout.config_path.parent().expect("config parent"),
        "config.toml",
    );
}

fn check_for(
    checks: &[storage::StorageDocumentCheck],
    kind: StorageDocumentKind,
) -> &storage::StorageDocumentCheck {
    checks
        .iter()
        .find(|check| check.kind == kind)
        .expect("document check should exist")
}

fn assert_no_corrupt_backups(parent: &Path, file_name: &str) {
    let prefix = format!("{file_name}.corrupt.");
    let has_backup = fs::read_dir(parent)
        .expect("parent should be readable")
        .filter_map(Result::ok)
        .any(|entry| entry.file_name().to_string_lossy().starts_with(&prefix));
    assert!(!has_backup, "repair should not have created a backup");
}

struct Fixture {
    root: PathBuf,
}

impl Fixture {
    fn new(case: &str) -> Self {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should be after the epoch")
            .as_nanos();
        Self {
            root: std::env::temp_dir().join(format!(
                "tundra-storage-document-health-{}-{nanos}-{case}",
                std::process::id()
            )),
        }
    }

    fn open(&self) -> (StorageManager, StorageLayout) {
        let paths = AppPaths::from_parts(
            self.root.join("config").join("config.toml"),
            self.root.join("data"),
            self.root.join("cache"),
            self.root.join("logs"),
            self.root.join("temp"),
        )
        .expect("fixture paths should be valid");
        let layout = StorageLayout::from_app_paths(&paths);
        let manager = StorageManager::open(paths)
            .expect("fixture storage should open")
            .manager;
        (manager, layout)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}
