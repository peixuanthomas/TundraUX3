use super::*;

impl ShellSession {
    pub(super) fn select_settings_application(&mut self, application: ui::SettingsApplication) {
        if let Some(state) = self.pages.settings.settings_state.as_mut() {
            state.application = application;
            state.selected_field = ui::SettingsField::ApplicationList;
            state.scroll_offset = 0;
            state.status = i18n::msg!(
                "settings-category-status",
                category = format!("{application:?}")
            )
            .into();
        }
    }

    pub(super) fn select_settings_application_delta(&mut self, delta: isize) {
        let Some(state) = self.pages.settings.settings_state.as_ref() else {
            return;
        };
        let applications = ui::SettingsApplication::ALL;
        let index = applications
            .iter()
            .position(|application| *application == state.application)
            .unwrap_or(0);
        let next = (index as isize + delta).rem_euclid(applications.len() as isize) as usize;
        self.select_settings_application(applications[next]);
    }
}
