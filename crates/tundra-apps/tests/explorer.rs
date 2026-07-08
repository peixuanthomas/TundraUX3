use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use tundra_apps::explorer::{ExplorerCommand, ExplorerController, ExplorerState};
use tundra_core::{AuthSession, UserRole};
use tundra_platform::mock::{MockCall, MockPlatform};
use tundra_platform::{
    FileAttributes, Platform, PlatformKind, UserDirs, build_windows_app_paths, cleanup_temp_path,
};
use tundra_storage::StorageManager;

#[test]
fn refresh_lists_directory_entries_and_filters_hidden_files() {
    let fixture = Fixture::new("refresh-hidden");
    fs::write(fixture.documents.join("visible.txt"), "visible").expect("visible fixture");
    fs::write(fixture.documents.join("secret.txt"), "hidden").expect("hidden fixture");
    fixture.platform.set_file_attributes(
        fixture.documents.join("secret.txt"),
        file_attributes(fixture.documents.join("secret.txt"), true, false),
    );
    let storage = fixture.storage();
    let controller = ExplorerController::default();
    let mut state = ExplorerState::new(&fixture.documents, false);

    controller.apply(
        &mut state,
        ExplorerCommand::Refresh,
        Some(&session()),
        &fixture.platform,
        &storage,
    );

    assert!(state.error.is_none(), "refresh failed: {:?}", state.error);
    assert!(
        state
            .entries
            .iter()
            .any(|entry| entry.name == "visible.txt")
    );
    assert!(!state.entries.iter().any(|entry| entry.name == "secret.txt"));

    controller.apply(
        &mut state,
        ExplorerCommand::ToggleHidden,
        Some(&session()),
        &fixture.platform,
        &storage,
    );

    assert!(state.entries.iter().any(|entry| entry.name == "secret.txt"));
}

#[test]
fn file_operations_create_rename_copy_cut_paste_and_trash_with_audit() {
    let fixture = Fixture::new("file-ops");
    let storage = fixture.storage();
    let controller = ExplorerController::default();
    let actor = session();
    let mut state = ExplorerState::new(&fixture.documents, true);

    controller.apply(
        &mut state,
        ExplorerCommand::NewFolder("folder".to_string()),
        Some(&actor),
        &fixture.platform,
        &storage,
    );
    assert!(fixture.documents.join("folder").is_dir());

    controller.apply(
        &mut state,
        ExplorerCommand::NewTextFile("note.txt".to_string()),
        Some(&actor),
        &fixture.platform,
        &storage,
    );
    assert!(fixture.documents.join("note.txt").is_file());

    select_name(&mut state, "note.txt");
    controller.apply(
        &mut state,
        ExplorerCommand::Rename("renamed.txt".to_string()),
        Some(&actor),
        &fixture.platform,
        &storage,
    );
    assert!(fixture.documents.join("renamed.txt").is_file());

    select_name(&mut state, "renamed.txt");
    controller.apply(
        &mut state,
        ExplorerCommand::Copy,
        Some(&actor),
        &fixture.platform,
        &storage,
    );
    let paste_target = fixture.documents.join("folder");
    state.current_path = paste_target.clone();
    controller.apply(
        &mut state,
        ExplorerCommand::Paste,
        Some(&actor),
        &fixture.platform,
        &storage,
    );
    assert!(paste_target.join("renamed.txt").is_file());

    state.current_path = fixture.documents.clone();
    controller.apply(
        &mut state,
        ExplorerCommand::NewTextFile("move.txt".to_string()),
        Some(&actor),
        &fixture.platform,
        &storage,
    );
    select_name(&mut state, "move.txt");
    controller.apply(
        &mut state,
        ExplorerCommand::Cut,
        Some(&actor),
        &fixture.platform,
        &storage,
    );
    state.current_path = paste_target.clone();
    controller.apply(
        &mut state,
        ExplorerCommand::Paste,
        Some(&actor),
        &fixture.platform,
        &storage,
    );
    assert!(paste_target.join("move.txt").is_file());
    assert!(!fixture.documents.join("move.txt").exists());

    state.current_path = fixture.documents.clone();
    controller.apply(
        &mut state,
        ExplorerCommand::Refresh,
        Some(&actor),
        &fixture.platform,
        &storage,
    );
    select_name(&mut state, "renamed.txt");
    controller.apply(
        &mut state,
        ExplorerCommand::ConfirmDelete,
        Some(&actor),
        &fixture.platform,
        &storage,
    );
    assert!(!fixture.documents.join("renamed.txt").exists());
    assert_eq!(
        storage.load_trash().expect("trash manifest").records.len(),
        1
    );
    let audit = storage.read_audit_lines().expect("audit lines").join("\n");
    assert!(audit.contains("WriteFile"));
    assert!(audit.contains("MoveFile"));
    assert!(audit.contains("DeleteFile"));
}

#[test]
fn guest_permissions_are_denied_and_audited_without_mutating_files() {
    let fixture = Fixture::new("permission-denied");
    let storage = fixture.storage();
    let controller = ExplorerController::default();
    let mut state = ExplorerState::new(&fixture.documents, true);

    controller.apply(
        &mut state,
        ExplorerCommand::NewTextFile("denied.txt".to_string()),
        None,
        &fixture.platform,
        &storage,
    );

    assert!(!fixture.documents.join("denied.txt").exists());
    assert!(
        state
            .error
            .as_deref()
            .unwrap_or_default()
            .contains("not_authenticated")
    );
    assert!(
        storage
            .read_audit_lines()
            .expect("audit")
            .join("\n")
            .contains("Denied")
    );
}

#[test]
fn lnk_files_are_blocked_before_platform_open() {
    let fixture = Fixture::new("blocked-lnk");
    let shortcut = fixture.documents.join("tool.lnk");
    fs::write(&shortcut, "shortcut").expect("shortcut fixture");
    fixture
        .platform
        .set_file_attributes(shortcut.clone(), file_attributes(shortcut, true, true));
    let storage = fixture.storage();
    let controller = ExplorerController::default();
    let mut state = ExplorerState::new(&fixture.documents, true);

    controller.apply(
        &mut state,
        ExplorerCommand::Refresh,
        Some(&session()),
        &fixture.platform,
        &storage,
    );
    select_name(&mut state, "tool.lnk");
    controller.apply(
        &mut state,
        ExplorerCommand::OpenSelected,
        Some(&session()),
        &fixture.platform,
        &storage,
    );

    assert!(
        state
            .error
            .as_deref()
            .unwrap_or_default()
            .contains("blocked")
    );
    assert_eq!(fixture.platform.calls(), Vec::<MockCall>::new());
}

#[test]
fn windows_executables_are_blocked_by_platform_open_policy() {
    let fixture = Fixture::new("blocked-exe");
    let program = fixture.documents.join("tool.exe");
    fs::write(&program, "program").expect("program fixture");
    let storage = fixture.storage();
    let controller = ExplorerController::default();
    let mut state = ExplorerState::new(&fixture.documents, true);

    controller.apply(
        &mut state,
        ExplorerCommand::Refresh,
        Some(&session()),
        &fixture.platform,
        &storage,
    );
    select_name(&mut state, "tool.exe");
    controller.apply(
        &mut state,
        ExplorerCommand::OpenSelected,
        Some(&session()),
        &fixture.platform,
        &storage,
    );

    assert!(
        state
            .error
            .as_deref()
            .unwrap_or_default()
            .contains("Windows")
    );
    assert_eq!(fixture.platform.calls(), Vec::<MockCall>::new());
}

fn select_name(state: &mut ExplorerState, name: &str) {
    state.selected_index = state
        .entries
        .iter()
        .position(|entry| entry.name == name)
        .unwrap_or_else(|| panic!("missing entry {name}; entries: {:?}", state.entries));
}

fn file_attributes(path: PathBuf, hidden: bool, shortcut: bool) -> FileAttributes {
    FileAttributes {
        path,
        is_file: true,
        is_dir: false,
        len: 0,
        readonly: false,
        modified: None,
        hidden,
        system: false,
        archive: false,
        symlink: false,
        junction: false,
        reparse_point: false,
        shortcut,
    }
}

fn session() -> AuthSession {
    AuthSession {
        session_id: "session-1".to_string(),
        user_id: "user-1".to_string(),
        username: "AdminUser".to_string(),
        role: UserRole::Admin,
        started_at_epoch_ms: unix_millis(),
    }
}

struct Fixture {
    root: PathBuf,
    documents: PathBuf,
    platform: MockPlatform,
}

impl Fixture {
    fn new(name: &str) -> Self {
        let root = unique_temp_root(name);
        let documents = root.join("Documents");
        fs::create_dir_all(&documents).expect("documents fixture");
        let app_paths =
            build_windows_app_paths(root.join("Roaming"), root.join("Local"), root.join("Temp"))
                .expect("app paths");
        let user_dirs = UserDirs::new(
            root.join("Desktop"),
            documents.clone(),
            root.join("Downloads"),
            root.join("Pictures"),
            root.join("Videos"),
            root.join("Music"),
            root.join("Roaming"),
        )
        .expect("user dirs");
        let platform = MockPlatform::new(user_dirs, app_paths).with_kind(PlatformKind::Windows);

        Self {
            root,
            documents,
            platform,
        }
    }

    fn storage(&self) -> StorageManager {
        StorageManager::open(self.platform.app_paths().expect("app paths"))
            .expect("storage")
            .manager
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = cleanup_temp_path(&self.root);
    }
}

fn unique_temp_root(name: &str) -> PathBuf {
    let millis = unix_millis();
    let path = std::env::temp_dir().join(format!(
        "tundra-apps-{name}-{millis}-{}",
        std::process::id()
    ));
    fs::create_dir_all(&path).expect("temp root");
    path
}

fn unix_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}
