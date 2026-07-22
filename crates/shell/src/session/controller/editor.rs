use super::super::*;
pub(in crate::session) const EDITOR_RECOVERY_IDLE: Duration = Duration::from_secs(2);
pub(in crate::session) const EDITOR_RECOVERY_INTERVAL: Duration = Duration::from_secs(10);
pub(in crate::session) const EDITOR_CURSOR_TIME_STEP_MS: u32 = 250;
pub(in crate::session) const EDITOR_CURSOR_MIN_TIME_MS: u32 = 250;
pub(in crate::session) const EDITOR_CURSOR_MAX_TIME_MS: u32 = 10_000;
pub(in crate::session) const EDITOR_CURSOR_MIN_HORIZONTAL_STEP: u8 = 2;
pub(in crate::session) const EDITOR_CURSOR_MAX_HORIZONTAL_STEP: u8 = 16;
pub(in crate::session) const EDITOR_CURSOR_MIN_VERTICAL_STEP: u8 = 1;
pub(in crate::session) const EDITOR_CURSOR_MAX_VERTICAL_STEP: u8 = 8;

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
        normalized_editor_config(self.app.storage_config().editor)
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
        self.editor_message = Some(
            "Editor settings are read-only. Administrator permission is required.".to_string(),
        );
        self.notify_status("Editor settings are read-only");
    }

    pub(in crate::session) fn advance_editor_document_generation(&mut self) {
        self.editor_document_generation = self.editor_document_generation.wrapping_add(1).max(1);
    }

    pub(in crate::session) fn open_editor(&mut self) {
        if self.editor_load_state.is_some() || self.editor_save_state.is_some() {
            self.report_editor_error(
                "Finish or cancel the active Editor file operation before creating a document",
            );
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
        self.editor_message = Some("New Markdown document".to_string());
        self.restore_editor_recovery_if_present();
        self.rebuild_editor_rich_render_cache();
        if self.active_screen() != ShellScreen::Editor {
            self.screen_stack.push(ShellScreen::Editor);
        }
        self.focused_component = ShellComponent::Editor;
        self.active_popup = None;
        self.notify_status("Editor");
        self.refresh_hit_map();
    }

    pub(in crate::session) fn rebuild_editor_rich_render_cache(&mut self) {
        self.editor_rich_render_cache = self.app.editor_state().and_then(|state| {
            let projection = state.rich_projection()?;
            Some(EditorRichRenderCache {
                revision: state.revision(),
                blocks: std::sync::Arc::from(editor_rich_render_blocks(&projection)),
            })
        });
    }

    pub fn to_editor_view_model(&self) -> ui::EditorViewModel {
        if let Some(load) = self.editor_load_state.as_ref()
            && matches!(load.operation, EditorLoadOperation::Open { .. })
        {
            let file_name = load
                .path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("Loading document");
            let mut model = ui::EditorViewModel::source(file_name, "");
            model.path_hint = Some(load.path.display().to_string());
            model.read_only = true;
            model.cursor = None;
            model.settings = self.editor_settings_view_model();
            model.status_message = Some(editor_load_status(load));
            return model;
        }
        let Some(state) = self.app.editor_state() else {
            let mut model = ui::EditorViewModel::new("Untitled.md", Vec::new());
            model.settings = self.editor_settings_view_model();
            return model;
        };
        let mut model = match state.mode {
            app::editor::EditorMode::Rich => {
                let mut model = if let Some(cache) = self
                    .editor_rich_render_cache
                    .as_ref()
                    .filter(|cache| cache.revision == state.revision())
                {
                    if state.document.source().len() <= 64 * 1024 && cache.blocks.len() <= 512 {
                        // Preserve the long-standing owned projection for
                        // small documents and external view-model consumers.
                        ui::EditorViewModel::new(
                            state.document.display_name(),
                            cache.blocks.to_vec(),
                        )
                    } else {
                        ui::EditorViewModel::new_shared(
                            state.document.display_name(),
                            std::sync::Arc::clone(&cache.blocks),
                        )
                    }
                } else {
                    let blocks = state.rich_projection().map_or_else(Vec::new, |projection| {
                        editor_rich_render_blocks(&projection)
                    });
                    ui::EditorViewModel::new(state.document.display_name(), blocks)
                };
                model.rich_table_column_widths = self.editor_table_column_widths.clone();
                model.rich_cursor = state.rich_cursor();
                model.rich_selection = state
                    .rich_selection()
                    .map(|selection| ui::RichRange::between(selection.anchor, selection.focus));
                model
            }
            app::editor::EditorMode::Source => {
                let total_line_count = state.source_line_count().unwrap_or(1).max(1);
                let requested_top_line = state
                    .viewport
                    .top_line
                    .min(total_line_count.saturating_sub(1));
                // The canvas is always smaller than the terminal. Two-sided
                // overscan also covers layout clamping when a log starts at
                // its final line or the terminal grows.
                let line_budget = usize::from(self.terminal_size.1).saturating_add(4);
                let first_line = requested_top_line.saturating_sub(line_budget);
                let end_line = requested_top_line
                    .saturating_add(line_budget)
                    .min(total_line_count);
                let column_budget = usize::from(self.terminal_size.0).saturating_add(4);
                let lines = state
                    .source_viewport_lines(
                        first_line..end_line,
                        state.viewport.left_column,
                        column_budget,
                    )
                    .into_iter()
                    .map(|line| {
                        ui::EditorSourceWindowLine::new(
                            ui::EditorSourceRange::new(
                                line.visible_byte_range.start,
                                line.visible_byte_range.end,
                            ),
                            line.start_column,
                            line.text,
                        )
                    })
                    .collect();
                let mut model = ui::EditorViewModel::source_viewport(
                    state.document.display_name(),
                    first_line,
                    total_line_count,
                    lines,
                );
                model.cursor = state
                    .source_display_position(state.cursor.byte_offset)
                    .map(|(line, column)| ui::EditorTextPosition::new(line, column));
                model.selection = state.selection.and_then(|selection| {
                    let (anchor_line, anchor_column) =
                        state.source_display_position(selection.anchor)?;
                    let (active_line, active_column) =
                        state.source_display_position(selection.focus)?;
                    Some(ui::EditorSelection {
                        anchor: ui::EditorTextPosition::new(anchor_line, anchor_column),
                        active: ui::EditorTextPosition::new(active_line, active_column),
                    })
                });
                // Include the virtual caret cell after the longest line so a
                // caret at line end can remain visible at maximum scroll.
                model.horizontal_content_width = state
                    .source_max_display_width()
                    .unwrap_or_default()
                    .saturating_add(1);
                model.cursor_offset = Some(state.cursor.byte_offset);
                model.selection_offsets = state.selection.map(|selection| {
                    ui::EditorSourceSelection::new(selection.anchor, selection.focus)
                });
                model
            }
        };
        model.path_hint = state
            .document
            .path
            .as_ref()
            .map(|path| path.display().to_string());
        model.dirty = state.is_dirty();
        let saving = self.editor_save_state.is_some();
        model.read_only = state.is_read_only() || saving;
        model.read_window =
            self.editor_read_session
                .as_ref()
                .map(|session| ui::EditorReadWindowViewModel {
                    start_byte: 0,
                    total_bytes: session.total_bytes,
                });
        model.reload_available = self
            .editor_read_session
            .as_ref()
            .is_some_and(|session| matches!(session.reload, EditorReloadPolicy::Log { .. }))
            && !saving;
        model.mode = state.mode;
        model.focus = self.editor_focus;
        model.open_menu = self.editor_open_menu;
        model.settings = self.editor_settings_view_model();
        let has_selection = state.has_selection();
        model.quick_menu = self.editor_quick_menu_anchor.and_then(|anchor| {
            (!state.is_read_only()
                && !saving
                && state.mode == app::editor::EditorMode::Rich
                && state.document.kind == app::editor::DocumentKind::Markdown
                && has_selection)
                .then_some(ui::EditorQuickMenuViewModel {
                    anchor,
                    block_actions_enabled: state.can_apply_block_format_to_selection(),
                })
        });
        model.selected_toolbar_action = self.editor_selected_toolbar_action;
        model.scroll_line = state.viewport.top_line;
        model.horizontal_scroll = state.viewport.left_column;
        model.toolbar.can_save =
            !state.is_read_only() && !saving && (state.document.path.is_some() || state.is_dirty());
        model.toolbar.can_undo = !state.is_read_only() && !saving && state.can_undo();
        model.toolbar.can_redo = !state.is_read_only() && !saving && state.can_redo();
        model.toolbar.can_cut = !state.is_read_only() && !saving && has_selection;
        model.toolbar.can_copy = has_selection;
        model.toolbar.can_paste = !state.is_read_only() && !saving;
        model.word_count = state.word_count();
        model.encoding = if state.document.metadata.utf8_bom {
            "UTF-8 BOM".to_string()
        } else {
            "UTF-8".to_string()
        };
        model.line_ending = editor_line_ending_label(state.document.metadata);
        model.image_protocol = ui::EditorImageProtocolStatus::Unsupported;
        model.status_message = self
            .editor_load_state
            .as_ref()
            .map(editor_load_status)
            .or_else(|| self.editor_save_state.as_ref().map(editor_save_status))
            .or_else(|| self.editor_message.clone());
        model
    }

    pub(in crate::session) fn editor_settings_view_model(
        &self,
    ) -> Option<ui::EditorSettingsViewModel> {
        self.editor_settings_dialog
            .map(|dialog| ui::EditorSettingsViewModel {
                editable: self.can_change_editor_settings(),
                enabled: dialog.draft.cursor_acceleration_enabled,
                activation_delay_ms: dialog.draft.cursor_acceleration_delay_ms,
                ramp_duration_ms: dialog.draft.cursor_acceleration_ramp_ms,
                horizontal_max_step: dialog.draft.cursor_horizontal_max_step,
                vertical_max_step: dialog.draft.cursor_vertical_max_step,
                selected: dialog.selected,
            })
    }
}

pub(in crate::session) fn editor_save_status(save: &EditorSaveState) -> String {
    format!("Saving · {} · {}", save.stage.label(), save.path.display())
}

pub(in crate::session) fn editor_load_status(load: &EditorLoadState) -> String {
    let action = if matches!(load.operation, EditorLoadOperation::Reload { .. }) {
        "Reloading"
    } else {
        "Loading"
    };
    match load.total_bytes {
        Some(total) if total > 0 => format!(
            "{action} · {} · {} / {} bytes · Esc Cancel",
            load.stage.label(),
            load.completed_bytes.min(total),
            total
        ),
        _ => format!("{action} · {} · Esc Cancel", load.stage.label()),
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
        return "Mixed".to_string();
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

pub(in crate::session) fn editor_recovery_rich_line_ending(
    ending: app::editor::LineEnding,
) -> app::rich_document::RichLineEnding {
    match ending {
        app::editor::LineEnding::Lf => app::rich_document::RichLineEnding::Lf,
        app::editor::LineEnding::CrLf => app::rich_document::RichLineEnding::CrLf,
        app::editor::LineEnding::Cr => app::rich_document::RichLineEnding::Cr,
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
    Option<String>,
) {
    let kind = match record.document_kind {
        app::editor_recovery::RecoveryDocumentKind::Markdown => app::editor::DocumentKind::Markdown,
        app::editor_recovery::RecoveryDocumentKind::PlainText => {
            app::editor::DocumentKind::PlainText
        }
    };
    let (mut state, fingerprint, unbound) =
        editor_recovery_base(record.path.as_ref(), record.saved_content_hash, kind);
    state.document.metadata = editor_metadata_from_recovery(record.metadata);
    match record.payload {
        app::editor_recovery::EditorRecoveryPayload::Rich {
            document,
            cursor,
            selection,
            ..
        } => {
            state.install_rich_draft(
                document,
                cursor,
                selection.map(|selection| {
                    app::rich_edit::RichSelection::new(selection.start, selection.end)
                }),
            );
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
            format!("{warning}; the original file also changed, so use Save As")
        } else {
            warning
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
            output.push(ui::EditorRenderBlock::RawHtml(format!(
                "Unsupported Markdown (read-only: {reason})\n{raw}"
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
                self.editor_message =
                    Some("Markdown formatting is available in Rich mode".to_string());
                return;
            }
            if editor_format_requires_selection(format) && !state.has_selection() {
                self.editor_message =
                    Some("Select text before applying inline formatting".to_string());
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
            self.editor_recovery_dirty_since = None;
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
    /// Keeps the Source caret inside the horizontal text viewport after a
    /// caret-moving command. Manual scrollbar and wheel scrolling deliberately
    /// bypass this hook, so users can inspect another column until their next
    /// keyboard or editing action.
    pub(in crate::session) fn reveal_source_caret(&mut self) {
        let Some((cursor_column, mut viewport)) = self
            .app
            .editor_state()
            .filter(|state| state.mode == app::editor::EditorMode::Source)
            .and_then(|state| {
                state
                    .source_display_position(state.cursor.byte_offset)
                    .map(|(_, column)| (column, state.viewport))
            })
        else {
            return;
        };
        let left_column = viewport.left_column;

        let area = Rect::new(0, 0, self.terminal_size.0, self.terminal_size.1);
        let editor_area = match ui::compute_shell_layout(area) {
            ui::ShellLayout::Compact(compact) => compact,
            ui::ShellLayout::Full { main, .. } => main,
        };
        let layout = ui::editor_layout(editor_area, &self.to_editor_view_model());
        let visible_width = usize::from(layout.canvas.width);
        if visible_width == 0 {
            return;
        }

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
                    Ok(()) => self.editor_message = Some("Copied".to_string()),
                    Err(error) => self.report_editor_error(format!("Could not copy: {error}")),
                }
            }
            app::editor::EditorEffect::ReadClipboard => match platform.read_clipboard_text() {
                Ok(text) => {
                    self.apply_editor_command(app::editor::EditorCommand::Paste(text), platform)
                }
                Err(error) => self.report_editor_error(format!("Could not paste: {error}")),
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
                        "Unsaved document",
                        "Save your changes before closing the Editor?",
                        ui::NotificationTone::Warning,
                        vec![
                            ShellNotificationAction::new("save", "Save")
                                .with_follow_up(ShellCommand::EditorSaveAndClose),
                            ShellNotificationAction::new("discard", "Discard")
                                .with_follow_up(ShellCommand::EditorDiscardAndClose),
                            ShellNotificationAction::new("cancel", "Cancel")
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

    #[cfg(test)]
    pub(in crate::session) fn handle_editor_key(&mut self, key: KeyInput, platform: &dyn Platform) {
        self.handle_editor_key_at(key, platform, Instant::now());
    }

    pub(in crate::session) fn handle_editor_key_at(
        &mut self,
        key: KeyInput,
        platform: &dyn Platform,
        received_at: Instant,
    ) {
        let cursor_direction = editor_cursor_direction(&key);
        if key.phase == InputPhase::Release {
            if self
                .editor_cursor_acceleration
                .is_some_and(|state| Some(state.direction) == cursor_direction)
            {
                self.editor_cursor_acceleration = None;
            }
            return;
        }
        if !key.phase.is_press_like() {
            return;
        }
        let repeated = key.phase == InputPhase::Repeat;
        if self.editor_settings_dialog.is_some() {
            self.editor_cursor_acceleration = None;
            self.handle_editor_settings_key(&key, repeated);
            return;
        }
        if cursor_direction.is_none() {
            self.editor_cursor_acceleration = None;
        }
        if self.editor_save_state.is_some() {
            return;
        }
        if self.editor_load_state.is_some() {
            if key.key == InputKey::Escape && !repeated {
                self.cancel_editor_load();
            }
            return;
        }
        if matches!(key.key, InputKey::Char('r' | 'R'))
            && key.modifiers == InputModifiers::none()
            && self
                .editor_read_session
                .as_ref()
                .is_some_and(|session| matches!(session.reload, EditorReloadPolicy::Log { .. }))
        {
            if !repeated {
                self.reload_log_editor();
            }
            return;
        }
        if key.key == InputKey::Escape {
            if repeated {
                return;
            }
            if self.editor_quick_menu_anchor.take().is_some() {
                self.editor_focus = ui::EditorFocus::Canvas;
                return;
            }
            if self.editor_open_menu.take().is_some()
                || self.editor_selected_toolbar_action.take().is_some()
            {
                self.editor_focus = ui::EditorFocus::Canvas;
                return;
            }
        }
        self.editor_open_menu = None;
        self.editor_selected_toolbar_action = None;
        self.editor_quick_menu_anchor = None;
        if key.key == InputKey::F(6) {
            self.editor_focus = match self.editor_focus {
                ui::EditorFocus::MenuBar => ui::EditorFocus::Toolbar,
                ui::EditorFocus::Toolbar => ui::EditorFocus::Canvas,
                ui::EditorFocus::Canvas => ui::EditorFocus::StatusBar,
                ui::EditorFocus::StatusBar => ui::EditorFocus::MenuBar,
            };
            return;
        }
        // Keyboard editing always returns the live caret to the document after
        // a pointer interaction with a menu or toolbar.
        self.editor_focus = ui::EditorFocus::Canvas;
        let command_key = key.modifiers.control
            || (platform.kind() == PlatformKind::Macos && key.modifiers.super_key);
        if command_key {
            let navigation = match key.key {
                InputKey::Left => Some(app::editor::CursorMove::WordLeft),
                InputKey::Right => Some(app::editor::CursorMove::WordRight),
                InputKey::Home => Some(app::editor::CursorMove::DocumentStart),
                InputKey::End => Some(app::editor::CursorMove::DocumentEnd),
                _ => None,
            };
            if let Some(movement) = navigation {
                self.apply_editor_command(
                    app::editor::EditorCommand::MoveCursor {
                        movement,
                        extend_selection: key.modifiers.shift,
                    },
                    platform,
                );
                return;
            }
            // Navigation and text editing may repeat while a key is held, but
            // document/clipboard/format actions must run once per physical key
            // press. In particular, a repeated Ctrl+W must never close a newly
            // opened document after the first close has already completed.
            if repeated {
                return;
            }
            let character = match key.key {
                InputKey::Char(character) => character.to_ascii_lowercase(),
                _ => '\0',
            };
            if key.modifiers.alt {
                let format = match character {
                    '0' => Some(app::editor::FormatCommand::Paragraph),
                    '1'..='6' => Some(app::editor::FormatCommand::Heading(
                        character.to_digit(10).unwrap_or_default() as u8,
                    )),
                    _ => None,
                };
                if let Some(format) = format {
                    self.apply_editor_command(
                        app::editor::EditorCommand::ApplyFormat(format),
                        platform,
                    );
                    return;
                }
            }
            let command = match (character, key.modifiers.shift) {
                ('n', _) => {
                    if self
                        .app
                        .editor_state()
                        .is_some_and(EditorState::is_read_only)
                    {
                        self.editor_message = Some("This document is read-only".to_string());
                        return;
                    }
                    if self.app.editor_state().is_some_and(EditorState::is_dirty) {
                        self.editor_message = Some(
                            "Save or close the current document before creating a new one"
                                .to_string(),
                        );
                    } else {
                        self.advance_editor_document_generation();
                        self.app.dispatch_at(
                            app::AppCommand::SetEditorState(Some(EditorState::new())),
                            Instant::now(),
                        );
                        self.editor_quick_menu_anchor = None;
                        self.editor_table_column_widths.clear();
                        self.editor_table_resize = None;
                        self.editor_fingerprint = None;
                        self.editor_message = Some("New Markdown document".to_string());
                        self.rebuild_editor_rich_render_cache();
                    }
                    return;
                }
                ('o', _) => app::editor::EditorCommand::RequestOpen,
                ('s', true) => app::editor::EditorCommand::RequestSaveAs,
                ('s', false) => app::editor::EditorCommand::RequestSave,
                ('w', _) => app::editor::EditorCommand::RequestClose,
                ('z', false) => app::editor::EditorCommand::Undo,
                ('y', _) | ('z', true) => app::editor::EditorCommand::Redo,
                ('x', true) => app::editor::EditorCommand::ApplyFormat(
                    app::editor::FormatCommand::Strikethrough,
                ),
                ('x', false) => app::editor::EditorCommand::Cut,
                ('c', _) => app::editor::EditorCommand::Copy,
                ('v', _) => app::editor::EditorCommand::RequestPaste,
                ('a', _) => app::editor::EditorCommand::SelectAll,
                ('b', _) => {
                    app::editor::EditorCommand::ApplyFormat(app::editor::FormatCommand::Bold)
                }
                ('i', _) => {
                    app::editor::EditorCommand::ApplyFormat(app::editor::FormatCommand::Italic)
                }
                ('m', true) => app::editor::EditorCommand::ToggleMode,
                ('f', _) => {
                    self.editor_message = Some("Find is not available in this build".to_string());
                    return;
                }
                ('h', _) => {
                    self.editor_message =
                        Some("Replace is not available in this build".to_string());
                    return;
                }
                ('k', _) => {
                    app::editor::EditorCommand::ApplyFormat(app::editor::FormatCommand::Link {
                        url: "https://".to_string(),
                        title: None,
                    })
                }
                _ => return,
            };
            self.apply_editor_command(command, platform);
            return;
        }

        if let Some(direction) = cursor_direction {
            let movement = match direction {
                EditorCursorDirection::Left => app::editor::CursorMove::Left,
                EditorCursorDirection::Right => app::editor::CursorMove::Right,
                EditorCursorDirection::Up => app::editor::CursorMove::Up,
                EditorCursorDirection::Down => app::editor::CursorMove::Down,
            };
            let step_count = self.editor_cursor_step_count(direction, key.phase, received_at);
            for _ in 0..step_count {
                self.apply_editor_command(
                    app::editor::EditorCommand::MoveCursor {
                        movement,
                        extend_selection: key.modifiers.shift,
                    },
                    platform,
                );
            }
            return;
        }

        let command = match key.key {
            InputKey::Escape => app::editor::EditorCommand::RequestClose,
            InputKey::Enter => app::editor::EditorCommand::InsertNewline,
            InputKey::Backspace => app::editor::EditorCommand::Backspace,
            InputKey::Delete => app::editor::EditorCommand::DeleteForward,
            InputKey::Tab => app::editor::EditorCommand::InsertText("    ".to_string()),
            InputKey::BackTab => {
                self.editor_message = Some("Outdent is not available for this block".to_string());
                return;
            }
            InputKey::Home => app::editor::EditorCommand::MoveCursor {
                movement: app::editor::CursorMove::LineStart,
                extend_selection: key.modifiers.shift,
            },
            InputKey::End => app::editor::EditorCommand::MoveCursor {
                movement: app::editor::CursorMove::LineEnd,
                extend_selection: key.modifiers.shift,
            },
            InputKey::PageUp => {
                if let Some(mut viewport) = self.app.editor_state().map(|state| state.viewport) {
                    viewport.top_line = viewport.top_line.saturating_sub(10);
                    self.app
                        .dispatch_at(app::AppCommand::SetEditorViewport(viewport), Instant::now());
                }
                return;
            }
            InputKey::PageDown => {
                if let Some(mut viewport) = self.app.editor_state().map(|state| state.viewport) {
                    viewport.top_line = viewport.top_line.saturating_add(10);
                    self.app
                        .dispatch_at(app::AppCommand::SetEditorViewport(viewport), Instant::now());
                }
                return;
            }
            InputKey::Char(character) if !key.has_non_shift_modifier() => {
                app::editor::EditorCommand::InsertText(character.to_string())
            }
            _ => return,
        };
        self.apply_editor_command(command, platform);
    }

    pub(in crate::session) fn editor_cursor_step_count(
        &mut self,
        direction: EditorCursorDirection,
        phase: InputPhase,
        received_at: Instant,
    ) -> u8 {
        let config = self.current_editor_config();
        if !config.cursor_acceleration_enabled {
            self.editor_cursor_acceleration = None;
            return 1;
        }
        if phase == InputPhase::Press
            || self
                .editor_cursor_acceleration
                .is_none_or(|state| state.direction != direction)
        {
            self.editor_cursor_acceleration = Some(EditorCursorAccelerationState {
                direction,
                started_at: received_at,
            });
            return 1;
        }
        let Some(state) = self.editor_cursor_acceleration else {
            return 1;
        };
        let held_ms = received_at
            .saturating_duration_since(state.started_at)
            .as_millis();
        let delay_ms = u128::from(config.cursor_acceleration_delay_ms);
        if held_ms <= delay_ms {
            return 1;
        }
        let maximum = match direction {
            EditorCursorDirection::Left | EditorCursorDirection::Right => {
                config.cursor_horizontal_max_step
            }
            EditorCursorDirection::Up | EditorCursorDirection::Down => {
                config.cursor_vertical_max_step
            }
        }
        .max(1);
        let ramp_ms = u128::from(config.cursor_acceleration_ramp_ms.max(1));
        let accelerated_ms = held_ms.saturating_sub(delay_ms).min(ramp_ms);
        let numerator = u128::from(maximum.saturating_sub(1))
            .saturating_mul(accelerated_ms)
            .saturating_mul(accelerated_ms);
        let denominator = ramp_ms.saturating_mul(ramp_ms).max(1);
        let extra = numerator.saturating_add(denominator - 1) / denominator;
        1u8.saturating_add(extra as u8).min(maximum)
    }

    pub(in crate::session) fn handle_editor_settings_key(
        &mut self,
        key: &KeyInput,
        repeated: bool,
    ) {
        let Some(selected) = self.editor_settings_dialog.map(|dialog| dialog.selected) else {
            return;
        };
        match key.key {
            InputKey::Escape if !repeated => self.editor_settings_dialog = None,
            InputKey::Tab | InputKey::Down => {
                self.select_editor_setting(selected.next());
            }
            InputKey::BackTab | InputKey::Up => {
                self.select_editor_setting(selected.previous());
            }
            InputKey::Left => self.adjust_editor_setting(selected, -1),
            InputKey::Right => self.adjust_editor_setting(selected, 1),
            InputKey::Enter | InputKey::Char(' ') if !repeated => {
                self.activate_editor_setting(selected)
            }
            _ => {}
        }
    }

    pub(in crate::session) fn open_editor_settings(&mut self) {
        self.editor_open_menu = None;
        self.editor_selected_toolbar_action = None;
        self.editor_quick_menu_anchor = None;
        self.editor_cursor_acceleration = None;
        self.editor_settings_dialog = Some(EditorSettingsDialogState {
            draft: self.current_editor_config(),
            selected: ui::EditorSettingsField::Enabled,
        });
        if !self.can_change_editor_settings() {
            self.reject_editor_settings_change();
        }
    }

    pub(in crate::session) fn activate_editor_settings_control(
        &mut self,
        control: ui::EditorSettingsControl,
    ) {
        use ui::{EditorSettingsControl as Control, EditorSettingsField as Field};
        match control {
            Control::ToggleEnabled => {
                self.select_editor_setting(Field::Enabled);
                self.activate_editor_setting(Field::Enabled);
            }
            Control::Decrease(field) => {
                self.select_editor_setting(field);
                self.adjust_editor_setting(field, -1);
            }
            Control::Increase(field) => {
                self.select_editor_setting(field);
                self.adjust_editor_setting(field, 1);
            }
            Control::RestoreDefaults => {
                self.select_editor_setting(Field::RestoreDefaults);
                self.activate_editor_setting(Field::RestoreDefaults);
            }
            Control::Save => {
                self.select_editor_setting(Field::Save);
                self.activate_editor_setting(Field::Save);
            }
            Control::Cancel => {
                self.select_editor_setting(Field::Cancel);
                self.activate_editor_setting(Field::Cancel);
            }
        }
    }

    pub(in crate::session) fn select_editor_setting(&mut self, field: ui::EditorSettingsField) {
        if let Some(dialog) = self.editor_settings_dialog.as_mut() {
            dialog.selected = field;
        }
    }

    pub(in crate::session) fn activate_editor_setting(&mut self, field: ui::EditorSettingsField) {
        use ui::EditorSettingsField as Field;
        if field != Field::Cancel && !self.can_change_editor_settings() {
            self.reject_editor_settings_change();
            return;
        }
        match field {
            Field::Enabled => {
                if let Some(dialog) = self.editor_settings_dialog.as_mut() {
                    dialog.draft.cursor_acceleration_enabled =
                        !dialog.draft.cursor_acceleration_enabled;
                }
            }
            Field::ActivationDelay
            | Field::RampDuration
            | Field::HorizontalMaxStep
            | Field::VerticalMaxStep => self.adjust_editor_setting(field, 1),
            Field::RestoreDefaults => {
                if let Some(dialog) = self.editor_settings_dialog.as_mut() {
                    dialog.draft = storage::EditorConfig::default();
                }
            }
            Field::Save => self.save_editor_settings(),
            Field::Cancel => self.editor_settings_dialog = None,
        }
    }

    pub(in crate::session) fn adjust_editor_setting(
        &mut self,
        field: ui::EditorSettingsField,
        direction: i8,
    ) {
        if !self.can_change_editor_settings() {
            self.reject_editor_settings_change();
            return;
        }
        let Some(dialog) = self.editor_settings_dialog.as_mut() else {
            return;
        };
        let increase = direction >= 0;
        match field {
            ui::EditorSettingsField::ActivationDelay => {
                dialog.draft.cursor_acceleration_delay_ms = adjust_u32_setting(
                    dialog.draft.cursor_acceleration_delay_ms,
                    EDITOR_CURSOR_TIME_STEP_MS,
                    increase,
                );
            }
            ui::EditorSettingsField::RampDuration => {
                dialog.draft.cursor_acceleration_ramp_ms = adjust_u32_setting(
                    dialog.draft.cursor_acceleration_ramp_ms,
                    EDITOR_CURSOR_TIME_STEP_MS,
                    increase,
                );
            }
            ui::EditorSettingsField::HorizontalMaxStep => {
                dialog.draft.cursor_horizontal_max_step =
                    adjust_u8_setting(dialog.draft.cursor_horizontal_max_step, increase);
            }
            ui::EditorSettingsField::VerticalMaxStep => {
                dialog.draft.cursor_vertical_max_step =
                    adjust_u8_setting(dialog.draft.cursor_vertical_max_step, increase);
            }
            ui::EditorSettingsField::Enabled
            | ui::EditorSettingsField::RestoreDefaults
            | ui::EditorSettingsField::Save
            | ui::EditorSettingsField::Cancel => return,
        }
        dialog.draft = normalized_editor_config(dialog.draft);
    }

    pub(in crate::session) fn save_editor_settings(&mut self) {
        if !self.can_change_editor_settings() {
            self.reject_editor_settings_change();
            return;
        }
        let Some(dialog) = self.editor_settings_dialog else {
            return;
        };
        let editor = normalized_editor_config(dialog.draft);
        let config = if let Some(storage) = self.storage_manager.as_ref() {
            let mut config = match storage.load_config() {
                Ok(config) => config,
                Err(error) => {
                    self.editor_message = Some(format!("Could not save Editor settings: {error}"));
                    return;
                }
            };
            config.editor = editor;
            if let Err(error) = storage.save_config(&config) {
                self.editor_message = Some(format!("Could not save Editor settings: {error}"));
                return;
            }
            self.editor_message = Some("Editor settings saved".to_string());
            config
        } else {
            let mut config = self.app.storage_config().clone();
            config.editor = editor;
            self.editor_message = Some("Editor settings applied for this session".to_string());
            config
        };
        self.replace_storage_config(config);
        self.editor_cursor_acceleration = None;
        self.editor_settings_dialog = None;
    }

    pub(in crate::session) fn handle_editor_paste(&mut self, value: String) {
        self.editor_cursor_acceleration = None;
        self.editor_quick_menu_anchor = None;
        let platform = platform::native_platform();
        self.apply_editor_command(app::editor::EditorCommand::Paste(value), platform.as_ref());
    }

    pub(in crate::session) fn handle_editor_pointer(
        &mut self,
        mouse: MouseInput,
        platform: &dyn Platform,
    ) {
        let coordinates = mouse.coordinates();
        let (hit, document_hit) = self.editor_hit_targets_at(coordinates);
        if !matches!(
            mouse,
            MouseInput {
                kind: ui::MouseEventKind::Moved,
                ..
            }
        ) {
            self.editor_cursor_acceleration = None;
        }
        if self.editor_settings_dialog.is_some() {
            match mouse {
                MouseInput {
                    kind: ui::MouseEventKind::Moved,
                    ..
                } => {
                    self.hovered_component = Some(ShellComponent::Editor);
                }
                MouseInput {
                    kind: ui::MouseEventKind::Down(PointerButton::Left),
                    ..
                } => match hit {
                    Some(ui::EditorHitTarget::SettingsControl(control)) => {
                        self.activate_editor_settings_control(control);
                    }
                    Some(ui::EditorHitTarget::SettingsField(field)) => {
                        self.select_editor_setting(field);
                    }
                    _ => {}
                },
                MouseInput {
                    kind: ui::MouseEventKind::Scroll(direction),
                    ..
                } => match direction {
                    ScrollDirection::Up | ScrollDirection::Left => {
                        if let Some(dialog) = self.editor_settings_dialog {
                            self.adjust_editor_setting(dialog.selected, -1);
                        }
                    }
                    ScrollDirection::Down | ScrollDirection::Right => {
                        if let Some(dialog) = self.editor_settings_dialog {
                            self.adjust_editor_setting(dialog.selected, 1);
                        }
                    }
                },
                MouseInput {
                    kind: ui::MouseEventKind::Down(_),
                    ..
                }
                | MouseInput {
                    kind: ui::MouseEventKind::Up(_),
                    ..
                }
                | MouseInput {
                    kind: ui::MouseEventKind::Drag(_),
                    ..
                }
                | MouseInput {
                    kind: ui::MouseEventKind::Click(_),
                    ..
                }
                | MouseInput {
                    kind: ui::MouseEventKind::DoubleClick(_),
                    ..
                } => {}
            }
            return;
        }
        match mouse {
            MouseInput {
                kind: ui::MouseEventKind::Moved,
                ..
            } => {
                self.hovered_component = hit.map(|_| ShellComponent::Editor);
            }
            MouseInput {
                kind: ui::MouseEventKind::Scroll(direction),
                ..
            } => {
                self.editor_quick_menu_anchor = None;
                if let Some(mut viewport) = self.app.editor_state().map(|state| state.viewport) {
                    match direction {
                        ScrollDirection::Up => {
                            viewport.top_line = viewport.top_line.saturating_sub(3);
                        }
                        ScrollDirection::Down => {
                            viewport.top_line = viewport.top_line.saturating_add(3);
                        }
                        ScrollDirection::Left => {
                            viewport.left_column = viewport.left_column.saturating_sub(4);
                        }
                        ScrollDirection::Right => {
                            viewport.left_column = viewport.left_column.saturating_add(4);
                        }
                    }
                    self.app
                        .dispatch_at(app::AppCommand::SetEditorViewport(viewport), Instant::now());
                }
            }
            MouseInput {
                kind: ui::MouseEventKind::Down(PointerButton::Left),
                modifiers,
                ..
            } => {
                if !matches!(hit, Some(ui::EditorHitTarget::QuickMenuPopup)) {
                    self.editor_quick_menu_anchor = None;
                }
                match hit {
                    Some(ui::EditorHitTarget::QuickMenuAction(action)) => {
                        self.activate_editor_quick_action(action, platform);
                        self.editor_focus = ui::EditorFocus::Canvas;
                    }
                    Some(ui::EditorHitTarget::QuickMenuPopup) => {}
                    Some(ui::EditorHitTarget::Menu(menu)) => {
                        if menu == ui::EditorMenu::Settings {
                            self.open_editor_settings();
                        } else {
                            self.editor_focus = ui::EditorFocus::MenuBar;
                            self.editor_open_menu =
                                (self.editor_open_menu != Some(menu)).then_some(menu);
                        }
                    }
                    Some(ui::EditorHitTarget::MenuAction(action)) => {
                        self.editor_open_menu = None;
                        self.editor_selected_toolbar_action = None;
                        match action {
                            ui::EditorMenuAction::Toolbar(action) => {
                                self.activate_editor_toolbar(action, platform);
                            }
                            ui::EditorMenuAction::Mode(mode) => self.apply_editor_command(
                                app::editor::EditorCommand::SetMode(mode),
                                platform,
                            ),
                        }
                        self.editor_focus = ui::EditorFocus::Canvas;
                    }
                    Some(ui::EditorHitTarget::MenuPopup) => {}
                    Some(ui::EditorHitTarget::Toolbar(action)) => {
                        self.editor_open_menu = None;
                        self.editor_selected_toolbar_action = Some(action);
                        self.activate_editor_toolbar(action, platform);
                        self.editor_selected_toolbar_action = None;
                        self.editor_focus = ui::EditorFocus::Canvas;
                    }
                    Some(ui::EditorHitTarget::Mode(mode)) => {
                        self.editor_open_menu = None;
                        self.apply_editor_command(
                            app::editor::EditorCommand::SetMode(mode),
                            platform,
                        );
                        self.editor_focus = ui::EditorFocus::Canvas;
                    }
                    Some(ui::EditorHitTarget::TableEdge { .. }) => {
                        self.editor_message =
                            Some("Switch to Rich mode to edit table structure".to_string());
                    }
                    Some(ui::EditorHitTarget::RichTableEdge { table_id, edge }) => {
                        self.edit_editor_table_edge(
                            table_id,
                            edge,
                            app::editor::TableColumnEdit::Insert,
                            platform,
                        );
                    }
                    Some(ui::EditorHitTarget::TableResize { .. }) => {
                        self.editor_message =
                            Some("Switch to Rich mode to resize table columns".to_string());
                    }
                    Some(ui::EditorHitTarget::RichTableResize {
                        table_id,
                        column_index,
                        width,
                    }) => {
                        self.editor_open_menu = None;
                        self.editor_focus = ui::EditorFocus::Canvas;
                        self.editor_drag_anchor = None;
                        self.editor_table_resize = Some(EditorTableResizeState {
                            table_id,
                            column_index,
                            start_x: coordinates.0,
                            start_width: width,
                        });
                        self.editor_message = Some(format!(
                            "Resizing table column {} ({width} cells)",
                            column_index + 1
                        ));
                    }
                    Some(ui::EditorHitTarget::Canvas(position)) => {
                        self.editor_open_menu = None;
                        self.editor_focus = ui::EditorFocus::Canvas;
                        let rich_mode = self
                            .app
                            .editor_state()
                            .is_some_and(|state| state.mode == app::editor::EditorMode::Rich);
                        let position = match document_hit {
                            Some(hit) if hit.editable => match hit.position {
                                ui::EditorDocumentPosition::Rich(position) => {
                                    app::editor::EditorPosition::Rich(position)
                                }
                                ui::EditorDocumentPosition::Source(offset) => {
                                    app::editor::EditorPosition::Source(offset)
                                }
                            },
                            Some(_) => {
                                self.editor_message = Some(
                                    "This Rich decoration is not directly editable; click its text"
                                        .to_string(),
                                );
                                return;
                            }
                            None if rich_mode => {
                                self.editor_message = Some(
                                    "This Rich cell has no editable text position".to_string(),
                                );
                                return;
                            }
                            None => self
                                .app
                                .editor_state()
                                .and_then(|state| {
                                    state.source_offset(position.line, position.column)
                                })
                                .map(app::editor::EditorPosition::Source)
                                .unwrap_or(app::editor::EditorPosition::Source(0)),
                        };
                        self.editor_drag_anchor = Some(position);
                        self.apply_editor_command(
                            app::editor::EditorCommand::MoveTo {
                                position,
                                extend_selection: modifiers.shift,
                            },
                            platform,
                        );
                    }
                    Some(ui::EditorHitTarget::StatusBar) => {
                        self.editor_open_menu = None;
                        self.editor_focus = ui::EditorFocus::StatusBar;
                    }
                    Some(ui::EditorHitTarget::VerticalScrollbar) => {
                        self.editor_open_menu = None;
                        self.begin_editor_scrollbar_drag(coordinates, ScrollbarAxis::Vertical);
                    }
                    Some(ui::EditorHitTarget::HorizontalScrollbar) => {
                        self.editor_open_menu = None;
                        self.begin_editor_scrollbar_drag(coordinates, ScrollbarAxis::Horizontal);
                    }
                    Some(ui::EditorHitTarget::SettingsControl(_))
                    | Some(ui::EditorHitTarget::SettingsField(_))
                    | Some(ui::EditorHitTarget::SettingsDialog) => {}
                    None => self.editor_open_menu = None,
                }
            }
            MouseInput {
                kind: ui::MouseEventKind::Down(PointerButton::Right),
                ..
            } => {
                self.editor_quick_menu_anchor = None;
                if let Some(ui::EditorHitTarget::RichTableEdge { table_id, edge }) = hit {
                    self.edit_editor_table_edge(
                        table_id,
                        edge,
                        app::editor::TableColumnEdit::Remove,
                        platform,
                    );
                    return;
                }
                if matches!(hit, Some(ui::EditorHitTarget::Canvas(_)))
                    && self.editor_quick_menu_is_available()
                {
                    self.editor_open_menu = None;
                    self.editor_selected_toolbar_action = None;
                    self.editor_focus = ui::EditorFocus::Canvas;
                    self.editor_quick_menu_anchor = Some(coordinates);
                }
            }
            MouseInput {
                kind: ui::MouseEventKind::Drag(PointerButton::Left),
                ..
            } => {
                self.editor_quick_menu_anchor = None;
                if matches!(self.scrollbar_drag, Some(ScrollbarDragState::Editor { .. })) {
                    self.drag_editor_scrollbar(coordinates);
                    return;
                }
                if self.editor_table_resize.is_some() {
                    self.resize_editor_table_column(coordinates.0);
                    return;
                }
                if let Some(ui::EditorHitTarget::Canvas(position)) = hit {
                    let rich_mode = self
                        .app
                        .editor_state()
                        .is_some_and(|state| state.mode == app::editor::EditorMode::Rich);
                    let position = match document_hit {
                        Some(hit) if hit.editable => match hit.position {
                            ui::EditorDocumentPosition::Rich(position) => {
                                app::editor::EditorPosition::Rich(position)
                            }
                            ui::EditorDocumentPosition::Source(offset) => {
                                app::editor::EditorPosition::Source(offset)
                            }
                        },
                        Some(_) => {
                            self.editor_message =
                                Some("Rich selection can only start on editable text".to_string());
                            return;
                        }
                        None if rich_mode => {
                            self.editor_message =
                                Some("This Rich cell has no editable text position".to_string());
                            return;
                        }
                        None => self
                            .app
                            .editor_state()
                            .and_then(|state| state.source_offset(position.line, position.column))
                            .map(app::editor::EditorPosition::Source)
                            .unwrap_or(app::editor::EditorPosition::Source(0)),
                    };
                    self.apply_editor_command(
                        app::editor::EditorCommand::MoveTo {
                            position,
                            extend_selection: true,
                        },
                        platform,
                    );
                }
            }
            MouseInput {
                kind: ui::MouseEventKind::Up(PointerButton::Left),
                ..
            } => {
                self.clear_editor_scrollbar_drag();
                self.editor_drag_anchor = None;
                self.editor_table_resize = None;
            }
            MouseInput {
                kind: ui::MouseEventKind::Down(_),
                ..
            }
            | MouseInput {
                kind: ui::MouseEventKind::Up(_),
                ..
            }
            | MouseInput {
                kind: ui::MouseEventKind::Drag(_),
                ..
            }
            | MouseInput {
                kind: ui::MouseEventKind::Click(_),
                ..
            }
            | MouseInput {
                kind: ui::MouseEventKind::DoubleClick(_),
                ..
            } => {}
        }
    }

    pub(in crate::session) fn resize_editor_table_column(&mut self, x: u16) {
        let Some(resize) = self.editor_table_resize else {
            return;
        };
        let delta = i32::from(x) - i32::from(resize.start_x);
        let width = (resize.start_width as i32 + delta).clamp(1, 120) as usize;
        let columns = self
            .editor_table_column_widths
            .entry(resize.table_id)
            .or_default();
        if columns.len() <= resize.column_index {
            columns.resize(resize.column_index + 1, 0);
        }
        columns[resize.column_index] = width;
        self.editor_message = Some(format!(
            "Table column {} width: {width}",
            resize.column_index + 1
        ));
    }

    pub(in crate::session) fn edit_editor_table_edge(
        &mut self,
        table_id: ui::NodeId,
        edge: ui::EditorTableEdge,
        edit: app::editor::TableColumnEdit,
        platform: &dyn Platform,
    ) {
        let before = self.app.editor_state().map(EditorState::revision);
        let domain_edge = match edge {
            ui::EditorTableEdge::Left => app::editor::TableColumnEdge::Left,
            ui::EditorTableEdge::Right => app::editor::TableColumnEdge::Right,
        };
        self.apply_editor_command(
            app::editor::EditorCommand::EditTableColumn {
                table_id,
                edge: domain_edge,
                edit,
            },
            platform,
        );
        let changed = before != self.app.editor_state().map(EditorState::revision);
        if changed {
            if let Some(widths) = self.editor_table_column_widths.get_mut(&table_id) {
                widths.clear();
            }
            let action = match edit {
                app::editor::TableColumnEdit::Insert => "added",
                app::editor::TableColumnEdit::Remove => "removed",
            };
            let side = match edge {
                ui::EditorTableEdge::Left => "left",
                ui::EditorTableEdge::Right => "right",
            };
            self.editor_message = Some(format!("Table column {action} on the {side}"));
        } else if edit == app::editor::TableColumnEdit::Remove {
            self.editor_message = Some("A table must keep at least one column".to_string());
        }
        self.editor_open_menu = None;
        self.editor_focus = ui::EditorFocus::Canvas;
        self.editor_drag_anchor = None;
        self.editor_table_resize = None;
    }

    pub(in crate::session) fn editor_quick_menu_is_available(&self) -> bool {
        self.app.editor_state().is_some_and(|state| {
            state.mode == app::editor::EditorMode::Rich
                && state.document.kind == app::editor::DocumentKind::Markdown
                && state.has_selection()
        })
    }

    pub(in crate::session) fn activate_editor_quick_action(
        &mut self,
        action: ui::EditorQuickAction,
        platform: &dyn Platform,
    ) {
        use app::editor::{EditorCommand, FormatCommand};
        use ui::EditorQuickAction;

        if matches!(
            action,
            EditorQuickAction::Paragraph | EditorQuickAction::Heading(_)
        ) && !self
            .app
            .editor_state()
            .is_some_and(EditorState::can_apply_block_format_to_selection)
        {
            return;
        }
        let format = match action {
            EditorQuickAction::Bold => FormatCommand::Bold,
            EditorQuickAction::Italic => FormatCommand::Italic,
            EditorQuickAction::Paragraph => FormatCommand::Paragraph,
            EditorQuickAction::Heading(level) => FormatCommand::Heading(level),
        };
        self.apply_editor_command(EditorCommand::ApplyFormat(format), platform);
    }

    pub(in crate::session) fn activate_editor_toolbar(
        &mut self,
        action: ui::EditorToolbarAction,
        platform: &dyn Platform,
    ) {
        use app::editor::{EditorCommand, FormatCommand};
        if self
            .app
            .editor_state()
            .is_some_and(EditorState::is_read_only)
            && !matches!(
                action,
                ui::EditorToolbarAction::Find | ui::EditorToolbarAction::More
            )
        {
            self.editor_message = Some("This document is read-only".to_string());
            return;
        }
        let command = match action {
            ui::EditorToolbarAction::New => {
                if self
                    .app
                    .editor_state()
                    .is_none_or(|state| !state.is_dirty())
                {
                    self.advance_editor_document_generation();
                    self.app.dispatch_at(
                        app::AppCommand::SetEditorState(Some(EditorState::new())),
                        Instant::now(),
                    );
                    self.editor_quick_menu_anchor = None;
                    self.editor_table_column_widths.clear();
                    self.editor_table_resize = None;
                    self.editor_fingerprint = None;
                    self.rebuild_editor_rich_render_cache();
                } else {
                    self.editor_message =
                        Some("Save or close the current document first".to_string());
                }
                return;
            }
            ui::EditorToolbarAction::Open => EditorCommand::RequestOpen,
            ui::EditorToolbarAction::Save => EditorCommand::RequestSave,
            ui::EditorToolbarAction::Undo => EditorCommand::Undo,
            ui::EditorToolbarAction::Redo => EditorCommand::Redo,
            ui::EditorToolbarAction::ParagraphStyle => {
                EditorCommand::ApplyFormat(FormatCommand::Paragraph)
            }
            ui::EditorToolbarAction::Bold => EditorCommand::ApplyFormat(FormatCommand::Bold),
            ui::EditorToolbarAction::Italic => EditorCommand::ApplyFormat(FormatCommand::Italic),
            ui::EditorToolbarAction::Strikethrough => {
                EditorCommand::ApplyFormat(FormatCommand::Strikethrough)
            }
            ui::EditorToolbarAction::InlineCode => {
                EditorCommand::ApplyFormat(FormatCommand::InlineCode)
            }
            ui::EditorToolbarAction::BulletList => {
                EditorCommand::ApplyFormat(FormatCommand::BulletList)
            }
            ui::EditorToolbarAction::OrderedList => {
                EditorCommand::ApplyFormat(FormatCommand::OrderedList)
            }
            ui::EditorToolbarAction::Quote => EditorCommand::ApplyFormat(FormatCommand::Quote),
            ui::EditorToolbarAction::Table => EditorCommand::ApplyFormat(FormatCommand::Table {
                columns: 3,
                rows: 2,
            }),
            ui::EditorToolbarAction::Link => {
                self.editor_message =
                    Some("Inserted a link placeholder; edit its URL in Source mode".to_string());
                EditorCommand::ApplyFormat(FormatCommand::Link {
                    url: "https://".to_string(),
                    title: None,
                })
            }
            ui::EditorToolbarAction::Image => {
                let alt = self
                    .app
                    .editor_state()
                    .and_then(EditorState::selected_text)
                    .filter(|text| !text.is_empty())
                    .unwrap_or_else(|| "image".to_string());
                self.editor_message =
                    Some("Inserted an image placeholder; edit its path in Source mode".to_string());
                EditorCommand::ApplyFormat(FormatCommand::Image {
                    url: "path/to/image.png".to_string(),
                    alt,
                    title: None,
                })
            }
            ui::EditorToolbarAction::Find | ui::EditorToolbarAction::More => {
                self.editor_message = Some("Use Source mode for this operation".to_string());
                return;
            }
        };
        self.apply_editor_command(command, platform);
    }

    pub(in crate::session) fn editor_hit_targets_at(
        &self,
        coordinates: CellPosition,
    ) -> (Option<ui::EditorHitTarget>, Option<ui::EditorDocumentHit>) {
        let area = Rect::new(0, 0, self.terminal_size.0, self.terminal_size.1);
        let editor_area = match ui::compute_shell_layout(area) {
            ui::ShellLayout::Compact(compact) => compact,
            ui::ShellLayout::Full { main, .. } => main,
        };
        let layout = ui::editor_layout(editor_area, &self.to_editor_view_model());
        (
            layout.hit_test(coordinates.0, coordinates.1),
            layout.hit_test_document(coordinates.0, coordinates.1),
        )
    }

    pub(in crate::session) fn begin_editor_scrollbar_drag(
        &mut self,
        coordinates: CellPosition,
        axis: ScrollbarAxis,
    ) {
        let area = Rect::new(0, 0, self.terminal_size.0, self.terminal_size.1);
        let editor_area = match ui::compute_shell_layout(area) {
            ui::ShellLayout::Compact(compact) => compact,
            ui::ShellLayout::Full { main, .. } => main,
        };
        let layout = ui::editor_layout(editor_area, &self.to_editor_view_model());
        let scrollbar = match axis {
            ScrollbarAxis::Vertical => layout.vertical_scrollbar,
            ScrollbarAxis::Horizontal => layout.horizontal_scrollbar,
        };
        let Some(scrollbar) = scrollbar else {
            return;
        };
        if !rect_contains(scrollbar.thumb, coordinates) {
            self.clear_editor_scrollbar_drag();
            return;
        }
        let grab_offset = match axis {
            ScrollbarAxis::Vertical => coordinates.1.saturating_sub(scrollbar.thumb.y),
            ScrollbarAxis::Horizontal => coordinates.0.saturating_sub(scrollbar.thumb.x),
        };
        self.editor_drag_anchor = None;
        self.editor_table_resize = None;
        self.scrollbar_drag = Some(ScrollbarDragState::Editor { axis, grab_offset });
    }

    pub(in crate::session) fn drag_editor_scrollbar(&mut self, coordinates: CellPosition) {
        let Some(ScrollbarDragState::Editor { axis, grab_offset }) = self.scrollbar_drag else {
            return;
        };
        let area = Rect::new(0, 0, self.terminal_size.0, self.terminal_size.1);
        let editor_area = match ui::compute_shell_layout(area) {
            ui::ShellLayout::Compact(compact) => compact,
            ui::ShellLayout::Full { main, .. } => main,
        };
        let model = self.to_editor_view_model();
        let layout = ui::editor_layout(editor_area, &model);
        let scrollbar = match axis {
            ScrollbarAxis::Vertical => layout.vertical_scrollbar,
            ScrollbarAxis::Horizontal => layout.horizontal_scrollbar,
        };
        let Some(scrollbar) = scrollbar else {
            self.clear_editor_scrollbar_drag();
            return;
        };

        let window_start = match axis {
            ScrollbarAxis::Vertical => scrollbar_window_start(
                coordinates.1,
                grab_offset,
                scrollbar.track.y,
                scrollbar.track.height,
                scrollbar.thumb.height,
                layout.document_line_count,
                layout.visible_capacity,
            ),
            ScrollbarAxis::Horizontal => {
                let visible_capacity = usize::from(layout.canvas.width);
                let content_width = model
                    .horizontal_content_width
                    .max(model.horizontal_scroll.saturating_add(visible_capacity));
                scrollbar_window_start(
                    coordinates.0,
                    grab_offset,
                    scrollbar.track.x,
                    scrollbar.track.width,
                    scrollbar.thumb.width,
                    content_width,
                    visible_capacity,
                )
            }
        };
        if let Some(mut viewport) = self.app.editor_state().map(|state| state.viewport) {
            match axis {
                ScrollbarAxis::Vertical => viewport.top_line = window_start,
                ScrollbarAxis::Horizontal => viewport.left_column = window_start,
            }
            self.app
                .dispatch_at(app::AppCommand::SetEditorViewport(viewport), Instant::now());
        }
    }

    pub(in crate::session) fn clear_editor_scrollbar_drag(&mut self) -> bool {
        if matches!(self.scrollbar_drag, Some(ScrollbarDragState::Editor { .. })) {
            self.scrollbar_drag = None;
            true
        } else {
            false
        }
    }

    pub(in crate::session) fn open_diagnostics_editor(
        &mut self,
        reload: EditorReloadPolicy,
    ) -> Result<(), String> {
        if self.app.editor_state().is_some_and(EditorState::is_dirty) {
            return Err(
                "the current Editor document has unsaved changes; close it before opening diagnostics"
                    .to_string(),
            );
        }
        let path = reload.path().to_path_buf();
        self.begin_editor_open_task(path, EditorTaskAccess::ReadOnly, Some(reload), false)
    }

    pub(in crate::session) fn begin_editor_open_task(
        &mut self,
        path: std::path::PathBuf,
        access: EditorTaskAccess,
        reload: Option<EditorReloadPolicy>,
        replacing_dirty: bool,
    ) -> Result<(), String> {
        if self.editor_load_state.is_some() {
            return Err("another Editor document is already loading".to_string());
        }
        if self.editor_save_state.is_some() {
            return Err("the current Editor document is still saving".to_string());
        }
        let navigation = match self.active_screen() {
            ShellScreen::Explorer
                if matches!(self.explorer_purpose, ExplorerPurpose::EditorOpen) =>
            {
                EditorLoadNavigation::EditorPicker
            }
            ShellScreen::Explorer => EditorLoadNavigation::Explorer,
            ShellScreen::Diagnostics => EditorLoadNavigation::Diagnostics,
            _ => EditorLoadNavigation::Editor,
        };
        let id = next_editor_task_id();
        self.editor_task_runtime
            .submit_load(id, path.clone(), access)?;

        if navigation == EditorLoadNavigation::EditorPicker
            && self.active_screen() == ShellScreen::Explorer
        {
            self.screen_stack.pop();
        }
        if self.active_screen() != ShellScreen::Editor {
            self.screen_stack.push(ShellScreen::Editor);
        }
        self.editor_load_state = Some(EditorLoadState {
            id,
            path: path.clone(),
            stage: EditorTaskStage::Inspecting,
            completed_bytes: 0,
            total_bytes: None,
            operation: EditorLoadOperation::Open {
                navigation,
                reload,
                replacing_dirty,
            },
        });
        self.editor_focus = ui::EditorFocus::Canvas;
        self.editor_open_menu = None;
        self.editor_selected_toolbar_action = None;
        self.editor_quick_menu_anchor = None;
        self.editor_drag_anchor = None;
        self.active_popup = None;
        self.focused_component = ShellComponent::Editor;
        self.editor_message = Some(format!("Loading {}", path.display()));
        self.notify_status(format!("Loading {}", path.display()));
        self.refresh_hit_map();
        Ok(())
    }

    pub(in crate::session) fn reload_log_editor(&mut self) {
        let Some(session) = self.editor_read_session.clone() else {
            return;
        };
        if !matches!(session.reload, EditorReloadPolicy::Log { .. }) {
            return;
        }
        let path = session.reload.path().to_path_buf();
        let model = self.to_editor_view_model();
        let area = Rect::new(0, 0, self.terminal_size.0, self.terminal_size.1);
        let editor_area = match ui::compute_shell_layout(area) {
            ui::ShellLayout::Compact(compact) => compact,
            ui::ShellLayout::Full { main, .. } => main,
        };
        let layout = ui::editor_layout(editor_area, &model);
        let visible_capacity = layout.visible_capacity.max(1);
        let old_maximum = layout.document_line_count.saturating_sub(visible_capacity);
        let was_at_bottom = layout.visible_start >= old_maximum;
        let old_top_line = layout.visible_start;
        let (old_left_column, old_cursor) = self
            .app
            .editor_state()
            .map(|state| (state.viewport.left_column, state.cursor.byte_offset))
            .unwrap_or_default();
        let id = next_editor_task_id();
        if let Err(error) =
            self.editor_task_runtime
                .submit_load(id, path.clone(), EditorTaskAccess::ReadOnly)
        {
            self.report_editor_error(format!("Could not reload {}: {error}", path.display()));
            return;
        }
        self.editor_load_state = Some(EditorLoadState {
            id,
            path: path.clone(),
            stage: EditorTaskStage::Inspecting,
            completed_bytes: 0,
            total_bytes: None,
            operation: EditorLoadOperation::Reload {
                session,
                was_at_bottom,
                visible_capacity,
                old_top_line,
                old_left_column,
                old_cursor,
            },
        });
        self.editor_message = Some(format!("Reloading {}", path.display()));
        self.refresh_hit_map();
    }

    pub(in crate::session) fn cancel_editor_load(&mut self) {
        let Some(load) = self.editor_load_state.take() else {
            return;
        };
        self.editor_task_runtime.cancel(load.id);
        self.restore_editor_load_navigation(&load.operation);
        self.editor_message = Some("Loading cancelled".to_string());
        self.notify_status("Loading cancelled");
        self.refresh_hit_map();
    }

    pub(in crate::session) fn restore_editor_load_navigation(
        &mut self,
        operation: &EditorLoadOperation,
    ) {
        let EditorLoadOperation::Open { navigation, .. } = operation else {
            if self.active_screen() == ShellScreen::Editor {
                self.focused_component = ShellComponent::Editor;
            }
            return;
        };
        match navigation {
            EditorLoadNavigation::EditorPicker => {
                if self.active_screen() == ShellScreen::Editor {
                    self.screen_stack.push(ShellScreen::Explorer);
                } else if let Some(editor_index) = self
                    .screen_stack
                    .iter()
                    .rposition(|screen| *screen == ShellScreen::Editor)
                {
                    let explorer_index = editor_index.saturating_add(1);
                    if self.screen_stack.get(explorer_index) != Some(&ShellScreen::Explorer) {
                        self.screen_stack
                            .insert(explorer_index, ShellScreen::Explorer);
                    }
                }
                if self.active_screen() == ShellScreen::Explorer {
                    self.focused_component = ShellComponent::Explorer;
                }
            }
            EditorLoadNavigation::Explorer => {
                if let Some(editor_index) = (1..self.screen_stack.len()).rev().find(|index| {
                    self.screen_stack[*index] == ShellScreen::Editor
                        && self.screen_stack[index.saturating_sub(1)] == ShellScreen::Explorer
                }) {
                    self.screen_stack.remove(editor_index);
                }
                if self.active_screen() == ShellScreen::Explorer {
                    self.focused_component = ShellComponent::Explorer;
                }
            }
            EditorLoadNavigation::Diagnostics => {
                if let Some(editor_index) = (1..self.screen_stack.len()).rev().find(|index| {
                    self.screen_stack[*index] == ShellScreen::Editor
                        && self.screen_stack[index.saturating_sub(1)] == ShellScreen::Diagnostics
                }) {
                    self.screen_stack.remove(editor_index);
                }
                if self.active_screen() == ShellScreen::Diagnostics {
                    self.focused_component = ShellComponent::Diagnostics;
                }
            }
            EditorLoadNavigation::Editor => {
                if self.active_screen() == ShellScreen::Editor {
                    self.focused_component = ShellComponent::Editor;
                }
            }
        }
    }

    pub(in crate::session) fn poll_editor_background_tasks(&mut self, platform: &dyn Platform) {
        let events = self.editor_task_runtime.drain_events();
        if events.is_empty() {
            return;
        }
        for event in events {
            match event {
                EditorTaskEvent::Progress {
                    id,
                    stage,
                    completed_bytes,
                    total_bytes,
                } => {
                    if let Some(load) = self.editor_load_state.as_mut().filter(|load| load.id == id)
                    {
                        load.stage = stage;
                        load.completed_bytes = completed_bytes;
                        load.total_bytes = total_bytes;
                    }
                    if let Some(save) = self.editor_save_state.as_mut().filter(|save| save.id == id)
                    {
                        save.stage = stage;
                    }
                }
                EditorTaskEvent::LoadFinished { id, result } => {
                    let Some(load) = self.editor_load_state.take_if(|load| load.id == id) else {
                        continue;
                    };
                    match *result {
                        Ok(document) => self.finish_editor_load(load, document),
                        Err(error) => {
                            let action =
                                if matches!(load.operation, EditorLoadOperation::Reload { .. }) {
                                    "reload"
                                } else {
                                    "open"
                                };
                            self.restore_editor_load_navigation(&load.operation);
                            if error != "Editor load cancelled" {
                                self.report_editor_error(format!(
                                    "Could not {action} {}: {error}",
                                    load.path.display()
                                ));
                            }
                        }
                    }
                }
                EditorTaskEvent::SaveFinished { id, result } => {
                    let Some(save) = self.editor_save_state.take_if(|save| save.id == id) else {
                        continue;
                    };
                    self.finish_editor_save(save, result, platform);
                }
            }
        }
        self.refresh_hit_map();
    }

    pub(in crate::session) fn finish_editor_save(
        &mut self,
        save: EditorSaveState,
        result: Result<DocumentFingerprint, EditorSaveTaskError>,
        platform: &dyn Platform,
    ) {
        if save.document_generation != self.editor_document_generation {
            self.editor_close_after_save = false;
            self.editor_open_after_save = false;
            return;
        }
        let path = save.path;
        match result {
            Ok(fingerprint) => {
                self.app.dispatch_at(
                    app::AppCommand::Editor(app::editor::EditorCommand::MarkSaved {
                        path: Some(path.clone()),
                        revision: save.revision,
                    }),
                    Instant::now(),
                );
                let _ = self.app.take_editor_effects();
                self.editor_fingerprint = Some(fingerprint);
                self.clear_editor_recovery();
                if self.app.editor_state().is_some_and(EditorState::is_dirty) {
                    self.editor_recovery_dirty_since = Some(Instant::now());
                }
                self.error_message = None;
                self.resolve_notification_alert(EDITOR_ALERT_KEY);
                self.editor_message = Some(format!("Saved {}", path.display()));
                self.notify_toast(format!("Saved {}", path.display()));
                let close_after_save = std::mem::take(&mut self.editor_close_after_save);
                let open_after_save = std::mem::take(&mut self.editor_open_after_save);
                let clean = self
                    .app
                    .editor_state()
                    .is_none_or(|state| !state.is_dirty());
                if close_after_save && clean {
                    self.finish_editor_close(false);
                } else if open_after_save && clean {
                    self.continue_editor_open_after_save(platform);
                } else if !clean && (close_after_save || open_after_save) {
                    self.editor_message = Some(
                        "Saved an earlier revision; newer edits are still unsaved".to_string(),
                    );
                }
            }
            Err(EditorSaveTaskError::ExternalModification) => {
                self.editor_close_after_save = false;
                self.editor_open_after_save = false;
                self.report_editor_error(
                    "The file changed outside the Editor. Use Save As or reload it before saving.",
                );
            }
            Err(EditorSaveTaskError::Write(error)) => {
                self.editor_close_after_save = false;
                self.editor_open_after_save = false;
                self.report_editor_error(format!("Could not save {}: {error}", path.display()));
            }
        }
    }

    pub(in crate::session) fn finish_editor_load(
        &mut self,
        load: EditorLoadState,
        document: EditorLoadedTaskDocument,
    ) {
        self.advance_editor_document_generation();
        let path = load.path;
        let EditorLoadedTaskDocument {
            mut state,
            fingerprint,
            total_bytes,
            rich_blocks,
        } = document;
        match load.operation {
            EditorLoadOperation::Open {
                navigation,
                reload,
                replacing_dirty,
            } => {
                let open_at_bottom = reload
                    .as_ref()
                    .is_some_and(|reload| matches!(reload, EditorReloadPolicy::Log { .. }));
                if open_at_bottom {
                    let _ = state.apply(app::editor::EditorCommand::MoveCursor {
                        movement: app::editor::CursorMove::DocumentEnd,
                        extend_selection: false,
                    });
                    state.viewport.top_line = state
                        .source_line_count()
                        .unwrap_or_else(|| state.document.line_count())
                        .saturating_sub(1);
                }
                if replacing_dirty {
                    self.clear_editor_recovery();
                }
                self.editor_read_session = reload.map(|reload| EditorReadSession {
                    reload,
                    total_bytes,
                });
                self.editor_rich_render_cache = rich_blocks.map(|blocks| EditorRichRenderCache {
                    revision: state.revision(),
                    blocks,
                });
                self.app
                    .dispatch_at(app::AppCommand::SetEditorState(Some(state)), Instant::now());
                self.editor_cursor_acceleration = None;
                self.editor_settings_dialog = None;
                self.editor_fingerprint = Some(fingerprint);
                self.editor_focus = ui::EditorFocus::Canvas;
                self.editor_open_menu = None;
                self.editor_selected_toolbar_action = None;
                self.editor_quick_menu_anchor = None;
                self.editor_drag_anchor = None;
                self.editor_table_column_widths.clear();
                self.editor_table_resize = None;
                self.editor_close_after_save = false;
                self.editor_open_after_save = false;
                self.editor_discard_for_open = false;
                self.editor_recovery_dirty_since = None;
                self.editor_last_recovery_write = None;
                if navigation == EditorLoadNavigation::EditorPicker {
                    self.explorer_purpose = ExplorerPurpose::Browse;
                    self.replace_explorer_state(None);
                }
                let read_only = self
                    .app
                    .editor_state()
                    .is_some_and(EditorState::is_read_only);
                self.editor_message = Some(if read_only {
                    format!("Read-only: {}", path.display())
                } else {
                    format!("Opened {}", path.display())
                });
            }
            EditorLoadOperation::Reload {
                session,
                was_at_bottom,
                visible_capacity,
                old_top_line,
                old_left_column,
                old_cursor,
            } => {
                let new_line_count = state
                    .source_line_count()
                    .unwrap_or_else(|| state.document.line_count())
                    .max(1);
                let new_maximum = new_line_count.saturating_sub(visible_capacity);
                state.viewport.left_column = old_left_column;
                state.selection = None;
                if was_at_bottom {
                    let _ = state.apply(app::editor::EditorCommand::MoveCursor {
                        movement: app::editor::CursorMove::DocumentEnd,
                        extend_selection: false,
                    });
                    state.viewport.top_line = new_maximum;
                } else {
                    let _ = state.apply(app::editor::EditorCommand::MoveTo {
                        position: app::editor::EditorPosition::Source(old_cursor),
                        extend_selection: false,
                    });
                    state.viewport.top_line = old_top_line.min(new_maximum);
                }
                self.app
                    .dispatch_at(app::AppCommand::SetEditorState(Some(state)), Instant::now());
                self.editor_rich_render_cache = rich_blocks.map(|blocks| EditorRichRenderCache {
                    revision: self.app.editor_state().map_or(0, EditorState::revision),
                    blocks,
                });
                self.editor_fingerprint = Some(fingerprint);
                self.editor_read_session = Some(EditorReadSession {
                    reload: session.reload,
                    total_bytes,
                });
                self.editor_quick_menu_anchor = None;
                self.editor_drag_anchor = None;
                self.editor_message = Some(format!("Reloaded {}", path.display()));
            }
        }
        if self.active_screen() == ShellScreen::Editor {
            self.active_popup = None;
            self.focused_component = ShellComponent::Editor;
        }
        self.notify_status(format!("Editor: {}", path.display()));
        self.refresh_hit_map();
    }

    pub(in crate::session) fn open_editor_path(&mut self, path: std::path::PathBuf) -> bool {
        let replacing_dirty = self.app.editor_state().is_some_and(EditorState::is_dirty);
        if replacing_dirty && !self.editor_discard_for_open {
            self.report_editor_error(
                "The current document has unsaved changes. Use Open in the Editor and choose Save or Discard first.",
            );
            return false;
        }
        if !self.authorize_editor_file(PermissionAction::ReadFile, &path) {
            return false;
        }
        let is_log = is_log_document_path(&path);
        let access = if is_log {
            EditorTaskAccess::ReadOnly
        } else {
            EditorTaskAccess::Editable
        };
        let reload = is_log.then(|| EditorReloadPolicy::Log { path: path.clone() });
        match self.begin_editor_open_task(path.clone(), access, reload, replacing_dirty) {
            Ok(()) => true,
            Err(error) => {
                self.report_editor_error(format!("Could not open {}: {error}", path.display()));
                false
            }
        }
    }

    pub(in crate::session) fn confirm_editor_open(&mut self) {
        self.notify_modal_with_options(
            ShellNotification::modal(
                "Unsaved document",
                "Save your changes before opening another document?",
                ui::NotificationTone::Warning,
                vec![
                    ShellNotificationAction::new("save", "Save")
                        .with_follow_up(ShellCommand::EditorSaveAndOpen),
                    ShellNotificationAction::new("discard", "Discard")
                        .with_follow_up(ShellCommand::EditorDiscardAndOpen),
                    ShellNotificationAction::new("cancel", "Cancel")
                        .cancel()
                        .with_follow_up(ShellCommand::EditorCancelOpen),
                ],
            )
            .with_key(EDITOR_OPEN_NOTIFICATION_KEY)
            .with_component(ShellComponent::NotificationDialog),
        );
    }

    pub(in crate::session) fn open_editor_picker(&mut self, platform: &dyn Platform) {
        self.open_explorer(platform);
        if self.active_screen() == ShellScreen::Explorer {
            self.explorer_purpose = ExplorerPurpose::EditorOpen;
            self.notify_status("Choose a Markdown or text document");
        } else {
            self.editor_open_after_save = false;
            self.editor_discard_for_open = false;
            self.report_editor_error("Could not open the file picker");
        }
    }

    pub(in crate::session) fn open_editor_save_picker(
        &mut self,
        platform: &dyn Platform,
        suggested_name: String,
        snapshot: app::editor::SaveSnapshot,
    ) {
        self.open_explorer(platform);
        if self.active_screen() != ShellScreen::Explorer {
            self.editor_close_after_save = false;
            self.editor_open_after_save = false;
            self.report_editor_error("Could not open the Save As picker");
            return;
        }
        self.explorer_purpose = ExplorerPurpose::EditorSaveAs { snapshot };
        self.begin_explorer_input(ExplorerInputMode::NewTextFile);
        self.explorer_input = suggested_name;
        self.explorer_input_replace_all = true;
        self.notify_status("Save As: enter a file name in the current directory");
    }

    pub(in crate::session) fn submit_editor_save_as_from_explorer(
        &mut self,
        platform: &dyn Platform,
    ) -> bool {
        let ExplorerPurpose::EditorSaveAs { snapshot, .. } = self.explorer_purpose.clone() else {
            return false;
        };
        if self.explorer_input_mode != ExplorerInputMode::NewTextFile {
            return false;
        }
        let name = self.explorer_input.trim();
        let name_path = std::path::Path::new(name);
        let valid_name = !name.is_empty()
            && !name_path.is_absolute()
            && name_path.components().count() == 1
            && matches!(
                name_path.components().next(),
                Some(std::path::Component::Normal(_))
            );
        if !valid_name {
            let _ = self.update_explorer_state(|state| {
                state.error = Some("Enter a single file name without path separators".to_string());
                state.message = None;
            });
            return true;
        }
        let Some(directory) = self
            .app
            .explorer_state()
            .map(|state| state.current_path.clone())
        else {
            self.report_editor_error("Save As destination is unavailable");
            return true;
        };
        let path = directory.join(name);
        if self.save_editor_document(path, snapshot, platform)
            && self.active_screen() == ShellScreen::Explorer
            && !matches!(self.explorer_purpose, ExplorerPurpose::EditorOpen)
        {
            self.return_from_editor_picker();
        }
        true
    }

    pub(in crate::session) fn return_from_editor_picker(&mut self) {
        if self.active_screen() == ShellScreen::Explorer {
            self.screen_stack.pop();
        }
        if self.app.editor_state().is_some() && self.active_screen() != ShellScreen::Editor {
            self.screen_stack.push(ShellScreen::Editor);
        }
        self.explorer_purpose = ExplorerPurpose::Browse;
        self.replace_explorer_state(None);
        self.explorer_input_mode = ExplorerInputMode::Browse;
        self.explorer_input.clear();
        self.explorer_input_replace_all = false;
        self.editor_discard_for_open = false;
        self.focused_component = ShellComponent::Editor;
        self.notify_status("Editor");
        self.refresh_hit_map();
    }

    pub(in crate::session) fn save_editor_document(
        &mut self,
        path: std::path::PathBuf,
        snapshot: app::editor::SaveSnapshot,
        _platform: &dyn Platform,
    ) -> bool {
        if self.editor_save_state.is_some() || self.editor_load_state.is_some() {
            return false;
        }
        if !self.authorize_editor_file(PermissionAction::WriteFile, &path) {
            self.editor_close_after_save = false;
            self.editor_open_after_save = false;
            return false;
        }
        let is_current_path = self
            .app
            .editor_state()
            .and_then(|state| state.document.path.as_ref())
            == Some(&path);
        let expected = is_current_path.then_some(self.editor_fingerprint).flatten();
        let id = next_editor_task_id();
        let revision = snapshot.revision;
        if let Err(error) =
            self.editor_task_runtime
                .submit_save(id, path.clone(), snapshot, expected)
        {
            self.editor_close_after_save = false;
            self.editor_open_after_save = false;
            self.report_editor_error(format!("Could not save {}: {error}", path.display()));
            return false;
        }
        if self.active_screen() == ShellScreen::Explorer
            && matches!(self.explorer_purpose, ExplorerPurpose::EditorSaveAs { .. })
        {
            self.return_from_editor_picker();
        }
        self.editor_save_state = Some(EditorSaveState {
            id,
            path: path.clone(),
            document_generation: self.editor_document_generation,
            revision,
            stage: EditorTaskStage::Writing,
        });
        self.editor_message = Some(format!("Saving {}", path.display()));
        self.notify_status(format!("Saving {}", path.display()));
        self.refresh_hit_map();
        true
    }

    pub(in crate::session) fn continue_editor_open_after_save(&mut self, platform: &dyn Platform) {
        if self.active_screen() == ShellScreen::Explorer {
            self.explorer_purpose = ExplorerPurpose::EditorOpen;
            self.explorer_input_mode = ExplorerInputMode::Browse;
            self.explorer_input.clear();
            self.explorer_input_replace_all = false;
            self.explorer_overlay_mode = None;
            self.focused_component = ShellComponent::Explorer;
            self.notify_status("Choose a Markdown or text document");
            self.apply_explorer_command(ExplorerCommand::Refresh, platform);
            self.refresh_hit_map();
        } else {
            self.open_editor_picker(platform);
        }
    }

    pub(in crate::session) fn authorize_editor_file(
        &mut self,
        action: PermissionAction,
        path: &std::path::Path,
    ) -> bool {
        if self.storage_manager.is_none() {
            return true;
        }
        let authorization = PermissionService::new(self.debug_policy).authorize(
            self.app.auth_session(),
            action,
            Some(path.display().to_string().as_str()),
        );
        if authorization.allowed {
            return true;
        }
        let reason = authorization
            .reason
            .unwrap_or_else(|| "permission_denied".to_string());
        self.report_editor_error(format!("Permission denied: {reason}"));
        false
    }

    pub(in crate::session) fn restore_editor_recovery_if_present(&mut self) {
        let Some((app_paths, user_key)) = self.editor_recovery_context() else {
            return;
        };
        let recovery = match app::editor_recovery::read_versioned_editor_recovery(
            &app_paths,
            user_key.as_str(),
        ) {
            Ok(Some(record)) => record,
            Ok(None) => return,
            Err(error) => {
                self.report_editor_error(format!("Could not read the Editor recovery: {error}"));
                return;
            }
        };

        let (mut state, fingerprint, unbound, warning) = match recovery {
            app::editor_recovery::VersionedEditorRecovery::V1(record) => {
                let kind = if record.markdown {
                    app::editor::DocumentKind::Markdown
                } else {
                    app::editor::DocumentKind::PlainText
                };
                let (mut state, fingerprint, unbound) =
                    editor_recovery_base(record.path.as_ref(), record.saved_content_hash, kind);
                if record.source_mode || kind == app::editor::DocumentKind::PlainText {
                    state.install_source_draft(record.source, record.cursor, None);
                } else {
                    match app::markdown_codec::MarkdownCodec::import_with_metadata(
                        &record.source,
                        state.document.metadata.utf8_bom,
                        editor_recovery_rich_line_ending(
                            state.document.metadata.preferred_line_ending,
                        ),
                    ) {
                        Ok(import) => {
                            let cursor = import.positions.rich_position_for(record.cursor);
                            state.install_rich_draft(import.document, cursor, None);
                        }
                        Err(_) => state.install_source_draft(record.source, record.cursor, None),
                    }
                }
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
        self.editor_quick_menu_anchor = None;
        self.editor_table_column_widths.clear();
        self.editor_table_resize = None;
        self.editor_fingerprint = fingerprint;
        self.editor_recovery_dirty_since = Some(Instant::now());
        self.editor_message = Some(if let Some(warning) = warning {
            warning
        } else if unbound {
            "Recovered as an unbound draft because the original file changed; use Save As"
                .to_string()
        } else {
            "Recovered an unsaved document".to_string()
        });
        self.notify_toast("Recovered an unsaved Editor document");
    }

    pub(in crate::session) fn persist_editor_recovery_if_due(&mut self, now: Instant) {
        if self
            .app
            .editor_state()
            .is_none_or(|state| !state.is_dirty())
        {
            return;
        }
        let Some(dirty_since) = self.editor_recovery_dirty_since else {
            self.editor_recovery_dirty_since = Some(now);
            return;
        };
        if now.saturating_duration_since(dirty_since) < EDITOR_RECOVERY_IDLE
            || self
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
            saved_content_hash: self.editor_fingerprint.map(|value| value.content_hash),
            updated_at_epoch_ms: 0,
            payload,
        };
        record.updated_at_epoch_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_millis().min(u128::from(u64::MAX)) as u64)
            .unwrap_or_default();
        match app::editor_recovery::write_editor_recovery_v2(&app_paths, user_key.as_str(), &record)
        {
            Ok(()) => {
                self.editor_last_recovery_write = Some(now);
                true
            }
            Err(error) => {
                self.report_editor_error(format!("Could not save recovery: {error}"));
                false
            }
        }
    }

    pub(in crate::session) fn editor_recovery_context(
        &self,
    ) -> Option<(platform::AppPaths, String)> {
        let storage = self.storage_manager.as_ref()?;
        let user_key = self.app.auth_session()?.user_id.clone();
        let app_paths = app_paths_from_storage_layout(storage.layout()).ok()?;
        Some((app_paths, user_key))
    }

    pub(in crate::session) fn finish_editor_close(&mut self, _discard: bool) {
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
        if self.active_screen() == ShellScreen::Editor {
            self.screen_stack.pop();
        }
        if self.active_screen() == ShellScreen::Explorer && self.app.explorer_state().is_some() {
            self.focused_component = ShellComponent::Explorer;
            self.notify_status("Explorer");
            self.refresh_hit_map();
        } else if self.active_screen() == ShellScreen::Diagnostics {
            self.focused_component = ShellComponent::Diagnostics;
            self.notify_status("Diagnostics");
            self.refresh_hit_map();
        } else {
            self.pop_to_home();
            self.notify_status("Ready");
        }
    }

    pub(in crate::session) fn report_editor_error(&mut self, message: impl Into<String>) {
        let message = message.into();
        self.editor_message = Some(message.clone());
        self.error_message = Some(message.clone());
        self.notify_alert_with_key(EDITOR_ALERT_KEY, message, ui::NotificationTone::Error);
    }

    pub(in crate::session) fn clear_editor_recovery(&mut self) {
        if let Some((app_paths, user_key)) = self.editor_recovery_context()
            && let Err(error) =
                app::editor_recovery::clear_editor_recovery(&app_paths, user_key.as_str())
        {
            self.editor_message = Some(format!("Could not clear recovery: {error}"));
        }
        self.editor_recovery_dirty_since = None;
        self.editor_last_recovery_write = None;
    }
}
