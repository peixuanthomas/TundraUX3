use super::*;

impl ExplorerController {
    pub fn new(file_service: ExplorerFileService) -> Self {
        Self {
            file_service,
            open_resolver: Arc::new(EditorAwareOpenRouteResolver::default()),
        }
    }

    pub fn with_open_resolver(mut self, resolver: Arc<dyn ExplorerOpenRouteResolver>) -> Self {
        self.open_resolver = resolver;
        self
    }

    pub fn with_editor_extensions(mut self, extensions: Vec<String>) -> Self {
        self.open_resolver = Arc::new(EditorAwareOpenRouteResolver::new(extensions));
        self
    }

    pub fn apply(
        &self,
        state: &mut ExplorerState,
        command: ExplorerCommand,
        session: Option<&AuthSession>,
        platform: &dyn Platform,
        storage: &StorageManager,
    ) -> ExplorerEffect {
        state.error = None;
        let operation = match &command {
            ExplorerCommand::Refresh => Some("refresh"),
            ExplorerCommand::Navigate(_)
            | ExplorerCommand::NavigateTrash
            | ExplorerCommand::OpenParent
            | ExplorerCommand::OpenBack
            | ExplorerCommand::OpenForward => Some("navigate"),
            ExplorerCommand::OpenSelected => Some("open"),
            ExplorerCommand::NewFolder(_) => Some("create_directory"),
            ExplorerCommand::NewTextFile(_) => Some("create_file"),
            ExplorerCommand::Rename(_) => Some("rename"),
            _ => None,
        };
        let log = operation.map(|operation| {
            let mut context = watchdog::AppWatchdog::current()
                .map(|app| app.log_context(operation))
                .unwrap_or_default();
            context.module = "ux.explorer".into();
            context.app = "explorer".into();
            context.operation = operation.into();
            context.owner_id = session.map(|session| session.user_id.clone());
            context.operation_id =
                watchdog::ProcessWatchdog::global().map(|process| process.new_log_operation_id());
            let mut event = runtime_log::RuntimeLogEvent::new(
                context,
                runtime_log::LogLevel::Info,
                runtime_log::LogPhase::Started,
                "Explorer operation started",
            );
            event.source_path = Some(state.current_path.clone());
            event.target_path = match &command {
                ExplorerCommand::Navigate(path) => Some(path.clone()),
                ExplorerCommand::NewFolder(name)
                | ExplorerCommand::NewTextFile(name)
                | ExplorerCommand::Rename(name) => Some(state.current_path.join(name)),
                _ => None,
            };
            emit_explorer_event(event.clone());
            event
        });
        let result = self.try_apply(state, command, session, platform, storage);
        if let Some(started) = log {
            let mut event = runtime_log::RuntimeLogEvent::new(
                started.context,
                if result.is_ok() {
                    runtime_log::LogLevel::Info
                } else {
                    runtime_log::LogLevel::Error
                },
                if result.is_ok() {
                    runtime_log::LogPhase::Succeeded
                } else {
                    runtime_log::LogPhase::Failed
                },
                "Explorer operation result",
            );
            event.source_path = started.source_path;
            event.target_path = started.target_path;
            if let Err(error) = &result {
                watchdog::capture_error(&mut event, error);
                if let ExplorerError::Platform(error) = error {
                    event.os_error_code = error.raw_os_error().map(i64::from);
                }
                event.error_code = Some("UX_EXPLORER_OPERATION_FAILED".into());
            }
            emit_explorer_event(event);
        }
        match result {
            Ok(effect) => {
                state.clamp_selection();
                effect
            }
            Err(error) => {
                state.set_error(error);
                ExplorerEffect::None
            }
        }
    }

    pub(super) fn try_apply(
        &self,
        state: &mut ExplorerState,
        command: ExplorerCommand,
        session: Option<&AuthSession>,
        platform: &dyn Platform,
        storage: &StorageManager,
    ) -> Result<ExplorerEffect, ExplorerError> {
        let effect = match command {
            ExplorerCommand::Refresh => {
                self.file_service
                    .refresh(state, session, platform, storage)?;
                ExplorerEffect::None
            }
            ExplorerCommand::SelectNext => {
                if !state.entries.is_empty() {
                    let index = (state.selected_index + 1).min(state.entries.len() - 1);
                    state.select_index(index, ExplorerSelectionMode::Replace);
                }
                ExplorerEffect::None
            }
            ExplorerCommand::SelectPrevious => {
                let index = state.selected_index.saturating_sub(1);
                state.select_index(index, ExplorerSelectionMode::Replace);
                ExplorerEffect::None
            }
            ExplorerCommand::SelectIndex(index) => {
                state.select_index(index, ExplorerSelectionMode::Replace);
                ExplorerEffect::None
            }
            ExplorerCommand::SelectIndexWithMode(index, mode) => {
                state.select_index(index, mode);
                ExplorerEffect::None
            }
            ExplorerCommand::SelectAll => {
                state.select_all();
                ExplorerEffect::None
            }
            ExplorerCommand::InvertSelection => {
                state.invert_selection();
                ExplorerEffect::None
            }
            ExplorerCommand::ClearSelection => {
                state.clear_selection();
                ExplorerEffect::None
            }
            ExplorerCommand::ToggleFocused => {
                let index = state.selected_index;
                state.select_index(index, ExplorerSelectionMode::Toggle);
                ExplorerEffect::None
            }
            ExplorerCommand::Search(query) => {
                state.query = query;
                state.apply_projection();
                ExplorerEffect::None
            }
            ExplorerCommand::ToggleHidden => {
                state.show_hidden = !state.show_hidden;
                state.apply_projection();
                state.set_success(if state.show_hidden {
                    msg!("app-explorer-hidden-visible")
                } else {
                    msg!("app-explorer-hidden-hidden")
                });
                ExplorerEffect::PersistConfig(state.to_config())
            }
            ExplorerCommand::ToggleSystem => {
                state.show_system = !state.show_system;
                state.apply_projection();
                ExplorerEffect::PersistConfig(state.to_config())
            }
            ExplorerCommand::ToggleExtensions => {
                state.show_extensions = !state.show_extensions;
                ExplorerEffect::PersistConfig(state.to_config())
            }
            ExplorerCommand::ToggleFoldersFirst => {
                state.folders_first = !state.folders_first;
                state.apply_projection();
                ExplorerEffect::PersistConfig(state.to_config())
            }
            ExplorerCommand::ToggleCaseSensitiveSort => {
                state.case_sensitive_sort = !state.case_sensitive_sort;
                state.apply_projection();
                ExplorerEffect::PersistConfig(state.to_config())
            }
            ExplorerCommand::ToggleSidebar => {
                state.show_sidebar = !state.show_sidebar;
                ExplorerEffect::PersistConfig(state.to_config())
            }
            ExplorerCommand::SetSort(field) => {
                if state.sort_field == field {
                    state.sort_direction = match state.sort_direction {
                        ExplorerSortDirection::Ascending => ExplorerSortDirection::Descending,
                        ExplorerSortDirection::Descending => ExplorerSortDirection::Ascending,
                    };
                } else {
                    state.sort_field = field;
                    state.sort_direction = if field == ExplorerSortField::Modified {
                        ExplorerSortDirection::Descending
                    } else {
                        ExplorerSortDirection::Ascending
                    };
                }
                state.apply_projection();
                ExplorerEffect::PersistConfig(state.to_config())
            }
            ExplorerCommand::ToggleSizeFormat => {
                state.size_format = match state.size_format {
                    ExplorerSizeFormat::HumanBinary => ExplorerSizeFormat::Bytes,
                    ExplorerSizeFormat::Bytes => ExplorerSizeFormat::HumanBinary,
                };
                ExplorerEffect::PersistConfig(state.to_config())
            }
            ExplorerCommand::ToggleDateZone => {
                state.date_zone = match state.date_zone {
                    ExplorerDateZone::ConfiguredTimezone => ExplorerDateZone::Utc,
                    ExplorerDateZone::Utc => ExplorerDateZone::ConfiguredTimezone,
                };
                ExplorerEffect::PersistConfig(state.to_config())
            }
            ExplorerCommand::ToggleDeleteConfirmation => {
                state.confirm_delete = !state.confirm_delete;
                ExplorerEffect::PersistConfig(state.to_config())
            }
            ExplorerCommand::ToggleConflictConfirmation => {
                state.confirm_name_conflicts = !state.confirm_name_conflicts;
                ExplorerEffect::PersistConfig(state.to_config())
            }
            ExplorerCommand::OpenParent => {
                ensure_filesystem_location(state)?;
                let parent = state
                    .current_path
                    .parent()
                    .ok_or_else(|| {
                        ExplorerError::Localized(LocalizedError::new(
                            "EXPLORER_INVALID_OPERATION",
                            msg!("app-explorer-no-parent"),
                        ))
                    })?
                    .to_path_buf();
                self.file_service
                    .navigate_directory(state, session, platform, storage, parent, true)?;
                ExplorerEffect::None
            }
            ExplorerCommand::OpenBack => {
                let target = state.back_history.last().cloned().ok_or_else(|| {
                    ExplorerError::Localized(LocalizedError::new(
                        "EXPLORER_INVALID_OPERATION",
                        msg!("app-explorer-no-back-history"),
                    ))
                })?;
                let previous = state.current_location.clone();
                self.file_service
                    .navigate_location(state, session, platform, storage, target, false)?;
                state.back_history.pop();
                state.forward_history.push(previous);
                ExplorerEffect::None
            }
            ExplorerCommand::OpenForward => {
                let target = state.forward_history.last().cloned().ok_or_else(|| {
                    ExplorerError::Localized(LocalizedError::new(
                        "EXPLORER_INVALID_OPERATION",
                        msg!("app-explorer-no-forward-history"),
                    ))
                })?;
                let previous = state.current_location.clone();
                self.file_service
                    .navigate_location(state, session, platform, storage, target, false)?;
                state.forward_history.pop();
                state.back_history.push(previous);
                ExplorerEffect::None
            }
            ExplorerCommand::Navigate(path) => {
                self.file_service
                    .navigate_directory(state, session, platform, storage, path, true)?;
                ExplorerEffect::None
            }
            ExplorerCommand::NavigateTrash => {
                self.file_service.navigate_location(
                    state,
                    session,
                    platform,
                    storage,
                    ExplorerLocation::Trash,
                    true,
                )?;
                ExplorerEffect::None
            }
            ExplorerCommand::OpenSelected => {
                let Some(entry) = state.single_selected_entry().cloned() else {
                    return Ok(ExplorerEffect::None);
                };
                self.file_service.open_entry(
                    state,
                    session,
                    platform,
                    storage,
                    &entry,
                    self.open_resolver.as_ref(),
                )?
            }
            ExplorerCommand::NewFolder(name) => {
                ensure_filesystem_location(state)?;
                self.file_service
                    .create_folder(state, session, platform, storage, &name)?;
                ExplorerEffect::None
            }
            ExplorerCommand::NewTextFile(name) => {
                ensure_filesystem_location(state)?;
                self.file_service
                    .create_text_file(state, session, platform, storage, &name)?;
                ExplorerEffect::None
            }
            ExplorerCommand::Rename(name) => {
                ensure_filesystem_location(state)?;
                let paths = state.effective_selected_paths();
                if paths.len() != 1 {
                    return Err(ExplorerError::Localized(LocalizedError::new(
                        "EXPLORER_INVALID_OPERATION",
                        msg!("app-explorer-rename-selection"),
                    )));
                }
                self.file_service
                    .rename(state, session, platform, storage, &paths[0], &name)?;
                ExplorerEffect::None
            }
            ExplorerCommand::DeleteToTrash => {
                ensure_filesystem_location(state)?;
                let paths = selected_paths_or_error(state)?;
                if state.confirm_delete {
                    state.pending_dialog = Some(ExplorerDialog::delete_many(&paths));
                } else {
                    self.file_service
                        .delete_many_to_trash(state, session, platform, storage, &paths)?;
                }
                ExplorerEffect::None
            }
            ExplorerCommand::ConfirmDelete => {
                ensure_filesystem_location(state)?;
                let dialog = state.pending_dialog.clone().ok_or_else(|| {
                    ExplorerError::Localized(LocalizedError::new(
                        "EXPLORER_INVALID_OPERATION",
                        msg!("app-explorer-no-delete-confirmation"),
                    ))
                })?;
                if dialog.kind != ExplorerDialogKind::DeleteToTrash || dialog.targets.is_empty() {
                    return Err(ExplorerError::Localized(LocalizedError::new(
                        "EXPLORER_INVALID_OPERATION",
                        msg!("app-explorer-wrong-delete-confirmation"),
                    )));
                }
                self.file_service.delete_many_to_trash(
                    state,
                    session,
                    platform,
                    storage,
                    &dialog.targets,
                )?;
                ExplorerEffect::None
            }
            ExplorerCommand::DumpTrash => {
                ensure_trash_location(state)?;
                state.pending_dialog = Some(ExplorerDialog::dump_trash(state.entries.len()));
                ExplorerEffect::None
            }
            ExplorerCommand::ConfirmDumpTrash => {
                ensure_trash_location(state)?;
                let dialog = state.pending_dialog.as_ref().ok_or_else(|| {
                    ExplorerError::Localized(LocalizedError::new(
                        "EXPLORER_INVALID_OPERATION",
                        msg!("app-explorer-no-dump-confirmation"),
                    ))
                })?;
                if dialog.kind != ExplorerDialogKind::DumpTrash {
                    return Err(ExplorerError::Localized(LocalizedError::new(
                        "EXPLORER_INVALID_OPERATION",
                        msg!("app-explorer-wrong-dump-confirmation"),
                    )));
                }
                if let Err(error) = platform.empty_trash() {
                    // Emptying may be partially completed by the native shell. Never leave a
                    // confirmation that can replay against a now-different Trash snapshot.
                    state.pending_dialog = None;
                    if let Err(refresh_error) =
                        self.file_service.refresh(state, session, platform, storage)
                    {
                        log_secondary_explorer(
                            "refresh_after_trash_failure",
                            session,
                            &refresh_error,
                            None,
                        );
                    }
                    return Err(error.into());
                }
                state.pending_dialog = None;
                match self.file_service.refresh(state, session, platform, storage) {
                    Ok(()) => state.set_success(msg!("app-explorer-trash-emptied")),
                    Err(error) => {
                        state.message = None;
                        state.error = Some(
                            msg!(
                                "app-explorer-dump-refresh-failed",
                                error = error.to_string()
                            )
                            .into(),
                        );
                    }
                }
                ExplorerEffect::None
            }
            ExplorerCommand::RestoreSelected => {
                ensure_trash_location(state)?;
                let entry = selected_trash_entry(state)?;
                let target = entry.original_path.clone().ok_or_else(|| {
                    ExplorerError::Localized(LocalizedError::new(
                        "EXPLORER_INVALID_OPERATION",
                        msg!("app-explorer-no-original-path"),
                    ))
                })?;
                self.file_service
                    .prepare_restore(state, session, platform, storage, &entry, target)?;
                ExplorerEffect::None
            }
            ExplorerCommand::RestoreSelectedToDirectory(directory) => {
                ensure_trash_location(state)?;
                let entry = selected_trash_entry(state)?;
                let target = restore_target_in_directory(&directory, &entry.name)?;
                self.file_service
                    .prepare_restore(state, session, platform, storage, &entry, target)?;
                ExplorerEffect::None
            }
            ExplorerCommand::ResolveRestoreConflict(action) => {
                ensure_trash_location(state)?;
                self.file_service
                    .resolve_restore_conflict(state, session, platform, storage, action)?;
                ExplorerEffect::None
            }
            ExplorerCommand::Copy => {
                ensure_filesystem_location(state)?;
                let paths = selected_paths_or_error(state)?;
                state.clipboard = Some(ExplorerClipboard {
                    paths,
                    mode: ExplorerClipboardMode::Copy,
                });
                state.set_success(msg!("app-explorer-copied-selection"));
                ExplorerEffect::None
            }
            ExplorerCommand::Cut => {
                ensure_filesystem_location(state)?;
                let paths = selected_paths_or_error(state)?;
                state.clipboard = Some(ExplorerClipboard {
                    paths,
                    mode: ExplorerClipboardMode::Cut,
                });
                state.set_success(msg!("app-explorer-cut-selection"));
                ExplorerEffect::None
            }
            ExplorerCommand::Paste => {
                ensure_filesystem_location(state)?;
                let clipboard = state.clipboard.clone().ok_or_else(|| {
                    ExplorerError::Localized(LocalizedError::new(
                        "EXPLORER_INVALID_OPERATION",
                        msg!("app-explorer-clipboard-empty"),
                    ))
                })?;
                self.file_service
                    .paste(state, session, platform, storage, clipboard)?;
                ExplorerEffect::None
            }
            ExplorerCommand::BeginDrag => {
                ensure_filesystem_location(state)?;
                let sources = selected_paths_or_error(state)?;
                state.drag = Some(ExplorerDragState {
                    sources,
                    target: None,
                    mode: ExplorerTransferMode::Move,
                    active: false,
                });
                ExplorerEffect::None
            }
            ExplorerCommand::UpdateDrag { target, mode } => {
                ensure_filesystem_location(state)?;
                if let Some(drag) = state.drag.as_mut() {
                    drag.target = target;
                    drag.mode = mode;
                    drag.active = true;
                }
                ExplorerEffect::None
            }
            ExplorerCommand::DropDrag => {
                ensure_filesystem_location(state)?;
                let Some(drag) = state.drag.take() else {
                    return Ok(ExplorerEffect::None);
                };
                if drag.active {
                    let destination = drag.target.ok_or_else(|| {
                        ExplorerError::Localized(LocalizedError::new(
                            "EXPLORER_INVALID_OPERATION",
                            msg!("app-explorer-drag-no-destination"),
                        ))
                    })?;
                    self.file_service.start_transfer(
                        state,
                        session,
                        platform,
                        storage,
                        ExplorerClipboard {
                            paths: drag.sources,
                            mode: drag.mode.into(),
                        },
                        destination,
                    )?;
                }
                ExplorerEffect::None
            }
            ExplorerCommand::CancelDrag => {
                state.drag = None;
                ExplorerEffect::None
            }
            ExplorerCommand::ResolveConflict {
                action,
                apply_to_all,
            } => {
                ensure_filesystem_location(state)?;
                self.file_service.resolve_pending_transfer(
                    state,
                    session,
                    platform,
                    storage,
                    action,
                    apply_to_all,
                )?;
                ExplorerEffect::None
            }
            ExplorerCommand::CancelOperation => {
                if let Some(operation) = state.operation.as_mut() {
                    operation.phase = ExplorerOperationPhase::Cancelled;
                    operation.cancellable = false;
                }
                state.pending_conflict = None;
                state.pending_restore = None;
                ExplorerEffect::None
            }
        };

        Ok(effect)
    }
}
