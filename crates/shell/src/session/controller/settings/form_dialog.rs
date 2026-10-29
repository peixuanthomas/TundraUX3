use super::*;
use ui::components::{FormAction, FormControl, FormEvent, TextEditState};

impl ShellSession {
    pub(in crate::session) fn handle_settings_form_event(
        &mut self,
        event: InputEvent,
        confirmed_pointer: bool,
    ) -> bool {
        let Some(mut form) = self
            .to_settings_view_model()
            .and_then(|model| model.form_dialog())
        else {
            return false;
        };
        let state = self.pages.settings.settings_state.as_ref().unwrap();
        let busy = state
            .time_sync_server_editor
            .as_ref()
            .is_some_and(|editor| editor.validating);
        if let Some(editor) = &state.number_editor {
            form.fields[0].control = FormControl::Number {
                minimum: editor.minimum,
                maximum: editor.maximum,
                step: editor.step,
            };
        }
        let result = match event {
            InputEvent::Mouse(mouse) if confirmed_pointer => {
                form.handle_confirmed_pointer(mouse, self.touch_main())
            }
            event => form.handle_event(event, self.touch_main()),
        };
        let state = self.pages.settings.settings_state.as_mut().unwrap();
        state.form_selected = form.selected;
        state.form_interaction = form.interaction_state();
        let field = form.fields.remove(0);
        if let Some(editor) = &mut state.number_editor {
            editor.selected_index = form.selected;
            if editor.input.value() != field.value {
                editor.error = None;
            }
            editor.input = field.input.unwrap_or_else(|| {
                let mut input = ui::components::TextInput::new("settings.number.input");
                input.set_value(field.value);
                input
            });
            editor.input.set_focused(form.selected == 0);
        } else if !busy {
            let (value, editing, error, invalid) = if let Some(editor) = &mut state.color_editor {
                let invalid = (field.value.len() > 7
                    || !field
                        .value
                        .chars()
                        .all(|c| c == '#' || c.is_ascii_hexdigit()))
                .then(|| i18n::msg!("settings-invalid-custom-color").into());
                (
                    &mut editor.value,
                    &mut editor.editing,
                    &mut editor.error,
                    invalid,
                )
            } else if let Some(editor) = &mut state.weather_location_editor {
                let invalid = if !field.value.chars().all(is_weather_location_character) {
                    Some(i18n::msg!("settings-weather-characters").into())
                } else if field.value.len() > WEATHER_LOCATION_MAX_LEN {
                    Some(
                        i18n::msg!("settings-weather-limit", limit = WEATHER_LOCATION_MAX_LEN)
                            .into(),
                    )
                } else {
                    None
                };
                (
                    &mut editor.value,
                    &mut editor.editing,
                    &mut editor.error,
                    invalid,
                )
            } else if let Some(editor) = &mut state.file_extensions_editor {
                let invalid = if !field.value.chars().all(is_editor_extension_input_character) {
                    Some(i18n::msg!("settings-suffix-characters").into())
                } else if field.value.len() > EDITOR_EXTENSIONS_INPUT_MAX_LEN {
                    Some(
                        i18n::msg!(
                            "settings-suffix-limit",
                            limit = EDITOR_EXTENSIONS_INPUT_MAX_LEN
                        )
                        .into(),
                    )
                } else {
                    None
                };
                (
                    &mut editor.value,
                    &mut editor.editing,
                    &mut editor.error,
                    invalid,
                )
            } else {
                let editor = state.time_sync_server_editor.as_mut().unwrap();
                let invalid = (field.value.len() > time::MAX_TIME_SERVER_URL_LEN).then(|| {
                    i18n::msg!(
                        "settings-server-limit",
                        limit = time::MAX_TIME_SERVER_URL_LEN
                    )
                    .into()
                });
                (
                    &mut editor.value,
                    &mut editor.editing,
                    &mut editor.error,
                    invalid,
                )
            };
            let accepted = invalid.is_none();
            if *value != field.value {
                *error = invalid;
                if error.is_none() {
                    *value = field.value;
                }
            }
            if accepted && let Some(input) = field.input {
                *editing = TextEditState::from_input(&input);
            }
        }
        match result {
            FormEvent::Action(FormAction::Submit) => {
                if state.number_editor.is_some() {
                    self.apply_settings_number();
                } else if state.color_editor.is_some() {
                    self.apply_settings_custom_color();
                } else if state.weather_location_editor.is_some() {
                    self.request_settings_weather_location_confirmation();
                } else if state.file_extensions_editor.is_some() {
                    self.save_settings_file_extensions();
                } else {
                    self.validate_settings_time_sync_server();
                }
            }
            FormEvent::Action(FormAction::Cancel) => {
                if state.time_sync_server_editor.is_some() {
                    state.system_time.server_field = None;
                }
                state.number_editor = None;
                state.color_editor = None;
                state.weather_location_editor = None;
                state.file_extensions_editor = None;
                state.time_sync_server_editor = None;
                state.time_sync_validation_request_id = None;
                state.status = i18n::msg!("settings-ready").into();
            }
            _ => {}
        }
        true
    }
}
