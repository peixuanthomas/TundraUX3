use super::*;
use ui::components::{FormAction, FormEvent, TextEditState};

impl ShellSession {
    pub(super) fn handle_management_form_key(&mut self, key: &KeyInput) -> bool {
        let Some(source) = self.pages.management.management_state.form.as_mut() else {
            return false;
        };
        if management_control_modifier(key) && matches!(key.key, InputKey::Char('u' | 'U')) {
            if let Some(field) = source.fields.get_mut(source.selected) {
                if field.choices.is_empty() {
                    use zeroize::Zeroize;
                    field.value.zeroize();
                    source.input_states.remove(&source.selected);
                }
            }
            return true;
        }
        self.handle_management_form_event(InputEvent::Key(key.clone()))
    }

    pub(super) fn handle_management_form_event(&mut self, event: InputEvent) -> bool {
        let Some(mut dialog) = self.to_management_view_model().form else {
            return false;
        };
        let source = self
            .pages
            .management
            .management_state
            .form
            .as_mut()
            .unwrap();
        for (index, field) in dialog.fields.iter_mut().enumerate() {
            field.value = source.fields[index].value.clone();
            if field.is_text() {
                field.input = Some(
                    source
                        .input_states
                        .get(&index)
                        .cloned()
                        .unwrap_or_default()
                        .input(field.id.as_str(), &field.value),
                );
            }
        }
        let event = dialog.handle_event(event, self.management_main());
        let state = &mut self.pages.management.management_state;
        state.form_field_scroll = dialog.field_scroll;
        let source = state.form.as_mut().unwrap();
        source.selected = dialog.selected;
        source.message_scroll = dialog.message_scroll;
        for (index, field) in dialog.fields.iter().enumerate() {
            if source.fields[index].control == FieldControl::Permissions {
                let changed = source.fields[index]
                    .value
                    .bytes()
                    .zip(field.value.bytes())
                    .enumerate()
                    .filter_map(|(cell, (before, after))| (before != after).then_some(cell))
                    .collect::<Vec<_>>();
                for cell in changed {
                    super::input::toggle_management_check(source, index, cell);
                }
            } else {
                source.fields[index].value = field.value.clone();
            }
            if let Some(input) = &field.input {
                source
                    .input_states
                    .insert(index, TextEditState::from_input(input));
            }
        }
        if let Some(field) = dialog.fields.get(dialog.selected) {
            if let Some(cell) = field.selected_cell {
                state.permission_cell = cell;
            }
        }
        if let Some(choice) = dialog.choice {
            self.open_management_choice_field(choice.field);
        }
        match event {
            FormEvent::Action(FormAction::Submit) => self.submit_management_form(),
            FormEvent::Action(FormAction::Cancel) => self.cancel_management_form(),
            _ => {}
        }
        true
    }
}
