use super::*;

pub(super) const PHASE_FIELDS: [ui::SettingsField; 9] = [
    ui::SettingsField::PageAcceleration,
    ui::SettingsField::PageCruise,
    ui::SettingsField::PageDeceleration,
    ui::SettingsField::DialogAcceleration,
    ui::SettingsField::DialogCruise,
    ui::SettingsField::DialogDeceleration,
    ui::SettingsField::MenuAcceleration,
    ui::SettingsField::MenuCruise,
    ui::SettingsField::MenuDeceleration,
];

pub(super) fn items(appearance: &storage::AppearanceConfig) -> Vec<ui::SettingsItemViewModel> {
    use ui::{SettingsControlKind as Kind, SettingsField as Field, SettingsItemViewModel as Item};
    let enabled = !appearance.motion_preference.reduced();
    vec![
        Item::new(
            Field::MotionPreference,
            i18n::tr!("settings-motion"),
            if enabled {
                i18n::tr!("settings-full")
            } else {
                i18n::tr!("settings-reduced")
            },
            i18n::tr!("settings-motion-help"),
            Kind::Cycle,
        ),
        Item::new(
            Field::AnimationSpeed,
            i18n::tr!("settings-animation-speed"),
            i18n::tr!(
                "settings-value-percent",
                value = appearance.normalized_animation_speed_percent()
            ),
            i18n::tr!("settings-animation-speed-help"),
            Kind::Stepper,
        )
        .enabled(enabled),
        Item::new(
            Field::AnimationAdvanced,
            i18n::tr!("settings-animation-advanced"),
            i18n::tr!("settings-animation-open"),
            i18n::tr!("settings-animation-advanced-help"),
            Kind::Action,
        ),
        reset_item(),
    ]
}

pub(super) fn phase_items(
    appearance: &storage::AppearanceConfig,
) -> Vec<ui::SettingsItemViewModel> {
    use ui::{SettingsControlKind as Kind, SettingsItemViewModel as Item};
    let enabled = !appearance.motion_preference.reduced();
    let mut items = Vec::new();
    let timings = appearance.animation_timings.normalized();
    let profiles = [timings.page, timings.dialog, timings.menu];
    let kinds = [
        "settings-animation-page",
        "settings-animation-dialog",
        "settings-animation-menu",
    ];
    let phases = [
        "settings-animation-acceleration",
        "settings-animation-cruise",
        "settings-animation-deceleration",
    ];
    for (index, field) in PHASE_FIELDS.into_iter().enumerate() {
        items.push(
            Item::new(
                field,
                i18n::tr!(
                    "settings-animation-phase-label",
                    kind = i18n::tr!(kinds[index / 3]),
                    phase = i18n::tr!(phases[index % 3])
                ),
                i18n::tr!(
                    "settings-animation-ms",
                    value = profiles[index / 3][index % 3]
                ),
                i18n::tr!("settings-animation-phase-help"),
                Kind::Stepper,
            )
            .enabled(enabled),
        );
    }
    items.push(reset_item());
    items
}

fn reset_item() -> ui::SettingsItemViewModel {
    ui::SettingsItemViewModel::new(
        ui::SettingsField::ResetAnimationSpeed,
        i18n::tr!("settings-restore-speed"),
        i18n::tr!("settings-reset"),
        i18n::tr!("settings-restore-speed-help"),
        ui::SettingsControlKind::Action,
    )
}

impl ShellSession {
    pub(super) fn handle_settings_animation_key(&mut self, key: &KeyInput) {
        if key.has_non_shift_modifier() {
            return;
        }
        let Some(editor) = self
            .pages
            .settings
            .settings_state
            .as_mut()
            .and_then(|state| state.animation_editor.as_mut())
        else {
            return;
        };
        let close = PHASE_FIELDS.len() + 1;
        let mut reveal = false;
        match key.key {
            InputKey::Escape => self.close_settings_animation(),
            InputKey::Up | InputKey::BackTab => {
                editor.selected_index = editor.selected_index.saturating_sub(1);
                reveal = true;
            }
            InputKey::Tab if key.modifiers.shift => {
                editor.selected_index = editor.selected_index.saturating_sub(1);
                reveal = true;
            }
            InputKey::Down | InputKey::Tab => {
                editor.selected_index = (editor.selected_index + 1).min(close);
                reveal = true;
            }
            InputKey::Home => {
                editor.selected_index = 0;
                reveal = true;
            }
            InputKey::End => {
                editor.selected_index = close;
                reveal = true;
            }
            InputKey::PageUp => self.scroll_settings_animation(-6),
            InputKey::PageDown => self.scroll_settings_animation(6),
            InputKey::Left | InputKey::Right => {
                let index = editor.selected_index;
                if let Some(field) = PHASE_FIELDS.get(index) {
                    self.adjust_animation_phase(
                        *field,
                        if key.key == InputKey::Left { -1 } else { 1 },
                    );
                }
            }
            InputKey::Enter | InputKey::Char(' ') => match editor.selected_index {
                index if index == close => self.close_settings_animation(),
                index if index == PHASE_FIELDS.len() => self.reset_settings_animation_speed(),
                _ => {}
            },
            _ => {}
        }
        if reveal {
            self.reveal_settings_animation_selection();
        }
        self.refresh_hit_map();
    }

    pub(super) fn close_settings_animation(&mut self) {
        if let Some(state) = self.pages.settings.settings_state.as_mut() {
            state.animation_editor = None;
        }
    }

    pub(super) fn select_settings_animation_field(&mut self, field: ui::SettingsField) {
        let index = PHASE_FIELDS
            .iter()
            .position(|candidate| *candidate == field)
            .unwrap_or(PHASE_FIELDS.len());
        if let Some(editor) = self
            .pages
            .settings
            .settings_state
            .as_mut()
            .and_then(|state| state.animation_editor.as_mut())
        {
            editor.selected_index = index;
        }
    }

    fn reveal_settings_animation_selection(&mut self) {
        let Some(layout) = self
            .current_settings_layout()
            .and_then(|layout| layout.animation_editor)
        else {
            return;
        };
        if let Some(editor) = self
            .pages
            .settings
            .settings_state
            .as_mut()
            .and_then(|state| state.animation_editor.as_mut())
        {
            let row = editor.selected_index.min(PHASE_FIELDS.len()) as u16;
            editor.scroll_offset = if row < layout.scroll_offset {
                row
            } else if row >= layout.scroll_offset.saturating_add(layout.viewport.height) {
                row.saturating_add(1).saturating_sub(layout.viewport.height)
            } else {
                layout.scroll_offset
            }
            .min(layout.max_scroll_offset);
        }
    }

    pub(super) fn scroll_settings_animation(&mut self, delta: i16) {
        let Some(layout) = self
            .current_settings_layout()
            .and_then(|layout| layout.animation_editor)
        else {
            return;
        };
        if let Some(editor) = self
            .pages
            .settings
            .settings_state
            .as_mut()
            .and_then(|state| state.animation_editor.as_mut())
        {
            editor.scroll_offset =
                settings_scroll_offset(layout.scroll_offset, delta, layout.max_scroll_offset);
        }
    }

    pub(super) fn adjust_animation_phase(
        &mut self,
        field: ui::SettingsField,
        direction: i8,
    ) -> bool {
        let Some(index) = PHASE_FIELDS
            .iter()
            .position(|candidate| *candidate == field)
        else {
            return false;
        };
        let Some(mut appearance) = self.app.active_appearance().cloned() else {
            return true;
        };
        if appearance.motion_preference.reduced() {
            self.set_settings_error(i18n::msg!("settings-full-motion-required"));
            return true;
        }
        appearance.animation_timings = appearance.animation_timings.normalized();
        let phases = match index / 3 {
            0 => &mut appearance.animation_timings.page,
            1 => &mut appearance.animation_timings.dialog,
            _ => &mut appearance.animation_timings.menu,
        };
        let value = &mut phases[index % 3];
        *value = if direction >= 0 {
            value.saturating_add(1).min(storage::MAX_ANIMATION_PHASE_MS)
        } else {
            value.saturating_sub(1).max(storage::MIN_ANIMATION_PHASE_MS)
        };
        self.save_settings_appearance(appearance, settings_saved_field(field));
        true
    }
}
