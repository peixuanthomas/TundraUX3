use crate::session::*;
use app::editor::{
    EditorCommand,
    search::{PreviewTarget, SearchEngine},
};
use ui::EditorSearchControl as Control;

impl ShellSession {
    pub(in crate::session) fn open_edit_search(&mut self, replace: bool) {
        let selected = self
            .app
            .editor_state()
            .and_then(EditorState::selected_text)
            .filter(|s| !s.contains(['\r', '\n']));
        let mut search = self
            .pages
            .editor
            .editor_search_bar
            .take()
            .or_else(|| self.pages.editor.editor_search_memory.take())
            .unwrap_or_default();
        if let Some(selected) = selected {
            search.query.set_value(selected);
        }
        search.replace = replace && self.app.editor_state().is_some_and(|s| !s.is_read_only());
        search.focus = Some(if search.replace && !search.query.value().is_empty() {
            Control::Replacement
        } else {
            Control::Query
        });
        self.pages.editor.editor_search_bar = Some(search);
        self.pages.editor.editor_open_menu = None;
        self.pages.editor.editor_focus = ui::EditorFocus::Canvas;
        self.run_edit_search(false, false);
    }

    fn run_edit_search(&mut self, next: bool, backwards: bool) {
        let Some(search) = &self.pages.editor.editor_search_bar else {
            return;
        };
        let query = search.query.value().to_owned();
        let options = search.options;
        if let Err(error) = SearchEngine::new(&query, options) {
            self.pages
                .editor
                .editor_search_bar
                .as_mut()
                .unwrap()
                .message = Some(error.to_string());
            self.pages.editor.editor_message = Some(error.to_string().into());
            return;
        }
        self.pages
            .editor
            .editor_search_bar
            .as_mut()
            .unwrap()
            .message = None;
        let platform = platform::native_platform();
        self.apply_editor_command(
            if next {
                EditorCommand::FindNext { backwards }
            } else {
                EditorCommand::Find {
                    query,
                    options,
                    backwards,
                }
            },
            platform.as_ref(),
        );
        self.pages.editor.editor_message = None;
    }

    pub(in crate::session) fn activate_edit_search_control(&mut self, control: Control) {
        let Some(search) = &mut self.pages.editor.editor_search_bar else {
            return;
        };
        search.focus = Some(control);
        match control {
            Control::Query | Control::Replacement => {}
            Control::MatchCase => {
                search.options.case_sensitive = !search.options.case_sensitive;
                self.run_edit_search(false, false);
            }
            Control::WholeWord => {
                search.options.whole_word = !search.options.whole_word;
                self.run_edit_search(false, false);
            }
            Control::Regex => {
                search.options.regex = !search.options.regex;
                self.run_edit_search(false, false);
            }
            Control::Previous | Control::Next => {
                self.run_edit_search(true, control == Control::Previous)
            }
            Control::Close => {
                self.cancel_inline_replacement();
                self.pages.editor.editor_search_memory = self.pages.editor.editor_search_bar.take();
                self.pages.editor.editor_focus = ui::EditorFocus::Canvas;
            }
            Control::Replace | Control::ReplaceAll => {
                self.start_inline_replacement(control == Control::ReplaceAll)
            }
        }
    }

    pub(in crate::session) fn handle_edit_search_key(
        &mut self,
        key: &KeyInput,
        platform: &dyn Platform,
    ) -> bool {
        let Some(search) = &mut self.pages.editor.editor_search_bar else {
            return false;
        };
        if search.focus.is_none() {
            return false;
        }
        if key.phase == InputPhase::Repeat && key.key == InputKey::Enter {
            return true;
        }
        if key.key == InputKey::Escape {
            self.activate_edit_search_control(Control::Close);
            return true;
        }
        if key.modifiers.control
            && matches!(search.focus, Some(Control::Query | Control::Replacement))
        {
            let field = search.focus.unwrap();
            let input = if field == Control::Query {
                &mut search.query
            } else {
                &mut search.replacement
            };
            input.set_focused(true);
            let mut changed = false;
            match key.key {
                InputKey::Char('c' | 'C' | 'x' | 'X') => {
                    if key.phase == InputPhase::Repeat {
                        return true;
                    }
                    if let Some(anchor) = input.selection_anchor().filter(|a| *a != input.cursor())
                    {
                        let start = anchor.min(input.cursor());
                        let end = anchor.max(input.cursor());
                        let text = input
                            .value()
                            .chars()
                            .skip(start)
                            .take(end - start)
                            .collect::<String>();
                        match platform.write_clipboard_text(&text) {
                            Ok(()) if matches!(key.key, InputKey::Char('x' | 'X')) => {
                                input.handle_event_borderless(
                                    InputEvent::key(InputKey::Backspace),
                                    Rect::default(),
                                );
                                changed = true;
                            }
                            Err(error) => search.message = Some(error.to_string()),
                            _ => {}
                        }
                    }
                }
                InputKey::Char('v' | 'V') => {
                    if key.phase == InputPhase::Repeat {
                        return true;
                    }
                    match platform.read_clipboard_text() {
                        Ok(value) => {
                            input
                                .handle_event_borderless(InputEvent::Paste(value), Rect::default());
                            changed = true;
                        }
                        Err(error) => search.message = Some(error.to_string()),
                    }
                }
                InputKey::Left
                | InputKey::Right
                | InputKey::Backspace
                | InputKey::Delete
                | InputKey::Home
                | InputKey::End => {
                    let characters = input.value().chars().collect::<Vec<_>>();
                    let cursor = input.cursor();
                    let mut target = cursor;
                    if matches!(key.key, InputKey::Left | InputKey::Backspace) {
                        while target > 0 && characters[target - 1].is_whitespace() {
                            target -= 1;
                        }
                        while target > 0 && !characters[target - 1].is_whitespace() {
                            target -= 1;
                        }
                    } else if matches!(key.key, InputKey::Right | InputKey::Delete) {
                        while target < characters.len() && !characters[target].is_whitespace() {
                            target += 1;
                        }
                        while target < characters.len() && characters[target].is_whitespace() {
                            target += 1;
                        }
                    } else {
                        target = if key.key == InputKey::Home {
                            0
                        } else {
                            characters.len()
                        };
                    }
                    let anchor = input.selection_anchor().unwrap_or(cursor);
                    input.set_cursor(target);
                    if matches!(key.key, InputKey::Backspace | InputKey::Delete) {
                        input.set_selection_anchor(Some(cursor));
                        input.handle_event_borderless(
                            InputEvent::key(InputKey::Backspace),
                            Rect::default(),
                        );
                        changed = true;
                    } else if key.modifiers.shift {
                        input.set_selection_anchor(Some(anchor));
                    }
                }
                InputKey::Char('z' | 'Z' | 'y' | 'Y') => return true,
                _ => { /* Document commands remain available from a search field. */ }
            }
            if matches!(
                key.key,
                InputKey::Char('c' | 'C' | 'x' | 'X' | 'v' | 'V')
                    | InputKey::Left
                    | InputKey::Right
                    | InputKey::Backspace
                    | InputKey::Delete
                    | InputKey::Home
                    | InputKey::End
            ) {
                if changed {
                    self.cancel_inline_replacement();
                    if field == Control::Query {
                        self.run_edit_search(false, false);
                    }
                }
                return true;
            }
        }
        if key.modifiers.alt
            || (key.modifiers.control
                && !matches!(key.key, InputKey::Char('a' | 'A') | InputKey::Enter))
            || matches!(key.key, InputKey::F(_))
        {
            return false;
        }
        if matches!(key.key, InputKey::Tab | InputKey::BackTab) {
            let controls = ui::editor_search_controls(search.replace);
            let index = controls
                .iter()
                .position(|c| Some(*c) == search.focus)
                .unwrap_or(0);
            search.focus = Some(
                controls[(index
                    + if key.key == InputKey::BackTab {
                        controls.len() - 1
                    } else {
                        1
                    })
                    % controls.len()],
            );
            return true;
        }
        let control = search.focus.unwrap();
        if key.key == InputKey::Enter
            || (key.key == InputKey::Char(' ')
                && !matches!(control, Control::Query | Control::Replacement))
        {
            let action = match control {
                Control::Query => {
                    if key.modifiers.shift {
                        Control::Previous
                    } else {
                        Control::Next
                    }
                }
                Control::Replacement => {
                    if key.modifiers.control {
                        Control::ReplaceAll
                    } else {
                        Control::Replace
                    }
                }
                _ => control,
            };
            self.activate_edit_search_control(action);
            // Enter keeps the text field focused for repeated searches/replacements.
            if matches!(control, Control::Query | Control::Replacement)
                && let Some(search) = &mut self.pages.editor.editor_search_bar
            {
                search.focus = Some(control);
            }
            return true;
        }
        if matches!(control, Control::Query | Control::Replacement) {
            let input = if control == Control::Query {
                &mut search.query
            } else {
                &mut search.replacement
            };
            input.set_focused(true);
            let changed = matches!(
                input.handle_event_borderless(InputEvent::Key(key.clone()), Rect::default()),
                ui::components::ComponentEvent::Changed(_)
            );
            if changed {
                self.cancel_inline_replacement();
                if control == Control::Query {
                    self.run_edit_search(false, false);
                }
            }
        }
        true
    }

    pub(in crate::session) fn handle_edit_search_paste(&mut self, value: &str) -> bool {
        let Some(search) = &mut self.pages.editor.editor_search_bar else {
            return false;
        };
        let input = match search.focus {
            Some(Control::Query) => &mut search.query,
            Some(Control::Replacement) => &mut search.replacement,
            _ => return false,
        };
        let query = search.focus == Some(Control::Query);
        input.set_focused(true);
        input.handle_event_borderless(InputEvent::Paste(value.to_owned()), Rect::default());
        self.cancel_inline_replacement();
        if query {
            self.run_edit_search(false, false);
        }
        true
    }

    pub(in crate::session) fn focus_edit_search_pointer(
        &mut self,
        control: Control,
        mouse: MouseInput,
    ) {
        if matches!(control, Control::Query | Control::Replacement) {
            let (_, area) = self
                .editor_current_layout()
                .search_items
                .into_iter()
                .find(|(c, _)| *c == control)
                .unwrap();
            let Some(search) = &mut self.pages.editor.editor_search_bar else {
                return;
            };
            search.focus = Some(control);
            let input = if control == Control::Query {
                &mut search.query
            } else {
                &mut search.replacement
            };
            input.set_focused(true);
            // The actual editor layout is used below by hit testing; the input
            // prefix is excluded when locating its character cursor.
            let prefix = ratatui::text::Line::raw(format!(
                " {}: ",
                ui::editor_search_label(control)
            ))
            .width() as u16;
            let actual_y = mouse.row();
            input.handle_event_borderless(
                InputEvent::Mouse(mouse),
                Rect::new(
                    area.x.saturating_add(prefix),
                    actual_y,
                    area.width.saturating_sub(prefix),
                    1,
                ),
            );
        } else {
            self.activate_edit_search_control(control);
        }
    }

    fn start_inline_replacement(&mut self, all: bool) {
        let Some(state) = self.app.editor_state() else {
            return;
        };
        if state.is_read_only()
            || self.pages.editor.editor_save_state.is_some()
            || self.pages.editor.editor_load_state.is_some()
        {
            return;
        }
        let Some(search) = &self.pages.editor.editor_search_bar else {
            return;
        };
        let engine = match SearchEngine::new(search.query.value(), search.options) {
            Ok(engine) => engine,
            Err(error) => {
                self.pages.editor.editor_message = Some(error.to_string().into());
                return;
            }
        };
        let Some(source) = state.source_rope_snapshot() else {
            return;
        };
        let target = (!all).then(|| PreviewTarget {
            selection: state.selected_range().map(std::ops::Range::from),
            cursor: state.cursor.byte_offset,
            backwards: false,
        });
        let replacement = search.replacement.value().to_owned();
        let revision = state.revision();
        self.pages.editor.editor_inline_replace_request =
            Some((self.pages.editor.editor_document_generation, revision));
        self.pages
            .editor
            .editor_preview_task
            .start(engine, source, replacement, target);
        self.pages.editor.editor_message = Some(i18n::msg!("editor-search-working").into());
    }

    pub(in crate::session) fn cancel_inline_replacement(&mut self) {
        if self
            .pages
            .editor
            .editor_inline_replace_request
            .take()
            .is_some()
        {
            self.pages.editor.editor_preview_task.cancel();
        }
    }

    pub(in crate::session) fn poll_inline_replacement(&mut self) {
        let Some((generation, revision)) = self.pages.editor.editor_inline_replace_request else {
            return;
        };
        if generation != self.pages.editor.editor_document_generation
            || self
                .app
                .editor_state()
                .is_none_or(|s| s.revision() != revision)
            || self.pages.editor.editor_search_bar.is_none()
        {
            self.cancel_inline_replacement();
            return;
        }
        let Some(result) = self.pages.editor.editor_preview_task.take() else {
            return;
        };
        self.pages.editor.editor_inline_replace_request = None;
        match result {
            Ok(replacements) => {
                let platform = platform::native_platform();
                self.apply_editor_command(
                    EditorCommand::ReplaceMatches {
                        revision,
                        replacements,
                    },
                    platform.as_ref(),
                );
                self.run_edit_search(false, false);
            }
            Err(_) => {
                self.pages.editor.editor_message =
                    Some(i18n::msg!("editor-search-preview-limit").into())
            }
        }
    }
}
