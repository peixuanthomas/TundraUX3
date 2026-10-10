use super::*;
use crate::session::*;

impl ShellSession {
    pub fn to_editor_view_model(&self) -> ui::EditorViewModel {
        let _language = i18n::enter_snapshot(self.language.clone());
        if let Some(load) = self.editor_load_state.as_ref()
            && matches!(load.operation, EditorLoadOperation::Open { .. })
        {
            let file_name = load
                .path
                .file_name()
                .and_then(|name| name.to_str())
                .map(str::to_owned)
                .unwrap_or_else(|| i18n::tr!("shell-loading-document"));
            let mut model = ui::EditorViewModel::source(file_name, "");
            model.path_hint = Some(load.path.display().to_string());
            model.read_only = true;
            model.cursor = None;
            model.settings = self.editor_settings_view_model();
            model.text_sizing_protocol = self.terminal_text_sizing_support;
            model.status_message = Some(editor_load_status(load));
            return model;
        }
        let Some(state) = self.app.editor_state() else {
            let mut model = ui::EditorViewModel::new("Untitled.md", Vec::new());
            model.settings = self.editor_settings_view_model();
            model.text_sizing_protocol = self.terminal_text_sizing_support;
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
                model.c_highlights = state.source_c_highlights();
                model.line_markers = state.source_line_markers();
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
        model.configuration = self.config_editor_active();
        if self.config_editor_form_visible() {
            model.config_form = Some(Box::new(self.to_management_view_model()));
        }
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
        model.text_sizing_protocol = self.terminal_text_sizing_support;
        model.status_message = self
            .editor_load_state
            .as_ref()
            .map(editor_load_status)
            .or_else(|| self.editor_save_state.as_ref().map(editor_save_status))
            .or_else(|| {
                self.editor_message
                    .as_ref()
                    .map(i18n::LocalizedText::render_current)
            });
        model
    }

    pub(in crate::session) fn editor_settings_view_model(
        &self,
    ) -> Option<ui::EditorSettingsViewModel> {
        self.editor_settings_dialog
            .as_ref()
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
