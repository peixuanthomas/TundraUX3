use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use tundra_apps::explorer_tasks::{
    DirectoryExplorerTrash, ExplorerCollisionPolicy, ExplorerCollisionResolution,
    ExplorerDeletePlan, ExplorerTaskEngine, ExplorerTaskError, ExplorerTaskEvent, ExplorerTaskPlan,
    ExplorerTaskSubmitError, ExplorerTransferOperation, ExplorerTransferPlan,
};
use tundra_platform::{
    AppPaths, Platform, PlatformCapabilities, PlatformError, PlatformKind, ProcessExit,
    ProcessSpec, UserDirs, default_file_attributes, default_rename_path,
};

static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

struct TempTree {
    root: PathBuf,
}

impl TempTree {
    fn new(label: &str) -> Self {
        let millis = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "tundra-explorer-task-{label}-{}-{millis}-{sequence}",
            std::process::id()
        ));
        fs::create_dir_all(&root).expect("create temporary tree");
        Self { root }
    }

    fn path(&self, relative: &str) -> PathBuf {
        self.root.join(relative)
    }
}

impl Drop for TempTree {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[derive(Debug, Default)]
struct TestPlatform {
    cross_device_source_root: Option<PathBuf>,
    unsafe_path: Option<PathBuf>,
}

impl TestPlatform {
    fn cross_device(source_root: PathBuf) -> Self {
        Self {
            cross_device_source_root: Some(source_root),
            unsafe_path: None,
        }
    }

    fn unsafe_path(path: PathBuf) -> Self {
        Self {
            cross_device_source_root: None,
            unsafe_path: Some(path),
        }
    }

    fn unsupported(capability: &'static str) -> PlatformError {
        PlatformError::Unsupported { capability }
    }
}

impl Platform for TestPlatform {
    fn kind(&self) -> PlatformKind {
        PlatformKind::Unsupported
    }

    fn capabilities(&self) -> PlatformCapabilities {
        PlatformCapabilities::unsupported()
    }

    fn user_dirs(&self) -> Result<UserDirs, PlatformError> {
        Err(Self::unsupported("user_dirs"))
    }

    fn app_paths(&self) -> Result<AppPaths, PlatformError> {
        Err(Self::unsupported("app_paths"))
    }

    fn open_path(&self, _path: &Path) -> Result<(), PlatformError> {
        Err(Self::unsupported("open_path"))
    }

    fn open_with(&self, _path: &Path, _application: &Path) -> Result<(), PlatformError> {
        Err(Self::unsupported("open_with"))
    }

    fn open_uri(&self, _uri: &str) -> Result<(), PlatformError> {
        Err(Self::unsupported("open_uri"))
    }

    fn spawn_detached(&self, _spec: &ProcessSpec) -> Result<(), PlatformError> {
        Err(Self::unsupported("spawn_detached"))
    }

    fn spawn_wait(&self, _spec: &ProcessSpec) -> Result<ProcessExit, PlatformError> {
        Err(Self::unsupported("spawn_wait"))
    }

    fn read_clipboard_text(&self) -> Result<String, PlatformError> {
        Err(Self::unsupported("clipboard_text"))
    }

    fn write_clipboard_text(&self, _text: &str) -> Result<(), PlatformError> {
        Err(Self::unsupported("clipboard_text"))
    }

    fn file_attributes(
        &self,
        path: &Path,
    ) -> Result<tundra_platform::FileAttributes, PlatformError> {
        let mut attributes = default_file_attributes(path)?;
        if self
            .unsafe_path
            .as_ref()
            .is_some_and(|unsafe_path| path == unsafe_path || path.starts_with(unsafe_path))
        {
            attributes.reparse_point = true;
        }
        Ok(attributes)
    }

    fn rename_path(&self, source: &Path, target: &Path) -> Result<(), PlatformError> {
        if self
            .cross_device_source_root
            .as_ref()
            .is_some_and(|root| source.starts_with(root))
        {
            return Err(PlatformError::CrossDevice {
                source: source.to_path_buf(),
                target: target.to_path_buf(),
                message: "injected cross-device rename".to_string(),
            });
        }
        default_rename_path(source, target)
    }
}

fn finished(
    engine: &ExplorerTaskEngine,
) -> (
    Vec<ExplorerTaskEvent>,
    tundra_apps::explorer_tasks::ExplorerTaskSummary,
) {
    let mut events = Vec::new();
    loop {
        let event = engine
            .recv_timeout(Duration::from_secs(10))
            .expect("task should finish");
        if let ExplorerTaskEvent::Finished { summary, .. } = &event {
            let summary = summary.clone();
            events.push(event);
            return (events, summary);
        }
        events.push(event);
    }
}

fn engine(platform: Arc<dyn Platform>, trash: PathBuf) -> ExplorerTaskEngine {
    ExplorerTaskEngine::new(platform, Arc::new(DirectoryExplorerTrash::new(trash)))
}

#[test]
fn copy_keep_both_is_staged_and_reports_monotonic_progress() {
    let tree = TempTree::new("keep-both");
    let source_dir = tree.path("source");
    let destination = tree.path("destination");
    fs::create_dir_all(&source_dir).unwrap();
    fs::create_dir_all(&destination).unwrap();
    let source = source_dir.join("notes.txt");
    fs::write(&source, b"new contents").unwrap();
    fs::write(destination.join("notes.txt"), b"old contents").unwrap();

    let engine = engine(Arc::new(TestPlatform::default()), tree.path("trash"));
    let plan = ExplorerTransferPlan::new(
        ExplorerTransferOperation::Copy,
        vec![source.clone()],
        &destination,
    );
    engine
        .submit(ExplorerTaskPlan::Transfer(plan))
        .expect("submit copy");
    let (events, summary) = finished(&engine);

    assert_eq!(
        fs::read(destination.join("notes.txt")).unwrap(),
        b"old contents"
    );
    assert_eq!(
        fs::read(destination.join("notes (2).txt")).unwrap(),
        b"new contents"
    );
    assert!(source.exists());
    assert_eq!(
        summary.succeeded_sources,
        vec![fs::canonicalize(&source).unwrap()]
    );
    assert_eq!(summary.failed_items, 0);
    assert_eq!(summary.processed_bytes, summary.total_bytes);

    let progress: Vec<_> = events
        .iter()
        .filter_map(|event| match event {
            ExplorerTaskEvent::Progress { progress, .. } => Some(progress),
            _ => None,
        })
        .collect();
    assert!(!progress.is_empty());
    assert!(progress.windows(2).all(|pair| {
        pair[0].processed_items <= pair[1].processed_items
            && pair[0].processed_bytes <= pair[1].processed_bytes
    }));
    assert!(fs::read_dir(&destination).unwrap().all(|entry| {
        !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".tundra-stage-")
    }));
}

#[test]
fn collision_cancel_is_resolved_before_any_mutation() {
    let tree = TempTree::new("cancel-conflict");
    let source_dir = tree.path("source");
    let destination = tree.path("destination");
    fs::create_dir_all(&source_dir).unwrap();
    fs::create_dir_all(&destination).unwrap();
    let source = source_dir.join("same.txt");
    let target = destination.join("same.txt");
    fs::write(&source, b"source").unwrap();
    fs::write(&target, b"target").unwrap();

    let engine = engine(Arc::new(TestPlatform::default()), tree.path("trash"));
    let mut plan = ExplorerTransferPlan::new(
        ExplorerTransferOperation::Copy,
        vec![source.clone()],
        &destination,
    );
    plan.collisions.default = ExplorerCollisionResolution::Cancel;
    engine.submit(ExplorerTaskPlan::Transfer(plan)).unwrap();
    let (_, summary) = finished(&engine);

    assert_eq!(fs::read(&source).unwrap(), b"source");
    assert_eq!(fs::read(&target).unwrap(), b"target");
    assert!(matches!(
        summary.fatal_error,
        Some(ExplorerTaskError::CollisionCancelled { .. })
    ));
    assert!(!tree.path("trash").exists());
}

#[test]
fn collision_skip_preserves_both_paths_and_reports_skip() {
    let tree = TempTree::new("skip-conflict");
    let source_dir = tree.path("source");
    let destination = tree.path("destination");
    fs::create_dir_all(&source_dir).unwrap();
    fs::create_dir_all(&destination).unwrap();
    let source = source_dir.join("same.txt");
    let target = destination.join("same.txt");
    fs::write(&source, b"source").unwrap();
    fs::write(&target, b"target").unwrap();

    let engine = engine(Arc::new(TestPlatform::default()), tree.path("trash"));
    let mut plan = ExplorerTransferPlan::new(
        ExplorerTransferOperation::Copy,
        vec![source.clone()],
        &destination,
    );
    plan.collisions.default = ExplorerCollisionResolution::Skip;
    engine.submit(ExplorerTaskPlan::Transfer(plan)).unwrap();
    let (_, summary) = finished(&engine);

    assert_eq!(summary.skipped_items, 1);
    assert_eq!(fs::read(source).unwrap(), b"source");
    assert_eq!(fs::read(target).unwrap(), b"target");
}

#[test]
fn directory_collisions_merge_and_resolve_children() {
    let tree = TempTree::new("merge");
    let source_parent = tree.path("source");
    let source = source_parent.join("project");
    let destination = tree.path("destination");
    let target = destination.join("project");
    fs::create_dir_all(&source).unwrap();
    fs::create_dir_all(&target).unwrap();
    fs::write(source.join("readme.md"), b"source readme").unwrap();
    fs::write(source.join("new.txt"), b"new").unwrap();
    fs::write(target.join("readme.md"), b"target readme").unwrap();
    fs::write(target.join("kept.txt"), b"kept").unwrap();

    let engine = engine(Arc::new(TestPlatform::default()), tree.path("trash"));
    let plan =
        ExplorerTransferPlan::new(ExplorerTransferOperation::Copy, vec![source], &destination);
    engine.submit(ExplorerTaskPlan::Transfer(plan)).unwrap();
    let (_, summary) = finished(&engine);

    assert!(summary.fatal_error.is_none());
    assert_eq!(
        fs::read(target.join("readme.md")).unwrap(),
        b"target readme"
    );
    assert_eq!(
        fs::read(target.join("readme (2).md")).unwrap(),
        b"source readme"
    );
    assert_eq!(fs::read(target.join("new.txt")).unwrap(), b"new");
    assert_eq!(fs::read(target.join("kept.txt")).unwrap(), b"kept");
}

#[test]
fn replace_moves_old_target_to_injected_trash() {
    let tree = TempTree::new("replace");
    let source_dir = tree.path("source");
    let destination = tree.path("destination");
    let trash = tree.path("trash");
    fs::create_dir_all(&source_dir).unwrap();
    fs::create_dir_all(&destination).unwrap();
    let source = source_dir.join("data.bin");
    let target = destination.join("data.bin");
    fs::write(&source, b"replacement").unwrap();
    fs::write(&target, b"original").unwrap();

    let engine = engine(Arc::new(TestPlatform::default()), trash.clone());
    let mut plan =
        ExplorerTransferPlan::new(ExplorerTransferOperation::Copy, vec![source], &destination);
    plan.collisions = ExplorerCollisionPolicy::replace();
    engine.submit(ExplorerTaskPlan::Transfer(plan)).unwrap();
    let (_, summary) = finished(&engine);

    assert!(summary.failures.is_empty());
    assert_eq!(fs::read(&target).unwrap(), b"replacement");
    let trashed: Vec<_> = fs::read_dir(trash)
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(trashed.len(), 1);
    assert_eq!(fs::read(trashed[0].path()).unwrap(), b"original");
}

#[test]
fn move_uses_cross_device_copy_commit_delete_fallback() {
    let tree = TempTree::new("cross-device");
    let source_root = tree.path("source");
    let source = source_root.join("folder");
    let destination = tree.path("destination");
    fs::create_dir_all(source.join("nested")).unwrap();
    fs::create_dir_all(&destination).unwrap();
    fs::write(source.join("nested/item.txt"), b"cross volume").unwrap();

    let platform = Arc::new(TestPlatform::cross_device(
        fs::canonicalize(&source_root).unwrap(),
    ));
    let engine = engine(platform, tree.path("trash"));
    let plan = ExplorerTransferPlan::new(
        ExplorerTransferOperation::Move,
        vec![source.clone()],
        &destination,
    );
    engine.submit(ExplorerTaskPlan::Transfer(plan)).unwrap();
    let (_, summary) = finished(&engine);

    assert!(summary.failures.is_empty(), "{:?}", summary.failures);
    assert!(!source.exists());
    assert_eq!(
        fs::read(destination.join("folder/nested/item.txt")).unwrap(),
        b"cross volume"
    );
    assert_eq!(summary.processed_bytes, summary.total_bytes);
}

#[test]
fn descendant_destination_is_rejected_without_creating_children() {
    let tree = TempTree::new("descendant");
    let source = tree.path("source");
    let destination = source.join("nested");
    fs::create_dir_all(&destination).unwrap();
    fs::write(source.join("item.txt"), b"data").unwrap();

    let engine = engine(Arc::new(TestPlatform::default()), tree.path("trash"));
    let plan =
        ExplorerTransferPlan::new(ExplorerTransferOperation::Copy, vec![source], &destination);
    engine.submit(ExplorerTaskPlan::Transfer(plan)).unwrap();
    let (_, summary) = finished(&engine);

    assert!(matches!(
        summary.fatal_error,
        Some(ExplorerTaskError::InvalidPlan { .. })
    ));
    assert!(!destination.join("source").exists());
}

#[test]
fn copy_preflight_blocks_reparse_points_without_following_them() {
    let tree = TempTree::new("unsafe-link");
    let source_dir = tree.path("source");
    let destination = tree.path("destination");
    fs::create_dir_all(&source_dir).unwrap();
    fs::create_dir_all(&destination).unwrap();
    let source = source_dir.join("unsafe-link");
    fs::write(&source, b"must not be copied").unwrap();
    let unsafe_path = fs::canonicalize(&source).unwrap();

    let engine = engine(
        Arc::new(TestPlatform::unsafe_path(unsafe_path)),
        tree.path("trash"),
    );
    let plan =
        ExplorerTransferPlan::new(ExplorerTransferOperation::Copy, vec![source], &destination);
    engine.submit(ExplorerTaskPlan::Transfer(plan)).unwrap();
    let (_, summary) = finished(&engine);

    assert!(matches!(
        summary.fatal_error,
        Some(ExplorerTaskError::UnsafeLink { .. })
    ));
    assert!(!destination.join("unsafe-link").exists());
}

#[test]
fn a_second_mutation_is_rejected_and_cancelled_plan_leaves_no_stage() {
    let tree = TempTree::new("busy-cancel");
    let source_dir = tree.path("source");
    let destination = tree.path("destination");
    fs::create_dir_all(&source_dir).unwrap();
    fs::create_dir_all(&destination).unwrap();
    let source = source_dir.join("large.bin");
    fs::write(&source, vec![0x5a; 8 * 1024 * 1024]).unwrap();

    let engine = engine(Arc::new(TestPlatform::default()), tree.path("trash"));
    let mut plan =
        ExplorerTransferPlan::new(ExplorerTransferOperation::Copy, vec![source], &destination);
    plan.chunk_size = 4096;
    let handle = engine
        .submit(ExplorerTaskPlan::Transfer(plan.clone()))
        .unwrap();
    assert!(matches!(
        engine.submit(ExplorerTaskPlan::Transfer(plan)),
        Err(ExplorerTaskSubmitError::Busy { active }) if active == handle.id
    ));
    handle.cancellation.cancel();
    let (_, summary) = finished(&engine);

    assert!(summary.cancelled);
    assert!(!destination.join("large.bin").exists());
    assert!(fs::read_dir(&destination).unwrap().all(|entry| {
        !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".tundra-stage-")
    }));
}

#[test]
fn delete_plan_moves_each_path_to_trash() {
    let tree = TempTree::new("delete");
    let source = tree.path("delete-me.txt");
    let trash = tree.path("trash");
    fs::write(&source, b"recoverable").unwrap();

    let engine = engine(Arc::new(TestPlatform::default()), trash.clone());
    engine
        .submit(ExplorerTaskPlan::DeleteToTrash(ExplorerDeletePlan::new(
            vec![source.clone()],
        )))
        .unwrap();
    let (_, summary) = finished(&engine);

    assert!(summary.failures.is_empty());
    assert!(!source.exists());
    let trashed: Vec<_> = fs::read_dir(trash)
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(trashed.len(), 1);
    assert_eq!(fs::read(trashed[0].path()).unwrap(), b"recoverable");
}
