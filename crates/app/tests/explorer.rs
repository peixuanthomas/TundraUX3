use std::fs;
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use app::explorer::{
    ExplorerCommand, ExplorerConflictAction, ExplorerController, ExplorerSelectionMode,
    ExplorerSortDirection, ExplorerSortField, ExplorerState,
};
use identity::{AuthSession, UserRole};
use platform::mock::{MockCall, MockPlatform};
use platform::{
    FileAttributes, Platform, PlatformError, PlatformKind, UserDirs, build_windows_app_paths,
    cleanup_temp_path,
};
use storage::StorageManager;

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
fn file_operations_create_rename_copy_cut_paste_and_trash() {
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
        ExplorerCommand::DeleteToTrash,
        Some(&actor),
        &fixture.platform,
        &storage,
    );
    assert!(state.pending_dialog.is_some());
    controller.apply(
        &mut state,
        ExplorerCommand::ConfirmDelete,
        Some(&actor),
        &fixture.platform,
        &storage,
    );
    // MockPlatform records the native system call without touching the host's real Recycle Bin.
    assert!(fixture.documents.join("renamed.txt").exists());
    assert!(fixture.platform.calls().iter().any(|call| {
        matches!(
            call,
            MockCall::MoveToTrash(paths)
                if paths == &vec![fixture.documents.join("renamed.txt")]
        )
    }));
    assert_eq!(
        storage.load_trash().expect("trash manifest").records.len(),
        0,
        "system Trash operations must never write the legacy private manifest"
    );
}

#[test]
fn guest_permissions_are_denied_without_mutating_files() {
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
    assert!(
        !fixture
            .platform
            .calls()
            .iter()
            .any(|call| matches!(call, MockCall::OpenPath(_)))
    );
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
    assert!(
        !fixture
            .platform
            .calls()
            .iter()
            .any(|call| matches!(call, MockCall::OpenPath(_)))
    );
}

#[test]
fn name_sort_is_natural_and_folders_remain_first() {
    let fixture = Fixture::new("natural-sort");
    fs::create_dir(fixture.documents.join("folder20")).expect("folder fixture");
    for name in ["file10.txt", "file2.txt", "file1.txt"] {
        fs::write(fixture.documents.join(name), name).expect("file fixture");
    }
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

    assert_eq!(
        state
            .entries
            .iter()
            .map(|entry| entry.name.as_str())
            .collect::<Vec<_>>(),
        vec!["folder20", "file1.txt", "file2.txt", "file10.txt"]
    );

    controller.apply(
        &mut state,
        ExplorerCommand::SetSort(ExplorerSortField::Name),
        Some(&session()),
        &fixture.platform,
        &storage,
    );
    assert_eq!(state.sort_direction, ExplorerSortDirection::Descending);
    assert_eq!(state.entries[0].name, "folder20");
    assert_eq!(state.entries[1].name, "file10.txt");
}

#[test]
fn ctrl_and_shift_style_selection_drives_batch_clipboard() {
    let fixture = Fixture::new("multi-select");
    for name in ["a.txt", "b.txt", "c.txt"] {
        fs::write(fixture.documents.join(name), name).expect("file fixture");
    }
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

    controller.apply(
        &mut state,
        ExplorerCommand::SelectIndexWithMode(0, ExplorerSelectionMode::Replace),
        Some(&session()),
        &fixture.platform,
        &storage,
    );
    controller.apply(
        &mut state,
        ExplorerCommand::SelectIndexWithMode(2, ExplorerSelectionMode::Range),
        Some(&session()),
        &fixture.platform,
        &storage,
    );
    controller.apply(
        &mut state,
        ExplorerCommand::Copy,
        Some(&session()),
        &fixture.platform,
        &storage,
    );

    assert_eq!(state.selected_paths.len(), 3);
    assert_eq!(
        state
            .clipboard
            .as_ref()
            .map(|clipboard| clipboard.paths.len()),
        Some(3)
    );
}

#[test]
fn implicit_selection_toggle_and_repeated_range_keep_expected_paths() {
    let fixture = Fixture::new("selection-regression");
    for name in ["a.txt", "b.txt", "c.txt"] {
        fs::write(fixture.documents.join(name), name).expect("file fixture");
    }
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

    state.select_index(1, ExplorerSelectionMode::Toggle);
    assert_eq!(state.effective_selected_paths().len(), 2);
    state.select_index(0, ExplorerSelectionMode::Toggle);
    assert_eq!(
        state.effective_selected_paths(),
        vec![fixture.documents.join("b.txt")]
    );
    state.clear_selection();
    assert!(state.effective_selected_paths().is_empty());

    let mut range_state = ExplorerState::new(&fixture.documents, true);
    controller.apply(
        &mut range_state,
        ExplorerCommand::Refresh,
        Some(&session()),
        &fixture.platform,
        &storage,
    );
    range_state.select_index(1, ExplorerSelectionMode::Range);
    range_state.select_index(2, ExplorerSelectionMode::Range);
    assert_eq!(range_state.effective_selected_paths().len(), 3);
    assert_eq!(
        range_state.selection_anchor,
        Some(fixture.documents.join("a.txt"))
    );
}

#[test]
fn type_size_and_modified_sort_keep_unknown_values_last() {
    let fixture = Fixture::new("metadata-sort");
    fs::create_dir(fixture.documents.join("folder")).expect("folder fixture");
    for name in ["alpha.rs", "beta.txt", "unknown.bin"] {
        fs::write(fixture.documents.join(name), name).expect("file fixture");
    }
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
    for entry in &mut state.all_entries {
        match entry.name.as_str() {
            "alpha.rs" => {
                entry.size = 10;
                entry.modified = Some(UNIX_EPOCH + Duration::from_secs(20));
            }
            "beta.txt" => {
                entry.size = 2;
                entry.modified = Some(UNIX_EPOCH + Duration::from_secs(10));
            }
            "unknown.bin" => {
                entry.kind = app::explorer::ExplorerEntryKind::Other;
                entry.type_label = "Other".to_string();
                entry.modified = None;
            }
            _ => {}
        }
    }
    state.apply_projection();

    controller.apply(
        &mut state,
        ExplorerCommand::SetSort(ExplorerSortField::Type),
        Some(&session()),
        &fixture.platform,
        &storage,
    );
    assert_eq!(
        entry_names(&state),
        vec!["folder", "unknown.bin", "alpha.rs", "beta.txt"]
    );

    controller.apply(
        &mut state,
        ExplorerCommand::SetSort(ExplorerSortField::Size),
        Some(&session()),
        &fixture.platform,
        &storage,
    );
    assert_eq!(
        entry_names(&state),
        vec!["folder", "beta.txt", "alpha.rs", "unknown.bin"]
    );

    controller.apply(
        &mut state,
        ExplorerCommand::SetSort(ExplorerSortField::Modified),
        Some(&session()),
        &fixture.platform,
        &storage,
    );
    assert_eq!(state.sort_direction, ExplorerSortDirection::Descending);
    assert_eq!(
        entry_names(&state),
        vec!["folder", "alpha.rs", "beta.txt", "unknown.bin"]
    );
    controller.apply(
        &mut state,
        ExplorerCommand::SetSort(ExplorerSortField::Modified),
        Some(&session()),
        &fixture.platform,
        &storage,
    );
    assert_eq!(
        entry_names(&state),
        vec!["folder", "beta.txt", "alpha.rs", "unknown.bin"]
    );
}

#[test]
fn unreadable_directory_refresh_clears_stale_actionable_rows() {
    let fixture = Fixture::new("unreadable-refresh");
    fs::write(fixture.documents.join("old.txt"), "old").expect("file fixture");
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
    assert_eq!(state.entries.len(), 1);
    fixture.platform.set_directory_error(
        fixture.documents.clone(),
        PlatformError::Io {
            operation: "read directory",
            path: Some(fixture.documents.clone()),
            message: "access denied".to_string(),
        },
    );

    controller.apply(
        &mut state,
        ExplorerCommand::Refresh,
        Some(&session()),
        &fixture.platform,
        &storage,
    );
    assert!(
        state
            .error
            .as_deref()
            .is_some_and(|error| error.contains("access denied"))
    );
    assert!(state.entries.is_empty());
    assert!(state.effective_selected_paths().is_empty());
}

#[test]
fn same_directory_copy_uses_conflict_resolution_and_keep_both_name() {
    let fixture = Fixture::new("keep-both");
    fs::write(fixture.documents.join("note.txt"), "original").expect("file fixture");
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
    select_name(&mut state, "note.txt");
    controller.apply(
        &mut state,
        ExplorerCommand::Copy,
        Some(&session()),
        &fixture.platform,
        &storage,
    );
    controller.apply(
        &mut state,
        ExplorerCommand::Paste,
        Some(&session()),
        &fixture.platform,
        &storage,
    );
    assert!(state.pending_conflict.is_some());

    controller.apply(
        &mut state,
        ExplorerCommand::ResolveConflict {
            action: ExplorerConflictAction::KeepBoth,
            apply_to_all: false,
        },
        Some(&session()),
        &fixture.platform,
        &storage,
    );

    assert!(fixture.documents.join("note (2).txt").is_file());
    assert!(state.pending_conflict.is_none());
}

fn select_name(state: &mut ExplorerState, name: &str) {
    let index = state
        .entries
        .iter()
        .position(|entry| entry.name == name)
        .unwrap_or_else(|| panic!("missing entry {name}; entries: {:?}", state.entries));
    state.select_index(index, app::explorer::ExplorerSelectionMode::Replace);
}

fn entry_names(state: &ExplorerState) -> Vec<&str> {
    state
        .entries
        .iter()
        .map(|entry| entry.name.as_str())
        .collect()
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
    let path =
        std::env::temp_dir().join(format!("tundra-app-{name}-{millis}-{}", std::process::id()));
    fs::create_dir_all(&path).expect("temp root");
    path
}

fn unix_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}
