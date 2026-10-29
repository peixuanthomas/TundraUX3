use crate::session::*;
use ui::LauncherEntryAction;
use ui::components::{ButtonRegion, TextInput};

impl ShellSession {
    fn launcher_main_area(&self) -> Option<Rect> {
        match self.shell_layout_for(Rect::new(0, 0, self.terminal_size.0, self.terminal_size.1)) {
            ui::ShellLayout::Full { main, .. } => Some(main),
            _ => None,
        }
    }

    pub(in crate::session) fn open_launcher_context_menu(&mut self, point: CellPosition) {
        if !self.can_manage_launcher() {
            return;
        }
        let Some(main) = self.launcher_main_area() else {
            return;
        };
        let model = self.to_launcher_view_model();
        let Some(ui::LauncherHitTarget::Item(index)) =
            ui::launcher_layout(main, &model).hit_test(point.0, point.1)
        else {
            return;
        };
        let item = &model.items[index];
        if item.is_builtin() {
            return;
        }
        self.select_launcher_index(index);
        self.pages.launcher.launcher_drag = None;
        self.pages.launcher.context_menu = Some(page_state::LauncherContextMenuState {
            id: item.id.clone(),
            menu: ui::LauncherContextMenu {
                anchor: point,
                selected: 0,
                command_enabled: self.can_execute_command_line(),
                wait_for_enter: self
                    .app
                    .launcher_state()
                    .and_then(|state| {
                        state
                            .items
                            .iter()
                            .find(|record| record.record.id == item.id)
                    })
                    .and_then(|item| {
                        item.record
                            .custom
                            .as_ref()
                            .map(|custom| custom.wait_for_enter)
                            .or_else(|| {
                                item.record
                                    .terminal
                                    .as_ref()
                                    .map(|settings| settings.wait_for_enter.unwrap_or(false))
                            })
                    }),
            },
        });
        self.last_click = None;
    }

    fn activate_launcher_context_action(&mut self, index: usize, platform: &dyn Platform) {
        let Some(state) = self.pages.launcher.context_menu.as_ref() else {
            return;
        };
        let Some(action) = state.menu.actions().nth(index) else {
            return;
        };
        if action == LauncherEntryAction::Command && !state.menu.command_enabled {
            return;
        }
        let id = state.id.clone();
        if action == LauncherEntryAction::WaitForEnter {
            let wait = !state.menu.wait_for_enter.unwrap_or(false);
            self.apply_launcher_command(
                LauncherCommand::EditEntry {
                    id,
                    edit: app::launcher::LauncherEntryEdit::WaitForEnter(wait),
                },
                platform,
            );
            if self.storage_manager.is_some()
                && self
                    .app
                    .launcher_state()
                    .is_some_and(|state| state.error.is_none())
            {
                self.pages
                    .launcher
                    .context_menu
                    .as_mut()
                    .unwrap()
                    .menu
                    .wait_for_enter = Some(wait);
            }
            return;
        }
        self.pages.launcher.context_menu = None;
        if action == LauncherEntryAction::Remove {
            self.confirm_launcher_remove();
            return;
        }
        let Some(item) = self
            .app
            .launcher_state()
            .and_then(|state| state.items.iter().find(|item| item.record.id == id))
        else {
            return;
        };
        let record = &item.record;
        let values = match action {
            LauncherEntryAction::Name => {
                vec![app::launcher::launcher_entry_name(record).to_owned()]
            }
            LauncherEntryAction::Command => vec![record.custom.as_ref().map_or_else(
                || {
                    let args = std::iter::once(record.path.as_str()).chain(
                        record
                            .terminal
                            .iter()
                            .flat_map(|settings| settings.arguments.iter().map(String::as_str)),
                    );
                    shell_words::join(args)
                },
                |custom| custom.command.clone(),
            )],
            LauncherEntryAction::Icon => {
                let icon = record
                    .icon
                    .as_deref()
                    .or_else(|| record.custom.as_ref().map(|custom| custom.icon.as_str()))
                    .unwrap_or_default();
                let mut lines = icon.split('\n');
                (0..app::launcher::LAUNCHER_ICON_HEIGHT)
                    .map(|_| {
                        let mut line = String::new();
                        for character in lines
                            .next()
                            .unwrap_or_default()
                            .chars()
                            .filter(|c| !c.is_control())
                        {
                            line.push(character);
                            if !app::launcher::validate_launcher_icon(&line) {
                                line.pop();
                                break;
                            }
                        }
                        line
                    })
                    .collect()
            }
            LauncherEntryAction::WaitForEnter | LauncherEntryAction::Remove => unreachable!(),
        };
        let inputs = values
            .into_iter()
            .enumerate()
            .map(|(index, value)| {
                let mut input =
                    TextInput::new(format!("launcher.edit.{index}")).with_horizontal_scroll(true);
                input.set_value(value);
                input
            })
            .collect();
        self.pages.launcher.entry_editor = Some(ui::LauncherEntryEditor {
            id,
            action,
            inputs,
            selected: 0,
            error: None,
        });
    }

    pub(in crate::session) fn launcher_entry_button_at(
        &self,
        point: CellPosition,
    ) -> Option<ButtonRegion> {
        let main = self.launcher_main_area()?;
        if let Some(state) = &self.pages.launcher.context_menu {
            let (_, rows) = ui::launcher_context_menu_layout(main, &state.menu);
            return rows
                .into_iter()
                .enumerate()
                .find(|(_, area)| rect_contains(*area, point))
                .map(|(index, area)| ButtonRegion {
                    id: format!("launcher.menu.{index}").into(),
                    area,
                    disabled: state.menu.actions().nth(index) == Some(LauncherEntryAction::Command)
                        && !state.menu.command_enabled,
                });
        }
        let editor = self.pages.launcher.entry_editor.as_ref()?;
        if editor.action != LauncherEntryAction::Icon {
            return ui::management_button_regions(
                main,
                &ui::ManagementViewModel {
                    form: Some(editor.form()),
                    ..Default::default()
                },
            )
            .into_iter()
            .find(|button| rect_contains(button.area, point));
        }
        let layout = ui::launcher_entry_editor_layout(main, editor);
        [
            ("launcher.edit.save", layout.submit),
            ("launcher.edit.cancel", layout.cancel),
        ]
        .into_iter()
        .find(|(_, area)| rect_contains(*area, point))
        .map(|(id, area)| ButtonRegion {
            id: id.into(),
            area,
            disabled: false,
        })
    }

    pub(in crate::session) fn handle_launcher_entry_input(
        &mut self,
        input: InputEvent,
        platform: &dyn Platform,
    ) {
        let Some(main) = self.launcher_main_area() else {
            return;
        };
        if let Some(state) = &mut self.pages.launcher.context_menu {
            let (area, rows) = ui::launcher_context_menu_layout(main, &state.menu);
            let count = state.menu.actions().count();
            let action = match input {
                InputEvent::Key(key)
                    if key.phase.is_press_like() && !key.has_non_shift_modifier() =>
                {
                    match key.key {
                        InputKey::Escape => {
                            self.pages.launcher.context_menu = None;
                            None
                        }
                        InputKey::Up | InputKey::BackTab => {
                            state.menu.selected = (state.menu.selected + count - 1) % count;
                            None
                        }
                        InputKey::Down | InputKey::Tab => {
                            state.menu.selected = (state.menu.selected + 1) % count;
                            None
                        }
                        InputKey::Enter | InputKey::Space | InputKey::Char(' ')
                            if key.phase == InputPhase::Press =>
                        {
                            Some(state.menu.selected)
                        }
                        _ => None,
                    }
                }
                InputEvent::Mouse(mouse) => match mouse.kind {
                    ui::MouseEventKind::Down(_) if !rect_contains(area, mouse.coordinates()) => {
                        self.pages.launcher.context_menu = None;
                        None
                    }
                    ui::MouseEventKind::Down(PointerButton::Left) => rows
                        .iter()
                        .position(|row| rect_contains(*row, mouse.coordinates())),
                    _ => None,
                },
                _ => None,
            };
            if let Some(index) = action {
                self.activate_launcher_context_action(index, platform);
            }
            return;
        }
        let Some(editor) = self.pages.launcher.entry_editor.as_mut() else {
            return;
        };
        if editor.action != LauncherEntryAction::Icon {
            let mut form = editor.form();
            let result = match input {
                InputEvent::Mouse(mouse) => form.handle_confirmed_pointer(mouse, main),
                input => form.handle_event(input, main),
            };
            editor.selected = form.selected;
            editor.inputs = form
                .fields
                .into_iter()
                .map(|field| field.input.unwrap())
                .collect();
            match result {
                ui::components::FormEvent::Action(ui::components::FormAction::Submit) => {
                    self.save_launcher_entry_edit(platform)
                }
                ui::components::FormEvent::Action(ui::components::FormAction::Cancel) => {
                    self.pages.launcher.entry_editor = None
                }
                _ => {}
            }
            return;
        }
        let layout = ui::launcher_entry_editor_layout(main, editor);
        let fields = editor.inputs.len();
        let mut save = false;
        let mut cancel = false;
        match input {
            InputEvent::Key(key) if key.phase.is_press_like() => match key.key {
                InputKey::Escape => cancel = true,
                InputKey::F(10) if key.phase == InputPhase::Press => save = true,
                InputKey::Tab | InputKey::Down => {
                    editor.selected = (editor.selected + 1) % (fields + 2)
                }
                InputKey::BackTab | InputKey::Up => {
                    editor.selected = (editor.selected + fields + 1) % (fields + 2)
                }
                InputKey::Enter if key.phase == InputPhase::Press => {
                    if editor.selected == fields + 1 {
                        cancel = true;
                    } else if editor.action == LauncherEntryAction::Icon
                        && editor.selected + 1 < fields
                    {
                        editor.selected += 1;
                    } else {
                        save = true;
                    }
                }
                _ => Self::edit_launcher_entry_text(editor, InputEvent::Key(key), Rect::default()),
            },
            InputEvent::Paste(value) => {
                Self::edit_launcher_entry_text(editor, InputEvent::Paste(value), Rect::default())
            }
            InputEvent::Mouse(mouse)
                if mouse.kind == ui::MouseEventKind::Down(PointerButton::Left) =>
            {
                let point = mouse.coordinates();
                if rect_contains(layout.submit, point) {
                    save = true;
                } else if rect_contains(layout.cancel, point) {
                    cancel = true;
                } else if let Some(index) = layout
                    .inputs
                    .iter()
                    .position(|area| rect_contains(*area, point))
                {
                    editor.selected = index;
                    Self::edit_launcher_entry_text(
                        editor,
                        InputEvent::Mouse(mouse),
                        layout.inputs[index],
                    );
                }
            }
            _ => {}
        }
        if cancel {
            self.pages.launcher.entry_editor = None;
        } else if save {
            self.save_launcher_entry_edit(platform);
        }
    }

    fn edit_launcher_entry_text(
        editor: &mut ui::LauncherEntryEditor,
        input: InputEvent,
        area: Rect,
    ) {
        if editor.selected >= editor.inputs.len() {
            return;
        }
        let previous = editor.inputs.clone();
        if editor.action == LauncherEntryAction::Icon
            && let InputEvent::Paste(value) = &input
        {
            let value = value.replace("\r\n", "\n");
            let lines = value.split('\n').collect::<Vec<_>>();
            if editor.selected + lines.len() > editor.inputs.len()
                || value.chars().any(|c| c.is_control() && c != '\n')
            {
                editor.error = Some(i18n::tr!("app-launcher-icon-bounds"));
                return;
            }
            for (index, line) in lines.iter().enumerate() {
                let field = &mut editor.inputs[editor.selected + index];
                field.set_focused(true);
                field.handle_event_borderless(InputEvent::Paste((*line).to_owned()), area);
            }
        } else {
            let field = &mut editor.inputs[editor.selected];
            field.set_focused(true);
            field.handle_event_borderless(input, area);
        }
        if editor.action == LauncherEntryAction::Icon
            && editor
                .inputs
                .iter()
                .any(|input| !app::launcher::validate_launcher_icon(input.value()))
        {
            editor.inputs = previous;
            editor.error = Some(i18n::tr!("app-launcher-icon-bounds"));
        } else {
            editor.error = None;
        }
    }

    fn save_launcher_entry_edit(&mut self, platform: &dyn Platform) {
        let Some(editor) = &self.pages.launcher.entry_editor else {
            return;
        };
        let edit = match editor.action {
            LauncherEntryAction::Name => {
                app::launcher::LauncherEntryEdit::Name(editor.inputs[0].value().to_owned())
            }
            LauncherEntryAction::Command => {
                app::launcher::LauncherEntryEdit::Command(editor.inputs[0].value().to_owned())
            }
            LauncherEntryAction::Icon => {
                let icon = editor
                    .inputs
                    .iter()
                    .map(TextInput::value)
                    .collect::<Vec<_>>()
                    .join("\n");
                app::launcher::LauncherEntryEdit::Icon(if icon.trim().is_empty() {
                    String::new()
                } else {
                    icon
                })
            }
            LauncherEntryAction::WaitForEnter | LauncherEntryAction::Remove => unreachable!(),
        };
        self.apply_launcher_command(
            LauncherCommand::EditEntry {
                id: editor.id.clone(),
                edit,
            },
            platform,
        );
        if let Some(error) = self
            .app
            .launcher_state()
            .and_then(|state| state.error.as_ref())
        {
            self.pages.launcher.entry_editor.as_mut().unwrap().error = Some(error.render_current());
        } else {
            self.pages.launcher.entry_editor = None;
        }
    }
}
