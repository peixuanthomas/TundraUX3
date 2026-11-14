use app::explorer::{
    ExplorerClipboard, ExplorerClipboardMode, ExplorerCommand, ExplorerController, ExplorerError,
    ExplorerFileService, ExplorerSelectionMode, ExplorerSharedOperations, ExplorerSharedRequest,
    ExplorerSharedSnapshot, ExplorerState,
};
use identity::{AuthSession, IdentitySource, UserRole};
use platform::mock::MockPlatform;
use platform::{Platform, PlatformKind, UserDirs, build_windows_app_paths, cleanup_temp_path};
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::{SystemTime, UNIX_EPOCH};
use storage::StorageManager;

#[test]
fn separate_windows_share_a_cut_and_undo_its_paste() {
    let fixture = Fixture::new("cut");
    let source = fixture.documents.join("剪切.txt");
    fs::write(&source, "shared contents").unwrap();

    let (first, mut from) = fixture.window(&fixture.documents);
    let (second, mut to) = fixture.window(&fixture.destination);
    select(&mut from, &source);
    fixture.apply(&first, &mut from, ExplorerCommand::Cut);
    fixture.apply(&second, &mut to, ExplorerCommand::Refresh);
    assert_eq!(to.clipboard.as_ref().unwrap().paths, [source.clone()]);

    fixture.apply(&second, &mut to, ExplorerCommand::Paste);
    let moved = fixture.destination.join("剪切.txt");
    assert!(to.error.is_none(), "{:?}", to.error);
    assert_eq!(fs::read_to_string(&moved).unwrap(), "shared contents");
    assert!(!source.exists());
    fixture.apply(&first, &mut from, ExplorerCommand::Refresh);
    assert!(from.clipboard.is_none());
    assert!(from.can_undo);

    fixture.apply(&first, &mut from, ExplorerCommand::Undo);
    assert_eq!(fs::read_to_string(&source).unwrap(), "shared contents");
    assert!(!moved.exists());
}

#[test]
fn paste_does_not_use_cached_clipboard_after_transport_failure() {
    let fixture = Fixture::new("disconnected");
    let source = fixture.documents.join("source.txt");
    fs::write(&source, "copy contents").unwrap();
    let (first, mut from) = fixture.window(&fixture.documents);
    let (second, mut to) = fixture.window(&fixture.destination);
    select(&mut from, &source);
    fixture.apply(&first, &mut from, ExplorerCommand::Copy);
    fixture.apply(&second, &mut to, ExplorerCommand::Refresh);
    assert_eq!(to.clipboard.as_ref().unwrap().paths, [source.clone()]);

    fixture.backend.available.store(false, Ordering::Relaxed);
    fixture.apply(&second, &mut to, ExplorerCommand::Paste);
    assert!(to.error.is_some());
    assert!(!fixture.destination.join("source.txt").exists());
    assert_eq!(fs::read_to_string(source).unwrap(), "copy contents");
}

#[test]
fn another_window_can_undo_a_rename_and_preserve_later_edits() {
    let fixture = Fixture::new("rename-edit");
    let original = fixture.documents.join("original.txt");
    fs::write(&original, "initial contents").unwrap();
    let (first, mut from) = fixture.window(&fixture.documents);
    let (second, mut to) = fixture.window(&fixture.documents);
    select(&mut from, &original);
    fixture.apply(
        &first,
        &mut from,
        ExplorerCommand::Rename("renamed.txt".into()),
    );
    assert!(from.can_undo, "{:?}", from.error);
    let renamed = fixture.documents.join("renamed.txt");
    fs::write(&renamed, "edited after rename").unwrap();

    fixture.apply(&second, &mut to, ExplorerCommand::Undo);
    assert!(to.error.is_none(), "{:?}", to.error);
    assert_eq!(fs::read_to_string(original).unwrap(), "edited after rename");
    assert!(!renamed.exists());
}

#[test]
fn another_window_cannot_undo_a_rename_after_replacement() {
    let fixture = Fixture::new("rename-replaced");
    let original = fixture.documents.join("original.txt");
    fs::write(&original, "original contents").unwrap();
    let (first, mut from) = fixture.window(&fixture.documents);
    let (second, mut to) = fixture.window(&fixture.documents);
    select(&mut from, &original);
    fixture.apply(
        &first,
        &mut from,
        ExplorerCommand::Rename("renamed.txt".into()),
    );
    assert!(from.can_undo, "{:?}", from.error);
    let renamed = fixture.documents.join("renamed.txt");
    let saved = fixture.documents.join("saved.txt");
    fs::rename(&renamed, &saved).unwrap();
    fs::write(&renamed, "replacement contents").unwrap();

    fixture.apply(&second, &mut to, ExplorerCommand::Undo);
    assert!(to.error.is_some());
    assert!(!original.exists());
    assert_eq!(fs::read_to_string(renamed).unwrap(), "replacement contents");
    assert_eq!(fs::read_to_string(saved).unwrap(), "original contents");
}

#[test]
fn clipboard_request_and_reply_keep_non_unicode_paths() {
    #[cfg(unix)]
    let name = {
        use std::os::unix::ffi::OsStringExt;
        std::ffi::OsString::from_vec(b"bad\xff.txt".to_vec())
    };
    #[cfg(windows)]
    let name = {
        use std::os::windows::ffi::OsStringExt;
        std::ffi::OsString::from_wide(&[b'a' as u16, 0xd800])
    };
    let fixture = Fixture::new("native-path");
    let path = fixture.documents.join(name);
    let snapshot = fixture
        .backend
        .execute(ExplorerSharedRequest::SetClipboard(Some(
            ExplorerClipboard {
                paths: vec![path.clone()],
                mode: ExplorerClipboardMode::Cut,
            },
        )))
        .unwrap();
    assert_eq!(snapshot.clipboard.unwrap().paths, [path.clone()]);
    let snapshot = fixture
        .backend
        .execute(ExplorerSharedRequest::ConsumeCutPaths(vec![path]))
        .unwrap();
    assert!(snapshot.clipboard.is_none());
}

struct WireBackend {
    files: ExplorerFileService,
    platform: MockPlatform,
    actor: AuthSession,
    available: AtomicBool,
}

impl fmt::Debug for WireBackend {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("WireBackend")
            .finish_non_exhaustive()
    }
}

impl ExplorerSharedOperations for WireBackend {
    fn execute(
        &self,
        request: ExplorerSharedRequest,
    ) -> Result<ExplorerSharedSnapshot, ExplorerError> {
        if !self.available.load(Ordering::Relaxed) {
            return Err(ExplorerError::InvalidOperation(
                "shared connection closed".into(),
            ));
        }
        let request = serde_json::to_vec(&request).unwrap();
        let request = serde_json::from_slice(&request).unwrap();
        let reply = self
            .files
            .apply_shared_request(request, Some(&self.actor), &self.platform)?;
        let reply = serde_json::to_vec(&reply).unwrap();
        Ok(serde_json::from_slice(&reply).unwrap())
    }
}

struct Fixture {
    root: PathBuf,
    documents: PathBuf,
    destination: PathBuf,
    backend: Arc<WireBackend>,
    storage: StorageManager,
}

impl Fixture {
    fn new(name: &str) -> Self {
        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
        let root = std::env::temp_dir().canonicalize().unwrap().join(format!(
            "tundra-shared-explorer-{name}-{}-{}",
            std::process::id(),
            now.as_nanos()
        ));
        fs::create_dir(&root).unwrap();
        let documents = root.join("Documents");
        let destination = root.join("Destination");
        fs::create_dir(&documents).unwrap();
        fs::create_dir(&destination).unwrap();
        let app_paths =
            build_windows_app_paths(root.join("Roaming"), root.join("Local"), root.join("Temp"))
                .unwrap();
        let user_dirs = UserDirs::new(
            root.join("Desktop"),
            documents.clone(),
            root.join("Downloads"),
            root.join("Pictures"),
            root.join("Videos"),
            root.join("Music"),
            root.clone(),
        )
        .unwrap();
        let platform = MockPlatform::new(user_dirs, app_paths).with_kind(PlatformKind::Windows);
        let storage = StorageManager::open(platform.app_paths().unwrap())
            .unwrap()
            .manager;
        let backend = Arc::new(WireBackend {
            files: ExplorerFileService::default(),
            platform,
            actor: AuthSession {
                source: IdentitySource::LocalAccount,
                session_id: "shared-session".into(),
                user_id: "user-1".into(),
                username: "AdminUser".into(),
                role: UserRole::Admin,
                started_at_epoch_ms: now.as_millis() as u64,
            },
            available: AtomicBool::new(true),
        });
        Self {
            root,
            documents,
            destination,
            backend,
            storage,
        }
    }

    fn window(&self, path: &Path) -> (ExplorerController, ExplorerState) {
        let files = ExplorerFileService::default().with_shared_operations(self.backend.clone());
        let controller = ExplorerController::new(files);
        let mut state = ExplorerState::new(path, true);
        self.apply(&controller, &mut state, ExplorerCommand::Refresh);
        assert!(state.error.is_none(), "{:?}", state.error);
        (controller, state)
    }

    fn apply(
        &self,
        controller: &ExplorerController,
        state: &mut ExplorerState,
        command: ExplorerCommand,
    ) {
        controller.apply(
            state,
            command,
            Some(&self.backend.actor),
            &self.backend.platform,
            &self.storage,
        );
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = cleanup_temp_path(&self.root);
    }
}

fn select(state: &mut ExplorerState, path: &Path) {
    let index = state
        .entries
        .iter()
        .position(|entry| entry.path == path)
        .unwrap();
    state.select_index(index, ExplorerSelectionMode::Replace);
}
