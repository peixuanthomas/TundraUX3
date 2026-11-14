use super::*;
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

static SHARED_OPERATIONS: OnceLock<Arc<dyn ExplorerSharedOperations>> = OnceLock::new();

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ExplorerSharedRequest {
    Snapshot,
    SetClipboard(Option<ExplorerClipboard>),
    ConsumeCutPaths(#[serde(with = "workspace_protocol::native_paths")] Vec<PathBuf>),
    SetBusy(bool),
    RecordMoves(Option<Vec<ExplorerUndoMove>>),
    Undo,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ExplorerSharedSnapshot {
    pub clipboard: Option<ExplorerClipboard>,
    pub can_undo: bool,
    pub busy: bool,
    pub undone: usize,
}

pub trait ExplorerSharedOperations: fmt::Debug + Send + Sync {
    fn execute(
        &self,
        request: ExplorerSharedRequest,
    ) -> Result<ExplorerSharedSnapshot, ExplorerError>;
}

pub fn install_shared_explorer_operations(operations: Arc<dyn ExplorerSharedOperations>) {
    let _ = SHARED_OPERATIONS.set(operations);
}

pub(super) fn shared_explorer_operations() -> Option<Arc<dyn ExplorerSharedOperations>> {
    SHARED_OPERATIONS.get().cloned()
}

impl ExplorerFileService {
    pub fn with_shared_operations(mut self, shared: Arc<dyn ExplorerSharedOperations>) -> Self {
        self.shared = Some(shared);
        self
    }

    pub fn refresh_shared(&self) -> Result<(), ExplorerError> {
        self.apply_shared(ExplorerSharedRequest::Snapshot)?;
        Ok(())
    }

    pub fn shared_snapshot(&self) -> ExplorerSharedSnapshot {
        let operations = self
            .operations
            .lock()
            .expect("Explorer operations lock poisoned");
        ExplorerSharedSnapshot {
            clipboard: operations.clipboard.clone(),
            can_undo: !operations.busy
                && (operations.shared_can_undo || !operations.history.is_empty()),
            busy: operations.busy,
            undone: 0,
        }
    }

    pub fn apply_shared_request(
        &self,
        request: ExplorerSharedRequest,
        session: Option<&AuthSession>,
        platform: &dyn Platform,
    ) -> Result<ExplorerSharedSnapshot, ExplorerError> {
        let mut undone = 0;
        match request {
            ExplorerSharedRequest::Snapshot => {}
            ExplorerSharedRequest::SetClipboard(clipboard) => self.set_clipboard(clipboard)?,
            ExplorerSharedRequest::ConsumeCutPaths(paths) => self.consume_cut_paths(&paths)?,
            ExplorerSharedRequest::SetBusy(busy) => self.set_operation_busy(busy)?,
            ExplorerSharedRequest::RecordMoves(moves) => self.record_moves(moves)?,
            ExplorerSharedRequest::Undo => undone = self.undo_batch(session, platform)?,
        }
        Ok(ExplorerSharedSnapshot {
            undone,
            ..self.shared_snapshot()
        })
    }

    pub(super) fn apply_shared(
        &self,
        request: ExplorerSharedRequest,
    ) -> Result<bool, ExplorerError> {
        let Some(shared) = &self.shared else {
            return Ok(false);
        };
        self.apply_shared_snapshot(shared.execute(request)?);
        Ok(true)
    }

    pub(super) fn apply_shared_snapshot(&self, snapshot: ExplorerSharedSnapshot) {
        let mut operations = self
            .operations
            .lock()
            .expect("Explorer operations lock poisoned");
        operations.clipboard = snapshot.clipboard;
        operations.busy = snapshot.busy;
        operations.shared_can_undo = snapshot.can_undo;
    }
}
