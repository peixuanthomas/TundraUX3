use super::*;
use crate::session::*;

impl ShellSession {
    pub(in crate::session) fn reset_editor_recovery_identity(&mut self) -> bool {
        if !self.clear_editor_recovery() {
            return false;
        }
        self.pages.editor.editor_recovery_lease = None;
        self.pages.editor.editor_recovery_id = Default::default();
        self.pages.editor.editor_recovery_dirty_since = None;
        self.pages.editor.editor_last_recovery_write = None;
        true
    }

    pub(in crate::session) fn restore_editor_recovery_if_present(&mut self) {
        let Some((app_paths, user_key)) = self.editor_recovery_context() else {
            return;
        };
        let (lease, recovery) =
            match app::editor_recovery::claim_next_editor_recovery(&app_paths, user_key.as_str()) {
                Ok(Some(record)) => record,
                Ok(None) => return,
                Err(error) => {
                    self.report_editor_error(i18n::LocalizedText::from(i18n::msg!(
                        "shell-could-not-read-the-editor-recovery-error",
                        error = error.to_string()
                    )));
                    return;
                }
            };
        self.pages.editor.editor_recovery_id = lease.id().clone();
        self.pages.editor.editor_recovery_lease = Some(lease);

        let (mut state, fingerprint, unbound, warning) = match recovery {
            app::editor_recovery::VersionedEditorRecovery::V1(record) => {
                let kind = app::editor::DocumentKind::PlainText;
                let (mut state, fingerprint, unbound) =
                    editor_recovery_base(record.path.as_ref(), record.saved_content_hash, kind);
                state.install_source_draft(record.source, record.cursor, None);
                (state, fingerprint, unbound, None)
            }
            app::editor_recovery::VersionedEditorRecovery::V2(record) => {
                restore_editor_recovery_v2(record, None)
            }
            app::editor_recovery::VersionedEditorRecovery::V2Fallback { record, warning } => {
                restore_editor_recovery_v2(record, Some(warning))
            }
        };
        if unbound {
            state.document.path = None;
        }
        self.advance_editor_document_generation();
        self.app
            .dispatch_at(app::AppCommand::SetEditorState(Some(state)), Instant::now());
        self.pages.editor.editor_quick_menu_anchor = None;
        self.pages.editor.editor_table_column_widths.clear();
        self.pages.editor.editor_table_resize = None;
        self.pages.editor.editor_fingerprint = fingerprint;
        self.pages.editor.editor_recovery_dirty_since = Some(Instant::now());
        self.pages.editor.editor_message = Some(if let Some(warning) = warning {
            warning
        } else if unbound {
            i18n::LocalizedText::from(i18n::msg!(
                "shell-recovered-as-an-unbound-draft-because-the-original-file-changed-use-save-as"
            ))
        } else {
            i18n::LocalizedText::from(i18n::msg!("shell-recovered-an-unsaved-document"))
        });
        self.notify_toast(i18n::LocalizedText::from(i18n::msg!(
            "shell-recovered-an-unsaved-editor-document"
        )));
    }

    pub(in crate::session) fn persist_editor_recovery_if_due(&mut self, now: Instant) {
        if self
            .app
            .editor_state()
            .is_none_or(|state| !state.is_dirty())
        {
            return;
        }
        let Some(dirty_since) = self.pages.editor.editor_recovery_dirty_since else {
            self.pages.editor.editor_recovery_dirty_since = Some(now);
            return;
        };
        if now.saturating_duration_since(dirty_since) < EDITOR_RECOVERY_IDLE
            || self
                .pages
                .editor
                .editor_last_recovery_write
                .is_some_and(|last| now.saturating_duration_since(last) < EDITOR_RECOVERY_INTERVAL)
        {
            return;
        }
        let _ = self.persist_editor_recovery_now(now);
    }

    /// Writes the current dirty buffer without debounce. Interactive exit and
    /// logout paths use the return value to avoid destroying the only copy of
    /// unsaved text when recovery storage is unavailable.
    pub(in crate::session) fn persist_editor_recovery_now(&mut self, now: Instant) -> bool {
        let Some(state) = self.app.editor_state() else {
            return true;
        };
        if !state.is_dirty() {
            return true;
        }
        let Some((app_paths, user_key)) = self.editor_recovery_context() else {
            // Storage-free/debug shells do not have a durable per-user context.
            return true;
        };
        let document_kind = match state.document.kind {
            app::editor::DocumentKind::Markdown => {
                app::editor_recovery::RecoveryDocumentKind::Markdown
            }
            app::editor::DocumentKind::PlainText => {
                app::editor_recovery::RecoveryDocumentKind::PlainText
            }
        };
        let payload = if let Some(document) = state.rich_document() {
            let selection = state.rich_selection().map(|selection| {
                app::rich_document::RichRange::new(selection.anchor, selection.focus)
            });
            app::editor_recovery::EditorRecoveryPayload::Rich {
                document: document.clone(),
                cursor: state.rich_cursor(),
                selection,
                markdown_fallback: state.export_text(),
            }
        } else {
            app::editor_recovery::EditorRecoveryPayload::Source {
                text: state.source_buffer().unwrap_or_default().into_owned(),
                cursor: state.cursor.byte_offset,
                selection: state.selection.map(|selection| {
                    app::editor_recovery::RecoverySourceSelection {
                        anchor: selection.anchor,
                        focus: selection.focus,
                    }
                }),
            }
        };
        let mut record = app::editor_recovery::EditorRecoveryRecordV2 {
            path: state.document.path.clone(),
            document_kind,
            metadata: editor_recovery_metadata(state.document.metadata),
            saved_content_hash: self
                .pages
                .editor
                .editor_fingerprint
                .map(|value| value.content_hash),
            updated_at_epoch_ms: 0,
            payload,
        };
        record.updated_at_epoch_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_millis().min(u128::from(u64::MAX)) as u64)
            .unwrap_or_default();
        if self.pages.editor.editor_recovery_lease.is_none() {
            match app::editor_recovery::acquire_editor_recovery(
                &app_paths,
                user_key.as_str(),
                &self.pages.editor.editor_recovery_id,
            ) {
                Ok(lease) => self.pages.editor.editor_recovery_lease = Some(lease),
                Err(error) => {
                    self.report_editor_error(i18n::LocalizedText::from(i18n::msg!(
                        "shell-could-not-save-recovery-error",
                        error = error.to_string()
                    )));
                    return false;
                }
            }
        }
        let lease = self
            .pages
            .editor
            .editor_recovery_lease
            .as_ref()
            .expect("recovery was acquired");
        match app::editor_recovery::write_editor_recovery_v2(lease, &record) {
            Ok(()) => {
                self.pages.editor.editor_last_recovery_write = Some(now);
                true
            }
            Err(error) => {
                self.report_editor_error(i18n::LocalizedText::from(i18n::msg!(
                    "shell-could-not-save-recovery-error",
                    error = error.to_string()
                )));
                false
            }
        }
    }

    pub(in crate::session) fn editor_recovery_context(
        &self,
    ) -> Option<(platform::AppPaths, String)> {
        if self.config_editor_active()
            || self.pages.editor.editor_read_session.is_some()
            || self
                .app
                .editor_state()
                .is_some_and(EditorState::is_read_only)
            || self
                .app
                .editor_state()
                .and_then(|s| s.document.path.as_deref())
                .is_some_and(Self::is_system_config_path)
        {
            return None;
        }
        let storage = self.storage_manager.as_ref()?;
        let user_key = self.app.auth_session()?.user_id.clone();
        let app_paths = app_paths_from_storage_layout(storage.layout()).ok()?;
        Some((app_paths, user_key))
    }

    pub(in crate::session) fn clear_editor_recovery(&mut self) -> bool {
        if self.editor_recovery_context().is_some()
            && let Some(lease) = self.pages.editor.editor_recovery_lease.as_ref()
            && let Err(error) = app::editor_recovery::clear_editor_recovery(lease)
        {
            self.report_editor_error(i18n::LocalizedText::from(i18n::msg!(
                "shell-could-not-clear-recovery-error",
                error = error.to_string()
            )));
            return false;
        }
        self.pages.editor.editor_recovery_dirty_since = None;
        self.pages.editor.editor_last_recovery_write = None;
        true
    }
}
