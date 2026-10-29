use crate::session::*;
use ui::components::{FormAction, FormEvent, TextInput};

#[cfg(test)]
#[path = "../../../../tests/unit/session/controller/launcher/run_settings.rs"]
mod tests;

impl ShellSession {
    pub(in crate::session) fn open_launcher_add(&mut self) {
        if !self.pages.launcher.organizing || !self.can_manage_launcher() {
            return;
        }
        self.pages.launcher.launcher_run_settings = Some(page_state::LauncherRunSettingsState {
            custom: true,
            wait_for_enter: false,
            id: String::new(),
            embedded: true,
            name: TextInput::new("launcher.custom.name").with_horizontal_scroll(true),
            arguments: TextInput::new("launcher.custom.command").with_horizontal_scroll(true),
            directory: TextInput::new("launcher.run.directory").with_horizontal_scroll(true),
            selected: 0,
            error: None,
            field_scroll: None,
            message_scroll: 0,
            interaction: Default::default(),
        });
        self.pages.launcher.launcher_drag = None;
    }

    pub(in crate::session) fn open_launcher_run_settings(&mut self) {
        if !self.can_manage_launcher() {
            return;
        }
        let Some(index) = self.selected_external_launcher_index() else {
            return;
        };
        let Some(item) = self
            .app
            .launcher_state()
            .and_then(|state| state.items.get(index))
        else {
            return;
        };
        if !matches!(
            item.record.executable_kind,
            Some(LauncherExecutableKind::NativeBinary)
        ) && !(cfg!(unix) && item.record.executable_kind == Some(LauncherExecutableKind::Script))
        {
            return;
        }
        let mut arguments = TextInput::new("launcher.run.arguments").with_horizontal_scroll(true);
        let mut directory = TextInput::new("launcher.run.directory").with_horizontal_scroll(true);
        if let Some(settings) = &item.record.terminal {
            arguments.set_value(shell_words::join(&settings.arguments));
            directory.set_value(settings.working_directory.as_deref().unwrap_or_default());
        }
        self.pages.launcher.launcher_run_settings = Some(page_state::LauncherRunSettingsState {
            custom: false,
            wait_for_enter: false,
            name: TextInput::new("launcher.custom.name"),
            id: item.record.id.clone(),
            embedded: item.record.terminal.is_some(),
            arguments,
            directory,
            selected: 0,
            error: None,
            field_scroll: None,
            message_scroll: 0,
            interaction: Default::default(),
        });
        self.pages.launcher.launcher_pending_confirmation = None;
        self.pages.launcher.launcher_drag = None;
    }

    pub(in crate::session) fn launcher_run_settings_form(&self) -> Option<ui::ManagementForm> {
        let state = self.pages.launcher.launcher_run_settings.as_ref()?;
        let mut fields = vec![ui::ManagementFormField {
            id: "mode".into(),
            label: i18n::tr!("shell-launcher-run-terminal"),
            value: state.embedded.to_string(),
            control: ui::ManagementFormControl::Toggle,
            ..Default::default()
        }];
        if state.embedded {
            fields.extend([
                ui::ManagementFormField {
                    id: "arguments".into(),
                    label: i18n::tr!("shell-launcher-run-arguments"),
                    value: state.arguments.value().to_owned(),
                    input: Some(state.arguments.clone()),
                    ..Default::default()
                },
                ui::ManagementFormField {
                    id: "directory".into(),
                    label: i18n::tr!("shell-launcher-run-directory"),
                    value: state.directory.value().to_owned(),
                    input: Some(state.directory.clone()),
                    ..Default::default()
                },
            ]);
        }
        if state.custom {
            fields = [
                ("name", i18n::tr!("shell-launcher-custom-name"), &state.name),
                (
                    "command",
                    i18n::tr!("shell-launcher-custom-command"),
                    &state.arguments,
                ),
            ]
            .into_iter()
            .map(|(id, label, input)| ui::ManagementFormField {
                id: id.into(),
                label,
                value: input.value().to_owned(),
                input: Some(input.clone()),
                ..Default::default()
            })
            .collect();
            fields.push(ui::ManagementFormField {
                id: "exit".into(),
                label: i18n::tr!("ui-launcher-keep-screen"),
                value: state.wait_for_enter.to_string(),
                control: ui::ManagementFormControl::Toggle,
                ..Default::default()
            });
        }
        Some(ui::ManagementForm {
            identity: format!("launcher-run:{}", state.id),
            title: if state.custom {
                i18n::tr!("ui-launcher-add")
            } else {
                i18n::tr!("shell-launcher-run-settings-title")
            },
            message: state
                .error
                .as_ref()
                .map(i18n::LocalizedText::render_current)
                .unwrap_or_else(|| {
                    if state.custom {
                        i18n::tr!("shell-launcher-custom-help")
                    } else {
                        i18n::tr!("shell-launcher-run-help")
                    }
                }),
            fields,
            selected: state.selected,
            field_scroll: state.field_scroll,
            message_scroll: state.message_scroll,
            interaction: state.interaction.clone(),
            submit_label: Some(i18n::tr!("shell-save")),
            cancel_label: Some(i18n::tr!("shell-cancel")),
            ..Default::default()
        })
    }

    fn edit_launcher_form(
        &mut self,
        edit: impl FnOnce(&mut ui::components::FormDialog, Rect) -> FormEvent,
    ) -> FormEvent {
        let Some(mut form) = self.launcher_run_settings_form() else {
            return FormEvent::None;
        };
        let ui::ShellLayout::Full { main, .. } =
            self.shell_layout_for(Rect::new(0, 0, self.terminal_size.0, self.terminal_size.1))
        else {
            return FormEvent::None;
        };
        let event = edit(&mut form, main);
        let state = self.pages.launcher.launcher_run_settings.as_mut().unwrap();
        state.selected = form.selected;
        state.field_scroll = form.field_scroll;
        state.message_scroll = form.message_scroll;
        state.interaction = form.interaction;
        let mut changed = false;
        for field in form.fields {
            let input = match field.id.as_str() {
                "name" => Some(&mut state.name),
                "command" | "arguments" => Some(&mut state.arguments),
                "directory" => Some(&mut state.directory),
                "exit" => {
                    let wait = field.value == "true";
                    changed |= state.wait_for_enter != wait;
                    state.wait_for_enter = wait;
                    None
                }
                "mode" => {
                    let embedded = field.value == "true";
                    changed |= state.embedded != embedded;
                    state.embedded = embedded;
                    None
                }
                _ => None,
            };
            if let (Some(target), Some(input)) = (input, field.input) {
                changed |= target.value() != input.value();
                *target = input;
            }
        }
        if changed {
            state.error = None;
            state.message_scroll = 0;
        }
        event
    }

    fn apply_launcher_form_event(&mut self, event: FormEvent, platform: &dyn Platform) {
        match event {
            FormEvent::Action(FormAction::Submit) => self.save_launcher_run_settings(platform),
            FormEvent::Action(FormAction::Cancel) => {
                self.pages.launcher.launcher_run_settings = None
            }
            _ => {}
        }
    }

    pub(in crate::session) fn handle_launcher_run_settings_key(
        &mut self,
        key: &KeyInput,
        platform: &dyn Platform,
    ) {
        let event = self
            .edit_launcher_form(|form, main| form.handle_event(InputEvent::Key(key.clone()), main));
        self.apply_launcher_form_event(event, platform);
    }

    pub(in crate::session) fn activate_launcher_run_setting(
        &mut self,
        index: usize,
        platform: &dyn Platform,
    ) {
        let event = self.edit_launcher_form(|form, _| {
            form.selected = index;
            form.field_scroll = None;
            if index < form.fields.len() {
                form.activate_field(index)
            } else {
                form.activate_button(index - form.fields.len())
            }
        });
        self.apply_launcher_form_event(event, platform);
    }

    pub(in crate::session) fn paste_launcher_run_settings(&mut self, value: &str) {
        self.edit_launcher_form(|form, main| {
            form.handle_event(InputEvent::Paste(value.to_owned()), main)
        });
    }

    pub(in crate::session) fn save_launcher_run_settings(&mut self, platform: &dyn Platform) {
        if self.storage_manager.is_none() {
            if let Some(state) = &mut self.pages.launcher.launcher_run_settings {
                state.error = Some(i18n::msg!("shell-storage-unavailable").into());
            }
            return;
        }
        let Some(state) = self.pages.launcher.launcher_run_settings.as_ref() else {
            return;
        };
        if state.custom {
            let custom = storage::LauncherCustomCommand {
                name: state.name.value().to_owned(),
                command: state.arguments.value().to_owned(),
                icon: String::new(),
                wait_for_enter: state.wait_for_enter,
            };
            self.apply_launcher_command(LauncherCommand::AddCustom(custom), platform);
            if let Some(error) = self
                .app
                .launcher_state()
                .and_then(|state| state.error.clone())
            {
                self.pages
                    .launcher
                    .launcher_run_settings
                    .as_mut()
                    .unwrap()
                    .error = Some(error);
            } else {
                self.pages.launcher.launcher_run_settings = None;
            }
            return;
        }
        let settings = if state.embedded {
            let arguments = match shell_words::split(state.arguments.value()) {
                Ok(arguments) => arguments,
                Err(_) => {
                    self.pages
                        .launcher
                        .launcher_run_settings
                        .as_mut()
                        .unwrap()
                        .error = Some(i18n::msg!("shell-launcher-run-invalid-arguments").into());
                    return;
                }
            };
            Some(storage::LauncherTerminalSettings {
                arguments,
                working_directory: (!state.directory.value().is_empty())
                    .then(|| state.directory.value().to_owned()),
                wait_for_enter: self
                    .app
                    .launcher_state()
                    .and_then(|launcher| {
                        launcher
                            .items
                            .iter()
                            .find(|item| item.record.id == state.id)
                    })
                    .and_then(|item| item.record.terminal.as_ref())
                    .and_then(|settings| settings.wait_for_enter),
            })
        } else {
            None
        };
        let id = state.id.clone();
        self.apply_launcher_command(
            LauncherCommand::SetTerminalSettings { id, settings },
            platform,
        );
        if let Some(error) = self
            .app
            .launcher_state()
            .and_then(|state| state.error.clone())
        {
            if let Some(form) = self.pages.launcher.launcher_run_settings.as_mut() {
                form.error = Some(error);
            }
        } else {
            self.pages.launcher.launcher_run_settings = None;
        }
    }

    pub(in crate::session) fn launcher_run_settings_button_at(
        &self,
        point: CellPosition,
    ) -> Option<ui::components::ButtonRegion> {
        let form = self.launcher_run_settings_form()?;
        let ui::ShellLayout::Full { main, .. } =
            self.shell_layout_for(Rect::new(0, 0, self.terminal_size.0, self.terminal_size.1))
        else {
            return None;
        };
        ui::management_button_regions(
            main,
            &ui::ManagementViewModel {
                form: Some(form),
                ..Default::default()
            },
        )
        .into_iter()
        .find(|button| rect_contains(button.area, point))
    }

    pub(in crate::session) fn handle_launcher_run_settings_pointer(
        &mut self,
        mouse: MouseInput,
        platform: &dyn Platform,
    ) {
        let event =
            self.edit_launcher_form(|form, main| form.handle_confirmed_pointer(mouse, main));
        self.apply_launcher_form_event(event, platform);
    }
}
