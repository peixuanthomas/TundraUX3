use crate::session::*;
use app::editor::{CursorMove as Move, EditorCommand as Command, EditorPosition};
use ui::EditorVimMode;

impl ShellSession {
    pub(in crate::session) fn toggle_editor_vim(&mut self) {
        let enabled = !self.app.storage_config().editor.vim_mode_enabled;
        let config = (|| {
            let mut config = if let Some(storage) = &self.storage_manager {
                storage.load_config()?
            } else {
                self.app.storage_config().clone()
            };
            config.editor.vim_mode_enabled = enabled;
            if let Some(storage) = &self.storage_manager {
                storage.save_config(&config)?;
            }
            Ok::<_, storage::StorageError>(config)
        })();
        match config {
            Ok(config) => {
                self.replace_storage_config(config);
                self.pages.editor.editor_vim_mode = EditorVimMode::Normal;
                self.pages.editor.editor_vim_pending = None;
                self.pages.editor.editor_cursor_acceleration = None;
            }
            Err(error) => self.report_editor_error(i18n::msg!(
                "shell-could-not-save-editor-settings-error",
                error = error.to_string()
            )),
        }
    }

    pub(super) fn handle_editor_vim_key(
        &mut self,
        key: &KeyInput,
        platform: &dyn Platform,
    ) -> bool {
        if !self.app.storage_config().editor.vim_mode_enabled {
            return false;
        }
        if key.key == InputKey::Escape {
            if self.pages.editor.editor_vim_mode == EditorVimMode::Insert {
                self.vim_move(Move::Left, false, platform);
            }
            self.pages.editor.editor_vim_mode = EditorVimMode::Normal;
            self.pages.editor.editor_vim_pending = None;
            self.apply_editor_command(Command::ClearSelection, platform);
            return true;
        }
        if self.pages.editor.editor_vim_mode == EditorVimMode::Insert {
            return false;
        }
        let pending = self.pages.editor.editor_vim_pending.take();
        if key.modifiers.control
            && !key.modifiers.alt
            && !key.modifiers.shift
            && matches!(key.key, InputKey::Char('r' | 'R'))
        {
            if key.phase == InputPhase::Press {
                self.apply_editor_command(Command::Redo, platform);
            }
            return true;
        }
        if key.has_non_shift_modifier() {
            return false;
        }
        let character = match key.key {
            InputKey::Char(character) => character,
            InputKey::Delete => 'x',
            InputKey::Enter | InputKey::Backspace => {
                self.vim_move(
                    if key.key == InputKey::Enter {
                        Move::Down
                    } else {
                        Move::Left
                    },
                    false,
                    platform,
                );
                return true;
            }
            InputKey::Tab | InputKey::BackTab => return true,
            _ => return false,
        };
        if key.phase == InputPhase::Repeat
            && matches!(
                character,
                'g' | 'd' | 'y' | 'i' | 'a' | 'I' | 'A' | 'o' | 'O'
            )
        {
            return true;
        }
        if pending == Some(character) {
            match character {
                'g' => self.vim_move(Move::DocumentStart, false, platform),
                'd' | 'y' => self.vim_line(character == 'd', platform),
                _ => {}
            }
            return true;
        }
        let movement = match character {
            'h' => Some(Move::Left),
            'j' => Some(Move::Down),
            'k' => Some(Move::Up),
            'l' => Some(Move::Right),
            'w' => Some(Move::WordRight),
            'b' => Some(Move::WordLeft),
            '0' => Some(Move::LineStart),
            '$' => Some(Move::LineEnd),
            'G' => Some(Move::DocumentEnd),
            _ => None,
        };
        if let Some(movement) = movement {
            self.vim_move(movement, false, platform);
            return true;
        }
        match character {
            'g' | 'd' | 'y' => self.pages.editor.editor_vim_pending = Some(character),
            'i' | 'a' | 'I' | 'A' | 'o' | 'O' => {
                match character {
                    'a' => self.vim_move(Move::Right, false, platform),
                    'I' | 'O' => self.vim_move(Move::LineStart, false, platform),
                    'A' | 'o' => self.vim_move(Move::LineEnd, false, platform),
                    _ => {}
                }
                if matches!(character, 'o' | 'O') {
                    self.apply_editor_command(Command::InsertNewline, platform);
                    if character == 'O' {
                        self.vim_move(Move::Left, false, platform);
                    }
                }
                self.pages.editor.editor_vim_mode = EditorVimMode::Insert;
            }
            'x' => {
                self.vim_move(Move::Right, true, platform);
                self.pages.editor.editor_vim_yank = self
                    .app
                    .editor_state()
                    .and_then(|s| s.selected_text())
                    .map(|s| (s, false));
                self.apply_editor_command(Command::DeleteSelection, platform);
            }
            'p' | 'P' => self.vim_paste(character == 'P', platform),
            'u' => self.apply_editor_command(Command::Undo, platform),
            '/' => self.open_editor_find(),
            'n' | 'N' => self.apply_editor_command(
                Command::FindNext {
                    backwards: character == 'N',
                },
                platform,
            ),
            _ => {}
        }
        true
    }

    fn vim_move(&mut self, movement: Move, extend_selection: bool, platform: &dyn Platform) {
        self.apply_editor_command(
            Command::MoveCursor {
                movement,
                extend_selection,
            },
            platform,
        );
    }

    fn vim_line(&mut self, delete: bool, platform: &dyn Platform) {
        let Some(state) = self.app.editor_state() else {
            return;
        };
        let original = state.position();
        let range = state
            .source_position(state.cursor.byte_offset)
            .and_then(|(line, _)| {
                let range = state.source_line_range(line)?;
                let next = state.source_line_range(line + 1);
                let start = if delete && next.is_none() && line > 0 {
                    state.source_line_range(line - 1)?.end
                } else {
                    range.start
                };
                Some((start, next.map_or(range.end, |r| r.start)))
            });
        if let Some((start, end)) = range {
            self.apply_editor_command(
                Command::MoveTo {
                    position: EditorPosition::Source(start),
                    extend_selection: false,
                },
                platform,
            );
            self.apply_editor_command(
                Command::MoveTo {
                    position: EditorPosition::Source(end),
                    extend_selection: true,
                },
                platform,
            );
        } else {
            self.vim_move(Move::LineStart, false, platform);
            self.vim_move(Move::LineEnd, true, platform);
            self.vim_move(Move::Right, true, platform);
        }
        let text = self
            .app
            .editor_state()
            .and_then(|s| s.selected_text())
            .unwrap_or_default();
        self.pages.editor.editor_vim_yank =
            Some((text.trim_matches(['\r', '\n']).to_string(), true));
        if delete {
            self.apply_editor_command(Command::DeleteSelection, platform);
        } else if let Some(position) = original {
            self.apply_editor_command(
                Command::MoveTo {
                    position,
                    extend_selection: false,
                },
                platform,
            );
        }
    }

    fn vim_paste(&mut self, before: bool, platform: &dyn Platform) {
        let Some((text, linewise)) = self.pages.editor.editor_vim_yank.clone() else {
            return;
        };
        if linewise {
            self.vim_move(
                if before {
                    Move::LineStart
                } else {
                    Move::LineEnd
                },
                false,
                platform,
            );
            let ending = self
                .app
                .editor_state()
                .unwrap()
                .document
                .metadata
                .preferred_line_ending
                .as_str();
            let text = if before {
                format!("{text}{ending}")
            } else {
                format!("{ending}{text}")
            };
            self.apply_editor_command(Command::InsertText(text), platform);
        } else {
            if !before {
                self.vim_move(Move::Right, false, platform);
            }
            self.apply_editor_command(Command::InsertText(text), platform);
        }
    }
}
