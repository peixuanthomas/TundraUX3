use super::*;
use crate::session::*;

impl ShellSession {
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
        if self.config_editor_form_visible() {
            self.handle_management_key(&key);
            return;
        }
        #[cfg(target_os = "linux")]
        if key.key == InputKey::F(10) && key.modifiers.control && key.phase == InputPhase::Press {
            self.config_editor_action(ui::EditorConfigAction::Menu);
            return;
        }
        let cursor_direction = editor_cursor_direction(&key);
        if key.phase == InputPhase::Release {
            if self
                .pages
                .editor
                .editor_cursor_acceleration
                .is_some_and(|state| Some(state.direction) == cursor_direction)
            {
                self.pages.editor.editor_cursor_acceleration = None;
            }
            return;
        }
        if !key.phase.is_press_like() {
            return;
        }
        let repeated = key.phase == InputPhase::Repeat;
        if self.pages.editor.editor_settings_dialog.is_some() {
            self.pages.editor.editor_cursor_acceleration = None;
            self.handle_editor_settings_key(&key, repeated);
            return;
        }
        if self.handle_edit_menu_key(&key, platform) {
            return;
        }
        if self.handle_edit_search_key(&key, platform) {
            return;
        }
        if cursor_direction.is_none() {
            self.pages.editor.editor_cursor_acceleration = None;
        }
        if self.pages.editor.editor_save_state.is_some() {
            if key.key == InputKey::Escape && !repeated {
                self.request_editor_close(platform);
            }
            return;
        }
        if self.pages.editor.editor_load_state.is_some() {
            if key.key == InputKey::Escape && !repeated {
                self.cancel_editor_load();
            }
            return;
        }
        if key.key == InputKey::F(8) && !key.has_non_shift_modifier() {
            if !repeated {
                self.toggle_editor_vim();
            }
            return;
        }
        if matches!(key.key, InputKey::Char('r' | 'R'))
            && key.modifiers == InputModifiers::none()
            && self
                .pages
                .editor
                .editor_read_session
                .as_ref()
                .is_some_and(|session| matches!(session.reload, EditorReloadPolicy::Log { .. }))
        {
            if !repeated {
                self.reload_log_editor();
            }
            return;
        }
        if key.key == InputKey::F(3) {
            self.apply_editor_command(
                app::editor::EditorCommand::FindNext {
                    backwards: key.modifiers.shift,
                },
                platform,
            );
            return;
        }
        if key.modifiers.alt && matches!(key.key, InputKey::Char('z' | 'Z')) {
            if !repeated {
                self.toggle_editor_wrap();
            }
            return;
        }
        if key.modifiers.control
            && key.modifiers.shift
            && matches!(key.key, InputKey::Char('j' | 'J'))
        {
            self.apply_editor_command(app::editor::EditorCommand::FormatJson, platform);
            return;
        }
        if key.key == InputKey::Escape {
            if repeated {
                return;
            }
            if self.pages.editor.editor_quick_menu_anchor.take().is_some() {
                self.pages.editor.editor_focus = ui::EditorFocus::Canvas;
                return;
            }
            if self.pages.editor.editor_open_menu.take().is_some()
                || self
                    .pages
                    .editor
                    .editor_selected_toolbar_action
                    .take()
                    .is_some()
            {
                self.pages.editor.editor_focus = ui::EditorFocus::Canvas;
                return;
            }
        }
        self.pages.editor.editor_open_menu = None;
        self.pages.editor.editor_selected_toolbar_action = None;
        self.pages.editor.editor_quick_menu_anchor = None;
        if key.key == InputKey::F(6) {
            self.pages.editor.editor_focus = match self.pages.editor.editor_focus {
                ui::EditorFocus::MenuBar => ui::EditorFocus::Canvas,
                ui::EditorFocus::Toolbar => ui::EditorFocus::Canvas,
                ui::EditorFocus::Canvas => ui::EditorFocus::MenuBar,
                ui::EditorFocus::StatusBar => ui::EditorFocus::MenuBar,
            };
            return;
        }
        // Keyboard editing always returns the live caret to the document after
        // a pointer interaction with a menu or toolbar.
        self.pages.editor.editor_focus = ui::EditorFocus::Canvas;
        if self.handle_editor_vim_key(&key, platform) {
            return;
        }
        let command_key = key.modifiers.control
            || (platform.kind() == PlatformKind::Macos && key.modifiers.super_key);
        if command_key {
            if matches!(key.key, InputKey::Tab | InputKey::BackTab) {
                let pages = self.editor_document_pages();
                if pages.len() > 1 {
                    self.activate_editor_document_page(
                        pages[if key.modifiers.shift {
                            pages.len() - 1
                        } else {
                            1
                        }],
                    );
                }
                return;
            }
            if matches!(key.key, InputKey::Backspace | InputKey::Delete) {
                self.apply_editor_command(
                    app::editor::EditorCommand::DeleteWord {
                        backwards: key.key == InputKey::Backspace,
                    },
                    platform,
                );
                return;
            }
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
            // document and clipboard actions must run once per physical key
            // press. In particular, a repeated Ctrl+W must never close a newly
            // opened document after the first close has already completed.
            if repeated {
                return;
            }
            let character = match key.key {
                InputKey::Char(character) => character.to_ascii_lowercase(),
                _ => '\0',
            };
            let command = match (character, key.modifiers.shift) {
                ('n', _) => {
                    self.new_edit_document();
                    return;
                }
                ('o', _) => app::editor::EditorCommand::RequestOpen,
                ('s', true) => app::editor::EditorCommand::RequestSaveAs,
                ('s', false) => app::editor::EditorCommand::RequestSave,
                ('g', _) => {
                    self.activate_editor_menu_action(ui::EditorMenuAction::GoToLine, platform);
                    return;
                }
                ('p', _) => {
                    self.activate_editor_menu_action(ui::EditorMenuAction::GoToFile, platform);
                    return;
                }
                ('r', _) => {
                    self.open_editor_replace();
                    return;
                }
                ('q', _) => {
                    self.activate_editor_menu_action(ui::EditorMenuAction::Exit, platform);
                    return;
                }
                ('w', _) => app::editor::EditorCommand::RequestClose,
                ('z', false) => app::editor::EditorCommand::Undo,
                ('y', _) | ('z', true) => app::editor::EditorCommand::Redo,
                ('x', false) => app::editor::EditorCommand::Cut,
                ('c', _) => app::editor::EditorCommand::Copy,
                ('v', _) => app::editor::EditorCommand::RequestPaste,
                ('a', _) => app::editor::EditorCommand::SelectAll,
                ('f', _) => {
                    self.open_editor_find();
                    return;
                }
                ('h', _) => {
                    self.open_editor_replace();
                    return;
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
            InputKey::Tab | InputKey::BackTab => app::editor::EditorCommand::Indent {
                width: self.pages.editor.editor_indent_width,
                tabs: self.pages.editor.editor_indent_tabs,
                outdent: key.key == InputKey::BackTab || key.modifiers.shift,
            },
            InputKey::Home => app::editor::EditorCommand::MoveCursor {
                movement: app::editor::CursorMove::LineStart,
                extend_selection: key.modifiers.shift,
            },
            InputKey::End => app::editor::EditorCommand::MoveCursor {
                movement: app::editor::CursorMove::LineEnd,
                extend_selection: key.modifiers.shift,
            },
            InputKey::PageUp | InputKey::PageDown => {
                let capacity = self
                    .editor_current_layout()
                    .visible_capacity
                    .saturating_sub(2)
                    .max(1);
                for _ in 0..capacity {
                    self.apply_editor_command(
                        app::editor::EditorCommand::MoveCursor {
                            movement: if key.key == InputKey::PageUp {
                                app::editor::CursorMove::Up
                            } else {
                                app::editor::CursorMove::Down
                            },
                            extend_selection: key.modifiers.shift,
                        },
                        platform,
                    );
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

    pub(in crate::session) fn toggle_editor_wrap(&mut self) {
        self.pages.editor.editor_soft_wrap = !self.pages.editor.editor_soft_wrap;
        if let Some(mut viewport) = self.app.editor_state().map(|s| s.viewport) {
            viewport.top_line = 0;
            viewport.left_column = 0;
            self.app
                .dispatch_at(app::AppCommand::SetEditorViewport(viewport), Instant::now());
        }
        self.reveal_source_caret();
    }

    pub(in crate::session) fn editor_cursor_step_count(
        &mut self,
        direction: EditorCursorDirection,
        phase: InputPhase,
        received_at: Instant,
    ) -> u8 {
        let config = self.current_editor_config();
        if !config.cursor_acceleration_enabled {
            self.pages.editor.editor_cursor_acceleration = None;
            return 1;
        }
        let starts_new_hold = self
            .pages
            .editor
            .editor_cursor_acceleration
            .is_none_or(|state| {
                state.direction != direction
                    || (phase == InputPhase::Press
                        && (state.reports_repeat
                            || received_at.saturating_duration_since(state.last_event_at)
                                > EDITOR_CURSOR_LEGACY_REPEAT_GAP))
            });
        if starts_new_hold {
            self.pages.editor.editor_cursor_acceleration = Some(EditorCursorAccelerationState {
                direction,
                started_at: received_at,
                last_event_at: received_at,
                reports_repeat: phase == InputPhase::Repeat,
            });
            return 1;
        }
        let Some(state) = self.pages.editor.editor_cursor_acceleration.as_mut() else {
            return 1;
        };
        state.last_event_at = received_at;
        state.reports_repeat |= phase == InputPhase::Repeat;
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

    pub(in crate::session) fn handle_editor_paste(&mut self, value: String) {
        if self.config_editor_form_visible() {
            self.handle_management_paste(&value);
            return;
        }
        if self.handle_edit_search_paste(&value) {
            return;
        }
        self.pages.editor.editor_cursor_acceleration = None;
        self.pages.editor.editor_quick_menu_anchor = None;
        let platform = platform::native_platform();
        self.apply_editor_command(app::editor::EditorCommand::Paste(value), platform.as_ref());
    }

    pub(in crate::session) fn handle_editor_pointer(
        &mut self,
        mouse: MouseInput,
        platform: &dyn Platform,
    ) {
        if self.config_editor_form_visible() {
            self.handle_management_pointer(mouse);
            return;
        }
        let coordinates = mouse.coordinates();
        let (hit, document_hit) = self.editor_hit_targets_at(coordinates);
        if let Some(menu) = self.pages.editor.editor_open_menu {
            let count = ui::editor_menu_actions(menu).len();
            if let ui::MouseEventKind::Scroll(direction) = mouse.kind {
                self.pages.editor.editor_menu_index =
                    if matches!(direction, ScrollDirection::Up | ScrollDirection::Left) {
                        self.pages.editor.editor_menu_index.saturating_sub(1)
                    } else {
                        self.pages
                            .editor
                            .editor_menu_index
                            .saturating_add(1)
                            .min(count.saturating_sub(1))
                    };
                return;
            }
            if hit == Some(ui::EditorHitTarget::MenuScroll)
                && matches!(
                    mouse.kind,
                    ui::MouseEventKind::Down(PointerButton::Left)
                        | ui::MouseEventKind::Drag(PointerButton::Left)
                )
            {
                if let Some(track) = self.editor_current_layout().menu_scrollbar {
                    self.pages.editor.editor_menu_index =
                        usize::from(mouse.row().saturating_sub(track.y))
                            .saturating_mul(count.saturating_sub(1))
                            / usize::from(track.height.saturating_sub(1).max(1));
                }
                return;
            }
        }
        if !matches!(
            mouse,
            MouseInput {
                kind: ui::MouseEventKind::Moved,
                ..
            }
        ) {
            self.pages.editor.editor_cursor_acceleration = None;
        }
        if self.pages.editor.editor_settings_dialog.is_some() {
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
                        if let Some(selected) = self
                            .pages
                            .editor
                            .editor_settings_dialog
                            .as_ref()
                            .map(|dialog| dialog.selected)
                        {
                            self.adjust_editor_setting(selected, -1);
                        }
                    }
                    ScrollDirection::Down | ScrollDirection::Right => {
                        if let Some(selected) = self
                            .pages
                            .editor
                            .editor_settings_dialog
                            .as_ref()
                            .map(|dialog| dialog.selected)
                        {
                            self.adjust_editor_setting(selected, 1);
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
                self.pages.editor.editor_quick_menu_anchor = None;
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
                    self.pages.editor.editor_quick_menu_anchor = None;
                }
                match hit {
                    Some(ui::EditorHitTarget::Search(control)) => {
                        self.focus_edit_search_pointer(control, mouse);
                    }
                    Some(ui::EditorHitTarget::QuickMenuAction(action)) => {
                        self.activate_editor_quick_action(action, platform);
                        self.pages.editor.editor_focus = ui::EditorFocus::Canvas;
                    }
                    Some(ui::EditorHitTarget::QuickMenuPopup) => {}
                    Some(ui::EditorHitTarget::Menu(menu)) => {
                        if menu == ui::EditorMenu::Settings {
                            self.open_editor_settings();
                        } else {
                            self.pages.editor.editor_selected_menu = menu;
                            self.pages.editor.editor_menu_index = 0;
                            self.pages.editor.editor_focus = ui::EditorFocus::MenuBar;
                            self.pages.editor.editor_open_menu =
                                (self.pages.editor.editor_open_menu != Some(menu)).then_some(menu);
                        }
                    }
                    Some(ui::EditorHitTarget::MenuAction(action)) => {
                        self.pages.editor.editor_open_menu = None;
                        self.pages.editor.editor_selected_toolbar_action = None;
                        self.activate_editor_menu_action(action, platform);
                    }
                    Some(ui::EditorHitTarget::MenuPopup) => {}
                    Some(ui::EditorHitTarget::MenuScroll) => {}
                    Some(ui::EditorHitTarget::Toolbar(action)) => {
                        self.pages.editor.editor_open_menu = None;
                        self.pages.editor.editor_selected_toolbar_action = Some(action);
                        self.activate_editor_toolbar(action, platform);
                        self.pages.editor.editor_selected_toolbar_action = None;
                        self.pages.editor.editor_focus = ui::EditorFocus::Canvas;
                    }
                    Some(ui::EditorHitTarget::Mode(mode)) => {
                        self.pages.editor.editor_open_menu = None;
                        self.apply_editor_command(
                            app::editor::EditorCommand::SetMode(mode),
                            platform,
                        );
                        self.pages.editor.editor_focus = ui::EditorFocus::Canvas;
                    }
                    Some(ui::EditorHitTarget::TableEdge { .. }) => {
                        self.pages.editor.editor_message = Some(i18n::LocalizedText::from(
                            i18n::msg!("shell-switch-to-rich-mode-to-edit-table-structure"),
                        ));
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
                        self.pages.editor.editor_message = Some(i18n::LocalizedText::from(
                            i18n::msg!("shell-switch-to-rich-mode-to-resize-table-columns"),
                        ));
                    }
                    Some(ui::EditorHitTarget::RichTableResize {
                        table_id,
                        column_index,
                        width,
                    }) => {
                        self.pages.editor.editor_open_menu = None;
                        self.pages.editor.editor_focus = ui::EditorFocus::Canvas;
                        self.pages.editor.editor_drag_anchor = None;
                        self.pages.editor.editor_table_resize = Some(EditorTableResizeState {
                            table_id,
                            column_index,
                            start_x: coordinates.0,
                            start_width: width,
                        });
                        self.pages.editor.editor_message =
                            Some(i18n::LocalizedText::from(i18n::msg!(
                                "shell-resizing-table-column-arg1-width-cells",
                                arg1 = column_index + 1,
                                width = width
                            )));
                    }
                    Some(ui::EditorHitTarget::Canvas(position)) => {
                        if let Some(search) = &mut self.pages.editor.editor_search_bar {
                            search.focus = None;
                        }
                        self.pages.editor.editor_open_menu = None;
                        self.pages.editor.editor_focus = ui::EditorFocus::Canvas;
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
                                self.pages.editor.editor_message = Some(i18n::LocalizedText::from(
                                    i18n::msg!(
                                        "shell-this-rich-decoration-is-not-directly-editable-click-its-text"
                                    ),
                                ));
                                return;
                            }
                            None if rich_mode => {
                                self.pages.editor.editor_message =
                                    Some(i18n::LocalizedText::from(i18n::msg!(
                                        "shell-this-rich-cell-has-no-editable-text-position"
                                    )));
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
                        self.pages.editor.editor_drag_anchor = Some(position);
                        self.apply_editor_command(
                            app::editor::EditorCommand::MoveTo {
                                position,
                                extend_selection: modifiers.shift,
                            },
                            platform,
                        );
                    }
                    Some(ui::EditorHitTarget::Reload) => self.reload_log_editor(),
                    Some(ui::EditorHitTarget::StatusBar) => {
                        self.pages.editor.editor_open_menu = None;
                        self.pages.editor.editor_focus = ui::EditorFocus::StatusBar;
                    }
                    Some(ui::EditorHitTarget::VerticalScrollbar) => {
                        self.pages.editor.editor_open_menu = None;
                        self.begin_editor_scrollbar_drag(coordinates, ScrollbarAxis::Vertical);
                    }
                    Some(ui::EditorHitTarget::HorizontalScrollbar) => {
                        self.pages.editor.editor_open_menu = None;
                        self.begin_editor_scrollbar_drag(coordinates, ScrollbarAxis::Horizontal);
                    }
                    Some(ui::EditorHitTarget::SettingsControl(_))
                    | Some(ui::EditorHitTarget::SettingsField(_))
                    | Some(ui::EditorHitTarget::SettingsDialog) => {}
                    None => self.pages.editor.editor_open_menu = None,
                }
            }
            MouseInput {
                kind: ui::MouseEventKind::Down(PointerButton::Right),
                ..
            } => {
                self.pages.editor.editor_quick_menu_anchor = None;
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
                    self.pages.editor.editor_open_menu = None;
                    self.pages.editor.editor_selected_toolbar_action = None;
                    self.pages.editor.editor_focus = ui::EditorFocus::Canvas;
                    self.pages.editor.editor_quick_menu_anchor = Some(coordinates);
                }
            }
            MouseInput {
                kind: ui::MouseEventKind::Drag(PointerButton::Left),
                ..
            } => {
                self.pages.editor.editor_quick_menu_anchor = None;
                if matches!(self.scrollbar_drag, Some(ScrollbarDragState::Editor { .. })) {
                    self.drag_editor_scrollbar(coordinates);
                    return;
                }
                if self.pages.editor.editor_table_resize.is_some() {
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
                            self.pages.editor.editor_message = Some(i18n::LocalizedText::from(
                                i18n::msg!("shell-rich-selection-can-only-start-on-editable-text"),
                            ));
                            return;
                        }
                        None if rich_mode => {
                            self.pages.editor.editor_message = Some(i18n::LocalizedText::from(
                                i18n::msg!("shell-this-rich-cell-has-no-editable-text-position"),
                            ));
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
                self.pages.editor.editor_drag_anchor = None;
                self.pages.editor.editor_table_resize = None;
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
        let Some(resize) = self.pages.editor.editor_table_resize else {
            return;
        };
        let delta = i32::from(x) - i32::from(resize.start_x);
        let width = (resize.start_width as i32 + delta).clamp(1, 120) as usize;
        let columns = self
            .pages
            .editor
            .editor_table_column_widths
            .entry(resize.table_id)
            .or_default();
        if columns.len() <= resize.column_index {
            columns.resize(resize.column_index + 1, 0);
        }
        columns[resize.column_index] = width;
        self.pages.editor.editor_message = Some(i18n::LocalizedText::from(i18n::msg!(
            "shell-table-column-arg1-width-width",
            arg1 = resize.column_index + 1,
            width = width
        )));
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
            if let Some(widths) = self
                .pages
                .editor
                .editor_table_column_widths
                .get_mut(&table_id)
            {
                widths.clear();
            }
            let action = match edit {
                app::editor::TableColumnEdit::Insert => {
                    i18n::LocalizedText::from(i18n::msg!("shell-added"))
                }
                app::editor::TableColumnEdit::Remove => {
                    i18n::LocalizedText::from(i18n::msg!("shell-removed"))
                }
            };
            let side = match edge {
                ui::EditorTableEdge::Left => i18n::LocalizedText::from(i18n::msg!("shell-left")),
                ui::EditorTableEdge::Right => i18n::LocalizedText::from(i18n::msg!("shell-right")),
            };
            self.pages.editor.editor_message = Some(i18n::LocalizedText::from(i18n::msg!(
                "shell-table-column-action-on-the-side",
                action = action,
                side = side
            )));
        } else if edit == app::editor::TableColumnEdit::Remove {
            self.pages.editor.editor_message = Some(i18n::LocalizedText::from(i18n::msg!(
                "shell-a-table-must-keep-at-least-one-column"
            )));
        }
        self.pages.editor.editor_open_menu = None;
        self.pages.editor.editor_focus = ui::EditorFocus::Canvas;
        self.pages.editor.editor_drag_anchor = None;
        self.pages.editor.editor_table_resize = None;
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
        if self.config_editor_active()
            && matches!(
                action,
                ui::EditorToolbarAction::New | ui::EditorToolbarAction::Open
            )
        {
            self.notify_toast(i18n::msg!("config-editor-finish-document"));
            return;
        }
        use app::editor::{EditorCommand, FormatCommand};
        if self
            .app
            .editor_state()
            .is_some_and(EditorState::is_read_only)
            && !matches!(
                action,
                ui::EditorToolbarAction::New
                    | ui::EditorToolbarAction::Open
                    | ui::EditorToolbarAction::Find
                    | ui::EditorToolbarAction::More
            )
        {
            self.pages.editor.editor_message = Some(i18n::LocalizedText::from(i18n::msg!(
                "shell-this-document-is-read-only"
            )));
            return;
        }
        let command = match action {
            ui::EditorToolbarAction::New => {
                self.new_edit_document();
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
                self.pages.editor.editor_message = Some(i18n::LocalizedText::from(i18n::msg!(
                    "shell-inserted-a-link-placeholder-edit-its-url-in-source-mode"
                )));
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
                self.pages.editor.editor_message = Some(i18n::LocalizedText::from(i18n::msg!(
                    "shell-inserted-an-image-placeholder-edit-its-path-in-source-mode"
                )));
                EditorCommand::ApplyFormat(FormatCommand::Image {
                    url: "path/to/image.png".to_string(),
                    alt,
                    title: None,
                })
            }
            ui::EditorToolbarAction::Find => {
                self.open_editor_find();
                return;
            }
            ui::EditorToolbarAction::More => {
                self.pages.editor.editor_message = Some(i18n::LocalizedText::from(i18n::msg!(
                    "shell-use-source-mode-for-this-operation"
                )));
                return;
            }
        };
        self.apply_editor_command(command, platform);
    }

    pub(in crate::session) fn editor_hit_targets_at(
        &self,
        coordinates: CellPosition,
    ) -> (Option<ui::EditorHitTarget>, Option<ui::EditorDocumentHit>) {
        let layout = self.editor_current_layout();
        (
            layout.hit_test(coordinates.0, coordinates.1),
            layout.hit_test_document(coordinates.0, coordinates.1),
        )
    }

    pub(in crate::session) fn editor_current_layout(&self) -> ui::EditorLayout {
        let area = Rect::new(0, 0, self.terminal_size.0, self.terminal_size.1);
        let editor_area = match self.shell_layout_for(area) {
            ui::ShellLayout::Compact(compact) => compact,
            ui::ShellLayout::Full { main, .. } => main,
        };
        ui::editor_layout(editor_area, &self.to_editor_view_model())
    }

    pub(in crate::session) fn begin_editor_scrollbar_drag(
        &mut self,
        coordinates: CellPosition,
        axis: ScrollbarAxis,
    ) {
        let area = Rect::new(0, 0, self.terminal_size.0, self.terminal_size.1);
        let editor_area = match self.shell_layout_for(area) {
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
        self.pages.editor.editor_drag_anchor = None;
        self.pages.editor.editor_table_resize = None;
        self.scrollbar_drag = Some(ScrollbarDragState::Editor { axis, grab_offset });
    }

    pub(in crate::session) fn drag_editor_scrollbar(&mut self, coordinates: CellPosition) {
        let Some(ScrollbarDragState::Editor { axis, grab_offset }) = self.scrollbar_drag else {
            return;
        };
        let area = Rect::new(0, 0, self.terminal_size.0, self.terminal_size.1);
        let editor_area = match self.shell_layout_for(area) {
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
                let content_width = layout
                    .horizontal_content_width
                    .max(layout.horizontal_scroll.saturating_add(visible_capacity));
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
}
