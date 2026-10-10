mod files;
mod input;
mod recovery;
mod settings;
pub(in crate::session) mod tasks;
mod view;
use crate::session::*;
pub(in crate::session) const EDITOR_RECOVERY_IDLE: Duration = Duration::from_secs(2);
pub(in crate::session) const EDITOR_RECOVERY_INTERVAL: Duration = Duration::from_secs(10);
pub(in crate::session) const EDITOR_CURSOR_TIME_STEP_MS: u32 = 250;
pub(in crate::session) const EDITOR_CURSOR_MIN_TIME_MS: u32 = 250;
pub(in crate::session) const EDITOR_CURSOR_MAX_TIME_MS: u32 = 10_000;
pub(in crate::session) const EDITOR_CURSOR_MIN_HORIZONTAL_STEP: u8 = 2;
pub(in crate::session) const EDITOR_CURSOR_MAX_HORIZONTAL_STEP: u8 = 16;
pub(in crate::session) const EDITOR_CURSOR_MIN_VERTICAL_STEP: u8 = 1;
pub(in crate::session) const EDITOR_CURSOR_MAX_VERTICAL_STEP: u8 = 8;
// Legacy terminals report auto-repeat as consecutive Press events and have no
// release event. Only a closely spaced stream counts as a held direction key.
const EDITOR_CURSOR_LEGACY_REPEAT_GAP: Duration = Duration::from_millis(150);

pub(in crate::session) fn editor_cursor_direction(key: &KeyInput) -> Option<EditorCursorDirection> {
    if key.has_non_shift_modifier() {
        return None;
    }
    match key.key {
        InputKey::Left => Some(EditorCursorDirection::Left),
        InputKey::Right => Some(EditorCursorDirection::Right),
        InputKey::Up => Some(EditorCursorDirection::Up),
        InputKey::Down => Some(EditorCursorDirection::Down),
        _ => None,
    }
}

pub(in crate::session) fn normalized_editor_config(
    mut config: storage::EditorConfig,
) -> storage::EditorConfig {
    config.cursor_acceleration_delay_ms = config
        .cursor_acceleration_delay_ms
        .clamp(EDITOR_CURSOR_MIN_TIME_MS, EDITOR_CURSOR_MAX_TIME_MS);
    config.cursor_acceleration_ramp_ms = config
        .cursor_acceleration_ramp_ms
        .clamp(EDITOR_CURSOR_MIN_TIME_MS, EDITOR_CURSOR_MAX_TIME_MS);
    config.cursor_horizontal_max_step = config.cursor_horizontal_max_step.clamp(
        EDITOR_CURSOR_MIN_HORIZONTAL_STEP,
        EDITOR_CURSOR_MAX_HORIZONTAL_STEP,
    );
    let vertical_maximum = EDITOR_CURSOR_MAX_VERTICAL_STEP
        .min(config.cursor_horizontal_max_step.saturating_sub(1))
        .max(EDITOR_CURSOR_MIN_VERTICAL_STEP);
    config.cursor_vertical_max_step = config
        .cursor_vertical_max_step
        .clamp(EDITOR_CURSOR_MIN_VERTICAL_STEP, vertical_maximum);
    config
}

pub(in crate::session) fn adjust_u32_setting(value: u32, step: u32, increase: bool) -> u32 {
    if increase {
        value.saturating_add(step)
    } else {
        value.saturating_sub(step)
    }
}

pub(in crate::session) fn adjust_u8_setting(value: u8, increase: bool) -> u8 {
    if increase {
        value.saturating_add(1)
    } else {
        value.saturating_sub(1)
    }
}

impl ShellSession {
    pub(in crate::session) fn current_editor_config(&self) -> storage::EditorConfig {
        normalized_editor_config(self.app.storage_config().editor.clone())
    }

    pub(in crate::session) fn can_change_editor_settings(&self) -> bool {
        PermissionService::new(self.debug_policy)
            .authorize(
                self.app.auth_session(),
                PermissionAction::ChangeSettings,
                None,
            )
            .allowed
    }

    pub(in crate::session) fn reject_editor_settings_change(&mut self) {
        self.editor_message = Some(i18n::LocalizedText::from(i18n::msg!(
            "shell-editor-settings-are-read-only-administrator-permission-is-required"
        )));
        self.notify_status(i18n::LocalizedText::from(i18n::msg!(
            "shell-editor-settings-are-read-only"
        )));
    }

    pub(in crate::session) fn advance_editor_document_generation(&mut self) {
        self.editor_document_generation = self.editor_document_generation.wrapping_add(1).max(1);
    }

    pub(in crate::session) fn open_editor(&mut self) {
        if self.config_editor_active() {
            self.notify_toast(i18n::msg!("config-editor-finish-document"));
            return;
        }
        if self.editor_load_state.is_some() || self.editor_save_state.is_some() {
            self.report_editor_error(i18n::LocalizedText::from(i18n::msg!(
                "shell-finish-or-cancel-the-active-editor-file-operation-before-creating-a-document"
            )));
            return;
        }
        self.editor_read_session = None;
        self.advance_editor_document_generation();
        self.app.dispatch_at(
            app::AppCommand::SetEditorState(Some(EditorState::new())),
            Instant::now(),
        );
        self.editor_cursor_acceleration = None;
        self.editor_settings_dialog = None;
        self.editor_focus = ui::EditorFocus::Canvas;
        self.editor_open_menu = None;
        self.editor_selected_toolbar_action = None;
        self.editor_quick_menu_anchor = None;
        self.editor_drag_anchor = None;
        self.editor_table_column_widths.clear();
        self.editor_table_resize = None;
        self.editor_fingerprint = None;
        self.editor_close_after_save = false;
        self.editor_open_after_save = false;
        self.editor_discard_for_open = false;
        self.editor_message = Some(i18n::LocalizedText::from(i18n::msg!(
            "shell-new-text-document"
        )));
        self.restore_editor_recovery_if_present();
        self.rebuild_editor_rich_render_cache();
        self.enter_screen(ShellScreen::Editor);
        self.active_popup = None;
        self.notify_status(i18n::LocalizedText::from(i18n::msg!("shell-editor")));
        self.refresh_hit_map();
    }

    pub(in crate::session) fn rebuild_editor_rich_render_cache(&mut self) {
        let _language = i18n::enter_snapshot(self.language.clone());
        self.editor_rich_render_cache = self.app.editor_state().and_then(|state| {
            let projection = state.rich_projection()?;
            Some(EditorRichRenderCache {
                revision: state.revision(),
                blocks: std::sync::Arc::from(editor_rich_render_blocks(&projection)),
            })
        });
    }
}

pub(in crate::session) fn editor_save_status(save: &EditorSaveState) -> String {
    i18n::tr!(
        "shell-saving-arg1-arg2",
        arg1 = save.stage.label(),
        arg2 = save.path.display().to_string()
    )
}

pub(in crate::session) fn editor_load_status(load: &EditorLoadState) -> String {
    let action = if matches!(load.operation, EditorLoadOperation::Reload { .. }) {
        i18n::tr!("shell-reloading")
    } else {
        i18n::tr!("shell-loading")
    };
    match load.total_bytes {
        Some(total) if total > 0 => i18n::tr!(
            "shell-action-arg1-arg2-arg3-bytes-esc-cancel",
            action = action,
            arg1 = load.stage.label(),
            arg2 = load.completed_bytes.min(total),
            arg3 = total
        ),
        _ => i18n::tr!(
            "shell-action-arg1-esc-cancel",
            action = action,
            arg1 = load.stage.label()
        ),
    }
}

pub(in crate::session) fn editor_format_requires_selection(
    format: &app::editor::FormatCommand,
) -> bool {
    matches!(
        format,
        app::editor::FormatCommand::Bold
            | app::editor::FormatCommand::Italic
            | app::editor::FormatCommand::Strikethrough
    )
}

pub(in crate::session) fn editor_line_ending_label(metadata: app::editor::TextMetadata) -> String {
    if metadata.mixed_line_endings {
        return i18n::tr!("shell-mixed");
    }
    match metadata.preferred_line_ending {
        app::editor::LineEnding::Lf => "LF".to_string(),
        app::editor::LineEnding::CrLf => "CRLF".to_string(),
        app::editor::LineEnding::Cr => "CR".to_string(),
    }
}

pub(in crate::session) fn editor_recovery_metadata(
    metadata: app::editor::TextMetadata,
) -> app::editor_recovery::RecoveryTextMetadata {
    app::editor_recovery::RecoveryTextMetadata {
        utf8_bom: metadata.utf8_bom,
        preferred_line_ending: match metadata.preferred_line_ending {
            app::editor::LineEnding::Lf => app::editor_recovery::RecoveryLineEnding::Lf,
            app::editor::LineEnding::CrLf => app::editor_recovery::RecoveryLineEnding::CrLf,
            app::editor::LineEnding::Cr => app::editor_recovery::RecoveryLineEnding::Cr,
        },
        mixed_line_endings: metadata.mixed_line_endings,
        has_final_newline: metadata.has_final_newline,
    }
}

pub(in crate::session) fn editor_metadata_from_recovery(
    metadata: app::editor_recovery::RecoveryTextMetadata,
) -> app::editor::TextMetadata {
    app::editor::TextMetadata {
        utf8_bom: metadata.utf8_bom,
        preferred_line_ending: match metadata.preferred_line_ending {
            app::editor_recovery::RecoveryLineEnding::Lf => app::editor::LineEnding::Lf,
            app::editor_recovery::RecoveryLineEnding::CrLf => app::editor::LineEnding::CrLf,
            app::editor_recovery::RecoveryLineEnding::Cr => app::editor::LineEnding::Cr,
        },
        mixed_line_endings: metadata.mixed_line_endings,
        has_final_newline: metadata.has_final_newline,
    }
}

pub(in crate::session) fn editor_recovery_base(
    path: Option<&std::path::PathBuf>,
    saved_content_hash: Option<u64>,
    kind: app::editor::DocumentKind,
) -> (EditorState, Option<DocumentFingerprint>, bool) {
    let Some(path) = path else {
        return (EditorState::untitled(kind), None, false);
    };
    let Some(expected_hash) = saved_content_hash else {
        return (EditorState::untitled(kind), None, true);
    };
    let Ok(loaded) = platform::read_document_bytes(path) else {
        return (EditorState::untitled(kind), None, true);
    };
    if loaded.fingerprint.content_hash != expected_hash {
        return (EditorState::untitled(kind), None, true);
    }
    match EditorState::open(path.clone(), &loaded.bytes) {
        Ok(state) => (state, Some(loaded.fingerprint), false),
        Err(_) => (EditorState::untitled(kind), None, true),
    }
}

pub(in crate::session) fn restore_editor_recovery_v2(
    record: app::editor_recovery::EditorRecoveryRecordV2,
    warning: Option<String>,
) -> (
    EditorState,
    Option<DocumentFingerprint>,
    bool,
    Option<i18n::LocalizedText>,
) {
    let kind = app::editor::DocumentKind::PlainText;
    let (mut state, fingerprint, unbound) =
        editor_recovery_base(record.path.as_ref(), record.saved_content_hash, kind);
    state.document.metadata = editor_metadata_from_recovery(record.metadata);
    match record.payload {
        app::editor_recovery::EditorRecoveryPayload::Rich {
            markdown_fallback, ..
        } => {
            let cursor = markdown_fallback.len();
            state.install_source_draft(markdown_fallback, cursor, None);
        }
        app::editor_recovery::EditorRecoveryPayload::Source {
            text,
            cursor,
            selection,
        } => {
            state.install_source_draft(
                text,
                cursor,
                selection.map(|selection| {
                    app::editor::Selection::new(selection.anchor, selection.focus)
                }),
            );
        }
    }
    let warning = warning.map(|warning| {
        if unbound {
            i18n::LocalizedText::from(i18n::msg!(
                "shell-warning-the-original-file-also-changed-so-use-save-as",
                warning = warning
            ))
        } else {
            warning.into()
        }
    });
    (state, fingerprint, unbound, warning)
}

pub(in crate::session) fn editor_rich_render_blocks(
    projection: &app::rich_document::RichProjection,
) -> Vec<ui::EditorRenderBlock> {
    let mut output = Vec::new();
    for block in &projection.blocks {
        append_editor_rich_block(block, 0, &mut output);
    }
    if output.is_empty() {
        output.push(ui::EditorRenderBlock::Blank);
    }
    output
}

pub(in crate::session) fn append_editor_rich_block(
    block: &app::rich_document::ProjectedBlock,
    depth: u8,
    output: &mut Vec<ui::EditorRenderBlock>,
) {
    use app::rich_document::{ProjectedBlockKind, RichListKind};
    match &block.kind {
        ProjectedBlockKind::Paragraph { content } => output.push(ui::EditorRenderBlock::Paragraph(
            editor_rich_spans_in(block.id, content),
        )),
        ProjectedBlockKind::Heading { level, content } => {
            output.push(ui::EditorRenderBlock::Heading {
                level: *level,
                spans: editor_rich_spans_in(block.id, content),
            });
        }
        ProjectedBlockKind::Quote { blocks } => {
            for nested in blocks {
                match &nested.kind {
                    ProjectedBlockKind::Paragraph { content }
                    | ProjectedBlockKind::Heading { content, .. } => {
                        output.push(ui::EditorRenderBlock::Quote {
                            depth: depth.saturating_add(1),
                            spans: editor_rich_spans_in(nested.id, content),
                        });
                    }
                    _ => append_editor_rich_block(nested, depth.saturating_add(1), output),
                }
            }
        }
        ProjectedBlockKind::CodeBlock { code, range, .. } => {
            let mut span = ui::EditorRenderSpan::code(code).with_rich_range(*range);
            span.color = ui::EditorSpanColor::Muted;
            output.push(ui::EditorRenderBlock::Paragraph(vec![span]));
        }
        ProjectedBlockKind::List {
            kind, start, items, ..
        } => {
            for (index, item) in items.iter().enumerate() {
                let mut primary = Vec::new();
                let mut nested = Vec::new();
                for item_block in &item.blocks {
                    match &item_block.kind {
                        ProjectedBlockKind::Paragraph { content }
                        | ProjectedBlockKind::Heading { content, .. }
                            if primary.is_empty() =>
                        {
                            primary = editor_rich_spans_in(item_block.id, content);
                        }
                        _ => nested.push(item_block),
                    }
                }
                match kind {
                    RichListKind::Bullet | RichListKind::Task => {
                        output.push(ui::EditorRenderBlock::BulletListItem {
                            depth,
                            checked: if *kind == RichListKind::Task {
                                item.checked.or(Some(false))
                            } else {
                                None
                            },
                            spans: primary,
                        });
                    }
                    RichListKind::Ordered => {
                        output.push(ui::EditorRenderBlock::OrderedListItem {
                            depth,
                            number: start.saturating_add(index) as u64,
                            spans: primary,
                        });
                    }
                }
                for nested_block in nested {
                    append_editor_rich_block(nested_block, depth.saturating_add(1), output);
                }
            }
        }
        ProjectedBlockKind::Table {
            alignments,
            header,
            rows,
        } => output.push(ui::EditorRenderBlock::RichTable {
            table_id: block.id,
            header: header.iter().map(editor_rich_table_cell).collect(),
            rows: rows
                .iter()
                .map(|row| row.iter().map(editor_rich_table_cell).collect())
                .collect(),
            alignments: alignments
                .iter()
                .map(|alignment| match alignment {
                    app::rich_document::RichTableAlignment::None
                    | app::rich_document::RichTableAlignment::Left => {
                        ui::EditorTableAlignment::Left
                    }
                    app::rich_document::RichTableAlignment::Center => {
                        ui::EditorTableAlignment::Center
                    }
                    app::rich_document::RichTableAlignment::Right => {
                        ui::EditorTableAlignment::Right
                    }
                })
                .collect(),
        }),
        ProjectedBlockKind::Rule => {
            output.push(ui::EditorRenderBlock::HorizontalRule);
        }
        ProjectedBlockKind::OpaqueMarkdown { raw, reason } => {
            output.push(ui::EditorRenderBlock::RawHtml(i18n::tr!(
                "shell-unsupported-markdown-read-only-reason-nraw",
                reason = reason.to_string(),
                raw = raw
            )));
        }
    }
}

pub(in crate::session) fn editor_rich_table_cell(
    cell: &app::rich_document::ProjectedTableCell,
) -> ui::EditorTableCell {
    let mut spans = editor_rich_spans(&cell.content);
    if spans.is_empty() {
        spans.push(
            ui::EditorRenderSpan::plain("").with_rich_range(ui::RichRange::in_node(cell.id, 0, 0)),
        );
    }
    ui::EditorTableCell { spans }
}

pub(in crate::session) fn editor_rich_spans(
    spans: &[app::rich_document::ProjectedInline],
) -> Vec<ui::EditorRenderSpan> {
    spans
        .iter()
        .map(|span| {
            let mut rendered = ui::EditorRenderSpan::plain(&span.text).with_rich_range(span.range);
            rendered.bold = span.marks.bold;
            rendered.italic = span.marks.italic;
            rendered.strikethrough = span.marks.strikethrough;
            rendered.inline_code = span.marks.code;
            if span.link.is_some() {
                rendered = rendered.with_link();
            }
            if span.image.is_some() {
                rendered.color = ui::EditorSpanColor::Accent;
                rendered.underlined = true;
            }
            rendered
        })
        .collect()
}

pub(in crate::session) fn editor_rich_spans_in(
    container_id: app::rich_document::NodeId,
    spans: &[app::rich_document::ProjectedInline],
) -> Vec<ui::EditorRenderSpan> {
    let mut rendered = editor_rich_spans(spans);
    if rendered.is_empty() {
        rendered.push(
            ui::EditorRenderSpan::plain("").with_rich_range(ui::RichRange::in_node(
                container_id,
                0,
                0,
            )),
        );
    }
    rendered
}

impl ShellSession {
    pub(in crate::session) fn request_editor_close(&mut self, platform: &dyn Platform) {
        self.apply_editor_command(app::editor::EditorCommand::RequestClose, platform);
    }

    pub(in crate::session) fn apply_editor_command(
        &mut self,
        command: app::editor::EditorCommand,
        platform: &dyn Platform,
    ) {
        if self.editor_config.pending_action.is_some() {
            return;
        }
        if self.config_editor_active()
            && matches!(
                command,
                app::editor::EditorCommand::RequestOpen | app::editor::EditorCommand::RequestSaveAs
            )
        {
            self.notify_toast(i18n::msg!("config-editor-finish-document"));
            return;
        }
        if self.editor_save_state.is_some() || self.editor_load_state.is_some() {
            return;
        }
        if matches!(
            &command,
            app::editor::EditorCommand::SetMode(_) | app::editor::EditorCommand::ToggleMode
        ) {
            self.editor_quick_menu_anchor = None;
        }
        if let app::editor::EditorCommand::ApplyFormat(format) = &command {
            let Some(state) = self.app.editor_state() else {
                return;
            };
            if state.mode != app::editor::EditorMode::Rich
                || state.document.kind != app::editor::DocumentKind::Markdown
            {
                self.editor_message = Some(i18n::LocalizedText::from(i18n::msg!(
                    "shell-markdown-formatting-is-available-in-rich-mode"
                )));
                return;
            }
            if editor_format_requires_selection(format) && !state.has_selection() {
                self.editor_message = Some(i18n::LocalizedText::from(i18n::msg!(
                    "shell-select-text-before-applying-inline-formatting"
                )));
                return;
            }
        }
        let caret_navigation = matches!(
            &command,
            app::editor::EditorCommand::MoveCursor { .. }
                | app::editor::EditorCommand::MoveTo { .. }
                | app::editor::EditorCommand::SelectAll
                | app::editor::EditorCommand::SetMode(_)
                | app::editor::EditorCommand::ToggleMode
        );
        let Some((revision_before, mode_before)) = self
            .app
            .editor_state()
            .map(|state| (state.revision(), state.mode))
        else {
            return;
        };
        let now = Instant::now();
        self.app.dispatch_at(app::AppCommand::Editor(command), now);
        let Some((revision_after, mode_after, is_dirty)) = self
            .app
            .editor_state()
            .map(|state| (state.revision(), state.mode, state.is_dirty()))
        else {
            return;
        };
        let mode_changed = mode_after != mode_before;
        let projection_changed = mode_changed || revision_after != revision_before;
        if revision_after != revision_before {
            self.editor_recovery_dirty_since = Some(now);
        }
        if !is_dirty {
            if revision_after != revision_before && self.editor_last_recovery_write.is_some() {
                // Undo/redo can return to the saved checkpoint after a dirty
                // draft was persisted. Leaving that draft on disk would
                // resurrect the undone edits at the next launch. Limit this
                // to an actual edit transition and a draft this session wrote;
                // merely viewing a clean document must not erase recovery.
                self.clear_editor_recovery();
            } else {
                self.editor_recovery_dirty_since = None;
            }
        }
        if mode_changed {
            self.editor_table_column_widths.clear();
            self.editor_table_resize = None;
            self.editor_quick_menu_anchor = None;
        }
        if projection_changed {
            self.rebuild_editor_rich_render_cache();
        }
        if caret_navigation || projection_changed {
            self.reveal_source_caret();
        }
        let effects = self.app.take_editor_effects();
        for effect in effects {
            self.handle_editor_effect(effect, platform);
        }
    }
    /// Keeps the Source caret inside the text viewport after a
    /// caret-moving command. Manual scrollbar and wheel scrolling deliberately
    /// bypass this hook, so users can inspect another region until their next
    /// keyboard or editing action.
    pub(in crate::session) fn reveal_source_caret(&mut self) {
        let Some(((cursor_line, cursor_column), mut viewport)) = self
            .app
            .editor_state()
            .filter(|state| state.mode == app::editor::EditorMode::Source)
            .and_then(|state| {
                state
                    .source_display_position(state.cursor.byte_offset)
                    .map(|position| (position, state.viewport))
            })
        else {
            return;
        };
        let area = Rect::new(0, 0, self.terminal_size.0, self.terminal_size.1);
        let editor_area = match self.shell_layout_for(area) {
            ui::ShellLayout::Compact(compact) => compact,
            ui::ShellLayout::Full { main, .. } => main,
        };
        let layout = ui::editor_layout(editor_area, &self.to_editor_view_model());
        let visible_width = usize::from(layout.canvas.width);
        let visible_height = layout.visible_capacity;
        if visible_width == 0 || visible_height == 0 {
            return;
        }

        // Use the actual rendered origin: layout may clamp a requested scroll
        // offset when opening a log at EOF, deleting lines or growing the terminal.
        let top_line = layout.visible_start;
        viewport.top_line = if cursor_line < top_line {
            cursor_line
        } else if cursor_line >= top_line.saturating_add(visible_height) {
            cursor_line.saturating_add(1).saturating_sub(visible_height)
        } else {
            top_line
        };
        let left_column = layout.horizontal_scroll;
        let next_left = if cursor_column < left_column {
            cursor_column
        } else if cursor_column >= left_column.saturating_add(visible_width) {
            cursor_column
                .saturating_add(1)
                .saturating_sub(visible_width)
        } else {
            left_column
        };
        viewport.left_column = next_left;
        self.app
            .dispatch_at(app::AppCommand::SetEditorViewport(viewport), Instant::now());
    }
    pub(in crate::session) fn handle_editor_effect(
        &mut self,
        effect: app::editor::EditorEffect,
        platform: &dyn Platform,
    ) {
        match effect {
            app::editor::EditorEffect::WriteClipboard(text) => {
                match platform.write_clipboard_text(&text) {
                    Ok(()) => {
                        self.editor_message =
                            Some(i18n::LocalizedText::from(i18n::msg!("shell-copied")))
                    }
                    Err(error) => self.report_editor_error(i18n::LocalizedText::from(i18n::msg!(
                        "shell-could-not-copy-error",
                        error = error.to_string()
                    ))),
                }
            }
            app::editor::EditorEffect::ReadClipboard => match platform.read_clipboard_text() {
                Ok(text) => {
                    self.apply_editor_command(app::editor::EditorCommand::Paste(text), platform)
                }
                Err(error) => self.report_editor_error(i18n::LocalizedText::from(i18n::msg!(
                    "shell-could-not-paste-error",
                    error = error.to_string()
                ))),
            },
            app::editor::EditorEffect::OpenFilePicker => {
                if self.app.editor_state().is_some_and(EditorState::is_dirty) {
                    self.confirm_editor_open();
                } else {
                    self.open_editor_picker(platform);
                }
            }
            app::editor::EditorEffect::SaveFile { path, snapshot } => {
                self.save_editor_document(path, snapshot, platform);
            }
            app::editor::EditorEffect::SaveFilePicker {
                suggested_name,
                snapshot,
            } => self.open_editor_save_picker(platform, suggested_name, snapshot),
            app::editor::EditorEffect::ConfirmClose => {
                self.notify_modal_with_options(
                    ShellNotification::modal(
                        i18n::LocalizedText::from(i18n::msg!("shell-unsaved-document")),
                        i18n::LocalizedText::from(i18n::msg!(
                            "shell-save-your-changes-before-closing-the-editor"
                        )),
                        ui::NotificationTone::Warning,
                        vec![
                            ShellNotificationAction::new(
                                "save",
                                i18n::LocalizedText::from(i18n::msg!("shell-save")),
                            )
                            .with_shortcut(InputKey::Char('s'))
                            .with_follow_up(ShellCommand::EditorSaveAndClose),
                            ShellNotificationAction::new(
                                "discard",
                                i18n::LocalizedText::from(i18n::msg!("shell-discard")),
                            )
                            .with_shortcut(InputKey::Char('d'))
                            .with_follow_up(ShellCommand::EditorDiscardAndClose),
                            ShellNotificationAction::new(
                                "cancel",
                                i18n::LocalizedText::from(i18n::msg!("shell-cancel")),
                            )
                            .with_shortcut(InputKey::Escape)
                            .cancel()
                            .with_follow_up(ShellCommand::EditorCancelClose),
                        ],
                    )
                    .with_key(EDITOR_CLOSE_NOTIFICATION_KEY)
                    .with_component(ShellComponent::NotificationDialog),
                );
            }
            app::editor::EditorEffect::Close => self.finish_editor_close(false),
        }
    }

    pub(in crate::session) fn finish_editor_close(&mut self, _discard: bool) {
        if self.editor_config.pending_action.is_some() {
            self.notify_toast(i18n::msg!("management-operation-running"));
            return;
        }
        self.notification_dismiss_modal_by_key(EDITOR_CLOSE_NOTIFICATION_KEY);
        if self.editor_read_session.is_none() {
            self.clear_editor_recovery();
        }
        if let Some(load) = self.editor_load_state.take() {
            self.editor_task_runtime.cancel(load.id);
        }
        self.advance_editor_document_generation();
        self.app
            .dispatch_at(app::AppCommand::SetEditorState(None), Instant::now());
        self.editor_rich_render_cache = None;
        self.editor_cursor_acceleration = None;
        self.editor_settings_dialog = None;
        self.editor_fingerprint = None;
        self.editor_open_menu = None;
        self.editor_selected_toolbar_action = None;
        self.editor_quick_menu_anchor = None;
        self.editor_drag_anchor = None;
        self.editor_table_column_widths.clear();
        self.editor_table_resize = None;
        self.editor_close_after_save = false;
        self.editor_open_after_save = false;
        self.editor_discard_for_open = false;
        self.editor_message = None;
        self.editor_read_session = None;
        self.editor_config = Default::default();
        self.management_state_clear_config_form();
        self.return_from_screen(ShellScreen::Editor);
    }

    pub(in crate::session) fn report_editor_error(
        &mut self,
        message: impl Into<i18n::LocalizedText>,
    ) {
        let message = message.into();
        self.editor_message = Some(message.clone());
        self.error_message = Some(message.clone());
        self.notify_alert_with_key(EDITOR_ALERT_KEY, message, ui::NotificationTone::Error);
    }
}
