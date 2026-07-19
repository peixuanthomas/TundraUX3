use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use tundra_apps::launcher::{
    LauncherAddOutcome, LauncherCommand, LauncherController, LauncherEffect, LauncherItemStatus,
    LauncherState,
};
use tundra_core::{AuthSession, UserRole};
use tundra_platform::mock::MockPlatform;
use tundra_platform::{
    ExecutableKind, FileAttributes, FileOpenPolicy, Platform, PlatformKind, UserDirs,
    build_windows_app_paths, cleanup_temp_path,
};
use tundra_storage::StorageManager;

#[test]
fn admin_batch_adds_only_approved_launcher_targets_and_saves_once() {
    let fixture = Fixture::new("admin-add");
    let executable = fixture.file("program.exe", b"program");
    let document = fixture.file("notes.txt", b"notes");
    fixture.approve(&executable, ExecutableKind::NativeBinary);
    fixture
        .platform
        .set_file_open_policy(document.clone(), FileOpenPolicy::system_default());
    let storage = fixture.storage();
    let controller = LauncherController::default();
    let mut state = LauncherState::default();

    let effect = controller.apply(
        &mut state,
        LauncherCommand::AddPaths(vec![
            executable.clone(),
            document.clone(),
            executable.clone(),
        ]),
        Some(&admin()),
        &fixture.platform,
        &storage,
    );

    let LauncherEffect::Added(results) = effect else {
        panic!("expected batch result");
    };
    assert!(matches!(
        results[0].outcome,
        LauncherAddOutcome::Added { .. }
    ));
    assert!(matches!(
        results[1].outcome,
        LauncherAddOutcome::Rejected { .. }
    ));
    assert_eq!(results[2].outcome, LauncherAddOutcome::Duplicate);
    let config = storage.load_config().expect("saved config");
    assert_eq!(config.launcher.entries.len(), 1);
    assert_eq!(state.items[0].status, LauncherItemStatus::Ready);
}

#[test]
fn user_can_launch_ready_entry_but_guest_cannot_manage_it() {
    let fixture = Fixture::new("user-launch");
    let executable = fixture.file("program.exe", b"program");
    fixture.approve(&executable, ExecutableKind::NativeBinary);
    let storage = fixture.storage();
    let controller = LauncherController::default();
    let mut state = LauncherState::default();
    controller.apply(
        &mut state,
        LauncherCommand::AddPaths(vec![executable.clone()]),
        Some(&admin()),
        &fixture.platform,
        &storage,
    );
    let id = state.items[0].record.id.clone();
    let approved_path = PathBuf::from(&state.items[0].record.path);

    let effect = controller.apply(
        &mut state,
        LauncherCommand::RequestLaunch(id),
        Some(&user()),
        &fixture.platform,
        &storage,
    );
    assert_eq!(
        effect,
        LauncherEffect::OpenRequested {
            path: approved_path
        }
    );
    let denied = controller.apply(
        &mut state,
        LauncherCommand::Remove(vec!["anything".into()]),
        None,
        &fixture.platform,
        &storage,
    );
    assert_eq!(denied, LauncherEffect::None);
    assert!(
        state
            .error
            .as_deref()
            .is_some_and(|error| error.contains("not_authenticated"))
    );
}

#[test]
fn content_changes_block_launch_and_scripts_require_fresh_confirmation() {
    let fixture = Fixture::new("integrity");
    let binary = fixture.file("program.exe", b"original");
    let script = fixture.file("script.cmd", b"echo hello");
    fixture.approve(&binary, ExecutableKind::NativeBinary);
    fixture.approve(&script, ExecutableKind::Script);
    let storage = fixture.storage();
    let controller = LauncherController::default();
    let mut state = LauncherState::default();
    controller.apply(
        &mut state,
        LauncherCommand::AddPaths(vec![binary.clone(), script.clone()]),
        Some(&admin()),
        &fixture.platform,
        &storage,
    );
    let binary_id = state
        .items
        .iter()
        .find(|item| {
            item.record.executable_kind
                == Some(tundra_storage::LauncherExecutableKind::NativeBinary)
        })
        .expect("binary")
        .record
        .id
        .clone();
    let script_id = state
        .items
        .iter()
        .find(|item| {
            item.record.executable_kind == Some(tundra_storage::LauncherExecutableKind::Script)
        })
        .expect("script")
        .record
        .id
        .clone();

    fs::write(&binary, b"replaced").expect("replace executable");
    assert_eq!(
        controller.apply(
            &mut state,
            LauncherCommand::RequestLaunch(binary_id.clone()),
            Some(&user()),
            &fixture.platform,
            &storage
        ),
        LauncherEffect::None
    );
    assert_eq!(
        state
            .items
            .iter()
            .find(|item| item.record.id == binary_id)
            .expect("item")
            .status,
        LauncherItemStatus::Changed
    );
    assert!(matches!(
        controller.apply(
            &mut state,
            LauncherCommand::RequestLaunch(script_id.clone()),
            Some(&user()),
            &fixture.platform,
            &storage
        ),
        LauncherEffect::ConfirmationRequired { .. }
    ));
    assert_eq!(
        controller.apply(
            &mut state,
            LauncherCommand::ConfirmLaunch(script_id),
            Some(&user()),
            &fixture.platform,
            &storage
        ),
        LauncherEffect::OpenRequested {
            path: PathBuf::from(
                &state
                    .items
                    .iter()
                    .find(|item| item.record.executable_kind
                        == Some(tundra_storage::LauncherExecutableKind::Script))
                    .expect("script")
                    .record
                    .path
            )
        }
    );
}

struct Fixture {
    root: PathBuf,
    documents: PathBuf,
    platform: MockPlatform,
}

impl Fixture {
    fn new(name: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "tundra-launcher-{name}-{}-{}",
            unix_millis(),
            std::process::id()
        ));
        let documents = root.join("Documents");
        fs::create_dir_all(&documents).expect("documents");
        let app_paths =
            build_windows_app_paths(root.join("Roaming"), root.join("Local"), root.join("Temp"))
                .expect("paths");
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
        Self {
            root,
            documents,
            platform: MockPlatform::new(user_dirs, app_paths).with_kind(PlatformKind::Windows),
        }
    }

    fn file(&self, name: &str, bytes: &[u8]) -> PathBuf {
        let path = self.documents.join(name);
        fs::write(&path, bytes).expect("fixture file");
        path
    }

    fn approve(&self, path: &Path, kind: ExecutableKind) {
        self.platform
            .set_file_attributes(path.to_path_buf(), attributes(path));
        self.platform.set_file_open_policy(
            path.to_path_buf(),
            FileOpenPolicy::launcher_required(kind, "test policy"),
        );
    }

    fn storage(&self) -> StorageManager {
        StorageManager::open(self.platform.app_paths().expect("paths"))
            .expect("storage")
            .manager
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = cleanup_temp_path(&self.root);
    }
}

fn attributes(path: &Path) -> FileAttributes {
    FileAttributes {
        path: path.to_path_buf(),
        is_file: true,
        is_dir: false,
        len: fs::metadata(path).expect("metadata").len(),
        readonly: false,
        modified: None,
        hidden: false,
        system: false,
        archive: false,
        symlink: false,
        junction: false,
        reparse_point: false,
        shortcut: false,
    }
}
fn admin() -> AuthSession {
    session(UserRole::Admin)
}
fn user() -> AuthSession {
    session(UserRole::User)
}
fn session(role: UserRole) -> AuthSession {
    AuthSession {
        session_id: "session".into(),
        user_id: "user".into(),
        username: "tester".into(),
        role,
        started_at_epoch_ms: unix_millis(),
    }
}
fn unix_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}
