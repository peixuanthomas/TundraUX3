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

pub(super) fn visible_settings_fields(state: &SettingsState) -> Vec<ui::SettingsField> {
    settings_fields(state.category)
        .iter()
        .copied()
        .filter(|field| state.animation_advanced_open || !PHASE_FIELDS.contains(field))
        .collect()
}

pub(super) fn items(
    state: &SettingsState,
    appearance: &storage::AppearanceConfig,
) -> Vec<ui::SettingsItemViewModel> {
    use ui::{SettingsControlKind as Kind, SettingsField as Field, SettingsItemViewModel as Item};
    let enabled = !appearance.motion_preference.reduced();
    let mut items = vec![
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
            if state.animation_advanced_open {
                i18n::tr!("settings-animation-collapse")
            } else {
                i18n::tr!("settings-animation-expand")
            },
            i18n::tr!("settings-animation-advanced-help"),
            Kind::Action,
        ),
    ];
    if state.animation_advanced_open {
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
    }
    items.push(Item::new(
        Field::ResetAnimationSpeed,
        i18n::tr!("settings-restore-speed"),
        i18n::tr!("settings-reset"),
        i18n::tr!("settings-restore-speed-help"),
        Kind::Action,
    ));
    items
}

impl ShellSession {
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
