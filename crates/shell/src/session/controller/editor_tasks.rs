use super::super::*;
use std::collections::BTreeSet;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc as StdArc, Mutex as StdMutex, mpsc as std_mpsc};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::session) enum EditorTaskAccess {
    Editable,
    ReadOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::session) enum EditorTaskStage {
    Inspecting,
    Reading,
    Decoding,
    ParsingMarkdown,
    Writing,
}

impl EditorTaskStage {
    pub(in crate::session) const fn label(self) -> &'static str {
        match self {
            Self::Inspecting => "Inspecting",
            Self::Reading => "Reading",
            Self::Decoding => "Decoding",
            Self::ParsingMarkdown => "Parsing Markdown",
            Self::Writing => "Writing",
        }
    }
}

#[derive(Debug)]
pub(in crate::session) struct EditorLoadTaskRequest {
    pub(in crate::session) id: u64,
    pub(in crate::session) path: PathBuf,
    pub(in crate::session) access: EditorTaskAccess,
}

#[derive(Debug)]
pub(in crate::session) struct EditorLoadedTaskDocument {
    pub(in crate::session) state: EditorState,
    pub(in crate::session) fingerprint: DocumentFingerprint,
    pub(in crate::session) total_bytes: u64,
    pub(in crate::session) rich_blocks: Option<std::sync::Arc<[ui::EditorRenderBlock]>>,
}

#[derive(Debug)]
pub(in crate::session) struct EditorSaveTaskRequest {
    pub(in crate::session) id: u64,
    pub(in crate::session) path: PathBuf,
    pub(in crate::session) snapshot: app::editor::SaveSnapshot,
    pub(in crate::session) expected: Option<DocumentFingerprint>,
}

#[derive(Debug)]
pub(in crate::session) enum EditorSaveTaskError {
    ExternalModification,
    Write(String),
}

#[derive(Debug)]
pub(in crate::session) enum EditorTaskEvent {
    Progress {
        id: u64,
        stage: EditorTaskStage,
        completed_bytes: u64,
        total_bytes: Option<u64>,
    },
    LoadFinished {
        id: u64,
        result: Box<Result<EditorLoadedTaskDocument, String>>,
    },
    SaveFinished {
        id: u64,
        result: Result<DocumentFingerprint, EditorSaveTaskError>,
    },
}

pub(in crate::session) struct ShellEditorTaskShared {
    pub(in crate::session) task_group: Option<ManagedTaskGroup>,
    pub(in crate::session) event_tx: std_mpsc::Sender<EditorTaskEvent>,
    pub(in crate::session) event_rx: StdMutex<std_mpsc::Receiver<EditorTaskEvent>>,
    pub(in crate::session) cancelled: StdArc<StdMutex<BTreeSet<u64>>>,
    pub(in crate::session) workers: StdMutex<BTreeMap<u64, ManagedThreadHandle<()>>>,
    /// Bitset of occupied load slots, allowing one stale cancelled parser and
    /// one current load to coexist without unbounded resource growth.
    pub(in crate::session) active_loads: StdArc<AtomicUsize>,
    pub(in crate::session) active_load_bytes: StdArc<AtomicU64>,
}

pub(in crate::session) const MAX_CONCURRENT_EDITOR_LOADS: usize = 2;
pub(in crate::session) const EDITOR_WATCHDOG_LOAD_SLOTS: u64 = 64;
pub(in crate::session) const MAX_IN_FLIGHT_EDITOR_LOAD_BYTES: u64 = platform::MAX_DOCUMENT_BYTES;
pub(in crate::session) static NEXT_EDITOR_RUNTIME_ID: AtomicU64 = AtomicU64::new(1);

impl Drop for ShellEditorTaskShared {
    fn drop(&mut self) {
        if let Ok(workers) = self.workers.get_mut() {
            if let Ok(mut cancelled) = self.cancelled.lock() {
                cancelled.extend(workers.keys().copied());
            }
            for worker in workers.values() {
                worker.cancel();
            }
        }
    }
}

/// Cloneable handle for independently managed Editor I/O/parse tasks.
///
/// Runtime infrastructure deliberately compares equal so it can live inside
/// `ShellSession`, whose equality is used by deterministic input tests.
#[derive(Clone)]
pub(in crate::session) struct ShellEditorTaskRuntime {
    pub(in crate::session) shared: StdArc<ShellEditorTaskShared>,
}

impl ShellEditorTaskRuntime {
    pub(in crate::session) fn new() -> Self {
        let watchdog = default_editor_watchdog();
        watchdog.map_or_else(Self::unavailable, Self::new_managed)
    }

    pub(in crate::session) fn unavailable() -> Self {
        let (event_tx, event_rx) = std_mpsc::channel();
        Self {
            shared: StdArc::new(ShellEditorTaskShared {
                task_group: None,
                event_tx,
                event_rx: StdMutex::new(event_rx),
                cancelled: StdArc::new(StdMutex::new(BTreeSet::new())),
                workers: StdMutex::new(BTreeMap::new()),
                active_loads: StdArc::new(AtomicUsize::new(0)),
                active_load_bytes: StdArc::new(AtomicU64::new(0)),
            }),
        }
    }

    pub(in crate::session) fn new_managed(watchdog: AppWatchdog) -> Self {
        let (event_tx, event_rx) = std_mpsc::channel();
        let runtime_id = NEXT_EDITOR_RUNTIME_ID
            .fetch_add(1, Ordering::Relaxed)
            .max(1);
        Self {
            shared: StdArc::new(ShellEditorTaskShared {
                task_group: Some(watchdog.task_group(&format!("editor-io-{runtime_id}"))),
                event_tx,
                event_rx: StdMutex::new(event_rx),
                cancelled: StdArc::new(StdMutex::new(BTreeSet::new())),
                workers: StdMutex::new(BTreeMap::new()),
                active_loads: StdArc::new(AtomicUsize::new(0)),
                active_load_bytes: StdArc::new(AtomicU64::new(0)),
            }),
        }
    }

    pub(in crate::session) fn submit_load(
        &self,
        id: u64,
        path: PathBuf,
        access: EditorTaskAccess,
    ) -> Result<(), String> {
        let task_group = self
            .shared
            .task_group
            .clone()
            .ok_or_else(|| "Editor loader worker is unavailable".to_string())?;
        let request = EditorLoadTaskRequest { id, path, access };
        let events = self.shared.event_tx.clone();
        let cancelled = StdArc::clone(&self.shared.cancelled);
        let active_load_bytes = StdArc::clone(&self.shared.active_load_bytes);
        let mut workers = self
            .shared
            .workers
            .lock()
            .map_err(|_| "Editor task registry is unavailable".to_string())?;
        let slot = acquire_editor_load_slot(&self.shared.active_loads).ok_or_else(|| {
            "Too many cancelled Editor loads are still finishing; wait for cleanup".to_string()
        })?;
        let slot_bit = 1_usize << slot;
        let watchdog_slot = id % EDITOR_WATCHDOG_LOAD_SLOTS;
        let task_id = match TaskId::new(format!("document-load-{watchdog_slot}")) {
            Ok(task_id) => task_id,
            Err(error) => {
                self.shared
                    .active_loads
                    .fetch_and(!slot_bit, Ordering::AcqRel);
                return Err(format!("invalid Editor load task: {error}"));
            }
        };
        let active_loads = StdArc::clone(&self.shared.active_loads);
        let worker = match task_group.spawn_thread(TaskSpec::one_shot(task_id), move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                load_editor_document_task(&request, &events, &cancelled, &active_load_bytes)
            }));
            active_loads.fetch_and(!slot_bit, Ordering::AcqRel);
            if let Ok(mut cancelled) = cancelled.lock() {
                cancelled.remove(&request.id);
            }
            match result {
                Ok(result) => {
                    let _ = events.send(EditorTaskEvent::LoadFinished {
                        id: request.id,
                        result: Box::new(result),
                    });
                }
                Err(payload) => {
                    let _ = events.send(EditorTaskEvent::LoadFinished {
                        id: request.id,
                        result: Box::new(Err("Editor load worker panicked".to_string())),
                    });
                    std::panic::resume_unwind(payload);
                }
            }
        }) {
            Ok(worker) => worker,
            Err(error) => {
                self.shared
                    .active_loads
                    .fetch_and(!slot_bit, Ordering::AcqRel);
                return Err(format!("Could not start Editor load task: {error}"));
            }
        };
        workers.insert(id, worker);
        Ok(())
    }

    pub(in crate::session) fn cancel(&self, id: u64) {
        if let Ok(mut cancelled) = self.shared.cancelled.lock() {
            cancelled.insert(id);
        }
        // Do not cancel the watchdog wrapper here: it checks its flag before
        // invoking the task factory. Skipping the factory would also skip the
        // load permit cleanup and terminal event. The reader uses the shared
        // cancellation set cooperatively and parsing checks it at boundaries.
    }

    pub(in crate::session) fn submit_save(
        &self,
        id: u64,
        path: PathBuf,
        snapshot: app::editor::SaveSnapshot,
        expected: Option<DocumentFingerprint>,
    ) -> Result<(), String> {
        let task_group = self
            .shared
            .task_group
            .clone()
            .ok_or_else(|| "Editor I/O worker is unavailable".to_string())?;
        let request = EditorSaveTaskRequest {
            id,
            path,
            snapshot,
            expected,
        };
        let events = self.shared.event_tx.clone();
        let task_id = TaskId::new("document-save")
            .map_err(|error| format!("invalid Editor save task: {error}"))?;
        let mut workers = self
            .shared
            .workers
            .lock()
            .map_err(|_| "Editor task registry is unavailable".to_string())?;
        let worker = task_group
            .spawn_thread(TaskSpec::one_shot(task_id), move || {
                let _ = events.send(EditorTaskEvent::Progress {
                    id: request.id,
                    stage: EditorTaskStage::Writing,
                    completed_bytes: 0,
                    total_bytes: None,
                });
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    platform::atomic_write_document_if_unchanged_with(
                        &request.path,
                        request.expected,
                        |writer| request.snapshot.write_to(writer),
                    )
                    .map_err(|error| match error {
                        platform::DocumentWriteError::ExternalModification { .. } => {
                            EditorSaveTaskError::ExternalModification
                        }
                        error => EditorSaveTaskError::Write(error.to_string()),
                    })
                }));
                match result {
                    Ok(result) => {
                        let _ = events.send(EditorTaskEvent::SaveFinished {
                            id: request.id,
                            result,
                        });
                    }
                    Err(payload) => {
                        let _ = events.send(EditorTaskEvent::SaveFinished {
                            id: request.id,
                            result: Err(EditorSaveTaskError::Write(
                                "Editor save worker panicked".to_string(),
                            )),
                        });
                        std::panic::resume_unwind(payload);
                    }
                }
            })
            .map_err(|error| format!("Could not start Editor save task: {error}"))?;
        workers.insert(id, worker);
        Ok(())
    }

    pub(in crate::session) fn drain_events(&self) -> Vec<EditorTaskEvent> {
        let Ok(receiver) = self.shared.event_rx.lock() else {
            return Vec::new();
        };
        let events = std::iter::from_fn(|| receiver.try_recv().ok()).collect::<Vec<_>>();
        drop(receiver);
        if let Ok(mut workers) = self.shared.workers.lock() {
            for event in &events {
                let finished = match event {
                    EditorTaskEvent::LoadFinished { id, .. }
                    | EditorTaskEvent::SaveFinished { id, .. } => Some(*id),
                    EditorTaskEvent::Progress { .. } => None,
                };
                if let Some(id) = finished {
                    workers.remove(&id);
                }
            }
        }
        if let Ok(mut cancelled) = self.shared.cancelled.lock() {
            for event in &events {
                if let EditorTaskEvent::LoadFinished { id, .. } = event {
                    cancelled.remove(id);
                }
            }
        }
        events
    }
}

pub(in crate::session) fn acquire_editor_load_slot(slots: &AtomicUsize) -> Option<usize> {
    loop {
        let occupied = slots.load(Ordering::Acquire);
        let slot =
            (0..MAX_CONCURRENT_EDITOR_LOADS).find(|slot| occupied & (1_usize << slot) == 0)?;
        let reserved = occupied | (1_usize << slot);
        match slots.compare_exchange_weak(occupied, reserved, Ordering::AcqRel, Ordering::Acquire) {
            Ok(_) => return Some(slot),
            Err(_) => continue,
        }
    }
}

pub(in crate::session) struct EditorLoadBytePermit<'a> {
    pub(in crate::session) active: &'a AtomicU64,
    pub(in crate::session) bytes: u64,
}

impl Drop for EditorLoadBytePermit<'_> {
    fn drop(&mut self) {
        self.active.fetch_sub(self.bytes, Ordering::AcqRel);
    }
}

pub(in crate::session) fn reserve_editor_load_bytes(
    active: &AtomicU64,
    bytes: u64,
) -> Result<EditorLoadBytePermit<'_>, String> {
    active
        .fetch_update(Ordering::AcqRel, Ordering::Acquire, |current| {
            current
                .checked_add(bytes)
                .filter(|total| *total <= MAX_IN_FLIGHT_EDITOR_LOAD_BYTES)
        })
        .map_err(|_| {
            "Another large Editor document is still being released; wait before opening this file"
                .to_string()
        })?;
    Ok(EditorLoadBytePermit { active, bytes })
}

pub(in crate::session) fn default_editor_watchdog() -> Option<AppWatchdog> {
    if let Some(process) = ProcessWatchdog::global() {
        return process.register_app(shell_watchdog_descriptor()).ok();
    }
    static FALLBACK: std::sync::OnceLock<Option<AppWatchdog>> = std::sync::OnceLock::new();
    FALLBACK
        .get_or_init(|| {
            let root = std::env::temp_dir().join(format!(
                "tundra-shell-editor-watchdog-{}",
                std::process::id()
            ));
            let config = watchdog::WatchdogConfig::new(
                root.join("reports"),
                root.join("fallback"),
                root.join("state"),
                "tundra-shell-editor",
                env!("CARGO_PKG_VERSION"),
            );
            let (runtime, process) = watchdog::WatchdogRuntime::start(config).ok()?;
            let process = process.install_global().ok()?;
            let _runtime = Box::leak(Box::new(runtime));
            process.register_app(shell_watchdog_descriptor()).ok()
        })
        .clone()
}

impl fmt::Debug for ShellEditorTaskRuntime {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ShellEditorTaskRuntime")
            .finish_non_exhaustive()
    }
}

impl PartialEq for ShellEditorTaskRuntime {
    fn eq(&self, _other: &Self) -> bool {
        true
    }
}

impl Eq for ShellEditorTaskRuntime {}

pub(in crate::session) fn load_editor_document_task(
    request: &EditorLoadTaskRequest,
    events: &std_mpsc::Sender<EditorTaskEvent>,
    cancelled: &StdArc<StdMutex<BTreeSet<u64>>>,
    active_load_bytes: &AtomicU64,
) -> Result<EditorLoadedTaskDocument, String> {
    let is_cancelled = || {
        cancelled
            .lock()
            .is_ok_and(|cancelled| cancelled.contains(&request.id))
    };
    if is_cancelled() {
        return Err("Editor load cancelled".to_string());
    }

    let _ = events.send(EditorTaskEvent::Progress {
        id: request.id,
        stage: EditorTaskStage::Inspecting,
        completed_bytes: 0,
        total_bytes: None,
    });
    let metadata = std::fs::symlink_metadata(&request.path)
        .map_err(|error| format!("Could not inspect {}: {error}", request.path.display()))?;
    let total_bytes = metadata.len();
    if total_bytes > platform::MAX_DOCUMENT_BYTES {
        return Err(format!(
            "Document is too large to open ({} bytes; maximum is {} bytes)",
            total_bytes,
            platform::MAX_DOCUMENT_BYTES
        ));
    }
    let _ = events.send(EditorTaskEvent::Progress {
        id: request.id,
        stage: EditorTaskStage::Reading,
        completed_bytes: 0,
        total_bytes: Some(total_bytes),
    });
    let mut last_reported = 0_u64;
    let mut byte_permit = None;
    let mut budget_error = None;
    let loaded = {
        let mut progress = |completed_bytes: u64, total_bytes: u64| {
            if byte_permit.is_none() {
                match reserve_editor_load_bytes(active_load_bytes, total_bytes) {
                    Ok(permit) => byte_permit = Some(permit),
                    Err(error) => {
                        budget_error = Some(error);
                        return false;
                    }
                }
            }
            if completed_bytes == 0
                || completed_bytes == total_bytes
                || completed_bytes.saturating_sub(last_reported) >= 1024 * 1024
            {
                last_reported = completed_bytes;
                let _ = events.send(EditorTaskEvent::Progress {
                    id: request.id,
                    stage: EditorTaskStage::Reading,
                    completed_bytes,
                    total_bytes: Some(total_bytes),
                });
            }
            !is_cancelled()
        };
        if is_log_document_path(&request.path) {
            platform::read_document_prefix_snapshot_limited_with_progress(
                &request.path,
                platform::MAX_DOCUMENT_BYTES,
                &mut progress,
            )
        } else {
            platform::read_document_bytes_limited_with_progress(
                &request.path,
                platform::MAX_DOCUMENT_BYTES,
                &mut progress,
            )
        }
    };
    let loaded = loaded.map_err(|error| {
        if let Some(error) = budget_error.take() {
            error
        } else if matches!(error, platform::PlatformError::Interrupted { .. }) && is_cancelled() {
            "Editor load cancelled".to_string()
        } else {
            error.to_string()
        }
    })?;
    if is_cancelled() {
        return Err("Editor load cancelled".to_string());
    }
    let _ = events.send(EditorTaskEvent::Progress {
        id: request.id,
        stage: EditorTaskStage::Decoding,
        completed_bytes: loaded.bytes.len() as u64,
        total_bytes: Some(loaded.bytes.len() as u64),
    });
    if app::editor::DocumentKind::from_path(&request.path) == app::editor::DocumentKind::Markdown {
        let _ = events.send(EditorTaskEvent::Progress {
            id: request.id,
            stage: EditorTaskStage::ParsingMarkdown,
            completed_bytes: loaded.bytes.len() as u64,
            total_bytes: Some(loaded.bytes.len() as u64),
        });
    }
    let loaded_bytes = loaded.bytes.len() as u64;
    let state = match request.access {
        EditorTaskAccess::Editable => EditorState::open_owned(request.path.clone(), loaded.bytes),
        EditorTaskAccess::ReadOnly => {
            EditorState::open_read_only_owned(request.path.clone(), loaded.bytes)
        }
    }
    .map_err(|error| error.to_string())?;
    if is_cancelled() {
        return Err("Editor load cancelled".to_string());
    }
    // Prime revision-scoped derived metadata before handing the state to the
    // UI thread. Source mode is already O(1); Rich mode avoids a first-frame
    // full projection scan hereafter.
    let _ = state.word_count();
    let rich_blocks = state
        .rich_projection()
        .map(|projection| std::sync::Arc::from(editor_rich_render_blocks(&projection)));
    if is_cancelled() {
        return Err("Editor load cancelled".to_string());
    }
    Ok(EditorLoadedTaskDocument {
        state,
        fingerprint: loaded.fingerprint,
        total_bytes: loaded_bytes,
        rich_blocks,
    })
}

pub(in crate::session) static NEXT_EDITOR_TASK_ID: AtomicU64 = AtomicU64::new(1);

pub(in crate::session) fn next_editor_task_id() -> u64 {
    NEXT_EDITOR_TASK_ID.fetch_add(1, Ordering::Relaxed).max(1)
}
