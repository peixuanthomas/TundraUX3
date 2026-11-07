use crate::session::*;
use ui::ExplorerSaveFocus;

impl ShellSession {
    pub(in crate::session) fn focus_explorer_save_picker(&mut self, next: ExplorerSaveFocus) {
        if let ExplorerPurpose::EditorSaveAs { input, focus, .. } =
            &mut self.pages.explorer.explorer_purpose
        {
            *focus = next;
            input.set_focused(next == ExplorerSaveFocus::Filename);
            if next != ExplorerSaveFocus::Browse {
                self.pages.explorer.explorer_input_mode = ExplorerInputMode::Browse;
            }
        }
    }

    pub(in crate::session) fn select_explorer_save_filename(&mut self) -> bool {
        let Some(entry) = self
            .app
            .explorer_state()
            .and_then(|state| state.selected_entry())
        else {
            return false;
        };
        if entry.kind == app::explorer::ExplorerEntryKind::Directory {
            return false;
        }
        let name = entry.name.clone();
        let ExplorerPurpose::EditorSaveAs { input, .. } = &mut self.pages.explorer.explorer_purpose
        else {
            return false;
        };
        input.set_value(name);
        input.set_cursor(input.value().chars().count());
        input.set_selection_anchor(Some(0));
        true
    }

    pub(in crate::session) fn handle_explorer_save_picker_input(
        &mut self,
        mut event: InputEvent,
        platform: &dyn Platform,
    ) {
        let ExplorerPurpose::EditorSaveAs { focus, .. } = self.pages.explorer.explorer_purpose
        else {
            return;
        };
        if let InputEvent::Key(key) = &event {
            if key.phase == InputPhase::Release {
                return;
            }
            if !key.has_non_shift_modifier() {
                match key.key {
                    InputKey::Tab | InputKey::BackTab => {
                        let order = [
                            ExplorerSaveFocus::Browse,
                            ExplorerSaveFocus::Filename,
                            ExplorerSaveFocus::Save,
                            ExplorerSaveFocus::Cancel,
                        ];
                        let index = order.iter().position(|item| *item == focus).unwrap();
                        let reverse = key.key == InputKey::BackTab || key.modifiers.shift;
                        self.focus_explorer_save_picker(
                            order[(index + if reverse { 3 } else { 1 }) % 4],
                        );
                        return;
                    }
                    InputKey::Escape if key.phase == InputPhase::Press => {
                        self.close_explorer();
                        return;
                    }
                    InputKey::F(10) | InputKey::Enter
                        if key.phase == InputPhase::Press
                            && (key.key != InputKey::F(10) || !key.modifiers.shift) =>
                    {
                        if focus == ExplorerSaveFocus::Cancel && key.key != InputKey::F(10) {
                            self.close_explorer();
                        } else {
                            self.submit_editor_save_as_from_explorer(platform);
                        }
                        return;
                    }
                    InputKey::Space | InputKey::Char(' ')
                        if focus != ExplorerSaveFocus::Filename
                            && key.phase == InputPhase::Press =>
                    {
                        if focus == ExplorerSaveFocus::Cancel {
                            self.close_explorer();
                        } else if focus == ExplorerSaveFocus::Save {
                            self.submit_editor_save_as_from_explorer(platform);
                        }
                        return;
                    }
                    _ => {}
                }
            }
        }
        if focus != ExplorerSaveFocus::Filename {
            return;
        }
        let layout = ui::explorer_layout(self.touch_main(), &self.to_explorer_view_model());
        let ExplorerPurpose::EditorSaveAs { input, .. } = &mut self.pages.explorer.explorer_purpose
        else {
            return;
        };
        if let InputEvent::Key(key) = &event
            && key.phase == InputPhase::Press
            && (key.modifiers.control || key.modifiers.super_key)
            && !key.modifiers.alt
        {
            match key.key {
                InputKey::Char('c' | 'C' | 'x' | 'X') => {
                    if let Some(text) = input.selected_text() {
                        if let Err(error) = platform.write_clipboard_text(&text) {
                            self.notify_status(error.to_string());
                            return;
                        }
                        if matches!(key.key, InputKey::Char('x' | 'X')) {
                            event = InputEvent::key(InputKey::Backspace);
                        }
                    }
                }
                InputKey::Char('v' | 'V') => match platform.read_clipboard_text() {
                    Ok(value) => event = InputEvent::Paste(value),
                    Err(error) => {
                        self.notify_status(error.to_string());
                        return;
                    }
                },
                _ => {}
            }
        }
        input.handle_event_borderless(event, layout.save_name);
    }

    pub(in crate::session) fn explorer_save_button_at(
        &self,
        point: CellPosition,
    ) -> Option<ui::components::ButtonRegion> {
        if self.active_screen() != ShellScreen::Explorer || !self.interactive_overlays().is_empty()
        {
            return None;
        }
        let layout = ui::explorer_layout(self.touch_main(), &self.to_explorer_view_model());
        let (id, area) = match layout.hit_test(point.0, point.1)? {
            ui::ExplorerHitTarget::SavePicker(ExplorerSaveFocus::Save) => {
                ("explorer.save", layout.save_button)
            }
            ui::ExplorerHitTarget::SavePicker(ExplorerSaveFocus::Cancel) => {
                ("explorer.save.cancel", layout.save_cancel)
            }
            _ => return None,
        };
        Some(ui::components::ButtonRegion {
            id: id.into(),
            area,
            disabled: false,
        })
    }
}
