use super::*;
use crate::session::*;

impl ShellSession {
    pub(in crate::session) fn handle_editor_settings_key(
        &mut self,
        key: &KeyInput,
        repeated: bool,
    ) {
        if self.pages.editor.editor_settings_dialog.is_none() {
            return;
        }
        if !repeated
            && key.modifiers.is_control_only()
            && matches!(key.key, InputKey::Char('s' | 'S'))
        {
            self.save_editor_settings();
            return;
        }
        if !key.has_non_shift_modifier() && !repeated {
            match key.key {
                InputKey::Char('t' | 'T') => {
                    self.activate_editor_settings_control(ui::EditorSettingsControl::ToggleEnabled);
                    return;
                }
                InputKey::Char('r' | 'R') => {
                    self.activate_editor_settings_control(
                        ui::EditorSettingsControl::RestoreDefaults,
                    );
                    return;
                }
                _ => {}
            }
        }
        self.handle_editor_settings_form(InputEvent::Key(key.clone()));
    }

    pub(in crate::session) fn handle_editor_settings_form(&mut self, event: InputEvent) {
        use ui::components::{FormAction, FormEvent};
        let Some(model) = self.editor_settings_view_model() else {
            return;
        };
        let mut form = model.dialog();
        let result = match event {
            InputEvent::Mouse(mouse) => form.handle_confirmed_pointer(mouse, self.touch_main()),
            event => form.handle_event(event, self.touch_main()),
        };
        self.update_editor_settings_form(form);
        match result {
            FormEvent::Action(FormAction::Submit) => self.save_editor_settings(),
            FormEvent::Action(FormAction::Cancel) => {
                self.pages.editor.editor_settings_dialog = None
            }
            FormEvent::Action(FormAction::Custom(_)) => {
                self.activate_editor_settings_control(ui::EditorSettingsControl::RestoreDefaults)
            }
            _ => {}
        }
    }

    pub(in crate::session) fn open_editor_settings(&mut self) {
        self.pages.editor.editor_open_menu = None;
        self.pages.editor.editor_selected_toolbar_action = None;
        self.pages.editor.editor_quick_menu_anchor = None;
        self.pages.editor.editor_cursor_acceleration = None;
        self.pages.editor.editor_settings_dialog = Some(EditorSettingsDialogState {
            interaction: Default::default(),
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
        if let Some(dialog) = self.pages.editor.editor_settings_dialog.as_mut() {
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
                if let Some(model) = self.editor_settings_view_model() {
                    let mut form = model.dialog();
                    form.selected = 0;
                    form.activate_field(0);
                    self.update_editor_settings_form(form);
                }
            }
            Field::ActivationDelay
            | Field::RampDuration
            | Field::HorizontalMaxStep
            | Field::VerticalMaxStep => self.adjust_editor_setting(field, 1),
            Field::RestoreDefaults => {
                if let Some(dialog) = self.pages.editor.editor_settings_dialog.as_mut() {
                    let explorer_open_extensions = dialog.draft.explorer_open_extensions.clone();
                    let vim_mode_enabled = dialog.draft.vim_mode_enabled;
                    dialog.draft = storage::EditorConfig {
                        explorer_open_extensions,
                        vim_mode_enabled,
                        ..storage::EditorConfig::default()
                    };
                }
            }
            Field::Save => self.save_editor_settings(),
            Field::Cancel => self.pages.editor.editor_settings_dialog = None,
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
        let Some(model) = self.editor_settings_view_model() else {
            return;
        };
        let mut form = model.dialog();
        let index = ui::EditorSettingsField::ALL
            .iter()
            .position(|candidate| *candidate == field)
            .unwrap();
        if index == 0 || index >= form.fields.len() {
            return;
        }
        form.selected = index;
        form.adjust_number(index, direction);
        self.update_editor_settings_form(form);
    }

    fn update_editor_settings_form(&mut self, form: ui::components::FormDialog) {
        let editable = self.can_change_editor_settings();
        let dialog = self.pages.editor.editor_settings_dialog.as_mut().unwrap();
        dialog.interaction = form.interaction_state();
        dialog.selected = ui::EditorSettingsField::ALL[form.selected];
        if editable {
            dialog.draft.cursor_acceleration_enabled = form.fields[0].value == "true";
            dialog.draft.cursor_acceleration_delay_ms = form.fields[1].value.parse().unwrap();
            dialog.draft.cursor_acceleration_ramp_ms = form.fields[2].value.parse().unwrap();
            dialog.draft.cursor_horizontal_max_step = form.fields[3].value.parse().unwrap();
            dialog.draft.cursor_vertical_max_step = form.fields[4].value.parse().unwrap();
            dialog.draft = normalized_editor_config(dialog.draft.clone());
        }
    }

    pub(in crate::session) fn save_editor_settings(&mut self) {
        if !self.can_change_editor_settings() {
            self.reject_editor_settings_change();
            return;
        }
        let Some(dialog) = self.pages.editor.editor_settings_dialog.clone() else {
            return;
        };
        let editor = normalized_editor_config(dialog.draft);
        let config = if let Some(storage) = self.storage_manager.as_ref() {
            let mut config = match storage.load_config() {
                Ok(config) => config,
                Err(error) => {
                    self.pages.editor.editor_message = Some(i18n::LocalizedText::from(i18n::msg!(
                        "shell-could-not-save-editor-settings-error",
                        error = error.to_string()
                    )));
                    return;
                }
            };
            config.editor = editor;
            if let Err(error) = storage.save_config(&config) {
                self.pages.editor.editor_message = Some(i18n::LocalizedText::from(i18n::msg!(
                    "shell-could-not-save-editor-settings-error",
                    error = error.to_string()
                )));
                return;
            }
            self.pages.editor.editor_message = Some(i18n::LocalizedText::from(i18n::msg!(
                "shell-editor-settings-saved"
            )));
            config
        } else {
            let mut config = self.app.storage_config().clone();
            config.editor = editor;
            self.pages.editor.editor_message = Some(i18n::LocalizedText::from(i18n::msg!(
                "shell-editor-settings-applied-for-this-session"
            )));
            config
        };
        self.replace_storage_config(config);
        self.pages.editor.editor_cursor_acceleration = None;
        self.pages.editor.editor_settings_dialog = None;
    }
}
