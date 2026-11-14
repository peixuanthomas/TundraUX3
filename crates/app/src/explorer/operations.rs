use super::*;

#[derive(Debug, Default)]
pub(super) struct ExplorerFileOperations {
    pub clipboard: Option<ExplorerClipboard>,
    pub(super) history: Vec<Vec<ExplorerUndoMove>>,
    pub busy: bool,
    pub shared_can_undo: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ExplorerUndoMove {
    #[serde(with = "workspace_protocol::native_path")]
    source: PathBuf,
    #[serde(with = "workspace_protocol::native_path")]
    target: PathBuf,
    source_parent: platform::FileIdentity,
    target_parent: platform::FileIdentity,
    identity: platform::FileIdentity,
}

impl ExplorerUndoMove {
    pub(super) fn prepare(source: &Path, target: &Path) -> Result<Self, ExplorerError> {
        platform::validate_no_follow_path(source, true)?;
        platform::validate_no_follow_path(target, false)?;
        let source_parent = source.parent().ok_or_else(|| undo_error(source))?;
        let target_parent = target.parent().ok_or_else(|| undo_error(target))?;
        Ok(Self {
            source: source.to_path_buf(),
            target: target.to_path_buf(),
            source_parent: platform::file_identity(source_parent)?,
            target_parent: platform::file_identity(target_parent)?,
            identity: platform::file_identity(source)?,
        })
    }

    pub(super) fn target_matches(&self) -> bool {
        platform::file_identity(&self.target).is_ok_and(|identity| identity == self.identity)
    }

    fn validate(&self) -> Result<(), ExplorerError> {
        platform::validate_no_follow_path(&self.source, false)?;
        platform::validate_no_follow_path(&self.target, true)?;
        if path_exists_no_follow(&self.source)?
            || platform::file_identity(self.source.parent().unwrap())? != self.source_parent
            || platform::file_identity(self.target.parent().unwrap())? != self.target_parent
            || !self.target_matches()
        {
            return Err(undo_error(&self.target));
        }
        Ok(())
    }
}

fn undo_error(path: &Path) -> ExplorerError {
    ExplorerError::Localized(LocalizedError::new(
        "EXPLORER_UNDO_CHANGED",
        msg!(
            "app-explorer-undo-changed",
            path = path.display().to_string()
        ),
    ))
}

impl ExplorerFileService {
    pub fn can_undo(&self) -> bool {
        let operations = self
            .operations
            .lock()
            .expect("Explorer operations lock poisoned");
        !operations.busy && (operations.shared_can_undo || !operations.history.is_empty())
    }

    pub fn clipboard(&self) -> Option<ExplorerClipboard> {
        self.operations
            .lock()
            .expect("Explorer operations lock poisoned")
            .clipboard
            .clone()
    }

    pub(super) fn set_clipboard(
        &self,
        clipboard: Option<ExplorerClipboard>,
    ) -> Result<(), ExplorerError> {
        if self.apply_shared(ExplorerSharedRequest::SetClipboard(clipboard.clone()))? {
            return Ok(());
        }
        self.operations
            .lock()
            .expect("Explorer operations lock poisoned")
            .clipboard = clipboard;
        Ok(())
    }

    pub fn consume_cut_paths(&self, paths: &[PathBuf]) -> Result<(), ExplorerError> {
        if self.apply_shared(ExplorerSharedRequest::ConsumeCutPaths(paths.to_vec()))? {
            return Ok(());
        }
        let mut operations = self
            .operations
            .lock()
            .expect("Explorer operations lock poisoned");
        if let Some(clipboard) = &mut operations.clipboard
            && clipboard.mode == ExplorerClipboardMode::Cut
        {
            clipboard.paths.retain(|path| !paths.contains(path));
            if clipboard.paths.is_empty() {
                operations.clipboard = None;
            }
        }
        Ok(())
    }

    pub fn set_operation_busy(&self, busy: bool) -> Result<(), ExplorerError> {
        if self.apply_shared(ExplorerSharedRequest::SetBusy(busy))? {
            return Ok(());
        }
        self.operations
            .lock()
            .expect("Explorer operations lock poisoned")
            .busy = busy;
        Ok(())
    }

    pub fn record_moves(&self, moves: Option<Vec<ExplorerUndoMove>>) -> Result<(), ExplorerError> {
        if self.apply_shared(ExplorerSharedRequest::RecordMoves(moves.clone()))? {
            return Ok(());
        }
        let mut operations = self
            .operations
            .lock()
            .expect("Explorer operations lock poisoned");
        match moves {
            Some(moves) if !moves.is_empty() => operations.history.push(moves),
            None => operations.history.clear(),
            _ => {}
        }
        Ok(())
    }

    pub(super) fn undo(
        &self,
        state: &mut ExplorerState,
        session: Option<&AuthSession>,
        platform: &dyn Platform,
        storage: &StorageManager,
    ) -> Result<(), ExplorerError> {
        let count = if let Some(shared) = &self.shared {
            let snapshot = shared.execute(ExplorerSharedRequest::Undo)?;
            let count = snapshot.undone;
            self.apply_shared_snapshot(snapshot);
            count
        } else {
            self.undo_batch(session, platform)?
        };
        state.set_success(msg!("app-explorer-undone", count = count as i64));
        self.refresh(state, session, platform, storage)
    }

    pub fn undo_batch(
        &self,
        session: Option<&AuthSession>,
        platform: &dyn Platform,
    ) -> Result<usize, ExplorerError> {
        let mut operations = self
            .operations
            .lock()
            .expect("Explorer operations lock poisoned");
        if operations.busy {
            return Err(ExplorerError::Localized(LocalizedError::new(
                "EXPLORER_UNDO_BUSY",
                msg!("app-explorer-undo-busy"),
            )));
        }
        let batch = operations.history.last_mut().ok_or_else(|| {
            ExplorerError::Localized(LocalizedError::new(
                "EXPLORER_UNDO_EMPTY",
                msg!("app-explorer-undo-empty"),
            ))
        })?;
        for movement in batch.iter() {
            self.authorize(session, PermissionAction::DeleteFile, &movement.target)?;
            self.authorize(session, PermissionAction::WriteFile, &movement.source)?;
            movement.validate()?;
        }
        let count = batch.len();
        while let Some(movement) = batch.last() {
            movement.validate()?;
            platform.rename_path_no_replace(&movement.target, &movement.source)?;
            batch.pop();
        }
        operations.history.pop();
        drop(operations);
        Ok(count)
    }
}
