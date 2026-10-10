use super::*;
use crate::session::*;

impl ShellSession {
    pub(in crate::session) fn handle_editor_settings_key(
        &mut self,
        key: &KeyInput,
        repeated: bool,
    ) {
        let Some(selected) = self
            .editor_settings_dialog
            .as_ref()
            .map(|dialog| dialog.selected)
        else {
            return;
        };
        if key.modifiers.is_control()
            && !key.modifiers.alt
            && !key.modifiers.super_key
            && !key.modifiers.hyper
            && !key.modifiers.meta
            && !key.modifiers.shift
            && matches!(key.key, InputKey::Char('s' | 'S'))
            && !repeated
        {
            self.activate_editor_settings_control(ui::EditorSettingsControl::Save);
            return;
        }
        if key.has_non_shift_modifier() {
            return;
        }
        match key.key {
            InputKey::Escape if !repeated => self.editor_settings_dialog = None,
            InputKey::Tab | InputKey::Down => {
                self.select_editor_setting(selected.next());
            }
            InputKey::BackTab | InputKey::Up => {
                self.select_editor_setting(selected.previous());
            }
            InputKey::Left | InputKey::Char('-') => self.adjust_editor_setting(selected, -1),
            InputKey::Right | InputKey::Char('+' | '=') => self.adjust_editor_setting(selected, 1),
            InputKey::Char('t' | 'T') if !repeated => {
                self.activate_editor_settings_control(ui::EditorSettingsControl::ToggleEnabled)
            }
            InputKey::Char('r' | 'R') if !repeated => {
                self.activate_editor_settings_control(ui::EditorSettingsControl::RestoreDefaults)
            }
            InputKey::Enter | InputKey::Char(' ') if !repeated => {
                self.activate_editor_setting(selected)
            }
            _ => {}
        }
    }

    pub(in crate::session) fn open_editor_settings(&mut self) {
        self.editor_open_menu = None;
        self.editor_selected_toolbar_action = None;
        self.editor_quick_menu_anchor = None;
        self.editor_cursor_acceleration = None;
        self.editor_settings_dialog = Some(EditorSettingsDialogState {
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
        if let Some(dialog) = self.editor_settings_dialog.as_mut() {
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
                if let Some(dialog) = self.editor_settings_dialog.as_mut() {
                    dialog.draft.cursor_acceleration_enabled =
                        !dialog.draft.cursor_acceleration_enabled;
                }
            }
            Field::ActivationDelay
            | Field::RampDuration
            | Field::HorizontalMaxStep
            | Field::VerticalMaxStep => self.adjust_editor_setting(field, 1),
            Field::RestoreDefaults => {
                if let Some(dialog) = self.editor_settings_dialog.as_mut() {
                    let explorer_open_extensions = dialog.draft.explorer_open_extensions.clone();
                    dialog.draft = storage::EditorConfig {
                        explorer_open_extensions,
                        ..storage::EditorConfig::default()
                    };
                }
            }
            Field::Save => self.save_editor_settings(),
            Field::Cancel => self.editor_settings_dialog = None,
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
        let Some(dialog) = self.editor_settings_dialog.as_mut() else {
            return;
        };
        let increase = direction >= 0;
        match field {
            ui::EditorSettingsField::ActivationDelay => {
                dialog.draft.cursor_acceleration_delay_ms = adjust_u32_setting(
                    dialog.draft.cursor_acceleration_delay_ms,
                    EDITOR_CURSOR_TIME_STEP_MS,
                    increase,
                );
            }
            ui::EditorSettingsField::RampDuration => {
                dialog.draft.cursor_acceleration_ramp_ms = adjust_u32_setting(
                    dialog.draft.cursor_acceleration_ramp_ms,
                    EDITOR_CURSOR_TIME_STEP_MS,
                    increase,
                );
            }
            ui::EditorSettingsField::HorizontalMaxStep => {
                dialog.draft.cursor_horizontal_max_step =
                    adjust_u8_setting(dialog.draft.cursor_horizontal_max_step, increase);
            }
            ui::EditorSettingsField::VerticalMaxStep => {
                dialog.draft.cursor_vertical_max_step =
                    adjust_u8_setting(dialog.draft.cursor_vertical_max_step, increase);
            }
            ui::EditorSettingsField::Enabled
            | ui::EditorSettingsField::RestoreDefaults
            | ui::EditorSettingsField::Save
            | ui::EditorSettingsField::Cancel => return,
        }
        dialog.draft = normalized_editor_config(dialog.draft.clone());
    }

    pub(in crate::session) fn save_editor_settings(&mut self) {
        if !self.can_change_editor_settings() {
            self.reject_editor_settings_change();
            return;
        }
        let Some(dialog) = self.editor_settings_dialog.clone() else {
            return;
        };
        let editor = normalized_editor_config(dialog.draft);
        let config = if let Some(storage) = self.storage_manager.as_ref() {
            let mut config = match storage.load_config() {
                Ok(config) => config,
                Err(error) => {
                    self.editor_message = Some(i18n::LocalizedText::from(i18n::msg!(
                        "shell-could-not-save-editor-settings-error",
                        error = error.to_string()
                    )));
                    return;
                }
            };
            config.editor = editor;
            if let Err(error) = storage.save_config(&config) {
                self.editor_message = Some(i18n::LocalizedText::from(i18n::msg!(
                    "shell-could-not-save-editor-settings-error",
                    error = error.to_string()
                )));
                return;
            }
            self.editor_message = Some(i18n::LocalizedText::from(i18n::msg!(
                "shell-editor-settings-saved"
            )));
            config
        } else {
            let mut config = self.app.storage_config().clone();
            config.editor = editor;
            self.editor_message = Some(i18n::LocalizedText::from(i18n::msg!(
                "shell-editor-settings-applied-for-this-session"
            )));
            config
        };
        self.replace_storage_config(config);
        self.editor_cursor_acceleration = None;
        self.editor_settings_dialog = None;
    }
}
