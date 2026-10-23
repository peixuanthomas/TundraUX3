use super::*;
use crate::session::*;

impl ShellSession {
    pub fn to_editor_view_model(&self) -> ui::EditorViewModel {
        let _language = i18n::enter_snapshot(self.language.clone());
        if let Some(terminal) = &self.editor_vim_terminal {
            let mut model = ui::EditorViewModel::source(
                self.app
                    .editor_state()
                    .map_or_else(String::new, |s| s.document.display_name()),
                "",
            );
            model.vim_terminal = Some(
                if self.interactive_overlays().is_empty() && !self.config_editor_form_visible() {
                    terminal.clone()
                } else {
                    let mut terminal = terminal.as_ref().clone();
                    for cell in &mut terminal.cells {
                        cell.cursor = false;
                    }
                    Arc::new(terminal)
                },
            );
            model.vim_mode = Some(ui::EditorVimMode::Normal);
            if self.config_editor_form_visible() {
                model.config_form = Some(Box::new(self.to_management_view_model()));
            }
            return model;
        }
        if let Some(load) = self.pages.editor.editor_load_state.as_ref()
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
        let language_path = self
            .pages
            .editor
            .editor_language
            .as_deref()
            .map(editor_language_path)
            .map(str::to_owned)
            .unwrap_or_else(|| {
                state
                    .document
                    .path
                    .as_ref()
                    .map(|p| p.to_string_lossy().into_owned())
                    .unwrap_or_else(|| state.document.display_name())
            });
        let mut model = match state.mode {
            app::editor::EditorMode::Rich => {
                let mut model = if let Some(cache) = self
                    .pages
                    .editor
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
                model.rich_table_column_widths =
                    self.pages.editor.editor_table_column_widths.clone();
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
                if !self.pages.editor.editor_soft_wrap {
                    model.syntax_highlights = state
                        .source_syntax_highlights_for_path(&language_path, first_line..end_line);
                }
                if let Some(search) = &state.search
                    && search.revision == state.revision()
                {
                    model.search_matches = std::sync::Arc::clone(&search.matches);
                }
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
        let saving = self.pages.editor.editor_save_state.is_some();
        model.read_only = state.is_read_only() || saving;
        model.read_window = self
            .pages
            .editor
            .editor_read_session
            .as_ref()
            .map(|session| ui::EditorReadWindowViewModel {
                start_byte: 0,
                total_bytes: session.total_bytes,
            });
        model.reload_available = self
            .pages
            .editor
            .editor_read_session
            .as_ref()
            .is_some_and(|session| matches!(session.reload, EditorReloadPolicy::Log { .. }))
            && !saving;
        model.mode = state.mode;
        model.focus = self.pages.editor.editor_focus;
        model.open_menu = self.pages.editor.editor_open_menu;
        model.search_bar = self.pages.editor.editor_search_bar.clone();
        model.selected_menu = self.pages.editor.editor_selected_menu;
        model.selected_menu_action = self.pages.editor.editor_menu_index;
        model.status_actions = vec![
            (
                ui::EditorMenuAction::Language,
                match self
                    .pages
                    .editor
                    .editor_language
                    .clone()
                    .unwrap_or_else(|| {
                        state
                            .document
                            .path
                            .as_ref()
                            .and_then(|p| p.extension())
                            .and_then(|s| s.to_str())
                            .unwrap_or("Text")
                            .to_owned()
                    })
                    .as_str()
                {
                    "Text" => i18n::tr!("ui-editor-language-text"),
                    language => language.to_owned(),
                },
            ),
            (
                ui::EditorMenuAction::LineEnding,
                editor_line_ending_label(state.document.metadata),
            ),
            (
                ui::EditorMenuAction::Encoding,
                state
                    .document
                    .metadata
                    .encoding
                    .label(state.document.metadata.utf8_bom)
                    .into(),
            ),
            (
                ui::EditorMenuAction::Indentation,
                i18n::tr!(
                    if self.pages.editor.editor_indent_tabs {
                        "ui-editor-indent-tabs"
                    } else {
                        "ui-editor-indent-spaces"
                    },
                    count = self.pages.editor.editor_indent_width
                ),
            ),
        ];
        model.status_actions.push((
            ui::EditorMenuAction::GoToLine,
            format!(
                "Ctrl+G {}:{}",
                model.cursor.unwrap_or_default().line + 1,
                model.cursor.unwrap_or_default().column + 1
            ),
        ));
        model.status_actions.push((
            ui::EditorMenuAction::GoToFile,
            format!(
                "Ctrl+P {}{}",
                state.document.display_name(),
                if self.editor_document_pages().len() > 1 {
                    format!(" +{}", self.editor_document_pages().len() - 1)
                } else {
                    String::new()
                }
            ),
        ));
        model.settings = self.editor_settings_view_model();
        let has_selection = state.has_selection();
        model.quick_menu = self
            .pages
            .editor
            .editor_quick_menu_anchor
            .and_then(|anchor| {
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
        model.selected_toolbar_action = self.pages.editor.editor_selected_toolbar_action;
        model.vim_mode = self
            .app
            .storage_config()
            .editor
            .vim_mode_enabled
            .then_some(ui::EditorVimMode::Normal);
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
        model.encoding = state
            .document
            .metadata
            .encoding
            .label(state.document.metadata.utf8_bom)
            .into();
        model.line_ending = editor_line_ending_label(state.document.metadata);
        model.image_protocol = ui::EditorImageProtocolStatus::Unsupported;
        model.text_sizing_protocol = self.terminal_text_sizing_support;
        model.status_message = self
            .pages
            .editor
            .editor_load_state
            .as_ref()
            .map(editor_load_status)
            .or_else(|| {
                self.pages
                    .editor
                    .editor_save_state
                    .as_ref()
                    .map(editor_save_status)
            })
            .or_else(|| {
                self.pages
                    .editor
                    .editor_message
                    .as_ref()
                    .map(i18n::LocalizedText::render_current)
            });
        if model.status_message.is_none()
            && let Some(search) = &state.search
            && search.revision == state.revision()
        {
            model.status_message = Some(if search.pending {
                i18n::tr!("editor-search-working")
            } else {
                i18n::tr!(
                    "editor-search-count",
                    current = search.current.map_or(0, |i| search.first_match + i + 1),
                    count = search.total_matches
                )
            });
        }
        if model.mode == app::editor::EditorMode::Source && self.pages.editor.editor_soft_wrap {
            model.soft_wrap = true;
            model.horizontal_scroll = 0;
            let area = Rect::new(0, 0, self.terminal_size.0, self.terminal_size.1);
            let editor_area = match self.shell_layout_for(area) {
                ui::ShellLayout::Compact(compact) => compact,
                ui::ShellLayout::Full { main, .. } => main,
            };
            let layout = ui::editor_layout(editor_area, &model);
            model.wrap_layout = state.source_wrap_layout(usize::from(layout.canvas.width));
            if let Some(wrapped) = &model.wrap_layout {
                let first = model.scroll_line.min(wrapped.row_count() - 1);
                let last =
                    (first + usize::from(self.terminal_size.1) + 4).min(wrapped.row_count() - 1);
                model.syntax_highlights = state.source_syntax_highlights_for_path(
                    &language_path,
                    wrapped.row(first).unwrap().logical_line
                        ..wrapped.row(last).unwrap().logical_line + 1,
                );
            } else {
                model.soft_wrap = false;
                if model.status_message.is_none() {
                    model.status_message = Some(i18n::tr!("editor-wrap-working"));
                }
                if let Some(window) = &model.source_window {
                    model.syntax_highlights = state.source_syntax_highlights_for_path(
                        &language_path,
                        window.first_line..window.first_line + window.lines.len(),
                    );
                }
            }
        }
        if let Some(error) = model
            .search_bar
            .as_ref()
            .and_then(|search| search.message.as_ref())
        {
            model.status_message = Some(error.clone());
        }
        model.vim_install = self.current_vim_install().cloned();
        model
    }

    pub(in crate::session) fn editor_settings_view_model(
        &self,
    ) -> Option<ui::EditorSettingsViewModel> {
        self.pages
            .editor
            .editor_settings_dialog
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

fn editor_language_path(language: &str) -> &'static str {
    match language {
        "Rust" => "file.rs",
        "C" => "file.c",
        "C++" => "file.cpp",
        "JSON" => "file.json",
        "Python" => "file.py",
        "JavaScript" => "file.js",
        "TypeScript" => "file.ts",
        "Markdown" => "file.md",
        "TOML" => "file.toml",
        "YAML" => "file.yaml",
        "HTML" => "file.html",
        "CSS" => "file.css",
        "Shell" => "file.sh",
        _ => "file.txt",
    }
}
