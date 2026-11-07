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

pub(super) fn editor(appearance: &storage::AppearanceConfig) -> SettingsAnimationEditorState {
    use ui::components::{FormAction, FormButton, FormControl, FormDialog, FormField};
    let timings = appearance.animation_timings.normalized();
    let values = [timings.page, timings.dialog, timings.menu];
    let fields = phase_items(appearance)
        .into_iter()
        .take(9)
        .enumerate()
        .map(|(index, item)| {
            let mut field = FormField::text(
                format!("phase-{index}"),
                item.label,
                values[index / 3][index % 3].to_string(),
            );
            field.control = FormControl::Number {
                minimum: storage::MIN_ANIMATION_PHASE_MS.into(),
                maximum: storage::MAX_ANIMATION_PHASE_MS.into(),
                step: 1,
            };
            field.disabled = !item.enabled;
            field
        })
        .collect();
    let mut form = FormDialog::new(
        "settings.animation",
        i18n::tr!("settings-animation-advanced"),
        fields,
    );
    form.columns = 3;
    form.submit_label = Some(i18n::tr!("settings-save"));
    form.cancel_label = Some(i18n::tr!("settings-cancel"));
    form.buttons = form.actions();
    form.buttons.insert(
        0,
        FormButton::new(
            "reset",
            i18n::tr!("settings-restore-speed"),
            FormAction::Custom("reset".into()),
        ),
    );
    SettingsAnimationEditorState {
        form,
        reset_speed: false,
    }
}

impl ShellSession {
    pub(super) fn open_settings_animation(&mut self) {
        let editor = editor(self.app.active_appearance().unwrap());
        if let Some(state) = self.pages.settings.settings_state.as_mut() {
            state.animation_editor = Some(editor);
        }
    }

    pub(super) fn reset_settings_animation_draft(&mut self) {
        let mut appearance = self.app.active_appearance().unwrap().clone();
        appearance.animation_timings = Default::default();
        let mut editor = editor(&appearance);
        editor.reset_speed = true;
        self.pages
            .settings
            .settings_state
            .as_mut()
            .unwrap()
            .animation_editor = Some(editor);
    }

    pub(super) fn apply_settings_animation(&mut self) {
        let mut appearance = self.app.active_appearance().unwrap().clone();
        let editor = self
            .pages
            .settings
            .settings_state
            .as_mut()
            .unwrap()
            .animation_editor
            .as_mut()
            .unwrap();
        let mut values = [0u16; 9];
        for (index, field) in editor.form.fields.iter().enumerate() {
            match field.value.parse::<u16>() {
                Ok(value)
                    if (storage::MIN_ANIMATION_PHASE_MS..=storage::MAX_ANIMATION_PHASE_MS)
                        .contains(&value) =>
                {
                    values[index] = value
                }
                _ => {
                    editor.form.selected = index;
                    editor.form.message = i18n::tr!(
                        "settings-number-range",
                        min = storage::MIN_ANIMATION_PHASE_MS,
                        max = storage::MAX_ANIMATION_PHASE_MS
                    );
                    return;
                }
            }
        }
        appearance
            .animation_timings
            .page
            .copy_from_slice(&values[0..3]);
        appearance
            .animation_timings
            .dialog
            .copy_from_slice(&values[3..6]);
        appearance
            .animation_timings
            .menu
            .copy_from_slice(&values[6..9]);
        if editor.reset_speed {
            appearance.animation_speed_percent = storage::DEFAULT_ANIMATION_SPEED_PERCENT;
        }
        if self.save_settings_appearance(
            appearance,
            settings_saved_field(ui::SettingsField::AnimationAdvanced),
        ) {
            self.pages
                .settings
                .settings_state
                .as_mut()
                .unwrap()
                .animation_editor = None;
        }
    }
}
