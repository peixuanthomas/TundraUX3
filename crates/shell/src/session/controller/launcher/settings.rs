use crate::session::*;
use ui::components::TextInput;

#[cfg(test)]
#[path = "../../../../tests/unit/session/controller/launcher/run_settings.rs"]
mod tests;

impl ShellSession {
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
            id: item.record.id.clone(),
            embedded: item.record.terminal.is_some(),
            arguments,
            directory,
            selected: 0,
            error: None,
            field_scroll: None,
            message_scroll: 0,
            scrollbar_drag: None,
        });
        self.pages.launcher.launcher_pending_confirmation = None;
        self.pages.launcher.launcher_drag = None;
    }

    pub(in crate::session) fn launcher_run_settings_form(&self) -> Option<ui::ManagementForm> {
        let state = self.pages.launcher.launcher_run_settings.as_ref()?;
        let mut fields = vec![ui::ManagementFormField {
            id: "mode".into(),
            label: i18n::tr!("shell-launcher-run-mode"),
            value: if state.embedded {
                i18n::tr!("shell-launcher-run-terminal")
            } else {
                i18n::tr!("shell-launcher-run-system")
            },
            choices: vec![
                i18n::tr!("shell-launcher-run-system"),
                i18n::tr!("shell-launcher-run-terminal"),
            ],
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
        Some(ui::ManagementForm {
            identity: format!("launcher-run:{}", state.id),
            title: i18n::tr!("shell-launcher-run-settings-title"),
            message: state
                .error
                .as_ref()
                .map(i18n::LocalizedText::render_current)
                .unwrap_or_else(|| i18n::tr!("shell-launcher-run-help")),
            fields,
            selected: state.selected,
            field_scroll: state.field_scroll,
            message_scroll: state.message_scroll,
            submit_label: Some(i18n::tr!("shell-save")),
            cancel_label: Some(i18n::tr!("shell-cancel")),
            ..Default::default()
        })
    }

    pub(in crate::session) fn handle_launcher_run_settings_key(
        &mut self,
        key: &KeyInput,
        platform: &dyn Platform,
    ) {
        if !key.phase.is_press_like() {
            return;
        }
        let Some(state) = self.pages.launcher.launcher_run_settings.as_mut() else {
            return;
        };
        state.field_scroll = None;
        state.scrollbar_drag = None;
        let fields = if state.embedded { 3 } else { 1 };
        match key.key {
            InputKey::Escape => self.pages.launcher.launcher_run_settings = None,
            InputKey::F(10) if key.phase == InputPhase::Press => {
                self.save_launcher_run_settings(platform)
            }
            InputKey::PageUp => state.message_scroll = state.message_scroll.saturating_sub(3),
            InputKey::PageDown => state.message_scroll = state.message_scroll.saturating_add(3),
            InputKey::Tab | InputKey::Down => state.selected = (state.selected + 1) % (fields + 2),
            InputKey::BackTab | InputKey::Up => {
                state.selected = (state.selected + fields + 1) % (fields + 2)
            }
            InputKey::Enter if key.phase == InputPhase::Press => {
                let index = state.selected;
                if index > 0 && index < fields {
                    self.save_launcher_run_settings(platform);
                } else {
                    self.activate_launcher_run_setting(index, platform);
                }
            }
            InputKey::Space | InputKey::Char(' ') | InputKey::Left | InputKey::Right
                if state.selected == 0 && key.phase == InputPhase::Press =>
            {
                state.embedded = !state.embedded;
                state.error = None;
                state.message_scroll = 0;
            }
            _ => {
                let input = match state.selected {
                    1 if state.embedded => Some(&mut state.arguments),
                    2 if state.embedded => Some(&mut state.directory),
                    _ => None,
                };
                if let Some(input) = input {
                    input.set_focused(true);
                    input.handle_event_borderless(InputEvent::Key(key.clone()), Rect::default());
                    state.error = None;
                    state.message_scroll = 0;
                }
            }
        }
    }

    pub(in crate::session) fn activate_launcher_run_setting(
        &mut self,
        index: usize,
        platform: &dyn Platform,
    ) {
        let Some(state) = self.pages.launcher.launcher_run_settings.as_mut() else {
            return;
        };
        let fields = if state.embedded { 3 } else { 1 };
        state.selected = index;
        state.field_scroll = None;
        state.scrollbar_drag = None;
        if index == 0 {
            state.embedded = !state.embedded;
            state.error = None;
            state.message_scroll = 0;
        } else if index == fields {
            self.save_launcher_run_settings(platform);
        } else if index == fields + 1 {
            self.pages.launcher.launcher_run_settings = None;
        }
    }

    pub(in crate::session) fn paste_launcher_run_settings(&mut self, value: &str) {
        let Some(state) = self.pages.launcher.launcher_run_settings.as_mut() else {
            return;
        };
        if !state.embedded {
            return;
        }
        let input = match state.selected {
            1 => &mut state.arguments,
            2 => &mut state.directory,
            _ => return,
        };
        input.set_focused(true);
        input.handle_event_borderless(InputEvent::Paste(value.to_owned()), Rect::default());
        state.error = None;
        state.message_scroll = 0;
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
        let Some(form) = self.launcher_run_settings_form() else {
            return;
        };
        let ui::ShellLayout::Full { main, .. } =
            self.shell_layout_for(Rect::new(0, 0, self.terminal_size.0, self.terminal_size.1))
        else {
            return;
        };
        let layout = ui::management_layout(
            main,
            &ui::ManagementViewModel {
                form: Some(form),
                ..Default::default()
            },
        );
        let state = self.pages.launcher.launcher_run_settings.as_mut().unwrap();
        let point = mouse.coordinates();
        if let Some((target, grab)) = state.scrollbar_drag {
            if matches!(
                mouse.kind,
                ui::MouseEventKind::Drag(PointerButton::Left)
                    | ui::MouseEventKind::Up(PointerButton::Left)
            ) {
                if let Some(bar) = layout.scrollbars.iter().find(|bar| bar.target == target) {
                    let offset = bar.offset_at(point, grab);
                    if target == ui::ManagementScrollTarget::FormMessage {
                        state.message_scroll = offset.min(usize::from(u16::MAX)) as u16;
                    } else {
                        state.field_scroll = Some(offset);
                    }
                }
                if matches!(mouse.kind, ui::MouseEventKind::Up(_)) {
                    state.scrollbar_drag = None;
                }
                return;
            }
        }
        match mouse.kind {
            ui::MouseEventKind::Scroll(direction) => {
                if rect_contains(layout.message, point) {
                    state.message_scroll = match direction {
                        ScrollDirection::Up => state.message_scroll.saturating_sub(3),
                        ScrollDirection::Down => state.message_scroll.saturating_add(3),
                        _ => state.message_scroll,
                    };
                    return;
                }
                let offset = layout.field_start;
                state.field_scroll = Some(match direction {
                    ScrollDirection::Up => offset.saturating_sub(3),
                    ScrollDirection::Down => offset.saturating_add(3),
                    _ => offset,
                });
            }
            ui::MouseEventKind::Down(PointerButton::Left) => {
                if let Some(bar) = layout.scrollbars.iter().find(|bar| {
                    matches!(
                        bar.target,
                        ui::ManagementScrollTarget::FormFields
                            | ui::ManagementScrollTarget::FormMessage
                    ) && rect_contains(bar.track, point)
                }) {
                    let grab = bar.grab_at(point);
                    state.scrollbar_drag = Some((bar.target, grab));
                    let offset = bar.offset_at(point, grab);
                    if bar.target == ui::ManagementScrollTarget::FormMessage {
                        state.message_scroll = offset.min(usize::from(u16::MAX)) as u16;
                    } else {
                        state.field_scroll = Some(offset);
                    }
                    return;
                }
                self.activate_launcher_at(point, platform);
            }
            _ => {}
        }
    }
}
