use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use app::launcher::{
    LauncherAddOutcome, LauncherCommand, LauncherController, LauncherEffect, LauncherItemStatus,
    LauncherState, verify_launcher_entry,
};
use identity::{AuthSession, UserRole};
use platform::mock::MockPlatform;
use platform::{
    ExecutableKind, FileAttributes, FileOpenPolicy, Platform, PlatformKind, UserDirs,
    build_windows_app_paths, cleanup_temp_path,
};
use storage::{LauncherTerminalSettings, StorageManager};

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
            path: approved_path,
            kind: ExecutableKind::NativeBinary,
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
            .as_ref()
            .map(i18n::LocalizedText::render_current)
            .is_some_and(|error| error.contains("not_authenticated"))
    );
}

#[test]
fn content_changes_allow_launch_and_scripts_require_fresh_confirmation() {
    let fixture = Fixture::new("presence");
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
            item.record.executable_kind == Some(storage::LauncherExecutableKind::NativeBinary)
        })
        .expect("binary")
        .record
        .id
        .clone();
    let script_id = state
        .items
        .iter()
        .find(|item| item.record.executable_kind == Some(storage::LauncherExecutableKind::Script))
        .expect("script")
        .record
        .id
        .clone();

    // Existing configs may still contain the old content digest. It must not
    // affect either the refreshed status or launch after the target changes.
    let mut config = storage.load_config().expect("config");
    config.launcher.entries[0].fingerprint = Some(storage::LauncherFingerprint {
        sha256: "obsolete digest".into(),
        byte_len: 8,
        modified_at_epoch_ms: Some(0),
    });
    storage.save_config(&config).expect("legacy fingerprint");
    state = controller.load(&storage).expect("reload");
    assert!(
        state
            .items
            .iter()
            .all(|item| item.status == LauncherItemStatus::Checking)
    );
    fs::write(&binary, b"updated program with a different length").expect("replace executable");
    fixture
        .platform
        .set_file_open_policy(binary.clone(), FileOpenPolicy::system_default());
    controller.apply(
        &mut state,
        LauncherCommand::Refresh,
        Some(&user()),
        &fixture.platform,
        &storage,
    );
    assert!(
        state
            .items
            .iter()
            .all(|item| item.status == LauncherItemStatus::Ready)
    );
    assert_eq!(
        controller.apply(
            &mut state,
            LauncherCommand::RequestLaunch(binary_id.clone()),
            Some(&user()),
            &fixture.platform,
            &storage
        ),
        LauncherEffect::OpenRequested {
            path: PathBuf::from(&state.items[0].record.path),
            kind: ExecutableKind::NativeBinary,
        }
    );
    assert_eq!(
        state
            .items
            .iter()
            .find(|item| item.record.id == binary_id)
            .expect("item")
            .status,
        LauncherItemStatus::Ready
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
                        == Some(storage::LauncherExecutableKind::Script))
                    .expect("script")
                    .record
                    .path
            ),
            kind: ExecutableKind::Script,
        }
    );
}

#[test]
fn moved_or_deleted_targets_are_missing_and_restored_targets_are_ready() {
    let fixture = Fixture::new("missing-target");
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
    let entry = state.items[0].record.clone();
    let moved = executable.with_file_name("moved.exe");
    fs::rename(&executable, &moved).expect("move target");
    assert_eq!(
        verify_launcher_entry(&entry, &fixture.platform).unwrap(),
        LauncherItemStatus::Missing
    );
    for command in [
        LauncherCommand::RequestLaunch(entry.id.clone()),
        LauncherCommand::ConfirmLaunch(entry.id.clone()),
    ] {
        assert_eq!(
            controller.apply(
                &mut state,
                command,
                Some(&user()),
                &fixture.platform,
                &storage
            ),
            LauncherEffect::None
        );
        assert_eq!(state.items[0].status, LauncherItemStatus::Missing);
    }
    fs::rename(&moved, &executable).expect("restore target");
    assert_eq!(
        verify_launcher_entry(&entry, &fixture.platform).unwrap(),
        LauncherItemStatus::Ready
    );
    fs::remove_file(&executable).expect("delete target");
    assert_eq!(
        verify_launcher_entry(&entry, &fixture.platform).unwrap(),
        LauncherItemStatus::Missing
    );
}

#[test]
fn application_bundles_need_only_the_target_directory_without_content_fingerprints() {
    let fixture = Fixture::new("bundle-presence");
    let bundle = fixture.documents.join("Example.app");
    fs::create_dir(&bundle).expect("bundle directory");
    let bundle = fs::canonicalize(bundle).expect("canonical bundle path");
    let mut bundle_attributes = attributes(&bundle);
    bundle_attributes.is_file = false;
    bundle_attributes.is_dir = true;
    fixture
        .platform
        .set_file_attributes(bundle.clone(), bundle_attributes);
    fixture.platform.set_file_open_policy(
        bundle.clone(),
        FileOpenPolicy::launcher_required(ExecutableKind::ApplicationBundle, "test bundle"),
    );
    let storage = fixture.storage();
    let controller = LauncherController::default();
    let mut state = LauncherState::default();
    controller.apply(
        &mut state,
        LauncherCommand::AddPaths(vec![bundle.clone()]),
        Some(&admin()),
        &fixture.platform,
        &storage,
    );
    assert_eq!(state.items.len(), 1, "{:?}", state.error);
    let entry = state.items[0].record.clone();
    assert_eq!(entry.fingerprint, None);
    assert_eq!(state.items[0].status, LauncherItemStatus::Ready);
    assert_eq!(
        controller.apply(
            &mut state,
            LauncherCommand::RequestLaunch(entry.id.clone()),
            Some(&user()),
            &fixture.platform,
            &storage
        ),
        LauncherEffect::OpenRequested {
            path: PathBuf::from(&entry.path),
            kind: ExecutableKind::ApplicationBundle,
        }
    );
    fs::remove_dir(&bundle).expect("delete bundle");
    assert_eq!(
        verify_launcher_entry(&entry, &fixture.platform).unwrap(),
        LauncherItemStatus::Missing
    );
}

#[test]
fn launcher_keeps_addition_order_and_manual_reorder_persists_in_both_directions() {
    let fixture = Fixture::new("manual-order");
    let charlie = fixture.file("charlie.exe", b"charlie");
    let alpha = fixture.file("alpha.exe", b"alpha");
    let bravo = fixture.file("bravo.exe", b"bravo");
    for executable in [&charlie, &alpha, &bravo] {
        fixture.approve(executable, ExecutableKind::NativeBinary);
    }
    let storage = fixture.storage();
    let controller = LauncherController::default();
    let mut state = LauncherState::default();
    controller.apply(
        &mut state,
        LauncherCommand::AddPaths(vec![charlie, alpha, bravo]),
        Some(&admin()),
        &fixture.platform,
        &storage,
    );

    assert_eq!(
        item_names(&state),
        vec!["charlie.exe", "alpha.exe", "bravo.exe"]
    );
    let charlie_id = state.items[0].record.id.clone();
    controller.apply(
        &mut state,
        LauncherCommand::Reorder {
            id: charlie_id.clone(),
            insertion_index: 3,
        },
        Some(&admin()),
        &fixture.platform,
        &storage,
    );
    assert_eq!(
        item_names(&state),
        vec!["alpha.exe", "bravo.exe", "charlie.exe"]
    );
    assert!(
        state
            .items
            .iter()
            .all(|item| item.status == LauncherItemStatus::Ready)
    );

    let persisted = storage.load_config().expect("reordered config");
    assert_eq!(
        persisted
            .launcher
            .entries
            .iter()
            .map(|entry| entry.id.as_str())
            .collect::<Vec<_>>(),
        state
            .items
            .iter()
            .map(|item| item.record.id.as_str())
            .collect::<Vec<_>>()
    );
    let reloaded = controller.load(&storage).expect("reloaded Launcher state");
    assert_eq!(
        item_names(&reloaded),
        vec!["alpha.exe", "bravo.exe", "charlie.exe"]
    );

    controller.apply(
        &mut state,
        LauncherCommand::Reorder {
            id: charlie_id,
            insertion_index: 0,
        },
        Some(&admin()),
        &fixture.platform,
        &storage,
    );
    assert_eq!(
        item_names(&state),
        vec!["charlie.exe", "alpha.exe", "bravo.exe"]
    );

    let alpha_id = state.items[1].record.id.clone();
    controller.apply(
        &mut state,
        LauncherCommand::Reorder {
            id: alpha_id,
            insertion_index: 2,
        },
        Some(&admin()),
        &fixture.platform,
        &storage,
    );
    assert_eq!(
        item_names(&state),
        vec!["charlie.exe", "alpha.exe", "bravo.exe"]
    );
}

#[test]
fn terminal_launch_preserves_argument_boundaries_and_working_directory() {
    let fixture = Fixture::new("terminal-launch");
    let executable = fixture.file("terminal app.exe", b"program");
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
    let settings = LauncherTerminalSettings {
        arguments: vec!["two words".into(), "$(not-a-command)".into(), "".into()],
        working_directory: Some(fixture.documents.to_string_lossy().into_owned()),
    };
    controller.apply(
        &mut state,
        LauncherCommand::SetTerminalSettings {
            id: id.clone(),
            settings: Some(settings.clone()),
        },
        Some(&admin()),
        &fixture.platform,
        &storage,
    );
    assert!(state.error.is_none(), "{:?}", state.error);
    assert_eq!(state.items[0].record.terminal, Some(settings.clone()));
    assert_eq!(
        controller.load(&storage).unwrap().items[0].record.terminal,
        Some(settings.clone())
    );
    assert_eq!(
        controller.apply(
            &mut state,
            LauncherCommand::RequestLaunch(id.clone()),
            Some(&admin()),
            &fixture.platform,
            &storage,
        ),
        LauncherEffect::OpenTerminalRequested {
            path: executable.clone(),
            args: settings.arguments,
            directory: Some(fixture.documents.clone()),
        }
    );
    controller.apply(
        &mut state,
        LauncherCommand::SetTerminalSettings {
            id: id.clone(),
            settings: None,
        },
        Some(&admin()),
        &fixture.platform,
        &storage,
    );
    assert_eq!(
        controller.apply(
            &mut state,
            LauncherCommand::RequestLaunch(id),
            Some(&user()),
            &fixture.platform,
            &storage,
        ),
        LauncherEffect::OpenRequested {
            path: executable,
            kind: ExecutableKind::NativeBinary,
        }
    );
}

#[test]
fn terminal_settings_require_management_permission_and_launch_requires_command_line_permission() {
    let fixture = Fixture::new("terminal-permissions");
    let executable = fixture.file("terminal.exe", b"program");
    fixture.approve(&executable, ExecutableKind::NativeBinary);
    let storage = fixture.storage();
    let controller = LauncherController::default();
    let mut state = LauncherState::default();
    controller.apply(
        &mut state,
        LauncherCommand::AddPaths(vec![executable]),
        Some(&admin()),
        &fixture.platform,
        &storage,
    );
    let id = state.items[0].record.id.clone();
    controller.apply(
        &mut state,
        LauncherCommand::SetTerminalSettings {
            id: id.clone(),
            settings: Some(LauncherTerminalSettings::default()),
        },
        Some(&user()),
        &fixture.platform,
        &storage,
    );
    assert!(state.error.is_some());
    assert!(
        storage.load_config().unwrap().launcher.entries[0]
            .terminal
            .is_none()
    );
    controller.apply(
        &mut state,
        LauncherCommand::SetTerminalSettings {
            id: id.clone(),
            settings: Some(LauncherTerminalSettings::default()),
        },
        Some(&admin()),
        &fixture.platform,
        &storage,
    );
    assert!(state.error.is_none());
    assert_eq!(
        controller.apply(
            &mut state,
            LauncherCommand::RequestLaunch(id),
            Some(&user()),
            &fixture.platform,
            &storage,
        ),
        LauncherEffect::None
    );
    assert!(state.error.is_some());
}

#[test]
fn terminal_settings_reject_unsupported_targets_and_invalid_directories() {
    let fixture = Fixture::new("terminal-settings-validation");
    let installer = fixture.file("setup.exe", b"installer");
    let executable = fixture.file("terminal.exe", b"program");
    fixture.approve(&installer, ExecutableKind::Installer);
    fixture.approve(&executable, ExecutableKind::NativeBinary);
    let storage = fixture.storage();
    let controller = LauncherController::default();
    let mut state = LauncherState::default();
    controller.apply(
        &mut state,
        LauncherCommand::AddPaths(vec![installer, executable.clone()]),
        Some(&admin()),
        &fixture.platform,
        &storage,
    );
    let installer_id = state.items[0].record.id.clone();
    let executable_id = state.items[1].record.id.clone();
    controller.apply(
        &mut state,
        LauncherCommand::SetTerminalSettings {
            id: installer_id,
            settings: Some(LauncherTerminalSettings::default()),
        },
        Some(&admin()),
        &fixture.platform,
        &storage,
    );
    assert!(state.error.is_some());
    for directory in [
        "relative/folder".to_string(),
        executable.to_string_lossy().into_owned(),
        fixture.root.join("missing").to_string_lossy().into_owned(),
    ] {
        controller.apply(
            &mut state,
            LauncherCommand::SetTerminalSettings {
                id: executable_id.clone(),
                settings: Some(LauncherTerminalSettings {
                    arguments: Vec::new(),
                    working_directory: Some(directory),
                }),
            },
            Some(&admin()),
            &fixture.platform,
            &storage,
        );
        assert!(state.error.is_some());
    }
    assert!(
        storage
            .load_config()
            .unwrap()
            .launcher
            .entries
            .iter()
            .all(|entry| entry.terminal.is_none())
    );
}

#[cfg(unix)]
#[test]
fn embedded_scripts_still_require_confirmation_and_recheck_directory_at_launch() {
    use std::os::unix::fs::PermissionsExt;

    let fixture = Fixture::for_platform("terminal-script", PlatformKind::Linux);
    let script = fixture.file("script.sh", b"#!/bin/sh\nprintf hello\n");
    fs::set_permissions(&script, fs::Permissions::from_mode(0o700)).unwrap();
    fixture.approve(&script, ExecutableKind::Script);
    let directory = fixture.root.join("work directory");
    fs::create_dir(&directory).unwrap();
    let storage = fixture.storage();
    let controller = LauncherController::default();
    let mut state = LauncherState::default();
    controller.apply(
        &mut state,
        LauncherCommand::AddPaths(vec![script.clone()]),
        Some(&admin()),
        &fixture.platform,
        &storage,
    );
    let id = state.items[0].record.id.clone();
    controller.apply(
        &mut state,
        LauncherCommand::SetTerminalSettings {
            id: id.clone(),
            settings: Some(LauncherTerminalSettings {
                arguments: Vec::new(),
                working_directory: Some(directory.to_string_lossy().into_owned()),
            }),
        },
        Some(&admin()),
        &fixture.platform,
        &storage,
    );
    assert!(state.error.is_none(), "{:?}", state.error);
    assert!(matches!(
        controller.apply(
            &mut state,
            LauncherCommand::RequestLaunch(id.clone()),
            Some(&admin()),
            &fixture.platform,
            &storage,
        ),
        LauncherEffect::ConfirmationRequired { .. }
    ));
    assert_eq!(
        controller.apply(
            &mut state,
            LauncherCommand::ConfirmLaunch(id.clone()),
            Some(&admin()),
            &fixture.platform,
            &storage,
        ),
        LauncherEffect::OpenTerminalRequested {
            path: script,
            args: Vec::new(),
            directory: Some(directory.clone()),
        }
    );
    fs::remove_dir(directory).unwrap();
    assert_eq!(
        controller.apply(
            &mut state,
            LauncherCommand::ConfirmLaunch(id),
            Some(&admin()),
            &fixture.platform,
            &storage,
        ),
        LauncherEffect::None
    );
    assert!(state.error.is_some());
}

fn item_names(state: &LauncherState) -> Vec<&str> {
    state
        .items
        .iter()
        .map(|item| {
            Path::new(&item.record.path)
                .file_name()
                .and_then(|name| name.to_str())
                .expect("Launcher item file name")
        })
        .collect()
}

struct Fixture {
    root: PathBuf,
    documents: PathBuf,
    platform: MockPlatform,
}

impl Fixture {
    fn new(name: &str) -> Self {
        Self::for_platform(name, PlatformKind::Windows)
    }

    fn for_platform(name: &str, kind: PlatformKind) -> Self {
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
            platform: MockPlatform::new(user_dirs, app_paths).with_kind(kind),
        }
    }

    fn file(&self, name: &str, bytes: &[u8]) -> PathBuf {
        let path = self.documents.join(name);
        fs::write(&path, bytes).expect("fixture file");
        fs::canonicalize(path).expect("canonical fixture path")
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
        source: identity::IdentitySource::LocalAccount,
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

#[test]
fn custom_command_persists_and_uses_terminal_with_command_permission() {
    let fixture = Fixture::new("custom-command");
    let storage = fixture.storage();
    let controller = LauncherController::default();
    let mut state = LauncherState::default();
    let custom = storage::LauncherCustomCommand {
        name: "my tool".into(),
        icon: "[★]".into(),
        command: "printf 'hello world'".into(),
    };
    controller.apply(
        &mut state,
        LauncherCommand::AddCustom(custom.clone()),
        None,
        &fixture.platform,
        &storage,
    );
    assert!(state.error.is_some());
    assert!(storage.load_config().unwrap().launcher.entries.is_empty());
    controller.apply(
        &mut state,
        LauncherCommand::AddCustom(custom.clone()),
        Some(&admin()),
        &fixture.platform,
        &storage,
    );
    assert!(state.error.is_none(), "{:?}", state.error);
    let mut loaded = controller.load(&storage).unwrap();
    assert_eq!(loaded.items[0].record.custom.as_ref(), Some(&custom));
    assert_eq!(
        verify_launcher_entry(&loaded.items[0].record, &fixture.platform).unwrap(),
        LauncherItemStatus::Ready
    );
    let id = loaded.items[0].record.id.clone();
    let effect = controller.apply(
        &mut loaded,
        LauncherCommand::RequestLaunch(id.clone()),
        Some(&admin()),
        &fixture.platform,
        &storage,
    );
    let LauncherEffect::OpenTerminalRequested {
        path,
        args,
        directory,
    } = effect
    else {
        panic!("terminal required: {effect:?}");
    };
    assert_eq!(
        path,
        PathBuf::from(if cfg!(windows) { "cmd.exe" } else { "/bin/sh" })
    );
    assert_eq!(args.last(), Some(&custom.command));
    assert_eq!(directory, None);
    assert_eq!(
        controller.apply(
            &mut loaded,
            LauncherCommand::RequestLaunch(id),
            None,
            &fixture.platform,
            &storage
        ),
        LauncherEffect::None
    );
    assert!(loaded.error.is_some());
}
