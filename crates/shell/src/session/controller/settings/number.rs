use super::*;

impl ShellSession {
    pub(super) fn open_settings_number(&mut self, field: ui::SettingsField) -> bool {
        use ui::SettingsField as Field;
        let config = self.app.storage_config();
        let editor = self.current_editor_config();
        let phase_index = animation::PHASE_FIELDS
            .iter()
            .position(|candidate| *candidate == field);
        let (value, minimum, maximum, step) = match field {
            Field::AnimationSpeed => (
                u32::from(
                    self.app
                        .active_appearance()
                        .unwrap()
                        .normalized_animation_speed_percent(),
                ),
                u32::from(storage::MIN_ANIMATION_SPEED_PERCENT),
                u32::from(storage::MAX_ANIMATION_SPEED_PERCENT),
                u32::from(storage::ANIMATION_SPEED_STEP_PERCENT),
            ),
            Field::SystemLowAvailable | Field::SystemCriticalAvailable => (
                u32::from(if field == Field::SystemLowAvailable {
                    config.system_status.low_available_gib
                } else {
                    config.system_status.critical_available_gib
                }),
                u32::from(storage::SYSTEM_STATUS_MIN_AVAILABLE_GIB),
                u32::from(storage::SYSTEM_STATUS_MAX_AVAILABLE_GIB),
                1,
            ),
            Field::SystemLowPercentage | Field::SystemCriticalPercentage => (
                u32::from(if field == Field::SystemLowPercentage {
                    config.system_status.low_percentage
                } else {
                    config.system_status.critical_percentage
                }),
                u32::from(storage::SYSTEM_STATUS_MIN_PERCENTAGE),
                u32::from(storage::SYSTEM_STATUS_MAX_PERCENTAGE),
                1,
            ),
            Field::CursorDelay | Field::CursorRamp => (
                if field == Field::CursorDelay {
                    editor.cursor_acceleration_delay_ms
                } else {
                    editor.cursor_acceleration_ramp_ms
                },
                EDITOR_CURSOR_MIN_TIME_MS,
                EDITOR_CURSOR_MAX_TIME_MS,
                EDITOR_CURSOR_TIME_STEP_MS,
            ),
            Field::CursorHorizontalStep => (
                u32::from(editor.cursor_horizontal_max_step),
                u32::from(EDITOR_CURSOR_MIN_HORIZONTAL_STEP),
                u32::from(EDITOR_CURSOR_MAX_HORIZONTAL_STEP),
                1,
            ),
            Field::CursorVerticalStep => (
                u32::from(editor.cursor_vertical_max_step),
                u32::from(EDITOR_CURSOR_MIN_VERTICAL_STEP),
                u32::from(
                    EDITOR_CURSOR_MAX_VERTICAL_STEP.min(editor.cursor_horizontal_max_step - 1),
                ),
                1,
            ),
            _ => {
                let Some(index) = phase_index else {
                    return false;
                };
                let timings = self
                    .app
                    .active_appearance()
                    .unwrap()
                    .animation_timings
                    .normalized();
                let profiles = [timings.page, timings.dialog, timings.menu];
                (
                    u32::from(profiles[index / 3][index % 3]),
                    u32::from(storage::MIN_ANIMATION_PHASE_MS),
                    u32::from(storage::MAX_ANIMATION_PHASE_MS),
                    1,
                )
            }
        };
        if field == Field::AnimationSpeed || phase_index.is_some() {
            if self
                .app
                .active_appearance()
                .unwrap()
                .motion_preference
                .reduced()
            {
                self.set_settings_error(i18n::msg!("settings-full-motion-required"));
                return true;
            }
        } else if !self.can_change_global_settings() {
            self.set_settings_error(i18n::msg!("settings-admin-required"));
            return true;
        }
        let title = self
            .to_settings_view_model()
            .and_then(|model| {
                model
                    .cards
                    .into_iter()
                    .flat_map(|card| card.items)
                    .chain(
                        model
                            .animation_editor
                            .into_iter()
                            .flat_map(|editor| editor.items),
                    )
                    .find(|item| item.field == field)
                    .map(|item| item.label)
            })
            .unwrap();
        let mut input = ui::components::TextInput::new("settings.number.input");
        input.set_value(value.to_string());
        input.set_focused(true);
        input.set_selection_anchor(Some(0));
        if let Some(state) = self.pages.settings.settings_state.as_mut() {
            state.number_editor = Some(SettingsNumberEditorState {
                field,
                title,
                input,
                minimum,
                maximum,
                step,
                error: None,
                selected_index: 0,
            });
        }
        true
    }

    pub(super) fn handle_settings_number_key(&mut self, key: &KeyInput) {
        if key.has_non_shift_modifier() && !matches!(key.key, InputKey::Char('a' | 'A')) {
            return;
        }
        let editor = self
            .pages
            .settings
            .settings_state
            .as_mut()
            .unwrap()
            .number_editor
            .as_mut()
            .unwrap();
        match key.key {
            InputKey::Escape => self.close_settings_number(),
            InputKey::Enter => {
                if editor.selected_index == 2 {
                    self.close_settings_number();
                } else {
                    self.apply_settings_number();
                }
            }
            InputKey::Up | InputKey::Down | InputKey::Tab | InputKey::BackTab => {
                let backwards =
                    matches!(key.key, InputKey::Up | InputKey::BackTab) || key.modifiers.shift;
                editor.selected_index = (editor.selected_index + if backwards { 2 } else { 1 }) % 3;
                editor.input.set_focused(editor.selected_index == 0);
            }
            InputKey::Char(character)
                if !character.is_ascii_digit() && !key.has_non_shift_modifier() => {}
            _ => {
                editor
                    .input
                    .handle_event_borderless(InputEvent::Key(key.clone()), Rect::default());
                editor.error = None;
            }
        }
        self.refresh_hit_map();
    }

    pub(super) fn adjust_settings_number(&mut self, direction: i8) {
        let editor = self
            .pages
            .settings
            .settings_state
            .as_mut()
            .unwrap()
            .number_editor
            .as_mut()
            .unwrap();
        let value = editor
            .input
            .value()
            .parse::<u32>()
            .unwrap_or(editor.minimum);
        let value = if direction < 0 {
            value.saturating_sub(editor.step)
        } else {
            value.saturating_add(editor.step)
        };
        editor
            .input
            .set_value(value.clamp(editor.minimum, editor.maximum).to_string());
        editor.selected_index = 0;
        editor.input.set_focused(true);
        editor.error = None;
    }

    pub(super) fn close_settings_number(&mut self) {
        if let Some(state) = self.pages.settings.settings_state.as_mut() {
            state.number_editor = None;
        }
    }

    pub(super) fn apply_settings_number(&mut self) {
        use ui::SettingsField as Field;
        let editor = self
            .pages
            .settings
            .settings_state
            .as_ref()
            .unwrap()
            .number_editor
            .as_ref()
            .unwrap()
            .clone();
        let value = match editor.input.value().parse::<u32>() {
            Ok(value) if (editor.minimum..=editor.maximum).contains(&value) => value,
            _ => {
                self.pages
                    .settings
                    .settings_state
                    .as_mut()
                    .unwrap()
                    .number_editor
                    .as_mut()
                    .unwrap()
                    .error = Some(
                    i18n::msg!(
                        "settings-number-range",
                        min = editor.minimum,
                        max = editor.maximum
                    )
                    .into(),
                );
                return;
            }
        };
        let phase_index = animation::PHASE_FIELDS
            .iter()
            .position(|field| *field == editor.field);
        let saved = if editor.field == Field::AnimationSpeed || phase_index.is_some() {
            let mut appearance = self.app.active_appearance().unwrap().clone();
            if let Some(index) = phase_index {
                let phases = match index / 3 {
                    0 => &mut appearance.animation_timings.page,
                    1 => &mut appearance.animation_timings.dialog,
                    _ => &mut appearance.animation_timings.menu,
                };
                phases[index % 3] = value as u16;
            } else {
                appearance.animation_speed_percent = value as u16;
            }
            self.save_settings_appearance(appearance, settings_saved_field(editor.field))
        } else {
            let Some(storage) = self.storage_manager.clone() else {
                return;
            };
            let mut config = match storage.load_config() {
                Ok(config) => config,
                Err(error) => {
                    self.set_settings_error(i18n::msg!(
                        "settings-load-failed",
                        reason = error.to_string()
                    ));
                    return;
                }
            };
            match editor.field {
                Field::SystemLowAvailable => config.system_status.low_available_gib = value as u16,
                Field::SystemLowPercentage => config.system_status.low_percentage = value as u8,
                Field::SystemCriticalAvailable => {
                    config.system_status.critical_available_gib = value as u16
                }
                Field::SystemCriticalPercentage => {
                    config.system_status.critical_percentage = value as u8
                }
                Field::CursorDelay => config.editor.cursor_acceleration_delay_ms = value,
                Field::CursorRamp => config.editor.cursor_acceleration_ramp_ms = value,
                Field::CursorHorizontalStep => {
                    config.editor.cursor_horizontal_max_step = value as u8
                }
                Field::CursorVerticalStep => config.editor.cursor_vertical_max_step = value as u8,
                _ => unreachable!(),
            }
            self.save_settings_global_config(config, editor.field)
        };
        if saved {
            self.close_settings_number();
        }
    }
}
