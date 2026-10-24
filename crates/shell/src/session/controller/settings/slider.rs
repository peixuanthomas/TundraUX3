use super::*;

impl ShellSession {
    pub(in crate::session) fn handle_settings_slider_pointer(&mut self, mouse: MouseInput) -> bool {
        let Some(layout) = self.current_settings_layout() else {
            return false;
        };
        let point = mouse.coordinates();
        match mouse.kind {
            ui::MouseEventKind::Down(PointerButton::Left) => {
                self.interaction.settings_slider_drag = layout
                    .sliders
                    .iter()
                    .find(|slider| slider.option_at(point).is_some())
                    .map(|slider| (slider.field, slider.area));
            }
            ui::MouseEventKind::Drag(PointerButton::Left)
            | ui::MouseEventKind::Up(PointerButton::Left) => {
                let Some((field, area)) = self.interaction.settings_slider_drag else {
                    return false;
                };
                let option = layout
                    .sliders
                    .iter()
                    .find(|slider| slider.field == field && slider.area == area)
                    .and_then(|slider| slider.option_at(point));
                if let Some(index) = option {
                    self.select_settings_slider_option(field, index);
                }
                if option.is_none() || matches!(mouse.kind, ui::MouseEventKind::Up(_)) {
                    self.interaction.settings_slider_drag = None;
                }
                self.refresh_hit_map();
                return true;
            }
            ui::MouseEventKind::Moved => {
                if self
                    .interaction
                    .settings_slider_drag
                    .is_some_and(|(_, area)| !rect_contains(area, point))
                {
                    self.interaction.settings_slider_drag = None;
                }
            }
            _ => self.interaction.settings_slider_drag = None,
        }
        false
    }

    fn select_settings_slider_option(&mut self, field: ui::SettingsField, index: usize) {
        let Some(model) = self.to_settings_view_model() else {
            return;
        };
        let Some(item) = model
            .cards
            .iter()
            .flat_map(|card| &card.items)
            .find(|item| item.field == field && item.enabled)
        else {
            return;
        };
        let ui::SettingsControlKind::Slider { selected } = item.kind else {
            return;
        };
        if let Some(state) = self.pages.settings.settings_state.as_mut() {
            state.selected_field = field;
        }
        if index != selected {
            self.save_global_setting(field, index as i8 - selected as i8);
        }
    }
}
