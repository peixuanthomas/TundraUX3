use super::*;

impl ExplorerFileService {
    pub fn new(permission_service: PermissionService) -> Self {
        Self { permission_service }
    }

    pub fn refresh(
        &self,
        state: &mut ExplorerState,
        session: Option<&AuthSession>,
        platform: &dyn Platform,
        storage: &StorageManager,
    ) -> Result<(), ExplorerError> {
        let location = state.current_location.clone();
        let (entries, warning_count) =
            match self.load_location(session, platform, storage, &location) {
                Ok(listing) => listing,
                Err(error) => {
                    // A refresh is different from navigation: the current location is unchanged, but
                    // rows whose existence/authorization can no longer be verified must not remain
                    // actionable. Navigation loads first and therefore keeps the previous rows on
                    // failure; refreshing the already-visible location deliberately clears them.
                    clear_location_listing(state);
                    return Err(error);
                }
            };
        commit_location_listing(state, location, entries, warning_count);
        Ok(())
    }

    pub(super) fn navigate_directory(
        &self,
        state: &mut ExplorerState,
        session: Option<&AuthSession>,
        platform: &dyn Platform,
        storage: &StorageManager,
        target: PathBuf,
        push_history: bool,
    ) -> Result<(), ExplorerError> {
        if !target.is_absolute() {
            return Err(ExplorerError::Localized(LocalizedError::new(
                "EXPLORER_INVALID_OPERATION",
                msg!("app-explorer-absolute-path-required"),
            )));
        }
        let attributes = platform.file_attributes(&target)?;
        if !attributes.is_dir {
            return Err(ExplorerError::Localized(LocalizedError::new(
                "EXPLORER_INVALID_OPERATION",
                msg!(
                    "app-explorer-not-directory",
                    path = target.display().to_string()
                ),
            )));
        }
        self.navigate_location(
            state,
            session,
            platform,
            storage,
            ExplorerLocation::Directory(target),
            push_history,
        )
    }

    pub(super) fn navigate_location(
        &self,
        state: &mut ExplorerState,
        session: Option<&AuthSession>,
        platform: &dyn Platform,
        storage: &StorageManager,
        target: ExplorerLocation,
        push_history: bool,
    ) -> Result<(), ExplorerError> {
        let (entries, warning_count) = self.load_location(session, platform, storage, &target)?;
        if push_history && state.current_location != target {
            state.back_history.push(state.current_location.clone());
            state.forward_history.clear();
        }
        commit_location_listing(state, target, entries, warning_count);
        Ok(())
    }

    pub(super) fn load_location(
        &self,
        session: Option<&AuthSession>,
        platform: &dyn Platform,
        storage: &StorageManager,
        location: &ExplorerLocation,
    ) -> Result<(Vec<ExplorerEntry>, usize), ExplorerError> {
        match location {
            ExplorerLocation::Directory(path) => {
                self.authorize(session, PermissionAction::ReadFile, path)?;
                let listing = platform.read_directory(path)?;
                let warning_count = listing.warnings.len();
                let entries = listing
                    .entries
                    .into_iter()
                    .map(|entry| {
                        ExplorerEntry::from_metadata(
                            entry.path,
                            entry.name,
                            entry.attributes,
                            entry.open_policy,
                        )
                    })
                    .collect();
                Ok((entries, warning_count))
            }
            ExplorerLocation::Trash => {
                self.authorize(
                    session,
                    PermissionAction::ReadFile,
                    &storage.layout().data_path,
                )?;
                let entries = platform
                    .list_trash()?
                    .into_iter()
                    .map(ExplorerEntry::from_trash)
                    .collect();
                Ok((entries, 0))
            }
        }
    }

    pub(super) fn open_entry(
        &self,
        state: &mut ExplorerState,
        session: Option<&AuthSession>,
        platform: &dyn Platform,
        storage: &StorageManager,
        entry: &ExplorerEntry,
        resolver: &dyn ExplorerOpenRouteResolver,
    ) -> Result<ExplorerEffect, ExplorerError> {
        if state.current_location.is_trash() {
            return Err(ExplorerError::Localized(LocalizedError::new(
                "EXPLORER_INVALID_OPERATION",
                msg!("app-explorer-restore-before-open"),
            )));
        }
        match &entry.open_policy {
            FileOpenPolicy::Blocked { reason } => {
                return Err(ExplorerError::BlockedPath(reason.clone()));
            }
            FileOpenPolicy::LauncherRequired { kind, reason } => {
                state.error = Some(
                    msg!(
                        "app-explorer-launcher-required",
                        reason = reason.clone(),
                        kind = kind.label()
                    )
                    .into(),
                );
                state.message = None;
                return Ok(ExplorerEffect::OpenRequested(ExplorerOpenRequest {
                    path: entry.path.clone(),
                    target: ExplorerOpenTarget::Launcher,
                }));
            }
            FileOpenPolicy::SystemDefault => {}
        }

        if entry.kind == ExplorerEntryKind::Directory {
            self.navigate_directory(state, session, platform, storage, entry.path.clone(), true)?;
            return Ok(ExplorerEffect::None);
        }

        let target = resolver.route(&entry.path, &entry.attributes);
        let permission = if target == ExplorerOpenTarget::Editor {
            PermissionAction::ReadFile
        } else {
            PermissionAction::OpenExternal
        };
        self.authorize(session, permission, &entry.path)?;
        Ok(ExplorerEffect::OpenRequested(ExplorerOpenRequest {
            path: entry.path.clone(),
            target,
        }))
    }

    pub(super) fn create_folder(
        &self,
        state: &mut ExplorerState,
        session: Option<&AuthSession>,
        platform: &dyn Platform,
        storage: &StorageManager,
        name: &str,
    ) -> Result<(), ExplorerError> {
        let path = child_path(&state.current_path, name)?;
        self.authorize(session, PermissionAction::WriteFile, &path)?;
        fs::create_dir(&path).map_err(|error| {
            ExplorerError::Platform(PlatformError::from_io(
                "create folder",
                Some(path.clone()),
                &error,
            ))
        })?;
        state.set_success(msg!(
            "app-explorer-created-folder",
            path = path.display().to_string()
        ));
        self.refresh(state, session, platform, storage)
    }

    pub(super) fn create_text_file(
        &self,
        state: &mut ExplorerState,
        session: Option<&AuthSession>,
        platform: &dyn Platform,
        storage: &StorageManager,
        name: &str,
    ) -> Result<(), ExplorerError> {
        let path = child_path(&state.current_path, name)?;
        self.authorize(session, PermissionAction::WriteFile, &path)?;
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .and_then(|file| file.sync_all())
            .map_err(|error| {
                ExplorerError::Platform(PlatformError::from_io(
                    "create text file",
                    Some(path.clone()),
                    &error,
                ))
            })?;
        state.set_success(msg!(
            "app-explorer-created-file",
            path = path.display().to_string()
        ));
        self.refresh(state, session, platform, storage)
    }

    pub(super) fn rename(
        &self,
        state: &mut ExplorerState,
        session: Option<&AuthSession>,
        platform: &dyn Platform,
        storage: &StorageManager,
        path: &Path,
        name: &str,
    ) -> Result<(), ExplorerError> {
        let parent = path.parent().ok_or_else(|| {
            ExplorerError::Localized(LocalizedError::new(
                "EXPLORER_INVALID_OPERATION",
                msg!("app-explorer-selected-no-parent"),
            ))
        })?;
        let target = child_path(parent, name)?;
        self.authorize(session, PermissionAction::WriteFile, path)?;
        platform.rename_path(path, &target)?;
        state.set_success(msg!(
            "app-explorer-renamed",
            path = target.display().to_string()
        ));
        self.refresh(state, session, platform, storage)
    }

    pub(super) fn delete_many_to_trash(
        &self,
        state: &mut ExplorerState,
        session: Option<&AuthSession>,
        platform: &dyn Platform,
        storage: &StorageManager,
        paths: &[PathBuf],
    ) -> Result<(), ExplorerError> {
        for path in paths {
            self.authorize(session, PermissionAction::DeleteFile, path)?;
        }
        state.operation = Some(ExplorerOperationProgress {
            operation: ExplorerTaskOperation::DeleteToTrash,
            phase: ExplorerOperationPhase::Executing,
            label: msg!("app-explorer-trashing").into(),
            completed_items: 0,
            total_items: Some(paths.len()),
            completed_bytes: 0,
            total_bytes: None,
            cancellable: false,
        });
        if let Err(error) = platform.move_to_trash(paths) {
            state.operation = None;
            // Native Trash APIs may report a partial operation. The old confirmation snapshot is
            // therefore no longer safe to replay blindly; refresh and require a new selection.
            if state
                .pending_dialog
                .as_ref()
                .is_some_and(|dialog| dialog.kind == ExplorerDialogKind::DeleteToTrash)
            {
                state.pending_dialog = None;
            }
            if let Err(refresh_error) = self.refresh(state, session, platform, storage) {
                log_secondary_explorer(
                    "refresh_after_trash_failure",
                    session,
                    &refresh_error,
                    None,
                );
            }
            return Err(error.into());
        }

        state.operation = None;
        state.pending_dialog = None;
        state.clear_selection();
        if let Err(error) = self.refresh(state, session, platform, storage) {
            state.message = None;
            state.error = Some(
                msg!(
                    "app-explorer-trash-refresh-failed",
                    count = paths.len() as i64,
                    error = error.to_string()
                )
                .into(),
            );
        } else {
            state.set_success(msg!("app-explorer-trashed", count = paths.len() as i64));
        }
        Ok(())
    }

    pub(super) fn prepare_restore(
        &self,
        state: &mut ExplorerState,
        session: Option<&AuthSession>,
        platform: &dyn Platform,
        storage: &StorageManager,
        entry: &ExplorerEntry,
        target: PathBuf,
    ) -> Result<(), ExplorerError> {
        if !target.is_absolute() {
            return Err(ExplorerError::Localized(LocalizedError::new(
                "EXPLORER_INVALID_OPERATION",
                msg!("app-explorer-restore-absolute-required"),
            )));
        }
        let parent = target.parent().ok_or_else(|| {
            ExplorerError::Localized(LocalizedError::new(
                "EXPLORER_INVALID_OPERATION",
                msg!("app-explorer-restore-no-parent-directory"),
            ))
        })?;
        let parent_attributes = platform.file_attributes(parent)?;
        if !parent_attributes.is_dir {
            return Err(ExplorerError::Localized(LocalizedError::new(
                "EXPLORER_INVALID_OPERATION",
                msg!(
                    "app-explorer-restore-parent-not-directory",
                    path = parent.display().to_string()
                ),
            )));
        }
        self.authorize(session, PermissionAction::WriteFile, &target)?;
        let id = entry.trash_id.clone().ok_or_else(|| {
            ExplorerError::Localized(LocalizedError::new(
                "EXPLORER_INVALID_OPERATION",
                msg!("app-explorer-trash-no-identity"),
            ))
        })?;
        if path_exists_no_follow(&target)? {
            state.pending_restore = Some(ExplorerPendingRestore {
                id,
                display_name: entry.name.clone(),
                target,
            });
            return Ok(());
        }
        let restore_target = if entry.original_path.as_ref() == Some(&target) {
            TrashRestoreTarget::OriginalLocation
        } else {
            TrashRestoreTarget::DestinationPath(target)
        };
        self.perform_restore(state, session, platform, storage, &id, restore_target)
    }

    pub(super) fn resolve_restore_conflict(
        &self,
        state: &mut ExplorerState,
        session: Option<&AuthSession>,
        platform: &dyn Platform,
        storage: &StorageManager,
        action: ExplorerConflictAction,
    ) -> Result<(), ExplorerError> {
        let pending = state.pending_restore.clone().ok_or_else(|| {
            ExplorerError::Localized(LocalizedError::new(
                "EXPLORER_INVALID_OPERATION",
                msg!("app-explorer-no-restore-conflict"),
            ))
        })?;
        match action {
            ExplorerConflictAction::Cancel | ExplorerConflictAction::Skip => {
                state.pending_restore = None;
                state.set_success(msg!("app-explorer-restore-cancelled"));
                Ok(())
            }
            ExplorerConflictAction::KeepBoth => {
                let target = unique_sibling_path(&pending.target)?;
                state.pending_restore = None;
                self.perform_restore(
                    state,
                    session,
                    platform,
                    storage,
                    &pending.id,
                    TrashRestoreTarget::DestinationPath(target),
                )
            }
            ExplorerConflictAction::Replace => {
                // The native restore consumes the Trash identity on success. Clear the pending
                // action before entering the transaction so a refresh/rollback warning can
                // never replay a now-stale identity.
                state.pending_restore = None;
                self.replace_with_restored_item(state, session, platform, storage, &pending)
            }
        }
    }

    pub(super) fn replace_with_restored_item(
        &self,
        state: &mut ExplorerState,
        session: Option<&AuthSession>,
        platform: &dyn Platform,
        storage: &StorageManager,
        pending: &ExplorerPendingRestore,
    ) -> Result<(), ExplorerError> {
        let parent = pending.target.parent().ok_or_else(|| {
            ExplorerError::Localized(LocalizedError::new(
                "EXPLORER_INVALID_OPERATION",
                msg!("app-explorer-restore-no-parent"),
            ))
        })?;
        let backup_dir = create_restore_rollback_directory(parent)?;
        let backup = backup_dir.join(pending.target.file_name().ok_or_else(|| {
            ExplorerError::Localized(LocalizedError::new(
                "EXPLORER_INVALID_OPERATION",
                msg!("app-explorer-target-no-name"),
            ))
        })?);
        if let Err(error) = platform.rename_path(&pending.target, &backup) {
            if let Err(cleanup_error) = fs::remove_dir(&backup_dir) {
                log_secondary_explorer(
                    "restore_cleanup",
                    session,
                    &cleanup_error,
                    Some(&backup_dir),
                );
            }
            return Err(error.into());
        }
        let restored = match platform.restore_trash_item(
            &pending.id,
            TrashRestoreTarget::DestinationPath(pending.target.clone()),
        ) {
            Ok(restored) => restored,
            Err(error) => {
                return match platform.rename_path(&backup, &pending.target) {
                    Ok(()) => {
                        if let Err(cleanup_error) = fs::remove_dir(&backup_dir) {
                            log_secondary_explorer(
                                "restore_cleanup",
                                session,
                                &cleanup_error,
                                Some(&backup_dir),
                            );
                        }
                        Err(error.into())
                    }
                    Err(rollback_error) => Err(ExplorerError::Localized(LocalizedError::new(
                        "EXPLORER_RESTORE_ROLLBACK",
                        msg!(
                            "app-explorer-restore-rollback-failed",
                            error = error.to_string(),
                            backup = backup.display().to_string(),
                            target = pending.target.display().to_string(),
                            rollback = rollback_error.to_string()
                        ),
                    ))),
                };
            }
        };
        if let Err(error) = platform.move_to_trash(std::slice::from_ref(&backup)) {
            // The replacement is already restored. Park it beside the backup before rolling the
            // previous target back; this ensures neither version is overwritten even if a native
            // Trash or rename operation fails midway through recovery.
            let rescued = backup_dir.join("restored-item");
            if let Err(park_error) = platform.rename_path(&restored, &rescued) {
                return Err(ExplorerError::Localized(LocalizedError::new(
                    "EXPLORER_RESTORE_PARK",
                    msg!(
                        "app-explorer-restore-park-failed",
                        restored = restored.display().to_string(),
                        backup = backup.display().to_string(),
                        error = error.to_string(),
                        park = park_error.to_string()
                    ),
                )));
            }
            if let Err(rollback_error) = platform.rename_path(&backup, &pending.target) {
                let rescue_rollback = platform.rename_path(&rescued, &pending.target);
                if let Err(rescue_error) = &rescue_rollback {
                    log_secondary_explorer(
                        "restore_rescue_rollback",
                        session,
                        rescue_error,
                        Some(&rescued),
                    );
                }
                let preserved = if rescue_rollback.is_ok() {
                    &pending.target
                } else {
                    &rescued
                };
                return Err(ExplorerError::Localized(LocalizedError::new(
                    "EXPLORER_RESTORE_RESCUE",
                    msg!(
                        "app-explorer-restore-rescue-failed",
                        error = error.to_string(),
                        backup = backup.display().to_string(),
                        target = pending.target.display().to_string(),
                        rollback = rollback_error.to_string(),
                        preserved = preserved.display().to_string()
                    ),
                )));
            }
            if let Err(retrash_error) = platform.move_to_trash(std::slice::from_ref(&rescued)) {
                return Err(ExplorerError::Localized(LocalizedError::new(
                    "EXPLORER_RESTORE_RETRASH",
                    msg!(
                        "app-explorer-restore-retrash-failed",
                        target = pending.target.display().to_string(),
                        error = retrash_error.to_string(),
                        preserved = rescued.display().to_string()
                    ),
                )));
            }
            if let Err(cleanup_error) = fs::remove_dir(&backup_dir) {
                log_secondary_explorer(
                    "restore_cleanup",
                    session,
                    &cleanup_error,
                    Some(&backup_dir),
                );
            }
            return Err(error.into());
        }
        if let Err(cleanup_error) = fs::remove_dir(&backup_dir) {
            log_secondary_explorer(
                "restore_cleanup",
                session,
                &cleanup_error,
                Some(&backup_dir),
            );
        }
        self.finish_restore_commit(state, session, platform, storage, restored)
    }

    pub(super) fn perform_restore(
        &self,
        state: &mut ExplorerState,
        session: Option<&AuthSession>,
        platform: &dyn Platform,
        storage: &StorageManager,
        id: &TrashEntryId,
        target: TrashRestoreTarget,
    ) -> Result<(), ExplorerError> {
        let restored = platform.restore_trash_item(id, target)?;
        state.pending_restore = None;
        self.finish_restore_commit(state, session, platform, storage, restored)
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn finish_restore_commit(
        &self,
        state: &mut ExplorerState,
        session: Option<&AuthSession>,
        platform: &dyn Platform,
        storage: &StorageManager,
        restored: PathBuf,
    ) -> Result<(), ExplorerError> {
        if let Err(error) = self.refresh(state, session, platform, storage) {
            state.message = None;
            state.error = Some(
                msg!(
                    "app-explorer-restore-refresh-failed",
                    path = restored.display().to_string(),
                    error = error.to_string()
                )
                .into(),
            );
        } else {
            state.set_success(msg!(
                "app-explorer-restored",
                path = restored.display().to_string()
            ));
        }
        Ok(())
    }

    pub(super) fn paste(
        &self,
        state: &mut ExplorerState,
        session: Option<&AuthSession>,
        platform: &dyn Platform,
        storage: &StorageManager,
        clipboard: ExplorerClipboard,
    ) -> Result<(), ExplorerError> {
        let destination = state.current_path.clone();
        self.start_transfer(state, session, platform, storage, clipboard, destination)
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn start_transfer(
        &self,
        state: &mut ExplorerState,
        session: Option<&AuthSession>,
        platform: &dyn Platform,
        storage: &StorageManager,
        clipboard: ExplorerClipboard,
        destination: PathBuf,
    ) -> Result<(), ExplorerError> {
        if clipboard.paths.is_empty() {
            return Err(ExplorerError::Localized(LocalizedError::new(
                "EXPLORER_INVALID_OPERATION",
                msg!("app-explorer-clipboard-empty"),
            )));
        }
        let mut conflicts = Vec::new();
        for source in &clipboard.paths {
            let file_name = source.file_name().ok_or_else(|| {
                ExplorerError::Localized(LocalizedError::new(
                    "EXPLORER_INVALID_OPERATION",
                    msg!("app-explorer-clipboard-no-name"),
                ))
            })?;
            let target = destination.join(file_name);
            if !(clipboard.mode == ExplorerClipboardMode::Copy
                && source.as_path() == target.as_path())
            {
                validate_transfer_destination(source, &target)?;
            }
            let permission = match clipboard.mode {
                ExplorerClipboardMode::Copy => PermissionAction::WriteFile,
                ExplorerClipboardMode::Cut => PermissionAction::MoveFile,
            };
            self.authorize(session, permission, &target)?;
            if path_exists_no_follow(&target)? {
                conflicts.push((source.clone(), target));
            }
        }

        if state.confirm_name_conflicts && !conflicts.is_empty() {
            let (source, target) = conflicts[0].clone();
            state.pending_conflict = Some(ExplorerConflict {
                source,
                target,
                remaining: conflicts.len(),
            });
            let operation = clipboard.mode.into();
            state.pending_transfer = Some(ExplorerPendingTransfer {
                clipboard,
                destination,
                conflicts,
                current_conflict: 0,
                resolutions: BTreeMap::new(),
            });
            state.operation = Some(ExplorerOperationProgress {
                operation,
                phase: ExplorerOperationPhase::WaitingForConflict,
                label: msg!("app-explorer-waiting-conflict").into(),
                completed_items: 0,
                total_items: None,
                completed_bytes: 0,
                total_bytes: None,
                cancellable: true,
            });
            return Ok(());
        }

        let resolutions = conflicts
            .into_iter()
            .map(|(_, target)| (target, ExplorerConflictAction::KeepBoth))
            .collect();
        self.execute_transfer(
            state,
            session,
            platform,
            storage,
            clipboard,
            destination,
            resolutions,
        )
    }

    pub(super) fn resolve_pending_transfer(
        &self,
        state: &mut ExplorerState,
        session: Option<&AuthSession>,
        platform: &dyn Platform,
        storage: &StorageManager,
        action: ExplorerConflictAction,
        apply_to_all: bool,
    ) -> Result<(), ExplorerError> {
        if action == ExplorerConflictAction::Cancel {
            state.pending_conflict = None;
            state.pending_transfer = None;
            state.operation = None;
            state.set_success(msg!("app-explorer-transfer-cancelled"));
            return Ok(());
        }

        let Some(pending) = state.pending_transfer.as_mut() else {
            return Err(ExplorerError::Localized(LocalizedError::new(
                "EXPLORER_INVALID_OPERATION",
                msg!("app-explorer-no-transfer-conflict"),
            )));
        };
        let (_, target) = pending
            .conflicts
            .get(pending.current_conflict)
            .cloned()
            .ok_or_else(|| {
                ExplorerError::Localized(LocalizedError::new(
                    "EXPLORER_INVALID_OPERATION",
                    msg!("app-explorer-invalid-conflict"),
                ))
            })?;
        pending.resolutions.insert(target, action);
        if apply_to_all {
            for (_, target) in pending.conflicts.iter().skip(pending.current_conflict + 1) {
                pending.resolutions.insert(target.clone(), action);
            }
            pending.current_conflict = pending.conflicts.len();
        } else {
            pending.current_conflict += 1;
        }

        if let Some((source, target)) = pending.conflicts.get(pending.current_conflict).cloned() {
            state.pending_conflict = Some(ExplorerConflict {
                source,
                target,
                remaining: pending.conflicts.len() - pending.current_conflict,
            });
            return Ok(());
        }

        let pending = state
            .pending_transfer
            .take()
            .expect("pending transfer checked above");
        state.pending_conflict = None;
        self.execute_transfer(
            state,
            session,
            platform,
            storage,
            pending.clipboard,
            pending.destination,
            pending.resolutions,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn execute_transfer(
        &self,
        state: &mut ExplorerState,
        session: Option<&AuthSession>,
        platform: &dyn Platform,
        storage: &StorageManager,
        clipboard: ExplorerClipboard,
        destination: PathBuf,
        resolutions: BTreeMap<PathBuf, ExplorerConflictAction>,
    ) -> Result<(), ExplorerError> {
        state.operation = Some(ExplorerOperationProgress {
            operation: clipboard.mode.into(),
            phase: ExplorerOperationPhase::Executing,
            label: match clipboard.mode {
                ExplorerClipboardMode::Copy => msg!("app-explorer-copying").into(),
                ExplorerClipboardMode::Cut => msg!("app-explorer-moving").into(),
            },
            completed_items: 0,
            total_items: Some(clipboard.paths.len()),
            completed_bytes: 0,
            total_bytes: None,
            cancellable: true,
        });

        let mut succeeded = Vec::new();
        let mut skipped = 0usize;
        for (index, source) in clipboard.paths.iter().enumerate() {
            let file_name = source.file_name().ok_or_else(|| {
                ExplorerError::Localized(LocalizedError::new(
                    "EXPLORER_INVALID_OPERATION",
                    msg!("app-explorer-clipboard-no-name"),
                ))
            })?;
            let original_target = destination.join(file_name);
            let resolution = resolutions
                .get(&original_target)
                .copied()
                .unwrap_or(ExplorerConflictAction::KeepBoth);
            let target_exists = path_exists_no_follow(&original_target)?;
            if resolution == ExplorerConflictAction::Skip {
                skipped += 1;
                if let Some(operation) = state.operation.as_mut() {
                    operation.completed_items = index + 1;
                }
                continue;
            }
            let target = if target_exists && resolution == ExplorerConflictAction::KeepBoth {
                unique_sibling_path(&original_target)?
            } else {
                original_target.clone()
            };
            if target_exists && resolution == ExplorerConflictAction::Replace {
                move_existing_to_trash(platform, &original_target)?;
            }

            match clipboard.mode {
                ExplorerClipboardMode::Copy => {
                    copy_path_staged(source, &target)?;
                }
                ExplorerClipboardMode::Cut => match platform.rename_path(source, &target) {
                    Ok(()) => {}
                    Err(PlatformError::CrossDevice { .. }) => {
                        copy_path_staged(source, &target)?;
                        remove_source_path(source)?;
                    }
                    Err(error) => return Err(error.into()),
                },
            }
            succeeded.push(target);
            if let Some(operation) = state.operation.as_mut() {
                operation.completed_items = index + 1;
            }
        }

        if clipboard.mode == ExplorerClipboardMode::Cut {
            state.clipboard = None;
        }
        state.operation = None;
        state.selected_paths = succeeded.into_iter().collect();
        state.set_success(msg!(
            "app-explorer-transferred",
            count = state.selected_paths.len() as i64,
            skipped = skipped as i64
        ));

        self.refresh(state, session, platform, storage)
    }

    pub(super) fn authorize(
        &self,
        session: Option<&AuthSession>,
        action: PermissionAction,
        resource: &Path,
    ) -> Result<(), ExplorerError> {
        let resource_display = resource.display().to_string();
        let authorization =
            self.permission_service
                .authorize(session, action, Some(resource_display.as_str()));
        if authorization.allowed {
            return Ok(());
        }

        let reason = authorization
            .reason
            .unwrap_or_else(|| "permission_denied".to_string());
        Err(ExplorerError::PermissionDenied {
            action,
            reason,
            path: resource.to_path_buf(),
        })
    }
}
